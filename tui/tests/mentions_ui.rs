//! `@entity` mentions in the REAL interface (2026-11-08 contract), driven
//! through AbstractTUI's capture harness: the roster popup, the chips, the
//! structured send (the person's turn, then one mention call per chip in
//! order), the answers' authors, and every refusal the person can meet.
//! The worker is a dummy command channel; each mention outcome is applied
//! exactly as the mentions lane's posted closure applies it
//! (`gateway::mentions::apply`). `MENTIONS_CAPTURE_DIR=<dir>` writes the
//! 120x40 screens as text + SVG.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;
use abstracttui::widgets::{AuthorAvatar, AuthorKind, ChatAuthor};

use abstractcode::config::Prefs;
use abstractcode::convo::Focus;
use abstractcode::entities::EntityInfo;
use abstractcode::gateway::mentions::{apply, Outcome};
use abstractcode::mention::{MentionCall, Refusal};
use abstractcode::runner::Cmd;
use abstractcode::store::{Phase, Store, Workflow};
use abstractcode::transcript::Item;
use abstractcode::ui::{self, UiCtx};

const SESSION: &str = "acode-mentions-session";

struct Harness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    rx: mpsc::Receiver<Cmd>,
}

fn harness() -> Harness {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let size = Size::new(120, 40);
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
        store.session_id.set(SESSION.into());
        store.workflow.set(Workflow {
            bundle_id: "basic-agent".into(),
            flow_id: "agent".into(),
            name: "basic-agent".into(),
            version: "9.9.9".into(),
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
            no_project_context: true,
            no_prompt_cache: false,
            replay_turns: 5,
            gateway_label: "127.0.0.1:18894".into(),
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
    let store = store_slot.borrow().expect("store");
    let mut h = Harness {
        app,
        term,
        driver,
        store,
        rx,
    };
    let entity = |slug: &str, name: &str, state: &str| EntityInfo {
        slug: slug.into(),
        name: name.into(),
        state: state.into(),
        liveness: "alive".into(),
        ..Default::default()
    };
    h.store.entities.set(vec![
        entity("castor", "Castor", "awake"),
        entity("pollux", "Pollux", "asleep"),
    ]);
    // Leave the animated splash (a conversation item) so frames settle.
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "the release plan is in docs/plan.md".into(),
        });
        f.push_item(Item::Assistant {
            text: "I read it: three steps, the gateway first.".into(),
            final_answer: true,
            author: None,
        });
    });
    for _ in 0..3 {
        h.turn();
    }
    h
}

impl Harness {
    fn turn(&mut self) -> String {
        self.driver
            .turn(&mut self.app, &mut self.term)
            .expect("turn");
        self.term.screen().to_text()
    }

    fn keys(&mut self, bytes: &[u8]) -> String {
        self.term.push_input(bytes);
        self.turn();
        self.turn()
    }

    /// Type `@query` and accept the popup's highlighted row: one chip.
    fn pick(&mut self, query: &str) -> String {
        self.keys(format!("@{query}").as_bytes());
        self.keys(b"\r")
    }

    fn cmds(&mut self) -> Vec<Cmd> {
        let mut out = Vec::new();
        while let Ok(c) = self.rx.try_recv() {
            out.push(c);
        }
        out
    }

    /// Text + SVG of the screen under `MENTIONS_CAPTURE_DIR`.
    fn capture(&mut self, name: &str) -> String {
        let screen = self.turn();
        if let Ok(dir) = std::env::var("MENTIONS_CAPTURE_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(format!("{dir}/{name}.txt"), &screen).unwrap();
            let svg = self.term.screen().screenshot().to_svg();
            std::fs::write(format!("{dir}/{name}.svg"), svg).unwrap();
        }
        screen
    }

    /// The two-chip message, sent: returns the commands it produced.
    fn send_two_chips(&mut self) -> Vec<Cmd> {
        self.pick("cas");
        self.keys(b"and ");
        self.pick("pol");
        self.keys(b"does the plan hold?");
        self.keys(b"\r");
        self.cmds()
    }
}

fn entity_author(name: &str, color: &str) -> ChatAuthor {
    ChatAuthor::new(AuthorKind::Entity, name)
        .entity_id(format!("entity:{}", name.to_lowercase()))
        .avatar(AuthorAvatar {
            style: Some("watch-halo".into()),
            color: Some(color.into()),
        })
}

fn call(entity: &str, name: &str, index: usize) -> MentionCall {
    MentionCall {
        entity: entity.into(),
        name: name.into(),
        index,
    }
}

