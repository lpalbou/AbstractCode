//! Width torture for the presence/density chrome (cycle-2 review):
//! header + footer + strip rendered at 60 / 80 / 100 / 120 / 200 / 271
//! cols with a fully-loaded store, pinning the degrade rules:
//!
//! - right clusters survive every width (header: session tail + orb;
//!   footer: theme + gateway host),
//! - no `…` self-truncation in header facts or footer instruments
//!   (whole-item drop, right-to-left),
//! - entity chips are never sacrificed to facts (paint order),
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
    let actions = app.actions();
    app.mount(move |cx| {
        let store = Store::create(cx);
        *store_out.borrow_mut() = Some(store);
        store.session_id.set("acode-test-session".into());
        store.conn.set(Conn::Ok);
        store.workflow.set(Workflow {
            bundle_id: "basic-agent".into(),
            flow_id: "81795ea9".into(),
            name: "basic-agent".into(),
            description: String::new(),
            ..Default::default()
        });
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
    SizedHarness {
        app,
        term,
        driver,
        store,
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

    /// The workflow widget's route: `/workflow` refreshes the catalog
    /// first (the picker itself needs a loaded catalog to show rows).
    fn asked_catalog(&self, id: &str) -> bool {
        id == ui::chrome::ACT_WORKFLOW
            && self
                ._rx
                .try_iter()
                .any(|c| matches!(c, Cmd::LoadCatalog { .. }))
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
    for width in [60, 80, 100, 120, 200, 271] {
        let mut h = harness_sized(width, 30);
        h.turn();
        load_everything(&h.store);
        let screen = h.turn();
        let lines: Vec<&str> = screen.lines().collect();
        let header = lines.first().copied().unwrap_or_default();
        let footer = lines.last().copied().unwrap_or_default();

        // HEADER — wordmark, workflow, route on the left; the gateway
        // beside its connection light on the right, never sacrificed.
        assert!(
            header.contains("▲ AbstractCode"),
            "[{width}] wordmark present:\n{header}"
        );
        assert!(
            header.trim_end().ends_with("127.0.0.1:8080 ●"),
            "[{width}] header right widget (gateway + light) intact:\n{header}"
        );
        // The folder moved to the status bar (2026-10-28 layout).
        assert!(
            !header.contains('⌂'),
            "[{width}] the folder is the status bar's, not the header's:\n{header}"
        );
        for fact in ["server-managed", "skills 2", "mcp 1", "128k tk"] {
            let prefix: String = fact.chars().take(3).collect();
            assert!(
                !header.contains(&prefix) || header.contains(fact),
                "[{width}] fact {fact:?} must paint whole or not at all:\n{header}"
            );
        }
        assert!(
            header.contains("◆castor") || !header.contains("server-managed"),
            "[{width}] a fact must never paint while the chip dropped:\n{header}"
        );

        // STATUS BAR (abstracttui 1260 slots) — the theme flush right, the
        // workspace + session in the middle, both kept at EVERY tested
        // width (top priorities); the gateway is the header's only.
        assert!(
            footer.trim_end().ends_with("Dark (Abstract) ▾"),
            "[{width}] the theme selector sits at the bar's right end:\n{footer}"
        );
        assert!(
            footer.contains("⌂ /tmp/ws"),
            "[{width}] the workspace path is in the middle:\n{footer}"
        );
        assert!(
            footer.contains("acode-test-session"),
            "[{width}] the session id is in the middle, whole:\n{footer}"
        );
        let ws = footer.find("⌂ /tmp/ws").unwrap();
        let sid = footer.find("acode-test-session").unwrap();
        let theme = footer.find("Dark (Abstract)").unwrap();
        assert!(
            ws < sid && sid < theme,
            "[{width}] order: workspace, session, theme:\n{footer}"
        );
        assert!(
            !footer.contains("127.0.0.1:8080"),
            "[{width}] the gateway is the header's, not repeated:\n{footer}"
        );
        // Instruments never fragment; they drop WHOLE, lowest priority
        // first (contract: ascending priority yields first).
        assert!(
            !footer.contains('…'),
            "[{width}] no item fragments at this load:\n{footer}"
        );
        let by_priority = [
            "ctx 41k/262k tk (15%, declared)",
            "100k↑ 28k↓ tk session",
            "gpu 28%",
            "skills 2",
            "mcp 1",
            "? keys + commands",
        ];
        for (i, lower) in by_priority.iter().enumerate().skip(1) {
            if footer.contains(lower) {
                for higher in &by_priority[..i] {
                    assert!(
                        footer.contains(higher),
                        "[{width}] {lower:?} shows while higher-priority {higher:?} dropped:\n{footer}"
                    );
                }
            }
        }
        if width >= 200 {
            for needle in by_priority {
                assert!(
                    footer.contains(needle),
                    "[{width}] full footer inventory carries {needle}:\n{footer}"
                );
            }
            assert!(
                header.contains("◆castor") && header.contains("server-managed"),
                "[{width}] chips + facts coexist at wide widths:\n{header}"
            );
        }
        if width >= 120 {
            assert!(
                footer.contains("ctx 41k/262k tk (15%, declared)"),
                "[{width}] ctx meter keeps its slot:\n{footer}"
            );
        }
        if width <= 80 {
            assert!(
                !footer.contains("? keys"),
                "[{width}] the keys pointer yields first:\n{footer}"
            );
        }

        // Every chrome widget is a live hit target at every width.
        for id in [
            ui::chrome::ACT_WORKFLOW,
            ui::chrome::ACT_MODEL,
            ui::chrome::ACT_GATEWAY,
            ui::chrome::ACT_WORKSPACE,
            ui::chrome::ACT_SESSION,
            ui::chrome::ACT_THEME,
        ] {
            assert!(
                h.app.tree().action_rect(id).is_some(),
                "[{width}] {id} is a drawn hit target:\n{screen}"
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
        ui::chrome::ACT_WORKFLOW => &["agent workflow", "no agent workflows discovered"],
        ui::chrome::ACT_MODEL => &["provider —", "no providers discovered"],
        ui::chrome::ACT_GATEWAY => &["status · Esc closes"],
        ui::chrome::ACT_WORKSPACE => &["Workspace"],
        ui::chrome::ACT_SESSION => &["Sessions", "sessions"],
        ui::chrome::ACT_THEME => &["☾ Dark"],
        _ => &[],
    }
}

#[test]
fn every_chrome_widget_opens_its_modal_on_click() {
    // ONE harness per width (the reactive runtime is thread-local: a
    // dropped harness's pending toast timer must not fire into the next).
    for width in [80, 140] {
        let mut h = harness_sized(width, 40);
        h.turn();
        for id in [
            ui::chrome::ACT_WORKFLOW,
            ui::chrome::ACT_MODEL,
            ui::chrome::ACT_GATEWAY,
            ui::chrome::ACT_WORKSPACE,
            ui::chrome::ACT_SESSION,
            ui::chrome::ACT_THEME,
        ] {
            h.close_all();
            let before = h.turn();
            let after = click_action(&mut h, id);
            let hit = opened(id)
                .iter()
                .any(|n| after.contains(n) && !before.contains(n));
            let hit = hit || h.asked_catalog(id);
            assert!(
                hit,
                "[{width}] a click on {id} opens {:?}:\n{after}",
                opened(id)
            );
        }
    }
}

#[test]
fn every_chrome_widget_opens_its_modal_from_the_keyboard() {
    // Keyboard: Tab focuses an action host (header, then the bar), Right
    // moves the cursor across its widgets, Enter activates.
    let header = [
        ui::chrome::ACT_WORKFLOW,
        ui::chrome::ACT_MODEL,
        ui::chrome::ACT_GATEWAY,
    ];
    let bar = [
        ui::chrome::ACT_WORKSPACE,
        ui::chrome::ACT_SESSION,
        ui::chrome::ACT_THEME,
    ];
    let mut h = harness_sized(140, 40);
    h.turn();
    for (host, ids) in [("header", &header), ("bar", &bar)] {
        for (steps, id) in ids.iter().enumerate() {
            h.close_all();
            let before = h.turn();
            // Tab until the host holds focus: its first widget prints the
            // focus ring (REVERSE) — found through the tree, not a cell.
            let mut focused = false;
            for _ in 0..12 {
                h.term.push_input(b"\t");
                h.turn();
                if h.focused_host() == Some(host) {
                    focused = true;
                    break;
                }
            }
            assert!(focused, "Tab reaches the {host}");
            // The host REMEMBERS its cursor across focus changes (engine
            // 0187: FocusIn keeps a still-drawn cursor), so after the
            // previous widget's activation one Right reaches the next.
            for _ in 0..steps.min(1) {
                h.term.push_input(b"\x1b[C");
                h.turn();
            }
            h.term.push_input(b"\r");
            h.turn();
            let after = h.turn();
            let hit = opened(id)
                .iter()
                .any(|n| after.contains(n) && !before.contains(n));
            let hit = hit || h.asked_catalog(id);
            assert!(hit, "Enter on {id} opens {:?}:\n{after}", opened(id));
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
