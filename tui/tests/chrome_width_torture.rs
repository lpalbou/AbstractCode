//! Width torture for the presence/density chrome (cycle-2 review):
//! header + footer + strip rendered at 60 / 80 / 100 / 120 / 200 / 271
//! cols with a fully-loaded store, pinning the degrade rules:
//!
//! - account/status and Tools remain reachable at every tested width;
//!   narrow headers collect secondary controls under Menu,
//! - no `…` self-truncation in header facts or footer instruments
//!   (whole-item drop, right-to-left),
//! - entity chips are never sacrificed to facts (paint order),
//! - capability choices have one grouped entry instead of duplicate counts,
//! - the idle strip summary ellipsizes instead of hard-cutting at the
//!   screen edge (P1-B).
//!
//! Own harness: the shared `tests/headless_ui.rs` one is file-private
//! and fixed at 100×30; this file parameterizes the size.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;

use abstractcode::config::Prefs;
use abstractcode::runner::Cmd;
use abstractcode::store::{Conn, GpuMeter, GpuSample, McpServer, SessionTotals, Store, Workflow};
use abstractcode::ui::{self, UiCtx};

struct SizedHarness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    ctx: UiCtx,
    /// Kept alive: dropping the receiver would make sends fail.
    _rx: mpsc::Receiver<Cmd>,
}

fn harness_sized(w: i32, h: i32) -> SizedHarness {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let size = Size::new(w, h);
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, rx) = mpsc::channel::<Cmd>();
    let store_slot: Rc<RefCell<Option<Store>>> = Rc::new(RefCell::new(None));
    let store_out = store_slot.clone();
    let context_slot = Rc::new(RefCell::new(None));
    let context_out = context_slot.clone();
    let actions = app.actions();
    app.mount(move |cx| {
        let store = Store::create(cx);
        *store_out.borrow_mut() = Some(store);
        store.session_id.set("acode-test-session".into());
        store.conn.set(Conn::Ok);
        let wf = Workflow {
            bundle_id: "basic-agent".into(),
            flow_id: "81795ea9".into(),
            name: "basic-agent".into(),
            description: String::new(),
            ..Default::default()
        };
        // A discovered catalog, so the workflow widget's picker opens.
        store.workflows.set(vec![wf.clone()]);
        store.workflow.set(wf);
        let ctx = UiCtx {
            tx,
            client: abstractcode::gateway::GatewayClient::new("http://127.0.0.1:1", None),
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            prefs: Rc::new(RefCell::new(Prefs::default())),
            workspace_root: Some("/tmp/ws".into()),
            max_iterations_explicit: false,
            max_iterations: 50,
            // Hermetic: harness runs never read the repo's AGENTS.md, so
            // editing that file can never move a UI assertion.
            no_project_context: true,
            // Harness default: absent posture = server truth, same as a launch
            // without --no-prompt-cache.
            no_prompt_cache: false,
            replay_turns: 20,
            gateway_label: "127.0.0.1:8080".into(),
            modal: Rc::new(RefCell::new(None)),
            modal_epoch: cx.signal(0u64),
            dismissed_wait: Rc::new(RefCell::new(None)),
            wait_modal_for: Rc::new(RefCell::new(None)),
        };
        *context_out.borrow_mut() = Some(ctx.clone());
        ui::root(cx, store, ctx, &actions)
    })
    .expect("mount");
    let mut term = CaptureTerm::new(size);
    let cfg = RunConfig {
        probe: false,
        caps: Some(abstracttui::term::Capabilities::with(|c| {
            c.truecolor = true;
            c.colors_256 = true;
            c.unicode_ok = true;
        })),
        ..RunConfig::default()
    };
    let driver = Driver::new(&mut app, &mut term, cfg).expect("driver");
    let store = store_slot.borrow().expect("store created");
    let ctx = context_slot.borrow().clone().expect("context created");
    SizedHarness {
        app,
        term,
        driver,
        store,
        ctx,
        _rx: rx,
    }
}

