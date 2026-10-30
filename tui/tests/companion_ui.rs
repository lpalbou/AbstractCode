//! Host-level companion integration with ephemeral preferences and no worker/network.
use abstractcode::{
    config::Prefs,
    docs_chat::ChatScope,
    runner::Cmd,
    store::{Conn, Store, Workflow},
    transcript::{Item, ToolStatus},
    ui::{self, UiCtx},
};
use abstracttui::{app::Driver, prelude::*, testing::CaptureTerm};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc, sync::mpsc};

struct Harness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    ctx: UiCtx,
    rx: mpsc::Receiver<Cmd>,
}
fn harness(width: i32, companion: Option<Value>) -> Harness {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let size = Size::new(width, 40);
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let actions = app.actions();
    let (tx, rx) = mpsc::channel();
    let slot = Rc::new(RefCell::new(None));
    let out = slot.clone();
    app.mount(move |cx| {
        let store = Store::create(cx);
        store.session_id.set("companion-fixture".into());
        store.conn.set(Conn::Ok);
        store.fold.update(|f| {
            f.begin_run("main-root");
            f.push_item(Item::User {
                text: "Existing user request".into(),
            });
            f.push_item(Item::Assistant {
                text: "Existing assistant reply".into(),
                final_answer: true,
            });
        });
        store.workflow.set(Workflow {
            bundle_id: "basic-agent".into(),
            flow_id: "basic".into(),
            name: "Basic agent".into(),
            ..Default::default()
        });
        let prefs = Prefs {
            companion,
            ..Default::default()
        };
        let ctx = UiCtx {
            tx,
            client: abstractcode::gateway::GatewayClient::new("http://127.0.0.1:1", None),
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            prefs: Rc::new(RefCell::new(prefs)),
            workspace_root: None,
            max_iterations_explicit: false,
            max_iterations: 50,
            no_project_context: true,
            no_prompt_cache: false,
            replay_turns: 20,
            gateway_label: "fixture".into(),
            modal: Rc::new(RefCell::new(None)),
            modal_epoch: cx.signal(0),
            dismissed_wait: Rc::new(RefCell::new(None)),
            wait_modal_for: Rc::new(RefCell::new(None)),
        };
        *out.borrow_mut() = Some((store, ctx.clone(), cx));
        ui::root(cx, store, ctx, &actions)
    })
    .unwrap();
    let mut term = CaptureTerm::new(size);
    let driver = Driver::new(
        &mut app,
        &mut term,
        RunConfig {
            probe: false,
            caps: Some(abstracttui::term::Capabilities::with(|c| {
                c.truecolor = true;
                c.colors_256 = true;
                c.unicode_ok = true;
            })),
            ..Default::default()
        },
    )
    .unwrap();
    let (store, ctx, _scope) = slot.borrow().clone().unwrap();
    Harness {
        app,
        term,
        driver,
        store,
        ctx,
        rx,
    }
}
impl Harness {
    fn turn(&mut self) -> String {
        self.driver.turn(&mut self.app, &mut self.term).unwrap();
        self.term.screen().to_text()
    }
    fn click(&mut self, label: &str) {
        let screen = self.turn();
        let (y, line) = screen
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(label))
            .unwrap_or_else(|| panic!("{label} missing:\n{screen}"));
        let byte = line.find(label).unwrap();
        let x = line[..byte].chars().count();
        self.term
            .push_input(format!("\x1b[<0;{};{}M", x + 1, y + 1).as_bytes());
        self.turn();
        self.term
            .push_input(format!("\x1b[<0;{};{}m", x + 1, y + 1).as_bytes());
        self.turn();
    }
    fn enter(&mut self) {
        self.term.push_input(b"\r");
        self.turn();
    }
    fn commands(&self) -> Vec<Cmd> {
        self.rx.try_iter().collect()
    }
}
fn advice_count(commands: &[Cmd]) -> usize {
    commands
        .iter()
        .filter(|c| {
            matches!(
                c,
                Cmd::AssistantAsk {
                    scope: ChatScope::Advice,
                    ..
                }
            )
        })
        .count()
}
fn tool() -> Item {
    Item::Tool {
        key: "tool-row".into(),
        name: "read_file".into(),
        args_preview: "file".into(),
        args_full: "never-in-advice".into(),
        status: ToolStatus::Ok,
        result: "large-payload-excluded".into(),
        error: String::new(),
        sandbox: None,
    }
}

