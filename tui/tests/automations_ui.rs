//! Headless UI tests for `/automations` and `/schedule`: the REAL interface
//! driven through AbstractTUI's capture harness (same pipeline as
//! production, no pty). The worker is a dummy command channel; gateway
//! answers are the canonical ui-kit fixtures, applied to the store exactly
//! as the automations lane's posted closures apply them.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;
use serde_json::{json, Value};

use abstractcode::automations as auto;
use abstractcode::config::Prefs;
use abstractcode::gateway::automations::AutoCmd;
use abstractcode::runner::Cmd;
use abstractcode::store::{Store, Workflow};
use abstractcode::ui::{self, UiCtx};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/automations");
const INBOX: &str = "53443dd0-25c4-5fa8-bdad-e1ac3fdfff8e";

fn fixture(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(format!("{FIXTURES}/{name}")).unwrap()).unwrap()
}

struct Harness {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    rx: mpsc::Receiver<Cmd>,
}

fn harness() -> Harness {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let size = Size::new(140, 44);
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, rx) = mpsc::channel::<Cmd>();
    let store_slot: Rc<RefCell<Option<Store>>> = Rc::new(RefCell::new(None));
    let store_out = store_slot.clone();
    let prefs = Rc::new(RefCell::new(Prefs::default()));
    let actions = app.actions();
    app.mount(move |cx| {
        let store = Store::create(cx);
        *store_out.borrow_mut() = Some(store);
        store.session_id.set("acode-test-session".into());
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
            prefs: prefs.clone(),
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
    // Leave the animated splash (a conversation item) so frames settle.
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "report the free memory of this computer".into(),
        })
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

    fn command(&mut self, text: &str) -> String {
        self.term.push_input(text.as_bytes());
        self.turn();
        self.keys(b"\r")
    }

    /// Every automations-lane command queued so far (other commands dropped).
    fn auto_cmds(&mut self) -> Vec<AutoCmd> {
        let mut out = Vec::new();
        while let Ok(cmd) = self.rx.try_recv() {
            if let Cmd::Automations(c) = cmd {
                out.push(c);
            }
        }
        out
    }

    /// Answer the list read with the fixture page (as the lane would).
    fn answer_list(&mut self) {
        let page = auto::parse_list_page(&fixture("list.json")).unwrap();
        self.store.automations.update(|v| {
            v.availability = Some(Ok(()));
            v.apply_list(page.items);
        });
        self.turn();
    }

    /// Answer the open automation with the fixture detail + occurrences.
    fn answer_detail(&mut self) -> String {
        let list = auto::parse_list_page(&fixture("list.json")).unwrap();
        let summary = list.items.into_iter().find(|s| s.id == INBOX).unwrap();
        let definition = auto::Definition {
            revision: 3,
            workflow_id: "inbox@1.0.0:triage".into(),
            tool_approval: "ask".into(),
            growing: Default::default(),
            max_attempts: Some(3),
            workspace_root: summary.workspace_root.clone().unwrap(),
        };
        let page = auto::parse_occurrence_page(&fixture("occurrences.json")).unwrap();
        self.store
            .automations
            .update(|v| v.apply_detail(INBOX, definition, summary, page));
        self.turn();
        self.turn()
    }
}

fn open_inbox(h: &mut Harness) -> String {
    h.command("/automations");
    h.answer_list();
    // Inbox triage is the first visible row.
    h.keys(b"\r");
    let cmds = h.auto_cmds();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, AutoCmd::Open { id } if id == INBOX)),
        "Enter opens the selected automation: {cmds:?}"
    );
    assert!(
        cmds.iter().any(
            |c| matches!(c, AutoCmd::Seen { id, cursor } if id == INBOX && cursor == "att1:2")
        ),
        "showing the attention items acknowledges the LAST displayed cursor: {cmds:?}"
    );
    h.answer_detail()
}

