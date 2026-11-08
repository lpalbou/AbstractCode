//! Real terminal input, Code composer and command-channel authority checks.
use abstractcode::{
    config::Prefs,
    runner::Cmd,
    store::{Conn, Store, Workflow},
    transcript::Item,
    ui::{self, UiCtx},
};
use abstracttui::{app::Driver, prelude::*, testing::CaptureTerm};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc, sync::mpsc};

struct Harness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
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
                author: None,
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
            platform_clipboard: false,
            caps: Some(abstracttui::term::Capabilities::with(|c| {
                c.truecolor = true;
                c.colors_256 = true;
                c.unicode_ok = true;
            })),
            ..Default::default()
        },
    )
    .unwrap();
    let (store, _ctx, _scope) = slot.borrow().clone().unwrap();
    Harness {
        app,
        term,
        driver,
        store,
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

fn sends(commands: &[Cmd]) -> usize {
    commands
        .iter()
        .filter(|c| {
            matches!(
                c,
                Cmd::Start { .. } | Cmd::Steer { .. } | Cmd::AssistantAsk { .. }
            )
        })
        .count()
}
#[test]
fn selected_explain_preserves_draft_and_requires_normal_explicit_submit() {
    let mut h = harness(100, None);
    h.turn();
    h.term.push_input(b"unfinished coding draft");
    h.turn();
    h.click("Existing assistant reply");
    let selected = h.turn();
    assert!(selected.contains("Selected word"), "{selected}");
    h.click("Paragraph");
    let bar = h
        .turn()
        .lines()
        .position(|line| line.contains("Use as context") && line.contains("Cancel"))
        .expect("selection action row");
    h.click("Explain");
    let screen = h.turn();
    assert!(
        !screen
            .lines()
            .nth(bar)
            .unwrap_or_default()
            .contains("Selected paragraph"),
        "{screen}"
    );
    assert_eq!(
        sends(&h.commands()),
        0,
        "selection actions have no send authority"
    );
    h.enter();
    let commands = h.commands();
    let prompt = commands
        .iter()
        .find_map(|c| {
            if let Cmd::Start { prompt, .. } = c {
                Some(prompt)
            } else {
                None
            }
        })
        .expect("normal Enter submits reviewed draft");
    assert!(
        prompt.starts_with("unfinished coding draft\n\nExplain this selected content."),
        "{prompt}"
    );
    assert!(prompt.contains("> Existing assistant reply"));
    assert!(!prompt.contains("companion-fixture") && !prompt.contains("source bytes"));
}
#[test]
fn cancel_selection_preserves_an_unfinished_draft() {
    let mut h = harness(100, None);
    h.turn();
    h.term.push_input(b"keep this draft");
    h.turn();
    h.click("Existing assistant reply");
    h.click("Cancel");
    assert_eq!(sends(&h.commands()), 0);
    h.enter();
    assert!(h
        .commands()
        .iter()
        .any(|c| matches!(c, Cmd::Start {prompt, ..} if prompt == "keep this draft")));
}
#[test]
fn selected_table_value_sends_header_relation_only_after_enter() {
    let mut h = harness(100, None);
    h.store.fold.update(|f| {
        f.push_item(Item::Assistant {
            text: "| Package | Status |\n|---|---|\n| AbstractAvatar | Pending |".into(),
            final_answer: true,
            author: None,
        })
    });
    h.turn();
    h.click("Pending");
    let screen = h.turn();
    assert!(screen.contains("Selected table cell"), "{screen}");
    h.click("Use as context");
    assert_eq!(sends(&h.commands()), 0);
    h.enter();
    let commands = h.commands();
    let prompt = commands
        .iter()
        .find_map(|c| {
            if let Cmd::Start { prompt, .. } = c {
                Some(prompt)
            } else {
                None
            }
        })
        .unwrap();
    assert!(
        prompt.contains("table body row 1, column 2 (header \"Status\")"),
        "{prompt}"
    );
    assert!(prompt.contains("> Pending"));
}

#[test]
fn bottom_edge_selection_and_later_messages_do_not_move_the_clicked_text() {
    let mut h = harness(100, None);
    let body = (0..24)
        .map(|i| format!("Passage {i} has some words.\n\n"))
        .collect::<String>()
        + "edgeword stays here";
    h.store.fold.update(|f| {
        f.push_item(Item::Assistant {
            text: body,
            final_answer: true,
            author: None,
        })
    });
    h.turn();
    h.turn();
    let before = h.turn();
    let y = before
        .lines()
        .position(|line| line.contains("edgeword"))
        .expect("tail visible");
    let old_line = before.lines().nth(y).unwrap().to_owned();
    h.click("edgeword");
    let after = h.turn();
    assert_eq!(
        after.lines().nth(y).unwrap(),
        old_line,
        "actions reuse spacer; no source row reflow on first click"
    );
    assert!(after.contains("Selected word"));
    h.store.fold.update(|f| {
        f.push_item(Item::Assistant {
            text: "a later assistant message\n\nwith more output".into(),
            final_answer: true,
            author: None,
        })
    });
    h.turn();
    h.turn();
    assert_eq!(
        h.turn()
            .lines()
            .nth(y)
            .unwrap()
            .chars()
            .take(80)
            .collect::<String>(),
        old_line.chars().take(80).collect::<String>(),
        "stream appends cannot move a retained selection"
    );
    assert_eq!(sends(&h.commands()), 0);
}
#[test]
fn narrow_action_menu_returns_to_composer_and_never_sends_on_menu_choice() {
    let mut h = harness(38, None);
    h.turn();
    h.term.push_input(b"unfinished draft");
    h.turn();
    h.click("Existing assistant reply");
    h.click("Actions");
    h.click("Use as context");
    assert_eq!(sends(&h.commands()), 0);
    h.enter();
    let commands = h.commands();
    let prompt = commands
        .iter()
        .find_map(|c| {
            if let Cmd::Start { prompt, .. } = c {
                Some(prompt)
            } else {
                None
            }
        })
        .expect("menu returns focus to normal composer");
    assert!(
        prompt.starts_with("unfinished draft\n\nUse this selected content as context"),
        "{prompt}"
    );
}
#[test]
fn keyboard_action_navigation_stages_then_normal_enter_sends() {
    let mut h = harness(100, None);
    h.turn();
    h.click("Existing assistant reply");
    h.term.push_input(b"\t");
    h.turn(); // next live tab stop is the selection action host
    let focused = h.app.tree().accessibility_tree();
    assert!(
        focused
            .focused()
            .is_some_and(|entry| entry.label == "Selected content actions"),
        "{}",
        focused.to_text()
    );
    h.term.push_input(b"\x1b[C ");
    h.turn(); // Right chooses Use as context; Space activates
    assert_eq!(
        sends(&h.commands()),
        0,
        "Space on a focused action must not send or dictate"
    );
    h.enter();
    let commands = h.commands();
    let prompt = commands
        .iter()
        .find_map(|c| {
            if let Cmd::Start { prompt, .. } = c {
                Some(prompt)
            } else {
                None
            }
        })
        .expect("normal Enter after staging");
    assert!(
        prompt.starts_with("Use this selected content as context"),
        "{prompt}"
    );
}
#[test]
fn slash_draft_refusal_and_escape_keep_the_original_command() {
    let mut h = harness(100, None);
    h.turn();
    h.term.push_input(b"/new");
    h.turn();
    h.click("Existing assistant reply");
    h.click("Explain");
    assert_eq!(sends(&h.commands()), 0);
    assert!(
        h.store
            .notices
            .get_untracked()
            .iter()
            .any(|notice| notice.contains("slash command")),
        "refusal posts user feedback"
    );
    h.term.push_input(b"\x1b[27u");
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("/new"),
        "Escape cancels selection before draft-clearing:\n{screen}"
    );
    assert!(!screen.contains("Selected word"));
}

#[test]
fn escape_cancels_voice_and_selection_without_clearing_the_coding_draft() {
    let mut h = harness(100, None);
    h.turn();
    h.term.push_input(b"keep my draft");
    h.turn();
    h.click("Existing assistant reply");
    h.store
        .voice
        .dictation
        .set(abstractcode::voice::Dictation::Transcribing {
            since: std::time::Instant::now(),
            route: "fixture".into(),
        });
    h.turn();
    h.term.push_input(b"\x1b[27u");
    h.turn();
    assert!(matches!(
        h.store.voice.dictation.get_untracked(),
        abstractcode::voice::Dictation::Idle
    ));
    assert!(!h.turn().contains("Selected word"));
    assert_eq!(sends(&h.commands()), 0);
    h.enter();
    assert!(h
        .commands()
        .iter()
        .any(|c| matches!(c, Cmd::Start {prompt, ..} if prompt == "keep my draft")));
}