impl SizedHarness {
    /// Escape out of whatever a widget opened (modal, dropdown, rail).
    fn close_all(&mut self) {
        for _ in 0..3 {
            self.term.push_input(&[0x1b]);
            self.turn();
            std::thread::sleep(std::time::Duration::from_millis(45));
            self.turn();
        }
        let _ = self._rx.try_iter().count();
    }

    /// Which chrome action host holds the keyboard focus, by the focused
    /// node's box: the header is the first row, the status bar the last.
    fn focused_host(&mut self) -> Option<&'static str> {
        let rows = self.term.screen().to_text().lines().count() as i32;
        let tree = self.app.tree();
        let r = tree.rect_of(tree.focused()?);
        match (r.y, r.h) {
            (0, 1) => Some("header"),
            (y, 1) if y == rows - 1 => Some("bar"),
            _ => None,
        }
    }

    fn turn(&mut self) -> String {
        self.driver
            .turn(&mut self.app, &mut self.term)
            .expect("turn");
        self.term.screen().to_text()
    }
}

/// Load every instrument at once: declared+measured ctx, split session
/// totals, GPU sample, skills, MCP, one parked entity conversation.
fn load_everything(store: &Store) {
    store.context_window.set(262_144);
    store.fold.update(|f| {
        f.begin_run("root");
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "hi",
                        "usage": {"input_tokens": 41_203, "output_tokens": 20}}
        });
        let _ = f.apply("root", &rec);
        // Idle chrome after the measurement (the run branch is not under
        // torture here; the ticker tests own it).
        f.finished = true;
        f.clear_llm_inflight();
    });
    store.totals.set(SessionTotals {
        input_tokens: 100_000,
        output_tokens: 28_000,
        total_tokens: 128_000,
        runs: 3,
    });
    store.gpu.set(GpuMeter::Ready(GpuSample {
        util_pct: 28.0,
        name: "Apple M5 Max".into(),
    }));
    store
        .selected_skills
        .set(vec!["coredoc".into(), "agora-channels".into()]);
    store.mcp_servers.set(vec![McpServer {
        name: "context7".into(),
        url: "https://mcp.example".into(),
        description: String::new(),
        auth_required: false,
    }]);
    store.convos.update(|cs| {
        let mut c = abstractcode::convo::EntityConvo::opening("castor", "awake");
        c.status = abstractcode::convo::ConvoStatus::Parked;
        cs.push(c);
    });
}

#[test]
fn chrome_degrades_whole_item_at_every_width() {
    for width in [60, 80, 100, 120, 160, 200, 271] {
        let mut h = harness_sized(width, 40);
        h.turn();
        load_everything(&h.store);
        let screen = h.turn();
        let footer = screen.lines().last().unwrap();
        assert!(
            !footer.contains("skills ") && !footer.contains("mcp "),
            "no duplicate capability counts: {footer}"
        );
        assert!(
            footer.contains("acode-test-session"),
            "session kept at {width}: {footer}"
        );
        assert!(
            !footer.contains("Dark (Abstract)"),
            "theme belongs to the top bar"
        );
        for id in [
            ui::chrome::ACT_TOOLS,
            ui::chrome::ACT_ACCOUNT,
            ui::chrome::ACT_SESSION,
        ] {
            let r = h
                .app
                .tree()
                .action_rect(id)
                .unwrap_or_else(|| panic!("{width}: {id} absent: {screen}"));
            assert!(r.x >= 0 && r.right() <= width, "{id} fits at {width}");
        }
        let right = if width >= 140 {
            vec![
                ui::chrome::ACT_DOCS,
                ui::chrome::ACT_ABOUT,
                ui::chrome::ACT_THEME,
            ]
        } else {
            vec![ui::chrome::ACT_MENU]
        };
        for id in right {
            assert!(
                h.app.tree().action_rect(id).is_some(),
                "{width}: {id}: {screen}"
            );
        }
    }
}