#[test]
fn the_list_reads_state_now_and_next_from_the_gateway() {
    let mut h = harness();
    h.command("/automations");
    let cmds = h.auto_cmds();
    assert!(
        matches!(cmds.first(), Some(AutoCmd::Refresh { open: None })),
        "/automations reads the list: {cmds:?}"
    );
    let screen = h.turn();
    assert!(screen.contains("asking the gateway"), "{screen}");
    h.answer_list();
    let screen = h.turn();
    assert!(screen.contains("Inbox triage · Active ▶ · 2 unseen · 2 waiting for you · every 30 minutes (UTC) · now: Run #7 running"), "{screen}");
    assert!(
        screen.contains("Weekly journal monitor · Paused ⏸"),
        "{screen}"
    );
    assert!(screen.contains("next: none while paused"), "{screen}");
    assert!(screen.contains("legacy schedule"), "{screen}");
}

#[test]
fn archived_rows_are_hidden_until_asked() {
    let mut h = harness();
    h.command("/automations");
    let mut page = auto::parse_list_page(&fixture("list.json")).unwrap();
    page.items[1].status = "archived".into();
    let archived_title = page.items[1].title.clone();
    h.store.automations.update(|v| v.apply_list(page.items));
    let screen = h.turn();
    assert!(
        !screen.contains(&format!("{archived_title} · ")),
        "{screen}"
    );
    assert!(
        screen.contains("1 archived hidden — h shows them"),
        "{screen}"
    );
    let screen = h.keys(b"h");
    assert!(
        screen.contains(&format!("{archived_title} · Archived")),
        "{screen}"
    );
}

#[test]
fn controls_follow_the_shared_rules_and_send_typed_commands() {
    let mut h = harness();
    h.command("/automations");
    h.answer_list();
    h.auto_cmds();
    // Inbox triage has a run in progress: run now is refused with the reason.
    let screen = h.keys(b"g");
    assert!(
        h.auto_cmds().is_empty(),
        "run now must not be sent while a run is in progress"
    );
    assert!(
        screen.contains("run now: An occurrence is in progress."),
        "{screen}"
    );
    // Stop current applies.
    h.keys(b"x");
    let cmds = h.auto_cmds();
    assert!(
        matches!(cmds.as_slice(), [AutoCmd::Command { id, command_type, .. }] if id == INBOX && command_type == "automation.stop_current"),
        "{cmds:?}"
    );
    // The paused row (third visible): run now works while paused, p resumes.
    h.store.automations.update(|v| v.busy = false);
    h.keys(b"\x1b[B\x1b[B");
    h.keys(b"g");
    h.store.automations.update(|v| v.busy = false);
    h.keys(b"p");
    let types: Vec<String> = h
        .auto_cmds()
        .into_iter()
        .filter_map(|c| match c {
            AutoCmd::Command { command_type, .. } => Some(command_type),
            _ => None,
        })
        .collect();
    assert_eq!(types, vec!["automation.run_now", "automation.resume"]);
}

#[test]
fn archive_needs_a_second_press_and_says_it_keeps_history() {
    let mut h = harness();
    h.command("/automations");
    h.answer_list();
    h.auto_cmds();
    let screen = h.keys(b"a");
    assert!(h.auto_cmds().is_empty(), "one press only asks");
    assert!(screen.contains("its history stays readable"), "{screen}");
    h.keys(b"a");
    let cmds = h.auto_cmds();
    assert!(
        matches!(cmds.as_slice(), [AutoCmd::Command { command_type, .. }] if command_type == "automation.archive"),
        "{cmds:?}"
    );
}