#[test]
fn both_styles_offer_a_ready_companion_and_open_the_run_without_sending_the_coding_draft() {
    for style in ["halo", "hover"] {
        let mut h = harness(
            220,
            Some(json!({"enabled":true,"style":style,"animate":false})),
        );
        let screen = h.turn();
        assert!(
            screen.contains("Ready") && screen.contains("Talk / ask"),
            "{style}:\n{screen}"
        );
        h.term.push_input(b"My coding draft");
        h.turn();
        h.click(if style == "halo" {
            "Ready"
        } else {
            "Talk / ask"
        });
        let screen = h.turn();
        assert!(
            screen.contains("● This run") && screen.contains("My coding draft"),
            "{screen}"
        );
        assert!(
            !h.commands().iter().any(|c| matches!(
                c,
                Cmd::Start { .. }
                    | Cmd::Steer { .. }
                    | Cmd::Resume { .. }
                    | Cmd::AssistantAsk { .. }
            )),
            "opening is presentation only"
        );
    }
}
#[test]
fn narrow_companion_remains_reachable_and_yields_to_the_assistant_drawer() {
    let mut h = harness(90, Some(json!({"enabled":true,"animate":false})));
    let screen = h.turn();
    assert!(
        screen.contains('◉') && !screen.contains("Talk / ask"),
        "{screen}"
    );
    h.click("◉");
    let screen = h.turn();
    assert!(
        screen.contains("Assistant · AbstractCode") && screen.contains("● This run"),
        "{screen}"
    );
    assert!(
        !screen.contains('◉'),
        "duplicate compact avatar should be hidden in drawer:\n{screen}"
    );
}
#[test]
fn companion_settings_are_reachable_and_change_only_ephemeral_preferences() {
    let mut h = harness(220, Some(json!({"enabled":true,"animate":false})));
    h.turn();
    h.click("⚙");
    let screen = h.turn();
    for label in [
        "Enable companion",
        "Appearance",
        "Animate activity",
        "Offer comments",
        "Read side replies",
        "Speaking voice",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    assert!(h.ctx.modal_open());
    assert!(!screen.contains("Talk / ask"), "modal owns interaction");
    h.click("Appearance");
    h.enter();
    assert_eq!(
        h.ctx.prefs.borrow().companion.as_ref().unwrap()["style"],
        "hover"
    );
    h.click("Offer comments");
    h.enter();
    assert_eq!(
        h.ctx.prefs.borrow().companion.as_ref().unwrap()["advice"],
        true
    );
    assert!(h.ctx.prefs.borrow().path.is_none());
    assert_eq!(
        advice_count(&h.commands()),
        0,
        "editing settings is not an inference trigger"
    );
}
#[test]
fn default_has_no_companion_or_background_inference_and_entity_focus_hides_it() {
    let mut h = harness(220, None);
    let screen = h.turn();
    assert!(!screen.contains("Talk / ask"));
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "A changed request".into(),
        })
    });
    h.turn();
    assert_eq!(advice_count(&h.commands()), 0);
    let mut h = harness(220, Some(json!({"enabled":true,"animate":false})));
    h.turn();
    h.store
        .focus
        .set(abstractcode::convo::Focus::Entity("named-entity".into()));
    let screen = h.turn();
    assert!(
        !screen.contains("Talk / ask"),
        "companion must not impersonate entity identity:\n{screen}"
    );
}
#[test]
fn advice_is_change_driven_not_replayed_and_tool_payloads_never_trigger_an_extra_request() {
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"advice":true,"animate":false})),
    );
    h.turn();
    assert_eq!(
        advice_count(&h.commands()),
        0,
        "initial loaded conversation is a baseline"
    );
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "A new visible question".into(),
        })
    });
    h.turn();
    let commands = h.commands();
    assert_eq!(advice_count(&commands), 1);
    let context = commands
        .iter()
        .find_map(|c| match c {
            Cmd::AssistantAsk {
                scope: ChatScope::Advice,
                context,
                ..
            } => Some(context),
            _ => None,
        })
        .unwrap();
    assert!(context.contains("A new visible question"));
    h.store.fold.update(|f| f.push_item(tool()));
    h.turn();
    assert_eq!(advice_count(&h.commands()), 0);
    h.store.advice_chat.update(|s| {
        s.busy = false;
        s.messages.push((
            "assistant".into(),
            "Check the source date before relying on this result.".into(),
        ));
    });
    h.turn();
    assert!(
        h.store.companion_advice.get_untracked().is_some(),
        "tool-only events do not obsolete the visible-message snapshot"
    );
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "Another new question during cooldown".into(),
        })
    });
    h.turn();
    assert_eq!(
        advice_count(&h.commands()),
        0,
        "second request is deferred by cooldown"
    );
    h.store.restoring.set(true);
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "An earlier replayed question".into(),
        })
    });
    h.turn();
    h.store.restoring.set(false);
    h.turn();
    assert_eq!(
        advice_count(&h.commands()),
        0,
        "history replay is not new activity"
    );
}

