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

    /// Answer the run level's dry run ("Use my default") as the lane would.
    fn answer_dry_run(&mut self) {
        let v: Value = serde_json::from_slice(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/workspaces/dryrun_default.json"
            ))
            .unwrap(),
        )
        .unwrap();
        let e = abstractcode::workspaces::as_effective(&v).unwrap();
        self.store.workspaces.update(|w| {
            w.loading.clear();
            w.put_run("null".into(), Ok(e));
        });
        self.turn();
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
            target: serde_json::Value::Null,
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
    assert!(screen.contains("Loading automations…"), "{screen}");
    h.answer_list();
    let screen = h.turn();
    // The Code web card: the name (+ the waiting badge), then
    // `↻ cadence · last`, then `next in …` with the Active switch.
    assert!(screen.contains("Inbox triage"), "{screen}");
    assert!(screen.contains("waiting for you"), "{screen}");
    // A run waits on a person: "waiting since", never "running now".
    assert!(
        screen.contains("↻ every 30 min · waiting since"),
        "{screen}"
    );
    assert!(screen.contains("↻ every 8 h · last"), "{screen}");
    assert!(screen.contains("↻ every 7 d · last"), "{screen}");
    assert!(screen.contains("[x] Active"), "{screen}");
    assert!(
        screen.contains("[ ] Active"),
        "the paused one is off:\n{screen}"
    );
    assert!(
        screen.contains("[-] Active"),
        "the legacy one cannot change:\n{screen}"
    );
    // Paused: no next part at all.
    let weekly = screen
        .lines()
        .skip_while(|l| !l.contains("Weekly journal monitor"))
        .take(3)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!weekly.contains("next "), "{weekly}");
}