#[test]
fn one_automation_shows_folder_waits_and_runs_as_chat_pairs() {
    let mut h = harness();
    let screen = open_inbox(&mut h);
    assert!(screen.contains("automation — Inbox triage"), "{screen}");
    assert!(screen.contains("now: Run #7 running"), "{screen}");
    assert!(
        screen.contains("folder: /srv/abstractgateway/data/workspaces/session-automation-53443dd0"),
        "{screen}"
    );
    assert!(
        screen.contains("tools: ask me before each tool call"),
        "{screen}"
    );
    assert!(screen.contains("⚠ Question for you · run #7"), "{screen}");
    assert!(screen.contains("⚠ Approval needed"), "{screen}");
    assert!(
        screen.contains("• run #5 failed: Inbox triage failed"),
        "{screen}"
    );
    assert!(screen.contains("#2 · completed · Notified"), "{screen}");
    assert!(screen.contains("task:   [Trigger schedule@1"), "{screen}");
    // Walking down scrolls the runs into view (oldest first).
    for _ in 0..7 {
        h.keys(b"\x1b[B");
    }
    let screen = h.turn();
    assert!(
        screen.contains("#5 · failed · Failed after 3 attempts"),
        "{screen}"
    );
    assert!(
        screen.contains("failed: occurrence_failed IMAP read timed out"),
        "{screen}"
    );
    // Showing the attention items acknowledged the LAST displayed cursor
    // once (`open_inbox` saw the Seen); nothing is re-sent while in flight.
    assert_eq!(
        h.store
            .automations
            .with_untracked(|v| v.ack_inflight.clone()),
        Some((INBOX.to_string(), "att1:2".to_string()))
    );
    assert!(!h
        .auto_cmds()
        .iter()
        .any(|c| matches!(c, AutoCmd::Seen { .. })));
    // The gateway accepted it: never sent again for that cursor.
    h.store
        .automations
        .update(|v| v.mark_acked(INBOX, "att1:2"));
    h.answer_detail();
    assert!(!h
        .auto_cmds()
        .iter()
        .any(|c| matches!(c, AutoCmd::Seen { .. })));
}

#[test]
fn a_tool_approval_is_answered_by_kind() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    // The cursor rests on the first wait (the question); down is the approval.
    h.keys(b"\x1b[B");
    h.keys(b"y");
    let cmds = h.auto_cmds();
    match cmds.as_slice() {
        [AutoCmd::Answer {
            id, wait, payload, ..
        }] => {
            assert_eq!(id, INBOX);
            assert_eq!(wait.kind, "tool_approval");
            assert_eq!(payload, &json!({"approved": true}));
        }
        other => panic!("expected one tool-approval answer, got {other:?}"),
    }
    // `y` on the question row is refused (the answer follows the kind).
    h.store.automations.update(|v| v.busy = false);
    h.keys(b"\x1b[A");
    let screen = h.keys(b"y");
    assert!(h.auto_cmds().is_empty());
    assert!(
        screen.contains("select an “Approval needed” row first"),
        "{screen}"
    );
}

#[test]
fn discuss_forks_a_finished_run_and_switches_this_terminal_to_it() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    // At rest the cursor is on a wait: discuss asks for a run.
    let screen = h.keys(b"d");
    assert!(screen.contains("select a run (↑↓) to discuss"), "{screen}");
    // The newest run (#7, at the bottom) is still waiting: discuss refuses it.
    for _ in 0..12 {
        h.keys(b"\x1b[B");
    }
    let screen = h.keys(b"d");
    assert!(
        screen.contains("discuss: run #7 has not finished yet"),
        "{screen}"
    );
    // Up to #6 (finished), discuss, type the first message.
    h.keys(b"\x1b[A");
    let screen = h.keys(b"d");
    assert!(screen.contains("discuss run #6"), "{screen}");
    assert!(screen.contains("mounted read-only"), "{screen}");
    h.term.push_input(b"what changed since #5?");
    h.turn();
    h.keys(b"\r");
    let cmds = h.auto_cmds();
    match cmds.as_slice() {
        [AutoCmd::Discuss {
            id, index, prompt, ..
        }] => {
            assert_eq!(id, INBOX);
            assert_eq!(*index, 6);
            assert_eq!(prompt, "what changed since #5?");
        }
        other => panic!("expected one discuss, got {other:?}"),
    }
    // The gateway answers: this terminal switches to the discussion session.
    h.store.automations.update(|v| {
        v.busy = false;
        v.discussion = Some((
            6,
            auto::DiscussResponse {
                session_id: "discussion-session:abc".into(),
                run_id: "run-d".into(),
                workspace_root: "/gw/workspaces/discussion-abc".into(),
                mounted_workspace: "/gw/workspaces/session-automation-53443dd0".into(),
            },
        ))
    });
    h.turn();
    h.turn();
    assert_eq!(h.store.session_id.get_untracked(), "discussion-session:abc");
    let mut attached = false;
    while let Ok(cmd) = h.rx.try_recv() {
        if let Cmd::ProbeAttach { session_id, .. } = cmd {
            attached = session_id == "discussion-session:abc";
        }
    }
    assert!(attached, "the discussion session is attached like any chat");
    assert!(h
        .store
        .notices
        .get_untracked()
        .iter()
        .any(|n| n.contains("its own workspace /gw/workspaces/discussion-abc")));
}