#[test]
fn a_genuine_approval_hides_the_companion_and_an_outside_click_never_resumes_it() {
    use abstractcode::transcript::{PendingWait, WaitKind};
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"advice":true,"animate":false})),
    );
    h.turn();
    h.store.fold.update(|f| {
        f.pending_wait = Some(PendingWait {
            run_id: "main-root".into(),
            wait_key: "tool-approval".into(),
            step_id: "step-1".into(),
            kind: WaitKind::Approval {
                tool_calls: vec![
                    json!({"name":"write_file","arguments":{"path":"file.txt","content":"data"}}),
                ],
            },
        })
    });
    let screen = h.turn();
    assert!(h.ctx.modal_open(), "approval should be visible:\n{screen}");
    assert!(
        !screen.contains("Talk / ask"),
        "companion must yield to approval:\n{screen}"
    );
    h.term.push_input(b"\x1b[<0;1;1M\x1b[<0;1;1m");
    h.turn();
    assert!(h.ctx.modal_open() && h.store.fold.with_untracked(|f| f.pending_wait.is_some()));
    assert!(
        !h.commands().iter().any(|c| matches!(c, Cmd::Resume { .. })),
        "outside interaction cannot approve"
    );
}
#[test]
fn a_late_automatic_comment_cannot_cross_a_main_session_switch() {
    let mut h = harness(
        220,
        Some(json!({"enabled":true,"advice":true,"animate":false})),
    );
    h.turn();
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "New work in original session".into(),
        })
    });
    h.turn();
    assert_eq!(advice_count(&h.commands()), 1);
    h.store.session_id.set("another-session".into());
    h.store.fold.update(|f| {
        f.begin_run("another-root");
        f.push_item(Item::User {
            text: "A different conversation".into(),
        });
    });
    h.turn();
    h.store.advice_chat.update(|s| {
        s.busy = false;
        s.messages.push((
            "assistant".into(),
            "Stale original-session suggestion".into(),
        ));
    });
    h.turn();
    assert!(h.store.companion_advice.get_untracked().is_none());
    assert_eq!(
        advice_count(&h.commands()),
        0,
        "switching to existing history does not trigger new inference"
    );
}

#[test]
fn a_ready_companion_emits_no_terminal_output_on_idle_turns() {
    let mut h = harness(220, Some(json!({"enabled":true,"animate":true})));
    h.turn();
    h.turn();
    h.term.take_bytes();
    for _ in 0..4 {
        h.term.push_idle();
        h.turn();
    }
    assert!(
        h.term.bytes().is_empty(),
        "ready state must not repaint in an idle loop"
    );
    assert_eq!(advice_count(&h.commands()), 0);
}
