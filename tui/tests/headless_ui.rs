//! Headless UI tests: the REAL interface driven through AbstractTUI's
//! capture harness — same pipeline as production, no pty.
//!
//! The worker thread is replaced by a dummy command channel; ledger records
//! come from the live-captured fixture, applied to the store between frames
//! exactly as posted closures would apply them.
//!
//! Background-thread wake posts DO reach this harness: `Driver::turn()`
//! runs `reactive::drain_posted()` first every frame, so a closure posted
//! from any thread lands on the next `h.turn()`. The one panic path these
//! tests cannot drive is `gateway::entities::spawn_named` with a panicking
//! body — the fn is private and no public entry takes an injectable body;
//! its end-to-end proof (panic → catch_unwind → wake.post → drain →
//! notice + guarded fold) lives as a unit test beside it
//! (`panic_fold_travels_the_wake_queue_end_to_end`), pumping the SAME
//! `drain_posted()` the driver calls.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;
use serde_json::Value;

use abstractcode::config::Prefs;
use abstractcode::runner::Cmd;
use abstractcode::store::{Phase, Store, Workflow};
use abstractcode::ui::{self, UiCtx};

struct Harness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    rx: mpsc::Receiver<Cmd>,
    /// The SAME prefs the UiCtx persists through. Path-less = ephemeral:
    /// `save()` never touches the filesystem (the real-prefs pollution
    /// incident of 2026-07-21 is structurally impossible here).
    prefs: Rc<RefCell<Prefs>>,
    /// The live UiCtx + root scope, for tests that drive modals directly.
    ctx: UiCtx,
    cx: Scope,
}

fn harness() -> Harness {
    harness_sized(Size::new(100, 30))
}

fn harness_sized(size: Size) -> Harness {
    // A fresh default theme per test (tests share a process).
    abstracttui::app::set_theme_by_id("abstract-dark");
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, rx) = mpsc::channel::<Cmd>();
    let store_slot: Rc<RefCell<Option<Store>>> = Rc::new(RefCell::new(None));
    let store_out = store_slot.clone();
    let ctx_slot: Rc<RefCell<Option<(UiCtx, Scope)>>> = Rc::new(RefCell::new(None));
    let ctx_out = ctx_slot.clone();
    let prefs = Rc::new(RefCell::new(Prefs::default()));
    let prefs_for_ctx = prefs.clone();
    let actions = app.actions();
    app.mount(move |cx| {
        let store = Store::create(cx);
        *store_out.borrow_mut() = Some(store);
        store.session_id.set("acode-test-session".into());
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
            prefs: prefs_for_ctx.clone(),
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
        *ctx_out.borrow_mut() = Some((ctx.clone(), cx));
        ui::root(cx, store, ctx, &actions)
    })
    .expect("mount");
    let mut term = CaptureTerm::new(size);
    let cfg = RunConfig {
        probe: false,
        // Fixed capabilities, never env detection: the host's TERM/
        // COLORTERM must not steer what these tests assert (the engine's
        // own rule for RunConfig.caps). Truecolor also makes inks emit as
        // exact RGB, so style assertions can read theme tokens back from
        // the modeled screen (the diff-tint test below).
        caps: Some(abstracttui::term::Capabilities::with(|c| {
            c.truecolor = true;
            c.colors_256 = true;
            c.unicode_ok = true;
        })),
        ..RunConfig::default()
    };
    let driver = Driver::new(&mut app, &mut term, cfg).expect("driver");
    let store = store_slot.borrow().expect("store created");
    let (ctx, cx) = ctx_slot.borrow().clone().expect("ctx created");
    Harness {
        app,
        term,
        driver,
        store,
        rx,
        prefs,
        ctx,
        cx,
    }
}

impl Harness {
    fn turn(&mut self) -> String {
        self.driver
            .turn(&mut self.app, &mut self.term)
            .expect("turn");
        self.term.screen().to_text()
    }

    fn type_text(&mut self, text: &str) {
        self.term.push_input(text.as_bytes());
    }

    fn press_enter(&mut self) {
        self.term.push_input(b"\r");
    }

    /// Drain queued worker commands until one matches (modal dispatches
    /// enqueue LoadTools/LoadSkills/... ahead of the command under test).
    fn find_cmd(&mut self, mut pred: impl FnMut(&Cmd) -> bool) -> Option<Cmd> {
        while let Ok(cmd) = self.rx.try_recv() {
            if pred(&cmd) {
                return Some(cmd);
            }
        }
        None
    }

    /// Leave the animated splash (push a conversation item + settle):
    /// tests that assert BYTE-IDLE must not run on the splash screen —
    /// its shimmer ticker legitimately emits every ~150ms, so an idle
    /// assert races the timer (caught live: the Ctrl+L both-bindings
    /// pin failed once-in-many runs). Emission-asserting tests need it
    /// too, the other way around: a shimmer tick must never masquerade
    /// as the emission under test.
    fn leave_splash(&mut self) {
        self.store.fold.update(|f| {
            f.push_item(abstractcode::transcript::Item::User {
                text: "settle".into(),
            });
        });
        for _ in 0..3 {
            self.turn();
        }
    }

    /// Send a bare Escape and let the parser's 30ms ESC-disambiguation
    /// deadline expire before the dispatching turn.
    fn press_escape(&mut self) {
        // The bare-ESC disambiguation deadline (30ms) anchors at the byte's
        // ARRIVAL at the reader — one turn to arrive, a real wait, then a
        // turn to resolve + dispatch.
        self.term.push_input(&[0x1b]);
        self.turn();
        std::thread::sleep(std::time::Duration::from_millis(45));
        self.turn();
    }
}

/// The board refuses activation until the gateway answers (the D1
/// guard). These harnesses have no worker, so answer for them: an
/// empty COMPLETE listing still offers the local rows, marked.
fn settle_session_board(h: &mut Harness) {
    h.store
        .session_index
        .set(abstractcode::store::SessionIndex::Loaded {
            rows: Vec::new(),
            truncated: false,
            labeled: 0,
        });
    h.turn();
}

fn fixture_records() -> Vec<(String, Value)> {
    let raw = include_str!("fixtures/agent_subrun_ledger.json");
    let records: Vec<Value> = serde_json::from_str(raw).expect("fixture parses");
    records
        .into_iter()
        .map(|r| {
            let run = r
                .get("run_id")
                .and_then(Value::as_str)
                .unwrap_or("sub")
                .to_string();
            (run, r)
        })
        .collect()
}

#[test]
fn boots_to_empty_state_with_composer() {
    let mut h = harness();
    let screen = h.turn();
    assert!(
        screen.contains("AbstractCode"),
        "wordmark visible:\n{screen}"
    );
    assert!(
        screen.contains("describe a task"),
        "composer placeholder visible:\n{screen}"
    );
    assert!(screen.contains("basic-agent"), "workflow badge:\n{screen}");
    assert!(screen.contains("/help"), "key legend:\n{screen}");
}

/// The splash (IDLE-2): the half-block logotype renders at boot, the
/// whole block is vertically CENTERED (operator ask, 2026-07-23 — the
/// old top-anchor put the first content row at pane row ~2), and the
/// shimmer animation exists ONLY while the splash is visible — the
/// byte channel is the observable (glyphs never move; only inks do),
/// and after the first conversation item the ticker cancels so the
/// idle app returns to zero emissions.
#[test]
fn splash_logo_centers_and_animates_only_while_visible() {
    let mut h = harness();
    let screen = h.turn();
    assert!(
        screen.contains("▄▀█"),
        "half-block logotype renders at boot:\n{screen}"
    );
    let logo_row = screen
        .lines()
        .position(|l| l.contains("▄▀█"))
        .expect("logo row");
    assert!(
        logo_row >= 4,
        "centered block starts well below the header (row {logo_row}):\n{screen}"
    );
    // Animation while visible: settle to byte-idle, then poll bounded
    // (refinement pass: ~0.7% of frame transitions are zero-delta —
    // shimmer in its dark zone + pulse at an extremum rounding to the
    // same ink — so ONE sampled frame could legitimately emit nothing;
    // any two consecutive frames cannot).
    for _ in 0..4 {
        h.turn();
    }
    let mut emitted_any = false;
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let t = h.driver.turn(&mut h.app, &mut h.term).expect("turn");
        emitted_any |= t.emitted;
        if emitted_any {
            break;
        }
    }
    assert!(emitted_any, "the splash shimmer re-emits while visible");
    // Conversation starts: the splash predicate flips, the ticker
    // cancels, and the app is byte-idle again even across a tick period.
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "first task".into(),
        })
    });
    for _ in 0..6 {
        h.turn();
    }
    std::thread::sleep(std::time::Duration::from_millis(220));
    let mut emitted_after = false;
    for _ in 0..2 {
        let t = h.driver.turn(&mut h.app, &mut h.term).expect("turn");
        emitted_after |= t.emitted;
    }
    assert!(
        !emitted_after,
        "no splash emissions after conversation starts (ticker cancelled)"
    );
}

/// Short panes degrade HONESTLY (refinement-pass P1, the 0240 class):
/// at 72×20 the content block is taller than the pane — the fix pins
/// every content row at shrink(0.0) with the outer column clipping, so
/// bottom rows drop WHOLE. The defective build overprinted card rows
/// ("sessionce…" interleavings) and silently downgraded the logo while
/// keeping less important rows.
#[test]
fn splash_short_pane_clips_whole_rows_and_keeps_the_logo() {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let size = Size::new(72, 20);
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, _rx) = mpsc::channel::<Cmd>();
    let actions = app.actions();
    app.mount(move |cx| {
        let store = Store::create(cx);
        store.session_id.set("acode-probe-session".into());
        store.workflow.set(Workflow {
            bundle_id: "basic-agent".into(),
            flow_id: "81795ea9".into(),
            name: "basic-agent".into(),
            description: String::new(),
        });
        store.fold.update(|f| {
            f.push_item(abstractcode::transcript::Item::Info {
                text: "session acode-probe-session · durable memory lives on the gateway".into(),
            });
            f.push_item(abstractcode::transcript::Item::Info {
                text: "workspace: gateway-managed — files land in the gateway's workspace".into(),
            });
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
    let mut driver = Driver::new(&mut app, &mut term, cfg).expect("driver");
    let mut screen = String::new();
    for _ in 0..3 {
        driver.turn(&mut app, &mut term).expect("turn");
        screen = term.screen().to_text();
    }
    // The logo survives (last casualty, never the first).
    assert!(
        screen.contains("▄▀█"),
        "logotype renders on the short pane:\n{screen}"
    );
    // No flex-shrink overprint: every card LABEL that renders as a
    // line's first word is followed by whitespace (the defective build
    // fused a crushed row's label with the surviving row's tail —
    // "sessionce", "skillsyce"). Chrome rows that legitimately start
    // with these words ("session <id> · no runs yet") pass the same
    // rule, so the check is structural, not screen-shape-brittle.
    let labels = [
        "version",
        "workflow",
        "route",
        "cwd",
        "workspace",
        "session",
        "gateway",
        "skills",
        "mcp",
        "context",
    ];
    for row in screen.lines() {
        let trimmed = row.trim_start();
        for label in labels {
            if let Some(rest) = trimmed.strip_prefix(label) {
                assert!(
                    rest.is_empty() || rest.starts_with(char::is_whitespace),
                    "card label {label:?} fused with overprinted text: {row:?}\n{screen}"
                );
            }
        }
    }
}

#[test]
fn typing_a_prompt_sends_start_and_renders_user_card() {
    let mut h = harness();
    h.turn();
    h.type_text("write a haiku");
    h.turn();
    h.press_enter();
    // Two pumps: the feed's first mount discovers its width during draw
    // and syncs the measured extent one frame later (engine contract —
    // the custom body block paints from the settled geometry).
    h.turn();
    let screen = h.turn();
    assert!(screen.contains("you"), "user card header:\n{screen}");
    assert!(screen.contains("write a haiku"), "prompt text:\n{screen}");
    match h.rx.try_recv() {
        Ok(Cmd::Start {
            prompt,
            flow_id,
            bundle_id,
            session_id,
            ..
        }) => {
            assert_eq!(prompt, "write a haiku");
            assert_eq!(flow_id, "81795ea9");
            assert_eq!(bundle_id, "basic-agent");
            assert_eq!(session_id, "acode-test-session");
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
    assert_eq!(h.store.phase.get_untracked(), Phase::Starting);
}

/// After a runner-thread panic the command loop is gone and every send
/// fails. The panic handler already resets the phase to Idle (no spinner
/// claiming control that no longer exists) — but the NEXT submit used to
/// flip the phase back to Starting over a send that went nowhere,
/// wedging the composer forever ("run is still starting…"). A failed
/// start must reset to Idle with an honest error card.
#[test]
fn submit_after_runner_death_never_wedges_the_composer_in_starting() {
    let mut h = harness();
    h.turn();
    // Kill the command loop: replace the receiver with a dead dummy and
    // drop the real one — ctx.tx now fails every send, exactly like a
    // panicked runner thread.
    let (_dummy_tx, dummy_rx) = mpsc::channel::<Cmd>();
    drop(_dummy_tx);
    let dead = std::mem::replace(&mut h.rx, dummy_rx);
    drop(dead);

    h.type_text("do a task");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        h.store.phase.get_untracked(),
        Phase::Idle,
        "a start that went nowhere must not leave the composer in Starting"
    );
    // Second pump: the feed discovers its width at draw and syncs the
    // measured extent one frame later (engine geometry contract).
    let screen = h.turn();
    assert!(
        screen.contains("gateway worker is dead"),
        "the failed start says why:\n{screen}"
    );
    // And a follow-up submit is still answered honestly (no silent wedge).
    h.type_text("try again");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.phase.get_untracked(), Phase::Idle);
}

#[test]
fn real_ledger_replay_renders_tools_approval_and_answer() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));

    let records = fixture_records();
    // Feed up to (and including) the approval wait.
    let mut fed = 0;
    for (run, rec) in &records {
        store.fold.update(|f| {
            let _ = f.apply(run, rec);
        });
        fed += 1;
        let waiting = store.fold.with_untracked(|f| f.pending_wait.is_some());
        if waiting {
            break;
        }
    }
    let screen = h.turn();
    assert!(
        screen.contains("write_file"),
        "tool card visible:\n{screen}"
    );
    assert!(
        screen.contains("tool approval"),
        "approval modal opened:\n{screen}"
    );
    assert!(screen.contains("approve (a)"), "approve button:\n{screen}");

    // Approve via the keyboard shortcut (close is deferred one tick).
    h.type_text("a");
    h.turn();
    let screen = h.turn();
    assert!(
        !screen.contains("approve (a)"),
        "modal closes on approval:\n{screen}"
    );
    match h.rx.try_recv() {
        Ok(Cmd::Resume {
            approved, payload, ..
        }) => {
            assert_eq!(approved, Some(true));
            assert_eq!(payload["approved"], serde_json::json!(true));
        }
        other => panic!("expected Cmd::Resume, got {:?}", other.map(|_| "cmd")),
    }

    // Feed the rest: tool result + final answer.
    for (run, rec) in records.iter().skip(fed) {
        store.fold.update(|f| {
            let _ = f.apply(run, rec);
        });
    }
    let screen = h.turn();
    assert!(
        screen.contains("assistant"),
        "assistant header after answer:\n{screen}"
    );
    assert!(screen.contains("DONE"), "answer text:\n{screen}");
    assert!(
        store.fold.with_untracked(|f| f.finished),
        "fold finished after answer"
    );
}

#[test]
fn slash_theme_switches_live() {
    let mut h = harness();
    h.turn();
    h.type_text("/theme nord");
    h.turn();
    h.press_enter();
    h.turn();
    h.turn(); // the theme rebuild settles (autofocus re-fires on the new tree)
    assert_eq!(abstracttui::app::current_theme().id, "nord");
    // And an unknown theme stays put with a notice queued.
    h.type_text("/theme not-a-theme");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(abstracttui::app::current_theme().id, "nord");
    let notices = h.store.notices.get_untracked();
    assert!(
        notices.iter().any(|n| n.contains("unknown theme")),
        "notice recorded: {notices:?}"
    );
}

#[test]
fn steer_while_running_and_escape_twice_cancels() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| {
        f.begin_run("root");
        // A cycling subrun marks the steer target.
        let rec = serde_json::json!({"run_id": "sub9", "node_id": "reason", "status": "started",
                                      "effect": {"type": "llm_call", "payload": {}}});
        let _ = f.apply("sub9", &rec);
    });
    h.turn();

    h.type_text("focus on tests");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(screen.contains("steer"), "steer chip rendered:\n{screen}");
    match h.rx.try_recv() {
        Ok(Cmd::Steer { run_id, text }) => {
            assert_eq!(run_id, "sub9");
            assert_eq!(text, "focus on tests");
        }
        other => panic!("expected Cmd::Steer, got {:?}", other.map(|_| "cmd")),
    }

    // Esc Esc cancels (composer is empty).
    h.press_escape();
    h.press_escape();
    match h.rx.try_recv() {
        Ok(Cmd::Cancel { run_id }) => assert_eq!(run_id, "root"),
        other => panic!("expected Cmd::Cancel, got {:?}", other.map(|_| "cmd")),
    }
}

#[test]
fn model_picker_browses_without_selecting_and_confirms_on_enter() {
    let mut h = harness();
    h.turn();
    h.store.providers.set(vec![
        abstractcode::store::ProviderInfo {
            name: "lmstudio".into(),
            models: vec!["qwen-a".into(), "qwen-b".into()],
        },
        abstractcode::store::ProviderInfo {
            name: "ollama".into(),
            models: vec![],
        },
    ]);
    h.type_text("/model");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("gateway defaults"),
        "stage 1 opens:\n{screen}"
    );

    // REGRESSION (live crash 2026-07-21): arrow movement must only BROWSE —
    // the old picker applied + closed on selection change, then panicked on
    // the disposed List scope. Browsing must not change the route.
    h.term.push_input(b"\x1b[B"); // Down -> lmstudio
    h.turn();
    assert_eq!(
        h.store.provider.get_untracked(),
        "",
        "browsing selects nothing"
    );
    let screen = h.turn();
    assert!(
        screen.contains("gateway defaults"),
        "modal still open:\n{screen}"
    );

    // Enter on lmstudio opens stage 2 SYNCHRONOUSLY (the modal swap is
    // atomic — same dispatch, same paint): the very next frame must show
    // the model list, with no tick where keys could land elsewhere.
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("lmstudio models"),
        "stage 2 opens on the same turn:\n{screen}"
    );

    // Down to the first model, Enter confirms, modal closes without panic.
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    h.turn();
    let screen = h.turn();
    assert_eq!(h.store.provider.get_untracked(), "lmstudio");
    assert_eq!(h.store.model.get_untracked(), "qwen-a");
    assert!(
        !screen.contains("lmstudio models"),
        "stage 2 closed:\n{screen}"
    );
}

#[test]
fn model_picker_handoff_keys_never_land_on_the_stale_stage_one_layer() {
    // REGRESSION (live 2026-07-21, /model stage 2): the stage-1 layer's
    // removal used to lag one tick behind stage 2's creation. With two
    // modal layers alive at the same z, input dispatch prefers the OLDEST
    // layer while the compositor paints the NEWEST — so the keys aimed at
    // the visible model list landed on the invisible provider list: an
    // arrow moved the DEAD list's selection and Enter applied a provider
    // the user never chose (and closed stage 2's slot). The modal swap
    // must be atomic: once stage 2 exists, stage 1 must be gone for input.
    let mut h = harness();
    h.turn();
    h.store.providers.set(vec![
        abstractcode::store::ProviderInfo {
            name: "lmstudio".into(),
            models: vec!["qwen-a".into(), "qwen-b".into()],
        },
        abstractcode::store::ProviderInfo {
            name: "ollama".into(),
            models: vec![],
        },
    ]);
    h.type_text("/model");
    h.turn();
    h.press_enter();
    h.turn();
    h.term.push_input(b"\x1b[B"); // Down -> lmstudio
    h.turn();
    h.press_enter(); // stage 2 for lmstudio
    h.turn();
    // Down + Enter arriving on the very next turn — the turn where the
    // old code's deferred callbacks land. They must browse + confirm in
    // STAGE 2, never in a lingering stage-1 layer.
    h.term.push_input(b"\x1b[B\r");
    h.turn();
    h.turn();
    let screen = h.turn();
    assert_eq!(
        h.store.provider.get_untracked(),
        "lmstudio",
        "the provider chosen on stage 1 applies — not whatever row the \
         stale layer's selection drifted to:\n{screen}"
    );
    assert_eq!(
        h.store.model.get_untracked(),
        "qwen-a",
        "the arrow+Enter landed on stage 2's list"
    );
    assert!(
        !screen.contains("models —"),
        "stage 2 closed after confirming:\n{screen}"
    );
}

#[test]
fn model_stage_two_stays_interactive_when_an_approval_lands_behind_the_picker() {
    // REGRESSION (live 2026-07-21, /model stage 2 "can't select the
    // model"): with a tool-approval wait pending BEHIND the picker (it
    // arrived while the user browsed providers — wire_wait_modals rightly
    // leaves the picker up), the stage-1 -> stage-2 handoff ran
    // close_modal's epoch bump OUTSIDE any dispatch batch, flushing
    // effects while the modal slot was momentarily empty. wire_wait_modals
    // saw "pending wait + no modal" mid-replacement and opened the
    // approval prompt re-entrantly; open_modal then overwrote the slot
    // with stage 2, DROPPING the prompt's Modal handle without closing it
    // (drop does not close). The zombie approval layer kept swallowing
    // every key while stage 2 painted over it: a visible, dead model list.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.providers.set(vec![
        abstractcode::store::ProviderInfo {
            name: "lmstudio".into(),
            models: vec!["qwen-a".into(), "qwen-b".into()],
        },
        abstractcode::store::ProviderInfo {
            name: "ollama".into(),
            models: vec![],
        },
    ]);

    // The user opens /model mid-run…
    h.type_text("/model");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(screen.contains("provider —"), "stage 1 opens:\n{screen}");

    // …and an approval wait lands while they browse. The picker stays;
    // the wait parks behind it.
    store.fold.update(|f| {
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "act", "status": "waiting", "step_id": "s1",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [{"name": "write_file"}]}},
            "result": {"wait": {"reason": "user", "wait_key": "tool_approval:k1",
                "details": {"mode": "approval_required",
                             "tool_calls": [{"name": "write_file", "arguments": {"f": "x"}}]}}}
        });
        let _ = f.apply("root", &rec);
    });
    let screen = h.turn();
    assert!(
        screen.contains("provider —") && !screen.contains("approve (a)"),
        "picker keeps covering the parked wait:\n{screen}"
    );

    // Down to lmstudio, Enter -> stage 2. The handoff must not let the
    // parked wait interject a prompt that then gets silently overwritten.
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("lmstudio models"),
        "stage 2 opens over the parked wait:\n{screen}"
    );

    // The model list must be LIVE: arrow + Enter select qwen-a.
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    h.turn();
    let screen = h.turn();
    assert_eq!(h.store.provider.get_untracked(), "lmstudio");
    assert_eq!(
        h.store.model.get_untracked(),
        "qwen-a",
        "stage 2 selection applies — no zombie modal layer ate the keys:\n{screen}"
    );

    // Stage 3 (the reasoning dial, first-citizen third axis) now covers
    // the parked wait — SAME zombie-layer contract as stage 1 -> 2: the
    // replacement must leave the list live, and closing it must let the
    // parked prompt return. Row 0 = gateway default closes the picker.
    let screen = h.turn();
    assert!(
        screen.contains("reasoning —"),
        "stage 3 opens after the model choice:\n{screen}"
    );
    h.press_enter();
    h.turn();
    h.turn();

    // And the parked approval prompt comes back, still answerable.
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "the parked approval prompt returns after the picker closes:\n{screen}"
    );
    h.type_text("a");
    h.turn();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume { approved, .. }) => assert_eq!(approved, Some(true)),
        other => panic!("expected Cmd::Resume, got {:?}", other.map(|_| "cmd")),
    }
}

#[test]
fn model_picker_defaults_row_and_empty_provider_apply_without_stage_two() {
    // Enter with no arrow movement = the "gateway defaults" row: clears
    // the override and closes. A provider with NO models applies
    // (provider, "") directly — stage 2 never opens for it.
    let mut h = harness();
    h.turn();
    h.store.providers.set(vec![
        abstractcode::store::ProviderInfo {
            name: "lmstudio".into(),
            models: vec!["qwen-a".into()],
        },
        abstractcode::store::ProviderInfo {
            name: "endpoint:airelay".into(),
            models: vec![],
        },
    ]);
    h.store.provider.set("lmstudio".into());
    h.store.model.set("qwen-a".into());

    // Enter straight away: with a provider override active, the cursor
    // starts on that provider's row — Enter must OPEN ITS STAGE 2, not
    // silently reset; the defaults row is one Up away.
    h.type_text("/model");
    h.turn();
    h.press_enter();
    h.turn();
    h.term.push_input(b"\x1b[A"); // Up -> gateway defaults row
    h.turn();
    h.press_enter();
    h.turn();
    h.turn();
    assert_eq!(h.store.provider.get_untracked(), "");
    assert_eq!(h.store.model.get_untracked(), "");
    let notices = h.store.notices.get_untracked();
    assert!(
        notices.iter().any(|n| n.contains("gateway defaults")),
        "route notice: {notices:?}"
    );

    // A provider with no discovered models: Enter applies (provider, "")
    // and closes — no model stage exists for it.
    h.type_text("/model");
    h.turn();
    h.press_enter();
    h.turn();
    h.term.push_input(b"\x1b[B\x1b[B"); // Down Down -> endpoint:airelay
    h.turn();
    h.press_enter();
    h.turn();
    let screen = h.turn();
    assert_eq!(h.store.provider.get_untracked(), "endpoint:airelay");
    assert_eq!(h.store.model.get_untracked(), "");
    assert!(
        !screen.contains("models —"),
        "no stage 2 for a model-less provider:\n{screen}"
    );
}

/// The /workflow picker through the shared picker shell: browse never
/// selects; SPACE activates (engine 0.2.1 — Enter is pinned by the
/// model/sessions tests through the same shell); the choice persists.
#[test]
fn workflow_picker_selects_and_persists_on_activation() {
    let mut h = harness();
    h.turn();
    h.store.workflows.set(vec![
        Workflow {
            bundle_id: "basic-agent".into(),
            flow_id: "81795ea9".into(),
            name: "basic-agent".into(),
            description: String::new(),
        },
        Workflow {
            bundle_id: "coder".into(),
            flow_id: "flow-2".into(),
            name: "coder".into(),
            description: "writes code".into(),
        },
    ]);
    h.type_text("/workflow");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(screen.contains("agent workflow"), "picker opens:\n{screen}");
    assert!(screen.contains("writes code"), "descriptions:\n{screen}");
    // Arrow to the second workflow: browsing must not select.
    h.term.push_input(b"\x1b[B");
    h.turn();
    assert_eq!(
        h.store.workflow.get_untracked().bundle_id,
        "basic-agent",
        "browsing selects nothing"
    );
    // SPACE chooses too (engine 0.2.1: List activation is Enter, Space,
    // or click-on-selected — Space has no toggle meaning in a
    // single-select List); Enter is pinned by the model/sessions tests
    // through the same shared picker shell.
    h.type_text(" ");
    h.turn();
    h.turn(); // deferred modal close
    let screen = h.turn();
    let picked = h.store.workflow.get_untracked();
    assert_eq!(picked.bundle_id, "coder");
    assert_eq!(picked.flow_id, "flow-2");
    assert!(
        !screen.contains("agent workflow —"),
        "picker closed after choosing:\n{screen}"
    );
    let prefs = h.prefs.borrow();
    assert_eq!(
        prefs.bundle_id.as_deref(),
        Some("coder"),
        "choice persisted"
    );
    assert_eq!(prefs.flow_id.as_deref(), Some("flow-2"));
}

/// /workflow refetches the catalog at open (the /tools //skills //mcp
/// Load*-before-open pattern): the boot's LoadCatalog was the only load a
/// healthy session ever ran, so a long-lived TUI pinned the launch-time
/// snapshot and entrypoints registered after launch never appeared
/// (operator incident 2026-07-25). The preference mirrors saved prefs —
/// the boot's own source — so `load_catalog` re-resolves the same
/// selection and never clobbers the user's.
#[test]
fn workflow_picker_open_refetches_the_catalog() {
    let mut h = harness();
    h.turn();
    {
        let mut p = h.prefs.borrow_mut();
        p.bundle_id = Some("basic-agent".into());
        p.flow_id = Some("81795ea9".into());
    }
    h.store.workflows.set(vec![Workflow {
        bundle_id: "basic-agent".into(),
        flow_id: "81795ea9".into(),
        name: "basic-agent".into(),
        description: String::new(),
    }]);
    while h.rx.try_recv().is_ok() {} // isolate the gesture's own commands
    h.type_text("/workflow");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(screen.contains("agent workflow"), "picker opens:\n{screen}");
    match h.rx.try_recv() {
        Ok(Cmd::LoadCatalog {
            preferred_bundle,
            preferred_flow,
        }) => {
            assert_eq!(preferred_bundle.as_deref(), Some("basic-agent"));
            assert_eq!(preferred_flow.as_deref(), Some("81795ea9"));
        }
        other => panic!("expected LoadCatalog at /workflow open, got {other:?}"),
    }
    assert!(h.rx.try_recv().is_err(), "exactly one refetch per open");
}

#[test]
fn theme_picker_previews_on_arrows_and_reverts_on_escape() {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let mut h = harness();
    h.turn();
    h.type_text("/theme");
    h.turn();
    h.press_enter();
    h.turn();
    // Arrow down: live preview switches the theme but must NOT close/confirm.
    h.term.push_input(b"\x1b[B");
    h.turn();
    let previewed = abstracttui::app::current_theme().id;
    assert_ne!(previewed, "abstract-dark", "arrow previews live");
    // Esc reverts to the original.
    h.press_escape();
    h.turn();
    assert_eq!(abstracttui::app::current_theme().id, "abstract-dark");
}

#[test]
fn details_toggle_keeps_thinking_visible_and_start_carries_context() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    // A finished first turn in the fold.
    store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "first question".into(),
        });
        f.push_item(abstractcode::transcript::Item::Thinking {
            iteration: 1,
            content: "let me think about xyzzy".into(),
            reasoning: String::new(),
            call: abstractcode::transcript::CallCost {
                gen_time_ms: Some(5_000.0),
                input_tokens: 10_000,
                output_tokens: 250,
                cached_tokens: 9_000,
            },
        });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "first answer".into(),
            final_answer: true,
        });
    });
    // Three pumps: width discovery, extent sync, gap-flip geometry
    // round (see truncation_drains… for the settle contract).
    h.turn();
    h.turn();
    let screen = h.turn();
    // Operator directive 2026-08-19: the thinking LEADS its cycle in
    // BOTH detail states — the collapsed default shows the gist under
    // the cycle rule, and Ctrl+D only changes verbosity.
    assert!(
        screen.contains("xyzzy"),
        "thinking gist visible in the collapsed default:\n{screen}"
    );
    assert!(
        screen.contains("── cycle 1"),
        "the cycle rule delimits the turn:\n{screen}"
    );
    // Provider-reported cache reuse rides the rule (operator ask
    // 2026-08-19): 9k of 10k prompt tokens served from cache = 90%.
    assert!(
        screen.contains("90% cached"),
        "the rule names the cache reuse when the provider reports it:\n{screen}"
    );

    // Ctrl+D expands detail; thinking and answers BOTH stay.
    h.term.push_input(&[0x04]); // Ctrl+D
    let screen = h.turn();
    assert!(
        screen.contains("xyzzy"),
        "thinking stays visible with details on:\n{screen}"
    );
    assert!(screen.contains("first answer"), "answers stay:\n{screen}");
    // Back to collapsed for the context-carry half below.
    h.term.push_input(&[0x04]); // Ctrl+D
    h.turn();

    // The next run carries the completed turn as client context.
    h.type_text("second question");
    h.turn();
    h.press_enter();
    h.turn();
    match h.rx.try_recv() {
        Ok(Cmd::Start { opts, .. }) => {
            assert_eq!(
                opts.messages,
                vec![
                    ("user".to_string(), "first question".to_string()),
                    ("assistant".to_string(), "first answer".to_string())
                ],
                "conversation context rides the start"
            );
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
}

#[test]
fn tools_selector_toggles_and_start_carries_allowlist() {
    let mut h = harness();
    h.turn();
    h.store.tools.set(vec![
        abstractcode::store::ToolInfo {
            name: "read_file".into(),
            description: "Read a file".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "web_search".into(),
            description: "Search the web".into(),
            toolset: "web".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "write_file".into(),
            description: "Write a file".into(),
            toolset: "files".into(),
            ..Default::default()
        },
    ]);
    h.type_text("/tools");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("gateway tools — 3 available · untouched"),
        "tools modal title:\n{screen}"
    );
    assert!(screen.contains("[✓] read_file"), "checked rows:\n{screen}");

    // Space toggles the first tool OFF; title flips to explicit-allowlist.
    h.type_text(" ");
    let screen = h.turn();
    assert!(
        screen.contains("2 on / 1 off · explicit allowlist"),
        "toggle reflected:\n{screen}"
    );
    assert!(screen.contains("[ ] read_file"), "unchecked row:\n{screen}");
    assert_eq!(
        h.store.disabled_tools.get_untracked(),
        vec!["read_file".to_string()]
    );

    // Enter closes; a run start now carries the checked set exactly.
    h.press_enter();
    h.turn();
    h.turn(); // deferred modal close
    h.type_text("do the thing");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            assert_eq!(
                opts.tools,
                Some(vec!["web_search".to_string(), "write_file".to_string()]),
                "allowlist = inventory minus disabled"
            );
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
}

/// Per-CATEGORY toggle (operator ask 2026-07-23): `c` flips every
/// grantable tool in the cursor's toolset on/off in one keystroke, and
/// leaves other categories untouched.
#[test]
fn tools_category_toggle_flips_a_whole_toolset() {
    let mut h = harness();
    h.turn();
    h.store.tools.set(vec![
        abstractcode::store::ToolInfo {
            name: "read_file".into(),
            description: "Read a file".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "write_file".into(),
            description: "Write a file".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "web_search".into(),
            description: "Search the web".into(),
            toolset: "web".into(),
            ..Default::default()
        },
    ]);
    h.type_text("/tools");
    h.turn();
    h.press_enter();
    h.turn();
    // Cursor starts at the first tool (read_file, toolset "files"). `c`
    // turns the WHOLE files category off (both files tools on → off).
    h.type_text("c");
    h.turn();
    let disabled = h.store.disabled_tools.get_untracked();
    assert!(
        disabled.contains(&"read_file".to_string()) && disabled.contains(&"write_file".to_string()),
        "the whole files category is off: {disabled:?}"
    );
    assert!(
        !disabled.contains(&"web_search".to_string()),
        "the web category is untouched: {disabled:?}"
    );
    // `c` again turns the files category back on.
    h.type_text("c");
    h.turn();
    let disabled = h.store.disabled_tools.get_untracked();
    assert!(
        !disabled.contains(&"read_file".to_string())
            && !disabled.contains(&"write_file".to_string()),
        "the files category is back on: {disabled:?}"
    );
}

/// Camera tools are OFF by default for a fresh session (operator ask
/// 2026-07-23: privacy). The seed fires once the inventory loads for a
/// session with no saved slot, disables the camera toolset's grantable
/// names, leaves everything else on, and is one-shot.
#[test]
fn camera_tools_seed_off_for_a_fresh_session() {
    let mut h = harness();
    h.turn();
    // A fresh session arms the seed at boot (lib.rs); the harness bypasses
    // boot, so arm it the same way.
    h.store.camera_seed_pending.set(true);
    h.store.tools.set(vec![
        abstractcode::store::ToolInfo {
            name: "read_file".into(),
            description: "Read a file".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "camera_open".into(),
            description: "Turn a camera on".into(),
            toolset: "camera".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "camera_capture_photo".into(),
            description: "Take a photo".into(),
            toolset: "camera".into(),
            ..Default::default()
        },
    ]);
    h.turn(); // the wire_camera_default_off effect fires on the inventory change
    let disabled = h.store.disabled_tools.get_untracked();
    assert!(
        disabled.contains(&"camera_open".to_string())
            && disabled.contains(&"camera_capture_photo".to_string()),
        "camera tools seeded off by default: {disabled:?}"
    );
    assert!(
        !disabled.contains(&"read_file".to_string()),
        "non-camera tools stay on: {disabled:?}"
    );
    assert!(
        !h.store.camera_seed_pending.get_untracked(),
        "the seed is one-shot (flag consumed)"
    );
}

/// Full-catalog surfacing (tool-tiers item H; this seat's c4555
/// commitment): a row the gateway serves `enabled: false` is VISIBLE
/// with its gate, never grantable — Space refuses with a notice naming
/// the gate, `p` refuses a pin, the title counts it separately, and a
/// customized run allowlist excludes it. Also pins the stale-pref
/// state (cycle-2 adversary P1-2): a persisted user-disabled name
/// whose row is NOW served-disabled counts NOWHERE — the title says
/// "untouched" AND the run sends no allowlist (the two surfaces share
/// one effective-disabled predicate; the divergence silently widened
/// the agent's tool set past the workflow's baked pin).
#[test]
fn served_disabled_tools_render_with_gate_and_are_never_grantable() {
    let mut h = harness();
    h.turn();
    // Stale pref: the user disabled send_email BEFORE the gateway's
    // gate turned it off; the name persists in prefs/disabled_tools.
    h.store.disabled_tools.set(vec!["send_email".into()]);
    h.store.tools.set(vec![
        abstractcode::store::ToolInfo {
            name: "send_email".into(),
            description: "Send an email".into(),
            toolset: "comms".into(),
            served_disabled: true,
            enable_gate: "ABSTRACT_ENABLE_COMMS_TOOLS".into(),
            why_disabled: "registered but disabled on this gateway".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "read_file".into(),
            description: "Read a file".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "write_file".into(),
            description: "Write a file".into(),
            toolset: "files".into(),
            ..Default::default()
        },
    ]);
    // The stale pref alone must NOT flip the run into allowlist mode:
    // a served-disabled row cannot run either way, so "untouched"
    // (workflow defaults, tools=None) is the truth both surfaces state.
    h.type_text("stale pref probe");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            assert_eq!(
                opts.tools, None,
                "a stale user-disable on a served-disabled row keeps workflow defaults"
            );
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
    // Reset the run state so the modal flow below starts clean.
    h.store.phase.set(Phase::Idle);
    h.type_text("/tools");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    // "(untouched:" is the discriminating prefix vs the "explicit
    // allowlist" branch; the full phrase ellipsizes at the modal width
    // now that the gated segment shares the title row.
    assert!(
        screen.contains("· untouched"),
        "title agrees with the wire: stale pref on a gated row is not a customization:\n{screen}"
    );
    // Visible with the gate; counted separately from the grantable pool.
    assert!(
        screen.contains("2 available · 1 gated off server-side"),
        "gated count in the title:\n{screen}"
    );
    assert!(
        screen
            .contains("send_email  [disabled on this gateway — gate: ABSTRACT_ENABLE_COMMS_TOOLS]"),
        "disabled row renders its gate:\n{screen}"
    );
    // Cursor row 0 is send_email (comms sorts before files): Space
    // refuses — the user disabled-set is UNCHANGED (the stale pref
    // stays; no new mutation), a toast names the gate.
    h.type_text(" ");
    h.turn();
    assert_eq!(
        h.store.disabled_tools.get_untracked(),
        vec!["send_email".to_string()],
        "Space on a served-disabled row never mutates the selection"
    );
    // `p` refuses a pin for the same reason.
    h.type_text("p");
    h.turn();
    assert!(
        h.store.tool_overrides.get_untracked().is_empty(),
        "no pin lands on a served-disabled row"
    );
    // Disable read_file (cursor down to it) to flip into allowlist mode:
    // the explicit allowlist must exclude the served-disabled row.
    h.term.push_input(b"\x1b[B"); // Down → read_file
    h.turn();
    h.type_text(" ");
    h.turn();
    h.press_enter();
    h.turn();
    h.turn(); // modal close settles
    h.type_text("do the thing");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            assert_eq!(
                opts.tools,
                Some(vec!["write_file".to_string()]),
                "allowlist excludes served-disabled rows AND the user-disabled one"
            );
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
}

/// `n` (all off) scopes to the GRANTABLE rows: served-disabled names are
/// a server fact, not a client selection — parking them in the user's
/// disabled set would persist stale names past a gate flip (cycle-2
/// adversary coverage gap (a)).
#[test]
fn all_off_excludes_served_disabled_rows_from_the_user_set() {
    let mut h = harness();
    h.turn();
    h.store.tools.set(vec![
        abstractcode::store::ToolInfo {
            name: "send_email".into(),
            description: "Send an email".into(),
            toolset: "comms".into(),
            served_disabled: true,
            enable_gate: "ABSTRACT_ENABLE_COMMS_TOOLS".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "read_file".into(),
            description: "Read a file".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "write_file".into(),
            description: "Write a file".into(),
            toolset: "files".into(),
            ..Default::default()
        },
    ]);
    h.type_text("/tools");
    h.turn();
    h.press_enter();
    h.turn();
    h.type_text("n");
    h.turn();
    let mut disabled = h.store.disabled_tools.get_untracked();
    disabled.sort();
    assert_eq!(
        disabled,
        vec!["read_file".to_string(), "write_file".to_string()],
        "n disables every GRANTABLE tool and never parks a served-disabled name"
    );
}

/// The approval card names a served-disabled call's state + gate
/// (cycle-2 adversary P2-1): a disabled call reaching a wait is the
/// defense-in-depth lane, and a bare tier line would imply an
/// approvability the gateway will refuse.
#[test]
fn approval_modal_names_the_gate_on_a_served_disabled_call() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.tools.set(vec![abstractcode::store::ToolInfo {
        name: "send_email".into(),
        description: "Send an email".into(),
        toolset: "comms".into(),
        served_disabled: true,
        enable_gate: "ABSTRACT_ENABLE_COMMS_TOOLS".into(),
        ..Default::default()
    }]);
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s-disabled",
                "tool_approval:disabled",
                serde_json::json!([{"name": "send_email",
                    "arguments": {"to": "x@y.z", "subject": "hi"}}]),
            ),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("disabled on this gateway"),
        "the card states the disabled fact:\n{screen}"
    );
    assert!(
        screen.contains("ABSTRACT_ENABLE_COMMS_TOOLS"),
        "the card names the gate:\n{screen}"
    );
}

/// item 4: `p` in the /tools modal cycles a per-tool approval pin
/// (none → auto → ask → none), renders it, persists it, and the pin
/// reaches the run-start policy expansion (auto lifts above the tier,
/// ask force-asks below it).
#[test]
fn tools_modal_pins_cycle_persist_and_reach_the_start_policy() {
    let mut h = harness();
    h.turn();
    h.store.tools.set(vec![
        abstractcode::store::ToolInfo {
            name: "read_file".into(),
            description: "Read a file".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "fetch_url".into(),
            description: "Fetch a URL".into(),
            toolset: "web".into(),
            ..Default::default()
        },
    ]);
    // read tier by default.
    h.type_text("/tools");
    h.turn();
    h.press_enter();
    h.turn();

    // Cursor on read_file (row 0): p → auto, rendered + persisted. Only
    // read_file is pinned, so the [pin:auto] marker uniquely names it.
    h.type_text("p");
    let screen = h.turn();
    assert!(
        screen.contains("read_file") && screen.contains("[pin:auto]"),
        "auto pin rendered:\n{screen}"
    );
    assert_eq!(
        h.prefs.borrow().tool_overrides,
        vec![("read_file".to_string(), "auto".to_string())]
    );
    // p again → ask.
    h.type_text("p");
    let screen = h.turn();
    assert!(screen.contains("[pin:ask]"), "ask pin rendered:\n{screen}");
    assert_eq!(
        h.store.tool_overrides.get_untracked(),
        vec![("read_file".to_string(), "ask".to_string())]
    );
    // p a third time → NONE: the wrap closes the full none→auto→ask→none
    // round trip headlessly (cycle-2 left this leg to a unit test) — the
    // marker disappears from the render and the persisted override clears.
    h.type_text("p");
    let screen = h.turn();
    assert!(
        !screen.contains("[pin:"),
        "cleared pin renders no marker:\n{screen}"
    );
    assert!(
        h.store.tool_overrides.get_untracked().is_empty(),
        "cleared pin leaves no override: {:?}",
        h.store.tool_overrides.get_untracked()
    );
    assert!(
        h.prefs.borrow().tool_overrides.is_empty(),
        "cleared pin is not persisted"
    );
    // Two more presses land read_file back on ask for the policy leg below.
    h.type_text("p");
    h.turn();
    h.type_text("p");
    let screen = h.turn();
    assert!(screen.contains("[pin:ask]"), "back on ask:\n{screen}");
    // Move to fetch_url (row 1) and pin it auto.
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.type_text("p");
    h.turn();
    assert!(h
        .store
        .tool_overrides
        .get_untracked()
        .contains(&("fetch_url".to_string(), "auto".to_string())));

    // Close and start: read_file is ask-pinned (force-ask), fetch_url is
    // auto-pinned (lifts above the read tier).
    h.press_enter();
    h.turn();
    h.turn();
    h.type_text("go");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            let auto = &opts.tool_policy.auto_approve_tools;
            assert!(
                auto.contains(&"fetch_url".to_string()),
                "auto pin: {auto:?}"
            );
            assert!(
                !auto.contains(&"read_file".to_string()),
                "ask pin: {auto:?}"
            );
            assert_eq!(
                opts.tool_policy.require_approval_tools,
                vec!["read_file".to_string()]
            );
        }
        other => panic!("expected Start, got {:?}", other.map(|_| "cmd")),
    }

    // The full none→auto→ask→none wrap was closed in-modal above (the
    // third `p` press); the unit test pins the same cycle at the fold.
}

#[test]
fn approve_all_sets_permissions_all_and_covers_later_batches() {
    // The c5028 consolidation: 'A' approves the batch AND sets the
    // PERSISTED permissions level to `all` (the old ephemeral blanket is
    // deleted — its ask-pin bypass, disabled-clamp bypass and empty-batch
    // holes died with it). `/permissions read` restores prompting.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(abstractcode::store::Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));

    let approval_record = |step: &str, key: &str, tool: &str| {
        serde_json::json!({
            "run_id": "root", "node_id": "act", "status": "waiting", "step_id": step,
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [{"name": tool}]}},
            "result": {"wait": {"reason": "user", "wait_key": key,
                "details": {"mode": "approval_required",
                             "tool_calls": [{"name": tool, "arguments": {"x": 1}}]}}}
        })
    };

    // First batch prompts; 'A' approves it AND sets permissions: all.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record("s1", "tool_approval:k1", "write_file"),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("approve all (A"),
        "approve-all affordance visible:\n{screen}"
    );
    h.type_text("A");
    h.turn();
    h.turn(); // deferred modal close lands
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume {
            approved, payload, ..
        }) => {
            assert_eq!(approved, Some(true));
            // R3 (c5028): the HUMAN gesture stamps approved_by: user.
            assert_eq!(
                payload.get("approved_by").and_then(|v| v.as_str()),
                Some("user"),
                "human clicks are ledger-distinguishable: {payload}"
            );
        }
        other => panic!("expected Resume, got {:?}", other.map(|_| "cmd")),
    }
    assert_eq!(
        store.accepted_tier.get_untracked(),
        "all",
        "A sets the persisted level"
    );

    // Second batch: NO modal, policy-resumed — and the resume payload
    // names the POLICY as the approver (R3).
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record("s2", "tool_approval:k2", "execute_command"),
        );
    });
    let screen = h.turn();
    assert!(
        !screen.contains("approve (a)"),
        "no prompt modal at permissions all:\n{screen}"
    );
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume {
            wait_key,
            approved,
            payload,
            ..
        }) => {
            assert_eq!(wait_key, "tool_approval:k2");
            assert_eq!(approved, Some(true));
            assert_eq!(
                payload.get("approved_by").and_then(|v| v.as_str()),
                Some("policy"),
                "policy auto-clicks are ledger-distinguishable: {payload}"
            );
        }
        other => panic!("expected auto Resume, got {:?}", other.map(|_| "cmd")),
    }

    // /permissions read restores prompting for the next batch.
    h.type_text("/permissions read");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(store.accepted_tier.get_untracked(), "read");
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record("s3", "tool_approval:k3", "fetch_url"),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "prompting resumes at permissions read:\n{screen}"
    );
}

#[test]
fn tools_selector_windows_long_lists_with_overflow_markers() {
    // REGRESSION (live 2026-07-21): with more rows than the modal body, the
    // bottom of the list was silently cut and never scrolled into view —
    // the window math used precomputed chrome arithmetic instead of the
    // rect the layout actually granted.
    let mut h = harness();
    h.turn();
    let mut tools = Vec::new();
    for i in 0..30 {
        tools.push(abstractcode::store::ToolInfo {
            name: format!("tool_{i:02}"),
            description: "does things".into(),
            toolset: if i < 15 { "files".into() } else { "web".into() },
            ..Default::default()
        });
    }
    h.store.tools.set(tools);
    h.type_text("/tools");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("more"),
        "overflow marker visible when the list exceeds the body:\n{screen}"
    );
    assert!(
        screen.contains("tool_00"),
        "top of the list visible initially:\n{screen}"
    );
    assert!(
        !screen.contains("tool_29"),
        "bottom rows genuinely beyond the window:\n{screen}"
    );
    // Walk the cursor to the LAST tool: the window must follow it.
    for _ in 0..30 {
        h.term.push_input(b"\x1b[B");
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("tool_29"),
        "cursor scrolls the window to the tail:\n{screen}"
    );
    assert!(
        screen.contains("↑") && screen.contains("more"),
        "overflow marker flips to the top edge:\n{screen}"
    );
}

#[test]
fn pause_and_resume_commands_ride_the_gateway_and_own_the_strip() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    // No active run: /pause refuses honestly.
    h.type_text("/pause");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(h.find_cmd(|c| matches!(c, Cmd::Pause { .. })).is_none());

    store.phase.set(abstractcode::store::Phase::Running);
    store.run_id.set("root".into());
    h.type_text("/pause");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Pause { .. })) {
        Some(Cmd::Pause { run_id }) => assert_eq!(run_id, "root"),
        other => panic!("expected Cmd::Pause, got {:?}", other.map(|_| "cmd")),
    }
    // The runner's ack flips the signal; the strip then owns the state.
    store.paused.set(true);
    let screen = h.turn();
    assert!(
        screen.contains("paused durably") && screen.contains("/resume"),
        "paused strip line:\n{screen}"
    );
    h.type_text("/resume");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::ResumePaused { .. })) {
        Some(Cmd::ResumePaused { run_id }) => assert_eq!(run_id, "root"),
        other => panic!("expected Cmd::ResumePaused, got {:?}", other.map(|_| "cmd")),
    }
}

#[test]
fn pending_wait_survives_covering_modals_and_defers_visibly() {
    // REGRESSION (live 2026-07-21): a pending approval could end up with NO
    // modal and no visible way back — a picker opened over the prompt
    // replaced it forever, and a deferred prompt left no trace once later
    // records overwrote the activity text.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(abstractcode::store::Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "act", "status": "waiting", "step_id": "s1",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [{"name": "write_file"}]}},
            "result": {"wait": {"reason": "user", "wait_key": "tool_approval:k1",
                "details": {"mode": "approval_required",
                             "tool_calls": [{"name": "write_file", "arguments": {"f": "x"}}]}}}
        });
        let _ = f.apply("root", &rec);
    });
    let screen = h.turn();
    assert!(screen.contains("approve (a)"), "prompt opens:\n{screen}");

    // A picker opened OVER the prompt replaces it…
    h.store.tools.set(vec![abstractcode::store::ToolInfo {
        name: "read_file".into(),
        description: "Read".into(),
        toolset: "files".into(),
        ..Default::default()
    }]);
    let (cx, ctx) = (h.cx, h.ctx.clone());
    abstractcode::ui::modals::open_tools(cx, store, &ctx);
    let screen = h.turn();
    assert!(
        screen.contains("gateway tools"),
        "picker covers the prompt:\n{screen}"
    );

    // …and when it closes, the approval prompt MUST come back.
    h.press_escape();
    h.turn();
    h.turn(); // deferred close + epoch-driven reopen
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "prompt returns after the covering modal closes:\n{screen}"
    );

    // Esc defers; the strip shows a persistent, loud waiting line even after
    // later records overwrite the activity text.
    h.press_escape();
    h.turn();
    store.fold.update(|f| {
        let rec = serde_json::json!({
            "run_id": "helper", "node_id": "reason", "status": "started",
            "effect": {"type": "llm_call", "payload": {}}
        });
        let _ = f.apply("helper", &rec);
    });
    let screen = h.turn();
    assert!(
        screen.contains("approval needed") && screen.contains("Enter"),
        "persistent waiting affordance on the strip:\n{screen}"
    );

    // Enter on the empty composer reopens the prompt.
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "Enter reopens the deferred prompt:\n{screen}"
    );
}

#[test]
fn stale_disabled_tools_never_underflow_or_force_allowlist_mode() {
    // REGRESSION (adversary findings 2+6): disabled names persisted from
    // another gateway exceed / miss the current inventory. The title must
    // never underflow (u64::MAX in release) and stale-only disabled must
    // count as UNTOUCHED (workflow defaults), not explicit-allowlist mode.
    let mut h = harness();
    h.turn();
    h.store.tools.set(vec![abstractcode::store::ToolInfo {
        name: "read_file".into(),
        description: "Read".into(),
        toolset: "files".into(),
        ..Default::default()
    }]);
    h.store.disabled_tools.set(vec![
        "gone_tool_a".into(),
        "gone_tool_b".into(),
        "gone_tool_c".into(),
    ]);
    h.type_text("/tools");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("1 available · untouched"),
        "stale-only disabled counts as untouched, no underflow:\n{screen}"
    );
    // A run start with stale-only disabled sends NO allowlist.
    h.press_escape();
    h.turn();
    h.type_text("go");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            assert_eq!(opts.tools, None, "stale names never force allowlist mode");
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
}

#[test]
fn multibyte_session_id_renders_without_panic() {
    // REGRESSION (adversary finding 3): the header truncated the session id
    // with a byte slice; a multibyte id paniced the render loop every frame.
    let mut h = harness();
    h.turn();
    h.store
        .session_id
        .set("aaaaéééééééééééééééé-très-long".into());
    let screen = h.turn();
    assert!(
        screen.contains("…"),
        "long multibyte session id truncates char-safely:\n{screen}"
    );
}

#[test]
fn delegate_child_calls_never_relabel_model_or_context() {
    // REGRESSION (adversary finding 4): a delegate child's tiny llm_call
    // must not overwrite the served-model label or the ctx chip; cumulative
    // totals still fold from the whole tree.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.fold.update(|f| f.begin_run("root"));
    let llm = |run: &str, node: &str, status: &str, model: &str, input: u64| {
        serde_json::json!({
            "run_id": run, "node_id": node, "status": status,
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "x", "model": model,
                        "usage": {"input_tokens": input, "output_tokens": 5}}
        })
    };
    store.fold.update(|f| {
        // The agent lane cycles on the root here (single-run flow).
        let _ = f.apply(
            "root",
            &llm("root", "reason", "completed", "ornith-1.0-35b", 30_000),
        );
        // A delegate child (followed subrun) completes with a tiny model.
        let _ = f.apply(
            "child",
            &llm("child", "call", "completed", "tiny-summarizer-1b", 400),
        );
    });
    let stats = store.fold.with_untracked(|f| f.stats.clone());
    assert_eq!(
        stats.effective_model, "ornith-1.0-35b",
        "served-model label stays on the answer lane"
    );
    assert_eq!(
        stats.last_input_tokens, 30_000,
        "ctx chip stays on the answer lane"
    );
    assert_eq!(
        stats.input_tokens, 30_400,
        "cumulative totals still fold the whole tree"
    );
}

#[test]
fn details_command_immediately_rerenders_mixed_content() {
    // The /details COMMAND path (not just Ctrl+D) must repaint at once.
    // New semantics (operator directive 2026-08-19): the collapsed
    // DEFAULT shows the thinking gist + one-line tool calls with
    // status words; /details expands args + result bodies; /details
    // again collapses. Thinking and every called tool stay visible in
    // BOTH states; errors too (honesty over tidiness).
    let mut h = harness();
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "run the suite".into(),
        });
        f.push_item(abstractcode::transcript::Item::Thinking {
            iteration: 1,
            content: "pondering the xyzzy strategy".into(),
            reasoning: String::new(),
            call: abstractcode::transcript::CallCost::default(),
        });
        f.push_item(abstractcode::transcript::Item::Tool {
            key: "call:1".into(),
            name: "execute_command".into(),
            args_preview: "cargo test".into(),
            args_full: String::new(),
            status: abstractcode::transcript::ToolStatus::Ok,
            result: "result-plugh-lines".into(),
            error: String::new(),
        });
        f.push_item(abstractcode::transcript::Item::Tool {
            key: "call:2".into(),
            name: "broken_tool".into(),
            args_preview: String::new(),
            args_full: String::new(),
            status: abstractcode::transcript::ToolStatus::Failed,
            result: String::new(),
            error: "exploded".into(),
        });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "all green".into(),
            final_answer: true,
        });
    });
    // Three pumps: width discovery, extent sync, gap-flip geometry
    // round (see truncation_drains… for the settle contract).
    h.turn();
    h.turn();
    let screen = h.turn();
    // Collapsed default: the thinking gist leads its cycle; the tool
    // row is the call + its status word; bodies are folded.
    assert!(
        screen.contains("xyzzy"),
        "thinking gist visible by default:\n{screen}"
    );
    assert!(
        screen.contains("execute_command") && screen.contains("ok"),
        "the collapsed tool row carries the call + its status word:\n{screen}"
    );
    assert!(
        !screen.contains("result-plugh-lines"),
        "tool result body folded by default:\n{screen}"
    );
    assert!(
        screen.contains("cargo test"),
        "the collapsed row keeps a one-line args hint (which call was this):\n{screen}"
    );
    assert!(
        screen.contains("broken_tool") && screen.contains("exploded"),
        "failed tools show their error in the collapsed view (honesty):\n{screen}"
    );
    assert!(screen.contains("all green"), "answer stays:\n{screen}");

    // /details: args + result bodies appear immediately.
    h.type_text("/details");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("result-plugh-lines"),
        "tool result body appears after /details:\n{screen}"
    );
    assert!(
        screen.contains("cargo test"),
        "the args preview appears after /details:\n{screen}"
    );
    assert!(
        screen.contains("xyzzy"),
        "thinking stays visible with details on:\n{screen}"
    );

    // /details again: back to the collapsed view at once.
    h.type_text("/details");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        !screen.contains("result-plugh-lines"),
        "result body collapses again:\n{screen}"
    );
    assert!(
        screen.contains("xyzzy"),
        "thinking still visible collapsed:\n{screen}"
    );
}

#[test]
fn skills_selector_attaches_and_start_carries_skills() {
    let mut h = harness();
    h.turn();
    h.store.skills_catalog.set(vec![
        abstractcode::store::SkillInfo {
            name: "coredoc".into(),
            description: "Documentation discipline".into(),
            trust: "adopted".into(),
            blocked: false,
        },
        abstractcode::store::SkillInfo {
            name: "sketchy".into(),
            description: "Not trusted".into(),
            trust: "unknown".into(),
            blocked: true,
        },
    ]);
    h.type_text("/skills");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("2 on the shelf · 0 attached"),
        "skills modal:\n{screen}"
    );
    h.type_text(" "); // attach coredoc
    let screen = h.turn();
    assert!(screen.contains("1 attached"), "attach reflected:\n{screen}");
    // A blocked skill refuses with a notice instead of toggling.
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.type_text(" ");
    h.turn();
    assert_eq!(
        h.store.selected_skills.get_untracked(),
        vec!["coredoc".to_string()]
    );
    let notices = h.store.notices.get_untracked();
    assert!(
        notices.iter().any(|n| n.contains("blocked")),
        "blocked notice: {notices:?}"
    );

    h.press_escape();
    h.turn();
    h.type_text("go");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            assert_eq!(opts.skills, vec!["coredoc".to_string()]);
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
}

#[test]
fn sessions_picker_switches_to_a_recent_session() {
    let mut h = harness();
    h.turn();
    {
        let mut prefs = h.prefs.borrow_mut();
        prefs.touch_session("acode-old-session", Some("fix the parser"));
        prefs.touch_session("acode-test-session", Some("current work"));
    }
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    h.turn();
    // The board waits on the gateway before it renders rows; this
    // harness has no worker, so answer for it. An empty LOADED listing
    // is the "gateway has nothing" case, which still offers the local
    // rows (marked) so a switch is possible.
    h.store
        .session_index
        .set(abstractcode::store::SessionIndex::Loaded {
            rows: Vec::new(),
            truncated: false,
            labeled: 0,
        });
    let screen = h.turn();
    assert!(
        screen.contains("fix the parser"),
        "sessions listed with labels:\n{screen}"
    );
    // Down to the older session, Enter switches.
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    h.turn(); // deferred close
    assert_eq!(
        h.store.session_id.get_untracked(),
        "acode-old-session",
        "session switched"
    );
    match h.find_cmd(|c| matches!(c, Cmd::ProbeAttach { .. })) {
        Some(Cmd::ProbeAttach { session_id, .. }) => assert_eq!(session_id, "acode-old-session"),
        other => panic!("expected Cmd::ProbeAttach, got {:?}", other.map(|_| "cmd")),
    }
}

/// The session-loading screen (operator ask, 2026-08-28): picking a
/// session in /sessions must show an ANIMATED waiting surface — spinner,
/// a bar that goes determinate on the worker's counters, honest words —
/// for the whole restore window, then hand off to the restored
/// transcript. Before this the window rendered the splash, whose
/// "describe a task below" guidance is a lie about a session with
/// history in flight.
#[test]
fn session_pick_shows_the_animated_loading_screen_until_history_lands() {
    let mut h = harness();
    h.turn();
    {
        let mut prefs = h.prefs.borrow_mut();
        prefs.touch_session("acode-old-session", Some("fix the parser"));
        prefs.touch_session("acode-test-session", Some("current work"));
    }
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    h.turn();
    settle_session_board(&mut h);
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    h.turn(); // deferred modal close
              // The pick armed the screen ON the UI thread — the waiting surface
              // must not depend on when the worker reaches the probe.
    assert!(
        h.store.restoring.get_untracked(),
        "restoring arms at the pick gesture"
    );
    let screen = h.turn();
    assert!(
        screen.contains("restoring session"),
        "the loading label names the act:\n{screen}"
    );
    assert!(
        screen.contains("old-session"),
        "…and the session being loaded:\n{screen}"
    );
    assert!(
        screen.contains("listing this session's runs"),
        "unknown denominator = the honest indeterminate caption:\n{screen}"
    );
    assert!(
        ['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷']
            .iter()
            .any(|g| screen.contains(*g)),
        "the braille spinner is on screen:\n{screen}"
    );
    // The worker posts the denominator + per-turn counters: the bar
    // turns determinate and both caption and strip count honestly.
    h.store.restore_progress.set(Some((3, 9)));
    let screen = h.turn();
    assert!(
        screen.contains("restored 3 of 9 prior turn(s)"),
        "determinate caption counts the fetches:\n{screen}"
    );
    assert!(
        screen.contains("(turn 3 of 9)"),
        "the strip repeats the counter:\n{screen}"
    );
    assert!(
        screen.lines().any(|l| l.contains("██████████")),
        "the bar carries real fill (a run no wordmark glyphs produce):\n{screen}"
    );
    // ANIMATED while restoring: the surface re-emits across ticker
    // frames (same bounded poll as the splash-shimmer pin — any two
    // consecutive frames cannot both be zero-delta).
    for _ in 0..4 {
        h.turn();
    }
    let mut emitted_any = false;
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let t = h.driver.turn(&mut h.app, &mut h.term).expect("turn");
        emitted_any |= t.emitted;
        if emitted_any {
            break;
        }
    }
    assert!(emitted_any, "the loading screen animates while restoring");
    // The restore lands — the fold swap posts, then the clear, exactly
    // probe_attach's order: the loading surface hands off to the
    // restored transcript, never to a splash flash.
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "prior turn".into(),
        });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "prior answer".into(),
            final_answer: true,
        });
    });
    let mid = h.turn();
    assert!(
        mid.contains("restoring session"),
        "fold landed but restoring not yet cleared: STILL the loading screen:\n{mid}"
    );
    h.store.restoring.set(false);
    h.store.restore_progress.set(None);
    h.turn(); // the Scroll mounts on this frame…
    let screen = h.turn(); // …and paints its content on the next
    assert!(
        !screen.contains("restoring session"),
        "the loading surface leaves with the flag:\n{screen}"
    );
    assert!(
        screen.contains("prior answer"),
        "the restored transcript is on screen:\n{screen}"
    );
}

/// Right-click on a transcript item (operator ask, 2026-08-28): a
/// secondary press on a tool row opens the engine ContextMenu with the
/// card's own actions; Enter commits the highlighted one, the copy
/// lands as an OSC 52 write, and the notice names what left.
///
/// SCROLLED, deliberately (adversarial review 2026-08-28, H-3): the
/// first cut used a short fold that fit the pane, so the scroll offset
/// was always 0 and the row mapping was never really tested — the
/// handler's `+ offset` term could be deleted and this still passed,
/// which is exactly how the pin-lag defect survived the suite. With
/// filler pushing the content past the viewport, the press must still
/// land on the row under the cursor.
#[test]
fn right_click_on_a_tool_row_opens_its_action_menu() {
    let mut h = harness();
    h.turn();
    h.leave_splash();
    h.store.fold.update(|f| {
        // Enough content to scroll: the mapping is only meaningful
        // once the viewport shows a window, not the whole fold.
        for i in 0..40 {
            f.push_item(abstractcode::transcript::Item::User {
                text: format!("filler line {i}"),
            });
        }
        f.push_item(abstractcode::transcript::Item::Tool {
            key: "t1".into(),
            name: "read_file".into(),
            // Absolute path: relative ones only link/copy when they
            // exist under the workspace root (linkify's honesty rule),
            // and this harness's /tmp/ws holds nothing.
            args_preview: "/etc/hosts head".into(),
            args_full: "path: /etc/hosts".into(),
            status: abstractcode::transcript::ToolStatus::Ok,
            result: "fn main() {}".into(),
            error: String::new(),
        });
    });
    for _ in 0..4 {
        h.turn();
    }
    let screen = h.turn();
    assert!(
        !screen.contains("filler line 0"),
        "the transcript is genuinely scrolled (row 0 is off-screen):\n{screen}"
    );
    let row0 = screen
        .lines()
        .position(|l| l.contains("read_file"))
        .expect("tool row on screen");
    let (x, y) = (4, row0 as i32 + 1); // SGR coordinates are 1-based
    h.term.push_input(format!("\x1b[<2;{x};{y}M").as_bytes());
    h.turn();
    h.term.push_input(format!("\x1b[<2;{x};{y}m").as_bytes());
    let screen = h.turn();
    assert!(
        screen.contains("Copy arguments") && screen.contains("Copy result"),
        "the TOOL card's menu is open — not a filler line's (a filler \
         item offers Copy message/Quote, so this also pins that the row \
         mapping resolved the right item):\n{screen}"
    );
    assert!(
        screen.contains("Copy path"),
        "an args path affords its own copy:\n{screen}"
    );
    h.press_enter(); // first enabled action: Copy arguments
    let screen = h.turn();
    assert!(
        !screen.contains("Copy arguments"),
        "the menu closed on commit:\n{screen}"
    );
    let notices = h.store.notices.get_untracked();
    assert!(
        notices.iter().any(|n| n.contains("copied arguments")),
        "the copy is announced by name: {notices:?}"
    );
}

/// A right-click that resolves to no item, and one on a card with
/// nothing to copy, both SAY so rather than dying silently: the press
/// is consumed either way, and an unexplained dead gesture reads as a
/// broken menu (adversarial review 2026-08-28, I-2).
#[test]
fn a_right_click_with_nothing_to_offer_says_so() {
    let mut h = harness();
    h.turn();
    h.leave_splash();
    h.store.fold.update(|f| {
        // A no-argument tool still Running: no args, no result, no
        // error — every row it could offer is disabled, so the engine's
        // ContextMenu would refuse to open and the press would vanish.
        f.push_item(abstractcode::transcript::Item::Tool {
            key: "t2".into(),
            name: "list_entities".into(),
            args_preview: String::new(),
            args_full: String::new(),
            status: abstractcode::transcript::ToolStatus::Running,
            result: String::new(),
            error: String::new(),
        });
    });
    for _ in 0..3 {
        h.turn();
    }
    let screen = h.turn();
    let row = screen
        .lines()
        .position(|l| l.contains("list_entities"))
        .expect("tool row on screen") as i32
        + 1;
    h.term.push_input(format!("\x1b[<2;4;{row}M").as_bytes());
    h.turn();
    h.term.push_input(format!("\x1b[<2;4;{row}m").as_bytes());
    h.turn();
    let notices = h.store.notices.get_untracked();
    assert!(
        notices.iter().any(|n| n.contains("nothing to copy")),
        "an inert card explains itself instead of ignoring the press: {notices:?}"
    );
}

/// The disclosure marker (operator ask, 2026-08-28). A folded tool card
/// NAMES the output it is hiding, clicking that marker expands it in
/// place, and clicking again puts it away — without flipping the whole
/// transcript to `/details`.
#[test]
fn a_tool_card_names_its_hidden_output_and_expands_on_the_marker() {
    let mut h = harness();
    h.turn();
    h.leave_splash();
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::Tool {
            key: "run-1:node:0:call-a".into(),
            name: "execute_command".into(),
            args_preview: "cargo build".into(),
            args_full: "command: cargo build".into(),
            status: abstractcode::transcript::ToolStatus::Ok,
            result: "compiling\nwarning: unused\nFinished in 41s".into(),
            error: String::new(),
        });
    });
    for _ in 0..3 {
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("▸ 3 lines"),
        "the folded row names what it hides:\n{screen}"
    );
    assert!(
        !screen.contains("Finished in 41s"),
        "…and is not showing it yet:\n{screen}"
    );
    // Click the marker itself (right-aligned on the tool row).
    let row = screen
        .lines()
        .position(|l| l.contains("▸ 3 lines"))
        .expect("marker row");
    let mx = screen
        .lines()
        .nth(row)
        .unwrap()
        .find('▸')
        .expect("marker x")
        + 2;
    h.term
        .push_input(format!("\x1b[<0;{mx};{}M", row + 1).as_bytes());
    h.turn();
    h.term
        .push_input(format!("\x1b[<0;{mx};{}m", row + 1).as_bytes());
    for _ in 0..2 {
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("Finished in 41s"),
        "the marker expanded THIS card in place:\n{screen}"
    );
    assert!(
        screen.contains("▾ 3 lines"),
        "the arrow reflects the open state:\n{screen}"
    );
    assert!(
        !h.store.show_details.get_untracked(),
        "per-card expansion never flips the GLOBAL details mode"
    );
    // Clicking again collapses it.
    h.term
        .push_input(format!("\x1b[<0;{mx};{}M", row + 1).as_bytes());
    h.turn();
    h.term
        .push_input(format!("\x1b[<0;{mx};{}m", row + 1).as_bytes());
    for _ in 0..2 {
        h.turn();
    }
    let screen = h.turn();
    assert!(
        !screen.contains("Finished in 41s"),
        "clicking the marker again puts the body away:\n{screen}"
    );
}

/// The marker's hit box is the marker's CELLS, nothing more. Screen
/// text-selection is enabled app-wide, and the engine's own
/// `Feed::on_item_press` consumes every press that lands on an item —
/// wiring that would have killed drag-to-select across the whole
/// transcript, so this pins that a press on the row's TEXT does not
/// toggle anything.
#[test]
fn a_press_off_the_marker_never_toggles_the_card() {
    let mut h = harness();
    h.turn();
    h.leave_splash();
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::Tool {
            key: "run-1:node:0:call-b".into(),
            name: "execute_command".into(),
            args_preview: "cargo build".into(),
            args_full: String::new(),
            status: abstractcode::transcript::ToolStatus::Ok,
            result: "compiling\nFinished".into(),
            error: String::new(),
        });
    });
    for _ in 0..3 {
        h.turn();
    }
    let screen = h.turn();
    let row = screen
        .lines()
        .position(|l| l.contains("▸ 2 lines"))
        .expect("marker row");
    // Press on the tool NAME, far left of the marker.
    h.term
        .push_input(format!("\x1b[<0;4;{}M", row + 1).as_bytes());
    h.turn();
    h.term
        .push_input(format!("\x1b[<0;4;{}m", row + 1).as_bytes());
    for _ in 0..2 {
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("▸ 2 lines") && !screen.contains("│ Finished"),
        "a press on the row's text selects, it does not expand:\n{screen}"
    );
    assert!(
        h.store.expanded_tools.get_untracked().is_empty(),
        "nothing was toggled"
    );
}

/// `/sessions` asks the GATEWAY which sessions exist (operator,
/// 2026-08-28). The board renders a session this client has never
/// heard of, states where each column came from, and never passes the
/// local file off as server truth.
#[test]
fn the_sessions_board_shows_gateway_sessions_and_names_its_sources() {
    let mut h = harness();
    h.turn();
    {
        let mut prefs = h.prefs.borrow_mut();
        prefs.touch_session("acode-test-session", Some("current work"));
    }
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    // Opening ASKS the gateway.
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadSessions { .. }))
            .is_some(),
        "the board fetches at the gesture"
    );
    assert!(
        screen.contains("asking the gateway"),
        "…and says the live column has not landed yet:\n{screen}"
    );
    // The worker answers with a session this client has never seen.
    h.store
        .session_index
        .set(abstractcode::store::SessionIndex::Loaded {
            rows: vec![
                abstractcode::store::SessionRow {
                    id: "acode-never-seen".into(),
                    state: abstractcode::store::SessionState::Waiting,
                    last_at: "2026-08-28T12:00:00Z".into(),
                    turns: 4,
                    first_run: String::new(),
                    prompt: None,
                },
                abstractcode::store::SessionRow {
                    id: "acode-test-session".into(),
                    state: abstractcode::store::SessionState::Done,
                    last_at: "2026-08-28T09:00:00Z".into(),
                    turns: 1,
                    first_run: String::new(),
                    prompt: None,
                },
            ],
            truncated: false,
            labeled: 0,
        });
    let screen = h.turn();
    assert!(
        screen.contains("never-seen"),
        "a gateway session absent from the local file is offered:\n{screen}"
    );
    assert!(
        screen.contains("waiting on you"),
        "its live state is the gateway's word:\n{screen}"
    );
    assert!(
        screen.contains("current work"),
        "the locally-remembered prompt still labels the row it belongs to:\n{screen}"
    );
    // Provenance is stated — server state vs client memory — and short
    // enough to actually FIT the hint row (review D5: the long form
    // ellipsized away at every terminal width).
    assert!(
        screen.contains("sessions on the gateway"),
        "provenance is stated — server state vs client memory:\n{screen}"
    );
}

/// On a pane too narrow to DRAW the marker, no cell on that row is a
/// control.
///
/// The draw drops a marker it cannot fit; the hit-test used to have no
/// such term, so at ~34 columns a left click on the args hint silently
/// expanded the body AND swallowed the press that would have started a
/// text selection — a control with no glyph on screen (adversarial
/// review 2026-08-29, D1). Both sides read one layout function now;
/// this pins that the wiring honors it.
#[test]
fn a_narrow_pane_that_cannot_draw_the_marker_has_no_hidden_control() {
    // 34 pane columns is the measured drop threshold for this row
    // (glyph + `execute_command` + `· ok` + a 9-cell marker); the
    // terminal is wider than the pane by its padding and scrollbar.
    let mut h = harness_sized(Size::new(36, 24));
    h.turn();
    h.leave_splash();
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::Tool {
            key: "run-1:node:0:narrow".into(),
            name: "execute_command".into(),
            args_preview: "cargo build".into(),
            args_full: String::new(),
            status: abstractcode::transcript::ToolStatus::Ok,
            result: "one\ntwo\nthree".into(),
            error: String::new(),
        });
    });
    for _ in 0..3 {
        h.turn();
    }
    let screen = h.turn();
    // The name ellipsizes at this width, so find the row by its tag.
    let row = screen
        .lines()
        .position(|l| l.contains("· ok"))
        .expect("tool row on screen");
    assert!(
        !screen.lines().nth(row).unwrap().contains('▸'),
        "this pane is too narrow to draw a marker (that is the premise):\n{screen}"
    );
    // Click every cell across the row, including the far right where a
    // marker WOULD have been. Nothing may toggle.
    for x in 1..=36 {
        h.term
            .push_input(format!("\x1b[<0;{x};{}M", row + 1).as_bytes());
        h.turn();
        h.term
            .push_input(format!("\x1b[<0;{x};{}m", row + 1).as_bytes());
        h.turn();
    }
    assert!(
        h.store.expanded_tools.get_untracked().is_empty(),
        "no cell on a markerless row is a control — found {:?}",
        h.store.expanded_tools.get_untracked()
    );
}

/// While the fetch is in flight the board shows the WAITING surface,
/// not a table (operator, 2026-08-29: "this is a bad display when
/// loading… you already have a loading screen, use it").
///
/// The bug: local rows rendered a state column nobody had asked the
/// gateway about yet, so every one of them read "not on the gateway" —
/// a claim about a server that had not been contacted.
#[test]
fn the_board_waits_before_it_claims_anything_about_the_gateway() {
    let mut h = harness();
    h.turn();
    {
        let mut prefs = h.prefs.borrow_mut();
        prefs.touch_session("acode-remembered", Some("older work"));
    }
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("asking the gateway"),
        "the waiting surface is shown:\n{screen}"
    );
    assert!(
        !screen.contains("not on the gateway"),
        "NOTHING is claimed about the gateway before it answers:\n{screen}"
    );
    assert!(
        !screen.contains("older work"),
        "and no half-populated table leaks through the wait:\n{screen}"
    );
    // The answer lands: now the board renders, and the local row is
    // marked with what the listing actually proved.
    h.store
        .session_index
        .set(abstractcode::store::SessionIndex::Loaded {
            rows: Vec::new(),
            truncated: false,
            labeled: 0,
        });
    let screen = h.turn();
    assert!(
        screen.contains("older work") && screen.contains("not on the gateway"),
        "a COMPLETE listing makes absence provable, so now it may be said:\n{screen}"
    );
}

/// A truncated listing may not call a missing session absent — it only
/// proves the session was not in THIS page. On the operator's gateway
/// (119 sessions) a 500-run cap made every missing session read "not on
/// the gateway" when all of them were there.
#[test]
fn a_truncated_listing_never_claims_a_session_is_missing() {
    let mut h = harness();
    h.turn();
    {
        let mut prefs = h.prefs.borrow_mut();
        prefs.touch_session("acode-elsewhere", Some("older work"));
    }
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    h.turn();
    h.store
        .session_index
        .set(abstractcode::store::SessionIndex::Loaded {
            rows: vec![abstractcode::store::SessionRow {
                id: "acode-listed".into(),
                state: abstractcode::store::SessionState::Running,
                last_at: "2026-08-29T12:00:00Z".into(),
                turns: 2,
                first_run: "r1".into(),
                prompt: Some("a real prompt from the gateway".into()),
            }],
            truncated: true,
            labeled: 1,
        });
    let screen = h.turn();
    assert!(
        screen.contains("outside this listing"),
        "a session missing from a TRUNCATED page is not proven absent:\n{screen}"
    );
    assert!(
        !screen.contains("not on the gateway"),
        "…and must never be called absent:\n{screen}"
    );
    assert!(
        screen.contains("2+ turns"),
        "counts from a partial page are floors:\n{screen}"
    );
    assert!(
        screen.contains("a real prompt from"),
        "the gateway's own prompt labels the row:\n{screen}"
    );
}

/// Prompts come from the GATEWAY, the board names how many it fetched,
/// and turn counts read as exact only when the listing was complete.
///
/// The whole prompt feature had zero coverage: dropping the fetch,
/// setting its bound to zero, or making every count a floor all left
/// the suite green (adversarial review 2026-08-29, D6).
#[test]
fn the_board_labels_from_the_gateway_and_names_what_it_did_not_fetch() {
    let mut h = harness();
    h.turn();
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    h.turn();
    let row = |id: &str, prompt: Option<&str>, turns: usize| abstractcode::store::SessionRow {
        id: id.into(),
        state: abstractcode::store::SessionState::Done,
        last_at: "2026-08-29T09:00:00Z".into(),
        turns,
        first_run: "r".into(),
        prompt: prompt.map(str::to_string),
    };
    h.store
        .session_index
        .set(abstractcode::store::SessionIndex::Loaded {
            rows: vec![
                row("acode-labeled", Some("port the tests to rstest"), 4),
                row("acode-unlabeled", None, 2),
            ],
            truncated: false,
            labeled: 1,
        });
    let screen = h.turn();
    assert!(
        screen.contains("port the tests to"),
        "the gateway's own prompt labels the row:\n{screen}"
    );
    assert!(
        screen.contains("4 turns") && screen.contains("2 turns"),
        "a COMPLETE listing gives exact turn counts, not floors:\n{screen}"
    );
    assert!(
        !screen.contains("4+ turns"),
        "…and must not mark an exact count as a floor:\n{screen}"
    );
    // The unfetched remainder is NAMED: one glyph must not silently
    // mean "still arriving", "outside the bound" and "none at all".
    assert!(
        screen.contains("prompts: top 1, 1 unfetched"),
        "the prompt bound is stated:\n{screen}"
    );
}

/// A failed restore is an EPHEMERAL condition, not a transcript entry
/// (operator, 2026-08-31: "those kinds of ephemeral warnings/errors
/// would be better served as temporary modal that disappear if the
/// connexion is re-established").
///
/// It used to be an `Item::Error` card: permanent, carrying the full
/// request URL, outliving the reconnection that fixed it, and telling
/// the operator to `/sessions` and re-select by hand. Now it shows
/// while the condition holds, names no URL, and the reconnect RETRIES
/// the restore rather than delegating that to the human.
#[test]
fn a_failed_restore_is_transient_and_retried_on_reconnect() {
    let mut h = harness();
    h.turn();
    h.leave_splash();
    // The worker reports a connectivity-class failure (URL-free, as
    // `GwError::compact_reason()` produces).
    h.store
        .restore_failed
        .set(Some("gateway unreachable".into()));
    h.store.conn.set(abstractcode::store::Conn::Down(
        "gateway unreachable: …".into(),
        true,
    ));
    let screen = h.turn();
    assert!(
        screen.contains("session history not restored"),
        "the condition is visible while it holds:\n{screen}"
    );
    assert!(
        screen.contains("retrying when the gateway answers"),
        "…and promises the recovery this client actually performs:\n{screen}"
    );
    assert!(
        !screen.contains("http://") && !screen.contains("/runs?"),
        "no request URL on screen — a failure label carrying the \
         gateway's own endpoint is an instruction kit:\n{screen}"
    );
    assert!(
        !screen.contains("re-select"),
        "it must not ask the operator to do what the reconnect does:\n{screen}"
    );
    // It is NOT a transcript entry: nothing was written to the fold.
    assert!(
        h.store.fold.with_untracked(|f| !f
            .items
            .iter()
            .any(|i| matches!(i, abstractcode::transcript::Item::Error { .. }))),
        "an ephemeral fault never becomes a permanent card"
    );
    // The gateway comes back: the app retries the restore itself…
    h.store.conn.set(abstractcode::store::Conn::Ok);
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::ProbeAttach { .. }))
            .is_some(),
        "the reconnect retries the restore instead of asking the user to"
    );
    // …and the notice goes away with the condition.
    h.store.restore_failed.set(None);
    let screen = h.turn();
    assert!(
        !screen.contains("session history not restored"),
        "the notice disappears once the condition resolves:\n{screen}"
    );
}

/// Enter on the WAITING board does nothing — it must not switch
/// sessions, and it must not cancel the live run.
///
/// The waiting surface renders a 2-row placeholder while `on_choose`
/// read the MERGED list — two different index spaces — and the engine
/// clamps the activation index into the RENDERED rows, so Enter on a
/// screen showing only a spinner resolved to a real session, switched
/// to it, and `switch_session` cancelled the in-flight run on the way
/// out. Destructive, first-open reachable (adversarial review
/// 2026-08-29, D1).
#[test]
fn enter_on_the_waiting_board_cannot_switch_or_cancel() {
    let mut h = harness();
    h.turn();
    {
        let mut prefs = h.prefs.borrow_mut();
        // Ordered so the CURRENT session sorts LAST: rows 0 and 1 —
        // the only indices the 2-row placeholder can clamp to — are
        // both other sessions. With the current session at row 0,
        // `switch_session` early-returns on "same id" and the test
        // passes for the wrong reason (measured: the first cut did).
        prefs.touch_session("acode-test-session", Some("current work"));
        prefs.touch_session("acode-elsewhere", Some("older work"));
        prefs.touch_session("acode-yak", Some("yak shaving"));
    }
    // A run is in flight — the thing a stray switch would destroy.
    h.store.phase.set(abstractcode::store::Phase::Running);
    h.store.run_id.set("run-live".into());
    h.turn();
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("asking the gateway"),
        "premise: the board is waiting:\n{screen}"
    );
    // Every activation gesture the List offers, on the placeholder.
    h.press_enter();
    h.turn();
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    h.turn();
    assert_eq!(
        h.store.session_id.get_untracked(),
        "acode-test-session",
        "no session switch from a screen with nothing selectable"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Cancel { .. })).is_none(),
        "and above all: the live run was NOT cancelled"
    );
    assert_eq!(
        h.store.phase.get_untracked(),
        abstractcode::store::Phase::Running,
        "the run is still running"
    );
}

/// `r` re-asks the gateway. The title promises it, so it must be
/// bound — removing the binding left the whole suite green while the
/// title still advertised it (adversarial review 2026-08-29).
#[test]
fn r_refreshes_the_session_board() {
    let mut h = harness();
    h.turn();
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    h.turn();
    // Drain the open-time fetch.
    assert!(h
        .find_cmd(|c| matches!(c, Cmd::LoadSessions { .. }))
        .is_some());
    h.type_text("r");
    h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadSessions { .. }))
            .is_some(),
        "`r` re-asks — the key the title promises is bound"
    );
}

/// The board says when its list is a PAGE, not the whole gateway.
/// `has_more` reached the store and then died on screen: the hint
/// sentence was already ~85 columns against an ~87-column budget, so
/// the truncation clause could not render at ANY width — silent
/// truncation of the truncation notice (adversarial review, D5).
#[test]
fn a_truncated_session_listing_says_so_on_screen() {
    let mut h = harness();
    h.turn();
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    h.turn();
    h.store
        .session_index
        .set(abstractcode::store::SessionIndex::Loaded {
            rows: vec![abstractcode::store::SessionRow {
                id: "acode-a".into(),
                state: abstractcode::store::SessionState::Done,
                last_at: "2026-08-29T09:00:00Z".into(),
                turns: 1,
                first_run: String::new(),
                prompt: None,
            }],
            truncated: true,
            labeled: 1,
        });
    let screen = h.turn();
    assert!(
        screen.contains("+older runs"),
        "the page boundary is VISIBLE, not just modelled:\n{screen}"
    );
    assert!(
        screen.contains("counts are floors"),
        "…and the turn counts say they are page-bounded:\n{screen}"
    );
}

/// A gateway that will not answer leaves the board honest: local rows
/// render, the failure is named, and no row claims a live state.
#[test]
fn a_failed_session_fetch_says_so_instead_of_showing_an_empty_gateway() {
    let mut h = harness();
    h.turn();
    {
        let mut prefs = h.prefs.borrow_mut();
        prefs.touch_session("acode-remembered", Some("older work"));
    }
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    h.turn();
    h.store
        .session_index
        .set(abstractcode::store::SessionIndex::Failed(
            "gateway unreachable: connection refused".into(),
        ));
    let screen = h.turn();
    assert!(
        screen.contains("gateway did not answer"),
        "the refusal is named:\n{screen}"
    );
    assert!(
        screen.contains("older work"),
        "remembered sessions still render:\n{screen}"
    );
    assert!(
        screen.contains("state unknown"),
        "…marked as UNKNOWN — a fetch that never answered proves nothing \
         about whether the session is there:\n{screen}"
    );
}

#[test]
fn header_names_what_gateway_defaults_resolves_to() {
    let mut h = harness();
    h.turn();
    // Before any resolution: the bare label.
    let screen = h.turn();
    assert!(screen.contains("gateway defaults"), "bare label:\n{screen}");
    // The capability route arrives -> the header names it.
    h.store
        .default_route
        .set(("lmstudio".into(), "ornith-1.0-35b".into()));
    let screen = h.turn();
    assert!(
        screen.contains("gateway defaults (lmstudio · ornith-1.0-35b)"),
        "resolved route:\n{screen}"
    );
    // A run's llm_call names the model that ACTUALLY served -> wins, in
    // the SAME `provider · model` format (the provider vanishing after
    // the first run read as data loss — adversary P3, 2026-07-22).
    // (On the ANSWER LANE — delegate children never relabel the header.)
    h.store.fold.update(|f| {
        f.begin_run("root");
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "hi", "model": "ornith-2",
                        "usage": {"input_tokens": 1500, "output_tokens": 20}}
        });
        let _ = f.apply("root", &rec);
    });
    let screen = h.turn();
    assert!(
        screen.contains("gateway defaults (lmstudio · ornith-2)"),
        "served model wins, provider kept:\n{screen}"
    );
}

#[test]
fn help_modal_opens_and_closes() {
    let mut h = harness();
    h.turn();
    h.type_text("/help");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("/sessions [id]"),
        "help modal lists commands:\n{screen}"
    );
    h.press_escape();
    h.turn(); // deferred close lands
    let screen = h.turn();
    assert!(
        !screen.contains("/sessions [id]"),
        "help closed on Esc:\n{screen}"
    );
}

/// Feed order = fold order across a MID-LIST visibility flip (the sync
/// contract's rebuild seam). Thinking and tools are now ALWAYS visible
/// (operator directives 2026-07-26 and 2026-08-19), so the details-
/// gated element that still flips mid-list is a PROBE body: hidden in
/// the collapsed view, it appears BETWEEN its neighbors when details
/// turn on — never at the feed tail (feed order is push order; a
/// tail-appended key would misplace it).
#[test]
fn feed_order_survives_mid_list_visibility_flips() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.show_details.set(false); // collapsed view: probe bodies hidden
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "AAA-question".into(),
        });
        // A probe body between the two visible items: hidden now.
        f.push_item(abstractcode::transcript::Item::Probe {
            title: "zz_marker_probe".into(),
            body: "probe body lines".into(),
        });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "BBB-update".into(),
            final_answer: false,
        });
    });
    h.turn();
    h.turn();
    let screen = h.turn();
    let pos = |s: &str, needle: &str| s.find(needle).unwrap_or(usize::MAX);
    assert!(
        !screen.contains("zz_marker_probe"),
        "probe body hidden in the collapsed view:\n{screen}"
    );
    assert!(
        pos(&screen, "AAA-question") < pos(&screen, "BBB-update"),
        "initial order user < update (probe folded):\n{screen}"
    );

    // Details ON: the probe body flips visible mid-list — it must
    // land BETWEEN its neighbors, never at the feed tail.
    h.term.push_input(&[0x04]); // Ctrl+D
    let screen = h.turn();
    assert!(
        pos(&screen, "AAA-question") < pos(&screen, "zz_marker_probe")
            && pos(&screen, "zz_marker_probe") < pos(&screen, "BBB-update"),
        "restored order user < probe < update:\n{screen}"
    );

    // Details OFF again: back to the folded order, rest intact.
    h.term.push_input(&[0x04]);
    let screen = h.turn();
    assert!(
        !screen.contains("zz_marker_probe")
            && pos(&screen, "AAA-question") < pos(&screen, "BBB-update"),
        "re-folded, order intact:\n{screen}"
    );
}

/// Truncation drains through the index-keyed feed sync, past TWO drains
/// (chunked hysteresis: items float in [MAX_ITEMS, MAX_ITEMS +
/// TRUNCATE_CHUNK]; each drain cuts back to MAX_ITEMS). The drain
/// arithmetic means `len < seen.len()` does NOT hold for every drain:
/// a batch observed once may drain AND refill to the same length —
/// the sync must then re-render every shifted index in place (keys are
/// positions, not item identities), which is order-correct without a
/// rebuild. A drain observed as a shrink takes the rebuild path. Both
/// paths must leave the rendered feed matching the fold in order:
/// notice first, oldest survivor next, newest at the tail, dropped
/// items gone.
#[test]
fn truncation_drains_keep_the_feed_in_sync_with_fold_order() {
    use abstractcode::transcript::{Item, MAX_ITEMS, TRUNCATE_CHUNK};
    let mut h = harness();
    h.turn();
    let store = h.store;
    let user = |i: usize| Item::User {
        text: format!("item-{i:04}"),
    };
    // Phase A: exactly MAX_ITEMS items, observed — the sync's seen list
    // is full-length with no drain yet.
    store.fold.update(|f| {
        for i in 0..MAX_ITEMS {
            f.push_item(user(i));
        }
    });
    // Three pumps: width discovery at first draw, the measured-extent
    // sync one frame later (engine contract), and one more deferred
    // geometry round from the feed's gap flip (the pane runs gap 0;
    // FeedState boots at the engine default of 1).
    h.turn();
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains(&format!("item-{:04}", MAX_ITEMS - 1)),
        "follow-tail shows the newest pre-drain item:\n{screen}"
    );

    // Phase B: ONE observed batch of TRUNCATE_CHUNK+1 pushes crosses the
    // ceiling: the FIRST drain inserts the notice at index 0 and cuts
    // back to MAX_ITEMS — the fold length lands EQUAL to seen.len(), so
    // the length check does NOT fire and the sync must correct every
    // shifted index through in-place keyed re-renders.
    store.fold.update(|f| {
        for i in MAX_ITEMS..(MAX_ITEMS + TRUNCATE_CHUNK + 1) {
            f.push_item(user(i));
        }
    });
    assert_eq!(
        store.fold.with_untracked(|f| f.items.len()),
        MAX_ITEMS,
        "each drain cuts back to MAX_ITEMS"
    );
    let oldest_survivor = store.fold.with_untracked(|f| match &f.items[1] {
        Item::User { text } => text.clone(),
        other => panic!("expected the oldest surviving user card, got {other:?}"),
    });
    assert_eq!(oldest_survivor, format!("item-{:04}", TRUNCATE_CHUNK + 2));
    let screen = h.turn();
    assert!(
        screen.contains(&format!("item-{:04}", MAX_ITEMS + TRUNCATE_CHUNK)),
        "tail shows the newest item after the equal-length drain:\n{screen}"
    );
    // Scroll to the very top: the notice renders first, then the oldest
    // SURVIVOR — never a dropped item under a stale key. 13 batches:
    // body-carrying cards are 3 rows since the rich-header adoption
    // (header · blank · body — the engine's block rhythm, matching the
    // assistant card's long-standing shape), so 500 items ≈ 2000 rows;
    // 13×20 PageUps × 10 rows covers it with margin.
    for _ in 0..13 {
        let presses: Vec<u8> = b"\x1b[5~".repeat(20);
        h.term.push_input(&presses);
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("#TRUNCATION"),
        "the standing notice renders at the feed top:\n{screen}"
    );
    assert!(
        screen.contains(&oldest_survivor),
        "the oldest survivor renders right under the notice:\n{screen}"
    );
    assert!(
        !screen.contains("item-0000")
            && !screen.contains(&format!("item-{:04}", TRUNCATE_CHUNK + 1)),
        "dropped items are gone from the rendered top:\n{screen}"
    );

    // Phase C: back to the tail, then a SECOND drain observed as a
    // shrink (grow first, observe, then cross the ceiling): the length
    // check fires and the rebuild path re-syncs the whole window.
    h.press_escape(); // re-arm follow (empty composer)
    h.turn();
    store.fold.update(|f| {
        for i in (MAX_ITEMS + TRUNCATE_CHUNK + 1)..(MAX_ITEMS + 2 * TRUNCATE_CHUNK) {
            f.push_item(user(i));
        }
    });
    let screen = h.turn();
    assert!(
        screen.contains(&format!("item-{:04}", MAX_ITEMS + 2 * TRUNCATE_CHUNK - 1)),
        "intermediate growth observed (seen advances past MAX_ITEMS):\n{screen}"
    );
    store.fold.update(|f| {
        for i in (MAX_ITEMS + 2 * TRUNCATE_CHUNK)..(MAX_ITEMS + 2 * TRUNCATE_CHUNK + 2) {
            f.push_item(user(i));
        }
    });
    assert_eq!(
        store.fold.with_untracked(|f| f.items.len()),
        MAX_ITEMS,
        "the second drain also cuts back to MAX_ITEMS"
    );
    let oldest_survivor = store.fold.with_untracked(|f| match &f.items[1] {
        Item::User { text } => text.clone(),
        other => panic!("expected the oldest surviving user card, got {other:?}"),
    });
    let screen = h.turn();
    assert!(
        screen.contains(&format!("item-{:04}", MAX_ITEMS + 2 * TRUNCATE_CHUNK + 1)),
        "tail shows the newest item after the shrink-observed drain:\n{screen}"
    );
    // 13 batches: 3-row cards since the rich-header adoption (see the
    // phase-B scroll above).
    for _ in 0..13 {
        let presses: Vec<u8> = b"\x1b[5~".repeat(20);
        h.term.push_input(&presses);
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("#TRUNCATION") && screen.contains(&oldest_survivor),
        "after the rebuild drain the top is the notice + oldest survivor:\n{screen}"
    );
}

/// A feed SHRINK while the user reads scrollback (details toggled off,
/// session switched) must never strand the scroll offset beyond the new
/// content extent — the pane rendered NOTHING and nothing re-clamped it
/// (review finding: offset 60+ over a 6-row feed = blank transcript
/// until a wheel/Esc rescue the user has no reason to know about).
#[test]
fn details_shrink_while_scrolled_up_never_blanks_the_pane() {
    let mut h = harness();
    h.turn();
    // Details ON: long thinking bodies render in full — the collapse
    // to 4-row gists is the feed shrink under test (thinking is always
    // visible now, so the shrink driver is VERBOSITY, not existence).
    h.store.show_details.set(true);
    let long_content = (0..15)
        .map(|j| format!("ponder step line {j}"))
        .collect::<Vec<_>>()
        .join("\n");
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "FIRST-QUESTION".into(),
        });
        for i in 0..40 {
            f.push_item(abstractcode::transcript::Item::Thinking {
                iteration: i + 1,
                content: long_content.clone(),
                reasoning: String::new(),
                call: abstractcode::transcript::CallCost::default(),
            });
        }
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "THE-FINAL-ANSWER".into(),
            final_answer: true,
        });
    });
    // Three pumps: width discovery, extent sync, gap-flip geometry
    // round (see truncation_drains… for the settle contract).
    h.turn();
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("THE-FINAL-ANSWER"),
        "follow-tail pins to the answer:\n{screen}"
    );
    // Page up into the scrollback (disengages follow).
    for _ in 0..4 {
        h.term.push_input(b"\x1b[5~");
        h.turn();
    }
    let screen = h.turn();
    assert!(
        !screen.contains("THE-FINAL-ANSWER"),
        "scrolled away from the tail:\n{screen}"
    );
    assert!(
        screen.contains("ponder step"),
        "reading mid-transcript:\n{screen}"
    );
    // Ctrl+D: all 40 thinking cards collapse to gists — the feed
    // shrinks far below the stranded offset.
    h.term.push_input(&[0x04]);
    h.turn();
    h.turn();
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("THE-FINAL-ANSWER")
            || screen.contains("FIRST-QUESTION")
            || screen.contains("ponder step"),
        "the pane must keep showing content after the shrink, not go blank:\n{screen}"
    );
}

/// Session switch from the /sessions picker replaces the fold with a
/// one-line transcript; a scrolled-up offset from the OLD session must
/// not leave the new one invisible — including once restored history
/// lands and the feed remounts far smaller than the stranded offset.
#[test]
fn session_switch_while_scrolled_up_shows_the_new_transcript() {
    let mut h = harness();
    h.turn();
    {
        let mut prefs = h.prefs.borrow_mut();
        prefs.touch_session("acode-other-session", Some("older work"));
        prefs.touch_session("acode-test-session", Some("current work"));
    }
    // Non-Info filler, deliberately: an Info-only fold takes the pane's
    // EMPTY-STATE branch (guidance + centered notices — no scroll), so
    // Info filler would never mount the feed this test means to scroll
    // (review correction, 2026-07-22).
    h.store.fold.update(|f| {
        for i in 0..60 {
            f.push_item(abstractcode::transcript::Item::User {
                text: format!("filler line {i}"),
            });
        }
    });
    h.turn();
    h.turn();
    for _ in 0..4 {
        h.term.push_input(b"\x1b[5~");
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("filler line") && !screen.contains("filler line 59"),
        "reading scrollback above the tail:\n{screen}"
    );
    h.type_text("/sessions");
    h.turn();
    h.press_enter();
    h.turn();
    settle_session_board(&mut h);
    h.term.push_input(b"\x1b[B"); // Down -> the other session
    h.turn();
    h.press_enter();
    h.turn();
    h.turn(); // deferred modal close
              // The pick arms the session-loading screen (`ui::loading`); this
              // harness has no worker to run the probe, so complete it by hand —
              // an empty session restores nothing and clears the flag.
    h.store.restoring.set(false);
    h.turn();
    let screen = h.turn();
    assert_eq!(h.store.session_id.get_untracked(), "acode-other-session");
    assert!(
        screen.contains("session switched"),
        "the new session's transcript is visible, not stranded off-screen:\n{screen}"
    );
    assert!(
        screen.contains("describe a task"),
        "an Info-only fold (fresh session) returns the pane to the guidance view:\n{screen}"
    );
    // Restored history lands (the ProbeAttach rehydration path): the
    // feed remounts with ~5 rows of content under a ~150-row stranded
    // offset — the shrink clamp must snap back into content instead of
    // leaving a blank pane (follow is still disengaged from the PageUps).
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "RESTORED-TURN".into(),
        });
    });
    h.turn();
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("RESTORED-TURN"),
        "restored content is visible after the shrink, not stranded off-screen:\n{screen}"
    );
}

/// Assistant ```diff fences tint through the ENGINE (0.2.1: Feed
/// markdown fences route fence labels to `text::DiffLexer`; no app
/// code). Proven at the pixel level: added/removed lines carry the
/// theme's ok/error inks and context lines the body ink — read back
/// from the modeled VT screen, not from text. The needles are ASCII,
/// so screen columns map 1:1 to `to_text` character positions.
#[test]
fn assistant_diff_fences_tint_added_and_removed_lines() {
    let mut h = harness();
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "Patch:\n\n```diff\n+added-marker line\n-removed-marker line\n unchanged-marker line\n```\n"
                .into(),
            final_answer: true,
        });
    });
    // Two pumps: the feed discovers its width at draw and typesets on
    // the following frame (engine geometry contract).
    h.turn();
    let screen_text = h.turn();
    assert!(
        screen_text.contains("+added-marker line"),
        "diff fence rendered:\n{screen_text}"
    );

    // Ink of the needle's first cell on the modeled screen.
    let ink_of = |h: &Harness, needle: &str| -> abstracttui::prelude::Rgba {
        let screen = h.term.screen();
        let size = screen.size();
        for y in 0..size.h {
            let row: String = (0..size.w)
                .map(|x| {
                    screen
                        .cell(x, y)
                        .map(|c| c.ch())
                        .filter(|ch| *ch != '\0')
                        .unwrap_or(' ')
                })
                .collect();
            if let Some(col) = row.find(needle) {
                let cell = screen.cell(col as i32, y).expect("cell in range");
                return cell.paint.fg.unwrap_or_else(|| {
                    panic!("needle {needle:?} has no explicit ink at {col},{y}")
                });
            }
        }
        panic!("needle {needle:?} not on the modeled screen:\n{screen_text}");
    };

    let t = abstracttui::app::current_theme().tokens;
    let added = ink_of(&h, "+added-marker");
    let removed = ink_of(&h, "-removed-marker");
    let context = ink_of(&h, "unchanged-marker");
    assert_eq!(added, t.ok, "added lines wear the theme's ok ink");
    assert_eq!(removed, t.error, "removed lines wear the theme's error ink");
    assert_eq!(context, t.text, "context lines keep the body ink");
    assert_ne!(added, context, "added ink differs from context");
    assert_ne!(removed, context, "removed ink differs from context");
    assert_ne!(added, removed, "added and removed inks differ");
}

/// The '/' completion must stay closed inside a command draft's ARGUMENTS:
/// "/steer fix /s" is a fully-typed command whose argument happens to
/// contain a slash token — Enter must SUBMIT it, not rewrite the argument
/// with a completion (review finding: the draft became "/steer fix /skills ").
#[test]
fn completion_never_opens_inside_command_arguments() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| {
        f.begin_run("root");
        let rec = serde_json::json!({"run_id": "sub9", "node_id": "reason", "status": "started",
                                      "effect": {"type": "llm_call", "payload": {}}});
        let _ = f.apply("sub9", &rec);
    });
    h.turn();
    h.type_text("/steer fix /s");
    h.turn();
    let screen = h.turn();
    assert!(
        !screen.contains("pick a recent session"),
        "no completion dropdown inside command arguments:\n{screen}"
    );
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Steer { .. })) {
        Some(Cmd::Steer { text, .. }) => assert_eq!(text, "fix /s"),
        other => panic!(
            "Enter must submit the steer, got {:?}",
            other.map(|_| "cmd")
        ),
    }
}

/// The '/' completion dropdown: a partial command offers candidates; a
/// fully-typed command closes the dropdown so the FIRST Enter submits
/// (the dropdown intercepts Enter while open — engine contract); and a
/// prompt merely MENTIONING a "/token" mid-sentence never completes.
#[test]
fn slash_completion_offers_partials_and_never_hijacks_prompts() {
    let mut h = harness();
    h.turn();

    // Partial command -> dropdown with the candidate's hint line.
    h.type_text("/he");
    let screen = h.turn();
    assert!(
        screen.contains("commands + keys"),
        "dropdown offers /help for the partial:\n{screen}"
    );

    // Completing the command by TYPING closes the dropdown (exact match
    // stands down), and Enter opens help in ONE keystroke.
    h.type_text("lp");
    let screen = h.turn();
    assert!(
        !screen.contains("commands + keys"),
        "dropdown stands down on the fully-typed command:\n{screen}"
    );
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("/sessions [id]"),
        "first Enter submitted the command (help open):\n{screen}"
    );
    h.press_escape();
    h.turn();
    h.turn();

    // A prompt mentioning a slash token mid-sentence: no dropdown, and
    // Enter SUBMITS the prompt (a run starts with the full text).
    h.type_text("explain /he");
    let screen = h.turn();
    assert!(
        !screen.contains("commands + keys"),
        "no dropdown for a mid-prompt /token:\n{screen}"
    );
    h.press_enter();
    h.turn();
    match h.rx.try_recv() {
        Ok(Cmd::Start { prompt, .. }) => assert_eq!(prompt, "explain /he"),
        Ok(_) => panic!("expected a Start for the prompt, got another command"),
        Err(e) => panic!("expected a Start for the prompt, got {e:?}"),
    }
}

// ---------------------------------------------------------------------------
// Tool-approval tier policy (bug (a), 2026-07-22): persisted gradation —
// batches at-or-below the accepted tier resume without a prompt.
// ---------------------------------------------------------------------------

fn approval_record(step: &str, key: &str, calls: serde_json::Value) -> Value {
    serde_json::json!({
        "run_id": "root", "node_id": "act", "status": "waiting", "step_id": step,
        "effect": {"type": "tool_calls", "payload": {"tool_calls": calls}},
        "result": {"wait": {"reason": "user", "wait_key": key,
            "details": {"mode": "approval_required", "tool_calls": calls}}}
    })
}

#[test]
fn tier_policy_auto_approves_at_or_below_and_prompts_above() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.accepted_tier.set("write".into());

    // A write-tier batch under an accepted "write" tier: NO modal, the
    // wait resumes approved.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s1",
                "tool_approval:k1",
                serde_json::json!([{"name": "write_file",
                                    "arguments": {"path": "a.rs", "content": "x"}}]),
            ),
        );
    });
    let screen = h.turn();
    assert!(
        !screen.contains("approve (a)"),
        "no prompt for an at-tier batch:\n{screen}"
    );
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume {
            wait_key, approved, ..
        }) => {
            assert_eq!(wait_key, "tool_approval:k1");
            assert_eq!(approved, Some(true));
        }
        other => panic!("expected tier auto Resume, got {:?}", other.map(|_| "cmd")),
    }
    // (The old session-blanket signal is deleted — the permissions level
    // is the ONE admission; nothing else to assert here.)

    // An above-tier batch (shell) STILL prompts, and the modal names both
    // sides of the decision.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s2",
                "tool_approval:k2",
                serde_json::json!([{"name": "execute_command",
                                    "arguments": {"command": "cargo build", "cwd": "/tmp/proj"}}]),
            ),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "above-tier batch prompts:\n{screen}"
    );
    assert!(
        screen.contains("permissions: write") && screen.contains("needs: all"),
        "the modal names accepted vs needed level:\n{screen}"
    );
}

#[test]
fn readonly_batches_auto_approve_at_the_default_read_tier() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    // No level configured: "" reads as the strictest ("read") — read-only
    // batches still flow without a prompt. (The former proven-git member
    // of this batch left with the retired client proof, c5057: git
    // approval is the runtime refiner's job now, server-side.)
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s1",
                "tool_approval:k1",
                serde_json::json!([
                    {"name": "read_file", "arguments": {"path": "a.rs"}},
                    {"name": "list_files", "arguments": {"path": "."}}
                ]),
            ),
        );
    });
    let screen = h.turn();
    assert!(
        !screen.contains("approve (a)"),
        "read-only batch auto-approves at read level:\n{screen}"
    );
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume { approved, .. }) => assert_eq!(approved, Some(true)),
        other => panic!("expected auto Resume, got {:?}", other.map(|_| "cmd")),
    }

    // The same batch with a WRITE call mixed in prompts (one above-tier
    // call prompts the whole batch).
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s2",
                "tool_approval:k2",
                serde_json::json!([
                    {"name": "read_file", "arguments": {"path": "a.rs"}},
                    {"name": "write_file", "arguments": {"path": "b.rs", "content": "y"}}
                ]),
            ),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "mixed batch with a write prompts at read tier:\n{screen}"
    );
}

/// facts #1: the START payload carries the server-side tool policy the
/// runtime honors with NO wait round-trip — expanded from the accepted
/// tier over the live inventory, with per-tool pins riding both ways.
#[test]
fn start_carries_server_side_tool_policy_expanded_from_the_tier() {
    let mut h = harness();
    h.turn();
    h.store.tools.set(vec![
        abstractcode::store::ToolInfo {
            name: "read_file".into(),
            description: "Read".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "write_file".into(),
            description: "Write".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "execute_command".into(),
            description: "Shell".into(),
            toolset: "system".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "fetch_url".into(),
            description: "Fetch".into(),
            toolset: "web".into(),
            ..Default::default()
        },
    ]);
    // write tier + an auto pin on fetch_url + an ask pin on read_file.
    h.store.accepted_tier.set("write".into());
    h.store.tool_overrides.set(vec![
        ("fetch_url".into(), "auto".into()),
        ("read_file".into(), "ask".into()),
    ]);

    h.type_text("build it");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            let auto = &opts.tool_policy.auto_approve_tools;
            // write_file rides by tier; fetch_url by the auto pin.
            assert!(auto.contains(&"write_file".to_string()), "{auto:?}");
            assert!(auto.contains(&"fetch_url".to_string()), "{auto:?}");
            // read_file is ask-pinned: excluded from auto, force-asked.
            assert!(!auto.contains(&"read_file".to_string()), "{auto:?}");
            // execute_command needs tier all (no per-call args at start).
            assert!(!auto.contains(&"execute_command".to_string()), "{auto:?}");
            assert_eq!(
                opts.tool_policy.require_approval_tools,
                vec!["read_file".to_string()]
            );
        }
        other => panic!("expected Start, got {:?}", other.map(|_| "cmd")),
    }
}

/// The empty-inventory case (facts #1): no inventory loaded yet → no
/// server-side policy rides the start (the client-side belt still gates).
#[test]
fn start_sends_no_tool_policy_when_inventory_is_empty() {
    let mut h = harness();
    h.turn();
    h.store.accepted_tier.set("all".into());
    // tools signal is empty by default.
    h.type_text("go");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            assert!(
                opts.tool_policy.is_empty(),
                "empty inventory sends no policy: {:?}",
                opts.tool_policy
            );
        }
        other => panic!("expected Start, got {:?}", other.map(|_| "cmd")),
    }
}

#[test]
fn tools_tier_command_sets_persists_and_refuses_garbage() {
    let mut h = harness();
    h.turn();
    h.type_text("/tools tier write");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.accepted_tier.get_untracked(), "write");
    assert_eq!(h.prefs.borrow().tool_accepted_tier, "write");

    // Unknown spellings refuse loudly and change nothing.
    h.type_text("/tools tier yolo");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.accepted_tier.get_untracked(), "write");
    assert_eq!(h.prefs.borrow().tool_accepted_tier, "write");
}

#[test]
fn raising_the_tier_resolves_an_open_approval_prompt() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s1",
                "tool_approval:k1",
                serde_json::json!([{"name": "write_file",
                                    "arguments": {"path": "a.rs", "content": "x"}}]),
            ),
        );
    });
    let screen = h.turn();
    assert!(screen.contains("approve (a)"), "prompt up:\n{screen}");

    // The user raises the accepted tier (e.g. via /tools tier from a
    // deferred prompt, or the tools modal's `t`): the pending wait
    // re-decides immediately — nothing at-or-below the tier ever asks.
    store.accepted_tier.set("write".into());
    h.turn();
    let screen = h.turn();
    assert!(
        !screen.contains("approve (a)"),
        "prompt resolved by the tier change:\n{screen}"
    );
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume { approved, .. }) => assert_eq!(approved, Some(true)),
        other => panic!("expected Resume, got {:?}", other.map(|_| "cmd")),
    }
}

// ---------------------------------------------------------------------------
// Approval modal rendering (bug (b), 2026-07-22): human-readable cards.
// ---------------------------------------------------------------------------

#[test]
fn approval_modal_renders_readable_cards_with_command_first_class() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s1",
                "tool_approval:k1",
                serde_json::json!([
                    {"name": "execute_command",
                     "arguments": {"command": "cargo test --lib", "cwd": "/tmp/proj"}},
                    {"name": "write_file",
                     "arguments": {"path": "src/a.rs", "content": "fn a() {}\n// two\n// three"}}
                ]),
            ),
        );
    });
    let screen = h.turn();
    // The command string is the thing being approved: first-class.
    assert!(
        screen.contains("$ cargo test --lib"),
        "command shown first-class:\n{screen}"
    );
    // Params as key/value rows (values unquoted), not a JSON dump. The
    // negative is scoped to PRETTY-JSON spacing (`"key": `): the
    // transcript's compact args preview behind the modal legitimately
    // contains `"cwd":"…"` — only the modal's f-mode prints pretty JSON.
    assert!(screen.contains("cwd") && screen.contains("/tmp/proj"));
    assert!(
        !screen.contains("\"cwd\": \""),
        "default view is NOT pretty JSON:\n{screen}"
    );
    // Batches of 2+ get per-call separation + intent summaries.
    assert!(
        screen.contains("call 1/2") && screen.contains("call 2/2"),
        "per-call separators:\n{screen}"
    );
    assert!(
        screen.contains("write src/a.rs"),
        "write_file intent summary:\n{screen}"
    );
    // Multi-line content is honest about what it hides.
    assert!(
        screen.contains("(+2 more lines)"),
        "multiline marker:\n{screen}"
    );
    // The truncation note points at the CLIENT surface (`f`), never at
    // the ledger (operator ruling 2026-07-26).
    assert!(
        screen.contains("values shortened — f shows the full JSON"),
        "shortened note names the f toggle:\n{screen}"
    );
    assert!(!screen.contains("ledger"), "no ledger pointer:\n{screen}");

    // `f` flips to the full JSON (and back).
    h.type_text("f");
    let screen = h.turn();
    assert!(
        screen.contains("\"command\": \"cargo test --lib\""),
        "full JSON behind f:\n{screen}"
    );
    h.type_text("f");
    let screen = h.turn();
    assert!(
        screen.contains("$ cargo test --lib"),
        "f toggles back to cards:\n{screen}"
    );

    // The keys still work over the new body: approve resumes.
    h.type_text("a");
    h.turn();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume { approved, .. }) => assert_eq!(approved, Some(true)),
        other => panic!("expected Resume, got {:?}", other.map(|_| "cmd")),
    }
}

/// The standing rulings around UNRECOGNIZED tool names (no gateway class,
/// no client table entry — the fabricated `browser_probe` tell) under the
/// consolidated permissions level (c5028; the /auto blanket whose
/// unrecognized clamp this test used to pin is DELETED — the clamp had
/// no lane left to gate):
/// - below `all`, an unrecognized name classifies All (fail closed) and
///   PROMPTS — never a silent auto-approval;
/// - at `all`, it DOES auto-approve (the 2026-07-22 maintainer ruling:
///   "nothing is ever asked at the top tier" — a deliberate, disclosed
///   choice, not a blind blanket).
#[test]
fn unrecognized_tool_prompts_below_all_and_clears_at_all() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.accepted_tier.set("write".into());
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s1",
                "tool_approval:k1",
                serde_json::json!([{"name": "browser_probe", "arguments": {"target": "x"}}]),
            ),
        );
    });
    let screen = h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Resume { .. })).is_none(),
        "an unrecognized tool never auto-resumes below `all`"
    );
    assert!(
        screen.contains("tool approval"),
        "the prompt surfaces instead:\n{screen}"
    );

    // Raising to `all` re-decides the OPEN prompt immediately (tracked
    // read) and the unrecognized call auto-approves — the ruled
    // deliberate choice.
    store.accepted_tier.set("all".into());
    h.turn();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume { approved, .. }) => assert_eq!(approved, Some(true)),
        other => panic!(
            "at `all` the unrecognized call clears, got {:?}",
            other.map(|_| "cmd")
        ),
    }
}

/// Locate the first screen cell where `needle` starts: (row, col),
/// 0-based cells. Cols count CHARS (one cell per char on these ASCII
/// screens) so the result can feed 1-based SGR mouse coordinates.
fn locate(screen: &str, needle: &str) -> Option<(usize, usize)> {
    for (row, line) in screen.lines().enumerate() {
        if let Some(byte_col) = line.find(needle) {
            return Some((row, line[..byte_col].chars().count()));
        }
    }
    None
}

/// Shift+A ("approve all") has TWO wire spellings and both must fire
/// (live P0, 2026-07-23): legacy terminals bake the shift into the char
/// (byte 0x41 → Char('A'), no mods) — the spelling the original chord
/// matched — while kitty-protocol terminals report the BASE key identity
/// plus the modifier (Char('a') + SHIFT; the engine keeps identity 'a'
/// even when the shifted alternate is reported). On kitty wires the
/// chord was a DEAD KEY: the user pressed Shift+A, nothing fired, and
/// every later batch prompted again ("why does it keep asking"). This
/// drives the kitty spelling end-to-end: approve-all fires, permissions
/// set to `all` (c5028), and the NEXT batch auto-resumes without a
/// prompt.
#[test]
fn approve_all_fires_on_the_kitty_shift_a_spelling_and_covers_the_next_batch() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s1",
                "tool_approval:k1",
                serde_json::json!([{"name": "write_file", "arguments": {"path": "a.txt"}}]),
            ),
        );
    });
    let screen = h.turn();
    assert!(screen.contains("tool approval"), "prompt up:\n{screen}");

    // The kitty keyboard protocol spelling of Shift+A: CSI 97;2 u
    // (unicode 'a', mods 2 = shift).
    h.term.push_input(b"\x1b[97;2u");
    h.turn();
    h.turn();
    assert_eq!(
        store.accepted_tier.get_untracked(),
        "all",
        "approve-all sets the persisted permissions level (c5028)"
    );
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume { approved, .. }) => assert_eq!(approved, Some(true)),
        other => panic!("expected Resume, got {:?}", other.map(|_| "cmd")),
    }

    // The user's actual complaint: the NEXT batch must not prompt.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s2",
                "tool_approval:k2",
                serde_json::json!([{"name": "execute_command", "arguments": {"command": "ls"}}]),
            ),
        );
    });
    let screen = h.turn();
    h.turn();
    assert!(
        !h.ctx.modal_open(),
        "second batch auto-resumes without a prompt:\n{screen}"
    );
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume {
            approved, wait_key, ..
        }) => {
            assert_eq!(approved, Some(true));
            assert_eq!(wait_key, "tool_approval:k2");
        }
        other => panic!("expected second Resume, got {:?}", other.map(|_| "cmd")),
    }
}

/// Buttons must be CLICKABLE while a modal is open WITH select mode on
/// (live P0, 2026-07-23: approve / approve all / deny ignored the
/// mouse). Root cause was the engine's screen-text selection layer
/// owning every left Down/Up ahead of overlay routing — filed as
/// first-app/0285 and FIXED at the engine in abstracttui 0.2.8: the
/// layer claims a gesture only once it DRAGS, so a plain click passes
/// through to the button. This app's interim workaround (open_modal
/// disabled select mode, close_modal re-enabled) is deleted; the test
/// now pins the engine truth end-to-end: select mode STAYS ENABLED
/// while the modal is up and a real SGR left click still fires the
/// approve button through the enabled layer.
#[test]
fn approval_buttons_are_clickable_with_select_mode_on() {
    let mut h = harness();
    h.turn();
    // Boot behavior: select mode ON (production arms it in run_tui,
    // which the harness bypasses — arm it the same way here).
    abstracttui::app::selection::selection().set_enabled(true);
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s1",
                "tool_approval:k1",
                serde_json::json!([{"name": "write_file", "arguments": {"path": "a.txt"}}]),
            ),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "prompt with buttons:\n{screen}"
    );
    assert!(
        abstracttui::app::selection::selection().enabled(),
        "select mode STAYS enabled while a modal is open (0.2.8 click-through)"
    );

    // A real SGR left click (press + release) on the approve button's
    // label cells. SGR coordinates are 1-based.
    let (row, col) = locate(&screen, "approve (a)").expect("approve button on screen");
    let (x, y) = (col + 3 + 1, row + 1);
    h.term.push_input(format!("\x1b[<0;{x};{y}M").as_bytes());
    h.turn();
    h.term.push_input(format!("\x1b[<0;{x};{y}m").as_bytes());
    h.turn();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume { approved, .. }) => assert_eq!(approved, Some(true)),
        other => panic!(
            "expected Resume from the CLICK, got {:?}",
            other.map(|_| "cmd")
        ),
    }
    assert!(!h.ctx.modal_open(), "modal closed by the click");
    assert!(
        abstracttui::app::selection::selection().enabled(),
        "select mode still enabled after the modal closes (single boot writer)"
    );
}

// ---------------------------------------------------------------------------
// Workspace scope UX (bug (d), 2026-07-22): /workspace modal + run wiring.
// ---------------------------------------------------------------------------

#[test]
fn workspace_modal_edits_mode_and_allowed_paths_persistently() {
    let mut h = harness();
    h.turn();
    h.type_text("/workspace");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("workspace — mode: server-managed"),
        "modal opens on the honest default:\n{screen}"
    );
    assert!(screen.contains("/tmp/ws"), "root shown:\n{screen}");
    assert!(
        screen.contains("GATEWAY enforces workspace policy"),
        "server-clamp honesty note:\n{screen}"
    );

    // ↓↓ to workspace_or_allowed, Space selects + persists.
    h.term.push_input(b"\x1b[B\x1b[B");
    h.turn();
    h.type_text(" ");
    let screen = h.turn();
    assert!(
        screen.contains("workspace — mode: workspace_or_allowed"),
        "mode applied:\n{screen}"
    );
    assert_eq!(
        h.store.workspace_mode.get_untracked(),
        "workspace_or_allowed"
    );
    assert_eq!(
        h.prefs.borrow().workspace_mode.as_deref(),
        Some("workspace_or_allowed")
    );

    // Tab to the path input; typed path lands on Enter + persists.
    h.term.push_input(b"\t");
    h.turn();
    h.type_text("/srv/data");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(screen.contains("/srv/data"), "added path listed:\n{screen}");
    assert_eq!(
        h.store.workspace_allowed.get_untracked(),
        vec!["/srv/data".to_string()]
    );
    assert_eq!(
        h.prefs.borrow().workspace_allowed,
        vec!["/srv/data".to_string()]
    );

    // Esc closes; the next run start carries the scope.
    h.press_escape();
    h.turn();
    h.turn();
    h.type_text("do something");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            assert_eq!(opts.workspace_mode.as_deref(), Some("workspace_or_allowed"));
            assert_eq!(opts.workspace_allowed, vec!["/srv/data".to_string()]);
        }
        other => panic!("expected Start, got {:?}", other.map(|_| "cmd")),
    }
}

#[test]
fn adding_an_allowed_path_auto_picks_the_mode_that_uses_it() {
    let mut h = harness();
    h.turn();
    h.type_text("/workspace");
    h.turn();
    h.press_enter();
    h.turn();
    // Straight to the input (mode untouched = server-managed): adding a
    // path silently doing nothing would be the dishonest outcome — the
    // modal switches to workspace_or_allowed and says so.
    h.term.push_input(b"\t");
    h.turn();
    h.type_text("/opt/shared");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        h.store.workspace_mode.get_untracked(),
        "workspace_or_allowed",
        "allowed paths only function in workspace_or_allowed — auto-picked"
    );
    assert_eq!(
        h.store.workspace_allowed.get_untracked(),
        vec!["/opt/shared".to_string()]
    );
}

/// bug (d): a path entry with a trailing slash normalizes to the bare
/// form (so a later bare add dedups against it), and a relative path is
/// REFUSED honestly (never silently sent — the gateway resolves paths on
/// its own host, where a relative path is meaningless).
#[test]
fn workspace_path_entry_normalizes_and_refuses_relative() {
    let mut h = harness();
    h.turn();
    h.type_text("/workspace");
    h.turn();
    h.press_enter();
    h.turn();
    h.term.push_input(b"\t");
    h.turn();
    // Trailing slash: stored WITHOUT it.
    h.type_text("/srv/data/");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        h.store.workspace_allowed.get_untracked(),
        vec!["/srv/data".to_string()],
        "trailing slash normalized off"
    );
    // The bare form is now a duplicate (dedup keys on the normal form).
    h.type_text("/srv/data");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        h.store.workspace_allowed.get_untracked(),
        vec!["/srv/data".to_string()],
        "bare form dedups against the trailing-slash form"
    );
    // A relative path is refused — the list is unchanged.
    h.type_text("relative/dir");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        h.store.workspace_allowed.get_untracked(),
        vec!["/srv/data".to_string()],
        "relative path refused, list unchanged"
    );
    // The refusal is honest (a toast notice naming why — the toast is an
    // async overlay, so assert on the notice queue, not the frame text).
    assert!(
        h.store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("not absolute")),
        "the refusal says why: {:?}",
        h.store.notices.get_untracked()
    );
}

// ---------------------------------------------------------------------------
// Queue / steer model (plan item 1, docs/design/plan-interaction-model.md)
// ---------------------------------------------------------------------------

/// Simulate the runner's terminal post: the fold finished, phase Idle,
/// outcome in the mailbox — exactly the closure `post_records`/`finish`
/// posts on the UI thread, in the SAME order (outcome BEFORE phase: each
/// signal write flushes effects synchronously, and the drain effect keys
/// on "phase Idle" — the ordering contract documented in runner.rs).
fn simulate_terminal(store: abstractcode::store::Store, outcome: abstractcode::store::RunOutcome) {
    store.last_outcome.set(outcome);
    store.run_started.set(None);
    store.phase.set(Phase::Idle);
}

#[test]
fn queue_enter_steers_while_slash_queue_enqueues() {
    // The steer-vs-queue split: Enter keeps steering (latency-sensitive
    // intent stays zero-friction); /queue is the FIFO lane. Steering
    // needs a CYCLING target (cycle-2): the fixture provides one.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| {
        f.begin_run("root");
        // A cycling subrun marks the steer target (root-targeted steers
        // are never folded on wrapper bundles).
        let rec = serde_json::json!({"run_id": "sub9", "node_id": "reason", "status": "started",
                                      "effect": {"type": "llm_call", "payload": {}}});
        let _ = f.apply("sub9", &rec);
    });
    h.turn();

    h.type_text("steer this");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(
            |c| matches!(c, Cmd::Steer { run_id, text } if text == "steer this" && run_id == "sub9")
        )
        .is_some(),
        "plain Enter while running steers the cycling run"
    );
    assert_eq!(store.queue.with_untracked(|q| q.len()), 0);

    h.type_text("/queue build the docs");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        store.queue.with_untracked(|q| q.len()),
        1,
        "/queue <text> enqueues"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Steer { .. })).is_none(),
        "/queue never steers"
    );
    // Discoverability: the strip names the queued count.
    let screen = h.turn();
    assert!(
        screen.contains("1 queued"),
        "activity strip shows the queue count:\n{screen}"
    );
}

#[test]
fn running_pre_cycle_submit_buffers_and_delivers_on_the_first_cycle() {
    // The cycle-2 generalization: a plain submit while Running with NO
    // cycling target yet must BUFFER (a root-targeted steer is silently
    // never folded on wrapper bundles), then deliver into the CYCLING
    // subrun once the first reason-cycle record lands.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    h.turn();

    h.type_text("hurry it up");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Steer { .. })).is_none(),
        "no cycling target yet: nothing may be sent"
    );
    let ps = store.pending_steer.get_untracked().expect("buffered");
    assert!(!ps.armed_while_starting, "armed while Running");
    assert_eq!(ps.armed_at_root, "root");

    // The agent subrun is discovered and cycles: delivery fires INTO IT.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &serde_json::json!({"run_id": "root", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:agent1",
                                     "details": {"sub_run_id": "agent1"}}}}),
        );
        let _ = f.apply(
            "agent1",
            &serde_json::json!({"run_id": "agent1", "node_id": "reason", "status": "started",
                "effect": {"type": "llm_call", "payload": {}}}),
        );
    });
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Steer { .. })) {
        Some(Cmd::Steer { run_id, text }) => {
            assert_eq!(run_id, "agent1", "delivered to the CYCLING subrun");
            assert_eq!(text, "hurry it up");
        }
        other => panic!("expected Cmd::Steer, got {:?}", other.map(|_| "cmd")),
    }
    assert!(store.pending_steer.get_untracked().is_none());
    let screen = h.turn();
    assert!(
        screen.contains("hurry it up"),
        "the steer card renders:\n{screen}"
    );
}

#[test]
fn buffered_steer_never_fires_on_a_stale_previous_run_cycle() {
    // The exact race the buffer exists for (cycle-2 identity predicate):
    // text armed during Starting reads the PREVIOUS run's fold — its
    // still-set cycling target must NOT satisfy delivery; only the NEW
    // tree's first cycle (after begin_run changed the root) may.
    let mut h = harness();
    h.turn();
    let store = h.store;
    // Run A lives in the fold with a cycling target.
    store.fold.update(|f| {
        f.begin_run("rootA");
        let _ = f.apply(
            "subA",
            &serde_json::json!({"run_id": "subA", "node_id": "reason", "status": "started",
                "effect": {"type": "llm_call", "payload": {}}}),
        );
    });
    // A new start is in flight; the runner has not posted begin_run yet.
    store.phase.set(Phase::Starting);
    h.turn();

    h.type_text("guidance for run B");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Steer { .. })).is_none(),
        "run A's stale cycling target must not receive run B's guidance"
    );
    assert!(store.pending_steer.get_untracked().is_some(), "still held");

    // The runner's Ok post lands (run_id -> phase -> begin_run order).
    store.run_id.set("rootB".into());
    store.phase.set(Phase::Running);
    store.fold.update(|f| f.begin_run("rootB"));
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Steer { .. })).is_none(),
        "begin_run cleared the cycling target: still no delivery"
    );
    assert!(store.pending_steer.get_untracked().is_some());

    // Run B's first cycle: NOW it delivers, into B's cycling run.
    store.fold.update(|f| {
        let _ = f.apply(
            "agentB",
            &serde_json::json!({"run_id": "agentB", "node_id": "reason", "status": "started",
                "effect": {"type": "llm_call", "payload": {}}}),
        );
    });
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Steer { .. })) {
        Some(Cmd::Steer { run_id, text }) => {
            assert_eq!(run_id, "agentB");
            assert_eq!(text, "guidance for run B");
        }
        other => panic!("expected Cmd::Steer, got {:?}", other.map(|_| "cmd")),
    }
    assert!(store.pending_steer.get_untracked().is_none());
}

#[test]
fn buffered_steer_disposes_visibly_when_the_run_ends_without_a_cycle() {
    // Disposal half 2 (cycle-2): armed while Running, the run finishes
    // before any cycle → Info card, never a silent drop (the Error card
    // is reserved for starts that never began a run).
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    h.turn();
    h.type_text("too late");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(store.pending_steer.get_untracked().is_some());
    // The run concludes with no cycle ever landing.
    store.fold.update(|f| f.run_terminal("completed"));
    simulate_terminal(store, abstractcode::store::RunOutcome::Success);
    h.turn();
    assert!(store.pending_steer.get_untracked().is_none());
    let screen = h.turn();
    assert!(
        screen.contains("steer arrived after the run finished") && screen.contains("too late"),
        "info card carries the words:\n{screen}"
    );
}

#[test]
fn queue_drains_next_as_a_new_run_with_the_prior_answer_in_context() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    // Run A: user card + final answer land in the fold (as the live
    // stream would fold them).
    store.phase.set(Phase::Running);
    store.run_id.set("rootA".into());
    store.fold.update(|f| {
        f.begin_run("rootA");
        f.push_item(abstractcode::transcript::Item::User {
            text: "first task".into(),
        });
        let rec = serde_json::json!({"run_id": "rootA", "node_id": "end", "status": "completed",
                                      "result": {"output": {"answer": "first answer"}}});
        let _ = f.apply("rootA", &rec);
    });
    h.turn();
    // Queue B while A is still running: nothing starts yet.
    h.type_text("/queue second task");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none());

    // A finishes successfully (the runner's finished_now post). The
    // dequeue is a DEFERRED job (after ZERO): give it a turn.
    simulate_terminal(store, abstractcode::store::RunOutcome::Success);
    h.turn();
    h.turn();
    // The drain started B as a NEW run whose context carries A's turn
    // (StartOpts built at drain time — chat_messages reads the fold).
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { prompt, opts, .. }) => {
            assert_eq!(prompt, "second task");
            assert_eq!(
                opts.messages,
                vec![
                    ("user".to_string(), "first task".to_string()),
                    ("assistant".to_string(), "first answer".to_string())
                ],
                "drain-time context carries the just-finished answer"
            );
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
    assert_eq!(store.queue.with_untracked(|q| q.len()), 0, "item popped");
    assert_eq!(store.phase.get_untracked(), Phase::Starting);
}

#[test]
fn queue_halts_on_failure_and_cancel_and_resumes_explicitly() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    h.turn();
    h.type_text("/queue held work");
    h.turn();
    h.press_enter();
    h.turn();

    // The run FAILS: queue pauses, items kept, nothing starts.
    simulate_terminal(store, abstractcode::store::RunOutcome::Failed);
    h.turn();
    h.turn();
    assert!(store.queue_paused.get_untracked(), "failure pauses");
    assert_eq!(store.queue.with_untracked(|q| q.len()), 1, "items kept");
    assert!(h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none());
    let screen = h.turn();
    assert!(
        screen.contains("paused"),
        "the strip says the queue is paused:\n{screen}"
    );

    // Explicit resume (the modal's `r`): the head drains (deferred job).
    store.queue_paused.set(false);
    h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { prompt, .. } if prompt == "held work"))
            .is_some(),
        "explicit resume drains the head"
    );
}

#[test]
fn queue_manual_run_while_paused_proceeds_without_resuming() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.queue_paused.set(true);
    store.queue.update(|q| {
        q.push(abstractcode::store::QueuedPrompt {
            id: 99,
            text: "parked".into(),
        })
    });
    h.turn();

    h.type_text("manual task");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { prompt, .. } if prompt == "manual task"))
            .is_some(),
        "manual runs proceed while the queue is paused"
    );
    // Its SUCCESS still does not auto-resume (explicit resume only).
    simulate_terminal(store, abstractcode::store::RunOutcome::Success);
    h.turn();
    h.turn();
    assert!(store.queue_paused.get_untracked(), "no auto-resume");
    assert_eq!(store.queue.with_untracked(|q| q.len()), 1, "items held");
    assert!(h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none());
}

#[test]
fn queue_start_refusal_restores_the_item_and_pauses() {
    // Cycle-2 REVERSAL (was: popped-and-lost): a queued start that cannot
    // even be SENT (dead worker channel — the synchronously observable
    // shape) must RESTORE the item at head and pause. Nothing was spent;
    // `r` retries the same item.
    let mut h = harness();
    h.turn();
    let store = h.store;
    // Kill the command loop (drop the receiver).
    let (_dummy_tx, dummy_rx) = mpsc::channel::<Cmd>();
    drop(_dummy_tx);
    let dead = std::mem::replace(&mut h.rx, dummy_rx);
    drop(dead);

    store.queue.update(|q| {
        q.push(abstractcode::store::QueuedPrompt {
            id: 1,
            text: "will not start".into(),
        });
        q.push(abstractcode::store::QueuedPrompt {
            id: 2,
            text: "second".into(),
        });
    });
    h.turn();
    h.turn();
    assert!(store.queue_paused.get_untracked(), "refused start pauses");
    assert_eq!(
        store.queue.with_untracked(|q| q.len()),
        2,
        "the refused item is RESTORED at head — nothing was spent"
    );
    assert_eq!(
        store.queue.with_untracked(|q| q[0].text.clone()),
        "will not start",
        "head order preserved for the retry"
    );
}

#[test]
fn queue_http_start_failure_restores_at_head_and_pauses() {
    // The ASYNC start-failure shape: the runner's Err post writes
    // RunOutcome::Failed + phase Idle WITHOUT begin_run — the fold root
    // never changed, which is how the drain knows the start itself
    // failed (restore) rather than the run failing mid-flight (spent).
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.queue.update(|q| {
        q.push(abstractcode::store::QueuedPrompt {
            id: 1,
            text: "gateway will refuse this".into(),
        });
        q.push(abstractcode::store::QueuedPrompt {
            id: 2,
            text: "second".into(),
        });
    });
    h.turn();
    h.turn();
    // The drain dequeued + sent the start (worker alive in this test).
    assert!(
        h.find_cmd(
            |c| matches!(c, Cmd::Start { prompt, .. } if prompt == "gateway will refuse this")
        )
        .is_some(),
        "the drain started the head item"
    );
    assert_eq!(store.queue.with_untracked(|q| q.len()), 1, "head popped");
    assert_eq!(store.phase.get_untracked(), Phase::Starting);

    // The runner's Err post: outcome BEFORE phase; begin_run never ran.
    store
        .last_outcome
        .set(abstractcode::store::RunOutcome::Failed);
    store.phase.set(Phase::Idle);
    h.turn();
    assert!(store.queue_paused.get_untracked(), "start failure pauses");
    assert_eq!(
        store
            .queue
            .with_untracked(|q| q.iter().map(|p| p.text.clone()).collect::<Vec<_>>()),
        vec!["gateway will refuse this".to_string(), "second".to_string()],
        "the failed-start item is restored AT HEAD"
    );

    // Contrast: a run that BEGAN (root changed) and then failed is spent —
    // resume, let it start, begin the run, then fail it.
    store.queue_paused.set(false);
    h.turn();
    h.turn();
    assert!(h
        .find_cmd(
            |c| matches!(c, Cmd::Start { prompt, .. } if prompt == "gateway will refuse this")
        )
        .is_some());
    store.run_id.set("rootX".into());
    store.fold.update(|f| f.begin_run("rootX"));
    store.phase.set(Phase::Running);
    h.turn();
    simulate_terminal(store, abstractcode::store::RunOutcome::Failed);
    h.turn();
    assert_eq!(
        store
            .queue
            .with_untracked(|q| q.iter().map(|p| p.text.clone()).collect::<Vec<_>>()),
        vec!["second".to_string()],
        "a run that began and failed is SPENT (transcript keeps the evidence)"
    );
    assert!(store.queue_paused.get_untracked());
}

#[test]
fn queue_client_refusal_without_workflow_keeps_the_item() {
    // Readiness is checked BEFORE dequeuing (cycle-2 guard): no workflow
    // selected → pause with the item KEPT, and the reason names the fix.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.workflow.set(abstractcode::store::Workflow::default());
    store.queue.update(|q| {
        q.push(abstractcode::store::QueuedPrompt {
            id: 1,
            text: "workflowless".into(),
        })
    });
    h.turn();
    h.turn();
    assert!(store.queue_paused.get_untracked(), "refusal pauses");
    assert_eq!(
        store.queue.with_untracked(|q| q.len()),
        1,
        "the item was never dequeued"
    );
    assert!(h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none());
    assert!(
        store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("/workflow")),
        "the strip notice names the reason"
    );
}

#[test]
fn queue_drain_holds_while_a_wait_is_pending_and_resumes_after() {
    // Cycle-2 guard: waits CAN arm after `finished` (helper subrun asks
    // have no finished gate) — a drain-started run would begin_run-wipe
    // the prompt and orphan the wait. The drain holds while
    // fold.pending_wait is some and RE-FIRES when the wait resolves
    // (fold-tracking is load-bearing: resolution is a fold change with
    // no phase change).
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    h.turn();
    h.type_text("/queue next chapter");
    h.turn();
    h.press_enter();
    h.turn();

    // A helper subrun's ask arms a wait; the answer already landed.
    store.fold.update(|f| {
        let _ = f.apply(
            "helper",
            &serde_json::json!({"run_id": "helper", "status": "waiting", "step_id": "s1",
                "result": {"wait": {"reason": "user", "wait_key": "user:helper:ask",
                                     "prompt": "Deploy too?"}}}),
        );
    });
    simulate_terminal(store, abstractcode::store::RunOutcome::Success);
    h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none(),
        "the drain holds while a wait is pending"
    );
    assert_eq!(store.queue.with_untracked(|q| q.len()), 1, "item held");
    assert!(
        !store.queue_paused.get_untracked(),
        "held, not paused — it resumes by itself when the wait resolves"
    );

    // The wait resolves (answered through the modal path): the drain
    // re-fires off the fold change alone.
    store.fold.update(|f| {
        let wait = f.pending_wait.clone().expect("wait pending");
        f.wait_answered(&wait.wait_key, &wait.step_id);
    });
    h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { prompt, .. } if prompt == "next chapter"))
            .is_some(),
        "wait resolution re-fires the drain"
    );
}

#[test]
fn queue_refused_under_entity_focus_and_hints_stay_agent_scoped() {
    // The queue is AGENT-LANE ONLY (cycle-2 composition section): /queue
    // under entity focus refuses toward the held-draft lane; queue hints
    // never render under entity focus (the strip belongs to the visit).
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.queue.update(|q| {
        q.push(abstractcode::store::QueuedPrompt {
            id: 7,
            text: "agent work".into(),
        })
    });
    store.queue_paused.set(true); // hold it so the drain leaves it alone
    store
        .focus
        .set(abstractcode::convo::Focus::Entity("castor".into()));
    h.turn();

    h.type_text("/queue do something");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        store.queue.with_untracked(|q| q.len()),
        1,
        "/queue under entity focus enqueues NOTHING"
    );
    assert!(
        store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("queue is agent-lane")),
        "the refusal points at the held-draft lane"
    );
    let screen = h.turn();
    assert!(
        !screen.contains("queued"),
        "queue hints are Agent-focus-scoped:\n{screen}"
    );

    // Back under agent focus the hint returns.
    store.focus.set(abstractcode::convo::Focus::Agent);
    let screen = h.turn();
    assert!(
        screen.contains("1 queued"),
        "agent focus shows the queue hint again:\n{screen}"
    );
}

#[test]
fn queue_drain_runs_regardless_of_focus() {
    // The plan's composition rule: only the /queue SURFACE is
    // agent-scoped — the agent lane keeps executing in the background
    // while the user looks at an entity visit.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.queue.update(|q| {
        q.push(abstractcode::store::QueuedPrompt {
            id: 1,
            text: "background agent work".into(),
        })
    });
    store
        .focus
        .set(abstractcode::convo::Focus::Entity("castor".into()));
    h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { prompt, .. } if prompt == "background agent work"))
            .is_some(),
        "the drain fires under entity focus"
    );
}

#[test]
fn queue_starting_phase_submit_buffers_and_delivers_on_the_new_trees_cycle() {
    // Wrapper-bundle-shaped delivery (divergence b): text buffered during
    // Starting delivers on the NEW tree's first reason-cycle record, INTO
    // the cycling subrun — never on the run id alone (root-targeted
    // guidance is never folded on wrapper bundles).
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Starting);
    h.turn();

    h.type_text("use the staging config");
    h.turn();
    h.press_enter();
    h.turn();
    let ps = store.pending_steer.get_untracked().expect("buffered");
    assert!(ps.armed_while_starting);
    assert_eq!(
        ps.text, "use the staging config",
        "Starting-phase submit buffers instead of dropping"
    );
    assert!(h.find_cmd(|c| matches!(c, Cmd::Steer { .. })).is_none());

    // The runner's Ok post (run_id -> phase -> begin_run): STILL no
    // delivery — the tree has no cycling target yet.
    store.run_id.set("rootB".into());
    store.phase.set(Phase::Running);
    store.fold.update(|f| f.begin_run("rootB"));
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Steer { .. })).is_none(),
        "run id alone must not deliver (nothing drains root guidance)"
    );

    // The wrapper bundle discovers the agent SUBRUN, which cycles: the
    // buffer delivers into IT.
    store.fold.update(|f| {
        let _ = f.apply(
            "rootB",
            &serde_json::json!({"run_id": "rootB", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:agentB",
                                     "details": {"sub_run_id": "agentB"}}}}),
        );
        let _ = f.apply(
            "agentB",
            &serde_json::json!({"run_id": "agentB", "node_id": "reason", "status": "started",
                "effect": {"type": "llm_call", "payload": {}}}),
        );
    });
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Steer { .. })) {
        Some(Cmd::Steer { run_id, text }) => {
            assert_eq!(run_id, "agentB", "delivered to the CYCLING subrun");
            assert_eq!(text, "use the staging config");
        }
        other => panic!("expected Cmd::Steer, got {:?}", other.map(|_| "cmd")),
    }
    assert!(store.pending_steer.get_untracked().is_none());
    let screen = h.turn();
    assert!(
        screen.contains("use the staging config"),
        "the steer card renders:\n{screen}"
    );
}

#[test]
fn queue_starting_phase_buffer_error_cards_when_the_start_fails() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Starting);
    h.turn();
    h.type_text("guidance for a run that dies");
    h.turn();
    h.press_enter();
    h.turn();
    // The start FAILS (the runner's Err post flips the phase back; the
    // fold root never changed — the "no new run began" signature).
    store.phase.set(Phase::Idle);
    h.turn();
    assert!(store.pending_steer.get_untracked().is_none());
    let screen = h.turn();
    assert!(
        screen.contains("guidance not delivered"),
        "the buffered text surfaces as an error card:\n{screen}"
    );
    assert!(
        screen.contains("guidance for a run that dies"),
        "the user's words are preserved in the card:\n{screen}"
    );
}

#[test]
fn queue_stashes_on_session_switch_and_restores_paused() {
    // Cycle-2 REVERSAL (was: cleared with drop echoes): the queue is
    // STASHED per session (write-through already made it durable) and the
    // target session's stash loads PAUSED — a restore never auto-starts.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.queue.update(|q| {
        q.push(abstractcode::store::QueuedPrompt {
            id: 1,
            text: "queued one".into(),
        });
        q.push(abstractcode::store::QueuedPrompt {
            id: 2,
            text: "queued two".into(),
        });
    });
    h.turn();
    // Write-through filed the queue under the CURRENT session already.
    assert_eq!(
        h.prefs.borrow().session_queue("acode-test-session"),
        vec!["queued one".to_string(), "queued two".to_string()],
        "every mutation writes through to the session slot"
    );

    abstractcode::ui::switch_session(store, &h.ctx, "acode-other-session");
    // No worker in this harness: complete the armed session-loading
    // screen by hand so the pane returns to the guidance view (where
    // the stash echo below renders).
    store.restoring.set(false);
    h.turn();
    assert_eq!(
        store.queue.with_untracked(|q| q.len()),
        0,
        "the new session starts with its own (empty) stash"
    );
    assert_eq!(
        h.prefs.borrow().session_queue("acode-test-session"),
        vec!["queued one".to_string(), "queued two".to_string()],
        "the old session's stash SURVIVES the switch"
    );
    let screen = h.turn();
    assert!(
        screen.contains("stashed with session"),
        "the stash is echoed visibly:\n{screen}"
    );

    // Switching BACK restores the stash PAUSED; nothing auto-starts.
    abstractcode::ui::switch_session(store, &h.ctx, "acode-test-session");
    store.restoring.set(false); // same hand-completed probe as above
    h.turn();
    h.turn();
    assert_eq!(
        store
            .queue
            .with_untracked(|q| q.iter().map(|p| p.text.clone()).collect::<Vec<_>>()),
        vec!["queued one".to_string(), "queued two".to_string()],
        "the stash restores in order"
    );
    assert!(store.queue_paused.get_untracked(), "restores PAUSED");
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none(),
        "a restore NEVER auto-starts"
    );
    assert!(
        store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("restored (paused")),
        "the restore says so (toast lane)"
    );
}

#[test]
fn queue_boot_restore_loads_paused_and_never_starts() {
    // The quit/reopen half of the same rule (lib.rs calls
    // restore_session_queue at mount): a pre-seeded prefs slot loads
    // PAUSED with a visible notice; the drain never fires.
    let mut h = harness();
    h.turn();
    let store = h.store;
    h.prefs
        .borrow_mut()
        .set_session_queue("acode-test-session", &["saved task".to_string()]);
    abstractcode::ui::restore_session_queue(store, &h.ctx, "acode-test-session");
    h.turn();
    h.turn();
    assert_eq!(
        store
            .queue
            .with_untracked(|q| q.iter().map(|p| p.text.clone()).collect::<Vec<_>>()),
        vec!["saved task".to_string()]
    );
    assert!(store.queue_paused.get_untracked(), "boot restore is PAUSED");
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none(),
        "restore never auto-starts"
    );
    assert!(
        store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("restored (paused")),
        "the restore announces itself"
    );
}

#[test]
fn queue_modal_removes_reorders_resumes_and_pops_to_composer() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    // Held queue (paused) so the drain effect leaves the items alone.
    store.queue_paused.set(true);
    store.queue.update(|q| {
        for (id, text) in [(1u64, "alpha task"), (2, "beta task"), (3, "gamma task")] {
            q.push(abstractcode::store::QueuedPrompt {
                id,
                text: text.into(),
            });
        }
    });
    h.turn();

    h.type_text("/queue");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("prompt queue — 3 waiting") && screen.contains("PAUSED"),
        "manager opens with the paused state:\n{screen}"
    );
    assert!(screen.contains("1. alpha task"), "rows render:\n{screen}");

    // x removes the selected head.
    h.type_text("x");
    let screen = h.turn();
    assert!(
        !screen.contains("alpha task") && screen.contains("1. beta task"),
        "x removes the selected item:\n{screen}"
    );
    // d moves beta below gamma (cursor follows the item).
    h.type_text("d");
    h.turn();
    assert_eq!(
        store
            .queue
            .with_untracked(|q| q.iter().map(|p| p.id).collect::<Vec<_>>()),
        vec![3, 2],
        "d reorders downward"
    );
    // u moves it back up.
    h.type_text("u");
    h.turn();
    assert_eq!(
        store
            .queue
            .with_untracked(|q| q.iter().map(|p| p.id).collect::<Vec<_>>()),
        vec![2, 3],
        "u reorders upward"
    );

    // e pops the selected item into the composer and closes the modal.
    h.type_text("e");
    h.turn();
    h.turn(); // composer_seed effect drains into the TextAreaState
    let screen = h.turn();
    assert_eq!(
        store.queue.with_untracked(|q| q.len()),
        1,
        "popped item left the queue"
    );
    assert!(
        screen.contains("beta task"),
        "popped text seeds the composer draft:\n{screen}"
    );
    assert!(
        !screen.contains("prompt queue —"),
        "modal closed after e:\n{screen}"
    );

    // r resumes a paused queue (reopen the manager first). The composer
    // still holds the popped draft — Esc clears it so "/queue" is the
    // whole submission, not an append.
    h.press_escape();
    h.type_text("/queue");
    h.turn();
    h.press_enter();
    h.turn();
    h.type_text("r");
    h.turn();
    assert!(!store.queue_paused.get_untracked(), "r resumes");
    h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { prompt, .. } if prompt == "gamma task"))
            .is_some(),
        "resume drains the held item (deferred job)"
    );
}

#[test]
fn queue_composer_placeholder_swaps_while_running() {
    // REST-1 moved the key legend behind `?` — the phase teaching lives
    // in the composer placeholder, which HDR-2c made visible while
    // FOCUSED (it was dead pixels: the engine paints its own
    // placeholder only unfocused and the composer autofocuses).
    let mut h = harness();
    let screen = h.turn();
    assert!(
        screen.contains("describe a task — Enter sends"),
        "idle placeholder teaches send:\n{screen}"
    );
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("root".into());
    let screen = h.turn();
    assert!(
        screen.contains("Enter steers the run") && screen.contains("/queue"),
        "running placeholder teaches steer + /queue:\n{screen}"
    );
}

// ---------------------------------------------------------------------------
// Ctrl+J newline (plan item 2)
// ---------------------------------------------------------------------------

#[test]
fn ctrl_j_inserts_newline_at_caret_without_submitting() {
    // Ctrl+J arrives as the LF byte (0x0a) on the legacy wire — the C0
    // arm decodes it to Ctrl+Char('j') and the ENGINE's TextArea edit
    // model inserts at the caret under every submit policy (abstracttui
    // 0.2.2, our 0295 ask; the app-side shortcut is deleted). The
    // submitted prompt proves the caret position: mid-draft insertion
    // yields "a\nb", an end-append would yield "ab".
    let mut h = harness();
    h.turn();
    h.type_text("ab");
    h.turn();
    h.term.push_input(b"\x1b[D"); // Left: caret between 'a' and 'b'
    h.turn();
    h.term.push_input(&[0x0a]); // Ctrl+J
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none(),
        "Ctrl+J never submits"
    );
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { prompt, .. }) => {
            assert_eq!(prompt, "a\nb", "newline landed AT THE CARET");
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
}

#[test]
fn alt_enter_still_inserts_newline_and_plain_enter_submits() {
    // ESC+CR (the legacy alt+Enter encoding) inserts a newline via the
    // engine's SubmitPolicy; plain CR submits. Both must keep working
    // beside the new Ctrl+J chord.
    let mut h = harness();
    h.turn();
    h.type_text("cd");
    h.turn();
    h.term.push_input(b"\x1b\r"); // Alt+Enter (one chunk: no ESC timeout)
    h.turn();
    h.type_text("ef");
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none(),
        "Alt+Enter never submits"
    );
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { prompt, .. }) => {
            assert_eq!(prompt, "cd\nef", "Alt+Enter inserted the newline");
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
}

// ---------------------------------------------------------------------------
// GFM tables in agent answers (free on 0.2.3+: Feed markdown items
// typeset the doc vocabulary)
// ---------------------------------------------------------------------------

#[test]
fn assistant_answer_tables_typeset_instead_of_raw_pipes() {
    // Assistant bodies are FeedBlock::Markdown; since abstracttui 0.2.3
    // Feed markdown items parse through `md::parse_doc`, so a pipe
    // table renders as a TABLE (cells typeset, delimiter row consumed)
    // instead of the raw `| a | b |` text that read as broken.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "compare".into(),
        });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "Here:\n\n| crate | tests |\n| --- | --- |\n| abstracttui | 1660 |\n".into(),
            final_answer: true,
        });
    });
    // Two pumps: first feed mount discovers width at draw; the measured
    // extent syncs on the following frame (engine geometry contract).
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("crate") && screen.contains("abstracttui") && screen.contains("1660"),
        "table cells render:\n{screen}"
    );
    assert!(
        !screen.contains("| crate |") && !screen.contains("| --- |"),
        "raw pipe source never renders (the table is typeset):\n{screen}"
    );
}

// ---------------------------------------------------------------------------
// /goal (plan item 3 — client half; dark until the flow seat publishes)
// ---------------------------------------------------------------------------

fn seed_goal_workflow(store: abstractcode::store::Store) {
    store
        .goal_workflows
        .set(vec![abstractcode::store::Workflow {
            bundle_id: "goal-agent".into(),
            flow_id: "goal-loop".into(),
            name: "goal-loop".into(),
            description: String::new(),
        }]);
}

#[test]
fn goal_dark_notice_when_no_goal_workflows_exist() {
    // The bundle is flow-seat-owned and unpublished: /goal ships DARK
    // behind catalog discovery, with the honest notice naming the
    // interface — never a fake start.
    let mut h = harness();
    h.turn();
    let store = h.store;
    h.type_text("/goal make it green");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none());
    assert!(store.goal.get_untracked().is_none());
    assert!(
        store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("abstractcode.goal.v1")),
        "the dark notice names the interface"
    );
    // Bare /goal with no active goal says so too.
    h.type_text("/goal");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(store
        .notices
        .get_untracked()
        .iter()
        .any(|n| n.contains("no active goal")));
}

#[test]
fn goal_start_refused_while_a_run_is_active() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    seed_goal_workflow(store);
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    h.turn();
    h.type_text("/goal ship it");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none(),
        "no goal start over a live run"
    );
    assert!(store.goal.get_untracked().is_none());
    assert!(store
        .notices
        .get_untracked()
        .iter()
        .any(|n| n.contains("a run is active")));
}

#[test]
fn goal_start_binds_the_run_and_sets_finish_on_root_only() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    seed_goal_workflow(store);
    h.turn();

    h.type_text("/goal make the suite green");
    h.turn();
    h.press_enter();
    h.turn();
    // The start rides the GOAL workflow with the goal input contract.
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start {
            prompt,
            flow_id,
            bundle_id,
            opts,
            ..
        }) => {
            assert_eq!(flow_id, "goal-loop");
            assert_eq!(bundle_id, "goal-agent");
            assert_eq!(prompt, "make the suite green");
            assert_eq!(
                opts.goal,
                Some(("make the suite green".to_string(), 8)),
                "goal + the pref-default max_cycles ride StartOpts"
            );
        }
        other => panic!("expected Cmd::Start, got {:?}", other.map(|_| "cmd")),
    }
    assert_eq!(store.phase.get_untracked(), Phase::Starting);
    let pending = store.goal.get_untracked().expect("goal armed");
    assert!(pending.run_id.is_empty(), "unbound until Running");

    // The runner's Ok post: the goal binds to the run that reaches
    // Running (starts are phase-serialized) and the fold flag arms.
    store.run_id.set("goalrun".into());
    store.phase.set(Phase::Running);
    store.fold.update(|f| f.begin_run("goalrun"));
    h.turn();
    let bound = store.goal.get_untracked().expect("still armed");
    assert_eq!(bound.run_id, "goalrun", "bound to the goal run");
    assert!(
        store.fold.with_untracked(|f| f.finish_on_root_only),
        "the P0 defense arms with the binding"
    );
    assert_eq!(
        h.prefs.borrow().session_goal("acode-test-session"),
        Some(("make the suite green".to_string(), "goalrun".to_string())),
        "the bound goal persists for restart/reattach"
    );

    // Iteration 1's agent subrun answers: NON-final card, composer stays
    // captured (fold not finished, phase still Running).
    store.fold.update(|f| {
        let _ = f.apply(
            "goalrun",
            &serde_json::json!({"run_id": "goalrun", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:iter1",
                                     "details": {"sub_run_id": "iter1"}}}}),
        );
        let _ = f.apply(
            "iter1",
            &serde_json::json!({"run_id": "iter1", "node_id": "reason", "status": "started",
                "effect": {"type": "llm_call", "payload": {}}}),
        );
        let _ = f.apply(
            "iter1",
            &serde_json::json!({"run_id": "iter1", "node_id": "done", "status": "completed",
                "result": {"output": {"answer": "iteration 1 done"}}}),
        );
    });
    h.turn();
    assert!(
        !store.fold.with_untracked(|f| f.finished),
        "a subrun answer must NOT finish a goal run (the iteration-1 P0)"
    );
    assert_eq!(store.phase.get_untracked(), Phase::Running);
    let screen = h.turn();
    assert!(
        screen.contains("goal:"),
        "the strip names the active goal:\n{screen}"
    );

    // The ROOT's own end concludes; terminal clears the goal slot.
    store.fold.update(|f| {
        let _ = f.apply(
            "goalrun",
            &serde_json::json!({"run_id": "goalrun", "node_id": "end", "status": "completed",
                "result": {"output": {"answer": "goal met"}}}),
        );
    });
    assert!(store.fold.with_untracked(|f| f.finished));
    simulate_terminal(store, abstractcode::store::RunOutcome::Success);
    h.turn();
    assert!(
        store.goal.get_untracked().is_none(),
        "an observed end retires the goal"
    );
    assert_eq!(
        h.prefs.borrow().session_goal("acode-test-session"),
        None,
        "the prefs slot clears with it"
    );
}

#[test]
fn goal_stop_cancels_durably_and_clears_the_slot() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.goal.set(Some(abstractcode::store::GoalState {
        text: "long goal".into(),
        run_id: "goalrun".into(),
    }));
    store.run_id.set("goalrun".into());
    store.phase.set(Phase::Running);
    h.prefs.borrow_mut().set_session_goal(
        "acode-test-session",
        Some(("long goal".into(), "goalrun".into())),
    );
    h.turn();

    h.type_text("/goal stop");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Cancel { run_id } if run_id == "goalrun"))
            .is_some(),
        "/goal stop cancels the goal run durably"
    );
    assert!(store.goal.get_untracked().is_none());
    assert_eq!(h.prefs.borrow().session_goal("acode-test-session"), None);
}

// ---------------------------------------------------------------------------
// Cycle-3 whole-system audit: the cross-lane compositions no single lane
// owned. Cell letters refer to the interaction matrix (audit deliverable).
// ---------------------------------------------------------------------------

/// Cell (a): /goal × queue. A goal is a STANDING run — the queue holds
/// through every iteration (subrun answers never flip the phase under
/// `finish_on_root_only`) and drains only after the goal's ROOT ends,
/// exactly like any run. The drained item starts as a NORMAL agent run
/// (agent workflow, no goal params, flag off) — never a second goal.
#[test]
fn goal_holds_the_queue_and_drains_it_after_the_goal_root_ends() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    seed_goal_workflow(store);
    h.turn();

    // Start the goal; simulate the runner's Ok post (bind + begin_run).
    h.type_text("/goal make the suite green");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { flow_id, .. } if flow_id == "goal-loop"))
            .is_some(),
        "the goal start rides the goal workflow"
    );
    store.run_id.set("goalrun".into());
    store.phase.set(Phase::Running);
    store.fold.update(|f| f.begin_run("goalrun"));
    h.turn();
    assert!(store.fold.with_untracked(|f| f.finish_on_root_only));

    // Queue a follow-up while the goal runs: held, nothing starts.
    h.type_text("/queue follow-up task");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(store.queue.with_untracked(|q| q.len()), 1);
    assert!(h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none());

    // Iteration 1 runs and ends with an answer-shaped flow end: the goal
    // stays open — and the QUEUE stays held (no phase flip, no drain).
    store.fold.update(|f| {
        let _ = f.apply(
            "goalrun",
            &serde_json::json!({"run_id": "goalrun", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:iter1",
                                     "details": {"sub_run_id": "iter1"}}}}),
        );
        let _ = f.apply(
            "iter1",
            &serde_json::json!({"run_id": "iter1", "node_id": "reason", "status": "started",
                "effect": {"type": "llm_call", "payload": {}}}),
        );
        let _ = f.apply(
            "iter1",
            &serde_json::json!({"run_id": "iter1", "node_id": "done", "status": "completed",
                "result": {"output": {"answer": "iteration 1 done"}}}),
        );
    });
    h.turn();
    h.turn();
    assert!(!store.fold.with_untracked(|f| f.finished));
    assert_eq!(store.phase.get_untracked(), Phase::Running);
    assert_eq!(
        store.queue.with_untracked(|q| q.len()),
        1,
        "the queue holds through goal iterations"
    );
    assert!(h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none());

    // The ROOT's own end concludes the goal; the runner's terminal post
    // writes the outcome mailbox then flips the phase.
    store.fold.update(|f| {
        let _ = f.apply(
            "goalrun",
            &serde_json::json!({"run_id": "goalrun", "node_id": "end", "status": "completed",
                "result": {"output": {"answer": "goal met"}}}),
        );
    });
    assert!(store.fold.with_untracked(|f| f.finished));
    simulate_terminal(store, abstractcode::store::RunOutcome::Success);
    h.turn();
    h.turn(); // deferred dequeue job
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start {
            prompt,
            flow_id,
            opts,
            ..
        }) => {
            assert_eq!(prompt, "follow-up task");
            assert_eq!(
                flow_id, "81795ea9",
                "the drained item runs the AGENT workflow, never the goal one"
            );
            assert!(opts.goal.is_none(), "no goal params ride a queued drain");
        }
        other => panic!("expected the drain Start, got {:?}", other.map(|_| "cmd")),
    }
    assert_eq!(store.queue.with_untracked(|q| q.len()), 0);
    assert!(
        store.goal.get_untracked().is_none(),
        "the observed goal end retired the slot before the drain started"
    );
    assert!(
        !store.fold.with_untracked(|f| f.finish_on_root_only),
        "the flag never leaks into the drained run"
    );
    assert_eq!(store.phase.get_untracked(), Phase::Starting);
}

/// Cell (b): /goal × tier policy. Goal runs build StartOpts through the
/// SHARED `agent_start_opts` path — the persisted tier policy, workspace
/// scope, and skills ride `input_data` exactly like a plain prompt's run;
/// goal params compose on top; no client transcript messages.
#[test]
fn goal_runs_carry_the_current_tier_policy_and_shared_start_opts() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    seed_goal_workflow(store);
    store.tools.set(vec![
        abstractcode::store::ToolInfo {
            name: "read_file".into(),
            description: "Read".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "write_file".into(),
            description: "Write".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "fetch_url".into(),
            description: "Fetch".into(),
            toolset: "web".into(),
            ..Default::default()
        },
    ]);
    store.accepted_tier.set("write".into());
    store
        .tool_overrides
        .set(vec![("fetch_url".into(), "ask".into())]);
    store.workspace_mode.set("workspace_or_allowed".into());
    store.workspace_allowed.set(vec!["/srv/data".into()]);
    store.selected_skills.set(vec!["coredoc".into()]);
    h.turn();

    h.type_text("/goal ship it");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            assert_eq!(opts.goal, Some(("ship it".to_string(), 8)));
            let auto = &opts.tool_policy.auto_approve_tools;
            assert!(auto.contains(&"read_file".to_string()), "{auto:?}");
            assert!(auto.contains(&"write_file".to_string()), "{auto:?}");
            assert_eq!(
                opts.tool_policy.require_approval_tools,
                vec!["fetch_url".to_string()],
                "ask pins force-ask on goal runs too"
            );
            assert_eq!(opts.workspace_mode.as_deref(), Some("workspace_or_allowed"));
            assert_eq!(opts.workspace_allowed, vec!["/srv/data".to_string()]);
            assert_eq!(opts.skills, vec!["coredoc".to_string()]);
            assert!(
                opts.messages.is_empty(),
                "goal runs carry no client transcript (server seed owns continuity)"
            );
        }
        other => panic!("expected the goal Start, got {:?}", other.map(|_| "cmd")),
    }
}

/// Cell (c): queue × tier. A queued run's StartOpts build AT DRAIN TIME —
/// a tier raised while the previous run was still working reaches the
/// drained run's server-side policy (enqueue-time snapshotting would
/// silently pin yesterday's posture).
#[test]
fn queued_items_drain_with_the_tier_policy_current_at_drain_time() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.tools.set(vec![
        abstractcode::store::ToolInfo {
            name: "read_file".into(),
            description: "Read".into(),
            toolset: "files".into(),
            ..Default::default()
        },
        abstractcode::store::ToolInfo {
            name: "write_file".into(),
            description: "Write".into(),
            toolset: "files".into(),
            ..Default::default()
        },
    ]);
    // Default tier ("" reads as read): write_file would NOT auto-approve.
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    h.turn();
    h.type_text("/queue write the docs");
    h.turn();
    h.press_enter();
    h.turn();

    // Mid-run the user raises the tier (the persisted dial).
    h.type_text("/tools tier write");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(store.accepted_tier.get_untracked(), "write");
    assert_eq!(h.prefs.borrow().tool_accepted_tier, "write");

    // The run succeeds; the drain builds the queued run's opts NOW.
    simulate_terminal(store, abstractcode::store::RunOutcome::Success);
    h.turn();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { prompt, .. } if prompt == "write the docs")) {
        Some(Cmd::Start { opts, .. }) => {
            assert!(
                opts.tool_policy
                    .auto_approve_tools
                    .contains(&"write_file".to_string()),
                "the drain expanded the CURRENT tier, not the enqueue-time one: {:?}",
                opts.tool_policy
            );
        }
        other => panic!("expected the drain Start, got {:?}", other.map(|_| "cmd")),
    }
}

/// Cell (d): entity focus × approval modal. An agent-run approval is
/// run-blocking: while an ENTITY conversation is focused, the modal still
/// opens, the strip names the lane ("agent: approval needed" — worker B's
/// exception), and the tier/blanket auto-approve paths still fire. Focus
/// never gates approval plumbing.
#[test]
fn agent_approvals_prompt_and_auto_approve_under_entity_focus() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    // An entity conversation holds the focus.
    store.convos.update(|cs| {
        let mut c = abstractcode::convo::EntityConvo::opening("castor", "awake");
        c.run_id = "visit-run".into();
        c.status = abstractcode::convo::ConvoStatus::Parked;
        cs.push(c);
    });
    store
        .focus
        .set(abstractcode::convo::Focus::Entity("castor".into()));
    // An agent run is live behind it.
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    h.turn();

    // An above-tier batch arms: the modal MUST open over entity focus,
    // and the strip names the agent lane.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s1",
                "tool_approval:k1",
                serde_json::json!([{"name": "write_file",
                                    "arguments": {"path": "a.rs", "content": "x"}}]),
            ),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "the approval modal opens even under entity focus:\n{screen}"
    );
    assert!(
        screen.contains("agent: approval needed"),
        "the strip names the agent lane under entity focus:\n{screen}"
    );

    // Approve through the modal; the resume rides the AGENT run.
    h.type_text("a");
    h.turn();
    h.turn(); // deferred modal close
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume {
            run_id, approved, ..
        }) => {
            assert_eq!(run_id, "root");
            assert_eq!(approved, Some(true));
        }
        other => panic!("expected Resume, got {:?}", other.map(|_| "cmd")),
    }

    // A read-tier batch auto-approves silently — focus-independent.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s2",
                "tool_approval:k2",
                serde_json::json!([{"name": "read_file", "arguments": {"path": "a.rs"}}]),
            ),
        );
    });
    let screen = h.turn();
    assert!(
        !screen.contains("approve (a)"),
        "at-tier batches never prompt, entity focus or not:\n{screen}"
    );
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume {
            wait_key, approved, ..
        }) => {
            assert_eq!(wait_key, "tool_approval:k2");
            assert_eq!(approved, Some(true));
        }
        other => panic!(
            "expected the tier auto Resume, got {:?}",
            other.map(|_| "cmd")
        ),
    }
    assert_eq!(
        store.focus.get_untracked(),
        abstractcode::convo::Focus::Entity("castor".into()),
        "approval plumbing never steals the focus"
    );
}

/// Cell (e): pending_steer × goal. A steer mid-cycle rides the CYCLING
/// iteration subrun; a steer typed BETWEEN iterations (the cycling run's
/// own end cleared the target — its guidance inbox died with it) BUFFERS
/// and delivers into the NEXT iteration's first cycle. Nothing is ever
/// injected into a dead run.
#[test]
fn steers_between_goal_iterations_buffer_and_deliver_into_the_next_cycle() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    seed_goal_workflow(store);
    h.turn();
    h.type_text("/goal loop it");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_some());
    store.run_id.set("goalrun".into());
    store.phase.set(Phase::Running);
    store.fold.update(|f| f.begin_run("goalrun"));
    h.turn();

    // Iteration 1 is discovered and cycles: a steer goes straight to it.
    store.fold.update(|f| {
        let _ = f.apply(
            "goalrun",
            &serde_json::json!({"run_id": "goalrun", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:iter1",
                                     "details": {"sub_run_id": "iter1"}}}}),
        );
        let _ = f.apply(
            "iter1",
            &serde_json::json!({"run_id": "iter1", "node_id": "reason", "status": "started",
                "effect": {"type": "llm_call", "payload": {}}}),
        );
    });
    h.turn();
    h.type_text("focus the tests");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Steer { .. })) {
        Some(Cmd::Steer { run_id, text }) => {
            assert_eq!(run_id, "iter1", "mid-cycle steers ride the live iteration");
            assert_eq!(text, "focus the tests");
        }
        other => panic!("expected Cmd::Steer, got {:?}", other.map(|_| "cmd")),
    }

    // Iteration 1 ends (non-final under the goal flag). A steer typed in
    // the gap must BUFFER — iteration 1's guidance inbox died with it.
    store.fold.update(|f| {
        let _ = f.apply(
            "iter1",
            &serde_json::json!({"run_id": "iter1", "node_id": "done", "status": "completed",
                "result": {"output": {"answer": "iteration 1 done"}}}),
        );
    });
    h.turn();
    h.type_text("also update the docs");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Steer { .. })).is_none(),
        "no steer may target the finished iteration"
    );
    let ps = store
        .pending_steer
        .get_untracked()
        .expect("buffered between iterations");
    assert_eq!(ps.armed_at_root, "goalrun");
    assert!(!ps.armed_while_starting);

    // Iteration 2 cycles: the buffer delivers INTO IT.
    store.fold.update(|f| {
        let _ = f.apply(
            "goalrun",
            &serde_json::json!({"run_id": "goalrun", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:iter2",
                                     "details": {"sub_run_id": "iter2"}}}}),
        );
        let _ = f.apply(
            "iter2",
            &serde_json::json!({"run_id": "iter2", "node_id": "reason", "status": "started",
                "effect": {"type": "llm_call", "payload": {}}}),
        );
    });
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Steer { .. })) {
        Some(Cmd::Steer { run_id, text }) => {
            assert_eq!(run_id, "iter2", "delivered into the NEXT iteration");
            assert_eq!(text, "also update the docs");
        }
        other => panic!(
            "expected the buffered Steer, got {:?}",
            other.map(|_| "cmd")
        ),
    }
    assert!(store.pending_steer.get_untracked().is_none());
}

/// Cell (f): /new and /sessions across EVERY lane at once. Enumerates the
/// reset contract: queue STASHES with its session (restores PAUSED),
/// pending_steer drops with an echo, the goal slot follows its session
/// (the old session's prefs slot survives as restart insurance — /goal
/// stop is the documented escape hatch), auto_approve resets, the
/// persisted tier survives, entity convos survive with focus reset, and
/// the old queue can never drain through the boundary.
#[test]
fn session_boundaries_reset_exactly_the_right_lanes() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    let old_sid = "acode-test-session".to_string();

    // Load EVERY lane. The permissions level (persisted per session +
    // global mirror — the c5028 consolidation; the old session blanket
    // is deleted):
    store.accepted_tier.set("write".into());
    h.prefs.borrow_mut().tool_accepted_tier = "write".into();
    // A live GOAL run:
    store.phase.set(Phase::Running);
    store.run_id.set("goalrun".into());
    store.fold.update(|f| f.begin_run("goalrun"));
    store.goal.set(Some(abstractcode::store::GoalState {
        text: "long goal".into(),
        run_id: "goalrun".into(),
    }));
    h.prefs
        .borrow_mut()
        .set_session_goal(&old_sid, Some(("long goal".into(), "goalrun".into())));
    h.turn();
    // Two queued prompts (held by the running phase, unpaused):
    h.type_text("/queue task one");
    h.turn();
    h.press_enter();
    h.turn();
    h.type_text("/queue task two");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(store.queue.with_untracked(|q| q.len()), 2);
    assert!(!store.queue_paused.get_untracked());
    // A buffered steer (no cycling target yet):
    h.type_text("hurry it up");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(store.pending_steer.get_untracked().is_some());
    // An entity conversation, focused:
    store.convos.update(|cs| {
        let mut c = abstractcode::convo::EntityConvo::opening("castor", "awake");
        c.run_id = "visit-run".into();
        c.status = abstractcode::convo::ConvoStatus::Parked;
        cs.push(c);
    });
    store
        .focus
        .set(abstractcode::convo::Focus::Entity("castor".into()));
    h.turn();

    // /new — the boundary. (Commands parse before entity routing, so it
    // works under entity focus.)
    h.type_text("/new");
    h.turn();
    h.press_enter();
    h.turn();
    h.turn(); // any deferred drain job must observe the swapped state

    // The live goal run was cancelled, not orphaned.
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Cancel { run_id } if run_id == "goalrun"))
            .is_some(),
        "/new cancels the live run"
    );
    let new_sid = store.session_id.get_untracked();
    assert_ne!(new_sid, old_sid);
    // Queue: stashed with the OLD session, empty + unpaused here, and it
    // NEVER drained through the boundary (the deferred job re-checks).
    assert_eq!(
        h.prefs.borrow().session_queue(&old_sid),
        vec!["task one".to_string(), "task two".to_string()],
        "the old session's queue is stashed, not dropped"
    );
    assert_eq!(store.queue.with_untracked(|q| q.len()), 0);
    assert!(!store.queue_paused.get_untracked());
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none(),
        "the old queue must never drain across a session boundary"
    );
    // Steer buffer: dropped WITH an echo.
    assert!(store.pending_steer.get_untracked().is_none());
    let screen = h.turn();
    assert!(
        screen.contains("buffered guidance dropped"),
        "the drop is echoed, never silent:\n{screen}"
    );
    // Goal: the slot follows its session — cleared here, retained in the
    // OLD session's prefs (restart insurance; /goal stop clears a stale
    // label after its run died).
    assert!(store.goal.get_untracked().is_none());
    assert!(
        !store.fold.with_untracked(|f| f.finish_on_root_only),
        "the goal flag never survives into the fresh session"
    );
    assert_eq!(
        h.prefs.borrow().session_goal(&old_sid),
        Some(("long goal".to_string(), "goalrun".to_string()))
    );
    // The permissions LEVEL survives the boundary (c5028 semantics: a
    // level — `all` included — persists per session and seeds new
    // sessions via the global baseline; the old die-at-session-end
    // blanket is deleted, hazard 1 disclosed to the operator).
    assert_eq!(store.accepted_tier.get_untracked(), "write");
    assert_eq!(h.prefs.borrow().tool_accepted_tier, "write");
    // Entity conversations survive; focus comes home to the agent.
    assert_eq!(store.convos.with_untracked(|cs| cs.len()), 1);
    assert_eq!(
        store.focus.get_untracked(),
        abstractcode::convo::Focus::Agent
    );

    // Switch BACK to the old session: the stash restores PAUSED, the goal
    // label returns, and a reattach probe goes out. Still nothing starts.
    h.type_text(&format!("/sessions {old_sid}"));
    h.turn();
    h.press_enter();
    h.turn();
    h.turn();
    assert_eq!(store.session_id.get_untracked(), old_sid);
    assert_eq!(store.queue.with_untracked(|q| q.len()), 2);
    assert!(
        store.queue_paused.get_untracked(),
        "restored queues land PAUSED, never auto-start"
    );
    assert_eq!(
        store.goal.get_untracked(),
        Some(abstractcode::store::GoalState {
            text: "long goal".into(),
            run_id: "goalrun".into(),
        }),
        "the goal label follows its session back"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::ProbeAttach { session_id, .. } if *session_id == old_sid))
            .is_some(),
        "a session switch probes for the live run"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none(),
        "restores never auto-start"
    );
}

// ---------------------------------------------------------------------------
// Wave 0 — PRESENCE + DENSITY (HDR-1 · REST-1 · CTX-0 · IDLE-1 · HDR-2 ·
// OBS-1a-live)
// ---------------------------------------------------------------------------

#[test]
fn header_carries_cockpit_facts_and_footer_carries_instruments() {
    // HDR-1 + REST-1: the header's blank middle carries facts; the
    // status bar is the instrument row (legend behind `?`).
    let mut h = harness();
    let screen = h.turn();
    assert!(
        screen.contains("⌂ ws") && screen.contains("server-managed"),
        "header names directory + workspace mode:\n{screen}"
    );
    assert!(
        screen.contains("? keys"),
        "footer points at the legend home:\n{screen}"
    );
    // Counts render when nonzero (never "skills 0" noise).
    assert!(!screen.contains("skills 0"), "{screen}");
    h.store
        .selected_skills
        .set(vec!["coredoc".into(), "agora-channels".into()]);
    h.store
        .mcp_servers
        .set(vec![abstractcode::store::McpServer {
            name: "context7".into(),
            url: "https://mcp.example".into(),
            description: String::new(),
            auth_required: false,
        }]);
    // Session totals at rest reach both header and footer. A provider
    // that reports the split renders it (the ↑/↓ vocabulary); the
    // splitless case is pinned in the drop test below.
    h.store.totals.set(abstractcode::store::SessionTotals {
        input_tokens: 100_000,
        output_tokens: 28_000,
        total_tokens: 128_000,
        runs: 3,
    });
    let screen = h.turn();
    assert!(
        screen.contains("skills 2") && screen.contains("mcp 1"),
        "capability counts render when nonzero:\n{screen}"
    );
    assert!(
        screen.contains("100k↑ 28k↓ tk session"),
        "footer carries the split session tokens:\n{screen}"
    );
}

#[test]
fn idle_fact_card_is_a_cockpit_and_dedupes_the_wordmark() {
    // IDLE-1: the empty state is the Python banner's fact set; the
    // wordmark renders ONCE (it appeared twice before — header + empty
    // state, while zero capability facts appeared at all).
    let mut h = harness();
    let screen = h.turn();
    for needle in [
        "workflow",
        "route",
        "workspace",
        "session",
        "gateway",
        "skills",
        "mcp",
        "context",
    ] {
        assert!(screen.contains(needle), "card names {needle}:\n{screen}");
    }
    assert!(
        screen.contains("window not declared"),
        "context source honesty (no fabricated window):\n{screen}"
    );
    assert!(
        screen.contains("127.0.0.1:8080"),
        "gateway host on the card:\n{screen}"
    );
    assert_eq!(
        screen.matches("▲ AbstractCode").count(),
        1,
        "wordmark deduped — header only:\n{screen}"
    );
}

#[test]
fn fresh_session_strip_shows_the_session_line_not_blank() {
    // REST-1: the reserved activity-strip row was a permanently blank
    // line on first launch.
    let mut h = harness();
    let screen = h.turn();
    assert!(
        screen.contains("no runs yet"),
        "fresh-session strip line:\n{screen}"
    );
}

#[test]
fn context_command_declares_persists_clears_and_refuses() {
    let mut h = harness();
    h.turn();
    h.type_text("/context 262k");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert_eq!(h.store.context_window.get_untracked(), 262_000);
    assert_eq!(
        h.prefs.borrow().context_window,
        262_000,
        "declaration persists"
    );
    assert!(
        screen.contains("ctx —/262k tk (declared)"),
        "declared-but-unmeasured meter renders an em-dash:\n{screen}"
    );
    // A measured call fills the meter with the % + source label.
    h.store.fold.update(|f| {
        f.begin_run("root");
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "hi",
                        "usage": {"input_tokens": 41_203, "output_tokens": 20}}
        });
        let _ = f.apply("root", &rec);
    });
    let screen = h.turn();
    assert!(
        screen.contains("ctx 41k/262k tk (15%, declared)"),
        "used/window meter, source-labeled:\n{screen}"
    );
    // Junk refuses loudly and changes nothing.
    h.type_text("/context lots");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.context_window.get_untracked(), 262_000);
    // `/context off` clears + persists.
    h.type_text("/context off");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert_eq!(h.store.context_window.get_untracked(), 0);
    assert_eq!(h.prefs.borrow().context_window, 0);
    assert!(
        screen.contains("ctx 41k tk"),
        "absence keeps today's honest absolute:\n{screen}"
    );
}

#[test]
fn declared_window_rides_run_starts_as_limits() {
    // CTX-0: the declaration feeds `_limits.max_tokens` on the wire.
    let mut h = harness();
    h.turn();
    h.store.context_window.set(32_000);
    h.type_text("run the suite");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { opts, .. }) => {
            assert_eq!(opts.context_window, 32_000);
            let input = abstractcode::run_input::build_input_data("x", &opts);
            assert_eq!(input["_limits"]["max_tokens"], serde_json::json!(32_000));
        }
        other => panic!("expected Start, got {:?}", other.map(|_| "cmd")),
    }
}

#[test]
fn ctrl_c_clears_the_draft_then_two_consecutive_presses_quit() {
    // Operator ruling (2026-07-23): Ctrl+C erases the current prompt (if
    // any); TWO consecutive Ctrl+C are required to quit. The registration
    // is a global ACTION so it shadows the engine's default
    // Ctrl+C-instant-quit everywhere, including while a modal is open.
    let mut h = harness();
    h.turn();
    h.type_text("half-typed prompt");
    let screen = h.turn();
    assert!(
        screen.contains("half-typed prompt"),
        "draft in composer:\n{screen}"
    );
    // First press: clears the draft, arms quit, never quits.
    h.term.push_input(&[0x03]);
    h.turn();
    let screen = h.turn();
    assert!(!h.app.quit_requested(), "first press never quits");
    assert!(
        !screen.contains("half-typed prompt"),
        "draft cleared:\n{screen}"
    );
    // The arm notice lands in the toast lane (toasts materialize on a
    // 60ms timer headless turns don't wait for — the notices signal is
    // the assertable truth, per the queue-restore precedent).
    assert!(
        h.store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("Ctrl+C again to quit")),
        "arm notice in the toast lane"
    );
    let _ = screen;
    // Second consecutive press: quits.
    h.term.push_input(&[0x03]);
    h.turn();
    h.turn();
    assert!(h.app.quit_requested(), "second consecutive press quits");
}

#[test]
fn tool_call_ticker_renders_while_a_batch_runs_and_clears_on_completion() {
    // The tool twin of the model-call ticker (live P0, 2026-07-23: an
    // 8m39s gateway-side search_files rendered as a bare "running
    // search_files" — no clock, read as a client hang). The strip must
    // carry "tool call Ns" while a batch executes and drop it when the
    // completion folds.
    let mut h = harness();
    h.turn();
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("root".into());
    h.store.fold.update(|f| {
        f.begin_run("root");
        let started = serde_json::json!({
            "run_id": "root", "node_id": "act", "status": "started",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"name": "search_files", "arguments": {"pattern": "x"}}
            ]}}
        });
        let _ = f.apply("root", &started);
    });
    let screen = h.turn();
    assert!(
        screen.contains("running search_files"),
        "activity names the tool:\n{screen}"
    );
    assert!(
        screen.contains("tool call 0s"),
        "in-flight batch ticks from the first second:\n{screen}"
    );
    h.store.fold.update(|f| {
        let done = serde_json::json!({
            "run_id": "root", "node_id": "act", "status": "completed",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"name": "search_files", "arguments": {"pattern": "x"}}
            ]}},
            "result": {"results": [
                {"name": "search_files", "success": true, "output": "ok"}
            ]}
        });
        let _ = f.apply("root", &done);
    });
    let screen = h.turn();
    // Substring-wide negative (cycle-3 nit): on a slow machine a BROKEN
    // clear would render "tool call 1s" and a "tool call 0s" negative
    // would false-pass. Nothing else on this screen carries the
    // substring (the pending-wait "N tool call(s)" line needs a wait).
    assert!(
        !screen.contains("tool call"),
        "completion drops the clock:\n{screen}"
    );
}

#[test]
fn model_call_ticker_and_last_call_rate() {
    // OBS-1a-live: the strip names the in-flight call from second zero;
    // a completed call mints the labeled last-call rate.
    let mut h = harness();
    h.turn();
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("root".into());
    h.store.fold.update(|f| {
        f.begin_run("root");
        let started = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "started",
            "effect": {"type": "llm_call", "payload": {}}
        });
        let _ = f.apply("root", &started);
    });
    let screen = h.turn();
    assert!(
        screen.contains("model call 0s"),
        "in-flight call ticks from the first second:\n{screen}"
    );
    // Let the client clock accumulate a measurable window, then complete.
    std::thread::sleep(std::time::Duration::from_millis(80));
    h.store.fold.update(|f| {
        let done = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "hi",
                        "usage": {"input_tokens": 1000, "output_tokens": 64}}
        });
        let _ = f.apply("root", &done);
    });
    h.turn();
    let rate = h.store.last_call_rate.get_untracked();
    assert!(
        rate.map(|r| r > 0.0).unwrap_or(false),
        "completed call mints a last-call rate, got {rate:?}"
    );
    // The NEXT call shows the labeled rate beside its elapsed.
    h.store.fold.update(|f| {
        let started = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "started",
            "effect": {"type": "llm_call", "payload": {}}
        });
        let _ = f.apply("root", &started);
    });
    let screen = h.turn();
    assert!(
        screen.contains("tok/s (last call)"),
        "rate labeled with its provenance:\n{screen}"
    );
}

#[test]
fn splitless_receipt_never_mints_a_tok_s_rate() {
    // Cycle-3 regression (cycle-2 review P1-A): splitless usage
    // (input==0 && output==0 && total>0, no raw split to repair from)
    // substitutes the call's TOTAL into the sparkline series — the
    // meter's numerator must never read that substitution, or the strip
    // divides prompt+output+reasoning by wall time and OVERSTATES
    // throughput (~130× on a 40k-context call). Splitless → honest
    // absence; a split receipt still mints the output-true rate.
    let mut h = harness();
    h.turn();
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("root".into());
    h.store.fold.update(|f| {
        f.begin_run("root");
        let started = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "started",
            "effect": {"type": "llm_call", "payload": {}}
        });
        let _ = f.apply("root", &started);
    });
    h.turn();
    std::thread::sleep(std::time::Duration::from_millis(80));
    h.store.fold.update(|f| {
        let done = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "hi",
                        "usage": {"input_tokens": 0, "output_tokens": 0,
                                   "total_tokens": 3180}}
        });
        let _ = f.apply("root", &done);
    });
    h.turn();
    assert_eq!(
        h.store.last_call_rate.get_untracked(),
        None,
        "a splitless receipt yields rate ABSENCE, never total/wall-time"
    );
    // The sparkline's total-tokens substitution itself stays (per-call
    // activity is its charter) — only the rate numerator ignores it.
    assert_eq!(
        h.store
            .fold
            .with_untracked(|f| f.stats.output_series.last().copied()),
        Some(3180.0)
    );

    // A SPLIT receipt on the NEXT call mints the output-true rate.
    h.store.fold.update(|f| {
        let started = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "started",
            "effect": {"type": "llm_call", "payload": {}}
        });
        let _ = f.apply("root", &started);
    });
    h.turn();
    std::thread::sleep(std::time::Duration::from_millis(80));
    h.store.fold.update(|f| {
        let done = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "hi",
                        "usage": {"input_tokens": 1000, "output_tokens": 64,
                                   "total_tokens": 1064}}
        });
        let _ = f.apply("root", &done);
    });
    h.turn();
    let rate = h
        .store
        .last_call_rate
        .get_untracked()
        .expect("a split receipt mints a rate");
    // Numerator = 64 output tokens over a ≥80ms window ⇒ ≤800 tok/s.
    // The bound cannot false-fail (a slower machine only shrinks the
    // rate) and catches numerator contamination: input/total (1000/
    // 1064) reads >800 at this window.
    assert!(
        rate > 0.0 && rate <= 800.0,
        "output-true numerator expected, got {rate} tok/s"
    );
}

#[test]
fn question_mark_opens_the_keys_reference() {
    // REST-1: the legend moved behind `?` — the footer names the
    // gesture and it must actually open the reference.
    let mut h = harness();
    h.turn();
    h.type_text("?");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("/sessions [id]"),
        "`?` opens the commands+keys reference:\n{screen}"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { .. })).is_none(),
        "`?` is never sent as a prompt"
    );
}

#[test]
fn ctrl_l_redraw_re_emits_the_full_frame() {
    // HDR-2a: after an EXTERNAL screen clear the engine's model still
    // believes the old cells, so byte-identical repaints emit nothing —
    // the maintainer's blank-header screenshot. Ctrl+L must force real
    // byte re-emission with the final scene unchanged. Since 0.2.6 the
    // mechanism is the engine's `request_full_redraw()` (our 0299):
    // poison-prev + presenter-invalidate, one full-frame emission.
    // (The harness cannot clear the modeled terminal externally;
    // re-emission over an unchanged scene IS the mechanism that heals
    // a cleared one.)
    let mut h = harness();
    h.leave_splash(); // byte-idle asserts cannot run on the animated splash
                      // Settle: drain turns until the app goes byte-idle.
    let mut before = String::new();
    for _ in 0..6 {
        before = h.turn();
    }
    let settled = h
        .driver
        .turn(&mut h.app, &mut h.term)
        .expect("settled turn");
    assert!(!settled.emitted, "app is byte-idle before Ctrl+L");
    // Ctrl+L is the LF-adjacent C0 byte 0x0c on the legacy wire.
    h.term.push_input(&[0x0c]);
    let mut emitted_any = false;
    for _ in 0..4 {
        let t = h.driver.turn(&mut h.app, &mut h.term).expect("turn");
        emitted_any |= t.emitted;
    }
    assert!(emitted_any, "Ctrl+L re-emits bytes on an unchanged scene");
    // The scene is byte-for-byte the same after the full re-emission.
    let after = h.turn();
    assert_eq!(before, after, "redraw never changes the scene");
    let idle = h.driver.turn(&mut h.app, &mut h.term).expect("turn");
    assert!(!idle.emitted, "redraw settles back to byte-idle");
}

#[test]
fn redraw_command_matches_ctrl_l() {
    let mut h = harness();
    h.leave_splash(); // a shimmer tick must not masquerade as /redraw's emission
    for _ in 0..4 {
        h.turn();
    }
    let before = h.turn();
    h.type_text("/redraw");
    h.turn();
    h.press_enter();
    let mut emitted_any = false;
    for _ in 0..4 {
        let t = h.driver.turn(&mut h.app, &mut h.term).expect("turn");
        emitted_any |= t.emitted;
    }
    assert!(emitted_any, "/redraw re-emits");
    assert_eq!(before, h.turn(), "scene unchanged");
}

#[test]
fn composer_hint_renders_while_focused_and_yields_to_typing() {
    // HDR-2c, engine-owned since 0.2.6 (our 0291):
    // `placeholder_while_focused(true)` paints the hint beside the
    // caret while the composer is focused-and-empty — the app-side
    // absolute overlay is deleted.
    let mut h = harness();
    let screen = h.turn();
    assert!(
        screen.contains("describe a task — Enter sends"),
        "hint visible while focused + empty:\n{screen}"
    );
    h.type_text("x");
    let screen = h.turn();
    assert!(
        !screen.contains("describe a task — Enter sends"),
        "hint yields to the draft:\n{screen}"
    );
    // Esc clears the draft; the hint returns.
    h.press_escape();
    let screen = h.turn();
    assert!(
        screen.contains("describe a task — Enter sends"),
        "hint returns when the draft clears:\n{screen}"
    );
}

#[test]
fn gpu_toggle_round_trip_and_footer_render() {
    // OBS-6: /gpu flips Off→Pending + issues GpuEnable; a Ready sample
    // renders on the status bar; /gpu again flips to Off + GpuDisable
    // and the segment disappears. Unsupported renders NOTHING on the
    // bar (the toggle toasts the reason once — never a fake meter).
    let mut h = harness();
    h.turn();
    h.type_text("/gpu");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::GpuEnable)).is_some(),
        "the toggle starts the poller"
    );
    assert!(matches!(
        h.store.gpu.get_untracked(),
        abstractcode::store::GpuMeter::Pending
    ));
    // A sample lands (posted by the poller in production; the store
    // signal is the seam) — the footer renders the percentage.
    h.store.gpu.set(abstractcode::store::GpuMeter::Ready(
        abstractcode::store::GpuSample {
            util_pct: 42.0,
            name: "Apple M5 Max".into(),
        },
    ));
    let screen = h.turn();
    assert!(screen.contains("gpu 42%"), "footer meter:\n{screen}");
    // Unsupported is honest: the segment leaves the bar entirely.
    h.store.gpu.set(abstractcode::store::GpuMeter::Unsupported(
        "host reports no GPU metrics".into(),
    ));
    let screen = h.turn();
    assert!(!screen.contains("gpu 42%"), "no stale meter:\n{screen}");
    // Toggle off from a non-Off state: Off + GpuDisable.
    h.type_text("/gpu");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::GpuDisable)).is_some(),
        "the toggle stops the poller"
    );
    assert!(matches!(
        h.store.gpu.get_untracked(),
        abstractcode::store::GpuMeter::Off
    ));
}

#[test]
fn status_bar_drops_whole_segments_never_self_ellipsis() {
    // POLISH-1: at a width too small for every instrument, the footer
    // drops WHOLE segments right-to-left — the old key legend rendered
    // a fragmented "/help comm…" at 120 cols (SYNTHESIS §2 baseline)
    // and read as broken. The harness is 100 cols; loading every
    // segment (ctx meter + session + gpu + skills + mcp + the ? hint)
    // overflows the left span, so the tail must vanish whole.
    let mut h = harness();
    h.turn();
    h.store.context_window.set(262_144);
    h.store.fold.update(|f| {
        f.begin_run("root");
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "hi",
                        "usage": {"input_tokens": 41_203, "output_tokens": 20}}
        });
        let _ = f.apply("root", &rec);
    });
    // Splitless totals (the coder-run provider shape): the footer shows
    // the honest total, never fabricated "0↑ 0↓".
    h.store.totals.set(abstractcode::store::SessionTotals {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 128_000,
        runs: 3,
    });
    h.store.gpu.set(abstractcode::store::GpuMeter::Ready(
        abstractcode::store::GpuSample {
            util_pct: 42.0,
            name: "Apple M5 Max".into(),
        },
    ));
    h.store
        .selected_skills
        .set(vec!["coredoc".into(), "agora-channels".into()]);
    h.store
        .mcp_servers
        .set(vec![abstractcode::store::McpServer {
            name: "context7".into(),
            url: "https://mcp.example".into(),
            description: String::new(),
            auth_required: false,
        }]);
    let screen = h.turn();
    let footer = screen.lines().last().unwrap_or_default().to_string();
    assert!(
        footer.contains("ctx 41k/262k tk (15%, declared)"),
        "the graded ctx meter keeps its slot:\n{footer}"
    );
    assert!(
        footer.contains("128k tk session") && footer.contains("gpu 42%"),
        "session (splitless: honest total) + gpu fit at 100 cols:\n{footer}"
    );
    assert!(
        !footer.contains('…'),
        "the footer never self-truncates a segment into an ellipsis \
         fragment — overflow drops segments whole:\n{footer}"
    );
    assert!(
        !footer.contains("skills") && !footer.contains("mcp"),
        "overflowing tail segments vanish WHOLE (right-to-left), \
         never as fragments:\n{footer}"
    );
    // The right cluster (theme · host) is never sacrificed to the left.
    assert!(
        footer.contains("127.0.0.1:8080"),
        "gateway host survives on the right:\n{footer}"
    );
}

#[test]
fn header_facts_drop_whole_before_workflow_and_route() {
    // HDR-1 degrade rule: when the middle span tightens, cockpit FACTS
    // drop whole (right-to-left) before workflow/route lose a char —
    // and never as `…` fragments.
    let mut h = harness();
    h.turn();
    h.store
        .selected_skills
        .set(vec!["coredoc".into(), "agora-channels".into()]);
    h.store
        .mcp_servers
        .set(vec![abstractcode::store::McpServer {
            name: "context7".into(),
            url: "https://mcp.example".into(),
            description: String::new(),
            auth_required: false,
        }]);
    h.store.totals.set(abstractcode::store::SessionTotals {
        input_tokens: 100_000,
        output_tokens: 28_000,
        total_tokens: 128_000,
        runs: 3,
    });
    let screen = h.turn();
    let header = screen.lines().next().unwrap_or_default().to_string();
    // Identity facts survive: workflow + route + the leading facts.
    assert!(
        header.contains("basic-agent") && header.contains("gateway defaults"),
        "workflow + route never yield to facts:\n{header}"
    );
    assert!(
        header.contains("⌂ ws") && header.contains("server-managed"),
        "leading facts fill the middle:\n{header}"
    );
    assert!(
        !header.contains('…'),
        "no fact ever renders as an ellipsis fragment — overflow drops \
         facts whole:\n{header}"
    );
    // At 100 cols with this load the tail facts (skills/mcp/tokens)
    // exceed the middle span — they must be ABSENT from the header row
    // (the footer still carries them; the header never fragments).
    assert!(
        !header.contains("skills") && !header.contains("mcp"),
        "overflowing facts drop whole from the header:\n{header}"
    );
    // Session id + orb keep the right edge.
    assert!(
        header.contains("acode-test-session"),
        "session id survives on the right:\n{header}"
    );
}

// ---------------------------------------------------------------------------
// HDR-2 redraw-defect suite (lane C, 2026-07-23): the blank-screen class.
// The harness cannot clear the MODELED terminal externally (CaptureTerm's
// VtScreen is fed only by emitted bytes) — so these tests pin the app's
// ENTRY POINTS into the heal (Ctrl+L root shortcut, Ctrl+L under a modal
// via the action registry, /redraw): forced byte re-emission over an
// UNCHANGED scene. The heal MECHANISM itself is engine-owned since
// abstracttui 0.2.6 (`request_full_redraw` — poison-prev + presenter-
// invalidate + image re-place, pinned engine-side in tests/wave_redraw.rs);
// the old ~5s heartbeat and its pty external-wipe harness
// (scripts/pty_redraw_heal_verify.py) are deleted/SUPERSEDED — no live
// external-wipe proof currently exists (a fresh one would assert Ctrl+L
// full-frame recovery + the focus-gained redraw).
// ---------------------------------------------------------------------------

/// Ctrl+L must work while a MODAL is open: modal trees swallow every key
/// they route (consumed or not) BEFORE root-tree shortcuts, so the root
/// binding alone dies exactly when recovery matters most (a wiped screen
/// with an invisible approval prompt up). The engine's action registry
/// runs LAST, only for keys nothing consumed — `register_global_actions`
/// parks the redraw there.
#[test]
fn ctrl_l_redraws_even_with_a_modal_open() {
    let mut h = harness();
    // Same registration production makes in run_tui.
    ui::register_global_actions(&h.app.actions());
    h.leave_splash(); // the byte-idle assert below races the splash ticker
    h.turn();
    // Open the help modal (any modal exercises the swallow path).
    h.type_text("/help");
    h.turn();
    h.press_enter();
    let mut before = String::new();
    for _ in 0..6 {
        before = h.turn();
    }
    assert!(
        before.contains("/sessions [id]"),
        "help modal is up:\n{before}"
    );
    let settled = h
        .driver
        .turn(&mut h.app, &mut h.term)
        .expect("settled turn");
    assert!(!settled.emitted, "byte-idle with the modal open");
    // Ctrl+L = C0 0x0c on the legacy wire. The modal consumes nothing
    // for it; the action registry must catch it.
    h.term.push_input(&[0x0c]);
    let mut emitted_any = false;
    for _ in 0..4 {
        let t = h.driver.turn(&mut h.app, &mut h.term).expect("turn");
        emitted_any |= t.emitted;
    }
    assert!(
        emitted_any,
        "Ctrl+L re-emits bytes with a modal open (action-registry path)"
    );
    let after = h.turn();
    assert_eq!(before, after, "redraw never changes the scene (modal kept)");
    assert!(
        after.contains("/sessions [id]"),
        "the modal survives the redraw:\n{after}"
    );
}

/// Esc on the approval prompt DEFERS — it must never deny (a dismissal
/// that tells the model "denied" would be a lie about the user's
/// intent). Pins: no Resume command leaves the client on Esc, the wait
/// stays pending, and `d` remains the only deny path.
#[test]
fn approval_escape_defers_never_denies() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record(
                "s-esc",
                "tool_approval:esc",
                serde_json::json!([{"name": "write_file", "arguments": {"path": "x"}}]),
            ),
        );
    });
    let screen = h.turn();
    assert!(screen.contains("approve (a)"), "prompt opens:\n{screen}");
    // Drain the command channel BEFORE Esc so the assertion below can
    // only see commands Esc itself produced.
    while h.rx.try_recv().is_ok() {}
    h.press_escape();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Resume { .. })).is_none(),
        "Esc sends NO resume — the run keeps waiting durably"
    );
    assert!(
        h.store.fold.with_untracked(|f| f.pending_wait.is_some()),
        "the wait is still pending after Esc (deferred, not answered)"
    );
    // And the deny path still exists: reopen (Enter on empty composer),
    // then `d` sends the explicit denial.
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "Enter reopens the deferred prompt:\n{screen}"
    );
    h.type_text("d");
    h.turn();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume { approved, .. }) => {
            assert_eq!(approved, Some(false), "d is the explicit deny")
        }
        other => panic!("expected deny Resume, got {:?}", other.map(|_| "cmd")),
    }
}

// ---------------------------------------------------------------------------
// Cycle-2 integration review (reviewer 3, 2026-07-23): appended pins.
// ---------------------------------------------------------------------------

/// Ctrl+L with BOTH bindings live (root shortcut + action-registry
/// fallback, the production registration) still redraws exactly like
/// the single-binding path: since 0.2.6 the redraw is an idempotent
/// engine REQUEST FLAG (`request_full_redraw`) drained once per turn —
/// a hypothetical double fire coalesces into one full-frame emission,
/// so the old veil-stacking double-fire hazard is structurally gone.
/// Pins: bytes re-emit, no overlay layer is ever parked, the scene is
/// unchanged, and the app settles back to byte-idle.
#[test]
fn ctrl_l_with_both_bindings_redraws_once_and_leaves_no_layers() {
    let mut h = harness();
    // Same registration production makes in run_tui — BOTH bindings live.
    ui::register_global_actions(&h.app.actions());
    h.leave_splash(); // the byte-idle assert below races the splash ticker
    let mut before = String::new();
    for _ in 0..4 {
        before = h.turn();
    }
    assert_eq!(h.ctx.overlays.top_z(), 0, "no overlay before the chord");
    // Ctrl+L = C0 0x0c on the legacy wire; no modal open, so the root
    // shortcut consumes it (the registry runs only for unconsumed keys).
    h.term.push_input(&[0x0c]);
    let mut emitted_any = false;
    for _ in 0..4 {
        let t = h.driver.turn(&mut h.app, &mut h.term).expect("turn");
        emitted_any |= t.emitted;
    }
    assert!(emitted_any, "the redraw re-emits bytes");
    assert_eq!(h.ctx.overlays.top_z(), 0, "no layer parked by the redraw");
    let after = h.turn();
    assert_eq!(before, after, "redraw never changes the scene");
    let idle = h.driver.turn(&mut h.app, &mut h.term).expect("turn");
    assert!(!idle.emitted, "settles back to byte-idle");
}

/// Esc on the ASK-USER modal defers like the approval prompt: the modal
/// closes and STAYS closed (P1-2 regression: a bare close bounced —
/// `wire_wait_modals` re-runs on the close's epoch bump, saw the
/// still-pending, not-dismissed wait, and reopened the prompt in the
/// same flush, making Esc a no-op blink). Enter on the empty composer
/// reopens; no Resume ever leaves the client on Esc.
#[test]
fn ask_escape_defers_and_stays_closed() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &serde_json::json!({
                "run_id": "root", "node_id": "ask", "status": "waiting",
                "step_id": "s-ask-esc",
                "result": {"wait": {"reason": "user",
                    "wait_key": "user:root:ask", "prompt": "Which one?"}}
            }),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("the agent asks"),
        "ask prompt opens:\n{screen}"
    );
    while h.rx.try_recv().is_ok() {}
    h.press_escape();
    // Settle several turns: the defective path reopened on the very
    // next effect flush, so one quiet turn is not proof — drain a few.
    let mut screen = String::new();
    for _ in 0..4 {
        screen = h.turn();
    }
    assert!(
        !screen.contains("the agent asks"),
        "Esc closes the ask prompt and it STAYS closed:\n{screen}"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Resume { .. })).is_none(),
        "Esc sends NO resume — the run keeps waiting durably"
    );
    assert!(
        h.store.fold.with_untracked(|f| f.pending_wait.is_some()),
        "the wait is still pending after Esc (deferred, not answered)"
    );
    // Enter on the empty composer reopens the deferred prompt.
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("the agent asks"),
        "Enter reopens the deferred ask prompt:\n{screen}"
    );
}

/// Operator rulings (2026-07-26; live screenshot: a plan-approval ask
/// truncated mid-sentence with a ledger pointer and NO visible way to
/// respond): an ask renders FULL — scrollable when long, never
/// truncated — and the response affordances (input + hint) stay
/// visible at all times. The prompt scrolls from the modal root's
/// ↑↓/PgUp/PgDn shortcuts while the TextInput KEEPS focus, so typing
/// the answer needs no focus gymnastics.
#[test]
fn long_ask_renders_full_scrollable_with_affordances_always_visible() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    // 60 uniquely-numbered lines (zero-padded: "ask line 01" must never
    // substring-match "ask line 10").
    let prompt = (1..=60)
        .map(|i| format!("ask line {i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &serde_json::json!({
                "run_id": "root", "node_id": "ask", "status": "waiting",
                "step_id": "s-ask-long",
                "result": {"wait": {"reason": "user",
                    "wait_key": "user:root:ask", "prompt": prompt}}
            }),
        );
    });
    let screen = h.turn();
    assert!(screen.contains("the agent asks"), "ask opens:\n{screen}");
    assert!(
        screen.contains("ask line 01"),
        "the question starts at the top:\n{screen}"
    );
    // The response affordances are UNMISSABLE: placeholder + hint render
    // inside the panel, never clipped below it (the old fixed 13-row
    // panel pushed them off the bottom on long asks).
    assert!(
        screen.contains("your answer"),
        "input placeholder visible:\n{screen}"
    );
    assert!(
        screen.contains("Enter answers"),
        "hint row visible:\n{screen}"
    );
    assert!(
        screen.contains("scroll"),
        "the hint advertises scrolling:\n{screen}"
    );
    // NEVER truncated, and no storage internals anywhere on screen.
    assert!(
        !screen.contains("#TRUNCATION"),
        "an ask is never truncated:\n{screen}"
    );
    assert!(!screen.contains("ledger"), "no ledger pointer:\n{screen}");

    // One Down arrow scrolls by a line while the input keeps focus
    // (TextInput leaves ↑↓/PgUp/PgDn unconsumed — the root shortcut
    // fires).
    h.type_text("\x1b[B");
    let screen = h.turn();
    assert!(
        !screen.contains("ask line 01") && screen.contains("ask line 02"),
        "Down scrolls the question one line:\n{screen}"
    );

    // PageDown reaches the very end of the question.
    for _ in 0..5 {
        h.type_text("\x1b[6~");
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("ask line 60"),
        "the full question is reachable:\n{screen}"
    );
    // Affordances survive the scroll to the bottom.
    assert!(
        screen.contains("your answer") && screen.contains("Enter answers"),
        "affordances stay visible at the bottom:\n{screen}"
    );

    // Typing + Enter still answers: focus never left the input.
    while h.rx.try_recv().is_ok() {}
    h.type_text("here you go");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume { payload, .. }) => {
            assert_eq!(
                payload.get("response").and_then(|v| v.as_str()),
                Some("here you go"),
                "the typed answer rides the resume"
            );
        }
        other => panic!("expected Resume, got {:?}", other.map(|_| "cmd")),
    }
}

/// A short ask still fits without a scroll hint and keeps the compact
/// panel — the full-render path must not inflate small prompts.
#[test]
fn short_ask_keeps_affordances_without_scroll_hint() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &serde_json::json!({
                "run_id": "root", "node_id": "ask", "status": "waiting",
                "step_id": "s-ask-short",
                "result": {"wait": {"reason": "user",
                    "wait_key": "user:root:ask", "prompt": "Which one?"}}
            }),
        );
    });
    let screen = h.turn();
    assert!(screen.contains("the agent asks"), "ask opens:\n{screen}");
    assert!(screen.contains("Which one?"), "prompt in full:\n{screen}");
    assert!(
        screen.contains("your answer") && screen.contains("Enter answers"),
        "affordances visible:\n{screen}"
    );
    assert!(
        !screen.contains("PgDn"),
        "no scroll hint when the prompt fits:\n{screen}"
    );
}

// ---------------------------------------------------------------------------
// Cycle-2 presence/density adversarial review (reviewer 2) — regression pins.
// ---------------------------------------------------------------------------

/// P2-C: IDLE-1's contract is "wordmark exactly once" — the header row
/// always carries it, so the gateway-Down recovery card must NOT render
/// its own (the normal branch was deduped; the Down branch had been
/// missed). The recovery teaching itself stays.
#[test]
fn down_state_card_teaches_recovery_with_one_wordmark() {
    let mut h = harness();
    h.turn();
    // Production messages are worded by GwError's kind-aware Display —
    // the splash renders them VERBATIM now (it used to stamp its own
    // "gateway unreachable —" prefix over every Down, timeouts included).
    h.store.conn.set(abstractcode::store::Conn::Down(
        "gateway unreachable: connection refused (os error 61)".into(),
        true,
    ));
    let screen = h.turn();
    assert!(
        screen.contains("gateway unreachable"),
        "recovery block present:\n{screen}"
    );
    assert!(
        screen.contains("abstractgateway serve"),
        "gone-evidence teaches the start command:\n{screen}"
    );
    assert_eq!(
        screen.matches("▲ AbstractCode").count(),
        1,
        "wordmark exactly once, Down state included:\n{screen}"
    );
}

/// HOLE A (comms audit, 2026-07-23): a Down mark born from the SOFT
/// threshold (repeated timeouts — the gateway is running, likely busy)
/// must never claim "unreachable": the splash renders the evidence-worded
/// message verbatim, swaps the start-one advice for a busy explanation,
/// and the status card words the state "not responding".
#[test]
fn soft_down_says_not_responding_never_unreachable() {
    let mut h = harness();
    h.turn();
    h.store.conn.set(abstractcode::store::Conn::Down(
        "gateway timed out: no response in 30s".into(),
        false,
    ));
    let screen = h.turn();
    assert!(
        screen.contains("gateway timed out"),
        "evidence-worded message renders verbatim:\n{screen}"
    );
    assert!(
        !screen.contains("unreachable"),
        "a timeout threshold must not claim unreachable:\n{screen}"
    );
    assert!(
        screen.contains("may be busy"),
        "soft-down advice explains busy instead of teaching start-one:\n{screen}"
    );
    assert!(
        !screen.contains("abstractgateway serve"),
        "start-one advice is gone-evidence-only:\n{screen}"
    );
}

/// P2-D: the durable-pause line owns the strip in ANY focus (like the
/// wait line) — so in entity focus it must name the AGENT lane; entity
/// turns are non-interruptible and never pause, and an unprefixed
/// "run paused" read as the visit being paused.
#[test]
fn paused_strip_names_the_agent_lane_in_entity_focus() {
    let mut h = harness();
    h.turn();
    h.store.convos.update(|cs| {
        let mut c = abstractcode::convo::EntityConvo::opening("castor", "awake");
        c.status = abstractcode::convo::ConvoStatus::Parked;
        cs.push(c);
    });
    h.store
        .focus
        .set(abstractcode::convo::Focus::Entity("castor".into()));
    h.store.paused.set(true);
    let screen = h.turn();
    assert!(
        screen.contains("⏸ agent: run paused durably"),
        "entity focus names the paused LANE:\n{screen}"
    );
    // Agent focus needs no prefix — the lane is unambiguous there.
    h.store.focus.set(abstractcode::convo::Focus::Agent);
    let screen = h.turn();
    assert!(
        screen.contains("⏸ run paused durably") && !screen.contains("agent: run paused"),
        "agent focus keeps the unprefixed line:\n{screen}"
    );
}

/// P1-B (zero-split half): an idle session whose runs never produced a
/// usage receipt renders NO tokens part — "0 in / 0 out tk" would claim
/// a measurement that never happened. Splitless totals keep the honest
/// total; split totals keep the split.
#[test]
fn idle_strip_summary_omits_unmeasured_tokens() {
    let mut h = harness();
    h.turn();
    // Runs counted, zero receipts (e.g. failed before the first call).
    h.store.totals.set(abstractcode::store::SessionTotals {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        runs: 2,
    });
    let screen = h.turn();
    assert!(
        screen.contains("session: 2 runs"),
        "run count renders:\n{screen}"
    );
    assert!(
        !screen.contains("0 in / 0 out"),
        "no fabricated zero split for unmeasured sessions:\n{screen}"
    );
    // Splitless providers: the honest total.
    h.store.totals.set(abstractcode::store::SessionTotals {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 3180,
        runs: 2,
    });
    let screen = h.turn();
    assert!(
        screen.contains("3.2k tk total"),
        "splitless total renders honestly:\n{screen}"
    );
    // Split providers: the split.
    h.store.totals.set(abstractcode::store::SessionTotals {
        input_tokens: 12_000,
        output_tokens: 900,
        total_tokens: 12_900,
        runs: 2,
    });
    let screen = h.turn();
    assert!(
        screen.contains("12k in / 900 out tk"),
        "split totals keep the split:\n{screen}"
    );
}

/// P1-B (run half, live capture frame-02): before the FIRST usage
/// receipt the run strip shows the model-call ticker WITHOUT a token
/// part — "0↑ 0↓ tk" beside "model call 0s" claimed a measurement that
/// had not happened. The split appears with the first receipt.
#[test]
fn run_strip_omits_the_token_split_before_the_first_receipt() {
    let mut h = harness();
    h.turn();
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("root".into());
    h.store.fold.update(|f| f.begin_run("root"));
    h.store.run_started.set(Some(std::time::Instant::now()));
    let screen = h.turn();
    assert!(
        !screen.contains("0↑ 0↓ tk"),
        "no fabricated zero split before the first receipt:\n{screen}"
    );
    h.store.fold.update(|f| {
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "hi",
                        "usage": {"input_tokens": 41_203, "output_tokens": 20}}
        });
        let _ = f.apply("root", &rec);
    });
    let screen = h.turn();
    assert!(
        screen.contains("41k↑ 20↓ tk"),
        "the split renders once measured:\n{screen}"
    );
}

// ---------------------------------------------------------------------------
// Lane-3 conformance forensics (interruptibility wave, 2026-07-23)
// ---------------------------------------------------------------------------

/// Replay a captured pty byte stream through the ENGINE's own VT
/// interpreter (`abstracttui::testing::VtScreen`) and report whether the
/// approval modal is on the final screen. Diagnostic for the T2 phantom-
/// modal investigation: pyte (the python harness's interpreter) and
/// VtScreen disagreeing on the same bytes = harness divergence; both
/// showing the modal = the bytes genuinely lack the close repaint (a
/// real emission gap a user's terminal would show too).
///
/// Ignored by default: needs a capture file. Run ad hoc:
///   ACODE_VT_REPLAY_FILE=/tmp/conf_t2-phantom-bytes.bin \
///     cargo test --release --test headless_ui vt_replay_probe -- --ignored --nocapture
#[test]
#[ignore = "forensic probe: point ACODE_VT_REPLAY_FILE at a pty byte capture"]
fn vt_replay_probe() {
    let path = std::env::var("ACODE_VT_REPLAY_FILE").expect("set ACODE_VT_REPLAY_FILE");
    let bytes = std::fs::read(&path).expect("readable capture file");
    let cols: i32 = std::env::var("ACODE_VT_COLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    let rows: i32 = std::env::var("ACODE_VT_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(44);
    let mut vt = abstracttui::testing::VtScreen::new(abstracttui::prelude::Size::new(cols, rows));
    vt.feed(&bytes);
    let mut hits = Vec::new();
    for y in 0..rows {
        let mut line = String::new();
        for x in 0..cols {
            if let Some(cell) = vt.cell(x, y) {
                line.push_str(cell.display());
            }
        }
        if line.contains("tool approval") || line.contains("approval needed") {
            hits.push(format!("{y:>3}| {}", line.trim_end()));
        }
    }
    println!(
        "vt_replay_probe: {} bytes, {} unknown seq(s), {} modal-needle row(s)",
        bytes.len(),
        vt.unknown_seq_count(),
        hits.len()
    );
    for h in &hits {
        println!("  {h}");
    }
    for sample in vt.unknown_samples() {
        println!("  unknown: {sample:?}");
    }
}

/// The observer scenario (maintainer contract, lane-3 conformance wave):
/// a wait resolved FROM ANOTHER APP must close this client's prompt
/// without any local answer. Mechanism under test: the fold clears
/// `pending_wait` on ANY later record from the waiting run (the ledger
/// shows the run moving past the wait after an external resume) and
/// `wire_wait_modals` closes the open prompt on the None edge. The rule
/// is kind-agnostic — proven here for BOTH kinds, because the live T3
/// scenario (ask_user) is unconstructible on gateways whose tool
/// inventory lacks an ask tool (live finding 2026-07-23: 14 tools, none
/// ask-like). Live proof for the approval kind: scripts/
/// pty_conformance_t2.py (turn 1, external-only resolution).
#[test]
fn wait_resolved_elsewhere_closes_the_modal_without_local_answer() {
    let mut h = harness();
    h.turn();
    let store = h.store;

    // ---- approval kind ----------------------------------------------------
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "act", "status": "waiting", "step_id": "s1",
            "effect": {"type": "tool_calls",
                        "payload": {"tool_calls": [{"name": "write_file", "call_id": "c1"}]}},
            "result": {"wait": {"reason": "user", "wait_key": "tool_approval:k1",
                "details": {"mode": "approval_required",
                             "tool_calls": [{"name": "write_file", "call_id": "c1",
                                              "arguments": {"file_path": "x"}}]}}}
        });
        let _ = f.apply("root", &rec);
    });
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "approval prompt opens:\n{screen}"
    );

    // The ledger shows the run progressing past the wait (what an external
    // resume produces): the tool_calls step completes.
    store.fold.update(|f| {
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "act", "status": "completed", "step_id": "s2",
            "effect": {"type": "tool_calls",
                        "payload": {"tool_calls": [{"name": "write_file", "call_id": "c1"}]}},
            "result": {"results": [{"call_id": "c1", "success": true, "output": "ok"}]}
        });
        let _ = f.apply("root", &rec);
    });
    h.turn(); // effect closes the modal; the deferred retire lands next tick
    let screen = h.turn();
    assert!(
        !screen.contains("approve (a)") && !screen.contains("tool approval"),
        "the approval prompt closes when the wait resolves elsewhere:\n{screen}"
    );
    assert!(
        !screen.contains("approval needed"),
        "the waiting strip clears too:\n{screen}"
    );
    assert!(
        store.fold.with_untracked(|f| f.pending_wait.is_none()),
        "no pending wait survives the resolution"
    );
    // The client never answered locally: no Resume command was sent.
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Resume { .. })).is_none(),
        "no local resume rides an externally-resolved approval"
    );

    // ---- ask kind (same slot, same rule, different modal) ------------------
    store.fold.update(|f| {
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "ask", "status": "waiting", "step_id": "s3",
            "effect": {"type": "tool_calls", "payload": {}},
            "result": {"wait": {"reason": "user", "wait_key": "user:root:ask",
                                 "prompt": "Which color do you want?"}}
        });
        let _ = f.apply("root", &rec);
    });
    let screen = h.turn();
    assert!(
        screen.contains("the agent asks"),
        "ask prompt opens:\n{screen}"
    );

    store.fold.update(|f| {
        let rec = serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "started", "step_id": "s4",
            "effect": {"type": "llm_call", "payload": {}}
        });
        let _ = f.apply("root", &rec);
    });
    h.turn(); // effect closes the modal; the deferred retire lands next tick
    let screen = h.turn();
    assert!(
        !screen.contains("the agent asks"),
        "the ask prompt closes when the wait resolves elsewhere:\n{screen}"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Resume { .. })).is_none(),
        "no local resume rides an externally-answered ask"
    );
}

/// Fresh per-test export dir under the OS temp root (never the cwd — the
/// prefs tests' pollution discipline applied to exports).
fn export_scratch_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "acode-export-headless-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[test]
fn export_command_writes_markdown_and_refuses_overwrite() {
    let mut h = harness();
    h.turn();
    let store = h.store;

    // Empty transcript (no conversation): /export refuses with a notice
    // and writes nothing.
    h.type_text("/export");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("nothing to export")),
        "empty-transcript refusal: {:?}",
        store.notices.get_untracked()
    );

    // Seed one complete turn + a tool, then export to an explicit path.
    store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "write hello.txt".into(),
        });
        f.push_item(abstractcode::transcript::Item::Tool {
            key: "k1".into(),
            name: "write_file".into(),
            args_preview: "{\"path\":\"hello.txt\"}".into(),
            args_full: String::new(),
            status: abstractcode::transcript::ToolStatus::Ok,
            result: "ok".into(),
            error: String::new(),
        });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "done — hello.txt written".into(),
            final_answer: true,
        });
    });
    h.turn();
    let dir = export_scratch_dir();
    let md_path = dir.join("t.md");
    h.type_text(&format!("/export {}", md_path.display()));
    h.turn();
    h.press_enter();
    h.turn();
    let md = std::fs::read_to_string(&md_path).expect("markdown file written");
    assert!(md.starts_with("# AbstractCode transcript"), "{md}");
    assert!(
        md.contains("## User") && md.contains("## Assistant"),
        "{md}"
    );
    assert!(
        md.contains("- session: `acode-test-session`"),
        "header names the session:\n{md}"
    );
    assert!(
        md.contains("- ✓ **write_file**"),
        "default view carries the one-line tool summary:\n{md}"
    );
    assert!(
        !md.contains("```result"),
        "no tool result fences without --details:\n{md}"
    );
    let notices = store.notices.get_untracked();
    assert!(
        notices
            .iter()
            .any(|n| n.contains("exported agent transcript")
                && n.contains("t.md")
                && n.contains("markdown")),
        "success notice names the file: {notices:?}"
    );

    // Same path again: refused, content untouched.
    h.type_text(&format!("/export {}", md_path.display()));
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("never overwrites")),
        "collision refusal: {:?}",
        store.notices.get_untracked()
    );
    assert_eq!(
        std::fs::read_to_string(&md_path).unwrap(),
        md,
        "collision left the original bytes untouched"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn export_jsonl_details_writes_training_lines() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User { text: "q1".into() });
        f.push_item(abstractcode::transcript::Item::Thinking {
            iteration: 1,
            content: String::new(),
            reasoning: "think first".into(),
            call: abstractcode::transcript::CallCost::default(),
        });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "a1".into(),
            final_answer: true,
        });
        // A dangling second prompt (no answer): skipped from the file,
        // counted in the notice.
        f.push_item(abstractcode::transcript::Item::User {
            text: "q2-unanswered".into(),
        });
    });
    h.turn();
    let dir = export_scratch_dir();
    let jl_path = dir.join("t.jsonl");
    h.type_text(&format!("/export jsonl --details {}", jl_path.display()));
    h.turn();
    h.press_enter();
    h.turn();
    let doc = std::fs::read_to_string(&jl_path).expect("jsonl file written");
    let lines: Vec<&str> = doc.lines().collect();
    assert_eq!(lines.len(), 1, "one completed turn = one line:\n{doc}");
    let v: serde_json::Value = serde_json::from_str(lines[0]).expect("line is valid JSON");
    let msgs = v["messages"].as_array().expect("chat schema");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[0]["content"], "q1");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[1]["content"], "a1");
    assert_eq!(
        v["details"]["cycles"][0]["reasoning"], "think first",
        "--details carries the turn's cycles"
    );
    assert!(
        !doc.contains("q2-unanswered"),
        "dangling prompts never enter the file:\n{doc}"
    );
    let notices = store.notices.get_untracked();
    assert!(
        notices.iter().any(|n| n.contains("1 training line(s)")
            && n.contains("1 incomplete turn(s) skipped")
            && n.contains("t.jsonl")),
        "notice counts lines + skipped turns: {notices:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Attachments: /attach + chips + drop-as-paste + custody (design
// untracked/reviews/attachments-design.md)
// ---------------------------------------------------------------------------

fn attach_tempfile(name: &str, bytes: &[u8]) -> (std::path::PathBuf, String) {
    // Unique dir PER FILE: tests run in parallel threads of one process
    // and each cleans its own dir — a shared dir raced (one test's
    // remove_dir_all deleted another's file mid-stage).
    let dir = std::env::temp_dir().join(format!("acode-attach-ui-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    let canon = std::fs::canonicalize(&p).unwrap().display().to_string();
    (dir, canon)
}

/// Push a bracketed-paste sequence (DEC 2004 — how every terminal
/// delivers a file DROP).
fn paste(h: &mut Harness, text: &str) {
    h.term.push_input(b"\x1b[200~");
    h.term.push_input(text.as_bytes());
    h.term.push_input(b"\x1b[201~");
}

#[test]
fn attach_command_stages_chips_and_start_carries_custody() {
    let (dir, path) = attach_tempfile("report.md", b"hello world");
    let mut h = harness();
    h.turn();
    // Stage via /attach <path> (typed args accept absolute paths).
    h.type_text(&format!("/attach {path}"));
    h.turn();
    h.press_enter();
    let screen = h.turn();
    let pending = h.store.pending_attachments.get_untracked();
    assert_eq!(pending.len(), 1, "one chip staged");
    assert_eq!(pending[0].name, "report.md");
    assert!(
        screen.contains("report.md"),
        "chips row renders the staged file:\n{screen}"
    );
    // Duplicate attach refuses (state unchanged).
    h.type_text(&format!("/attach {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        h.store.pending_attachments.with_untracked(|p| p.len()),
        1,
        "duplicate refused"
    );
    // A missing path refuses with a notice, nothing staged.
    h.type_text("/attach /definitely/not/here.txt");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.pending_attachments.with_untracked(|p| p.len()), 1);
    // Send a prompt: Cmd::Start carries the pending list (custody rides
    // to the worker; chips stay until the run STARTS) AND the cap as a
    // UI-thread snapshot — the worker must never read the signal itself
    // (thread stamp panics; verify-pass NEW-1).
    h.store.max_attachment_bytes.set(26_214_400);
    h.type_text("summarize the attached file");
    h.turn();
    h.press_enter();
    h.turn();
    let (sent, cap) = loop {
        match h.rx.try_recv() {
            Ok(Cmd::Start {
                attachments,
                attachment_cap,
                ..
            }) => break (attachments, attachment_cap),
            Ok(_) => continue,
            Err(e) => panic!("expected Cmd::Start, got {e:?}"),
        }
    };
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].path, path);
    assert_eq!(cap, 26_214_400, "the cap rides the command");
    assert_eq!(
        h.store.pending_attachments.with_untracked(|p| p.len()),
        1,
        "chips KEPT until the run starts (custody rule — the assistant's optimistic-clear defect)"
    );
    // Simulate the worker's started post: sent batch leaves, 📎 records.
    abstractcode::runner::clear_sent_attachments(&h.store, &h.ctx.tx.clone(), &sent);
    let screen = h.turn();
    assert_eq!(
        h.store.pending_attachments.with_untracked(|p| p.len()),
        0,
        "started clears the sent batch"
    );
    assert!(
        screen.contains("📎") || screen.contains("report.md"),
        "transcript records what rode the turn:\n{screen}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dropped_path_paste_attaches_and_ctrl_o_undoes() {
    let (dir, path) = attach_tempfile("dropped.txt", b"drop me");
    let mut h = harness();
    h.turn();
    // A real drop: bracketed paste of one existing absolute path.
    paste(&mut h, &path);
    h.turn();
    let screen = h.turn();
    let pending = h.store.pending_attachments.get_untracked();
    assert_eq!(pending.len(), 1, "drop attached directly:\n{screen}");
    assert_eq!(pending[0].name, "dropped.txt");
    // Consumed: the composer draft stays EMPTY (nothing inserted).
    assert!(
        !screen.contains(&path),
        "path text never lands in the composer on a consumed drop:\n{screen}"
    );
    assert!(
        h.store.paste_undo.get_untracked().is_some(),
        "undo slot armed"
    );
    // Ctrl+O: undo — chip out, RAW text back into the draft.
    h.term.push_input(&[0x0f]);
    h.turn();
    let screen = h.turn();
    assert_eq!(
        h.store.pending_attachments.with_untracked(|p| p.len()),
        0,
        "undo removes the chip"
    );
    assert!(
        screen.contains("dropped.txt"),
        "undo restores the pasted path text into the composer:\n{screen}"
    );
    assert!(h.store.paste_undo.get_untracked().is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn prose_paste_inserts_and_never_attaches() {
    let mut h = harness();
    h.turn();
    paste(&mut h, "see /usr/bin for details");
    h.turn();
    let screen = h.turn();
    assert_eq!(
        h.store.pending_attachments.with_untracked(|p| p.len()),
        0,
        "prose never attaches (classifier asymmetry: existence + spelling gates)"
    );
    assert!(
        screen.contains("see /usr/bin for details"),
        "prose paste inserts byte-identical:\n{screen}"
    );
}

#[test]
fn session_boundary_discards_pending_chips_with_notice() {
    let (dir, path) = attach_tempfile("stale.txt", b"x");
    let mut h = harness();
    h.turn();
    h.type_text(&format!("/attach {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.pending_attachments.with_untracked(|p| p.len()), 1);
    h.type_text("/new");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        h.store.pending_attachments.with_untracked(|p| p.len()),
        0,
        "session rotation discards chips (cached refs are session-bound)"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn entity_focus_refuses_attach_and_suppresses_the_drop_hook() {
    let (dir, path) = attach_tempfile("efile.txt", b"x");
    let mut h = harness();
    h.turn();
    h.store
        .focus
        .set(abstractcode::convo::Focus::Entity("castor".into()));
    h.turn();
    // /attach refuses on the entity lane.
    h.type_text(&format!("/attach {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(
        h.store.pending_attachments.with_untracked(|p| p.len()),
        0,
        "entity lane refuses /attach (v1)"
    );
    // A drop inserts as text (no chip, no consume) on the entity lane.
    paste(&mut h, &path);
    h.turn();
    h.turn();
    assert_eq!(
        h.store.pending_attachments.with_untracked(|p| p.len()),
        0,
        "drop hook suppressed outside the agent lane"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn bare_attach_opens_picker_when_nothing_pending() {
    let mut h = harness();
    h.turn();
    h.type_text("/attach");
    h.turn();
    h.press_enter();
    h.turn();
    let screen = h.turn();
    // The engine FilePicker renders a breadcrumb + filter row inside our
    // modal (smoke: the modal exists and shows the start directory).
    assert!(
        h.ctx.modal.borrow().is_some(),
        "bare /attach with nothing pending opens the picker modal:\n{screen}"
    );
}

// ---------------------------------------------------------------------------
// Activity strip: the cycle gist is ATTRIBUTED to the cycle it came from
// (operator report 2026-08-21 — cycle 2 was quoting cycle 1's words)
// ---------------------------------------------------------------------------

/// One `reason` llm_call record, in the ledger's own shape.
fn reason_record_for(run: &str, status: &str, content: &str) -> Value {
    let mut rec = serde_json::json!({
        "run_id": run,
        "step_id": format!("step-{run}-{status}-{}", content.len()),
        "node_id": "reason",
        "status": status,
        "effect": {"type": "llm_call"},
        "started_at": "2026-08-21T00:00:00Z",
    });
    if status == "completed" {
        rec["result"] = serde_json::json!({
            "content": content,
            "reasoning": "",
            "tool_calls": [],
        });
        rec["ended_at"] = serde_json::json!("2026-08-21T00:00:10Z");
    }
    rec
}

fn reason_record(status: &str, content: &str) -> Value {
    reason_record_for("root", status, content)
}

/// The activity strip's line (the one naming the live cycle).
fn strip_line(screen: &str, needle: &str) -> String {
    screen
        .lines()
        .find(|l| l.contains(needle))
        .unwrap_or_default()
        .to_string()
}

#[test]
fn the_strip_never_shows_one_cycles_words_as_another_cycles_thinking() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));

    // Cycle 1 runs and finishes with its own words.
    for rec in [
        reason_record("started", "one"),
        reason_record(
            "completed",
            "I'll inspect the project structure to locate the game code.",
        ),
    ] {
        store.fold.update(|f| {
            let _ = f.apply("root", &rec);
        });
    }
    let screen = h.turn();
    assert!(
        screen.contains("thinking (cycle 1) — “I'll inspect the project structure"),
        "a cycle's OWN gist rides an em-dash:\n{screen}"
    );

    // Cycle 2 starts: its words do not exist yet (the ledger carries
    // them only in the RESULT record), so cycle 1's line must not be
    // presented as cycle 2's thinking.
    store.fold.update(|f| {
        let _ = f.apply("root", &reason_record("started", "two"));
    });
    let screen = h.turn();
    assert!(
        screen.contains("thinking (cycle 2)"),
        "the strip names the live cycle:\n{screen}"
    );
    assert!(
        !screen.contains("thinking (cycle 2) — “I'll inspect"),
        "cycle 1's words are NOT rendered as cycle 2's intent:\n{screen}"
    );
    assert!(
        screen.contains("last: “I'll inspect"),
        "they are still shown, marked as the lane's LAST words, not this cycle's:\n{screen}"
    );

    // Cycle 2 finishes: ITS words take over, on the em-dash.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &reason_record(
                "completed",
                "I found an empty workspace, so I need to create the game.",
            ),
        );
    });
    let screen = h.turn();
    let strip = screen
        .lines()
        .find(|l| l.contains("thinking (cycle 2)"))
        .unwrap_or_default();
    assert!(
        strip.contains("thinking (cycle 2) — “I found an empty workspace"),
        "the newest cycle's own words lead once they exist:\n{screen}"
    );
    assert!(
        !strip.contains("I'll inspect"),
        "and the older gist is off the strip (the transcript still has it):\n{strip}"
    );

    // A TOOL-ONLY cycle (result carries calls, no prose) contributes no
    // words. The lane's last real words must survive it — an intent
    // label that goes dark for every tool cycle is dark most of the run.
    for rec in [
        reason_record("started", "three"),
        reason_record("completed", ""),
        reason_record("started", "four"),
    ] {
        store.fold.update(|f| {
            let _ = f.apply("root", &rec);
        });
    }
    let screen = h.turn();
    let strip = strip_line(&screen, "thinking (cycle 4)");
    assert!(
        strip.contains("last: “I found an empty workspace"),
        "the lane's last real words survive a tool-only cycle:\n{strip}"
    );
}

#[test]
fn another_runs_words_are_never_shown_as_this_lanes_thinking() {
    // `cycles` counts PER RUN while the displayed cycle number is a max
    // ACROSS runs, so comparing a per-run number against it attributes
    // one run's words to another run's cycle (adversary finding P1,
    // 2026-08-21). The gist must belong to the run that is cycling.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));

    for rec in [
        reason_record_for("root", "started", "a"),
        reason_record_for("root", "completed", "ROOT-ONE reading the repository"),
        reason_record_for("root", "started", "b"),
    ] {
        store.fold.update(|f| {
            let _ = f.apply("root", &rec);
        });
    }
    let screen = h.turn();
    assert!(
        strip_line(&screen, "thinking (cycle 2)").contains("last: “ROOT-ONE"),
        "the cycling lane's own last words show:\n{screen}"
    );

    // A DELEGATE child's cycle result lands while the root is still the
    // cycling lane (the partial-replay shape: a completed whose started
    // never folded). Its words are not the root's intent.
    store.fold.update(|f| {
        let _ = f.apply(
            "child",
            &reason_record_for("child", "completed", "SUBAGENT-WORDS grepping for tests"),
        );
    });
    let screen = h.turn();
    let strip = strip_line(&screen, "thinking (cycle 2)");
    assert!(
        !strip.contains("SUBAGENT-WORDS"),
        "another run's words never label this lane's cycle:\n{strip}"
    );
    assert!(
        strip.contains("last: “ROOT-ONE"),
        "and the lane's own words survive it:\n{strip}"
    );
}

// ---------------------------------------------------------------------------
// Attachment PREVIEW: `/attach preview` + the manager's p/Enter — the
// file's real bytes (text documents and PNG/JPEG pictures) drawn before
// they ride a run. The loader runs on the worker (Cmd::LoadPreview), so
// each test drives BOTH halves: the command the UI emits, and the body
// the worker posts back through `runner::apply_preview`.
// ---------------------------------------------------------------------------

/// Pull the LoadPreview command the UI just emitted (path + seq).
fn take_load_preview(h: &mut Harness) -> (u64, String) {
    match h.find_cmd(|c| matches!(c, Cmd::LoadPreview { .. })) {
        Some(Cmd::LoadPreview { seq, path }) => (seq, path),
        other => panic!("expected Cmd::LoadPreview, got {other:?}"),
    }
}

/// Run the REAL loader (the worker's body) and post it back the way the
/// worker's wake closure does.
fn deliver_preview(h: &Harness, seq: u64, path: &str) {
    let body = abstractcode::preview::load(path);
    abstractcode::runner::apply_preview(&h.store, seq, body);
}

#[test]
fn attach_preview_shows_a_text_document_with_line_numbers() {
    let (dir, path) = attach_tempfile(
        "notes.md",
        b"# Heading\n\nthe second paragraph\nlast line\n",
    );
    let mut h = harness();
    h.turn();
    h.type_text(&format!("/attach preview {path}"));
    h.turn();
    h.press_enter();
    let screen = h.turn();
    // The modal opens IMMEDIATELY on the loading body — a slow decode
    // must never block the frame that opened the preview.
    assert!(h.ctx.modal.borrow().is_some(), "preview modal opens");
    assert!(
        screen.contains("reading"),
        "loading state is visible while the worker reads:\n{screen}"
    );
    let (seq, cmd_path) = take_load_preview(&mut h);
    assert_eq!(cmd_path, path, "the command names the canonical path");
    deliver_preview(&h, seq, &path);
    let screen = h.turn();
    assert!(
        screen.contains("notes.md"),
        "header names the file:\n{screen}"
    );
    assert!(screen.contains("text"), "header names the kind:\n{screen}");
    assert!(
        screen.contains("# Heading") && screen.contains("last line"),
        "the document's own words render:\n{screen}"
    );
    // The GUTTER renders, not just the row text: the numbers are the
    // preview's index into the file (4 lines here, so one column).
    assert!(
        screen.contains("1 # Heading") && screen.contains("4 last line"),
        "line numbers render beside their lines:\n{screen}"
    );
    assert!(
        screen.contains("row 1/"),
        "the hint row places you in the document:\n{screen}"
    );
    // Nothing was staged: preview is a look, not an attach.
    assert_eq!(h.store.pending_attachments.with_untracked(|p| p.len()), 0);
    h.press_escape();
    h.turn();
    assert!(h.ctx.modal.borrow().is_none(), "Esc closes the preview");
    assert!(
        h.store.preview.with_untracked(|p| p.is_none()),
        "closing drops the body — no bitmap or document outlives the modal"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn attach_preview_draws_a_picture_and_names_its_pixels() {
    let img = abstracttui::widgets::Bitmap::from_fn(64, 32, |x, y| {
        Rgba::rgb((x * 4) as u8, (y * 8) as u8, 120)
    });
    let (dir, path) = attach_tempfile("shot.png", &abstracttui::gfx::png_encode::encode(&img));
    let mut h = harness();
    h.turn();
    h.type_text(&format!("/attach preview {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    let (seq, p) = take_load_preview(&mut h);
    deliver_preview(&h, seq, &p);
    let screen = h.turn();
    assert!(
        screen.contains("PNG 64×32"),
        "the header names the format and the file's TRUE pixel size:\n{screen}"
    );
    // The mosaic painted: the body rows carry non-space cells that are
    // not part of the header or the hint.
    let body: String = screen
        .lines()
        .skip_while(|l| !l.contains("PNG 64×32"))
        .skip(1)
        .take_while(|l| !l.contains("Esc closes"))
        .collect();
    assert!(
        body.chars().any(|c| !c.is_whitespace()),
        "the picture itself is drawn, not an empty box:\n{screen}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_format_the_engine_cannot_draw_is_named_and_the_attachment_still_stands() {
    // WebP: still outside the decoder family (engine 0.6.0 moved GIF
    // INSIDE it — a GIF here would now exercise the decoder-error path
    // instead; that path is pinned in preview::tests).
    let (dir, path) = attach_tempfile("shot.webp", b"RIFF\x24\x00\x00\x00WEBPVP8 ");
    let mut h = harness();
    h.turn();
    // Stage it FIRST: the refusal must never read as "your attachment
    // is broken" — the file uploads perfectly well.
    h.type_text(&format!("/attach {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.pending_attachments.with_untracked(|p| p.len()), 1);
    h.type_text("/attach preview");
    h.turn();
    h.press_enter();
    h.turn();
    let (seq, p) = take_load_preview(&mut h);
    deliver_preview(&h, seq, &p);
    let screen = h.turn();
    assert!(screen.contains("WebP"), "the FORMAT is named:\n{screen}");
    assert!(
        screen.contains("attaches"),
        "and the attachment is explicitly still fine:\n{screen}"
    );
    assert_eq!(
        h.store.pending_attachments.with_untracked(|p| p.len()),
        1,
        "previewing never unstages anything"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_manager_previews_the_chip_under_the_cursor() {
    let (dir_a, first) = attach_tempfile("first.txt", b"alpha content\n");
    let (dir_b, second) = attach_tempfile("second.txt", b"beta content\n");
    let mut h = harness();
    h.turn();
    for path in [&first, &second] {
        h.type_text(&format!("/attach {path}"));
        h.turn();
        h.press_enter();
        h.turn();
    }
    assert_eq!(h.store.pending_attachments.with_untracked(|p| p.len()), 2);
    // Bare /attach with chips staged opens the manager.
    h.type_text("/attach");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("Enter/p preview"),
        "the manager teaches the key:\n{screen}"
    );
    // ↓ to the second chip, then p.
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.term.push_input(b"p");
    h.turn();
    let (seq, path) = take_load_preview(&mut h);
    assert_eq!(path, second, "the CURSOR's chip previews, not the first");
    deliver_preview(&h, seq, &path);
    let screen = h.turn();
    assert!(
        screen.contains("beta content"),
        "the selected file's content renders:\n{screen}"
    );
    std::fs::remove_dir_all(&dir_a).ok();
    std::fs::remove_dir_all(&dir_b).ok();
}

#[test]
fn a_stale_load_never_repaints_a_newer_preview() {
    let (dir_a, first) = attach_tempfile("slow.txt", b"the file you left\n");
    let (dir_b, second) = attach_tempfile("quick.txt", b"the file you asked for\n");
    let mut h = harness();
    h.turn();
    h.type_text(&format!("/attach preview {first}"));
    h.turn();
    h.press_enter();
    h.turn();
    let (stale_seq, stale_path) = take_load_preview(&mut h);
    // Leave the first preview before its loader answers (the modal traps
    // focus, so the composer is only reachable once it closes) and open
    // the second.
    h.press_escape();
    h.turn();
    h.type_text(&format!("/attach preview {second}"));
    h.turn();
    h.press_enter();
    h.turn();
    let (live_seq, live_path) = take_load_preview(&mut h);
    assert_ne!(stale_seq, live_seq, "each preview mints its own seq");
    // The SLOW loader lands last — and must be dropped on the floor.
    deliver_preview(&h, live_seq, &live_path);
    deliver_preview(&h, stale_seq, &stale_path);
    let screen = h.turn();
    assert!(
        screen.contains("the file you asked for"),
        "the newer preview owns the modal:\n{screen}"
    );
    assert!(
        !screen.contains("the file you left"),
        "the stale body never repaints a newer subject:\n{screen}"
    );
    std::fs::remove_dir_all(&dir_a).ok();
    std::fs::remove_dir_all(&dir_b).ok();
}

#[test]
fn preview_scrolls_a_long_document_and_says_where_you_are() {
    let mut body = String::new();
    for i in 1..=400 {
        body.push_str(&format!("line {i} of the document\n"));
    }
    let (dir, path) = attach_tempfile("long.log", body.as_bytes());
    let mut h = harness();
    h.turn();
    h.type_text(&format!("/attach preview {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    let (seq, p) = take_load_preview(&mut h);
    deliver_preview(&h, seq, &p);
    let screen = h.turn();
    assert!(screen.contains("line 1 of the document"), "{screen}");
    assert!(
        screen.contains("400 lines"),
        "the header counts them:\n{screen}"
    );
    // End jumps to the tail; the hint row moves with it.
    h.term.push_input(b"\x1b[F");
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("line 400 of the document"),
        "End reaches the last line:\n{screen}"
    );
    assert!(
        !screen.contains("row 1/400"),
        "the position indicator moved:\n{screen}"
    );
    // Home comes back.
    h.term.push_input(b"\x1b[H");
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("row 1/400"),
        "Home returns to the top:\n{screen}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn preview_of_a_missing_path_notifies_and_opens_nothing() {
    let mut h = harness();
    h.turn();
    h.type_text("/attach preview /definitely/not/here.txt");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        h.ctx.modal.borrow().is_none(),
        "no modal over a file that does not exist:\n{screen}"
    );
    let notices = h.store.notices.get_untracked();
    assert!(
        notices.iter().any(|n| n.contains("no such file")),
        "the refusal says why: {notices:?}"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadPreview { .. }))
            .is_none(),
        "nothing is dispatched for a path that does not exist"
    );
    assert!(h.store.preview.with_untracked(|p| p.is_none()));
}

/// Locate a name ON THE CHIPS ROW. Scanning the whole screen finds the
/// attach TOAST first whenever one is still up ("attached notes.md —
/// rides your next message"), which is not a click target and made
/// these tests depend on toast timing.
fn locate_chip(screen: &str, needle: &str) -> Option<(usize, usize)> {
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains('\u{1f4ce}'))?;
    let byte_col = line.find(needle)?;
    Some((row, abstracttui::text::width(&line[..byte_col]) as usize))
}

/// The chips row as rendered (empty when no chips are staged).
fn chips_row_text(screen: &str) -> String {
    screen
        .lines()
        .find(|l| l.contains('\u{1f4ce}'))
        .unwrap_or_default()
        .to_string()
}

/// Press + release a left button at a 0-based CELL position.
fn click_cell(h: &mut Harness, row: usize, col: usize) {
    let (x, y) = (col + 1, row + 1);
    h.term.push_input(format!("\x1b[<0;{x};{y}M").as_bytes());
    h.turn();
    h.term.push_input(format!("\x1b[<0;{x};{y}m").as_bytes());
    h.turn();
    h.turn();
}

#[test]
fn clicking_a_chip_opens_its_preview() {
    // THREE chips, and the click lands on the MIDDLE one: with a single
    // chip staged, "always preview the first" would pass this test.
    let (dir0, other0) = attach_tempfile("first_one.md", b"# not this one\n");
    let (dir, path) = attach_tempfile("clickable.md", b"# clicked open\nbody\n");
    let (dir2, other2) = attach_tempfile("third_one.md", b"# nor this one\n");
    let mut h = harness();
    h.turn();
    for p in [&other0, &path, &other2] {
        h.type_text(&format!("/attach {p}"));
        h.turn();
        h.press_enter();
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("clickable.md"),
        "the chip renders:\n{screen}"
    );
    // The chips row teaches nothing in prose — the NAME is the
    // affordance, so it must be the thing that responds to a click.
    assert!(
        !chips_row_text(&screen).contains("/attach preview looks inside"),
        "no instructional tail on the chips row:\n{screen}"
    );
    let (row, col) = locate_chip(&screen, "clickable.md").expect("chip on the row");
    click_cell(&mut h, row, col + 2);
    let screen = h.turn();
    assert!(
        h.ctx.modal.borrow().is_some(),
        "the click opened a modal:\n{screen}"
    );
    let (seq, clicked) = take_load_preview(&mut h);
    assert_eq!(clicked, path, "the clicked chip is the one previewed");
    deliver_preview(&h, seq, &clicked);
    let screen = h.turn();
    assert!(screen.contains("# clicked open"), "{screen}");
    // Still staged: a preview is a look.
    assert_eq!(h.store.pending_attachments.with_untracked(|p| p.len()), 3);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&dir0).ok();
    std::fs::remove_dir_all(&dir2).ok();
}

#[test]
fn clicking_beside_a_chip_does_nothing() {
    let (dir, path) = attach_tempfile("quiet.md", b"nothing happens\n");
    let mut h = harness();
    h.turn();
    h.type_text(&format!("/attach {path}"));
    h.turn();
    h.press_enter();
    let screen = h.turn();
    let (row, col) = locate_chip(&screen, "quiet.md").expect("chip on the row");
    // Two cells PAST the end of the chip label (past "quiet.md (16 B)").
    let past = col + abstracttui::text::width("quiet.md (16.0 B)") as usize + 6;
    click_cell(&mut h, row, past);
    h.turn();
    assert!(
        h.ctx.modal.borrow().is_none(),
        "only the chip itself is clickable"
    );
    assert!(h
        .find_cmd(|c| matches!(c, Cmd::LoadPreview { .. }))
        .is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_long_filename_is_capped_on_the_row_and_still_clickable() {
    // One long name must not own the row that shows every staged file.
    let (dir, path) = attach_tempfile("Screenshot 2026-08-21 at 4.37.29 AM.png", b"img\n");
    // Unique name: `attach_tempfile` keys its directory on the FILE
    // NAME, so two tests sharing one races (each removes the other's
    // directory at the end).
    let (dir2, short) = attach_tempfile("row_notes.md", b"short\n");
    let mut h = harness();
    h.turn();
    for p in [&path, &short] {
        h.type_text(&format!("/attach {p}"));
        h.turn();
        h.press_enter();
        h.turn();
    }
    let screen = h.turn();
    let chips = chips_row_text(&screen);
    assert!(
        chips.contains("Screenshot 2026-08-2…"),
        "the name is cut at 20 characters plus an ellipsis:\n{chips}"
    );
    assert!(
        !chips.contains("4.37.29 AM.png"),
        "the tail of the name is not on the row:\n{chips}"
    );
    assert!(
        chips.contains("row_notes.md (6 B)"),
        "a short name is untouched:\n{chips}"
    );
    // The cap is display only — the chip still previews ITS file, and
    // the preview header spells the whole name out.
    let (row, col) = locate_chip(&screen, "Screenshot").expect("chip on the row");
    click_cell(&mut h, row, col + 2);
    h.turn();
    let (seq, clicked) = take_load_preview(&mut h);
    assert_eq!(clicked, path, "the capped chip still names its own file");
    deliver_preview(&h, seq, &clicked);
    let screen = h.turn();
    assert!(
        screen.contains("Screenshot 2026-08-21 at 4.37.29 AM.png"),
        "the preview header carries the FULL name:\n{screen}"
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&dir2).ok();
}

#[test]
fn clicking_the_x_unstages_that_chip_and_only_that_one() {
    let (dir_a, first) = attach_tempfile("keep_me.txt", b"kept\n");
    let (dir_b, second) = attach_tempfile("drop_me.txt", b"dropped\n");
    let mut h = harness();
    h.turn();
    for path in [&first, &second] {
        h.type_text(&format!("/attach {path}"));
        h.turn();
        h.press_enter();
        h.turn();
    }
    let screen = h.turn();
    assert_eq!(h.store.pending_attachments.with_untracked(|p| p.len()), 2);
    // The × sits one cell past the end of the chip it belongs to.
    let (row, col) = locate_chip(&screen, "drop_me.txt").expect("chip on the row");
    let x_col = col + abstracttui::text::width("drop_me.txt (8 B)") as usize + 1;
    click_cell(&mut h, row, x_col);
    let screen = h.turn();
    let left = h.store.pending_attachments.get_untracked();
    assert_eq!(left.len(), 1, "one chip removed:\n{screen}");
    assert_eq!(left[0].name, "keep_me.txt", "the OTHER chip survived");
    assert!(
        !chips_row_text(&screen).contains("drop_me.txt"),
        "the row reflects it immediately:\n{screen}"
    );
    assert!(
        h.ctx.modal.borrow().is_none(),
        "removing never opens the preview:\n{screen}"
    );
    std::fs::remove_dir_all(&dir_a).ok();
    std::fs::remove_dir_all(&dir_b).ok();
}

#[test]
fn a_name_too_long_for_the_row_is_truncated_not_dropped() {
    // Dropping it left the row reading "📎  · +1 more": a staged file
    // that WILL ride the next send, with no name and a separator
    // separating nothing (adversary finding P2, 2026-08-21).
    let (dir, path) = attach_tempfile(
        "2026-08-20-composer-crush-and-caret-clip-and-more-words.md",
        b"x",
    );
    // Narrow enough that even the 20-character name cap leaves the row
    // short — this is the ROW's own truncation, one layer below the cap.
    let mut h = harness_sized(Size::new(30, 20));
    h.turn();
    h.type_text(&format!("/attach {path}"));
    h.turn();
    h.press_enter();
    let screen = h.turn();
    let chips = chips_row_text(&screen);
    assert!(
        chips.contains("2026-08-20-composer"),
        "the name still renders, ellipsized:\n{chips}"
    );
    assert!(
        !chips.contains("· +"),
        "and no tail separates nothing:\n{chips}"
    );
    // Still clickable where it is drawn.
    let (row, col) = locate_chip(&screen, "2026-08-20").expect("chip on the row");
    click_cell(&mut h, row, col + 2);
    h.turn();
    assert!(
        h.ctx.modal.borrow().is_some(),
        "a truncated chip is still the preview target"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn preview_by_index_picks_the_named_chip() {
    let (dir_a, first) = attach_tempfile("index_one.txt", b"the first file\n");
    let (dir_b, second) = attach_tempfile("index_two.txt", b"the second file\n");
    let mut h = harness();
    h.turn();
    for path in [&first, &second] {
        h.type_text(&format!("/attach {path}"));
        h.turn();
        h.press_enter();
        h.turn();
    }
    h.type_text("/attach preview 2");
    h.turn();
    h.press_enter();
    h.turn();
    let (seq, path) = take_load_preview(&mut h);
    assert_eq!(path, second, "the index names the chip, 1-based");
    deliver_preview(&h, seq, &path);
    let screen = h.turn();
    assert!(screen.contains("the second file"), "{screen}");
    std::fs::remove_dir_all(&dir_a).ok();
    std::fs::remove_dir_all(&dir_b).ok();
}

#[test]
fn a_cut_text_preview_says_it_was_cut() {
    // Bigger than the 512 KB text cap: the header must state the cut —
    // showing half a file without saying so is the ADR 0001 failure.
    let mut body = String::new();
    let mut n = 1;
    while body.len() < 600 * 1024 {
        body.push_str(&format!("line {n} of a very long log file\n"));
        n += 1;
    }
    let (dir, path) = attach_tempfile("huge.log", body.as_bytes());
    let mut h = harness();
    h.turn();
    h.type_text(&format!("/attach preview {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    let (seq, p) = take_load_preview(&mut h);
    deliver_preview(&h, seq, &p);
    let screen = h.turn();
    assert!(
        screen.contains("showing the first 512.0 KB of"),
        "the cut is named in the header:\n{screen}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn shrinking_the_terminal_re_wraps_instead_of_clipping_the_text() {
    // The engine re-clamps a modal to the viewport on resize. Wrapping
    // against the size the modal ASKED for would ellipsize every row
    // and say nothing — the horizontal twin of a silent truncation.
    let line = "AAAAAAAA BBBBBBBB CCCCCCCC DDDDDDDD EEEEEEEE FFFFFFFF GGGGGGGG HHHHHHHH";
    let (dir, path) = attach_tempfile("wide.txt", format!("{line}\n{line}\n").as_bytes());
    let mut h = harness_sized(Size::new(100, 30));
    h.turn();
    h.type_text(&format!("/attach preview {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    let (seq, p) = take_load_preview(&mut h);
    deliver_preview(&h, seq, &p);
    let screen = h.turn();
    assert_eq!(
        screen.matches("HHHHHHHH").count(),
        2,
        "wide terminal shows both lines whole:\n{screen}"
    );
    // Now shrink to half width. (The capture buffer keeps its old
    // width, so cells past the new viewport hold stale paint — read
    // only the columns the app now owns.)
    h.term.push_resize(Size::new(50, 30));
    h.turn();
    let screen = h.turn();
    let live: String = screen
        .lines()
        .map(|l| l.chars().take(50).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        live.matches("HHHHHHHH").count(),
        2,
        "after the shrink each line's tail is still reachable — wrapped, not cut:\n{live}"
    );
    assert!(
        live.contains("row 1/4"),
        "the wrap is re-measured, so the row count reflects the narrow width:\n{live}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_tiny_terminal_still_shows_some_of_the_file() {
    // A modal cannot ask for more room than the terminal has (the
    // engine clamps it), and an over-padded layout can leave ZERO body
    // rows while the hint still claims "row 1/3".
    let (dir, path) = attach_tempfile("tiny.txt", b"alpha\nbeta\ngamma\n");
    let mut h = harness_sized(Size::new(20, 8));
    h.turn();
    h.type_text(&format!("/attach preview {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    let (seq, p) = take_load_preview(&mut h);
    deliver_preview(&h, seq, &p);
    let screen = h.turn();
    assert!(
        screen.contains("alpha"),
        "the file's first line renders even on an 20x8 terminal:\n{screen}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn another_modal_taking_the_slot_frees_the_preview_body() {
    let img = abstracttui::widgets::Bitmap::from_fn(32, 16, |_, _| Rgba::rgb(9, 9, 9));
    let (dir, path) = attach_tempfile("held.png", &abstracttui::gfx::png_encode::encode(&img));
    let mut h = harness();
    h.turn();
    h.type_text(&format!("/attach preview {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    let (seq, p) = take_load_preview(&mut h);
    deliver_preview(&h, seq, &p);
    h.turn();
    assert!(h.store.preview.with_untracked(|p| p.is_some()));
    // Any other modal replaces this one — Esc is not the only exit, and
    // a decoded bitmap must not outlive the modal that showed it.
    abstractcode::ui::modals::open_help(h.cx, &h.ctx);
    h.turn();
    assert!(
        h.store.preview.with_untracked(|p| p.is_none()),
        "the preview body is dropped when its modal is replaced"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn opening_a_second_preview_never_wipes_the_one_that_replaced_it() {
    // The replaced modal's cleanup must not clear the NEWER preview it
    // was replaced by (the seq guard on the cleanup).
    let (dir_a, first) = attach_tempfile("one.txt", b"the first file\n");
    let (dir_b, second) = attach_tempfile("two.txt", b"the second file\n");
    let mut h = harness();
    h.turn();
    h.type_text(&format!("/attach preview {first}"));
    h.turn();
    h.press_enter();
    h.turn();
    let _ = take_load_preview(&mut h);
    // Straight into the second preview via the UI entry point (the
    // composer is behind the modal's focus trap).
    abstractcode::ui::preview::open_path(h.cx, h.store, &h.ctx, &second);
    h.turn();
    let (seq, path) = take_load_preview(&mut h);
    assert_eq!(path, second);
    deliver_preview(&h, seq, &path);
    let screen = h.turn();
    assert!(
        h.store.preview.with_untracked(|p| p.is_some()),
        "the newer preview survives the older modal's cleanup"
    );
    assert!(screen.contains("the second file"), "{screen}");
    std::fs::remove_dir_all(&dir_a).ok();
    std::fs::remove_dir_all(&dir_b).ok();
}

#[test]
fn preview_works_in_an_entity_lane_because_it_stages_nothing() {
    // The attachment lane guard refuses entity focus because chips ride
    // AGENT runs. Preview stages nothing and sends nothing, so that
    // reason is not true of it.
    let (dir, path) = attach_tempfile("lane.txt", b"readable anywhere\n");
    let mut h = harness();
    h.turn();
    h.store
        .focus
        .set(abstractcode::convo::Focus::Entity("someone".into()));
    h.turn();
    h.type_text(&format!("/attach preview {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    let (seq, p) = take_load_preview(&mut h);
    deliver_preview(&h, seq, &p);
    let screen = h.turn();
    assert!(screen.contains("readable anywhere"), "{screen}");
    let notices = h.store.notices.get_untracked();
    assert!(
        !notices
            .iter()
            .any(|n| n.contains("attachments ride agent runs only")),
        "the attachment-lane refusal never fires for a look: {notices:?}"
    );
    // Staging one there still refuses, unchanged.
    h.type_text(&format!("/attach {path}"));
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.pending_attachments.with_untracked(|p| p.len()), 0);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Attachments: the custody/undo lane the design ordered tested hardest
// (impl-review P1-5; P1-1/P1-2 regressions probe-shaped)
// ---------------------------------------------------------------------------

/// P1-1 regression: a drop whose SPELLING crosses a symlink
/// (macOS /tmp → /private/tmp) must still undo — the undo slot keys on
/// the CANONICAL paths the chips carry, never the pasted spelling.
#[test]
fn drop_through_a_symlinked_prefix_still_undoes() {
    // std::env::temp_dir() on macOS lives under the /var symlink; use
    // the /tmp spelling explicitly so the test exercises the class on
    // every platform where it exists (elsewhere it degrades to the
    // plain undo test — still valid).
    let dir = format!("/tmp/acode-symlink-undo-{}", std::process::id());
    std::fs::create_dir_all(&dir).unwrap();
    let spelled = format!("{dir}/sym.txt");
    std::fs::write(&spelled, b"x").unwrap();
    let canonical = std::fs::canonicalize(&spelled)
        .unwrap()
        .display()
        .to_string();

    let mut h = harness();
    h.turn();
    paste(&mut h, &spelled);
    h.turn();
    h.turn();
    let pending = h.store.pending_attachments.get_untracked();
    assert_eq!(pending.len(), 1, "drop attached");
    assert_eq!(pending[0].path, canonical, "chips store canonical paths");
    let (_, undo_paths) = h.store.paste_undo.get_untracked().expect("undo armed");
    assert_eq!(
        undo_paths,
        vec![canonical],
        "undo slot keys on the canonical path the chip carries"
    );
    h.term.push_input(&[0x0f]); // Ctrl+O
    h.turn();
    let screen = h.turn();
    assert_eq!(
        h.store.pending_attachments.with_untracked(|p| p.len()),
        0,
        "undo removed the chip across the symlink spelling:\n{screen}"
    );
    assert!(
        screen.contains("sym.txt"),
        "path text restored into the composer:\n{screen}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// P1-2 regression: once the dropped chips RODE a run, Ctrl+O must do
/// nothing — no chip changes, no stale path text injected, no "drop
/// undone" claim (the artifact is permanent server-side).
#[test]
fn ctrl_o_after_the_chips_rode_a_run_is_a_dead_key() {
    let (dir, path) = attach_tempfile("sent.txt", b"x");
    let mut h = harness();
    h.turn();
    paste(&mut h, &path);
    h.turn();
    h.turn();
    let sent = h.store.pending_attachments.get_untracked();
    assert_eq!(sent.len(), 1);
    // Simulate the worker's started post (custody transfer).
    abstractcode::runner::clear_sent_attachments(&h.store, &h.ctx.tx.clone(), &sent);
    h.turn();
    assert!(
        h.store.paste_undo.get_untracked().is_none(),
        "the undo slot dies with the send"
    );
    h.term.push_input(&[0x0f]);
    h.turn();
    let screen = h.turn();
    assert!(
        !screen.contains("drop undone"),
        "no undo claim after the send:\n{screen}"
    );
    assert!(
        !screen.contains("sent.txt") || screen.contains("📎"),
        "no stale path text injected into the composer (the 📎 record may name it):\n{screen}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Custody unit lane over the two pub worker helpers (impl-review
/// P1-5): sibling ref caching merges BY PATH into the live list, a
/// removed chip stays removed, and a foreign-session cached ref never
/// counts as uploaded.
#[test]
fn merge_cached_refs_and_session_predicates_hold_custody() {
    use abstractcode::store::PendingAttachment;
    let h = harness();
    let mk = |path: &str, uploaded: Option<(&str, &str)>| PendingAttachment {
        path: path.into(),
        name: path.rsplit('/').next().unwrap_or(path).into(),
        size: 1,
        uploaded: uploaded.map(|(sid, id)| (sid.to_string(), serde_json::json!({"$artifact": id}))),
    };
    // Live list: a (no ref), b (no ref). Worker snapshot: a uploaded,
    // b failed (no ref), c was removed mid-flight but uploaded.
    h.store
        .pending_attachments
        .set(vec![mk("/f/a", None), mk("/f/b", None)]);
    let done = vec![
        mk("/f/a", Some(("sid-1", "ref-a"))),
        mk("/f/b", None),
        mk("/f/c", Some(("sid-1", "ref-c"))),
    ];
    abstractcode::runner::merge_cached_refs(&h.store, &done);
    let live = h.store.pending_attachments.get_untracked();
    assert_eq!(live.len(), 2, "merge never resurrects a removed chip");
    assert_eq!(
        live[0]
            .uploaded
            .as_ref()
            .map(|(s, r)| (s.as_str(), r["$artifact"].as_str().unwrap())),
        Some(("sid-1", "ref-a")),
        "the successful sibling's ref cached back by path"
    );
    assert!(live[1].uploaded.is_none(), "the failed item stays refless");
    // The reuse predicate the worker applies: a cached ref counts ONLY
    // for the session it was minted in.
    let a = &live[0];
    let same = a.uploaded.as_ref().is_some_and(|(sid, _)| sid == "sid-1");
    let foreign = a.uploaded.as_ref().is_some_and(|(sid, _)| sid == "sid-2");
    assert!(same && !foreign, "foreign-session refs never reuse");
    // clear_sent_attachments removes exactly the sent batch and kills
    // the undo slot.
    h.store
        .paste_undo
        .set(Some(("raw".into(), vec!["/f/a".into()])));
    abstractcode::runner::clear_sent_attachments(&h.store, &h.ctx.tx.clone(), &[live[0].clone()]);
    let after = h.store.pending_attachments.get_untracked();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].path, "/f/b", "only the sent chip left");
    assert!(
        h.store.paste_undo.get_untracked().is_none(),
        "paste_undo cleared on send"
    );
}

/// Removing a chip (manager x / clear) kills the armed undo — an undo
/// slot must never outlive the chips it names (P1-2 sibling class).
#[test]
fn chip_removal_and_attach_clear_kill_the_undo_slot() {
    let (dir, path) = attach_tempfile("undoable.txt", b"x");
    let mut h = harness();
    h.turn();
    paste(&mut h, &path);
    h.turn();
    assert!(h.store.paste_undo.get_untracked().is_some());
    // /attach clear discards + clears the slot.
    h.type_text("/attach clear");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.pending_attachments.with_untracked(|p| p.len()), 0);
    assert!(
        h.store.paste_undo.get_untracked().is_none(),
        "clear kills the undo slot"
    );
    // Ctrl+O now: dead key (no text injected).
    h.term.push_input(&[0x0f]);
    h.turn();
    let screen = h.turn();
    assert!(!screen.contains("drop undone"), "{screen}");
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Run-state visibility wave (untracked/reviews/run-visibility-review.md)
// ---------------------------------------------------------------------------

/// P1-2 + P2-8: scrolled-up state renders on the strip, and the Esc that
/// jumps back to the tail is CONSUMED — it must never arm the double-Esc
/// run cancel (the visibility-restoring gesture cannot destroy the run).
#[test]
fn scrolled_up_renders_on_the_strip_and_esc_jump_never_arms_cancel() {
    let mut h = harness();
    h.turn();
    // A running phase with enough REAL transcript to scroll (a user
    // card kills the splash — an all-Info fold keeps it, and the
    // splash's unclamped notice list crushes the strip row).
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("run-1".into());
    h.store.fold.update(|f| {
        f.begin_run("run-1");
        f.push_item(abstractcode::transcript::Item::User {
            text: "long task".into(),
        });
        for i in 0..40 {
            f.push_item(abstractcode::transcript::Item::Assistant {
                text: format!("progress line {i}"),
                final_answer: false,
            });
        }
    });
    h.turn();
    // Scroll up: wheel/PageUp disengages follow.
    h.term.push_input(b"\x1b[5~"); // PageUp
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("scrolled up"),
        "the strip names the scrolled-up state while running:\n{screen}"
    );
    // Esc from scrollback: jumps to tail, consumed — NO cancel arm
    // (the arm state is `last_esc`; toasts drain, so assert the state).
    h.press_escape();
    h.turn();
    let screen = h.turn();
    assert!(
        h.store.last_esc.get_untracked().is_none(),
        "the jump press never arms cancel"
    );
    assert!(
        !screen.contains("scrolled up"),
        "back at the tail the segment clears:\n{screen}"
    );
    // The NEXT Esc (already at the tail) arms cancel as before.
    h.press_escape();
    h.turn();
    h.turn();
    assert!(
        h.store.last_esc.get_untracked().is_some(),
        "tail Esc still arms cancel"
    );
}

/// P0-1: submit anchors the elapsed clock — a stale hours-old
/// `run_started` from a prior attach must never render into the new
/// Starting window.
#[test]
fn submit_anchors_the_clock_killing_the_stale_starting_lie() {
    let mut h = harness();
    h.turn();
    // The stale state the boot-attach paths leave behind: an anchor
    // hours in the past with phase Idle.
    let stale = std::time::Instant::now() - std::time::Duration::from_secs(9 * 3600);
    h.store.run_started.set(Some(stale));
    h.turn();
    h.type_text("do the thing");
    h.turn();
    h.press_enter();
    h.turn();
    let anchored = h
        .store
        .run_started
        .get_untracked()
        .expect("submit anchors the clock");
    assert!(
        anchored.elapsed().as_secs() < 5,
        "the anchor is NOW, not the stale attach-time value"
    );
}

/// P1-1 (chrome half): the idle strip leads with the newest conclusion
/// ("last run: …") so "did it finish?" is answered from fixed chrome.
#[test]
fn idle_strip_names_the_last_outcome() {
    let mut h = harness();
    h.turn();
    h.store.fold.update(|f| {
        f.begin_run("r1");
        f.done_note = "done · 12s · 3 llm calls".into();
        f.finished = true;
    });
    h.store.totals.set(abstractcode::store::SessionTotals {
        runs: 1,
        input_tokens: 1000,
        output_tokens: 50,
        total_tokens: 1050,
    });
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("last run: done"),
        "the idle line answers 'did it finish?':\n{screen}"
    );
    assert!(
        screen.contains("session: 1 run ") || screen.contains("session: 1 run ·"),
        "grammar: '1 run', never '1 runs':\n{screen}"
    );
}

/// Reactive pickers (the c5483 claim receipt): a catalog refresh landing
/// while /workflow is OPEN renders in place — the static-shell limit is
/// retired — and Enter activates against the RE-READ source, never the
/// open-time snapshot.
#[test]
fn workflow_picker_rows_follow_a_mid_open_catalog_refresh() {
    let mut h = harness();
    h.turn();
    h.store.workflows.set(vec![Workflow {
        bundle_id: "basic-agent".into(),
        flow_id: "81795ea9".into(),
        name: "basic-agent".into(),
        description: String::new(),
    }]);
    h.turn();
    h.type_text("/workflow");
    h.turn();
    h.press_enter();
    h.turn();
    let screen = h.turn();
    assert!(screen.contains("agent workflow"), "picker open:\n{screen}");
    assert!(
        !screen.contains("react-coding"),
        "the new entrypoint has not landed yet:\n{screen}"
    );
    // The catalog refresh lands WHILE the picker is open (the runner's
    // LoadCatalog post writes this signal). Two entrypoints arrive: a
    // pickable coding flow AND an entity-lane flow — the picker must render
    // the first live and keep the second hidden (the coding-picker filter,
    // operator finding 2026-08-01: entity/test entrypoints made /workflow a
    // registry dump).
    h.store.workflows.update(|ws| {
        ws.push(Workflow {
            bundle_id: "react-coding".into(),
            flow_id: "react-coder".into(),
            name: "React coder".into(),
            description: String::new(),
        });
        ws.push(Workflow {
            bundle_id: "entity-life".into(),
            flow_id: "entity-chat".into(),
            name: "entity-life".into(),
            description: String::new(),
        })
    });
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("react-coding"),
        "live rows: the new entrypoint renders WITHOUT a reopen:\n{screen}"
    );
    assert!(
        !screen.contains("entity-life"),
        "entity-lane flows stay OUT of the coding picker even when they land mid-open:\n{screen}"
    );
    // Move to it and Enter: the choose re-reads the signal, so the
    // selection is the entry that appeared mid-open.
    h.term.push_input(b"\x1b[B"); // Down
    h.turn();
    h.term.push_input(b"\r");
    h.turn();
    h.turn();
    let picked = h.store.workflow.get_untracked();
    assert_eq!(
        (picked.bundle_id.as_str(), picked.flow_id.as_str()),
        ("react-coding", "react-coder"),
        "activation re-reads the live source (and indexes the FILTERED view — \
         picking through the filter must land on the row the user saw)"
    );
}

/// Deferred-items completion (operator ask, 2026-07-25): /status renders
/// the client view + fires the server-truth probe; the probe result
/// lands reactively in the open modal.
#[test]
fn status_modal_shows_client_view_and_live_server_probe() {
    let mut h = harness();
    h.turn();
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("run-abc12345-6789".into());
    h.turn();
    h.type_text("/status");
    h.turn();
    h.press_enter();
    h.turn();
    let screen = h.turn();
    assert!(screen.contains("status"), "modal open:\n{screen}");
    assert!(
        screen.contains("client     running"),
        "client phase renders:\n{screen}"
    );
    assert!(
        screen.contains("probing…"),
        "server probe pending state:\n{screen}"
    );
    // The dispatch fired the probe command.
    let mut probed = false;
    while let Ok(cmd) = h.rx.try_recv() {
        if let Cmd::ProbeRunStatus { run_id } = cmd {
            assert_eq!(run_id, "run-abc12345-6789");
            probed = true;
        }
    }
    assert!(probed, "/status probes server truth at the gesture");
    // The worker's post lands while the modal is open: rendered live.
    h.store.run_status_probe.set(Some((
        "run-abc12345-6789".into(),
        "waiting · node poll".into(),
    )));
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("waiting · node poll"),
        "server truth renders when the probe lands:\n{screen}"
    );
}

/// The strip names the newest cycle's intent (visibility review P2-1) —
/// and the gist hides the moment the activity stops being a thinking
/// label (lifetime rides `activity`, no bespoke clears).
#[test]
fn strip_names_cycle_intent_from_the_models_own_words() {
    let mut h = harness();
    h.turn();
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("r1".into());
    h.store.fold.update(|f| {
        f.begin_run("r1");
        f.push_item(abstractcode::transcript::Item::User {
            text: "task".into(),
        });
        // Cycle 1 starts + its result lands (the gist source).
        f.apply(
            "r1",
            &serde_json::json!({"run_id": "r1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        f.apply(
            "r1",
            &serde_json::json!({"run_id": "r1", "node_id": "reason", "status": "completed",
                    "effect": {"type": "llm_call", "payload": {}},
                    "result": {"content": "fixing the end_line computation in game.js"}}),
        );
        // Cycle 2's call is in flight: the strip shows cycle 1's words.
        f.apply(
            "r1",
            &serde_json::json!({"run_id": "r1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
    });
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("fixing the end_line computation"),
        "the cycle names its intent on the strip:\n{screen}"
    );
}

/// Image attachments echo as a mosaic preview (attachments v2 echo,
/// completed): the started post pushes the Image card + fetch effect,
/// and rehydration restores it from input_data.context.attachments.
#[test]
fn image_attachments_echo_as_previews_live_and_on_restore() {
    use abstractcode::store::PendingAttachment;
    let mut h = harness();
    h.turn();
    let sent = vec![PendingAttachment {
        path: "/tmp/photo.png".into(),
        name: "photo.png".into(),
        size: 1000,
        uploaded: Some((
            "acode-test-session".into(),
            serde_json::json!({"$artifact": "img1", "artifact_id": "img1",
                               "content_type": "image/png", "modality": "image",
                               "run_id": "session_memory_acode-test-session",
                               "filename": "photo.png"}),
        )),
    }];
    abstractcode::runner::clear_sent_attachments(&h.store, &h.ctx.tx.clone(), &sent);
    let has_image = h.store.fold.with_untracked(|f| {
        f.items.iter().any(|i| {
            matches!(i,
            abstractcode::transcript::Item::Image { artifact_id, label, .. }
                if artifact_id == "img1" && label.contains("photo.png"))
        })
    });
    assert!(has_image, "the attached image gets a preview card");
    let mut fetched = false;
    while let Ok(cmd) = h.rx.try_recv() {
        if let Cmd::FetchImage {
            run_id,
            artifact_id,
        } = cmd
        {
            assert_eq!(run_id, "session_memory_acode-test-session");
            assert_eq!(artifact_id, "img1");
            fetched = true;
        }
    }
    assert!(fetched, "the mosaic fetch fires for the attached image");
}

// ---------------------------------------------------------------------------
// Quit-with-live-run gate (untracked/reviews/quit-modal-design.md)
// ---------------------------------------------------------------------------

fn arm_live_run(h: &mut Harness) {
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("run-quit-test-0001".into());
    h.store.fold.update(|f| {
        f.begin_run("run-quit-test-0001");
        f.push_item(abstractcode::transcript::Item::User { text: "t".into() });
    });
}

/// E1: idle quit is instant — byte-identical to before the gate.
#[test]
fn quit_idle_is_instant_no_modal() {
    let mut h = harness();
    h.turn();
    h.term.push_input(&[0x11]); // Ctrl+Q
    h.turn();
    h.turn();
    assert!(h.app.quit_requested(), "idle Ctrl+Q quits instantly");
    assert!(!h.ctx.modal_open(), "no modal");
}

/// E2 + D3: a live run opens the modal (teaching line); Enter = leave &
/// quit with NOTHING sent; Esc = stay.
#[test]
fn quit_with_live_run_opens_modal_enter_leaves_esc_stays() {
    let mut h = harness();
    h.turn();
    arm_live_run(&mut h);
    h.turn();
    h.term.push_input(&[0x11]); // Ctrl+Q
    h.turn();
    let screen = h.turn();
    assert!(!h.app.quit_requested(), "gated: not quit yet");
    assert!(
        screen.contains("never stops"),
        "the thin-client teach line:\n{screen}"
    );
    // Esc stays: modal gone, state None, app alive.
    h.press_escape();
    h.turn();
    assert!(!h.app.quit_requested(), "Esc stays");
    assert!(!h.ctx.modal_open(), "modal closed");
    assert!(matches!(
        h.store.quit_state.get_untracked(),
        abstractcode::store::QuitState::None
    ));
    // Re-quit + Enter: leave & quit, no verb commands sent.
    h.term.push_input(&[0x11]);
    h.turn();
    h.turn();
    h.term.push_input(b"\r");
    h.turn();
    h.turn();
    assert!(h.app.quit_requested(), "Enter leaves & quits");
    while let Ok(cmd) = h.rx.try_recv() {
        assert!(
            !matches!(cmd, Cmd::Pause { .. } | Cmd::Cancel { .. }),
            "leave sends NO verb"
        );
    }
}

/// Quit-delivery plan v2 (operator-validated): the pause verb rides a
/// DEDICATED one-shot send — the worker channel gets NOTHING (two send
/// paths would mint two command_ids; dedup would not collapse them).
/// Against the harness's dead port the send fails fast and honestly:
/// Delivering → err-ack via wake → Failed (definitive) → Enter quits
/// anyway.
#[test]
fn quit_pause_dedicated_send_bypasses_the_worker_and_fails_honestly() {
    let mut h = harness();
    h.turn();
    arm_live_run(&mut h);
    h.turn();
    h.term.push_input(&[0x11]);
    h.turn();
    h.turn();
    h.term.push_input(b"p");
    h.turn();
    // The worker channel got NO verb — the dedicated thread owns it.
    while let Ok(cmd) = h.rx.try_recv() {
        assert!(
            !matches!(cmd, Cmd::Pause { .. } | Cmd::Cancel { .. }),
            "the quit lane never enqueues on the worker"
        );
    }
    // The dedicated send hits the dead port (connect refused) and the
    // err-ack posts back via wake — pump until the Failed state lands
    // (bounded: threads schedule when they schedule).
    let mut failed = false;
    for _ in 0..100 {
        h.turn();
        if matches!(
            h.store.quit_state.get_untracked(),
            abstractcode::store::QuitState::Failed {
                definitive: true,
                ..
            }
        ) {
            failed = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        failed,
        "the dead-gateway send fails definitively (no fake spinner)"
    );
    let screen = h.turn();
    assert!(
        screen.contains("pause not confirmed"),
        "honest failed state:\n{screen}"
    );
    // Enter: quit anyway.
    h.term.push_input(b"\r");
    h.turn();
    h.turn();
    assert!(h.app.quit_requested());
}

/// The sequencer's ack contract, state-level (deterministic — no
/// threads): a matching ok-ack in Delivering completes the quit.
#[test]
fn quit_sequencer_matching_ack_completes_the_quit() {
    let mut h = harness();
    h.turn();
    arm_live_run(&mut h);
    h.turn();
    h.store
        .quit_state
        .set(abstractcode::store::QuitState::Delivering {
            verb: abstractcode::store::QuitVerb::Pause,
            run_id: "run-quit-test-0001".into(),
            gen: 1,
        });
    h.turn();
    h.store.verb_ack.set(Some(abstractcode::store::VerbAck {
        verb: abstractcode::store::QuitVerb::Pause,
        run_id: "run-quit-test-0001".into(),
        ok: true,
        definitive: true,
        error: String::new(),
    }));
    h.turn();
    h.turn();
    assert!(h.app.quit_requested(), "ACK confirmed → quit");
    assert!(matches!(
        h.store.quit_state.get_untracked(),
        abstractcode::store::QuitState::Acked { .. }
    ));
}
/// E8/E14 state-level (no threads — the dedicated send is exercised by
/// its own test): stale acks for another run are ignored; the real
/// err-ack lands the honest Failed state; Enter quits anyway.
#[test]
fn quit_cancel_failure_and_stale_acks() {
    let mut h = harness();
    h.turn();
    arm_live_run(&mut h);
    h.turn();
    // Open the modal first (the Failed state renders inside it), then
    // drive the state machine directly — deterministic, no send thread.
    h.term.push_input(&[0x11]);
    h.turn();
    h.turn();
    h.store
        .quit_state
        .set(abstractcode::store::QuitState::Delivering {
            verb: abstractcode::store::QuitVerb::Cancel,
            run_id: "run-quit-test-0001".into(),
            gen: 7,
        });
    h.turn();
    // Stale ack (another run): ignored — still Delivering.
    h.store.verb_ack.set(Some(abstractcode::store::VerbAck {
        verb: abstractcode::store::QuitVerb::Cancel,
        run_id: "some-other-run".into(),
        ok: true,
        definitive: true,
        error: String::new(),
    }));
    h.turn();
    assert!(
        matches!(
            h.store.quit_state.get_untracked(),
            abstractcode::store::QuitState::Delivering { .. }
        ),
        "mismatched ack ignored"
    );
    assert!(!h.app.quit_requested());
    // The real ack fails: Failed state, honest wording, app alive.
    h.store.verb_ack.set(Some(abstractcode::store::VerbAck {
        verb: abstractcode::store::QuitVerb::Cancel,
        run_id: "run-quit-test-0001".into(),
        ok: false,
        definitive: false,
        error: "gateway timed out".into(),
    }));
    h.turn();
    let screen = h.turn();
    assert!(!h.app.quit_requested(), "failure never quits by itself");
    assert!(
        screen.contains("cancel not confirmed"),
        "failed state renders:\n{screen}"
    );
    // Enter: quit anyway.
    h.term.push_input(b"\r");
    h.turn();
    h.turn();
    assert!(h.app.quit_requested(), "quit-anyway exits");
}
/// D5 + E11/E12: the run concluding under the open modal auto-quits,
/// and the drain guard holds queued prompts back (no new run starts
/// under a quitting user).
#[test]
fn quit_modal_auto_quits_on_conclusion_and_never_drains() {
    let mut h = harness();
    h.turn();
    arm_live_run(&mut h);
    h.store.queue.update(|q| {
        q.push(abstractcode::store::QueuedPrompt {
            id: 1,
            text: "next task".into(),
        })
    });
    h.turn();
    h.term.push_input(&[0x11]);
    h.turn();
    h.turn();
    assert!(!h.app.quit_requested());
    // The run concludes (the runner-post shape: outcome + phase).
    h.store
        .last_outcome
        .set(abstractcode::store::RunOutcome::Success);
    h.store.phase.set(Phase::Idle);
    h.turn();
    h.turn();
    assert!(h.app.quit_requested(), "conclusion under the modal → quit");
    while let Ok(cmd) = h.rx.try_recv() {
        assert!(
            !matches!(cmd, Cmd::Start { .. }),
            "the drain guard held the queued prompt back"
        );
    }
    assert_eq!(
        h.store.queue.with_untracked(|q| q.len()),
        1,
        "the queued prompt persists for the next launch"
    );
}

/// E13 + E6: repeat gesture = leave & quit; Starting with no bound run
/// disables the verbs (p sends nothing).
#[test]
fn quit_repeat_gesture_leaves_and_unbound_start_disables_verbs() {
    let mut h = harness();
    h.turn();
    // Starting, unbound.
    h.store.phase.set(Phase::Starting);
    h.store.run_id.set(String::new());
    h.turn();
    h.term.push_input(&[0x11]);
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("not yet bound"),
        "starting variant renders:\n{screen}"
    );
    h.term.push_input(b"p");
    h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Pause { .. })).is_none(),
        "unbound: no verb sent"
    );
    assert!(!h.app.quit_requested());
    // Repeat gesture: leave & quit.
    h.term.push_input(&[0x11]);
    h.turn();
    h.turn();
    assert!(h.app.quit_requested(), "Ctrl+Q ×2 always exits");
}

/// Audit P2: a late ack landing AFTER the 8s timeout (state Failed)
/// still honors the declared intent — the verb WAS delivered, so the
/// app quits instead of showing "not confirmed" beside a "paused
/// durably" toast.
///
/// The state is DRIVEN directly (no `p` press): choosing pause spawns
/// a real send thread against the harness's dead port, and its err-ack
/// could overwrite this test's synthetic ok-ack in the same drain
/// (adversary D4 — posted jobs run before the effect flush).
#[test]
fn quit_late_ack_after_timeout_still_quits() {
    let mut h = harness();
    h.turn();
    arm_live_run(&mut h);
    h.turn();
    h.term.push_input(&[0x11]);
    h.turn();
    h.turn();
    // Simulate deliver()'s Delivering→timeout→Failed outcome without
    // the real thread.
    h.store
        .quit_state
        .set(abstractcode::store::QuitState::Failed {
            verb: abstractcode::store::QuitVerb::Pause,
            run_id: "run-quit-test-0001".into(),
            definitive: false,
            error: "no confirmation in 8s".into(),
        });
    h.turn();
    assert!(!h.app.quit_requested());
    // The late ack lands: delivered → quit.
    h.store.verb_ack.set(Some(abstractcode::store::VerbAck {
        verb: abstractcode::store::QuitVerb::Pause,
        run_id: "run-quit-test-0001".into(),
        ok: true,
        definitive: true,
        error: String::new(),
    }));
    h.turn();
    h.turn();
    assert!(h.app.quit_requested(), "late delivery honors the intent");
    assert!(matches!(
        h.store.quit_state.get_untracked(),
        abstractcode::store::QuitState::Acked { .. }
    ));
}

/// Adversary D3: the exactly-once contract's two unpinned halves in one
/// live round trip — (a) the transient retry reuses the SAME command_id
/// (a per-attempt-mint refactor would break dedup silently: the whole
/// gate passed without this pin), and (b) the verb delivers with ZERO
/// worker involvement (dead-worker delivery — `send_verb_blocking` is
/// called directly, no `Cmd` channel anywhere).
///
/// Mock gateway: attempt 1 is read fully then DROPPED without a
/// response (transport error → transient → retry); attempt 2 answers
/// 200. Both captured bodies must carry the minted id.
#[test]
fn quit_verb_retry_reuses_the_same_command_id_without_a_worker() {
    use std::io::{Read as _, Write as _};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let (body_tx, body_rx) = std::sync::mpsc::channel::<String>();

    // Minimal HTTP reader: head to CRLFCRLF, then Content-Length bytes.
    fn read_request(sock: &mut std::net::TcpStream) -> String {
        let mut buf = Vec::new();
        let mut b = [0u8; 1024];
        let head_end = loop {
            let n = sock.read(&mut b).expect("read");
            assert!(n > 0, "peer closed mid-request");
            buf.extend_from_slice(&b[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        let len: usize = head
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().parse().unwrap())
            })
            .expect("content-length");
        while buf.len() < head_end + len {
            let n = sock.read(&mut b).expect("read body");
            assert!(n > 0, "peer closed mid-body");
            buf.extend_from_slice(&b[..n]);
        }
        String::from_utf8_lossy(&buf[head_end..head_end + len]).to_string()
    }

    let server = std::thread::spawn(move || {
        // Attempt 1: capture, then drop with no response (transport
        // error client-side — the transient class that retries).
        let (mut s1, _) = listener.accept().expect("accept 1");
        body_tx.send(read_request(&mut s1)).unwrap();
        drop(s1);
        // Attempt 2: capture, answer 200.
        let (mut s2, _) = listener.accept().expect("accept 2");
        body_tx.send(read_request(&mut s2)).unwrap();
        let resp = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 11\r\n\r\n{\"ok\":true}";
        s2.write_all(resp.as_bytes()).unwrap();
    });

    let mut h = harness();
    h.turn();
    let client =
        abstractcode::gateway::GatewayClient::new(&format!("http://127.0.0.1:{port}"), None);
    let wake = abstracttui::reactive::wake_handle();
    let command_id = abstractcode::gateway::mint_command_id();
    // Direct call: no worker thread, no Cmd channel — the dedicated
    // send lane's exact shape (blocks through both attempts).
    abstractcode::runner::send_verb_blocking(
        &client,
        &wake,
        h.store,
        abstractcode::store::QuitVerb::Pause,
        "run-quit-retry-0001".into(),
        &command_id,
    );
    server.join().expect("server thread");

    let b1 = body_rx.recv().expect("attempt 1 body");
    let b2 = body_rx.recv().expect("attempt 2 body");
    assert!(
        b1.contains(&format!("\"command_id\":\"{command_id}\"")),
        "attempt 1 carries the minted id: {b1}"
    );
    assert_eq!(b1, b2, "the retry reuses the SAME body — same command_id");

    // The ok-ack reaches the store through wake alone (dead-worker
    // delivery): pump and read.
    h.turn();
    let ack = h.store.verb_ack.get_untracked().expect("ack posted");
    assert!(ack.ok, "delivered on the retry: {ack:?}");
    assert!(ack.definitive);
    assert_eq!(ack.run_id, "run-quit-retry-0001");
}

/// Bloc history (laurent's ruling): /history streams the previous bloc
/// — dispatch honesty (nothing older → notice; count/all parse; the
/// worker command carries session + cursor + count).
#[test]
fn history_command_dispatches_blocs_honestly() {
    let mut h = harness();
    h.turn();
    // Nothing older: honest notice, no command.
    h.type_text("/history");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(h
        .store
        .notices
        .get_untracked()
        .iter()
        .any(|n| n.contains("no earlier history")));
    assert!(h
        .find_cmd(|c| matches!(c, Cmd::LoadHistory { .. }))
        .is_none());
    // Older turns known: the command carries the cursor.
    h.store.older_turns.set(7);
    h.store
        .history_cursor
        .set(Some("2026-07-23T18:00:00Z".into()));
    h.turn();
    h.type_text("/history 3");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::LoadHistory { .. })) {
        Some(Cmd::LoadHistory {
            session_id,
            before,
            count,
        }) => {
            assert_eq!(session_id, "acode-test-session");
            assert_eq!(before, "2026-07-23T18:00:00Z");
            assert_eq!(count, 3);
        }
        other => panic!("expected LoadHistory, got {:?}", other.map(|_| "cmd")),
    }
    // A second /history while the first bloc is IN FLIGHT is refused
    // (one bloc at a time — the auto-loader shares this guard), and the
    // refusal is SPOKEN (a silent return read as a dead keystroke).
    assert!(h.store.history_loading.get_untracked());
    h.type_text("/history all");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadHistory { .. }))
            .is_none(),
        "in-flight guard holds for the slash command too"
    );
    assert!(
        h.store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("already streaming")),
        "the in-flight refusal names itself"
    );
    // After completion, `all` = everything older.
    h.store.history_loading.set(false);
    h.turn();
    h.type_text("/history all");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::LoadHistory { .. })) {
        Some(Cmd::LoadHistory { count, .. }) => assert_eq!(count, 7),
        other => panic!("expected LoadHistory, got {:?}", other.map(|_| "cmd")),
    }
}

/// Operator UX ruling (2026-07-25): reaching the TOP of a scrolled-up
/// transcript auto-loads the previous history bloc — no /history
/// incantation required — with the stub line as the visible progress
/// surface. Pins: the top-edge dispatch, the in-flight guard (no
/// double-dispatch), the cascade re-arm after completion, and stub
/// honesty through the streaming window.
#[test]
fn scroll_to_top_autoloads_previous_history_bloc_with_progress() {
    let mut h = harness();
    h.turn();
    // A session with older turns on the gateway: stub + cursor + count
    // (what probe_attach seeds after a bloc-limited boot restore).
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::Info {
            text: abstractcode::runner::history_stub_text(3),
        });
        f.push_item(abstractcode::transcript::Item::User {
            text: "recent task".into(),
        });
        for i in 0..40 {
            f.push_item(abstractcode::transcript::Item::Assistant {
                text: format!("line {i}"),
                final_answer: false,
            });
        }
    });
    h.store.older_turns.set(3);
    h.store
        .history_cursor
        .set(Some("2026-01-01T00:00:00Z".into()));
    h.turn();
    // Scroll to the very top (PageUp clamps at offset 0).
    for _ in 0..20 {
        h.term.push_input(b"\x1b[5~");
        h.turn();
    }
    h.turn();
    // The edge dispatched ONE bloc load.
    let cmd = h.find_cmd(|c| matches!(c, Cmd::LoadHistory { .. }));
    assert!(cmd.is_some(), "top edge dispatches the previous bloc");
    assert!(h.store.history_loading.get_untracked());
    // Stub is the progress surface; the strip names it too.
    let screen = h.turn();
    assert!(
        screen.contains("streaming"),
        "progress renders while the bloc streams:\n{screen}"
    );
    // In-flight guard: still at the top, loading — no second dispatch.
    h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadHistory { .. }))
            .is_none(),
        "no double-dispatch while a bloc is in flight"
    );
    // Completion (what the runner posts): prepended items, fewer older
    // turns, loading off. Still at the top -> the cascade re-arms and
    // fetches the NEXT bloc.
    h.store.fold.update(|f| {
        abstractcode::runner::prepend_history_items(
            f,
            vec![abstractcode::transcript::Item::User {
                text: "an older task".into(),
            }],
            2,
        );
    });
    h.store.older_turns.set(2);
    h.store.history_loading.set(false);
    h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadHistory { .. }))
            .is_some(),
        "holding at the top cascades into the next bloc"
    );
    // Esc jumps to the tail -> follow re-arms -> the cascade stops:
    // the SECOND bloc's completion lands after the jump and must not
    // dispatch a third.
    h.press_escape();
    h.turn();
    h.store.history_loading.set(false);
    h.store.older_turns.set(1);
    h.turn();
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadHistory { .. }))
            .is_none(),
        "back at the tail nothing auto-loads"
    );
}

/// A transcript that FITS the pane must not auto-load history off a
/// PageUp that visibly moves nothing. The engine's wheel gesture keeps
/// follow armed on fitting content (Scroll's `derive_follow`: offset 0
/// IS the bottom edge when max_off == 0), and the app's PageUp now
/// derives follow from the same geometry — pre-fix it released
/// unconditionally, so one keypress on a short restored transcript
/// flipped follow=false at offset 0 and the scroll-top auto-loader
/// cascaded the WHOLE session in, bloc by bloc, off a gesture that
/// scrolled nothing.
#[test]
fn pageup_on_a_fitting_transcript_never_autoloads_history() {
    let mut h = harness();
    h.turn();
    // A short restored session: stub + one complete turn — well under
    // the 30-row harness pane.
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::Info {
            text: abstractcode::runner::history_stub_text(3),
        });
        f.push_item(abstractcode::transcript::Item::User { text: "hi".into() });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "short answer".into(),
            final_answer: true,
        });
    });
    h.store.older_turns.set(3);
    h.store
        .history_cursor
        .set(Some("2026-01-01T00:00:00Z".into()));
    h.turn();
    for _ in 0..3 {
        h.term.push_input(b"\x1b[5~");
        h.turn();
    }
    h.turn();
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadHistory { .. }))
            .is_none(),
        "a no-op PageUp on fitting content must not dispatch a history bloc"
    );
    assert!(
        !h.store.history_loading.get_untracked(),
        "nothing armed the streaming state"
    );
}

/// First-citizen reasoning (operator directive c5710): the model picker's
/// THIRD stage — probe dispatch, live capability rows, selection persists
/// the pair-coupled triple, and a route change resets the override.
#[test]
fn reasoning_stage_probes_selects_and_route_change_resets() {
    let mut h = harness();
    h.turn();
    h.store
        .providers
        .set(vec![abstractcode::store::ProviderInfo {
            name: "endpoint:airelay".into(),
            models: vec!["gpt-5.6-sol".into(), "gpt-5.6-luna".into()],
        }]);
    h.turn();
    // Stage 1 -> 2 -> pick gpt-5.6-sol.
    h.type_text("/model");
    h.turn();
    h.press_enter();
    h.turn();
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("reasoning — gpt-5.6-sol"),
        "stage 3 opens for the chosen model:\n{screen}"
    );
    // The capability probe was dispatched for the pair.
    match h.find_cmd(|c| matches!(c, Cmd::ProbeModelReasoning { .. })) {
        Some(Cmd::ProbeModelReasoning { provider, model }) => {
            assert_eq!(provider, "endpoint:airelay");
            assert_eq!(model, "gpt-5.6-sol");
        }
        other => panic!("expected probe dispatch, got {:?}", other.map(|_| "cmd")),
    }
    // Probe lands: a reasoning model with declared levels — live rows
    // pick it up in place.
    h.store
        .reasoning_probe
        .set(Some(abstractcode::store::ReasoningProbe {
            provider: "endpoint:airelay".into(),
            model: "gpt-5.6-sol".into(),
            supported: Some(true),
            levels: vec!["low".into(), "medium".into(), "high".into(), "xhigh".into()],
            source: "exact".into(),
        }));
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("reasoning model — pick the effort"),
        "declared-support caption renders:\n{screen}"
    );
    // Rows: 0 default · 1 none · 2 caption · 3.. levels. Choose "high"
    // (row index 5): down x5 then Enter.
    for _ in 0..5 {
        h.term.push_input(b"\x1b[B");
        h.turn();
    }
    h.press_enter();
    h.turn();
    h.turn();
    assert_eq!(h.store.reasoning.get_untracked(), "high");
    // Pair-coupled persistence.
    {
        let prefs = h.ctx.prefs.borrow();
        assert_eq!(prefs.reasoning.as_deref(), Some("high"));
        assert_eq!(
            prefs.reasoning_provider.as_deref(),
            Some("endpoint:airelay")
        );
        assert_eq!(prefs.reasoning_model.as_deref(), Some("gpt-5.6-sol"));
    }
    // The header names the triple.
    let screen = h.turn();
    assert!(
        screen.contains("gpt-5.6-sol · high"),
        "route label carries the third axis:\n{screen}"
    );

    // ROUTE CHANGE RESETS (the coupling rule): pick the OTHER model,
    // Esc at stage 3 — the override must be gone, prefs cleared.
    h.type_text("/model");
    h.turn();
    h.press_enter();
    h.turn();
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    h.term.push_input(b"\x1b[B\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    h.press_escape(); // stage 3: keep gateway default for the new model
    h.turn();
    assert_eq!(h.store.model.get_untracked(), "gpt-5.6-luna");
    assert_eq!(
        h.store.reasoning.get_untracked(),
        "",
        "a model change resets the effort override"
    );
    assert!(
        h.ctx.prefs.borrow().reasoning.is_none(),
        "prefs triple cleared on route change"
    );
}

/// `/reasoning <level>` fast path + validation + `default` clearing; the
/// locked/unknown caption for a registry non-reasoner.
#[test]
fn reasoning_command_fast_path_and_locked_caption() {
    let mut h = harness();
    h.turn();
    h.store.provider.set("lmstudio".into());
    h.store.model.set("qwen3-4b".into());
    h.turn();
    h.type_text("/reasoning high");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.reasoning.get_untracked(), "high");
    h.type_text("/reasoning bogus");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.reasoning.get_untracked(), "high", "junk refused");
    assert!(h
        .store
        .notices
        .get_untracked()
        .iter()
        .any(|n| n.contains("none|minimal|low|medium|high|xhigh|auto")));
    h.type_text("/reasoning default");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.reasoning.get_untracked(), "", "default clears");

    // Bare /reasoning opens the dial; a supported=false probe renders
    // the honest locked caption WITH the set-anyway override rows
    // (three-state coupling — never a hard lock without provenance).
    h.type_text("/reasoning");
    h.turn();
    h.press_enter();
    h.turn();
    h.store
        .reasoning_probe
        .set(Some(abstractcode::store::ReasoningProbe {
            provider: "lmstudio".into(),
            model: "qwen3-4b".into(),
            supported: Some(false),
            levels: Vec::new(),
            source: String::new(),
        }));
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("does not reason"),
        "registry-false caption renders:\n{screen}"
    );
    assert!(
        screen.contains("set anyway"),
        "the override affordance stays available:\n{screen}"
    );
    h.press_escape();
    h.turn();
}

/// Thinking three-state (first-citizen: "by default, thinking should be
/// folded, but we should be able to examine them"): folded gist by
/// default, /details full reveals content AND the reasoning channel
/// (the old render DROPPED reasoning whenever content existed), /details
/// fold returns to gists. Replay parity is free (projection-side).
#[test]
fn thinking_cards_fold_by_default_and_expand_with_details_on() {
    let mut h = harness();
    h.turn();
    h.store.fold.update(|f| {
        f.begin_run("root");
        f.push_item(abstractcode::transcript::Item::User {
            text: "task".into(),
        });
        f.push_item(abstractcode::transcript::Item::Thinking {
            iteration: 3,
            content: "I will edit the file next.".into(),
            reasoning: "SECRETPLAN alpha beta gamma".into(),
            call: abstractcode::transcript::CallCost::default(),
        });
    });
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("I will edit the file next."),
        "folded gist shows the first content line:\n{screen}"
    );
    assert!(
        !screen.contains("SECRETPLAN"),
        "reasoning stays folded by default:\n{screen}"
    );
    assert!(
        screen.contains("words of reasoning"),
        "the fold NAMES what expansion holds:\n{screen}"
    );
    // Examine: /details full.
    h.type_text("/details full");
    h.turn();
    h.press_enter();
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("SECRETPLAN"),
        "full mode reveals the reasoning channel:\n{screen}"
    );
    assert!(
        screen.contains("— reasoning —"),
        "channels stay labeled, never coalesced:\n{screen}"
    );
    // Back to gists.
    h.type_text("/details fold");
    h.turn();
    h.press_enter();
    h.turn();
    let screen = h.turn();
    assert!(!screen.contains("SECRETPLAN"), "folded again:\n{screen}");
}

/// Optional coder gating (operator request 2026-07-27): picking a
/// gating-capable workflow opens the gated/unattended choice; No sets
/// gating_mode=auto (sent on start, top-level input_data), and /gating
/// wait re-gates. A non-gating workflow shows no modal and resets the
/// mode. /status names an unattended run so it is never a surprise.
#[test]
fn gating_modal_on_coder_select_and_status_surfaces_unattended() {
    let mut h = harness();
    h.turn();
    h.store.workflows.set(vec![
        abstractcode::store::Workflow {
            bundle_id: "multiagent-coding".into(),
            flow_id: "multiagent-coder".into(),
            name: "Multi-agent coder".into(),
            description: String::new(),
        },
        abstractcode::store::Workflow {
            bundle_id: "basic-agent".into(),
            flow_id: "basic".into(),
            name: "Basic agent".into(),
            description: String::new(),
        },
    ]);
    h.turn();
    // Pick the coder (row 0) -> the gating choice opens.
    h.type_text("/workflow");
    h.turn();
    h.press_enter();
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("run gated?"),
        "the gated/unattended choice opens for the coder:\n{screen}"
    );
    // Choose No (row 1) -> unattended.
    h.term.push_input(b"\x1b[B");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.gating_mode.get_untracked(), "auto");
    // /status names it.
    let rows = abstractcode::ui::transcript_view::status_card_rows(h.store, "gw", "");
    assert!(
        rows.iter()
            .any(|(k, v)| *k == "gating" && v.contains("UNATTENDED")),
        "status surfaces the unattended mode: {rows:?}"
    );
    // /gating wait re-gates.
    h.type_text("/gating wait");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(h.store.gating_mode.get_untracked(), "");

    // Selecting a non-gating workflow opens NO modal and clears any mode.
    h.store.gating_mode.set("auto".into());
    h.type_text("/workflow");
    h.turn();
    h.press_enter();
    h.turn();
    h.term.push_input(b"\x1b[B"); // move to basic-agent (row 1)
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        !screen.contains("run gated?"),
        "no gating modal for a non-gating workflow:\n{screen}"
    );
    assert_eq!(
        h.store.gating_mode.get_untracked(),
        "",
        "switching to a non-gating workflow resets the mode"
    );
}

/// Type-to-focus: the transcript `Scroll` is focusable, so a Tab (or a
/// click in the scrollback) parks the keyboard off the composer — and
/// the Scroll answers only navigation keys, so every character typed
/// there used to be DROPPED with no sign of where it went (operator
/// report 2026-08-16). Typing now hands focus back AND keeps the first
/// character; Enter proves the focus really moved (an unfocused
/// composer never submits).
#[test]
fn typing_off_the_composer_recovers_focus_and_keeps_the_first_character() {
    let mut h = harness();
    h.turn();
    // A conversation (not the splash) is what mounts the scrollable
    // transcript the keyboard can wander into.
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User { text: "hi".into() });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "hello there".into(),
            final_answer: true,
        });
    });
    h.turn();

    // Tab moves focus to the transcript.
    h.type_text("\t");
    h.turn();

    // Typing lands in the draft — including the very first character.
    h.type_text("write a haiku");
    let screen = h.turn();
    assert!(
        screen.contains("write a haiku"),
        "the first keystroke is not swallowed:\n{screen}"
    );

    // Enter submits: only a FOCUSED composer sees it.
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { prompt, .. }) => assert_eq!(prompt, "write a haiku"),
        _ => panic!("typing recovered the draft but not the focus: Enter never submitted"),
    }
}

/// The same recovery for the gesture that actually causes this in the
/// field: a plain left CLICK in the scrollback (to read, or to start a
/// selection) focuses the nearest focusable ancestor — the transcript
/// Scroll — and the keyboard silently stops reaching the composer.
#[test]
fn typing_after_a_click_in_the_transcript_recovers_focus() {
    let mut h = harness();
    h.turn();
    abstracttui::app::selection::selection().set_enabled(true);
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User { text: "hi".into() });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "a line worth clicking".into(),
            final_answer: true,
        });
    });
    h.turn();
    let screen = h.turn();
    let (row, col) = locate(&screen, "a line worth clicking").expect("answer on screen");
    let (x, y) = (col + 2, row + 1); // SGR is 1-based
    h.term.push_input(format!("\x1b[<0;{x};{y}M").as_bytes());
    h.turn();
    h.term.push_input(format!("\x1b[<0;{x};{y}m").as_bytes());
    h.turn();

    h.type_text("write a haiku");
    let screen = h.turn();
    assert!(
        screen.contains("❯"),
        "sanity: the composer row is on screen:\n{screen}"
    );
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { prompt, .. }) => assert_eq!(prompt, "write a haiku"),
        _ => panic!("typing after a transcript click never reached the composer"),
    }
}

/// A `/command` typed off the composer arrives whole: the character
/// lands in the draft AND the completion dropdown opens on it, because
/// the recovery writes the same value/caret signals the engine's
/// completion controller watches (operator ask: "typing a text or /").
#[test]
fn a_slash_typed_off_the_composer_opens_the_command_dropdown() {
    let mut h = harness();
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User { text: "hi".into() });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "hello there".into(),
            final_answer: true,
        });
    });
    h.turn();
    h.type_text("\t");
    h.turn();

    h.type_text("/he");
    let screen = h.turn();
    assert!(
        screen.contains("commands + keys"),
        "the dropdown opens on a '/' typed from the transcript:\n{screen}"
    );
}

/// Pasted text off the composer: the same recovery, with the engine's
/// block-paste rule (newlines normalized, nothing dropped).
#[test]
fn pasted_text_off_the_composer_lands_in_the_draft_and_recovers_focus() {
    let mut h = harness();
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User { text: "hi".into() });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "hello there".into(),
            final_answer: true,
        });
    });
    h.turn();
    h.type_text("\t");
    h.turn();

    paste(&mut h, "write a haiku");
    let screen = h.turn();
    assert!(
        screen.contains("write a haiku"),
        "the paste is not swallowed:\n{screen}"
    );
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { prompt, .. }) => assert_eq!(prompt, "write a haiku"),
        _ => panic!("the paste landed but the focus never came back"),
    }
}

/// A file DROP off the composer stages its chip (the drop-as-paste
/// contract runs whichever widget holds focus) and hands focus back, so
/// the prompt that goes with the file can be typed straight away.
#[test]
fn a_file_dropped_on_the_transcript_stages_a_chip_and_recovers_focus() {
    let (dir, path) = attach_tempfile("dropped.md", b"hello world");
    let mut h = harness();
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User { text: "hi".into() });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "hello there".into(),
            final_answer: true,
        });
    });
    h.turn();
    h.type_text("\t");
    h.turn();

    paste(&mut h, &path);
    let screen = h.turn();
    let pending = h.store.pending_attachments.get_untracked();
    assert_eq!(pending.len(), 1, "the drop staged a chip:\n{screen}");
    assert_eq!(pending[0].name, "dropped.md");
    assert!(
        !screen.contains(&path),
        "a consumed drop inserts no path text:\n{screen}"
    );

    // Focus is back on the composer: the prompt that goes with the file
    // types and sends without a Tab.
    h.type_text("summarize it");
    h.turn();
    h.press_enter();
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::Start { .. })) {
        Some(Cmd::Start { prompt, .. }) => assert_eq!(prompt, "summarize it"),
        _ => panic!("the drop staged its chip but the focus never came back"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// The recovery claims TYPING only: the transcript keeps its navigation
/// keys, and Ctrl chords keep reaching the root shortcut table.
#[test]
fn type_to_focus_leaves_navigation_and_ctrl_chords_alone() {
    let mut h = harness();
    h.turn();
    h.store.fold.update(|f| {
        for i in 0..80 {
            f.push_item(abstractcode::transcript::Item::Info {
                text: format!("line {i}"),
            });
        }
        f.push_item(abstractcode::transcript::Item::User {
            text: "the tail card".into(),
        });
    });
    // Three pumps: width discovery, extent sync, and the feed's gap-flip
    // geometry round (see truncation_drains… for the settle contract).
    h.turn();
    h.turn();
    let tail = h.turn();
    assert!(tail.contains("the tail card"), "sanity: pinned to the tail");

    // Focus the transcript, then page up: navigation must stay
    // navigation, and nothing may land in the draft.
    h.type_text("\t");
    h.turn();
    h.term.push_input(b"\x1b[5~");
    let scrolled = h.turn();
    assert!(
        !scrolled.contains("the tail card"),
        "PageUp still scrolls the transcript:\n{scrolled}"
    );
    assert!(
        scrolled.contains("describe a task"),
        "the draft stays empty — no navigation key became a character:\n{scrolled}"
    );

    // Ctrl+D reaches the root shortcut table even while the transcript
    // holds focus (the handler claims plain characters only).
    let before = h.store.show_details.get_untracked();
    h.term.push_input(&[0x04]);
    h.turn();
    assert_ne!(
        h.store.show_details.get_untracked(),
        before,
        "Ctrl+D still reaches the root shortcut"
    );
}

/// The working indicator on the activity strip is the app's block wave,
/// not the engine's one-cell spinner (operator report, 2026-08-19: the
/// old dot was too easy to miss). Pins that it renders while a run is
/// live, and that it MOVES as frames advance — a frozen wave would look
/// identical to a hung run.
#[test]
fn the_activity_strip_shows_a_moving_wave_while_a_run_is_live() {
    let mut h = harness();
    h.turn();
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("root".into());
    h.store.fold.update(|f| f.begin_run("root"));
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User { text: "hi".into() });
    });

    let wave_row = |screen: &str| -> Option<String> {
        screen
            .lines()
            .find(|l| l.contains("working"))
            .map(|l| l.trim_end().to_string())
    };

    let first = wave_row(&h.turn()).expect("the strip names the running state");
    assert!(
        first
            .chars()
            .any(|c| ('\u{2581}'..='\u{2588}').contains(&c)),
        "the wave draws block cells:\n{first}"
    );

    // Advance past the strip ticker's 120ms cadence: the picture must
    // change (the wave's own unit tests pin the shape; this pins that
    // the frame signal actually reaches the strip).
    let mut moved = false;
    for _ in 0..6 {
        std::thread::sleep(std::time::Duration::from_millis(130));
        if wave_row(&h.turn()).is_some_and(|row| row != first) {
            moved = true;
            break;
        }
    }
    assert!(
        moved,
        "the wave never advanced — a frozen run reads as hung"
    );
}

// ---------------------------------------------------------------------------
// RENDER GALLERY (dev tool, #[ignore]d): dumps the transcript rendering
// as text + SVG screens for design review — the working surface of the
// 2026-08-19 turn-readability redesign. Not a regression test: it
// asserts nothing beyond "renders".
//
//   cargo test --test headless_ui render_gallery -- --ignored
//
// Writes into $GALLERY_DIR (or target/gallery).
// ---------------------------------------------------------------------------

fn gallery_fold(store: &Store) {
    use abstractcode::transcript::{CallCost, Item, ToolStatus};
    store.fold.update(|f| {
        f.begin_run("root");
        f.push_item(Item::User {
            text: "the game freezes after level 3 — find the root cause and fix it".into(),
        });
        f.push_item(Item::Thinking {
            iteration: 5,
            content: "I've found the root cause. Let me read the rest of `game.js` to see the full picture before deciding on the fix.".into(),
            reasoning: "The freeze reproduces exactly when the invincibility timer underflows: `player.invincible` is decremented every frame but never clamped, so after level 3's long shield pickup it wraps negative and the collision loop spins. I should confirm the update() path before patching — the audio loop also touches this counter, and a blind clamp could mask the audio bug instead of fixing the freeze. Reading the remaining sections of game.js first.".into(),
            call: CallCost {
                gen_time_ms: Some(332_000.0),
                input_tokens: 26_412,
                output_tokens: 348,
                cached_tokens: 0,
            },
        });
        f.push_item(Item::Tool {
            key: "call:1".into(),
            name: "read_file".into(),
            args_preview: "…/todel2/js/game.js  start_line=720  end_line=1187".into(),
            args_full: String::new(),
            status: ToolStatus::Ok,
            result: "File: /workspace/todel2/js/game.js (468 lines)\n720:     ctx.fillStyle = p.color;\n721:     ctx.beginPath();\n722:     ctx.arc(p.x, p.y, p.size + pulse, 0, Math.PI * 2);\n723:     ctx.fill();\n… (+18 more lines)".into(),
            error: String::new(),
        });
        f.push_item(Item::Tool {
            key: "call:2".into(),
            name: "execute_command".into(),
            args_preview: "wc -l js/game.js js/audio.js js/levels.js index.html  working_directory=…/todel2".into(),
            args_full: String::new(),
            status: ToolStatus::Ok,
            result: "🖥 Command executed on Darwin\n⏱ Execution time: 0.09s · ✅ return code 0\n📤 Output:\n  301 js/game.js\n  212 js/audio.js\n  164 js/levels.js\n   89 index.html\n  766 total".into(),
            error: String::new(),
        });
        f.push_item(Item::Thinking {
            iteration: 6,
            content: "The collision loop never resets the invincibility timer — that is the freeze. I'll patch `update()` to clamp it at zero and re-run the level-3 repro.".into(),
            reasoning: String::new(),
            call: CallCost {
                gen_time_ms: Some(41_000.0),
                input_tokens: 28_907,
                output_tokens: 512,
                cached_tokens: 26_530,
            },
        });
        f.push_item(Item::Tool {
            key: "call:3".into(),
            name: "edit_file".into(),
            args_preview: "js/game.js  old=if (player.invincible > 0) player.invincible--;  new=player.invincible = Math.max(0, player.invincible - 1);".into(),
            args_full: String::new(),
            status: ToolStatus::Ok,
            result: "edited js/game.js: 1 replacement".into(),
            error: String::new(),
        });
        f.push_item(Item::Tool {
            key: "call:4".into(),
            name: "execute_command".into(),
            args_preview: "node smoke.js --level 3".into(),
            args_full: String::new(),
            status: ToolStatus::Failed,
            result: String::new(),
            error: "node smoke.js exited 1: TypeError: player.reset is not a function (smoke.js:41)".into(),
        });
        f.push_item(Item::Thinking {
            iteration: 7,
            content: "The smoke harness is stale — `player.reset` moved into `respawn()` last refactor. Fixing the harness call, then re-running.".into(),
            reasoning: "Not a regression from my clamp: smoke.js:41 calls player.reset() which was renamed in commit 7abb716. The game code is fine; the harness needs the new name.".into(),
            call: CallCost {
                gen_time_ms: Some(18_400.0),
                input_tokens: 29_961,
                output_tokens: 233,
                cached_tokens: 29_004,
            },
        });
        f.push_item(Item::Tool {
            key: "call:5".into(),
            name: "edit_file".into(),
            args_preview: "smoke.js  old=player.reset()  new=player.respawn()".into(),
            args_full: String::new(),
            status: ToolStatus::Ok,
            result: "edited smoke.js: 1 replacement".into(),
            error: String::new(),
        });
        f.push_item(Item::Tool {
            key: "call:6".into(),
            name: "execute_command".into(),
            args_preview: "node smoke.js --level 3".into(),
            args_full: String::new(),
            status: ToolStatus::Running,
            result: String::new(),
            error: String::new(),
        });
        // Design sheet, not a replay: the final answer renders below a
        // still-running row so ONE screen shows both treatments.
        f.push_item(Item::Assistant {
            text: "Fixed. The freeze was an **invincibility timer underflow**: `player.invincible` decremented past zero and the collision loop spun forever.\n\n- `js/game.js` — clamp the timer at zero in `update()`\n- `smoke.js` — the harness called the renamed `player.reset()`; now `respawn()`\n\nLevel 3 survives 500 frames in the smoke run.".into(),
            final_answer: true,
        });
    });
}

#[test]
#[ignore = "render gallery: writes design-review screens, run explicitly"]
fn render_gallery() {
    let dir = std::env::var("GALLERY_DIR").unwrap_or_else(|_| "target/gallery".into());
    std::fs::create_dir_all(&dir).expect("gallery dir");
    let mut shots: Vec<(String, String, String)> = Vec::new(); // (name, text, svg)

    for (label, size) in [("wide", Size::new(160, 45)), ("narrow", Size::new(100, 30))] {
        let mut h = harness_sized(size);
        h.turn();
        gallery_fold(&h.store);
        for _ in 0..4 {
            h.turn();
        }
        let text = h.turn();
        let svg = h.term.screen().screenshot().to_svg();
        shots.push((format!("{label}-collapsed"), text, svg));

        h.store.show_details.set(true);
        for _ in 0..4 {
            h.turn();
        }
        let text = h.turn();
        let svg = h.term.screen().screenshot().to_svg();
        shots.push((format!("{label}-full"), text, svg));
    }

    for (name, text, svg) in shots {
        std::fs::write(format!("{dir}/{name}.txt"), &text).expect("write txt");
        std::fs::write(format!("{dir}/{name}.svg"), &svg).expect("write svg");
    }
    eprintln!("gallery written to {dir}");
}

/// The composer must GROW with the draft (1..4 rows) and keep the caret
/// row visible — even under a long transcript.
///
/// Live report (2026-08-20): the prompt panel stopped at two rows and
/// then scrolled the text out from under the caret — typing past the
/// visible rows became blind. Cause: the composer row was flex-
/// SHRINKABLE inside the chrome column, whose transcript sibling
/// carries a content-sized basis of hundreds of rows, so the CSS-scaled
/// shrink pass took rows back from the composer. The TextArea's own
/// `shrink(0)` saved the WIDGET, not its rect: the engine drew a 4-row
/// widget inside a 2-row rect, and the widget's scroll window (computed
/// against `max_rows`, not the drawn height) parked the caret on a row
/// the clip ate. The fix is `shrink(0.0)` on the composer row.
#[test]
fn composer_grows_to_four_rows_and_keeps_the_caret_row_visible() {
    let mut h = harness_sized(Size::new(100, 30));
    // A transcript long enough to dominate the column's flex basis —
    // the exact condition under which the crush appeared live.
    h.store.fold.update(|f| {
        for i in 0..60 {
            f.push_item(abstractcode::transcript::Item::User {
                text: format!("history line {i}"),
            });
        }
    });
    for _ in 0..3 {
        h.turn();
    }
    // Eight wrapped rows of draft: past `max_rows`, so the widget must
    // scroll internally and show the LAST four, caret row included.
    let mut draft = String::new();
    for i in 1..=8 {
        draft.push_str(&format!("LINE{i}{} ", "x".repeat(88)));
    }
    h.type_text(&draft);
    h.turn();
    let screen = h.turn();
    let rows: Vec<&str> = screen.lines().collect();
    let composer_rows = rows
        .iter()
        .filter(|l| l.contains('▐') && l.contains('▌'))
        .count();
    assert_eq!(
        composer_rows, 4,
        "the composer must grow to its full 4 rows, never be flex-crushed:\n{screen}"
    );
    // Following the caret: the newest row is on screen, the oldest is not.
    assert!(
        screen.contains("LINE8"),
        "the caret's row must stay visible as the draft grows:\n{screen}"
    );
    assert!(
        !screen.contains("LINE4"),
        "the window must ride the caret, not the buffer head:\n{screen}"
    );
}

#[test]
fn cache_modal_reports_three_scopes_and_scrolls_to_its_tail() {
    // The /cache panel carries more rows than any sane modal height, so
    // the tail (session block + reading notes) is only reachable if the
    // body genuinely SCROLLS. The previous fixed-window renderer printed
    // "↓ N more" and no key could move it — a metric that cannot be read
    // is not reported.
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.cache.set(Some(abstractcode::store::CacheInfo {
        provider: "mlx".into(),
        model: "mlx-community/Qwen3.8-27B-4bit".into(),
        supported: true,
        mode: "local_control_plane".into(),
    }));
    let llm = |input: u64, output: u64, cached: u64| {
        serde_json::json!({
            "run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "x", "model": "mlx-community/Qwen3.8-27B-4bit",
                "gen_time": 12_000.0,
                "usage": {"input_tokens": input, "output_tokens": output,
                          "total_tokens": input + output,
                          "prompt_tokens_details": {"cached_tokens": cached}}}
        })
    };
    // Run 1 (closed), then run 2 — so the session block reports strictly
    // more than the run block, which is the whole point of adding it.
    store.fold.update(|f| {
        f.begin_run("root");
        let _ = f.apply("root", &llm(4_000, 200, 0));
        let _ = f.apply("root", &llm(9_000, 300, 3_000));
        f.begin_run("root");
        let _ = f.apply("root", &llm(6_200, 150, 0));
        let _ = f.apply("root", &llm(10_000, 558, 6_000));
    });

    h.type_text("/cache");
    h.turn();
    h.press_enter();
    let head = h.turn();
    assert!(head.contains("prompt cache + context"), "{head}");
    assert!(
        head.contains("latest model call"),
        "the newest call block:\n{head}"
    );
    assert!(
        head.contains("carried forward"),
        "the derived reuse split:\n{head}"
    );
    // The session block is BELOW the fold at this height — proving the
    // scroll is load-bearing rather than decorative.
    assert!(
        !head.contains("how to read this"),
        "the tail starts off-screen (otherwise this test proves nothing):\n{head}"
    );

    // Page down: everything below the fold must become readable. The
    // union of the frames is what "reachable by keyboard" means — no
    // single frame holds the whole panel, and that is fine.
    let mut seen = head.clone();
    for _ in 0..8 {
        h.term.push_input(b"\x1b[6~"); // PgDn
        seen.push_str(&h.turn());
    }
    let tail = h.turn();
    seen.push_str(&tail);
    for needle in [
        "session (every run in this conversation)",
        "runs               2",
        "model calls        4",
        "how to read this",
    ] {
        assert!(
            seen.contains(needle),
            "{needle:?} never became readable while paging down:\n{tail}"
        );
    }
    // Run scope vs session scope are DIFFERENT numbers here (run 2 sent
    // 16.2k, the session 29.2k) — the session block is not a relabeled
    // copy of the run block.
    assert!(
        seen.contains("16,200 in") && seen.contains("29,200 in"),
        "run and session totals are distinct:\n{seen}"
    );

    // …and back up: the head returns (the scroll is two-way, and the
    // panel is not a one-shot dump).
    for _ in 0..12 {
        h.term.push_input(b"\x1b[5~"); // PgUp
        h.turn();
    }
    let back = h.turn();
    assert!(
        back.contains("latest model call"),
        "scrolls back to the head:\n{back}"
    );

    // Esc still closes with the scroll focused — the title advertises it.
    h.press_escape();
    let closed = h.turn();
    assert!(
        !closed.contains("prompt cache + context"),
        "Esc closes the panel from a focused scroll:\n{closed}"
    );
}

#[test]
fn cache_modal_never_reports_a_cold_cache_when_the_provider_is_silent() {
    // Honesty pin at the SCREEN, not just the row builder: a provider
    // that never reports hit counts must produce "not reported", never a
    // 0 that reads as a measured miss. Driven at 80x24 — the small
    // terminal is where a fixed-width panel eats its own numbers.
    let mut h = harness_sized(Size::new(80, 24));
    h.turn();
    let store = h.store;
    store.fold.update(|f| {
        f.begin_run("root");
        for i in 0..3 {
            let _ = f.apply(
                "root",
                &serde_json::json!({
                    "run_id": "root", "node_id": "reason", "status": "completed",
                    "effect": {"type": "llm_call", "payload": {}},
                    "result": {"content": "x", "usage": {
                        "input_tokens": 1_000 * (i + 1), "output_tokens": 50,
                        "total_tokens": 1_000 * (i + 1) + 50}}
                }),
            );
        }
    });
    h.type_text("/cache");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("never reported by this provider"),
        "silence named as silence:\n{screen}"
    );
    assert!(
        !screen.contains("0 tk served"),
        "silence must never render as a measured miss:\n{screen}"
    );
    // …and at 80 columns the value wraps into its own column instead of
    // losing its tail: the continuation line must be on screen.
    assert!(
        screen.contains("split below is derived client-side"),
        "values wrap at a narrow width rather than truncating:\n{screen}"
    );
}

/// `/details` shows ALL the details — nothing is shortened, anywhere.
///
/// Operator report, 2026-08-20: `[#TRUNCATION: shortened for display]`
/// and `… (+57 more lines)` were appearing in the FULL view. Two layers
/// were cutting: the fold stored preview-bounded copies (700 chars of a
/// tool result, 200 of an error, 8k of a thinking block), so the text
/// was gone before any view could decide; and the details render still
/// applied the folded view's row caps.
///
/// EVERY capped arm of `render_item` is exercised here, each with a body
/// past its own cap plus `CAP_SLACK`, and each asserted by a unique tail
/// marker — the first cut of this test only covered two arms, and the
/// adversarial review proved the other five assertions were no-ops
/// (findings F1/F7). A failed tool carrying BOTH an error and output is
/// included: that output used to be dropped entirely (F3).
#[test]
fn details_mode_truncates_nothing() {
    // Tall enough to hold the whole feed: the point is to read every
    // rendered row, not to model a real terminal.
    let mut h = harness_sized(Size::new(120, 1400));
    h.turn();
    let lines = |prefix: &str, n: usize, tail: &str| -> String {
        (1..=n)
            .map(|i| format!("{prefix}-{i}\n"))
            .collect::<String>()
            + tail
    };
    let result = lines("out-line", 400, "TAIL-OF-RESULT");
    let thinking = format!("HEAD-OF-THINKING {} TAIL-OF-THINKING", "z ".repeat(4_000));
    let args_full = lines("arg-line", 40, "TAIL-OF-ARGS");
    let tool_error = lines("err-line", 40, "TAIL-OF-TOOLERROR");
    h.store.fold.update(|f| {
        use abstractcode::transcript::Item;
        f.push_item(Item::User {
            text: lines("u-line", 300, "TAIL-OF-USER"),
        });
        f.push_item(Item::Steer {
            text: lines("s-line", 80, "TAIL-OF-STEER"),
        });
        f.push_item(Item::Thinking {
            iteration: 1,
            content: thinking.clone(),
            reasoning: String::new(),
            call: abstractcode::transcript::CallCost::default(),
        });
        f.push_item(Item::Tool {
            key: "call:1".into(),
            name: "execute_command".into(),
            args_preview: "cargo test".into(),
            args_full: args_full.clone(),
            status: abstractcode::transcript::ToolStatus::Ok,
            result: result.clone(),
            error: String::new(),
        });
        // Failed WITH output: both must render (F3).
        f.push_item(Item::Tool {
            key: "call:2".into(),
            name: "broken_tool".into(),
            args_preview: String::new(),
            args_full: String::new(),
            status: abstractcode::transcript::ToolStatus::Failed,
            result: lines("fail-out", 20, "TAIL-OF-FAILED-OUTPUT"),
            error: tool_error.clone(),
        });
        f.push_item(Item::Info {
            text: lines("info-line", 30, "TAIL-OF-INFO"),
        });
        f.push_item(Item::Error {
            text: lines("erritem-line", 30, "TAIL-OF-ERRORITEM"),
        });
        f.push_item(Item::Probe {
            title: "memory digest".into(),
            body: lines("probe-line", 30, "TAIL-OF-PROBE"),
        });
        f.push_item(Item::Image {
            run_id: "root".into(),
            artifact_id: "img-1".into(),
            label: "chart".into(),
        });
    });
    // An image whose fetch FAILED: its reason is a body like any other.
    h.store.upsert_image(abstractcode::store::ImageEntry {
        artifact_id: "img-1".into(),
        bitmap: None,
        error: lines("imgerr-line", 20, "TAIL-OF-IMAGEERROR"),
    });
    h.type_text("/details");
    h.turn();
    h.press_enter();
    for _ in 0..3 {
        h.turn();
    }
    let screen = h.turn();
    for marker in ["[#TRUNCATION", "more lines)"] {
        assert!(
            !screen.contains(marker),
            "details mode must not shorten anything — found {marker:?}"
        );
    }
    for needle in [
        // first line and last line of every body
        "out-line-1",
        "out-line-400",
        "TAIL-OF-RESULT",
        "HEAD-OF-THINKING",
        "TAIL-OF-THINKING",
        "arg-line-1",
        "TAIL-OF-ARGS",
        "err-line-1",
        "TAIL-OF-TOOLERROR",
        "TAIL-OF-FAILED-OUTPUT",
        "u-line-1",
        "TAIL-OF-USER",
        "TAIL-OF-STEER",
        "TAIL-OF-INFO",
        "TAIL-OF-ERRORITEM",
        "TAIL-OF-PROBE",
        "imgerr-line-1",
        "TAIL-OF-IMAGEERROR",
    ] {
        assert!(screen.contains(needle), "details mode must keep {needle:?}");
    }
}

/// The FOLDED view is a summary and must stay one: the opposite bug
/// (everything unbounded everywhere) would bury the operator in a wall
/// of output with no way back. Bodies are capped there, the cut is
/// LABELED, and a tool row stays exactly one line even though the fold
/// now stores multi-line errors.
#[test]
fn folded_view_stays_a_bounded_labelled_summary() {
    let mut h = harness_sized(Size::new(120, 60));
    h.turn();
    h.store.fold.update(|f| {
        use abstractcode::transcript::Item;
        f.push_item(Item::User {
            text: (1..=300)
                .map(|i| format!("u-line-{i}\n"))
                .collect::<String>(),
        });
        f.push_item(Item::Tool {
            key: "call:1".into(),
            name: "broken_tool".into(),
            args_preview: "cargo test".into(),
            args_full: "cargo test".into(),
            status: abstractcode::transcript::ToolStatus::Failed,
            result: String::new(),
            error: "line one\nline two\nline three\nline four\nline five".into(),
        });
    });
    for _ in 0..3 {
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("more lines)"),
        "the folded view caps long bodies and LABELS the cut:\n{screen}"
    );
    assert!(
        !screen.contains("u-line-300"),
        "the folded view does not render a 300-line prompt whole:\n{screen}"
    );
    // The folded tool row is one line + at most its 3 error rows.
    assert!(
        screen.contains("broken_tool") && screen.contains("line one"),
        "the folded row still names the call and its error:\n{screen}"
    );
}

/// An undelivered steer is never silently swallowed: the words ride the
/// error card AND come back to an empty composer, so the operator can
/// resend with one Enter.
///
/// Live failure, 2026-08-20: a steer died on a reset socket
/// ("steer not delivered: … Connection reset by peer (os error 54)") and
/// the paragraph the operator had typed was gone — the composer clears
/// on submit, and nothing put it back.
#[test]
fn an_undelivered_steer_keeps_the_words_and_restores_the_composer() {
    let mut h = harness_sized(Size::new(120, 40));
    h.turn();
    let words = "no, i really want you to find a way to use AbstractTUI";
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::Error {
            text: format!("steer not delivered: boom\n\n— your steer —\n{words}"),
        });
    });
    // The runner posts this after a failed send; root() drains it.
    h.store.steer_restore.set(Some(words.to_string()));
    for _ in 0..3 {
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("steer not delivered"),
        "the failure is stated:\n{screen}"
    );
    assert!(
        screen.contains("find a way to use AbstractTUI"),
        "the operator's words survive in the card:\n{screen}"
    );
    // …and the composer holds them again: submitting now resends.
    assert!(
        screen.matches("find a way to use AbstractTUI").count() >= 2,
        "the words are back in the composer too:\n{screen}"
    );
}

/// A draft typed after the failure OUTRANKS the restore — the restore
/// must never clobber words the operator is in the middle of writing.
#[test]
fn a_steer_restore_never_clobbers_a_draft() {
    let mut h = harness_sized(Size::new(120, 40));
    h.turn();
    h.type_text("a newer draft");
    h.turn();
    h.store.steer_restore.set(Some("the failed steer".into()));
    for _ in 0..3 {
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("a newer draft"),
        "the draft in progress wins:\n{screen}"
    );
    assert!(
        !screen.contains("the failed steer"),
        "the restore stands down rather than overwrite it:\n{screen}"
    );
}

/// Boot gallery (design review, 2026-08-21): the FIRST screen — the one
/// the boot animation hands off to — captured at the sizes where the
/// hero lockup is affordable and at the size where it degrades back to
/// the compact `▲` lockup. Ignored by default like `render_gallery`;
/// `GALLERY_DIR=… cargo test --test headless_ui boot_gallery -- --ignored`
/// writes SVG + text so the handoff can be looked at, not guessed at.
#[test]
#[ignore = "render gallery: writes design-review screens, run explicitly"]
fn boot_gallery() {
    let dir = std::env::var("GALLERY_DIR").unwrap_or_else(|_| "target/gallery".into());
    std::fs::create_dir_all(&dir).expect("gallery dir");
    for (label, size) in [
        ("boot-160x45", Size::new(160, 45)),
        ("boot-120x40", Size::new(120, 40)),
        ("boot-100x36", Size::new(100, 36)),
        ("boot-100x30", Size::new(100, 30)),
        ("boot-80x24", Size::new(80, 24)),
    ] {
        let mut h = harness_sized(size);
        // Frame 0 is the entrance; step the ticker signal by hand so the
        // capture shows the lockup fully arrived as well.
        for frame in [0u64, 1, 4] {
            h.store.fold.update(|_f| {});
            h.turn();
            let text = h.turn();
            let svg = h.term.screen().screenshot().to_svg();
            std::fs::write(format!("{dir}/{label}-f{frame}.txt"), &text).expect("txt");
            std::fs::write(format!("{dir}/{label}-f{frame}.svg"), &svg).expect("svg");
        }
    }
    eprintln!("boot gallery written to {dir}");
}

/// Animation gallery (design review, 2026-08-21): the three `/animation`
/// variants over a synthetic run, at a normal and a small pane. Ignored
/// like the other galleries; `GALLERY_DIR=… cargo test --test headless_ui
/// animation_gallery -- --ignored` writes SVG + text.
#[test]
#[ignore = "render gallery: writes design-review screens, run explicitly"]
fn animation_gallery() {
    use abstractcode::store::Phase;
    use abstractcode::transcript::{CallCost, Item, ToolStatus};
    let dir = std::env::var("GALLERY_DIR").unwrap_or_else(|_| "target/gallery".into());
    std::fs::create_dir_all(&dir).expect("gallery dir");

    for (label, size) in [
        ("anim-120x36", Size::new(120, 36)),
        ("anim-80x24", Size::new(80, 24)),
    ] {
        let mut h = harness_sized(size);
        h.turn();
        gallery_fold(&h.store);
        // A longer synthetic run so the chart has a shape: alternating
        // cycles and tools, with a failing streak in the middle.
        h.store.fold.update(|f| {
            for i in 0..24u32 {
                f.push_item(Item::Thinking {
                    iteration: i,
                    content: "…".into(),
                    reasoning: String::new(),
                    call: CallCost {
                        gen_time_ms: Some(4000.0),
                        input_tokens: 20_000 + i as u64 * 900,
                        output_tokens: 120 + (i as u64 * 37) % 400,
                        cached_tokens: 12_000,
                    },
                });
                for k in 0..2u32 {
                    let n = i * 2 + k;
                    let (name, args) = match n % 5 {
                        0 => ("read_file", "src/ui/mosaic.rs start_line=1"),
                        1 => ("search_files", "pattern=quantize path=src"),
                        2 => ("edit_file", "src/gfx/dither.rs"),
                        3 => ("execute_command", "cargo test --quiet"),
                        _ => ("fetch_url", "https://example.invalid/spec"),
                    };
                    f.push_item(Item::Tool {
                        key: format!("gcall:{n}"),
                        name: name.into(),
                        args_preview: args.into(),
                        args_full: String::new(),
                        status: if (10..14).contains(&n) {
                            ToolStatus::Failed
                        } else {
                            ToolStatus::Ok
                        },
                        result: "ok".into(),
                        error: String::new(),
                    });
                }
            }
        });
        h.store.phase.set(Phase::Running);
        h.store.elapsed_secs.set(2_615);
        h.store.context_window.set(262_144);
        h.store.last_call_rate.set(Some(38.0));
        for variant in 1..=3u8 {
            h.store.animation.set(variant);
            for _ in 0..6 {
                h.turn();
            }
            let text = h.turn();
            let svg = h.term.screen().screenshot().to_svg();
            std::fs::write(format!("{dir}/{label}-v{variant}.txt"), &text).expect("txt");
            std::fs::write(format!("{dir}/{label}-v{variant}.svg"), &svg).expect("svg");
        }
        h.store.animation.set(0);
    }
    eprintln!("animation gallery written to {dir}");
}

/// The ambient pane replaces the PANE and nothing else: the composer,
/// the chrome and the status bar stay live behind it, and it says how to
/// get out. (The feature's safety case rests on the exit being obvious
/// and cheap.)
///
/// NOTE: no command sets `store.animation` today — the `/animation`
/// surface is deliberately closed (`docs/backlog/proposed/
/// ambient-run-animations.md`). The signal is set directly here so the
/// pane, its exit and its honesty contract stay under test while the
/// door is shut.
#[test]
fn animation_replaces_the_pane_and_says_how_to_leave() {
    let mut h = harness_sized(Size::new(120, 36));
    h.turn();
    h.store.animation.set(1);
    for _ in 0..4 {
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("Esc or click returns to the transcript"),
        "the way out is on screen:\n{screen}"
    );
    // The composer is still there: the animation never takes the input.
    assert!(
        screen.contains("Enter sends") || screen.contains("Enter steers"),
        "the composer stays live:\n{screen}"
    );
}

/// Esc leaves the animation, CONSUMES the press, and clears the cancel
/// arm. Esc in this app is quadruple-loaded (clear draft, jump to tail,
/// arm cancel, fire cancel) — a user tapping it twice to get the words
/// back must never reach "cancel the run".
#[test]
fn escape_leaves_the_animation_without_arming_cancel() {
    use abstractcode::store::Phase;
    let mut h = harness_sized(Size::new(120, 36));
    h.turn();
    h.store.phase.set(Phase::Running);
    h.store.animation.set(2);
    h.turn();
    h.press_escape();
    for _ in 0..3 {
        h.turn();
    }
    assert_eq!(
        h.store.animation.get_untracked(),
        0,
        "Esc returns to the transcript"
    );
    assert!(
        h.store.last_esc.get_untracked().is_none(),
        "the press is CONSUMED — the cancel arm must not be set by the exit"
    );
    // A second Esc arms cancel exactly as it always did (the ladder is
    // intact, just one rung longer). The arm STATE is the assertion —
    // its toast drains, as the older Esc test notes.
    h.press_escape();
    for _ in 0..3 {
        h.turn();
    }
    assert!(
        h.store.last_esc.get_untracked().is_some(),
        "the normal cancel ladder still works after the exit"
    );
}

/// A run that is DOWN or idle must never be drawn as working: the state
/// line under every variant is computed from the same signals the
/// activity strip uses, and it is the animation's honesty contract.
#[test]
fn the_animation_states_the_truth_about_a_dead_gateway() {
    use abstractcode::store::{Conn, Phase};
    let mut h = harness_sized(Size::new(120, 36));
    h.turn();
    h.store.phase.set(Phase::Running);
    h.store.animation.set(1);
    h.store.conn.set(Conn::Down(
        "gateway unreachable: connection refused".into(),
        true,
    ));
    for _ in 0..3 {
        h.turn();
    }
    let screen = h.turn();
    assert!(
        screen.contains("gateway not answering"),
        "a dead gateway says so on the animation pane:\n{screen}"
    );
}

/// A turn that STOPPED before finishing must not drain the queue.
///
/// `/help` promises the queue "auto-runs after the current run succeeds;
/// halts on failure/cancel". A budget-exhausted or stuck-loop turn is neither
/// success nor failure, and it used to be reported as `Success` — so the next
/// queued prompt was stacked on top of incomplete work, which is the one
/// outcome where continuing compounds the problem. The chrome, the card and
/// the exit code all called that turn unfinished; only the queue disagreed.
#[test]
fn a_turn_that_stopped_short_holds_the_queue_instead_of_draining_it() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    h.turn();
    h.type_text("/queue follow-up work");
    h.turn();
    h.press_enter();
    h.turn();
    assert_eq!(store.queue.with_untracked(|q| q.len()), 1, "queued");

    simulate_terminal(store, abstractcode::store::RunOutcome::StoppedShort);
    h.turn();
    h.turn();

    assert_eq!(
        store.queue.with_untracked(|q| q.len()),
        1,
        "the held prompt must NOT be started on top of an unfinished turn"
    );
    assert!(
        store.queue_paused.get_untracked(),
        "the queue pauses, exactly as it does on failure/cancel"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::Start { prompt, .. } if prompt == "follow-up work"))
            .is_none(),
        "no start command was issued"
    );
}

/// One panel row's WHOLE text, rejoined. The resources panel WRAPS long
/// rows (nothing is truncated off the right edge), so a two-word claim like
/// `ctx 28672*` or `cache 2.0 GiB` can straddle the fold — and the fold moves
/// whenever a cell's width changes, as it did when the binary unit strings
/// (`GiB`, one cell wider than `GB`) landed. An assertion here is about the
/// TEXT, never about where the terminal happened to break it. Continuation
/// lines are the ones indented past the row's own indent; the right-hand
/// scrollbar/border glyphs are dropped.
fn panel_row_text(screen: &str, needle: &str) -> String {
    fn clean(l: &str) -> &str {
        l.trim_end_matches(['█', '▌', '▐', ' ']).trim_start()
    }
    let mut lines = screen.lines().skip_while(|l| !l.contains(needle));
    let first = lines
        .next()
        .unwrap_or_else(|| panic!("no row for {needle}:\n{screen}"));
    let indent = first.len() - first.trim_start().len();
    let mut parts = vec![clean(first).to_string()];
    for l in lines {
        let body = l.trim_start();
        if body.is_empty() || l.len() - body.len() <= indent {
            break;
        }
        parts.push(clean(l).to_string());
    }
    parts
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `/resources` end to end: contracts gate, the modal's honest rendering
/// from parsed host facts, and the `u` → confirm → `Cmd::UnloadModel`
/// action wiring (the tri-state residency and lock marker render on the
/// real screen, not just in the pure row builder).
#[test]
fn resources_modal_renders_host_facts_and_unload_confirms_before_sending() {
    use abstractcode::store::{HostContracts, HostState};

    let mut h = harness();
    h.leave_splash();

    // Contracts declared + a parsed /host/state snapshot (the same pure
    // parser the worker uses, over the wire shape).
    h.store.host_contracts.set(Some(HostContracts {
        model_residency: true,
        host_state: true,
        session_caches: true,
        modality_labels: vec![("text-generation".into(), "LLM".into())],
    }));
    let facts = abstractcode::discovery::host_state_from_response(&serde_json::json!({
        "ok": true,
        "memory": {
            "ram": {"total_bytes": 137438953472u64, "used_bytes": 85238953472u64,
                     "percent": 62.0},
            "process": {"rss_bytes": 512000000u64},
            // The LIVE device block: `allocated_bytes` is PROCESS-LOCAL
            // and reads 0 while ~98 GB of weights are resident.
            "device": {"backend": "metal", "allocated_bytes": 0u64,
                        "total_bytes": 137438953472u64,
                        "host_in_use_bytes": 105743990784u64,
                        "wired_limit_bytes": 115343360000u64},
            "host": {"host_name": "studio.local"}
        },
        "gpu": {"supported": true, "utilization_gpu_pct": 21.0},
        "models": [
            {"task": "text-generation", "provider": "lmstudio", "model": "qwen3-4b",
             "resident": true, "state": "loaded", "locked": true, "lockable": true,
             "size_bytes": 4508876800u64, "cache_bytes": 268435456u64,
             "context_length": 32768,
             "calibrated_context_length": 28672, "context_calibrated": true},
            {"task": "text-generation", "provider": "ollama", "model": "phi4",
             "resident": null, "locked": false, "lockable": true},
            // The host-sweep row AS THE WIRE EMITS IT: LM Studio holds
            // it, so the gateway stamps `source: "provider_server"` and
            // `lockable: true` — the adopt wording keys on the SOURCE,
            // never on `lockable`. Only an ESTIMATE of its weights was
            // ever reported.
            {"runtime_id": "rt-glm", "task": "text-generation", "provider": "lmstudio",
             "model": "glm-4.6-gguf",
             "source": "provider_server", "resident": true, "state": "provider_loaded",
             "locked": false, "lockable": true, "size_bytes": null,
             "est_weights_bytes": 95563022336u64, "cache_bytes": 2147483648u64}
        ],
        "session_caches": [
            {"key": "k1", "provider": "lmstudio", "model": "qwen3-4b",
             "session_id": "acode-abc", "bytes": 1048576, "token_count": 2100}
        ],
        "totals": {"resident_models": 2}
    }));
    h.store.host_state.set(HostState::Ready(facts));

    h.type_text("/resources");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("host resources") && screen.contains("studio.local"),
        "modal opens on the host facts:\n{screen}"
    );
    assert!(
        screen.contains("62%"),
        "ram meter from served percent:\n{screen}"
    );
    // THE METAL BUG: the accelerator line shows the all-processes pair
    // and names its scope in the EXACT spec label — never the
    // process-local 0 beside a full machine, and never a word that reads
    // as the whole machine's memory.
    let dev = screen
        .lines()
        .find(|l| l.contains("Accelerator heap"))
        .expect("the accelerator line renders");
    assert!(dev.contains("98.5 GiB / 107.4 GiB"), "{dev}");
    assert!(
        dev.contains("Accelerator heap · metal (all processes)"),
        "{dev}"
    );
    assert!(
        !dev.contains("0 B"),
        "the process-local zero is not the bar: {dev}"
    );
    assert!(!screen.contains("host-wide"), "spec PART A3:\n{screen}");
    // The note is the DIM SUB-LINE directly under the meter — without it
    // the figure reads as "how full is this machine", which it is not.
    let note = screen
        .lines()
        .position(|l| l.contains("Accelerator heap"))
        .map(|ix| screen.lines().nth(ix + 1).unwrap_or_default().to_string())
        .expect("the accelerator line renders");
    assert!(
        note.contains("memory-mapped GGUF weights are not counted here"),
        "the note rides with the number:\n{screen}"
    );
    // Under the meters: WHAT is consuming the memory, itemized — and NO
    // remainder, in any spelling (spec PART B4).
    assert!(screen.contains("consuming memory"), "{screen}");
    assert!(!screen.contains("nattributed"), "{screen}");
    // The items render AT REST, on the default terminal: the body opens
    // at its own top, so nothing above the first cursor target is
    // stranded behind a "↓ N more" no key could move.
    assert!(screen.contains("qwen3-4b"), "{screen}");
    assert!(
        screen.contains("~89.0 GiB"),
        "the MARKED estimate:\n{screen}"
    );
    assert!(screen.contains("model KV caches"), "{screen}");
    assert!(screen.contains("gateway process RSS"), "{screen}");
    // The open dispatched a fresh fetch (open + r are the ONLY fetch
    // moments — no polling lane exists for /host/state).
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadHostState)).is_some(),
        "opening /resources fetches host state"
    );

    // `u` on the selected (first) model ARMS a confirm — nothing sent yet.
    h.type_text("u");
    let screen = h.turn();
    assert!(
        screen.contains("unload lmstudio/qwen3-4b?"),
        "confirm line armed:\n{screen}"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::UnloadModel { .. }))
            .is_none(),
        "no unload before the confirm"
    );
    // `y` confirms → the exact command, force=false.
    h.type_text("y");
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::UnloadModel { .. })) {
        Some(Cmd::UnloadModel {
            provider,
            model,
            force,
        }) => {
            assert_eq!(
                (provider.as_str(), model.as_str()),
                ("lmstudio", "qwen3-4b")
            );
            assert!(!force, "plain u never forces");
        }
        other => panic!("expected UnloadModel, got {:?}", other.map(|_| "cmd")),
    }

    // `k` on the locked row unlocks (the toggle reads the row's truth).
    h.type_text("k");
    h.turn();
    assert!(
        matches!(
            h.find_cmd(|c| matches!(c, Cmd::UnlockModel { .. })),
            Some(Cmd::UnlockModel { provider, model })
                if provider == "lmstudio" && model == "qwen3-4b"
        ),
        "locked row + k = unlock"
    );

    // `e` probes the context estimate for the selected row.
    h.type_text("e");
    h.turn();
    assert!(
        matches!(
            h.find_cmd(|c| matches!(c, Cmd::EstimateContext { .. })),
            Some(Cmd::EstimateContext { provider, context_length, .. })
                if provider == "lmstudio" && context_length == Some(32768)
        ),
        "e sends the estimate probe with the row's context length"
    );

    // `f` arms the FORCE confirm — labeled, still two-step — and `y`
    // sends the same command with force:true.
    h.type_text("f");
    let screen = h.turn();
    assert!(
        screen.contains("FORCED"),
        "force confirm is labeled as such:\n{screen}"
    );
    h.type_text("y");
    h.turn();
    match h.find_cmd(|c| matches!(c, Cmd::UnloadModel { .. })) {
        Some(Cmd::UnloadModel {
            provider, force, ..
        }) => {
            assert_eq!(provider, "lmstudio");
            assert!(force, "f + confirm sends force:true");
        }
        other => panic!(
            "expected forced UnloadModel, got {:?}",
            other.map(|_| "cmd")
        ),
    }

    // The footer carries the host-RAM segment from the same fetch —
    // present because the served percent is known, muted at 62%. RAM is
    // deliberately what it shows: the device meter's host figure lives
    // in the modal, where it can name its own scope.
    assert!(
        screen.contains("mem 62%"),
        "footer mem segment from the last fetch:\n{screen}"
    );

    // ↓ walks the panel one target at a time. This panel is TALLER than
    // the terminal, so each row is asserted where the cursor reaches it —
    // which is the point: every row is reachable, and the pinned meters
    // never scroll away while walking.
    let meters_still_pinned = |screen: &str| {
        assert!(
            screen.contains("Accelerator heap · metal (all processes)")
                && screen.contains("memory-mapped GGUF weights are not counted here"),
            "the meters and their note are PINNED — no cursor position \
             scrolls them away:\n{screen}"
        );
    };
    h.term.push_input(b"\x1b[B"); // ↓ onto the unknown-residency row
    let screen = h.turn();
    meters_still_pinned(&screen);
    assert!(
        screen.contains("lmstudio/qwen3-4b")
            && panel_row_text(&screen, "· lmstudio/qwen3-4b ·").contains("ctx 28672*"),
        "the first model row, with its starred calibrated ctx:\n{screen}"
    );
    assert!(
        screen.contains("🔒") && screen.contains("k unlocks"),
        "the locked row keeps its marker and names its verb:\n{screen}"
    );
    assert!(
        screen.contains("resident unknown") && screen.contains("no lock ("),
        "tri-state residency stays distinct and says why it cannot lock:\n{screen}"
    );
    h.term.push_input(b"\x1b[B\x1b[B"); // ↓↓ on to the cache row
    let screen = h.turn();
    meters_still_pinned(&screen);
    // The corrected fixture still exercises ADOPT: the wire's
    // `source: provider_server` is the ONE selector for that wording —
    // `lockable: true` rides on the row too, so it can never be it.
    assert!(
        screen.contains("k locks (adopts it)"),
        "the externally-loaded provider_server row is offered the lock:\n{screen}"
    );
    // The row itself wraps; rejoined, it carries the marked estimate and the
    // cache figure beside it (`cache 2.0 GiB` straddles the fold at this
    // width — the unit string is what the fold moved on, not the content).
    let swept = panel_row_text(&screen, "lmstudio/glm-4.6-gguf");
    assert!(
        swept.contains("~89.0 GiB"),
        "estimate MARKED with ~: {swept}"
    );
    assert!(swept.contains("cache 2.0 GiB"), "{swept}");
    // The tail is reachable in the same breath: the cursor is ON the
    // session-cache row, which is a cursor target with no action.
    assert!(screen.contains("acode-abc"), "cache row:\n{screen}");

    // ↑ back onto the sweep row: `k` there sends the LOCK. The gateway
    // ADOPTS a model it did not load; this client never invents a
    // refusal for one.
    h.term.push_input(b"\x1b[A");
    h.turn();
    h.type_text("k");
    h.turn();
    assert!(
        matches!(
            h.find_cmd(|c| matches!(c, Cmd::LockModel { .. })),
            Some(Cmd::LockModel { provider, model })
                if provider == "lmstudio" && model == "glm-4.6-gguf"
        ),
        "a sweep-resident row locks (adoption)"
    );
}

/// The HEAD must be REACHABLE too, at the DEFAULT terminal size and with
/// the cursor where it opens. The window follows the cursor, so rows
/// above the first cursor target can never be scrolled back to: once
/// `/resources` grew the accelerator meter, its GGUF note and the
/// itemized breakdown, the preamble outgrew the window and stranded the
/// meters — the very thing the panel is opened for — behind an `↑ N more`
/// no key could move. The meters are now a PINNED head (they render at
/// every cursor position) and the body opens at its own top (so the
/// itemization is not stranded one level down).
///
/// This is the regression test for that fix: 100x30, no scrolling, no
/// resizing — just open it and look.
#[test]
fn resources_head_is_visible_at_the_default_size_and_never_scrolls_away() {
    use abstractcode::store::{HostContracts, HostState};

    let mut h = harness();
    h.leave_splash();
    h.store.host_contracts.set(Some(HostContracts {
        model_residency: true,
        host_state: true,
        session_caches: true,
        modality_labels: Vec::new(),
    }));
    // Enough rows that the panel is comfortably taller than the window.
    let caches: Vec<serde_json::Value> = (0..20)
        .map(|i| {
            serde_json::json!({
                "key": format!("k{i}"), "provider": "lmstudio", "model": "qwen3-4b",
                "session_id": format!("sess-{i:02}"), "bytes": 1024, "token_count": 10
            })
        })
        .collect();
    let facts = abstractcode::discovery::host_state_from_response(&serde_json::json!({
        "memory": {
            "ram": {"total_bytes": 137438953472u64, "used_bytes": 33741111296u64,
                     "percent": 29.9},
            "process": {"rss_bytes": 76762775552u64},
            "device": {"backend": "metal", "allocated_bytes": 0u64,
                        "total_bytes": 137438953472u64,
                        "host_in_use_bytes": 1042120704u64,
                        "wired_limit_bytes": 115343360000u64}
        },
        "models": [
            {"runtime_id": "rt-a", "provider": "huggingface", "model": "big-gguf",
             "source": "provider_server", "resident": true, "lockable": true,
             "est_weights_bytes": 89986353824u64, "cache_bytes": 2147483648u64}
        ],
        "session_caches": caches,
        "totals": {"session_cache_bytes": 4352519172u64}
    }));
    h.store.host_state.set(HostState::Ready(facts));

    h.type_text("/resources");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    // THE PIN: the accelerator label and its note, at the cursor's
    // initial position, with nothing touched.
    assert!(
        screen.contains("Accelerator heap · metal (all processes)"),
        "the accelerator label renders at rest:\n{screen}"
    );
    assert!(
        screen.contains("memory-mapped GGUF weights are not counted here"),
        "and so does its note:\n{screen}"
    );
    // The panel really is taller than the window — otherwise this test
    // would pass for the wrong reason.
    assert!(
        screen.contains("↓ ") && screen.contains(" more"),
        "the fixture must overflow the window, or this proves nothing:\n{screen}"
    );
    // The itemization is not stranded one level down either.
    assert!(screen.contains("consuming memory"), "{screen}");
    assert!(screen.contains("gateway process RSS"), "{screen}");

    // …and the head survives every cursor position, including the last.
    for _ in 0..40 {
        h.term.push_input(b"\x1b[B");
    }
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("sess-19"),
        "the tail is still reachable:\n{screen}"
    );
    assert!(
        screen.contains("Accelerator heap · metal (all processes)")
            && screen.contains("memory-mapped GGUF weights are not counted here"),
        "the pinned head does not scroll away at the tail:\n{screen}"
    );
}

/// The tail must be REACHABLE (review HIGH-1): with few models and many
/// cache rows the windowed body used to pin at the top — cache and
/// totals rows are cursor-reachable now, so ↓ walks the window to the
/// tail. Also pins the shrink clamp (review MEDIUM-6): after the row set
/// shrinks under a deep cursor, the action target is the row the
/// highlight shows — never an out-of-range ghost.
#[test]
fn resources_tail_is_reachable_and_shrink_keeps_cursor_and_action_aligned() {
    use abstractcode::store::{HostContracts, HostState};

    let mut h = harness();
    h.leave_splash();
    h.store.host_contracts.set(Some(HostContracts {
        model_residency: true,
        host_state: true,
        session_caches: true,
        modality_labels: Vec::new(),
    }));
    let caches: Vec<serde_json::Value> = (0..30)
        .map(|i| {
            serde_json::json!({
                "key": format!("k{i}"), "provider": "lmstudio", "model": "qwen3-4b",
                "session_id": format!("sess-{i:02}"), "bytes": 1024, "token_count": 10
            })
        })
        .collect();
    let facts = abstractcode::discovery::host_state_from_response(&serde_json::json!({
        "models": [
            {"provider": "lmstudio", "model": "qwen3-4b", "resident": true},
            {"provider": "ollama", "model": "phi4", "resident": true}
        ],
        "session_caches": caches,
        "totals": {"resident_models": 2}
    }));
    h.store.host_state.set(HostState::Ready(facts));

    h.type_text("/resources");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(screen.contains("host resources"), "modal opens:\n{screen}");
    assert!(
        !screen.contains("sess-29"),
        "the tail overflows the window before scrolling:\n{screen}"
    );
    // ↓ through models + caches: the window follows the cursor to the tail.
    for _ in 0..40 {
        h.term.push_input(b"\x1b[B");
    }
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("sess-29"),
        "the last cache row is reachable by keyboard:\n{screen}"
    );

    // SHRINK under a deep cursor: one model remains; the clamped cursor
    // and the action target agree — u confirms the row the highlight is on.
    let solo = abstractcode::discovery::host_state_from_response(&serde_json::json!({
        "models": [{"provider": "solo", "model": "model-a", "resident": true}]
    }));
    h.store.host_state.set(HostState::Ready(solo));
    h.turn();
    h.type_text("u");
    let screen = h.turn();
    assert!(
        screen.contains("unload solo/model-a?"),
        "after the shrink the action targets the clamped row:\n{screen}"
    );
}

/// The contracts GATE: an old gateway (contracts answered, host_state
/// absent) gets an honest "not supported" modal and NO host-state fetch;
/// still-probing contracts get the probing state + a capabilities retry.
#[test]
fn resources_modal_is_honest_when_the_contract_is_absent_or_unprobed() {
    use abstractcode::store::HostContracts;

    // Known-absent: the gateway ANSWERED and declares no host_state.
    let mut h = harness();
    h.leave_splash();
    h.store.host_contracts.set(Some(HostContracts::default()));
    h.type_text("/resources");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("not supported by this gateway"),
        "known-absent contract says so:\n{screen}"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadHostState)).is_none(),
        "no fetch against a gateway that declared no contract"
    );

    // Still probing (None): honest probing state + a capabilities retry.
    let mut h = harness();
    h.leave_splash();
    h.type_text("/resources");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("probing gateway capabilities"),
        "unprobed contracts render the probing state:\n{screen}"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadCapabilities)).is_some(),
        "the gesture retries the capabilities fetch"
    );
    assert!(
        h.find_cmd(|c| matches!(c, Cmd::LoadHostState)).is_none(),
        "never a host-state fetch before the contract is confirmed"
    );
}
