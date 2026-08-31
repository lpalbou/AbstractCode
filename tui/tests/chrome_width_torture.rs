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
use abstractcode::store::{
    Conn, GpuMeter, GpuSample, McpServer, SessionTotals, Store, Workflow,
};
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

        // Right clusters are NEVER sacrificed to the left span.
        assert!(
            header.contains("acode-test-session"),
            "[{width}] header right cluster (session id) intact:\n{header}"
        );
        assert!(
            header.contains('●'),
            "[{width}] connection orb present:\n{header}"
        );
        assert!(
            footer.contains("127.0.0.1:8080"),
            "[{width}] footer right cluster (gateway host) intact:\n{footer}"
        );

        // The wordmark leads the header at every width.
        assert!(
            header.contains("▲ AbstractCode"),
            "[{width}] wordmark present:\n{header}"
        );

        // FACTS never self-truncate into `…` fragments — they drop
        // WHOLE. (The workflow/route IDENTITY span ellipsizes by design:
        // at 60 cols "gate…" hints a route exists, where whole-dropping
        // it would show nothing — only the instrument tiers are bound to
        // whole-drop.) Pin: a fact prefix on screen implies the whole
        // fact on screen.
        for fact in ["⌂ ws", "server-managed", "skills 2", "mcp 1", "128k tk"] {
            let prefix: String = fact.chars().take(3).collect();
            assert!(
                !header.contains(&prefix) || header.contains(fact),
                "[{width}] fact {fact:?} must paint whole or not at all:\n{header}"
            );
        }
        assert!(
            !footer.contains('…'),
            "[{width}] footer segments drop whole, never fragment:\n{footer}"
        );

        // Chips beat facts: if the chip did not fit, no fact may have
        // taken its space (paint order pins priority).
        assert!(
            header.contains("◆castor") || !header.contains("server-managed"),
            "[{width}] a fact must never paint while the chip dropped:\n{header}"
        );

        // Wide screens carry the full instrument row.
        if width >= 200 {
            for needle in [
                "ctx 41k/262k tk (15%, declared)",
                "100k↑ 28k↓ tk session",
                "gpu 28%",
                "skills 2",
                "mcp 1",
                "? keys + commands",
            ] {
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
        // The graded ctx meter holds its slot down to 100 cols.
        if width >= 100 {
            assert!(
                footer.contains("ctx 41k/262k tk (15%, declared)"),
                "[{width}] ctx meter keeps its slot:\n{footer}"
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