const TEXT: &str = "@Castor and @Pollux does the plan hold?";

/// The run's answer landed and the run ended (what the runner posts).
fn agent_answered(h: &mut Harness) {
    h.store.fold.update(|f| {
        f.push_item(Item::Assistant {
            text: "Yes: the gateway goes first, then the clients.".into(),
            final_answer: true,
            author: Some(ChatAuthor::new(AuthorKind::Agent, "basic-agent")),
        })
    });
    h.store.phase.set(Phase::Idle);
}

#[test]
fn the_popup_offers_the_roster_and_a_pick_is_one_chip() {
    let mut h = harness();
    let screen = h.keys(b"@");
    let open = h.capture("120x40-roster-popup");
    assert!(open.contains("@Castor"), "{screen}");
    assert!(open.contains("@Pollux"), "{open}");
    assert!(
        open.contains("entity · asleep"),
        "the hint rides the row:\n{open}"
    );
    // Narrow by name, accept: the composer holds ONE chip, spelled @Castor.
    h.keys(b"ca");
    let screen = h.keys(b"\r");
    assert!(screen.contains("❯ ▐ @Castor "), "{screen}");
    // The first-use hint names who will read the history.
    assert!(
        screen.contains("Castor will read this conversation's history."),
        "{screen}"
    );
    // Backspace twice (the trailing space, then the chip) empties it: the
    // chip goes whole.
    h.keys(&[0x7f]);
    let screen = h.keys(&[0x7f]);
    assert!(!screen.contains("@Castor"), "{screen}");
    assert!(
        !screen.contains("will read this conversation's history"),
        "{screen}"
    );
}

#[test]
fn two_chips_send_the_turn_then_one_mention_call_per_chip_in_order() {
    let mut h = harness();
    h.pick("cas");
    h.keys(b"and ");
    h.pick("pol");
    let draft = h.keys(b"does the plan hold?");
    assert!(
        draft.contains("Castor and Pollux will read this conversation's history."),
        "{draft}"
    );
    h.keys(b"\r");
    let cmds = h.cmds();
    let start = cmds
        .iter()
        .position(|c| matches!(c, Cmd::Start { prompt, .. } if prompt == TEXT))
        .expect("the person's turn is sent as typed, chips spelled @name");
    let mention = cmds
        .iter()
        .position(|c| matches!(c, Cmd::Mentions { .. }))
        .expect("the mention calls");
    assert!(start < mention, "the turn reaches the runner first");
    let Cmd::Mentions {
        session_id,
        text,
        calls,
        repeat_turn,
        ..
    } = &cmds[mention]
    else {
        unreachable!()
    };
    assert_eq!(session_id, SESSION);
    assert_eq!(text, TEXT);
    assert!(
        *repeat_turn,
        "the words started a turn: the calls name it as repeat_of"
    );
    assert_eq!(
        calls,
        &vec![call("castor", "Castor", 0), call("pollux", "Pollux", 1)]
    );
    // Told once per session: the next draft naming Castor shows no hint.
    let next = h.pick("cas");
    assert!(
        !next.contains("will read this conversation's history"),
        "{next}"
    );
}

#[test]
fn entity_replies_name_their_author_and_a_plain_answer_stays_assistant() {
    let mut h = harness();
    h.send_two_chips();
    agent_answered(&mut h);
    apply(
        h.store,
        SESSION,
        &call("castor", "Castor", 0),
        Outcome::Answered {
            reply: "It holds, if the gateway restarts before the clients ship.".into(),
            author: Some(entity_author("Castor", "#39a5d0")),
        },
        "",
    );
    apply(
        h.store,
        SESSION,
        &call("pollux", "Pollux", 1),
        Outcome::Answered {
            reply: "Agreed with Castor; I would add a rollback note.".into(),
            author: Some(entity_author("Pollux", "#d0396b")),
        },
        "",
    );
    h.turn();
    let screen = h.capture("120x40-two-chips-two-replies");
    assert!(
        screen.contains(&format!("▌ ❯ {TEXT}")),
        "the person's card:\n{screen}"
    );
    assert!(screen.contains("══ ✦ basic-agent · agent ═"), "{screen}");
    assert!(screen.contains("══ ● Castor · entity ═"), "{screen}");
    assert!(screen.contains("══ ● Pollux · entity ═"), "{screen}");
    assert!(screen.contains("if the gateway restarts"), "{screen}");
    assert!(screen.contains("rollback note"), "{screen}");
    // Order: the agent, then Castor, then Pollux.
    let at = |s: &str| screen.find(s).unwrap();
    assert!(at("basic-agent · agent") < at("Castor · entity"));
    assert!(at("Castor · entity") < at("Pollux · entity"));
    // The setup's answer has no author: exactly the plain row.
    assert!(screen.contains("══ assistant ═"), "{screen}");
}