#[test]
fn the_folder_is_browsed_through_the_gateway_workspace_routes() {
    let mut h = harness();
    open_inbox(&mut h);
    while h.rx.try_recv().is_ok() {}
    let screen = h.keys(b"w");
    assert!(
        screen.contains("automation folder — “Inbox triage” on the gateway"),
        "{screen}"
    );
    let mut loaded = None;
    while let Ok(cmd) = h.rx.try_recv() {
        if let Cmd::LoadWorkspaceFiles { run_id, .. } = cmd {
            loaded = Some(run_id);
        }
    }
    assert_eq!(
        loaded.as_deref(),
        Some(INBOX),
        "the automation id is its controller run id"
    );
}

#[test]
fn schedule_creates_the_shared_definition_from_the_current_workflow() {
    let mut h = harness();
    let screen = h.command("/schedule");
    assert!(screen.contains("new automation — 1/4 the task"), "{screen}");
    assert!(
        screen.contains("The task below is sent as the prompt of every run."),
        "every info line fits:\n{screen}"
    );
    // The task defaults to the conversation's last prompt.
    let screen = h.keys(b"\r");
    // Every When row is visible (the panel fits its rows).
    assert!(
        screen.contains("every 5 minutes (UTC)")
            && screen.contains("once, at a date and time (UTC)"),
        "{screen}"
    );
    // When: presets start at "every 24 hours"; go up to "every 5 minutes".
    for _ in 0..4 {
        h.keys(b"\x1b[A");
    }
    h.keys(b"\r");
    // Context: Growing.
    h.keys(b"\x1b[B");
    let screen = h.keys(b"\r");
    assert!(screen.contains("4/4 tools"), "{screen}");
    assert!(
        screen.contains("Run without asking — tools run without asking"),
        "{screen}"
    );
    assert!(
        screen.contains("Ask me before each tool call"),
        "both tool choices are visible:\n{screen}"
    );
    // Tools: ask each time.
    h.keys(b"\x1b[B");
    h.keys(b"\r");
    let cmds = h.auto_cmds();
    let body = match cmds.iter().find(|c| matches!(c, AutoCmd::Create { .. })) {
        Some(AutoCmd::Create { body }) => body.clone(),
        other => panic!("expected a create, got {other:?} in {cmds:?}"),
    };
    let rid = body["request_id"].as_str().unwrap().to_string();
    assert!(!rid.is_empty());
    assert_eq!(
        body,
        json!({
            "request_id": rid,
            "title": "report the free memory of this computer",
            "target": {"bundle_ref": "basic-agent@9.9.9", "flow_id": "agent",
                       "input_data": {"prompt": "report the free memory of this computer"}},
            "trigger": {"source_id": "schedule", "source_version": 1, "config": {"every": "5m"}},
            "context": {"mode": "growing"},
            "policy": {"tool_approval": "ask"},
        })
    );
    // The gateway answers: the new automation opens.
    h.store.automations.update(|v| {
        v.busy = false;
        v.created = Some("new-auto".into());
    });
    h.turn();
    h.turn();
    let cmds = h.auto_cmds();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, AutoCmd::Open { id } if id == "new-auto")),
        "{cmds:?}"
    );
}