/// Click the centre of a chrome widget (SGR press + release), by its
/// action id — the test, like the app, never hard-codes a cell.
fn click_action(h: &mut SizedHarness, id: &str) -> String {
    let r = h
        .app
        .tree()
        .action_rect(id)
        .unwrap_or_else(|| panic!("{id} not drawn"));
    let (col, row) = (r.x + r.w / 2 + 1, r.y + 1);
    h.term
        .push_input(format!("\x1b[<0;{col};{row}M").as_bytes());
    h.turn();
    h.term
        .push_input(format!("\x1b[<0;{col};{row}m").as_bytes());
    h.turn();
    h.turn()
}

/// What each chrome widget opens (operator layout 2026-10-28): a
/// distinctive line of the modal (or its no-data notice) per action.
fn opened(id: &str) -> &'static [&'static str] {
    match id {
        ui::chrome::ACT_WORKFLOW => &["agent workflow —"],
        ui::chrome::ACT_MODEL => &["provider —", "no providers discovered"],
        ui::chrome::ACT_GATEWAY => &["status · Esc closes"],
        ui::chrome::ACT_ACCOUNT => &["Gateway:"],
        ui::chrome::ACT_TOOLS => &["choices for this session"],
        ui::chrome::ACT_DOCS => &["Documentation · AbstractCode"],
        ui::chrome::ACT_ABOUT => &["About AbstractCode"],
        ui::chrome::ACT_MENU => &["Docs assistant", "Theme", "About AbstractCode"],
        ui::chrome::ACT_WORKSPACE => &["Workspace"],
        ui::chrome::ACT_SESSION => &["Conversations"],
        ui::chrome::ACT_THEME => &["☾ Dark"],
        ui::chrome::ACT_SKILLS => &["gateway skills —"],
        ui::chrome::ACT_MCP => &["MCP servers on the gateway"],
        ui::chrome::ACT_CACHE => &["prompt cache", "prompt-cache"],
        ui::chrome::ACT_RESOURCES => &["host resources"],
        ui::chrome::ACT_HELP => &["▲ AbstractCode v"],
        _ => &[],
    }
}

#[test]
fn every_chrome_widget_opens_its_modal_on_click() {
    for width in [80, 240] {
        let mut h = harness_sized(width, 40);
        h.turn();
        let mut ids = vec![
            ui::chrome::ACT_WORKFLOW,
            ui::chrome::ACT_MODEL,
            ui::chrome::ACT_TOOLS,
            ui::chrome::ACT_ACCOUNT,
            ui::chrome::ACT_WORKSPACE,
            ui::chrome::ACT_SESSION,
        ];
        if width >= 140 {
            ids.extend([
                ui::chrome::ACT_DOCS,
                ui::chrome::ACT_ABOUT,
                ui::chrome::ACT_THEME,
            ]);
        } else {
            ids.push(ui::chrome::ACT_MENU);
        }
        for id in ids {
            h.close_all();
            let after = click_action(&mut h, id);
            assert!(
                opened(id).iter().any(|n| after.contains(n)),
                "{width}: {id}: {after}"
            );
        }
    }
}

#[test]
fn every_chrome_widget_opens_its_modal_from_the_keyboard() {
    let mut h = harness_sized(240, 40);
    h.turn();
    for (host, ids) in [
        (
            "header",
            vec![
                ui::chrome::ACT_WORKFLOW,
                ui::chrome::ACT_MODEL,
                ui::chrome::ACT_TOOLS,
                ui::chrome::ACT_DOCS,
                ui::chrome::ACT_THEME,
                ui::chrome::ACT_ABOUT,
                ui::chrome::ACT_ACCOUNT,
            ],
        ),
        (
            "bar",
            vec![
                ui::chrome::ACT_HELP,
                ui::chrome::ACT_WORKSPACE,
                ui::chrome::ACT_SESSION,
            ],
        ),
    ] {
        for id in ids {
            h.close_all();
            for _ in 0..16 {
                h.term.push_input(b"\t");
                h.turn();
                if h.focused_host() == Some(host) {
                    break;
                }
            }
            assert_eq!(h.focused_host(), Some(host));
            let mut r = h.app.tree().action_rect(id).unwrap();
            if host == "bar" && id == ui::chrome::ACT_WORKSPACE {
                let screen = h.turn();
                let lines: Vec<_> = screen.lines().collect();
                let footer = lines.last().unwrap();
                let byte = footer.find("⌂ /tmp/ws").unwrap();
                r = Rect::new(
                    abstracttui::text::width(&footer[..byte]),
                    lines.len() as i32 - 1,
                    1,
                    1,
                );
            }
            let highlighted = |h: &SizedHarness| {
                h.term
                    .screen()
                    .cell(r.x, r.y)
                    .unwrap()
                    .paint
                    .attrs
                    .contains(abstracttui::testing::Attrs::REVERSE)
            };
            for _ in 0..16 {
                if highlighted(&h) {
                    break;
                }
                h.term.push_input(b"\x1b[C");
                h.turn();
            }
            assert!(highlighted(&h), "{id} has a focus ring");
            h.term.push_input(b" ");
            h.turn();
            let after = h.turn();
            assert!(
                opened(id).iter().any(|n| after.contains(n)),
                "{id}: {after}"
            );
        }
    }
}

