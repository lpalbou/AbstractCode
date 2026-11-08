//! Separate feature entries over the same presentation and grounded-QA backend.
use abstractcode::{
    config::Prefs,
    docs_chat::ChatScope,
    runner::Cmd,
    store::Store,
    transcript::Item,
    ui::{self, UiCtx},
};
use abstracttui::{app::Driver, prelude::*, testing::CaptureTerm};
use std::{cell::RefCell, rc::Rc, sync::mpsc};

struct Harness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    rx: mpsc::Receiver<Cmd>,
}
fn harness() -> Harness {
    let size = Size::new(150, 32);
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, rx) = mpsc::channel();
    let slot = Rc::new(RefCell::new(None));
    let out = slot.clone();
    app.mount(move |cx| {
        let store = Store::create(cx);
        store.session_id.set("entry-session".into());
        store.fold.update(|f| {
            f.push_item(Item::User {
                text: "Main conversation request".into(),
            })
        });
        for (scope, answer, draft) in [
            (
                ChatScope::Docs,
                "Documentation history",
                "Documentation draft",
            ),
            (ChatScope::Run, "Companion history", "Companion draft"),
        ] {
            scope.chat(store).update(|chat| {
                chat.source_session = "entry-session".into();
                chat.messages.push(("assistant".into(), answer.into()));
            });
            scope.draft(store).set(draft.into());
        }
        let ctx = UiCtx {
            tx,
            client: abstractcode::gateway::GatewayClient::new("http://127.0.0.1:1", None),
            overlays,
            quitter,
            prefs: Rc::new(RefCell::new(Prefs::default())),
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
        ui::docs_assistant::install(cx, store, &ctx);
        *out.borrow_mut() = Some(store);
        Element::new().build()
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
                c.unicode_ok = true;
            })),
            ..Default::default()
        },
    )
    .unwrap();
    let store = slot.borrow().unwrap();
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
    fn open(&mut self, scope: ChatScope) -> String {
        self.store.help_scope.set(scope);
        self.store.docs_open.set(true);
        self.turn()
    }
    fn escape(&mut self) {
        self.term.push_input(b"\x1b");
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while self.store.docs_open.get_untracked() && std::time::Instant::now() < deadline {
            self.turn();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

#[test]
fn fixed_titles_and_separate_histories_survive_switching_while_open_and_escape() {
    let mut h = harness();
    h.turn();
    let docs = h.open(ChatScope::Docs);
    assert!(
        docs.contains("Documentation · AbstractCode")
            && docs.contains("Documentation history")
            && docs.contains("Documentation draft"),
        "{docs}"
    );
    assert!(
        !docs.contains("Companion · AbstractCode") && !docs.contains("This run"),
        "no scope tabs: {docs}"
    );
    let companion = h.open(ChatScope::Run);
    assert!(
        h.store.docs_open.get_untracked(),
        "closing the previous drawer must not close its replacement"
    );
    assert!(
        companion.contains("Companion · AbstractCode")
            && companion.contains("Your current conversation")
            && companion.contains("Companion history")
            && companion.contains("Companion draft"),
        "{companion}"
    );
    assert!(
        !companion.contains("Documentation history") && !companion.contains("This run"),
        "{companion}"
    );
    h.escape();
    assert!(
        !h.store.docs_open.get_untracked(),
        "after Escape: {}",
        h.turn()
    );
    let docs = h.open(ChatScope::Docs);
    assert!(
        docs.contains("Documentation draft") && docs.contains("Documentation history"),
        "{docs}"
    );
    h.escape();
    assert!(!h.store.docs_open.get_untracked());
    let companion = h.open(ChatScope::Run);
    assert!(
        companion.contains("Companion draft") && companion.contains("Companion history"),
        "{companion}"
    );
    assert!(
        h.rx.try_iter().next().is_none(),
        "opening/switching must not start inference"
    );
}

#[test]
fn each_fixed_entry_submits_only_to_its_own_side_history() {
    let mut h = harness();
    h.turn();
    for scope in [ChatScope::Docs, ChatScope::Run] {
        h.open(scope);
        h.term.push_input(b"\r");
        h.turn();
        let cmd = h.rx.try_recv().unwrap();
        let Cmd::AssistantAsk {
            scope: actual,
            corpus,
            context,
            question,
            ..
        } = cmd
        else {
            panic!("unexpected command: {cmd:?}")
        };
        assert_eq!(actual, scope);
        assert!(context.contains("Main conversation request"));
        assert_eq!(
            question,
            if scope == ChatScope::Docs {
                "Documentation draft"
            } else {
                "Companion draft"
            }
        );
        assert_eq!(
            corpus,
            if scope == ChatScope::Docs {
                None
            } else {
                Some(String::new())
            }
        );
        assert!(
            h.rx.try_iter().next().is_none(),
            "no coding command may be submitted"
        );
        h.escape();
    }
    assert!(h.store.docs_chat.with_untracked(|s| s.busy));
    assert!(h.store.run_chat.with_untracked(|s| s.busy));
    assert_eq!(
        h.store
            .docs_chat
            .with_untracked(|s| s.messages.last().unwrap().1.clone()),
        "Documentation draft"
    );
    assert_eq!(
        h.store
            .run_chat
            .with_untracked(|s| s.messages.last().unwrap().1.clone()),
        "Companion draft"
    );
}

#[test]
fn outside_close_synchronizes_the_selected_entry_without_reopening() {
    let mut h = harness();
    h.turn();
    for scope in [ChatScope::Docs, ChatScope::Run] {
        h.open(scope);
        h.term.push_input(b"\x1b[<0;1;10M\x1b[<0;1;10m");
        h.turn();
        h.turn();
        assert!(!h.store.docs_open.get_untracked());
        let screen = h.turn();
        assert!(!screen.contains("· AbstractCode"), "{screen}");
        let reopened = h.open(scope);
        assert!(reopened.contains("· AbstractCode"), "{reopened}");
        h.escape();
    }
}

#[test]
fn dropping_the_host_with_either_drawer_open_does_not_read_disposed_store_signals() {
    for scope in [ChatScope::Docs, ChatScope::Run] {
        let mut h = harness();
        h.turn();
        h.open(scope);
        drop(h);
    }
}

#[test]
fn a_side_question_sends_the_history_window_and_shows_what_it_left_out() {
    let mut h = harness();
    h.turn();
    // An older turn larger than the 50,000-token window, then a newer one.
    let huge = "older reply ".repeat(20_000);
    h.store.fold.update(|f| {
        f.push_item(Item::Assistant {
            text: huge.clone(),
            final_answer: true,
            author: None,
        });
        f.push_item(Item::User {
            text: "Newest request".into(),
        });
    });
    h.open(ChatScope::Run);
    h.term.push_input(b"\r");
    let screen = h.turn();
    let Cmd::AssistantAsk { context, .. } = h.rx.try_recv().unwrap() else {
        panic!("expected a side question")
    };
    assert!(context.contains("## User\nNewest request"), "{context}");
    assert!(!context.contains("Main conversation request") && !context.contains(&huge));
    assert!(context.contains("[#TRUNCATION: 2 earlier message(s)"));
    let note = h.store.run_chat.with_untracked(|s| s.context_note.clone());
    let note = note.expect("the left-out messages are stated in the drawer");
    assert!(note.contains("2 older message(s)"), "{note}");
    assert!(screen.contains("This question included"), "{screen}");
}