#[test]
fn archived_automations_live_under_archived_n_with_unarchive() {
    let mut h = harness();
    h.command("/automations");
    let mut page = auto::parse_list_page(&fixture("list.json")).unwrap();
    // The gateway leaves archived ones out of the listing and counts them.
    let mut archived = page.items.remove(1);
    archived.status = "archived".into();
    archived.capabilities = vec!["unarchive".into(), "discuss".into()];
    let title = archived.title.clone();
    let shown = page.items.len();
    h.store.automations.update(|v| {
        v.archived_count = 1;
        v.archived = Some(Ok(vec![archived.clone()]));
        v.apply_list(page.items);
    });
    let screen = h.turn();
    assert!(!screen.contains(&title), "{screen}");
    assert!(screen.contains("Archived · 1"), "{screen}");
    assert!(!screen.contains("Show archived"), "{screen}");
    // Down to the line, Enter opens it inline; the row offers Unarchive.
    for _ in 0..shown {
        h.keys(b"\x1b[B");
    }
    let screen = h.keys(b"\r");
    assert!(screen.contains(&title), "{screen}");
    assert!(screen.contains("Unarchive"), "{screen}");
    h.auto_cmds();
    h.keys(b"\x1b[B");
    h.keys(b"\r");
    let cmds = h.auto_cmds();
    assert!(
        matches!(cmds.as_slice(), [AutoCmd::Command { id, command_type, .. }] if *id == archived.id && command_type == "automation.unarchive"),
        "{cmds:?}"
    );
    // A count of 0 leaves no line at all.
    h.store.automations.update(|v| {
        v.busy = false;
        v.archived_count = 0;
    });
    let screen = h.turn();
    assert!(!screen.contains("Archived ·"), "{screen}");
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
        screen.contains("Run now: An occurrence is in progress."),
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
fn archive_asks_inline_and_y_confirms() {
    let mut h = harness();
    h.command("/automations");
    h.answer_list();
    h.auto_cmds();
    let screen = h.keys(b"a");
    assert!(h.auto_cmds().is_empty(), "one press only asks");
    assert!(
        screen
            .contains("Archive “Inbox triage”? It will not run again; its history stays readable."),
        "{screen}"
    );
    assert!(screen.contains("y Archive · n Keep it"), "{screen}");
    // n keeps it.
    let screen = h.keys(b"n");
    assert!(h.auto_cmds().is_empty());
    assert!(!screen.contains("y Archive · n Keep it"), "{screen}");
    h.keys(b"a");
    h.keys(b"y");
    let cmds = h.auto_cmds();
    assert!(
        matches!(cmds.as_slice(), [AutoCmd::Command { command_type, .. }] if command_type == "automation.archive"),
        "{cmds:?}"
    );
    assert_eq!(
        h.store.automations.with_untracked(|v| v.notice.clone()),
        "Archiving…"
    );
}

#[test]
fn one_automation_shows_folder_waits_and_runs_as_chat_pairs() {
    let mut h = harness();
    let screen = open_inbox(&mut h);
    assert!(screen.contains("Automations / Inbox triage"), "{screen}");
    assert!(screen.contains("Run #7 running"), "{screen}");
    assert!(screen.contains("every 30 min · waiting since"), "{screen}");
    assert!(screen.contains("waiting for you"), "{screen}");
    // The workspace as a short name (never the full path in the header).
    assert!(
        screen.contains("Workspace session-automation-53443dd0-25c4-5fa8-bdad-e1ac3-db8ae8ce21b3 (w browses it)"),
        "{screen}"
    );
    assert!(!screen.contains("/srv/abstractgateway"), "{screen}");
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

    // The discussion's first turn answered; the operator's SECOND turn in
    // that session must not carry context.messages: the gateway seeds a
    // discussion itself and refuses a start that sends them (HTTP 400
    // "seeded by the gateway") — the tag-gate B1 regression.
    h.store.phase.set(abstractcode::store::Phase::Idle);
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "what changed since #5?".into(),
        });
        f.push_item(abstractcode::transcript::Item::Assistant {
            text: "the inbox rule changed".into(),
            final_answer: true,
        });
    });
    h.turn();
    h.term.push_input(b"and then?");
    h.turn();
    h.keys(b"\r");
    let mut start = None;
    while let Ok(cmd) = h.rx.try_recv() {
        if let Cmd::Start {
            prompt,
            session_id,
            opts,
            ..
        } = cmd
        {
            start = Some((prompt, session_id, opts));
        }
    }
    let (prompt, session_id, opts) = start.expect("the second discussion turn starts a run");
    assert_eq!(prompt, "and then?");
    assert_eq!(session_id, "discussion-session:abc");
    let input = abstractcode::run_input::build_input_data(&prompt, &opts);
    assert!(
        input["context"].get("messages").is_none(),
        "a discussion turn never sends context.messages: {input}"
    );
    assert_eq!(input["use_session_history"], json!(true));
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
    assert!(screen.contains("new automation — 1/6 the task"), "{screen}");
    assert!(
        screen.contains("The task below is sent as the prompt of every run."),
        "every info line fits:\n{screen}"
    );
    // The task defaults to the conversation's last prompt.
    let screen = h.keys(b"\r");
    // Every When row is visible (the panel fits its rows).
    assert!(
        screen.contains("every 5 minutes (UTC)")
            && screen.contains("Daily")
            && screen.contains("Weekly")
            && screen.contains("Monthly")
            && screen.contains("Once at…"),
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
    assert!(screen.contains("4/6 tools"), "{screen}");
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
    let screen = h.keys(b"\r");
    // 5/6 Workspaces: the kit chooser at the run level, visible (no
    // disclosure), read from the gateway's dry run.
    assert!(
        screen.contains("new automation — 5/6 Workspaces"),
        "{screen}"
    );
    assert!(screen.contains("Continue — Title and limits"), "{screen}");
    h.answer_dry_run();
    let screen = h.turn();
    assert!(screen.contains("[x] Use my default"), "{screen}");
    assert!(
        screen.contains("Gateway: Allow everything, refuse listed workspaces (rw)"),
        "{screen}"
    );
    assert!(
        screen.contains("The workspaces this run uses, among the eligible ones."),
        "{screen}"
    );
    // Continue (the cursor starts on it): 6/6 Title and limits, visible.
    let screen = h.keys(b"\r");
    assert!(
        screen.contains("new automation — 6/6 Title and limits"),
        "{screen}"
    );
    for label in [
        "Title",
        "Defaults to the task's first line",
        "First run at (UTC; empty = now)",
        "Stop after this many runs",
        "Stop at (UTC)",
        "Create automation",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    assert!(!screen.contains("Advanced"), "{screen}");
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
            "trigger": {"source_id": "schedule", "source_version": 2, "config": {"kind": "every", "every": "5m"}},
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
    h.keys(b"\r"); // Workspaces: Continue (Use my default)
    h.keys(b"\r"); // Title and limits: Create automation
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
    assert_eq!(
        body["trigger"]["config"],
        json!({"kind": "every", "every": "24h"})
    );
    assert_eq!(body["context"], json!({"mode": "independent"}));
    assert_eq!(body["policy"], json!({"tool_approval": "auto"}));
}

#[test]
fn the_dialog_workspaces_and_limits_ride_the_create_body() {
    use abstractcode::gateway::workspaces::{RunCommit, WsCmd};
    let mut h = harness();
    h.command("/schedule watch the disk");
    h.keys(b"\r"); // task
    h.keys(b"\r"); // every 24 hours
    h.keys(b"\r"); // independent
    h.keys(b"\r"); // tools run without asking
    h.answer_dry_run();
    // Use my default OFF (the first card): one dry run of what applies now.
    for _ in 0..12 {
        h.keys(b"\x1b[A");
    }
    h.keys(b"\r");
    let mut change = None;
    while let Ok(cmd) = h.rx.try_recv() {
        if let Cmd::Workspaces(WsCmd::RunChange { value, key, commit }) = cmd {
            change = Some((value, key, commit));
        }
    }
    let (value, key, commit) = change.expect("a run-level change");
    assert_eq!(key, "follow");
    assert!(matches!(commit, RunCommit::Draft));
    let value = value.expect("a payload (Use my default off)");
    assert_eq!(value.folders.len(), 3, "starts from what applies now");
    // The gateway accepted it: the lane's answer.
    h.store.workspaces.update(|w| {
        w.busy = None;
        w.draft = Some(value.clone());
    });
    h.turn();
    // Continue, then set a limit, then create.
    for _ in 0..30 {
        h.keys(b"\x1b[B");
    }
    let screen = h.keys(b"\r");
    assert!(screen.contains("6/6 Title and limits"), "{screen}");
    h.keys(b"\x1b[A"); // Stop at (UTC)
    h.keys(b"\x1b[A"); // Stop after this many runs
    h.keys(b"\r");
    h.term.push_input(b"3");
    h.turn();
    let screen = h.keys(b"\r");
    assert!(
        screen.contains("3 runs max"),
        "the preview reads the limit:\n{screen}"
    );
    h.keys(b"\r"); // Create automation (the cursor is back on it)
    let body = h
        .auto_cmds()
        .into_iter()
        .find_map(|c| match c {
            AutoCmd::Create { body } => Some(body),
            _ => None,
        })
        .expect("create");
    assert_eq!(
        body["trigger"]["config"],
        json!({"kind": "every", "every": "24h", "count": 3})
    );
    assert_eq!(body["target"]["input_data"]["workspace"], value.to_json());
    assert_eq!(
        body["target"]["input_data"]["workspace"]["posture"],
        json!("any_except_denied")
    );
}

#[test]
fn archive_from_one_automation_also_asks_first() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    let screen = h.keys(b"a");
    assert!(h.auto_cmds().is_empty(), "one press only asks");
    assert!(
        screen
            .contains("Archive “Inbox triage”? It will not run again; its history stays readable."),
        "{screen}"
    );
    h.keys(b"y");
    let cmds = h.auto_cmds();
    assert!(
        matches!(cmds.as_slice(), [AutoCmd::Command { id, command_type, .. }] if id == INBOX && command_type == "automation.archive"),
        "{cmds:?}"
    );
}