#[test]
fn idle_strip_summary_ellipsizes_at_narrow_widths() {
    // P1-B: the idle summary was the ONLY strip branch printed without
    // truncation — `canvas.print` clips at the SCREEN edge mid-word.
    // With a queue suffix the line overflows 60 cols and must ellipsize.
    let mut h = harness_sized(60, 30);
    h.turn();
    h.store.totals.set(SessionTotals {
        input_tokens: 12_000,
        output_tokens: 900,
        total_tokens: 12_900,
        runs: 3,
    });
    h.store.fold.update(|f| {
        f.begin_run("root");
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "hi",
                        "usage": {"input_tokens": 41_203, "output_tokens": 20}}
        });
        let _ = f.apply("root", &rec);
        f.finished = true;
        f.clear_llm_inflight();
    });
    // PAUSED queue (the restore posture): an unpaused queue on an idle
    // session auto-drains into a run — pausing keeps the idle summary up
    // AND lengthens it (the paused suffix) for the overflow assertion.
    h.store.queue_paused.set(true);
    h.store.queue.update(|q| {
        q.push(abstractcode::store::QueuedPrompt {
            id: 1,
            text: "later task".into(),
        });
        q.push(abstractcode::store::QueuedPrompt {
            id: 2,
            text: "even later".into(),
        });
    });
    let screen = h.turn();
    let strip = screen
        .lines()
        .find(|l| l.contains("session: 3 runs"))
        .unwrap_or_default();
    assert!(
        strip.contains('…'),
        "the overflowing idle summary ellipsizes instead of hard-cutting:\n{strip}"
    );
    // And it still leads with the truth it has room for.
    assert!(
        strip.contains("12k in / 900 out tk"),
        "token truth precedes the cut:\n{strip}"
    );
}

/// A click hands the keyboard back to the prompt (review P1): after the
/// modal closes, Enter and typing reach the composer — they never re-run
/// the widget that was clicked (Enter is the probe: Space on an empty
/// prompt is hold-to-talk).
#[test]
fn a_click_hands_the_keyboard_back_to_the_prompt() {
    let mut h = harness_sized(140, 40);
    h.turn();
    for id in [
        ui::chrome::ACT_MODEL,
        ui::chrome::ACT_ACCOUNT,
        ui::chrome::ACT_SESSION,
        ui::chrome::ACT_WORKSPACE,
    ] {
        h.close_all();
        let opened_screen = click_action(&mut h, id);
        let needle = opened(id)[0];
        assert!(
            opened_screen.contains(needle),
            "{id} opened:\n{opened_screen}"
        );
        h.close_all();
        assert!(!h.turn().contains(needle), "{id} closed");
        assert_eq!(
            h.focused_host(),
            None,
            "{id}: no chrome host keeps the focus"
        );
        h.term.push_input(b"\r");
        h.turn();
        let s = h.turn();
        assert!(
            !s.contains(needle),
            "{id}: Enter after the click must not reopen it:\n{s}"
        );
    }
    h.term.push_input(b"zq");
    h.turn();
    let s = h.turn();
    assert!(
        s.lines().any(|l| l.contains("zq")),
        "typing reaches the prompt:\n{s}"
    );
}

