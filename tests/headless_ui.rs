//! Headless UI tests: the REAL interface driven through AbstractTUI's
//! capture harness — same pipeline as production, no pty.
//!
//! The worker thread is replaced by a dummy command channel; ledger records
//! come from the live-captured fixture, applied to the store between frames
//! exactly as posted closures would apply them.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;
use serde_json::Value;

use abstractcode_tui::config::Prefs;
use abstractcode_tui::runner::Cmd;
use abstractcode_tui::store::{Phase, Store, Workflow};
use abstractcode_tui::ui::{self, UiCtx};

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
    // A fresh default theme per test (tests share a process).
    abstracttui::app::set_theme_by_id("abstract-dark");
    let size = Size::new(100, 30);
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
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            prefs: prefs_for_ctx.clone(),
            workspace_root: Some("/tmp/ws".into()),
            workspace_mode: None,
            max_iterations: 50,
            replay_turns: 20,
            gateway_label: "127.0.0.1:8080".into(),
            modal: Rc::new(RefCell::new(None)),
            modal_epoch: cx.signal(0u64),
            dismissed_wait: Rc::new(RefCell::new(None)),
            wait_modal_for: Rc::new(RefCell::new(None)),
        };
        *ctx_out.borrow_mut() = Some((ctx.clone(), cx));
        ui::root(cx, store, ctx)
    })
    .expect("mount");
    let mut term = CaptureTerm::new(size);
    let cfg = RunConfig {
        probe: false,
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

#[test]
fn typing_a_prompt_sends_start_and_renders_user_card() {
    let mut h = harness();
    h.turn();
    h.type_text("write a haiku");
    h.turn();
    h.press_enter();
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
    h.turn(); // deferred focus_composer lands on the rebuilt input
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
        abstractcode_tui::store::ProviderInfo {
            name: "lmstudio".into(),
            models: vec!["qwen-a".into(), "qwen-b".into()],
        },
        abstractcode_tui::store::ProviderInfo {
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
        abstractcode_tui::store::ProviderInfo {
            name: "lmstudio".into(),
            models: vec!["qwen-a".into(), "qwen-b".into()],
        },
        abstractcode_tui::store::ProviderInfo {
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
        abstractcode_tui::store::ProviderInfo {
            name: "lmstudio".into(),
            models: vec!["qwen-a".into(), "qwen-b".into()],
        },
        abstractcode_tui::store::ProviderInfo {
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
        abstractcode_tui::store::ProviderInfo {
            name: "lmstudio".into(),
            models: vec!["qwen-a".into()],
        },
        abstractcode_tui::store::ProviderInfo {
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
fn details_toggle_hides_thinking_and_start_carries_context() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    // A finished first turn in the fold.
    store.fold.update(|f| {
        f.push_item(abstractcode_tui::transcript::Item::User {
            text: "first question".into(),
        });
        f.push_item(abstractcode_tui::transcript::Item::Thinking {
            iteration: 1,
            content: "let me think about xyzzy".into(),
            reasoning: String::new(),
        });
        f.push_item(abstractcode_tui::transcript::Item::Assistant {
            text: "first answer".into(),
            final_answer: true,
        });
    });
    // Two pumps: the feed's first mount discovers its width during draw
    // and syncs the measured extent one frame later (engine contract —
    // FeedState rows are "0 until the first draw discovers a width").
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("xyzzy"),
        "details shown by default:\n{screen}"
    );

    // Ctrl+D hides the thinking block; answers stay.
    h.term.push_input(&[0x04]); // Ctrl+D
    let screen = h.turn();
    assert!(!screen.contains("xyzzy"), "thinking hidden:\n{screen}");
    assert!(screen.contains("first answer"), "answers stay:\n{screen}");

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
        abstractcode_tui::store::ToolInfo {
            name: "read_file".into(),
            description: "Read a file".into(),
            toolset: "files".into(),
        },
        abstractcode_tui::store::ToolInfo {
            name: "web_search".into(),
            description: "Search the web".into(),
            toolset: "web".into(),
        },
        abstractcode_tui::store::ToolInfo {
            name: "write_file".into(),
            description: "Write a file".into(),
            toolset: "files".into(),
        },
    ]);
    h.type_text("/tools");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        screen.contains("gateway tools — 3 available (untouched"),
        "tools modal title:\n{screen}"
    );
    assert!(screen.contains("[✓] read_file"), "checked rows:\n{screen}");

    // Space toggles the first tool OFF; title flips to explicit-allowlist.
    h.type_text(" ");
    let screen = h.turn();
    assert!(
        screen.contains("2 on / 1 off (explicit allowlist"),
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

#[test]
fn approve_all_auto_resumes_later_batches_until_toggled_off() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.phase.set(abstractcode_tui::store::Phase::Running);
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

    // First batch prompts; 'A' approves it AND arms auto-approve.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record("s1", "tool_approval:k1", "write_file"),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("approve all (A)"),
        "approve-all affordance visible:\n{screen}"
    );
    h.type_text("A");
    h.turn();
    h.turn(); // deferred modal close lands
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume { approved, .. }) => assert_eq!(approved, Some(true)),
        other => panic!("expected Resume, got {:?}", other.map(|_| "cmd")),
    }
    assert!(store.auto_approve.get_untracked(), "auto-approve armed");

    // Second batch: NO modal, auto-resumed.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record("s2", "tool_approval:k2", "execute_command"),
        );
    });
    let screen = h.turn();
    assert!(
        !screen.contains("approve (a)"),
        "no prompt modal while auto-approve is on:\n{screen}"
    );
    match h.find_cmd(|c| matches!(c, Cmd::Resume { .. })) {
        Some(Cmd::Resume {
            wait_key, approved, ..
        }) => {
            assert_eq!(wait_key, "tool_approval:k2");
            assert_eq!(approved, Some(true));
        }
        other => panic!("expected auto Resume, got {:?}", other.map(|_| "cmd")),
    }

    // /auto turns it off; the next batch prompts again.
    h.type_text("/auto");
    h.turn();
    h.press_enter();
    h.turn();
    assert!(!store.auto_approve.get_untracked(), "auto-approve off");
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &approval_record("s3", "tool_approval:k3", "fetch_url"),
        );
    });
    let screen = h.turn();
    assert!(
        screen.contains("approve (a)"),
        "prompting resumes after /auto off:\n{screen}"
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
        tools.push(abstractcode_tui::store::ToolInfo {
            name: format!("tool_{i:02}"),
            description: "does things".into(),
            toolset: if i < 15 { "files".into() } else { "web".into() },
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

    store.phase.set(abstractcode_tui::store::Phase::Running);
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
    store.phase.set(abstractcode_tui::store::Phase::Running);
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
    h.store.tools.set(vec![abstractcode_tui::store::ToolInfo {
        name: "read_file".into(),
        description: "Read".into(),
        toolset: "files".into(),
    }]);
    let (cx, ctx) = (h.cx, h.ctx.clone());
    abstractcode_tui::ui::modals::open_tools(cx, store, &ctx);
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
    h.store.tools.set(vec![abstractcode_tui::store::ToolInfo {
        name: "read_file".into(),
        description: "Read".into(),
        toolset: "files".into(),
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
        screen.contains("1 available (untouched"),
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
    // The /details COMMAND path (not just Ctrl+D) must repaint at once:
    // thinking blocks vanish, tool RESULT previews collapse, tool headers
    // and answers stay.
    let mut h = harness();
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(abstractcode_tui::transcript::Item::User {
            text: "run the suite".into(),
        });
        f.push_item(abstractcode_tui::transcript::Item::Thinking {
            iteration: 1,
            content: "pondering the xyzzy strategy".into(),
            reasoning: String::new(),
        });
        f.push_item(abstractcode_tui::transcript::Item::Tool {
            key: "call:1".into(),
            name: "execute_command".into(),
            args_preview: "cargo test".into(),
            status: abstractcode_tui::transcript::ToolStatus::Ok,
            result_preview: "result-plugh-lines".into(),
            error: String::new(),
        });
        f.push_item(abstractcode_tui::transcript::Item::Tool {
            key: "call:2".into(),
            name: "broken_tool".into(),
            args_preview: String::new(),
            status: abstractcode_tui::transcript::ToolStatus::Failed,
            result_preview: String::new(),
            error: "exploded".into(),
        });
        f.push_item(abstractcode_tui::transcript::Item::Assistant {
            text: "all green".into(),
            final_answer: true,
        });
    });
    // Two pumps: first feed mount discovers width at draw; the measured
    // extent syncs on the following frame (engine geometry contract).
    h.turn();
    let screen = h.turn();
    assert!(screen.contains("xyzzy"), "thinking visible:\n{screen}");
    assert!(
        screen.contains("result-plugh-lines"),
        "tool result visible:\n{screen}"
    );

    h.type_text("/details");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(
        !screen.contains("xyzzy"),
        "thinking hidden immediately after /details:\n{screen}"
    );
    assert!(
        !screen.contains("result-plugh-lines"),
        "tool result preview collapsed:\n{screen}"
    );
    assert!(
        !screen.contains("execute_command"),
        "finished-OK tool cards fold entirely in the clean view:\n{screen}"
    );
    assert!(
        screen.contains("broken_tool") && screen.contains("exploded"),
        "failed tools stay visible in the clean view (honesty):\n{screen}"
    );
    assert!(screen.contains("all green"), "answer stays:\n{screen}");

    // Toggle back on: everything returns.
    h.type_text("/details");
    h.turn();
    h.press_enter();
    let screen = h.turn();
    assert!(screen.contains("xyzzy"), "details restored:\n{screen}");
}

#[test]
fn skills_selector_attaches_and_start_carries_skills() {
    let mut h = harness();
    h.turn();
    h.store.skills_catalog.set(vec![
        abstractcode_tui::store::SkillInfo {
            name: "coredoc".into(),
            description: "Documentation discipline".into(),
            trust: "adopted".into(),
            blocked: false,
        },
        abstractcode_tui::store::SkillInfo {
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
        screen.contains("/session [id]"),
        "help modal lists commands:\n{screen}"
    );
    h.press_escape();
    h.turn(); // deferred close lands
    let screen = h.turn();
    assert!(
        !screen.contains("/session [id]"),
        "help closed on Esc:\n{screen}"
    );
}

/// Feed order = fold order across a MID-LIST visibility flip (the sync
/// contract's rebuild seam): in the clean view a running tool card is
/// visible, later items render after it; when it completes OK it folds
/// away (mid-list flip -> rebuild), and toggling details back on must
/// re-insert it at its FOLD position, never at the feed tail.
#[test]
fn feed_order_survives_mid_list_visibility_flips() {
    let mut h = harness();
    h.turn();
    let store = h.store;
    store.show_details.set(false); // clean view
    store.fold.update(|f| f.begin_run("root"));
    store.fold.update(|f| {
        f.push_item(abstractcode_tui::transcript::Item::User {
            text: "AAA-question".into(),
        });
        // A running tool: visible even in the clean view.
        let _ = f.apply(
            "root",
            &serde_json::json!({"run_id": "root", "node_id": "act", "status": "started",
                "effect": {"type": "tool_calls",
                            "payload": {"tool_calls": [{"name": "zz_marker_tool", "call_id": "c1"}]}}}),
        );
    });
    h.turn();
    store.fold.update(|f| {
        f.push_item(abstractcode_tui::transcript::Item::Assistant {
            text: "BBB-update".into(),
            final_answer: false,
        });
    });
    h.turn();
    let screen = h.turn();
    let pos = |s: &str, needle: &str| s.find(needle).unwrap_or(usize::MAX);
    assert!(
        pos(&screen, "AAA-question") < pos(&screen, "zz_marker_tool")
            && pos(&screen, "zz_marker_tool") < pos(&screen, "BBB-update"),
        "initial order user < tool < update:\n{screen}"
    );

    // The tool completes OK: mid-list visibility flips false in the
    // clean view — the card must disappear, order of the rest intact.
    store.fold.update(|f| {
        let _ = f.apply(
            "root",
            &serde_json::json!({"run_id": "root", "node_id": "act", "status": "completed",
                "effect": {"type": "tool_calls",
                            "payload": {"tool_calls": [{"name": "zz_marker_tool", "call_id": "c1"}]}},
                "result": {"results": [{"call_id": "c1", "success": true, "output": "fine"}]}}),
        );
    });
    let screen = h.turn();
    assert!(
        !screen.contains("zz_marker_tool"),
        "finished-OK card folds in the clean view:\n{screen}"
    );
    assert!(
        pos(&screen, "AAA-question") < pos(&screen, "BBB-update"),
        "remaining order intact:\n{screen}"
    );

    // Details back on: the card must come back BETWEEN its neighbors
    // (feed order is push order — a tail-appended key would render it
    // after BBB-update).
    h.term.push_input(&[0x04]); // Ctrl+D
    let screen = h.turn();
    assert!(
        pos(&screen, "AAA-question") < pos(&screen, "zz_marker_tool")
            && pos(&screen, "zz_marker_tool") < pos(&screen, "BBB-update"),
        "restored order user < tool < update:\n{screen}"
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
    use abstractcode_tui::transcript::{Item, MAX_ITEMS, TRUNCATE_CHUNK};
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
    // SURVIVOR — never a dropped item under a stale key.
    for _ in 0..9 {
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
    for _ in 0..9 {
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
    h.store.fold.update(|f| {
        f.push_item(abstractcode_tui::transcript::Item::User {
            text: "FIRST-QUESTION".into(),
        });
        for i in 0..40 {
            f.push_item(abstractcode_tui::transcript::Item::Thinking {
                iteration: i + 1,
                content: format!("ponder step {i}"),
                reasoning: String::new(),
            });
        }
        f.push_item(abstractcode_tui::transcript::Item::Assistant {
            text: "THE-FINAL-ANSWER".into(),
            final_answer: true,
        });
    });
    // Width discovery + measured-extent sync (engine geometry contract).
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
    // Ctrl+D: the clean view folds all 40 thinking cards — the feed
    // shrinks far below the stranded offset.
    h.term.push_input(&[0x04]);
    h.turn();
    h.turn();
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("THE-FINAL-ANSWER") || screen.contains("FIRST-QUESTION"),
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
            f.push_item(abstractcode_tui::transcript::Item::User {
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
    h.term.push_input(b"\x1b[B"); // Down -> the other session
    h.turn();
    h.press_enter();
    h.turn();
    h.turn(); // deferred modal close
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
        f.push_item(abstractcode_tui::transcript::Item::User {
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
        screen.contains("/session [id]"),
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