#[test]
fn an_answer_without_an_author_renders_exactly_as_before() {
    // Pinned: author None is the pre-mention rendering, row for row.
    let mut h = harness();
    let screen = h.turn();
    let row = screen
        .lines()
        .find(|l| l.contains("assistant"))
        .expect("the answer rule");
    assert!(
        row.trim_start().starts_with("══ assistant ══"),
        "the old rule, unchanged: {row:?}"
    );
    assert!(!screen.contains(" · agent"), "{screen}");
    assert!(!screen.contains("● "), "{screen}");
}

#[test]
fn a_409_is_a_notice_row_and_the_next_mention_still_answers() {
    let mut h = harness();
    h.send_two_chips();
    agent_answered(&mut h);
    apply(
        h.store,
        SESSION,
        &call("castor", "Castor", 0),
        Outcome::Refused(Refusal::Elsewhere),
        "",
    );
    apply(
        h.store,
        SESSION,
        &call("pollux", "Pollux", 1),
        Outcome::Answered {
            reply: "Castor is busy elsewhere; the plan holds.".into(),
            author: Some(entity_author("Pollux", "#d0396b")),
        },
        "",
    );
    h.turn();
    let screen = h.capture("120x40-409-notice");
    assert!(
        screen.contains("· Castor is in another conversation."),
        "{screen}"
    );
    assert!(screen.contains("══ ● Pollux · entity ═"), "{screen}");
    assert!(
        h.store.steer_restore.get_untracked().is_none(),
        "a 409 keeps nothing in the composer"
    );
}

#[test]
fn a_503_says_why_and_puts_the_message_back_in_the_composer() {
    let mut h = harness();
    h.pick("cas");
    h.keys(b"is it ready?");
    h.keys(b"\r");
    let raw = h
        .cmds()
        .into_iter()
        .find_map(|c| match c {
            Cmd::Mentions { draft, .. } => Some(draft),
            _ => None,
        })
        .expect("mention sent");
    agent_answered(&mut h);
    apply(
        h.store,
        SESSION,
        &call("castor", "Castor", 0),
        Outcome::Refused(Refusal::CannotAnswer(
            "Castor cannot answer: no model chosen".into(),
        )),
        &raw,
    );
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("· Castor cannot answer: no model chosen"),
        "{screen}"
    );
    assert!(
        screen.contains("❯ ▐ @Castor is it ready?"),
        "the message (its chip included) is back in the composer:\n{screen}"
    );
}

#[test]
fn a_gateway_without_the_route_says_so_once_and_turns_chips_off() {
    let mut h = harness();
    h.send_two_chips();
    agent_answered(&mut h);
    apply(
        h.store,
        SESSION,
        &call("castor", "Castor", 0),
        Outcome::Refused(Refusal::NotOffered),
        "",
    );
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("· This gateway does not offer mentions yet."),
        "{screen}"
    );
    assert!(h.store.mentions_off.get_untracked());
    // The popup offers nothing now: `@cas` stays typed text.
    let screen = h.keys(b"@cas");
    assert!(
        !screen.contains("entity · awake"),
        "no roster rows open:\n{screen}"
    );
    h.keys(b"\r");
    let cmds = h.cmds();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Cmd::Start { prompt, .. } if prompt == "@cas")),
        "typed text is a plain prompt"
    );
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::Mentions { .. })));
}

#[test]
fn under_an_entity_focus_chips_ask_no_one_and_say_so() {
    let mut h = harness();
    h.store.focus.set(Focus::Entity("castor".into()));
    h.turn();
    h.pick("pol");
    h.keys(b"what do you think?");
    h.keys(b"\r");
    let cmds = h.cmds();
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::Mentions { .. })));
    assert!(
        h.store
            .notices
            .get_untracked()
            .iter()
            .any(|n| n.contains("Pollux not asked: mentions are sent from the agent conversation")),
        "{:?}",
        h.store.notices.get_untracked()
    );
}