/// A gateway-down reason never clears the bar (review P2): cut to 40
/// columns, it yields to the workspace path, and the ctx meter, the
/// session id and the theme hold their places.
#[test]
fn a_gateway_down_reason_never_clears_the_status_bar() {
    for width in [100, 120, 160] {
        let mut h = harness_sized(width, 30);
        h.turn();
        load_everything(&h.store);
        let reason = "gateway unreachable: health: http://127.0.0.1:8080/api/gateway/health: \
                      Connection Failed: Connect error: Connection refused (os error 61)";
        h.store.conn.set(Conn::Down(reason.into(), true));
        let screen = h.turn();
        let footer = screen.lines().last().unwrap_or_default();
        // The session id, the theme, the workspace path and the reason
        // hold at every width; the instruments yield to the reason (the
        // gateway is down), and the ctx meter is back beside it at 160.
        for needle in ["acode-test-session", "⌂ /tmp/ws", "gateway unreachable"] {
            assert!(
                footer.contains(needle),
                "[{width}] {needle} kept:\n{footer}"
            );
        }
        if width >= 160 {
            assert!(
                footer.contains("ctx 41k/262k tk (15%, declared)"),
                "[{width}] the ctx meter fits beside the reason:\n{footer}"
            );
        }
        if footer.contains("gateway unreachable") {
            assert!(
                !footer.contains("Connection refused"),
                "[{width}] the reason is cut to 40 columns:\n{footer}"
            );
        }
        let header = screen.lines().next().unwrap_or_default();
        assert!(
            header.contains("✗ ▾"),
            "[{width}] the header light says down:\n{header}"
        );
    }
}

/// Exercise both hosts: the same action may be drawn in both bars.
/// Locate footer clicks from their rendered labels, without fixed cells.
#[test]
fn instrument_labels_open_their_panels_in_both_bars() {
    let mut h = harness_sized(320, 40);
    h.turn();
    load_everything(&h.store);
    h.store
        .host_state
        .set(abstractcode::store::HostState::Ready(
            abstractcode::store::HostFacts {
                ram_percent: Some(62.0),
                ..Default::default()
            },
        ));
    h.store
        .host_contracts
        .set(Some(abstractcode::store::HostContracts {
            host_state: true,
            ..Default::default()
        }));
    h.turn();
    for (label, id) in [
        ("ctx 41k", ui::chrome::ACT_CACHE),
        ("100k↑ 28k↓", ui::chrome::ACT_CACHE),
        ("gpu 28%", ui::chrome::ACT_RESOURCES),
        ("mem 62%", ui::chrome::ACT_RESOURCES),
        ("? keys + commands", ui::chrome::ACT_HELP),
    ] {
        h.close_all();
        let screen = h.turn();
        let lines: Vec<_> = screen.lines().collect();
        let footer = lines.last().unwrap();
        let byte = footer
            .find(label)
            .unwrap_or_else(|| panic!("{label} absent: {footer}"));
        let col = abstracttui::text::width(&footer[..byte]) + 2;
        let row = lines.len();
        h.term
            .push_input(format!("\x1b[<0;{col};{row}M").as_bytes());
        h.turn();
        h.term
            .push_input(format!("\x1b[<0;{col};{row}m").as_bytes());
        h.turn();
        let after = h.turn();
        assert!(
            opened(id).iter().any(|s| after.contains(s)),
            "{label} opens {id}:\n{after}"
        );
        h.close_all();
        assert_eq!(h.focused_host(), None, "{label} hands focus back");
    }
}