/// The rail's queued revisions (`Cmd::Rail(SaveRevision)`), other commands dropped.
fn saves(h: &mut Harness) -> Vec<(u64, serde_json::Value)> {
    let mut out = Vec::new();
    while let Ok(cmd) = h.rx.try_recv() {
        if let Cmd::Rail(abstractcode::gateway::rail::RailCmd::SaveRevision {
            expected_revision,
            changes,
            ..
        }) = cmd
        {
            out.push((expected_revision, changes));
        }
    }
    out
}

#[test]
fn edit_opens_the_settings_on_the_automation_and_a_refusal_stays_readable() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    let screen = h.keys(b"e");
    // The rail, bound to this automation, on its Workflow panel.
    assert!(screen.contains("Automation Inbox triage"), "{screen}");
    assert!(screen.contains("Revision 3"), "{screen}");
    assert!(
        screen.contains("Changes are saved as a new revision and apply from the next run."),
        "{screen}"
    );
    assert!(screen.contains("Repeat every (UTC)"), "{screen}");
    // Workflow, Title, Repeat every: Enter edits it; an invalid interval.
    h.keys(b"\x1b[B\x1b[B");
    h.keys(b"\r");
    h.keys(b"\x1b[F\x7f\x7f\x7f");
    h.term.push_input(b"6 hours");
    h.turn();
    let screen = h.keys(b"\r");
    assert!(saves(&mut h).is_empty(), "nothing is sent");
    assert!(
        screen.contains("Not saved: Interval must be a whole number of minutes, hours or days"),
        "{screen}"
    );
}