#[test]
fn an_answer_for_a_conversation_left_meanwhile_is_not_written_here() {
    let mut h = harness();
    h.send_two_chips();
    h.store.session_id.set("another-session".into());
    let before = h.store.fold.with_untracked(|f| f.items.len());
    apply(
        h.store,
        SESSION,
        &call("castor", "Castor", 0),
        Outcome::Answered {
            reply: "late".into(),
            author: Some(entity_author("Castor", "#39a5d0")),
        },
        "",
    );
    assert_eq!(h.store.fold.with_untracked(|f| f.items.len()), before);
    assert!(h
        .store
        .notices
        .get_untracked()
        .iter()
        .any(|n| n.starts_with("Castor: the answer is in the conversation you left")));
}

#[test]
fn chips_sent_as_guidance_name_no_turn_to_repeat() {
    // Mid-run the words steer the running turn: they start no turn of
    // their own, so the mention calls carry no repeat_of.
    let mut h = harness();
    h.store.phase.set(Phase::Running);
    h.store.run_id.set("root".into());
    h.turn();
    h.pick("cas");
    h.keys(b"check this");
    h.keys(b"\r");
    let cmds = h.cmds();
    assert!(
        !cmds.iter().any(|c| matches!(c, Cmd::Start { .. })),
        "a steer, not a start: {cmds:?}"
    );
    let repeat = cmds.iter().find_map(|c| match c {
        Cmd::Mentions { repeat_turn, .. } => Some(*repeat_turn),
        _ => None,
    });
    assert_eq!(repeat, Some(false));
}

#[test]
fn an_entity_this_account_cannot_mention_is_a_notice_row() {
    let mut h = harness();
    h.send_two_chips();
    agent_answered(&mut h);
    apply(
        h.store,
        SESSION,
        &call("castor", "Castor", 0),
        Outcome::Refused(Refusal::NotFound),
        "",
    );
    h.turn();
    let screen = h.turn();
    assert!(
        screen.contains("· Castor cannot be mentioned here."),
        "{screen}"
    );
    assert!(!h.store.mentions_off.get_untracked(), "mentions stay on");
}

#[test]
fn a_live_reply_is_labelled_by_its_frames_author() {
    use abstractcode::runner::{apply_live_event, reset_live_on_connect};
    let mut h = harness();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| {
        f.begin_run("root");
        f.push_item(Item::User {
            text: "explain streaming".into(),
        });
    });
    h.turn();
    reset_live_on_connect(&store, "root");
    let frame = |author: serde_json::Value| {
        let data = serde_json::json!({
            "run_id": "root", "root_run_id": "root", "node_id": "reason",
            "call_id": "c1", "seq": 1, "text": "Streaming words", "channel": "content",
            "snapshot": true, "author": author,
        });
        abstractcode::live::parse_event("llm.delta", &data.to_string())
            .expect("valid frame")
            .expect("a live event")
    };
    apply_live_event(
        &store,
        "root",
        frame(serde_json::json!({"kind": "agent", "name": "basic-agent",
            "entity_id": null, "identity_ref": null, "avatar": null})),
    );
    let screen = h.turn();
    assert!(
        screen.contains("✦ basic-agent · agent · reason"),
        "the bubble says who answers:\n{screen}"
    );
    assert!(screen.contains("streaming…"), "{screen}");
    // An older gateway's frame (no author): the caption it always had.
    let mut h = harness();
    let store = h.store;
    store.phase.set(Phase::Running);
    store.run_id.set("root".into());
    store.fold.update(|f| f.begin_run("root"));
    h.turn();
    reset_live_on_connect(&store, "root");
    apply_live_event(&store, "root", frame(serde_json::Value::Null));
    let screen = h.turn();
    assert!(screen.contains("✎ reply · reason"), "{screen}");
}

#[test]
fn chips_inside_a_command_ask_no_one_and_say_so() {
    let mut h = harness();
    h.keys(b"/queue ");
    h.pick("cas");
    h.keys(b"review the plan");
    h.keys(b"\r");
    let cmds = h.cmds();
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::Mentions { .. })));
    assert!(
        h.store.notices.get_untracked().iter().any(|n| n
            .contains("Castor not asked: mentions are sent with a message, not with a command")),
        "{:?}",
        h.store.notices.get_untracked()
    );
    // The command still ran with the text (an idle queue drains at once).
    h.turn();
    let queued_or_started =
        h.store
            .queue
            .with_untracked(|q| q.iter().any(|p| p.text == "@Castor review the plan"))
            || h.cmds().iter().chain(cmds.iter()).any(
                |c| matches!(c, Cmd::Start { prompt, .. } if prompt == "@Castor review the plan"),
            );
    assert!(queued_or_started, "the command still ran with the text");
}