#[test]
fn footer_instruments_are_reachable_with_tab_arrows_and_space() {
    let mut h = harness_sized(320, 40);
    h.turn();
    load_everything(&h.store);
    h.turn();
    for (label, id) in [
        ("ctx 41k", ui::chrome::ACT_CACHE),
        ("gpu 28%", ui::chrome::ACT_RESOURCES),
        ("? keys + commands", ui::chrome::ACT_HELP),
    ] {
        h.close_all();
        let screen = h.turn();
        let lines: Vec<_> = screen.lines().collect();
        let footer = lines.last().unwrap();
        let byte = footer
            .find(label)
            .unwrap_or_else(|| panic!("{label}: {footer}"));
        let x = abstracttui::text::width(&footer[..byte]);
        let y = lines.len() as i32 - 1;
        for _ in 0..12 {
            h.term.push_input(b"\t");
            h.turn();
            if h.focused_host() == Some("bar") {
                break;
            }
        }
        assert_eq!(h.focused_host(), Some("bar"));
        let highlighted = |h: &SizedHarness| {
            h.term
                .screen()
                .cell(x, y)
                .unwrap()
                .paint
                .attrs
                .contains(abstracttui::testing::Attrs::REVERSE)
        };
        for _ in 0..12 {
            if highlighted(&h) {
                break;
            }
            h.term.push_input(b"\x1b[C");
            h.turn();
        }
        assert!(highlighted(&h), "{label} has a keyboard focus ring");
        h.term.push_input(b" ");
        h.turn();
        let after = h.turn();
        assert!(
            opened(id).iter().any(|s| after.contains(s)),
            "{id} opens from the keyboard:\n{after}"
        );
    }
}

fn click_cell(h: &mut SizedHarness, x: i32, y: i32) {
    h.term
        .push_input(format!("\x1b[<0;{};{}M", x + 1, y + 1).as_bytes());
    h.turn();
    h.term
        .push_input(format!("\x1b[<0;{};{}m", x + 1, y + 1).as_bytes());
    h.turn();
}

#[test]
fn ordinary_modal_closes_outside_without_activating_the_tools_under_it() {
    let mut h = harness_sized(240, 40);
    h.turn();
    click_action(&mut h, ui::chrome::ACT_WORKSPACE);
    assert!(h.ctx.modal_open());
    // Clicking its content does not dismiss it.
    click_cell(&mut h, 120, 20);
    assert!(h.ctx.modal_open());
    let tools = h.app.tree().action_rect(ui::chrome::ACT_TOOLS).unwrap();
    click_cell(&mut h, tools.x, tools.y);
    assert!(!h.ctx.modal_open());
    assert!(
        !h.turn().contains("choices for this session"),
        "the dismissing click never acts underneath"
    );
    h.term.push_input(b"preserved typing");
    let screen = h.turn();
    assert!(screen.contains("preserved typing"));
}

#[test]
fn approval_and_question_waits_stay_open_on_outside_click_without_any_decision() {
    use abstractcode::transcript::{PendingWait, WaitKind};
    let mut h = harness_sized(240, 40);
    h.turn();
    for kind in [
        WaitKind::Approval {
            tool_calls: vec![
                serde_json::json!({"name":"write_file","arguments":{"path":"note.txt","content":"x"}}),
            ],
        },
        WaitKind::Ask {
            prompt: "Which project?".into(),
            gate: String::new(),
        },
    ] {
        h.store.phase.set(abstractcode::store::Phase::Running);
        h.store.fold.update(|f| {
            f.pending_wait = Some(PendingWait {
                run_id: "root".into(),
                wait_key: "gate".into(),
                step_id: "step".into(),
                kind,
            })
        });
        h.turn();
        assert!(h.ctx.modal_open());
        let _ = h._rx.try_iter().count();
        click_cell(&mut h, 1, 1);
        assert!(h.ctx.modal_open());
        assert!(h.store.fold.with_untracked(|f| f.pending_wait.is_some()));
        assert!(!h
            ._rx
            .try_iter()
            .any(|cmd| matches!(cmd, Cmd::Resume { .. } | Cmd::Cancel { .. })));
        h.store.fold.update(|f| f.pending_wait = None);
        h.turn();
    }
}