#[test]
fn a_settings_change_is_saved_as_a_new_revision() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    h.keys(b"e");
    h.keys(b"\x1b[B\x1b[B");
    h.keys(b"\r");
    h.keys(b"\x1b[F\x7f\x7f\x7f");
    h.term.push_input(b"6h");
    h.turn();
    let screen = h.keys(b"\r");
    let sent = saves(&mut h);
    match sent.as_slice() {
        [(rev, changes)] => {
            assert_eq!(*rev, 3, "expected_revision is the definition's");
            assert_eq!(changes["trigger"]["config"]["every"], "6h");
            assert!(
                changes.get("title").is_none() && changes.get("context").is_none(),
                "only what changed: {changes}"
            );
        }
        other => panic!("expected one revision, got {other:?}"),
    }
    assert!(screen.contains("Saving…"), "{screen}");
    // The gateway saved it: the panel says so, with the new number.
    h.store
        .rail
        .update(|r| r.save = abstractcode::rail::SaveState::Saved(4));
    let screen = h.turn();
    assert!(
        screen.contains("Saved as revision 4; applies from the next run."),
        "{screen}"
    );
}

#[test]
fn an_automation_workspaces_change_is_one_revision_after_the_dry_run() {
    use abstractcode::gateway::workspaces::{RunCommit, WsCmd};
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    h.store.automations.update(|v| {
        let def = v.detail.as_mut().unwrap().definition.as_mut().unwrap();
        def.target = json!({"bundle_ref": "inbox@1.0.0", "flow_id": "triage",
            "input_data": {"prompt": "triage", "workspace_access_mode": "workspace_or_allowed",
                           "workspace_allowed_paths": ["/old/list"]}});
    });
    h.keys(b"e");
    h.keys(b"5"); // the Workspace panel
                  // The panel asks for the dry run of the stored value (none = Use my default).
    let mut dry = false;
    while let Ok(cmd) = h.rx.try_recv() {
        if let Cmd::Workspaces(WsCmd::DryRun { value: None }) = cmd {
            dry = true;
        }
    }
    assert!(dry, "the chooser reads the gateway's dry run");
    h.answer_dry_run();
    let screen = h.turn();
    assert!(
        screen.contains("Runs work in the automation folder"),
        "{screen}"
    );
    assert!(screen.contains("[x] Use my default"), "{screen}");
    assert!(
        !screen.contains("Access mode"),
        "the R9 access mode is gone:\n{screen}"
    );
    // Use my default OFF: dry run first, then ONE revision carrying the payload.
    h.keys(b"\r");
    let mut change = None;
    while let Ok(cmd) = h.rx.try_recv() {
        if let Cmd::Workspaces(WsCmd::RunChange { value, commit, .. }) = cmd {
            change = Some((value, commit));
        }
    }
    match change {
        Some((
            Some(value),
            RunCommit::Revision {
                id,
                expected_revision,
                changes,
                ..
            },
        )) => {
            assert_eq!(id, INBOX);
            assert_eq!(expected_revision, 3);
            let input = &changes["target"]["input_data"];
            assert_eq!(input["workspace"], value.to_json());
            assert!(input.get("workspace_access_mode").is_none(), "{input}");
            assert!(
                input.get("workspace_allowed_paths").is_none(),
                "a payload replaces the R9 list: {input}"
            );
            assert_eq!(input["prompt"], json!("triage"), "everything else kept");
        }
        other => panic!("expected one run-level revision, got {other:?}"),
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

#[test]
fn run_now_says_the_shared_one_line_on_both_screens() {
    // Operator 2026-09-28: the same "Run now" explanation in every client; the
    // terminal shows the kit's one line under the key hints.
    let line = "g run now: Run it once now, without waiting for the schedule; the next scheduled run keeps its time.";
    assert_eq!(auto::run_now_key_line(), line);
    let mut h = harness();
    h.command("/automations");
    h.answer_list();
    let screen = h.turn();
    assert!(screen.contains(line), "list: {screen}");
    let mut h = harness();
    let screen = open_inbox(&mut h);
    assert!(screen.contains(line), "one automation: {screen}");
}

// ---------------------------------------------------------------------------
// Round 16 (R16.1): calendar rules in /schedule's When step
// ---------------------------------------------------------------------------

fn served_preview(trigger: &Value, sentence: &str) -> auto::PreviewState {
    auto::PreviewState::Ready(
        auto::parse_schedule_preview(&json!({
            "trigger": trigger,
            "time_zone": "Europe/Paris",
            "schedule_rule_text": "served rule",
            "schedule_text": "served rule · next Fri 9 Oct 08:00",
            "next_run_at": "2026-10-09T06:00:00+00:00",
            "next_run_local": "2026-10-09T08:00:00+02:00",
            "first_run_sentence": sentence,
        }))
        .unwrap(),
    )
}

fn preview_asked(h: &mut Harness) -> Value {
    h.auto_cmds()
        .into_iter()
        .find_map(|c| match c {
            AutoCmd::Preview { trigger } => Some(trigger),
            _ => None,
        })
        .expect("the When step asks the gateway's schedule-preview")
}

/// From the When picker (cursor on "every 24 hours", row 4) to row `ix`.
fn pick_when(h: &mut Harness, ix: usize) -> String {
    h.command("/schedule brief me");
    h.keys(b"\r"); // the task
    for _ in 4..ix {
        h.keys(b"\x1b[B");
    }
    h.keys(b"\r")
}

#[test]
fn schedule_daily_shows_the_gateways_words_and_writes_schedule_v2() {
    let mut h = harness();
    let screen = pick_when(&mut h, 7);
    assert!(screen.contains("2/6 Daily at"), "{screen}");
    assert!(screen.contains("Time of day: HH:MM"), "{screen}");
    let screen = h.keys(b"\r"); // 08:00 (the default)
    let trigger = preview_asked(&mut h);
    assert_eq!(
        trigger,
        json!({"source_id": "schedule", "source_version": 2, "config": {"kind": "daily", "at": "08:00"}})
    );
    assert!(screen.contains("Checking the schedule…"), "{screen}");
    // Not ready yet: Enter waits (it never invents a sentence).
    h.keys(b"\r");
    assert!(h.turn().contains("2/6 when · Daily"));
    let sentence = "Runs every day at 08:00 (Europe/Paris), first run Fri 9 Oct 08:00.";
    h.store
        .automations
        .update(|v| v.apply_preview(&trigger, served_preview(&trigger, sentence)));
    let screen = h.turn();
    assert!(
        screen.contains("in Europe/Paris (your account's time zone)"),
        "{screen}"
    );
    assert!(
        screen.contains(sentence),
        "the served sentence verbatim:\n{screen}"
    );
    let screen = h.keys(b"\r");
    assert!(
        screen.contains("3/6 context · Runs every day at 08:00 (Europe/Paris)"),
        "{screen}"
    );
    h.keys(b"\r"); // independent
    h.keys(b"\r"); // tools run without asking
    let screen = h.keys(b"\r"); // Workspaces: Continue
    assert!(screen.contains("6/6 Title and limits"), "{screen}");
    assert!(screen.contains(sentence), "{screen}");
    assert!(screen.contains("Stop after this many runs"), "{screen}");
    assert!(
        !screen.contains("First run at"),
        "a calendar rule has no first-run time:\n{screen}"
    );
    h.keys(b"\r"); // Create automation
    let body = h
        .auto_cmds()
        .into_iter()
        .find_map(|c| match c {
            AutoCmd::Create { body } => Some(body),
            _ => None,
        })
        .expect("create");
    assert_eq!(body["trigger"], trigger);
}

#[test]
fn schedule_weekly_days_are_state_showing_toggles() {
    let mut h = harness();
    let screen = pick_when(&mut h, 8);
    assert!(screen.contains("2/6 Weekly · On"), "{screen}");
    assert!(
        screen.contains("[x] Mon") && screen.contains("[ ] Tue"),
        "{screen}"
    );
    h.keys(b"\x1b[B");
    let screen = h.keys(b"\r"); // Tue on
    assert!(
        screen.contains("[x] Mon") && screen.contains("[x] Tue"),
        "{screen}"
    );
    let screen = h.keys(b"\r"); // Mon off (the cursor starts on Mon again)
    assert!(
        screen.contains("[ ] Mon") && screen.contains("[x] Tue"),
        "{screen}"
    );
    for _ in 0..7 {
        h.keys(b"\x1b[B");
    }
    let screen = h.keys(b"\r"); // Continue — at HH:MM
    assert!(screen.contains("2/6 Weekly at"), "{screen}");
    h.keys(b"\r");
    assert_eq!(
        preview_asked(&mut h)["config"],
        json!({"kind": "weekly", "days": ["tue"], "at": "08:00"})
    );
}

#[test]
fn schedule_monthly_last_day_and_once_send_their_rules() {
    let mut h = harness();
    let screen = pick_when(&mut h, 9);
    assert!(screen.contains("2/6 Monthly · on day"), "{screen}");
    for _ in 0..31 {
        h.keys(b"\x1b[B");
    }
    h.keys(b"\r"); // last
    h.keys(b"\r"); // 08:00
    assert_eq!(
        preview_asked(&mut h)["config"],
        json!({"kind": "monthly", "day": "last", "at": "08:00"})
    );

    let mut h = harness();
    let screen = pick_when(&mut h, 10);
    assert!(screen.contains("2/6 Run once at"), "{screen}");
    h.term.push_input(b"2026-10-09 10:00");
    h.turn();
    h.keys(b"\r");
    assert_eq!(
        preview_asked(&mut h)["config"],
        json!({"kind": "once", "at": "2026-10-09T10:00"})
    );
}

#[test]
fn a_refused_rule_shows_the_gateways_sentence_and_does_not_continue() {
    let mut h = harness();
    pick_when(&mut h, 7);
    h.keys(b"\r");
    let trigger = preview_asked(&mut h);
    let refusal = "The automation definition is not valid. Unknown time zone 'Mars/Olympus'. (field trigger.config.time_zone)";
    h.store
        .automations
        .update(|v| v.apply_preview(&trigger, auto::PreviewState::Failed(refusal.into())));
    let screen = h.turn();
    assert!(flat(&screen).contains(refusal), "{screen}");
    let screen = h.keys(b"\r");
    assert!(!screen.contains("3/6 context"), "{screen}");
}

/// The screen as one line (wrapped text joined, box glyphs and runs of blanks collapsed).
fn flat(screen: &str) -> String {
    screen
        .lines()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '█' || c == '│'))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every command sent since the last drain: (schedule-preview triggers, revisions).
fn previews_and_saves(h: &mut Harness) -> (Vec<Value>, Vec<(u64, Value)>) {
    let (mut previews, mut saved) = (Vec::new(), Vec::new());
    while let Ok(cmd) = h.rx.try_recv() {
        match cmd {
            Cmd::Automations(AutoCmd::Preview { trigger }) => previews.push(trigger),
            Cmd::Rail(abstractcode::gateway::rail::RailCmd::SaveRevision {
                expected_revision,
                changes,
                ..
            }) => saved.push((expected_revision, changes)),
            _ => {}
        }
    }
    (previews, saved)
}

/// The Edit panels on the schedule@2 daily fixture row ("Morning briefing").
fn edit_morning_briefing(h: &mut Harness) -> auto::Summary {
    let list = auto::parse_list_page(&fixture("list.json")).unwrap();
    let summary = list
        .items
        .iter()
        .find(|s| s.title == "Morning briefing")
        .unwrap()
        .clone();
    h.command("/automations");
    h.answer_list();
    for _ in 0..3 {
        h.keys(b"\x1b[B");
    }
    h.keys(b"\r");
    let definition = auto::Definition {
        revision: 5,
        workflow_id: "brief@1.0.0:agent".into(),
        tool_approval: "auto".into(),
        growing: Default::default(),
        max_attempts: Some(3),
        workspace_root: summary.workspace_root.clone().unwrap(),
        target: Value::Null,
    };
    let s2 = summary.clone();
    h.store.automations.update(|v| {
        v.apply_detail(
            &s2.id,
            definition,
            s2.clone(),
            auto::Page {
                items: vec![],
                next_cursor: None,
            },
        )
    });
    h.turn();
    h.keys(b"e");
    summary
}

#[test]
fn edit_a_calendar_rule_keeps_its_time_zone_and_shows_the_gateways_line() {
    let mut h = harness();
    edit_morning_briefing(&mut h);
    let (previews, _) = previews_and_saves(&mut h);
    let trigger = previews
        .last()
        .cloned()
        .expect("the panel asks the gateway to word the stored rule");
    assert_eq!(
        trigger,
        json!({"source_id": "schedule", "source_version": 2,
               "config": {"kind": "daily", "at": "08:00", "time_zone": "Europe/Paris"}})
    );
    let screen = h.turn();
    assert!(
        screen.contains("When") && screen.contains("Daily"),
        "{screen}"
    );
    assert!(
        screen.contains("Time of day") && screen.contains("08:00"),
        "{screen}"
    );
    assert!(screen.contains("Checking the schedule…"), "{screen}");
    let sentence = "Runs every day at 08:00 (Europe/Paris), first run Mon 28 Sep 08:00.";
    h.store.automations.update(|v| {
        v.apply_preview(
            &trigger,
            auto::PreviewState::Ready(
                auto::parse_schedule_preview(&json!({
                    "trigger": trigger, "time_zone": "Europe/Paris",
                    "schedule_rule_text": "Every day at 08:00 (Europe/Paris)",
                    "schedule_text": "Every day at 08:00 (Europe/Paris) · next Mon 28 Sep 08:00",
                    "next_run_at": "2026-09-28T06:00:00+00:00",
                    "next_run_local": "2026-09-28T08:00:00+02:00",
                    "first_run_sentence": sentence
                }))
                .unwrap(),
            ),
        )
    });
    let screen = h.turn();
    assert!(
        screen.contains("in Europe/Paris (this automation's time zone)"),
        "{screen}"
    );
    assert!(screen.contains(sentence), "{screen}");
    // When (row 3: Workflow, Title, When) → Weekly: ONE revision with the
    // rule and the binding's own zone.
    h.keys(b"\x1b[B\x1b[B");
    let screen = h.keys(b"\r");
    assert!(
        screen.contains("Weekly") && screen.contains("Monthly"),
        "{screen}"
    );
    h.keys(b"\x1b[B");
    h.keys(b"\r");
    let (_, saved) = previews_and_saves(&mut h);
    match saved.as_slice() {
        [(rev, changes)] => {
            assert_eq!(*rev, 5);
            assert_eq!(
                changes,
                &json!({"trigger": {"source_id": "schedule", "source_version": 2,
                    "config": {"kind": "weekly", "days": ["mon"], "at": "08:00", "time_zone": "Europe/Paris"}}})
            );
        }
        other => panic!("expected one revision, got {other:?}"),
    }
}

#[test]
fn edit_a_calendar_time_sends_only_a_changed_rule() {
    let mut h = harness();
    edit_morning_briefing(&mut h);
    previews_and_saves(&mut h);
    // Time of day (row 4).
    h.keys(b"\x1b[B\x1b[B\x1b[B");
    let screen = h.keys(b"\r");
    assert!(screen.contains("Time of day"), "{screen}");
    // The same time: nothing is sent.
    h.keys(b"\r");
    let (_, saved) = previews_and_saves(&mut h);
    assert!(saved.is_empty(), "{saved:?}");
    // 07:30: the rule changes, the zone stays (the panel reopens on the same row).
    h.keys(b"\r");
    h.keys(b"\x1b[F\x7f\x7f\x7f\x7f\x7f");
    h.term.push_input(b"07:30");
    h.turn();
    h.keys(b"\r");
    let (_, saved) = previews_and_saves(&mut h);
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert_eq!(
        saved[0].1["trigger"]["config"],
        json!({"kind": "daily", "at": "07:30", "time_zone": "Europe/Paris"})
    );
    // A malformed time is refused with the kit's sentence, nothing sent.
    h.keys(b"\r");
    h.keys(b"\x1b[F\x7f\x7f\x7f\x7f\x7f");
    h.term.push_input(b"7h30");
    h.turn();
    let screen = h.keys(b"\r");
    let (_, saved) = previews_and_saves(&mut h);
    assert!(saved.is_empty(), "{saved:?}");
    assert!(
        flat(&screen).contains("Pick the time of day (HH:MM)."),
        "{screen}"
    );
}