#[test]
fn schedule_with_the_gateway_default_targets_at_default() {
    let mut h = harness();
    h.store.workflow.update(|w| w.gateway_default = true);
    h.command("/schedule watch the disk");
    h.keys(b"\r"); // task (seeded from the argument)
    h.keys(b"\r"); // every 24 hours
    h.keys(b"\r"); // independent
    h.keys(b"\r"); // tools run without asking
    let body = h
        .auto_cmds()
        .into_iter()
        .find_map(|c| match c {
            AutoCmd::Create { body } => Some(body),
            _ => None,
        })
        .expect("create");
    assert_eq!(
        body["target"],
        json!({"flow_id": "@default", "interface": "abstractcode.agent.v1", "input_data": {"prompt": "watch the disk"}})
    );
    assert_eq!(body["trigger"]["config"], json!({"every": "24h"}));
    assert_eq!(body["context"], json!({"mode": "independent"}));
    assert_eq!(body["policy"], json!({"tool_approval": "auto"}));
}

#[test]
fn archive_from_one_automation_also_asks_first() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    let screen = h.keys(b"a");
    assert!(h.auto_cmds().is_empty(), "one press only asks");
    assert!(screen.contains("its history stays readable"), "{screen}");
    h.keys(b"a");
    let cmds = h.auto_cmds();
    assert!(
        matches!(cmds.as_slice(), [AutoCmd::Command { id, command_type, .. }] if id == INBOX && command_type == "automation.archive"),
        "{cmds:?}"
    );
}

#[test]
fn a_refused_revision_stays_readable_on_the_automation() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    h.keys(b"e"); // 1/3 title
    h.keys(b"\r");
    // 2/3 interval (empty keeps "30m"): an invalid one.
    h.term.push_input(b"6 hours");
    h.turn();
    h.keys(b"\r");
    let screen = h.keys(b"\r"); // 3/3 context → back to the automation
    assert!(
        h.auto_cmds()
            .iter()
            .all(|c| !matches!(c, AutoCmd::Revise { .. })),
        "nothing is sent"
    );
    assert!(screen.contains("automation — Inbox triage"), "{screen}");
    assert!(
        screen.contains("Interval must be a whole number of minutes, hours or days"),
        "{screen}"
    );
}

#[test]
fn revise_sends_only_what_changed_and_empty_keeps_the_current_value() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    let screen = h.keys(b"e");
    assert!(
        screen.contains("Now: Inbox triage — leave empty and press Enter to keep it."),
        "{screen}"
    );
    h.keys(b"\r"); // title kept
    h.term.push_input(b"6h");
    h.turn();
    h.keys(b"\r"); // interval 6h
    h.keys(b"\r"); // context kept (growing is preselected)
    let cmds = h.auto_cmds();
    match cmds.iter().find(|c| matches!(c, AutoCmd::Revise { .. })) {
        Some(AutoCmd::Revise {
            id,
            changes,
            expected_revision,
            ..
        }) => {
            assert_eq!(id, INBOX);
            assert_eq!(*expected_revision, Some(1));
            assert_eq!(changes["trigger"]["config"]["every"], "6h");
            assert!(
                changes.get("title").is_none() && changes.get("context").is_none(),
                "{changes}"
            );
        }
        other => panic!("expected a revise, got {other:?} in {cmds:?}"),
    }
}

#[test]
fn discuss_never_cancels_a_run_in_progress_here() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    h.store.phase.set(abstractcode::store::Phase::Running);
    for _ in 0..12 {
        h.keys(b"\x1b[B");
    }
    h.keys(b"\x1b[A"); // run #6 (finished)
    let screen = h.keys(b"d");
    assert!(
        screen.contains("a run is in progress in this session"),
        "{screen}"
    );
    assert!(!screen.contains("discuss run #6"), "{screen}");
}