#[test]
fn tools_menu_groups_current_session_tools_skills_and_mcp() {
    let mut h = harness_sized(240, 40);
    h.turn();
    load_everything(&h.store);
    h.turn();
    for (downs, expected) in [
        (0, "tools"),
        (1, "gateway skills"),
        (2, "MCP servers on the gateway"),
    ] {
        h.close_all();
        let menu = click_action(&mut h, ui::chrome::ACT_TOOLS);
        for label in ["Tools", "Skills", "MCP servers", "choices for this session"] {
            assert!(menu.contains(label), "{menu}");
        }
        for _ in 0..downs {
            h.term.push_input(b"\x1b[B");
            h.turn();
        }
        h.term.push_input(b"\r");
        let screen = h.turn();
        assert!(
            screen.to_lowercase().contains(&expected.to_lowercase()),
            "{screen}"
        );
        let footer = screen.lines().last().unwrap();
        assert!(!footer.contains("mcp ") && !footer.contains("skills "));
    }
}

#[test]
fn docs_drawer_keeps_its_question_and_never_steers_the_coding_session() {
    let mut h = harness_sized(240, 40);
    h.turn();
    h.term.push_input(b"coding draft");
    h.turn();
    let _ = h._rx.try_iter().count();
    click_action(&mut h, ui::chrome::ACT_DOCS);
    assert!(h.store.docs_open.get_untracked());
    assert!(h.turn().contains("Ask a documentation question"));
    h.term.push_input(b"How do I open Tools?\r");
    let screen = h.turn();
    assert!(screen.contains("How do I open Tools?"));
    let commands: Vec<_> = h._rx.try_iter().collect();
    assert!(commands.iter().any(|c|matches!(c,Cmd::AssistantAsk{question,session,..} if question=="How do I open Tools?" && session.starts_with("code-docs-assistant:"))));
    assert!(!commands
        .iter()
        .any(|c| matches!(c, Cmd::Start { .. } | Cmd::Steer { .. })));
    assert_eq!(h.store.session_id.get_untracked(), "acode-test-session");
    assert_eq!(
        h.store.phase.get_untracked(),
        abstractcode::store::Phase::Idle
    );
    h.store.docs_chat.update(|s| {
        s.messages
            .push(("assistant".into(), "Open Tools in the top bar.".into()));
        s.busy = false;
    });
    h.turn();
    h.term.push_input(b"\x1b");
    h.turn();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while h.store.docs_open.get_untracked() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.turn();
    }
    let screen = h.turn();
    assert!(!h.store.docs_open.get_untracked());
    assert!(screen.contains("coding draft"));
    h.term.push_input(b" extended");
    assert!(h.turn().contains("coding draft extended"));
    click_action(&mut h, ui::chrome::ACT_DOCS);
    assert!(h.turn().contains("Open Tools in the top bar."));
    click_cell(&mut h, 1, 1);
    assert!(!h.store.docs_open.get_untracked());
}

#[test]
fn account_menu_shows_gateway_identity_and_disconnect_requires_confirmation() {
    let mut h = harness_sized(240, 40);
    h.turn();
    h.store.account.set(abstractcode::store::AccountIdentity {
        user_id: "admin".into(),
        tenant_id: "team".into(),
        error: String::new(),
    });
    h.turn();
    let menu = click_action(&mut h, ui::chrome::ACT_ACCOUNT);
    for label in [
        "Signed in as admin",
        "Tenant: team",
        "Gateway:",
        "Connection status",
        "Copy user id",
        "Disconnect this terminal",
    ] {
        assert!(menu.contains(label), "{menu}");
    }
    // Select by the visible action, so additive menu choices cannot retarget it.
    let (y, line) = menu
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("Disconnect this terminal"))
        .unwrap();
    click_cell(&mut h, line.find("Disconnect").unwrap() as i32, y as i32);
    let screen = h.turn();
    assert!(screen.contains("Disconnect this terminal?"), "{screen}");
    assert!(h.ctx.modal_open());
    click_cell(&mut h, 1, 1);
    assert!(h.ctx.modal_open(), "a disconnect needs an explicit choice");
    assert!(!h.app.quit_requested());
}

#[test]
fn only_a_genuine_human_wait_takes_priority_over_the_docs_drawer() {
    use abstractcode::transcript::{PendingWait, WaitKind};
    let mut h = harness_sized(240, 40);
    h.turn();
    click_action(&mut h, ui::chrome::ACT_DOCS);
    for (tool, expected_open) in [("read_file", true), ("write_file", false)] {
        h.store.fold.update(|f| {
            f.pending_wait = Some(PendingWait {
                run_id: "root".into(),
                wait_key: format!("gate-{tool}"),
                step_id: format!("step-{tool}"),
                kind: WaitKind::Approval {
                    tool_calls: vec![
                        serde_json::json!({"name":tool,"arguments":{"path":"note.txt"}}),
                    ],
                },
            })
        });
        h.turn();
        assert_eq!(h.store.docs_open.get_untracked(), expected_open, "{tool}");
    }
    assert!(h.ctx.modal_open());
}

#[test]
fn run_questions_capture_loaded_context_without_steering_or_crossing_docs_history() {
    let mut h = harness_sized(240, 40);
    h.turn();
    h.store.fold.update(|f| {
        f.begin_run("active-root");
        f.push_item(abstractcode::transcript::Item::User {
            text: "Find the parser problem".into(),
        });
    });
    h.store.phase.set(abstractcode::store::Phase::Running);
    h.store.run_id.set("active-root".into());
    h.store.docs_chat.update(|s| {
        s.source_session = h.store.session_id.get_untracked();
        s.messages
            .push(("assistant".into(), "Existing docs answer".into()))
    });
    h.turn();
    h.store
        .help_scope
        .set(abstractcode::docs_chat::ChatScope::Run);
    h.store.docs_open.set(true);
    assert!(h.turn().contains("Companion · AbstractCode"));
    let _ = h._rx.try_iter().count();
    h.term.push_input(b"What is this run doing?\r");
    h.turn();
    let commands = h._rx.try_iter().collect::<Vec<_>>();
    let request = commands
        .iter()
        .find_map(|c| match c {
            Cmd::AssistantAsk {
                scope: abstractcode::docs_chat::ChatScope::Run,
                context: text,
                ..
            } => Some(text),
            _ => None,
        })
        .expect("run question request");
    assert!(request.contains("active-root") && request.contains("Find the parser problem"));
    assert!(request.contains("observed data") && request.contains("currently loaded"));
    assert!(!commands.iter().any(|c| matches!(
        c,
        Cmd::Start { .. } | Cmd::Steer { .. } | Cmd::Resume { .. } | Cmd::Cancel { .. }
    )));
    assert_eq!(h.store.run_id.get_untracked(), "active-root");
    assert!(h.store.run_chat.with_untracked(|s| s.busy));
    assert!(!h.store.docs_chat.with_untracked(|s| s.busy));
    assert_eq!(h.store.docs_chat.with_untracked(|s| s.messages.len()), 1);
}

#[test]
fn docs_questions_include_fresh_visible_conversation_even_with_cached_documentation() {
    let mut h = harness_sized(240, 40);
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "Todays AI news".into(),
        });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "The visible news summary".into(),
            final_answer: true,
            author: None,
        });
    });
    h.turn();
    click_action(&mut h, ui::chrome::ACT_DOCS);
    h.turn();
    h.term.push_input(b"What is the main conversation about?\r");
    h.turn();
    let context = h
        ._rx
        .try_iter()
        .find_map(|cmd| match cmd {
            Cmd::AssistantAsk {
                scope: abstractcode::docs_chat::ChatScope::Docs,
                context,
                ..
            } => Some(context),
            _ => None,
        })
        .unwrap();
    assert!(context.contains("Todays AI news") && context.contains("visible news summary"));
    h.store.docs_chat.update(|s| {
        s.busy = false;
        s.corpus = Some("Cached product docs".into());
    });
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "A newer visible request".into(),
        })
    });
    h.turn();
    h.term.push_input(b"What changed?\r");
    h.turn();
    let (context, corpus) = h
        ._rx
        .try_iter()
        .find_map(|cmd| match cmd {
            Cmd::AssistantAsk {
                context, corpus, ..
            } => Some((context, corpus)),
            _ => None,
        })
        .unwrap();
    assert!(context.contains("A newer visible request"));
    assert_eq!(corpus.as_deref(), Some("Cached product docs"));
}
