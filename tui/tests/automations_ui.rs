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
    harness_sized(Size::new(140, 44))
}

fn harness_sized(size: Size) -> Harness {
    abstracttui::app::set_theme_by_id("abstract-dark");
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
            notify: serde_json::Value::Null,
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
        screen.contains("↻ Every 30 minutes (UTC) · waiting since"),
        "{screen}"
    );
    // Every schedule row reads the gateway's words, bounds included.
    assert!(screen.contains("↻ Every 8 hours (UTC) · last"), "{screen}");
    assert!(
        screen.contains("↻ Every 7 days (UTC) · 12 runs max · last"),
        "{screen}"
    );
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
    assert!(
        screen.contains("Every 30 minutes (UTC) · waiting since"),
        "{screen}"
    );
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

const SCHEDULE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/schedule");

fn schedule_fixture(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(format!("{SCHEDULE}/{name}")).unwrap()).unwrap()
}

impl Harness {
    /// Answer `/schedule`'s Prepare as the lane would: the email status
    /// (`me_email_*.json`), the executable workflows, and the conversation
    /// workflow's schema (basic-agent's recorded schema).
    fn answer_prepare(&mut self, email: &str) {
        let status =
            abstractcode::automation_email::EmailStatus::parse(&schedule_fixture(email)).ok();
        let exec = abstractcode::workflow_picker::parse(
            &schedule_fixture("bundles_executable_code_agent.json"),
            auto::CODE_AGENT_INTERFACE,
        );
        let schema = abstractcode::schedule_input::normalize_input_schema(&schedule_fixture(
            "input_schema_basic_agent.json",
        ))
        .unwrap();
        self.store.automations.update(|v| {
            v.email = status;
            v.executable = Some(exec);
            v.put_schema(
                auto::schema_key("basic-agent", "9.9.9", "agent"),
                Ok(schema),
            );
        });
        self.turn();
    }

    fn created(&mut self) -> Value {
        self.auto_cmds()
            .into_iter()
            .find_map(|c| match c {
                AutoCmd::Create { body } => Some(body),
                _ => None,
            })
            .expect("create")
    }

    fn up(&mut self, n: usize) -> String {
        let mut screen = String::new();
        for _ in 0..n {
            screen = self.keys(b"\x1b[A");
        }
        screen
    }

    fn down(&mut self, n: usize) -> String {
        let mut screen = String::new();
        for _ in 0..n {
            screen = self.keys(b"\x1b[B");
        }
        screen
    }

    /// End (the cursor to Continue — a change keeps it on the row changed),
    /// then Enter.
    fn cont(&mut self) -> String {
        self.keys(b"\x1b[F");
        self.keys(b"\r")
    }
}

const BASIC_TOOLS: [&str; 9] = [
    "edit_file",
    "analyze_code",
    "execute_command",
    "fetch_url",
    "list_files",
    "read_file",
    "search_files",
    "web_search",
    "write_file",
];

#[test]
fn schedule_creates_the_shared_definition_from_the_current_workflow() {
    let mut h = harness();
    let screen = h.command("/schedule");
    let cmds = h.auto_cmds();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, AutoCmd::Prepare { schema: Some((b, v, f)) }
            if b == "basic-agent" && v == "9.9.9" && f == "agent")),
        "the dialog reads the email status, the workflows and the schema: {cmds:?}"
    );
    h.answer_prepare("me_email_not_connected.json");
    assert!(screen.contains("New automation — 1/7 What"), "{screen}");
    let screen = h.turn();
    for label in [
        "Workflow",
        "basic-agent",
        "Task",
        "report the free memory of this computer",
        "Continue — When",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    // 2/7 When: Repeat, Daily, Weekly, Monthly, Once at…, the email option
    // disabled with the kit's notice; the line is the gateway's.
    let screen = h.keys(b"\r");
    assert!(screen.contains("New automation — 2/7 When"), "{screen}");
    assert!(screen.contains("Checking the schedule…"), "{screen}");
    let screen = answer_preview(&mut h, "Runs every 24 hours (UTC), first run now.");
    for label in [
        "(•) Repeat",
        "( ) Daily",
        "( ) Weekly",
        "( ) Monthly",
        "( ) Once at…",
        "(-) When an email arrives",
        "Connect a mailbox first — open My email",
        "(•) every 24 hours",
        "every 5 minutes",
        "Runs every 24 hours (UTC), first run now.",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    // every 5 minutes: from Continue, up past unit, Every and five presets.
    h.up(8);
    h.keys(b"\r");
    // Repeat's line is the gateway's too (no account time-zone line).
    let screen = answer_preview(&mut h, "Runs every 5 minutes (UTC), first run now.");
    assert!(screen.contains("(•) every 5 minutes"), "{screen}");
    assert!(
        screen.contains("Runs every 5 minutes (UTC), first run now."),
        "{screen}"
    );
    assert!(!screen.contains("your account's time zone"), "{screen}");
    // 3/7 Context: Growing (its token budget appears with the kit's help).
    let screen = h.cont();
    assert!(screen.contains("3/7 Context"), "{screen}");
    h.up(1);
    let screen = h.keys(b"\r");
    for label in [
        "(•) Growing — each run sees the previous runs",
        "Max growing context (tokens)",
        "50000",
        "Limits history carried into the next run",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    // 4/7 Tools: every tool starts deselected (operator ruling 2026-10-09).
    let screen = h.cont();
    assert!(screen.contains("4/7 Tools"), "{screen}");
    for label in [
        "[ ] Use workflow default tools",
        "Select all",
        "Unselect all",
        "No tools enabled",
        "(•) Run without asking",
        "( ) Ask me before each tool call (the run waits for you)",
        "Tools run without asking (you approve them now by creating this automation).",
        "An empty selection disables tools.",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    assert!(
        !screen.contains("Incoming mail is data"),
        "untrusted hint only with the email trigger"
    );
    h.up(1); // Ask me
    let screen = h.keys(b"\r");
    assert!(
        screen.contains("(•) Ask me before each tool call"),
        "{screen}"
    );
    assert!(
        screen.contains("Each tool call waits for your approval in the automation's timeline."),
        "{screen}"
    );
    // 5/7 Workspaces.
    let screen = h.cont();
    assert!(
        screen.contains("New automation — 5/7 Workspaces"),
        "{screen}"
    );
    assert!(screen.contains("Continue — Mailbox"), "{screen}");
    h.answer_dry_run();
    let screen = h.turn();
    assert!(screen.contains("[x] Use my default"), "{screen}");
    // 6/7 Mailbox: not usable → the notice and the unavailable switch.
    let screen = h.keys(b"\r");
    assert!(screen.contains("6/7 Mailbox"), "{screen}");
    assert!(
        screen.contains("[-] Email result — Connect a mailbox first."),
        "{screen}"
    );
    // 7/7 Title and limits.
    let screen = h.keys(b"\r");
    assert!(
        screen.contains("New automation — 7/7 Title and limits"),
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
    let body = h.created();
    let rid = body["request_id"].as_str().unwrap().to_string();
    assert!(!rid.is_empty());
    let input = &body["target"]["input_data"];
    assert_eq!(body["target"]["bundle_ref"], json!("basic-agent@9.9.9"));
    assert_eq!(body["target"]["flow_id"], json!("agent"));
    assert_eq!(
        input["prompt"],
        json!("report the free memory of this computer")
    );
    assert_eq!(
        input["context"],
        json!({"task": "report the free memory of this computer"})
    );
    assert_eq!(input["use_session_history"], json!(true));
    assert_eq!(input["use_context"], json!(false));
    assert_eq!(input["max_iterations"], json!(20), "a schema default");
    // Nothing selected: an empty selection disables tools (both fields).
    assert_eq!(input["tools"], json!([]));
    assert_eq!(input["_runtime"]["allowed_tools"], json!([]));
    assert!(input.get("workspace_root").is_none());
    assert_eq!(
        body["title"],
        json!("report the free memory of this computer")
    );
    assert_eq!(
        body["trigger"],
        json!({"source_id": "schedule", "source_version": 2, "config": {"kind": "every", "every": "5m"}})
    );
    assert_eq!(body["context"], json!({"mode": "growing"}));
    assert_eq!(body["policy"], json!({"tool_approval": "ask"}));
    assert!(
        body.get("notify").is_none(),
        "nothing email-shaped without a usable mailbox"
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
    h.answer_prepare("me_email_not_connected.json");
    for _ in 0..6 {
        h.keys(b"\r"); // What, When, Context, Tools, Workspaces, Mailbox
    }
    h.keys(b"\r"); // Title and limits: Create automation
    let body = h.created();
    assert_eq!(body["target"]["flow_id"], json!("@default"));
    assert_eq!(body["target"]["interface"], json!("abstractcode.agent.v1"));
    assert_eq!(
        body["target"]["input_data"]["prompt"],
        json!("watch the disk")
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
    h.answer_prepare("me_email_not_connected.json");
    h.keys(b"\r"); // What
                   // When: every 24 hours, worded by the gateway.
    answer_preview(&mut h, "Runs every 24 hours (UTC), first run now.");
    h.keys(b"\r"); // When: Continue
    h.keys(b"\r"); // independent
    h.keys(b"\r"); // tools
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
    // Continue, Mailbox, then set a limit, then create.
    for _ in 0..30 {
        h.keys(b"\x1b[B");
    }
    let screen = h.keys(b"\r");
    assert!(screen.contains("6/7 Mailbox"), "{screen}");
    let screen = h.keys(b"\r");
    assert!(screen.contains("7/7 Title and limits"), "{screen}");
    h.keys(b"\x1b[A"); // Stop at (UTC)
    h.keys(b"\x1b[A"); // Stop after this many runs
    h.keys(b"\r");
    h.term.push_input(b"3");
    h.turn();
    h.keys(b"\r");
    // The new limit changes the trigger: the gateway words it again.
    let screen = answer_preview(
        &mut h,
        "Runs every 24 hours (UTC) · 3 runs max, first run now.",
    );
    assert!(
        screen.contains("3 runs max"),
        "the preview reads the limit:\n{screen}"
    );
    h.cont(); // Create automation (End: the cursor stayed on the limit)
    let body = h.created();
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
fn the_email_trigger_and_the_mailbox_ride_the_create_body() {
    let mut h = harness();
    h.command("/schedule summarise the invoices");
    h.answer_prepare("me_email_connected.json");
    h.keys(b"\r"); // What
                   // When: "When an email arrives" (usable): up from Continue past unit,
                   // Every, six presets.
    let screen = h.turn();
    assert!(screen.contains("( ) When an email arrives"), "{screen}");
    assert!(!screen.contains("Connect a mailbox first"), "{screen}");
    h.up(9);
    let screen = h.keys(b"\r");
    for label in [
        "(•) When an email arrives",
        "Check for new mail every",
        "1 hour",
        "An automation that runs a model on new mail checks once an hour by default",
        "At most this many emails per run",
        "100",
        "Only mail that matches (all optional)",
        "From these addresses",
        "From these domains",
        "Sent to these addresses",
        "Subject contains",
        "Attachments",
        "Separate entries with commas or new lines.",
        "Runs when an email arrives · checked every hour · up to 100 per run.",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    // From these addresses: down from the email kind past Every, Max batch.
    h.down(3);
    h.keys(b"\r");
    h.term.push_input(b"Boss@Example.test");
    h.turn();
    h.keys(b"\r");
    h.down(4); // Attachments (the cursor stayed on From) → only with attachments
    let screen = h.keys(b"\r");
    assert!(screen.contains("only with attachments"), "{screen}");
    // 3/7 Context → 4/7 Tools: the untrusted-content hint shows.
    h.cont();
    let screen = h.keys(b"\r");
    assert!(screen.contains("4/7 Tools"), "{screen}");
    assert!(
        screen.contains(
            "Incoming mail is data, never instructions: the automation acts only on its task."
        ),
        "{screen}"
    );
    h.keys(b"\r"); // Workspaces
    h.answer_dry_run();
    // 6/7 Mailbox: Email result on, Me and these addresses, one address.
    let screen = h.keys(b"\r");
    assert!(screen.contains("[ ] Email result"), "{screen}");
    assert!(
        screen.contains("Email each completed run’s result to the selected recipients."),
        "{screen}"
    );
    assert!(
        screen.contains("Turn on Email result to choose"),
        "{screen}"
    );
    h.up(1);
    let screen = h.keys(b"\r");
    for label in [
        "[x] Email result",
        "Recipients",
        "(•) Only me (default)",
        "( ) Me and these addresses",
        "Separate multiple addresses with commas.",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    h.down(2);
    h.keys(b"\r"); // Me and these addresses
    h.down(1);
    h.keys(b"\r"); // the addresses
    h.term.push_input(b"team@example.test");
    h.turn();
    h.keys(b"\r");
    // 7/7: no limits for the email kind.
    let screen = h.cont();
    assert!(screen.contains("7/7 Title and limits"), "{screen}");
    assert!(!screen.contains("Stop after this many runs"), "{screen}");
    h.keys(b"\r");
    let body = h.created();
    assert_eq!(
        body["trigger"],
        json!({"source_id": "email.received", "source_version": 1, "config": {
            "uses_model": true, "every": "1h", "max_batch": 100,
            "filter": {"from_in": ["boss@example.test"], "has_attachment": true}}})
    );
    assert_eq!(
        body["notify"],
        json!({"channels": ["console", "email"], "recipients": ["self", "team@example.test"]})
    );
}

#[test]
fn an_unusable_mailbox_keeps_the_email_option_disabled() {
    let mut h = harness();
    h.command("/schedule watch the disk");
    h.answer_prepare("me_email_admin_disabled.json");
    let screen = h.keys(b"\r");
    assert!(
        screen.contains("Connect a mailbox first — open My email (Your admin turned mailboxes off for your account.)"),
        "{screen}"
    );
    h.up(9);
    let screen = h.keys(b"\r");
    assert!(
        screen.contains("(•) Repeat"),
        "choosing the disabled option changes nothing:\n{screen}"
    );
    assert!(!screen.contains("Check for new mail every"), "{screen}");
}

#[test]
fn a_missing_required_input_stops_the_create_with_the_validators_sentence() {
    let mut h = harness();
    h.command("/schedule watch the disk");
    h.answer_prepare("me_email_not_connected.json");
    // The conversation's workflow requires a pin nobody sets.
    let schema = json!({"properties": {"topic": {"type": "string", "title": "Topic"}, "prompt": {"type": "string"}}, "required": ["topic"]});
    h.store.automations.update(|v| {
        v.put_schema(
            auto::schema_key("basic-agent", "9.9.9", "agent"),
            Ok(schema),
        )
    });
    for _ in 0..6 {
        h.keys(b"\r");
    }
    let screen = h.keys(b"\r");
    assert!(screen.contains("Topic is required."), "{screen}");
    assert!(
        !h.auto_cmds()
            .iter()
            .any(|c| matches!(c, AutoCmd::Create { .. })),
        "no POST when the inputs are invalid"
    );
}

#[test]
fn the_workflow_picker_lists_the_executable_set_with_the_gateway_default_first() {
    let mut h = harness();
    h.command("/schedule check the build");
    h.answer_prepare("me_email_not_connected.json");
    h.up(2); // Workflow
    let screen = h.keys(b"\r");
    assert!(
        screen.contains("Gateway default — Basic agent @0.0.5"),
        "{screen}"
    );
    assert!(screen.contains("ReAct agent  @0.1.0 · Shared"), "{screen}");
    let gd = screen.find("Gateway default —").unwrap();
    let basic = screen.find("Basic agent  @0.0.5").unwrap();
    assert!(gd < basic, "Gateway default first:\n{screen}");
    // Pick ReAct agent (type-ahead is not offered: walk to it).
    let rows = abstractcode::ui::schedule_view::picker_labels(
        &abstractcode::workflow_picker::parse(
            &schedule_fixture("bundles_executable_code_agent.json"),
            auto::CODE_AGENT_INTERFACE,
        )
        .unwrap(),
    );
    let react = rows
        .iter()
        .position(|r| r.starts_with("ReAct agent"))
        .unwrap();
    // The picker starts on Gateway default (the conversation's workflow is
    // not in the executable set).
    for _ in 0..react {
        h.keys(b"\x1b[B");
    }
    let screen = h.keys(b"\r");
    assert!(screen.contains("ReAct agent @0.1.0"), "{screen}");
    let cmds = h.auto_cmds();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, AutoCmd::Schema { bundle, version, flow }
            if bundle == "react-agent" && version == "0.1.0" && flow == "react")),
        "{cmds:?}"
    );
    let schema = abstractcode::schedule_input::normalize_input_schema(&schedule_fixture(
        "input_schema_react_agent.json",
    ))
    .unwrap();
    h.store.automations.update(|v| {
        v.put_schema(
            auto::schema_key("react-agent", "0.1.0", "react"),
            Ok(schema),
        )
    });
    h.cont(); // What → When (the cursor stayed on Workflow)
    h.keys(b"\r"); // When → Context
    let screen = h.keys(b"\r"); // Context → Tools
    assert!(
        screen.contains("[ ] Use workflow default tools") && screen.contains("No tools enabled"),
        "a picked workflow starts with no tool too:\n{screen}"
    );
    h.keys(b"\r"); // Tools → Workspaces
    h.keys(b"\r"); // Workspaces → Mailbox
    h.keys(b"\r"); // Mailbox → Title and limits
    h.keys(b"\r"); // Create
    let body = h.created();
    assert_eq!(body["target"]["bundle_ref"], json!("react-agent@0.1.0"));
    assert_eq!(body["target"]["flow_id"], json!("react"));
    assert_eq!(body["target"]["input_data"]["tools"], json!([]));
    assert_eq!(
        body["target"]["input_data"]["prompt"],
        json!("check the build")
    );
}

// ---------------------------------------------------------------------------
// Operator feedback 2026-10-09 (autofix): the Tools step starts with every
// tool deselected, Select all / Unselect all, a tri-state box per category,
// a scrolling list (wheel, page keys), a click toggles a line, Space keeps
// the focus on the line toggled; the task is a multiline text; /automation.
// ---------------------------------------------------------------------------

fn tool(name: &str, toolset: &str) -> abstractcode::store::ToolInfo {
    abstractcode::store::ToolInfo {
        name: name.into(),
        toolset: toolset.into(),
        description: format!("{name} tool"),
        ..Default::default()
    }
}

/// files: read_file, write_file · web: web_search, fetch_url · system:
/// execute_command (disabled on this gateway).
fn small_inventory() -> Vec<abstractcode::store::ToolInfo> {
    let mut gated = tool("execute_command", "system");
    gated.served_disabled = true;
    vec![
        tool("read_file", "files"),
        tool("write_file", "files"),
        tool("web_search", "web"),
        tool("fetch_url", "web"),
        gated,
    ]
}

/// `/automation watch the disk` → What, When, Context → 4/7 Tools.
fn open_tools_step(h: &mut Harness, tools: Vec<abstractcode::store::ToolInfo>) -> String {
    h.store.tools.set(tools);
    h.command("/automation watch the disk");
    h.answer_prepare("me_email_not_connected.json");
    h.keys(b"\r");
    h.keys(b"\r");
    let screen = h.keys(b"\r");
    assert!(screen.contains("New automation — 4/7 Tools"), "{screen}");
    screen
}

/// The line the cursor marker `▸` is on.
fn focused_line(screen: &str) -> String {
    screen
        .lines()
        .find(|l| l.contains('▸'))
        .unwrap_or_default()
        .to_string()
}

/// The screen line holding `needle`, as (row, column of the needle), 0-based.
fn locate(screen: &str, needle: &str) -> (u16, u16) {
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not on screen:\n{screen}"));
    let col = line[..line.find(needle).unwrap()].chars().count();
    (row as u16, col as u16)
}

impl Harness {
    /// A bare Esc (the 30 ms disambiguation deadline, then dispatch).
    fn esc(&mut self) -> String {
        self.term.push_input(&[0x1b]);
        self.turn();
        std::thread::sleep(std::time::Duration::from_millis(45));
        self.turn();
        self.turn()
    }

    /// SGR left click (press + release) at a 0-based cell.
    fn click(&mut self, row: u16, col: u16) -> String {
        self.term
            .push_input(format!("\x1b[<0;{};{}M", col + 1, row + 1).as_bytes());
        self.turn();
        self.term
            .push_input(format!("\x1b[<0;{};{}m", col + 1, row + 1).as_bytes());
        self.turn();
        self.turn()
    }

    /// SGR wheel at a 0-based cell (`up` = away from you).
    fn wheel(&mut self, row: u16, col: u16, up: bool) -> String {
        let b = if up { 64 } else { 65 };
        self.term
            .push_input(format!("\x1b[<{b};{};{}M", col + 1, row + 1).as_bytes());
        self.turn();
        self.turn()
    }

    fn click_on(&mut self, needle: &str) -> String {
        let screen = self.turn();
        let (row, col) = locate(&screen, needle);
        self.click(row, col + 1)
    }
}

/// Continue through Tools, Workspaces, Mailbox, then Create (each step's
/// first visit puts the cursor on its Continue).
fn finish_from_tools(h: &mut Harness) -> Value {
    h.cont(); // Tools → Workspaces
    h.keys(b"\r"); // Workspaces → Mailbox
    h.keys(b"\r"); // Mailbox → Title and limits
    h.keys(b"\r"); // Create automation
    h.created()
}

#[test]
fn the_tools_step_starts_with_every_tool_deselected() {
    let mut h = harness();
    // The conversation customised its tools: the automation still starts
    // from none (operator ruling 2026-10-09).
    h.store.disabled_tools.set(vec!["write_file".into()]);
    let screen = open_tools_step(&mut h, small_inventory());
    for label in [
        "[ ] Use workflow default tools",
        "Select all",
        "Unselect all",
        "[ ] files",
        "[ ] read_file  read_file tool",
        "[ ] write_file",
        "[ ] web",
        "[ ] web_search",
        "[-] system",
        "[-] execute_command — disabled on this gateway",
        "No tools enabled",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    assert!(!screen.contains("[x] "), "nothing selected:\n{screen}");
    let body = finish_from_tools(&mut h);
    assert_eq!(body["target"]["input_data"]["tools"], json!([]));
    assert_eq!(
        body["target"]["input_data"]["_runtime"]["allowed_tools"],
        json!([])
    );
}

#[test]
fn space_keeps_the_focus_on_the_line_toggled() {
    let mut h = harness();
    open_tools_step(&mut h, small_inventory());
    // Items: Use workflow default (0), Select all (1), Unselect all (2),
    // files (3), read_file (4), write_file (5), web (6), web_search (7),
    // fetch_url (8), system (9), execute_command (10), Run without asking
    // (11), Ask me (12), Continue (13).
    h.keys(b"\x1b[H"); // Home
    for (n, name) in [(4usize, "read_file"), (5, "write_file"), (7, "web_search")] {
        h.keys(b"\x1b[H");
        h.down(n);
        assert!(focused_line(&h.turn()).contains(name), "{name}");
        let screen = h.keys(b" ");
        assert!(
            screen.contains(&format!("[x] {name}")),
            "Space toggles {name}:\n{screen}"
        );
        let line = focused_line(&screen);
        assert!(
            line.contains(name),
            "the focus stays on line {n} ({name}), not on Continue: {line:?}\n{screen}"
        );
        // A second Space on the same line toggles the same tool back.
        let screen = h.keys(b" ");
        assert!(screen.contains(&format!("[ ] {name}")), "{screen}");
        assert!(focused_line(&screen).contains(name), "{screen}");
    }
    // Enter toggles too, and keeps the focus.
    h.keys(b"\x1b[H");
    h.down(8);
    let screen = h.keys(b"\r");
    assert!(screen.contains("[x] fetch_url"), "{screen}");
    assert!(focused_line(&screen).contains("fetch_url"), "{screen}");
    let body = finish_from_tools(&mut h);
    assert_eq!(body["target"]["input_data"]["tools"], json!(["fetch_url"]));
}

#[test]
fn select_all_and_unselect_all_change_every_grantable_tool() {
    let mut h = harness();
    open_tools_step(&mut h, small_inventory());
    h.keys(b"\x1b[H");
    h.down(1); // Select all
    let screen = h.keys(b" ");
    for label in [
        "[x] files",
        "[x] read_file",
        "[x] write_file",
        "[x] web",
        "[x] web_search",
        "[x] fetch_url",
        "[-] system",
        "[-] execute_command",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    assert!(focused_line(&screen).contains("Select all"), "{screen}");
    h.down(1); // Unselect all
    let screen = h.keys(b" ");
    assert!(!screen.contains("[x] "), "{screen}");
    assert!(screen.contains("No tools enabled"), "{screen}");
    h.up(1);
    h.keys(b" "); // Select all again
    let body = finish_from_tools(&mut h);
    let all = json!(["read_file", "write_file", "web_search", "fetch_url"]);
    assert_eq!(body["target"]["input_data"]["tools"], all);
    assert_eq!(
        body["target"]["input_data"]["_runtime"]["allowed_tools"],
        all
    );
}

#[test]
fn a_category_box_is_tri_state_and_toggles_its_tools() {
    let mut h = harness();
    open_tools_step(&mut h, small_inventory());
    h.keys(b"\x1b[H");
    h.down(4); // read_file
    let screen = h.keys(b" ");
    assert!(screen.contains("[~] files"), "some → [~]:\n{screen}");
    h.up(1); // files
    let screen = h.keys(b" ");
    assert!(
        screen.contains("[x] files")
            && screen.contains("[x] read_file")
            && screen.contains("[x] write_file"),
        "some → all:\n{screen}"
    );
    assert!(
        screen.contains("[ ] web"),
        "other categories untouched:\n{screen}"
    );
    let screen = h.keys(b" ");
    assert!(
        screen.contains("[ ] files")
            && screen.contains("[ ] read_file")
            && screen.contains("[ ] write_file"),
        "all → none:\n{screen}"
    );
    let screen = h.keys(b" ");
    assert!(screen.contains("[x] files"), "none → all:\n{screen}");
    // A category whose every tool is disabled here changes nothing.
    h.down(6); // system
    assert!(focused_line(&h.turn()).contains("system"));
    let screen = h.keys(b" ");
    assert!(screen.contains("[-] system"), "{screen}");
    let body = finish_from_tools(&mut h);
    assert_eq!(
        body["target"]["input_data"]["tools"],
        json!(["read_file", "write_file"])
    );
}

/// The Tools step's action kinds a click test presses (meta-test below).
const CLICKED_TOOLS_ACTIONS: [&str; 8] = [
    "DefaultTools",
    "SelectAll",
    "UnselectAll",
    "Category",
    "Tool",
    "Auto",
    "Ask",
    "Next",
];

#[test]
fn every_offered_tools_action_has_a_click_test() {
    use abstractcode::ui::schedule_view::{tools_lines, ToolsAct};
    let (_, acts) = tools_lines(Some(&[]), "auto", &small_inventory(), false);
    let mut kinds: Vec<&str> = acts
        .iter()
        .map(|a| match a {
            ToolsAct::DefaultTools => "DefaultTools",
            ToolsAct::SelectAll => "SelectAll",
            ToolsAct::UnselectAll => "UnselectAll",
            ToolsAct::Category(_) => "Category",
            ToolsAct::Tool(_) => "Tool",
            ToolsAct::Auto => "Auto",
            ToolsAct::Ask => "Ask",
            ToolsAct::Next => "Next",
        })
        .collect();
    kinds.sort();
    kinds.dedup();
    let mut clicked = CLICKED_TOOLS_ACTIONS.to_vec();
    clicked.sort();
    assert_eq!(kinds, clicked, "an offered action without a click test");
}

#[test]
fn a_click_on_a_tool_line_toggles_it() {
    let mut h = harness();
    open_tools_step(&mut h, small_inventory());
    // "Use workflow default tools": on hides the list, off shows it empty.
    let screen = h.click_on("[ ] Use workflow default tools");
    assert!(
        screen.contains("[x] Use workflow default tools"),
        "{screen}"
    );
    assert!(!screen.contains("web_search"), "{screen}");
    let screen = h.click_on("[x] Use workflow default tools");
    assert!(screen.contains("No tools enabled"), "{screen}");
    let (row, col) = locate(&screen, "[ ] web_search");
    let screen = h.click(row, col + 6);
    assert!(screen.contains("[x] web_search"), "{screen}");
    assert!(
        focused_line(&screen).contains("web_search"),
        "the clicked line is the focused line:\n{screen}"
    );
    // A click on a category header toggles the category.
    let screen = h.click_on("[~] web");
    assert!(screen.contains("[x] fetch_url"), "{screen}");
    // The buttons: Unselect all, then Select all.
    let screen = h.click_on("Unselect all");
    assert!(screen.contains("No tools enabled"), "{screen}");
    let screen = h.click_on("Select all");
    assert!(screen.contains("[x] read_file"), "{screen}");
    // A click on a selected tool unselects it.
    let screen = h.click_on("[x] write_file");
    assert!(screen.contains("[ ] write_file"), "{screen}");
    // The approval radios.
    let screen = h.click_on("( ) Ask me before each tool call");
    assert!(
        screen.contains("(•) Ask me before each tool call"),
        "{screen}"
    );
    let screen = h.click_on("( ) Run without asking");
    assert!(screen.contains("(•) Run without asking"), "{screen}");
    let screen = h.click_on("( ) Ask me before each tool call");
    assert!(screen.contains("(•) Ask me"), "{screen}");
    // A click on Continue continues.
    let screen = h.click_on("Continue — Workspaces");
    assert!(screen.contains("5/7 Workspaces"), "{screen}");
    h.keys(b"\r"); // Workspaces → Mailbox
    h.keys(b"\r"); // Mailbox → Title and limits
    h.keys(b"\r"); // Create automation
    let body = h.created();
    assert_eq!(
        body["target"]["input_data"]["tools"],
        json!(["read_file", "web_search", "fetch_url"])
    );
    assert_eq!(body["policy"], json!({"tool_approval": "ask"}));
}

/// 60 tools in 6 toolsets: taller than the modal at 140x44.
fn long_inventory() -> Vec<abstractcode::store::ToolInfo> {
    (0..60)
        .map(|i| tool(&format!("tool_{i:02}"), &format!("set_{}", i / 10)))
        .collect()
}

#[test]
fn the_wheel_and_the_page_keys_scroll_the_tool_list() {
    let mut h = harness();
    let screen = open_tools_step(&mut h, long_inventory());
    // The cursor starts on Continue: the list shows its end.
    assert!(screen.contains("tool_59"), "{screen}");
    assert!(!screen.contains("tool_00"), "{screen}");
    assert!(screen.contains("more"), "the overflow is said:\n{screen}");
    let (row, col) = locate(&screen, "tool_55");
    let mut screen = screen;
    for _ in 0..30 {
        screen = h.wheel(row, col, true);
    }
    assert!(
        screen.contains("tool_00"),
        "the wheel scrolls to the top:\n{screen}"
    );
    assert!(!screen.contains("tool_59"), "{screen}");
    // A click after a wheel toggles the line under the pointer.
    screen = h.click_on("[ ] tool_02");
    assert!(screen.contains("[x] tool_02"), "{screen}");
    for _ in 0..30 {
        screen = h.wheel(row, col, false);
    }
    assert!(
        screen.contains("tool_59"),
        "the wheel scrolls back down:\n{screen}"
    );
    // Page keys move the cursor a page at a time, the window follows.
    h.keys(b"\x1b[F"); // End: Continue
    let mut screen = String::new();
    for _ in 0..8 {
        screen = h.keys(b"\x1b[5~"); // PgUp
    }
    assert!(
        screen.contains("tool_00"),
        "PgUp reaches the top:\n{screen}"
    );
    let before = focused_line(&screen);
    let screen = h.keys(b"\x1b[6~"); // PgDn
    assert_ne!(focused_line(&screen), before, "{screen}");
    for _ in 0..8 {
        h.keys(b"\x1b[6~");
    }
    let screen = h.turn();
    assert!(
        focused_line(&screen).contains("Continue — Workspaces"),
        "{screen}"
    );
}

#[test]
fn use_workflow_default_tools_hides_the_list_and_sends_no_tools() {
    let mut h = harness();
    open_tools_step(&mut h, small_inventory());
    h.keys(b"\x1b[H");
    let screen = h.keys(b" ");
    assert!(
        screen.contains("[x] Use workflow default tools"),
        "{screen}"
    );
    assert!(!screen.contains("Select all"), "{screen}");
    assert!(!screen.contains("read_file"), "{screen}");
    let body = finish_from_tools(&mut h);
    assert!(body["target"]["input_data"].get("tools").is_none());
}

#[test]
fn the_task_is_a_multiline_text_that_reaches_the_prompt() {
    let mut h = harness();
    h.command("/automation");
    h.answer_prepare("me_email_not_connected.json");
    h.up(1); // Task
    let screen = h.keys(b"\r");
    for label in [
        "New automation — 1/7 What · Task",
        "Continue — When",
        "Enter / Ctrl+J newline",
        "report the free memory of this computer",
    ] {
        assert!(screen.contains(label), "{label}:\n{screen}");
    }
    // Enter inserts a newline (it never leaves the field), Ctrl+J too.
    h.keys(b"\x1b[F"); // End of the line
    h.keys(b"\r");
    h.term.push_input(b"in MB");
    h.turn();
    h.keys(b"\n"); // Ctrl+J
    h.term.push_input(b"and swap");
    let screen = h.turn();
    let screen = if screen.contains("and swap") {
        screen
    } else {
        h.turn()
    };
    assert!(
        screen.contains("New automation — 1/7 What · Task"),
        "Enter stayed in the editor:\n{screen}"
    );
    assert!(
        screen.contains("in MB") && screen.contains("and swap"),
        "{screen}"
    );
    // The visible Continue keeps it (a click presses it).
    let screen = h.click_on("Continue — When");
    assert!(screen.contains("1/7 What"), "{screen}");
    assert!(
        focused_line(&screen).contains("Task"),
        "back on the Task row:\n{screen}"
    );
    for label in ["in MB", "and swap"] {
        assert!(
            screen.contains(label),
            "the What step shows every line: {label}\n{screen}"
        );
    }
    h.cont(); // What → When
    for _ in 0..6 {
        h.keys(b"\r"); // When, Context, Tools, Workspaces, Mailbox, Create
    }
    let body = h.created();
    let task = "report the free memory of this computer\nin MB\nand swap";
    assert_eq!(body["target"]["input_data"]["prompt"], json!(task));
    assert_eq!(
        body["title"],
        json!("report the free memory of this computer"),
        "the title defaults to the task's first line"
    );
}

#[test]
fn the_task_editor_continue_is_reachable_with_tab_and_esc_keeps_nothing() {
    let mut h = harness();
    h.command("/automation one line");
    h.answer_prepare("me_email_not_connected.json");
    h.up(1);
    h.keys(b"\r");
    h.keys(b"\x1b[F");
    h.keys(b"\r");
    h.term.push_input(b"two");
    h.turn();
    // Esc goes back without keeping.
    let screen = h.esc();
    assert!(!screen.contains("· Task"), "{screen}");
    assert!(!screen.contains("two"), "{screen}");
    // Tab reaches Continue, Enter presses it.
    h.keys(b"\r"); // the cursor stayed on Task: the editor again
    h.keys(b"\x1b[F");
    h.keys(b"\r");
    h.term.push_input(b"two");
    h.turn();
    h.keys(b"\t");
    let screen = h.keys(b"\r");
    assert!(!screen.contains("· Task"), "{screen}");
    assert!(screen.contains("two"), "{screen}");
    h.cont();
    for _ in 0..6 {
        h.keys(b"\r");
    }
    assert_eq!(
        h.created()["target"]["input_data"]["prompt"],
        json!("one line\ntwo")
    );
}

#[test]
fn select_all_keeps_the_gateways_order() {
    let mut h = harness();
    open_tools_step(
        &mut h,
        BASIC_TOOLS.iter().map(|n| tool(n, "core")).collect(),
    );
    h.keys(b"\x1b[H");
    h.down(1);
    h.keys(b" ");
    let body = finish_from_tools(&mut h);
    assert_eq!(body["target"]["input_data"]["tools"], json!(BASIC_TOOLS));
}

#[test]
fn schedule_is_a_silent_alias_of_automation() {
    let mut h = harness();
    let screen = h.command("/schedule watch the disk");
    assert!(screen.contains("New automation — 1/7 What"), "{screen}");
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
    // Workflow, Repeat every (What, then When): Enter edits it; an invalid interval.
    h.keys(b"\x1b[B");
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
    h.keys(b"\x1b[B"); // Repeat every
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
fn the_definition_panel_edits_the_limits_as_one_revision() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    h.keys(b"e");
    assert!(
        h.auto_cmds()
            .iter()
            .any(|c| matches!(c, AutoCmd::EmailStatus)),
        "the Mailbox rows read the account's email status"
    );
    // Workflow, Repeat every, Context, Max growing context (Inbox triage is
    // growing), Tools, Email result, Title, then "Stop after this many runs"
    // (the kit's last section, Title and limits).
    let mut screen = String::new();
    for _ in 0..7 {
        screen = h.keys(b"\x1b[B");
    }
    assert!(screen.contains("Title and limits"), "{screen}");
    assert!(screen.contains("Stop after this many runs"), "{screen}");
    assert!(screen.contains("no limit"), "{screen}");
    h.keys(b"\r");
    h.term.push_input(b"5");
    h.turn();
    h.keys(b"\r");
    let sent = saves(&mut h);
    match sent.as_slice() {
        [(3, changes)] => assert_eq!(
            changes,
            &json!({"trigger": {"source_id": "schedule", "source_version": 1, "config": {
                "start_at": "2026-09-27T04:00:00.412307+00:00",
                "anchor": "2026-09-27T04:00:00.412307+00:00",
                "every": "30m", "count": 5}}})
        ),
        other => panic!("expected one revision, got {other:?}"),
    }
    // Stop at: a bad value is the kit's sentence, nothing sent.
    h.keys(b"\x1b[B");
    h.keys(b"\r");
    h.term.push_input(b"next week");
    h.turn();
    let screen = h.keys(b"\r");
    assert!(saves(&mut h).is_empty());
    assert!(
        screen.contains("Stop at must be a date and time (UTC)."),
        "{screen}"
    );
    h.keys(b"\r");
    h.term.push_input(b"2026-12-31 18:00");
    h.turn();
    h.keys(b"\r");
    match saves(&mut h).as_slice() {
        [(3, changes)] => {
            assert_eq!(
                changes["trigger"]["config"]["until"],
                json!("2026-12-31T18:00:00Z")
            );
            assert_eq!(changes["trigger"]["config"]["every"], json!("30m"));
        }
        other => panic!("expected one revision, got {other:?}"),
    }
}

#[test]
fn the_definition_panel_edits_the_email_options() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    h.keys(b"e");
    // Not usable: the kit's notice; the switch refuses with its sentence.
    h.store.automations.update(|v| {
        v.email = abstractcode::automation_email::EmailStatus::parse(&schedule_fixture(
            "me_email_not_connected.json",
        ))
        .ok()
    });
    // Workflow, Repeat every, Context, Max growing context, Tools, then
    // Email result (Mailbox).
    let mut screen = String::new();
    for _ in 0..5 {
        screen = h.keys(b"\x1b[B");
    }
    assert!(screen.contains("Mailbox"), "{screen}");
    assert!(
        screen.contains("Connect a mailbox first — open My email"),
        "{screen}"
    );
    assert!(
        screen.contains("[-] Email result — Connect a mailbox first."),
        "{screen}"
    );
    let screen = h.keys(b" ");
    assert!(saves(&mut h).is_empty());
    assert!(screen.contains("Connect a mailbox first."), "{screen}");
    // Usable: Email result on = one revision with `notify`.
    h.store.automations.update(|v| {
        v.email = abstractcode::automation_email::EmailStatus::parse(&schedule_fixture(
            "me_email_connected.json",
        ))
        .ok()
    });
    let screen = h.turn();
    assert!(screen.contains("[ ] Email result"), "{screen}");
    h.keys(b" ");
    match saves(&mut h).as_slice() {
        [(3, changes)] => assert_eq!(
            changes,
            &json!({"notify": {"channels": ["console", "email"]}})
        ),
        other => panic!("expected one revision, got {other:?}"),
    }
    // The gateway stored it: the recipients row appears; "Me and these addresses".
    h.store.automations.update(|v| {
        v.detail
            .as_mut()
            .unwrap()
            .definition
            .as_mut()
            .unwrap()
            .notify = json!({"channels": ["console", "email"]});
    });
    let screen = h.turn();
    assert!(screen.contains("[x] Email result"), "{screen}");
    assert!(screen.contains("Only me"), "{screen}");
    h.keys(b"\x1b[B");
    h.keys(b"\r");
    h.keys(b"\x1b[B");
    h.keys(b"\r");
    h.term.push_input(b"boss@example.test");
    h.turn();
    h.keys(b"\r");
    match saves(&mut h).as_slice() {
        [(3, changes)] => assert_eq!(
            changes,
            &json!({"notify": {"channels": ["console", "email"], "recipients": ["self", "boss@example.test"]}})
        ),
        other => panic!("expected one revision, got {other:?}"),
    }
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
    h.keys(b"3"); // the Workspaces section (an automation's third)
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

/// The latest schedule-preview the dialog asked (every change of the
/// When rule reopens the step and asks once for the new trigger).
fn preview_asked(h: &mut Harness) -> Value {
    h.auto_cmds()
        .into_iter()
        .rev()
        .find_map(|c| match c {
            AutoCmd::Preview { trigger } => Some(trigger),
            _ => None,
        })
        .expect("the When step asks the gateway's schedule-preview")
}

/// The When step's kinds, in the kit's order (the first six selectable cards).
const KIND_REPEAT: usize = 0;
const KIND_DAILY: usize = 1;
const KIND_WEEKLY: usize = 2;
const KIND_MONTHLY: usize = 3;
const KIND_ONCE: usize = 4;

/// `/schedule brief me` → What: Continue → the When step (Repeat, cursor on
/// Continue = the 15th selectable card) → the radio of `kind`.
fn pick_when(h: &mut Harness, kind: usize) -> String {
    h.command("/schedule brief me");
    h.answer_prepare("me_email_not_connected.json");
    let screen = h.keys(b"\r"); // What: Continue
    assert!(screen.contains("2/7 When"), "{screen}");
    if kind == KIND_REPEAT {
        return screen;
    }
    h.up(14 - kind);
    h.keys(b"\r")
}

#[test]
fn schedule_daily_shows_the_gateways_words_and_writes_schedule_v2() {
    let mut h = harness();
    let screen = pick_when(&mut h, KIND_DAILY);
    assert!(screen.contains("(•) Daily"), "{screen}");
    assert!(screen.contains("Time of day"), "{screen}");
    assert!(screen.contains("08:00"), "the default time:\n{screen}");
    assert!(
        flat(&screen).contains("keep this wall-clock time when daylight saving time changes"),
        "{screen}"
    );
    let trigger = preview_asked(&mut h);
    assert_eq!(
        trigger,
        json!({"source_id": "schedule", "source_version": 2, "config": {"kind": "daily", "at": "08:00"}})
    );
    // Not answered yet: the kit's "Checking the schedule…", never an invented sentence.
    assert!(screen.contains("Checking the schedule…"), "{screen}");
    assert!(!screen.contains("Runs every day"), "{screen}");
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
        flat(&screen).contains(sentence),
        "the served sentence verbatim:\n{screen}"
    );
    let screen = h.cont(); // When: Continue
    assert!(screen.contains("3/7 Context"), "{screen}");
    h.keys(b"\r"); // Context: Continue
    h.keys(b"\r"); // Tools: Continue
    h.keys(b"\r"); // Workspaces: Continue
    let screen = h.keys(b"\r"); // Mailbox: Continue
    assert!(screen.contains("7/7 Title and limits"), "{screen}");
    assert!(flat(&screen).contains(sentence), "{screen}");
    assert!(screen.contains("Stop after this many runs"), "{screen}");
    assert!(screen.contains("Stop at (UTC)"), "{screen}");
    assert!(
        !screen.contains("First run at"),
        "a calendar rule has no first-run time:\n{screen}"
    );
    h.keys(b"\r"); // Create automation
    let body = h.created();
    assert_eq!(body["trigger"], trigger);
}

#[test]
fn schedule_weekly_days_are_state_showing_toggles() {
    let mut h = harness();
    let screen = pick_when(&mut h, KIND_WEEKLY);
    assert!(screen.contains("(•) Weekly"), "{screen}");
    assert!(
        screen.contains("[x] Mon") && screen.contains("[ ] Tue"),
        "{screen}"
    );
    // Selectable: six kinds, Mon..Sun (6..12), Time of day (13), Continue (14).
    // The cursor stays on Weekly (2) after the pick.
    h.down(5); // Tue
    let screen = h.keys(b"\r"); // Tue on
    assert!(
        screen.contains("[x] Mon") && screen.contains("[x] Tue"),
        "{screen}"
    );
    h.up(1); // Mon (the cursor stayed on Tue)
    let screen = h.keys(b"\r"); // Mon off
    assert!(
        screen.contains("[ ] Mon") && screen.contains("[x] Tue"),
        "{screen}"
    );
    assert_eq!(
        preview_asked(&mut h)["config"],
        json!({"kind": "weekly", "days": ["tue"], "at": "08:00"})
    );
    // Tue off too: an empty day set is refused on Continue, never refilled.
    h.down(1);
    let screen = h.keys(b"\r");
    assert!(screen.contains("[ ] Tue"), "{screen}");
    assert!(screen.contains("Incomplete schedule."), "{screen}");
    let screen = h.cont(); // Continue
    assert!(screen.contains("Pick at least one day."), "{screen}");
    assert!(!screen.contains("3/7 Context"), "{screen}");
    // Weekly → Monthly → Weekly keeps the picked (empty) set and the time.
    h.up(14 - KIND_MONTHLY);
    let screen = h.keys(b"\r");
    assert!(screen.contains("(•) Monthly"), "{screen}");
    h.up(KIND_MONTHLY - KIND_WEEKLY);
    let screen = h.keys(b"\r");
    assert!(
        screen.contains("[ ] Mon") && screen.contains("[ ] Tue"),
        "{screen}"
    );
}

#[test]
fn schedule_monthly_last_day_and_once_send_their_rules() {
    let mut h = harness();
    let screen = pick_when(&mut h, KIND_MONTHLY);
    assert!(screen.contains("(•) Monthly"), "{screen}");
    assert!(screen.contains("on day"), "{screen}");
    // Selectable: six kinds, on day (6), Time of day (7), Continue (8);
    // the cursor stayed on Monthly (3).
    h.down(3);
    h.keys(b"\r"); // edit "on day" (its value "1")
    h.keys(b"\x1b[F\x7f");
    h.term.push_input(b"last");
    h.turn();
    let screen = h.keys(b"\r");
    assert!(screen.contains("last"), "{screen}");
    assert_eq!(
        preview_asked(&mut h)["config"],
        json!({"kind": "monthly", "day": "last", "at": "08:00"})
    );

    let mut h = harness();
    let screen = pick_when(&mut h, KIND_ONCE);
    assert!(screen.contains("(•) Once at…"), "{screen}");
    assert!(screen.contains("Run once at"), "{screen}");
    // Selectable: six kinds, Run once at (6), Continue (7); the cursor
    // stayed on Once at… (4).
    h.down(2);
    h.keys(b"\r");
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
    pick_when(&mut h, KIND_DAILY);
    let trigger = preview_asked(&mut h);
    let refusal = "The automation definition is not valid. Unknown time zone 'Mars/Olympus'. (field trigger.config.time_zone)";
    h.store
        .automations
        .update(|v| v.apply_preview(&trigger, auto::PreviewState::Failed(refusal.into())));
    let screen = h.turn();
    assert!(flat(&screen).contains(refusal), "{screen}");
    let screen = h.cont();
    assert!(!screen.contains("3/7 Context"), "{screen}");
    assert!(flat(&screen).contains(refusal), "{screen}");
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
        notify: Value::Null,
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
    // When (row 2: Workflow, When) → Weekly: ONE revision with the
    // rule and the binding's own zone.
    h.keys(b"\x1b[B");
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
    // Time of day (row 3: Workflow, When, Time of day).
    h.keys(b"\x1b[B\x1b[B");
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

/// The gateway saved `changes`: the bound automation now carries that trigger.
fn gateway_applies(h: &mut Harness, id: &str, changes: &Value) {
    let config = changes["trigger"]["config"].as_object().unwrap().clone();
    h.store.automations.update(|v| {
        if let Some(d) = v.detail.as_mut().filter(|d| d.id == id) {
            if let Some(s) = d.summary.as_mut() {
                s.trigger.config = config.clone();
            }
        }
    });
    h.turn();
}

fn one_save(h: &mut Harness) -> Value {
    let (_, saved) = previews_and_saves(h);
    assert_eq!(saved.len(), 1, "{saved:?}");
    saved[0].1.clone()
}

#[test]
fn edit_weekly_days_survive_monthly_and_back() {
    let mut h = harness();
    let s = edit_morning_briefing(&mut h);
    previews_and_saves(&mut h);
    // When (row 2: Workflow, When) → Weekly.
    h.keys(b"\x1b[B");
    h.keys(b"\r");
    h.keys(b"\x1b[B");
    h.keys(b"\r");
    let c = one_save(&mut h);
    assert_eq!(c["trigger"]["config"]["days"], json!(["mon"]));
    gateway_applies(&mut h, &s.id, &c);
    // Fri (rows: Workflow, Title, When, Mon … Sun): from When, 5 down.
    for _ in 0..5 {
        h.keys(b"\x1b[B");
    }
    let screen = h.keys(b"\r");
    let c = one_save(&mut h);
    assert_eq!(
        c["trigger"]["config"]["days"],
        json!(["mon", "fri"]),
        "{screen}"
    );
    gateway_applies(&mut h, &s.id, &c);
    // Back to When → Monthly.
    for _ in 0..5 {
        h.keys(b"\x1b[A");
    }
    h.keys(b"\r");
    h.keys(b"\x1b[B");
    h.keys(b"\r");
    let c = one_save(&mut h);
    assert_eq!(c["trigger"]["config"]["kind"], "monthly");
    assert!(c["trigger"]["config"].get("days").is_none());
    gateway_applies(&mut h, &s.id, &c);
    // When → Weekly again: Mon and Fri, not a fresh Monday.
    let screen = h.keys(b"\r");
    assert!(screen.contains("Monthly"), "{screen}");
    h.keys(b"\x1b[A");
    h.keys(b"\r");
    let c = one_save(&mut h);
    assert_eq!(
        c["trigger"]["config"],
        json!({"kind": "weekly", "days": ["mon", "fri"], "at": "08:00", "time_zone": "Europe/Paris"})
    );
}

/// The gateway answers the latest schedule-preview with `sentence`.
fn answer_preview(h: &mut Harness, sentence: &str) -> String {
    let trigger = preview_asked(h);
    h.store
        .automations
        .update(|v| v.apply_preview(&trigger, served_preview(&trigger, sentence)));
    h.turn()
}

// ---------------------------------------------------------------------------
// Operator feedback 2026-10-09 (autofix 2): the definition panel scrolls
// and edits the growing budget; an automation opens as a chat; the rail
// offers only the automation's sections.
// ---------------------------------------------------------------------------

/// Inbox triage (growing) open in Edit on a small terminal.
fn edit_inbox_small(size: Size) -> Harness {
    let mut h = harness_sized(size);
    open_inbox(&mut h);
    h.auto_cmds();
    h.keys(b"e");
    h
}

/// The line the cursor marker `▸ ` (a card's) is on, if any.
fn card_cursor_line(screen: &str) -> Option<String> {
    screen
        .lines()
        .find(|l| l.trim_start().starts_with("▸ "))
        .map(str::to_string)
}

#[test]
fn the_definition_panel_scrolls_to_mailbox_and_title_and_limits() {
    let mut h = edit_inbox_small(Size::new(100, 26));
    let screen = h.turn();
    assert!(screen.contains("↓"), "the panel overflows here:\n{screen}");
    assert!(!screen.contains("Stop at (UTC)"), "{screen}");
    // ↓ walks every row; the focused row is always on screen.
    let mut seen = String::new();
    for _ in 0..12 {
        let screen = h.keys(b"\x1b[B");
        assert!(
            card_cursor_line(&screen).is_some(),
            "the focused row is visible:\n{screen}"
        );
        seen.push_str(&flat(&screen));
    }
    for n in [
        "Mailbox",
        "Email result",
        "Title and limits",
        "Inbox triage",
        "Stop after this many runs",
        "Stop at (UTC)",
    ] {
        assert!(seen.contains(n), "{n} reached");
    }
    // At the last row the panel's end shows (nothing below it).
    let screen = h.turn();
    assert!(
        card_cursor_line(&screen).unwrap().contains("Stop at (UTC)"),
        "{screen}"
    );
    assert!(
        !screen
            .lines()
            .any(|l| l.contains('↓') && l.contains(" more")),
        "nothing below the end:\n{screen}"
    );
    // PgUp goes back up a page at a time; Home to the first row (the top).
    h.keys(b"\x1b[5~");
    let screen = h.keys(b"\x1b[H");
    assert!(
        card_cursor_line(&screen).unwrap().contains("Workflow"),
        "{screen}"
    );
    assert!(screen.contains("What"), "{screen}");
    // PgDn twice reaches the end too.
    h.keys(b"\x1b[6~");
    h.keys(b"\x1b[6~");
    h.keys(b"\x1b[6~");
    let screen = h.keys(b"\x1b[6~");
    assert!(screen.contains("Stop at (UTC)"), "{screen}");
    // The wheel scrolls without moving the cursor: back to the top rows.
    let (row, col) = locate(&screen, "Stop at (UTC)");
    let mut screen = screen;
    for _ in 0..10 {
        screen = h.wheel(row, col, true);
    }
    assert!(
        screen.contains("Workflow"),
        "the wheel scrolls up:\n{screen}"
    );
    for _ in 0..10 {
        screen = h.wheel(row, col, false);
    }
    assert!(screen.contains("Stop at (UTC)"), "and down:\n{screen}");
}

#[test]
fn the_growing_budget_round_trips_to_a_revision() {
    let mut h = harness();
    open_inbox(&mut h);
    h.auto_cmds();
    h.keys(b"e");
    // What: Workflow · When: Repeat every · Context: Context, budget.
    h.keys(b"\x1b[B\x1b[B\x1b[B");
    let screen = h.turn();
    let line = card_cursor_line(&screen).unwrap();
    assert!(
        line.contains("Max growing context (tokens)") && line.contains("50000"),
        "{screen}"
    );
    assert!(
        screen.contains("Limits history carried into the next run"),
        "{screen}"
    );
    // A bad value: the kit's sentence, nothing sent.
    h.keys(b"\r");
    h.keys(b"\x1b[F\x7f\x7f\x7f\x7f\x7f");
    h.term.push_input(b"lots");
    h.turn();
    let screen = h.keys(b"\r");
    assert!(saves(&mut h).is_empty());
    assert!(
        flat(&screen).contains("Max growing context must be a positive whole number of tokens."),
        "{screen}"
    );
    // 80000: ONE revision, the kit's context shape.
    h.keys(b"\r");
    h.keys(b"\x1b[F\x7f\x7f\x7f\x7f\x7f");
    h.term.push_input(b"80000");
    h.turn();
    h.keys(b"\r");
    match saves(&mut h).as_slice() {
        [(3, changes)] => assert_eq!(
            changes,
            &json!({"context": {"mode": "growing", "growing": {"max_tokens": 80000}}})
        ),
        other => panic!("expected one revision, got {other:?}"),
    }
    // The gateway stored it: the panel reads it back.
    h.store.automations.update(|v| {
        let def = v.detail.as_mut().unwrap().definition.as_mut().unwrap();
        def.growing.insert("max_tokens".into(), json!(80000));
        def.revision = 4;
    });
    let screen = h.turn();
    assert!(
        card_cursor_line(&screen).unwrap().contains("80000"),
        "{screen}"
    );
    // Back to the default: the kit sends the mode alone.
    h.keys(b"\r");
    h.keys(b"\x1b[F\x7f\x7f\x7f\x7f\x7f");
    h.term.push_input(b"50000");
    h.turn();
    h.keys(b"\r");
    match saves(&mut h).as_slice() {
        [(4, changes)] => assert_eq!(changes, &json!({"context": {"mode": "growing"}})),
        other => panic!("expected one revision, got {other:?}"),
    }
}

#[test]
fn open_as_chat_renders_every_run_as_a_transcript() {
    let mut h = harness();
    open_inbox(&mut h);
    let screen = h.keys(b"o");
    assert!(
        screen.contains("Automations / Inbox triage · as chat · 7 runs (read-only)"),
        "{screen}"
    );
    // The newest run at the end, in the transcript's own cards.
    assert!(screen.contains("━━ #7"), "{screen}");
    assert!(
        screen.contains("══ you"),
        "the task is your turn:\n{screen}"
    );
    // Home: the oldest run; every separator is met on the way down.
    let mut seen = flat(&h.keys(b"\x1b[H"));
    assert!(seen.contains("#1 · completed"), "{seen}");
    for _ in 0..40 {
        seen.push_str(&flat(&h.keys(b"\x1b[6~")));
    }
    for i in 1..=7 {
        assert!(seen.contains(&format!("━━ #{i} ")), "run #{i}:\n{seen}");
    }
    assert!(seen.contains("No new email needs a reply today."), "{seen}");
    // Esc: back to the automation.
    let screen = h.esc();
    assert!(screen.contains("Automations / Inbox triage"), "{screen}");
    assert!(!screen.contains("· as chat ·"), "{screen}");
    // Enter on a run (the newest, at the end) opens the chat on that run.
    h.down(30);
    let screen = h.keys(b"\r");
    assert!(screen.contains("· as chat ·"), "{screen}");
}

#[test]
fn the_rail_offers_only_the_automations_sections() {
    let mut h = harness();
    open_inbox(&mut h);
    let screen = h.keys(b"e");
    for label in [
        "Sections (1–5)",
        "the automation's",
        "definition — changes",
        "1 Task and schedule",
        "2 Model and limits",
        "3 Workspaces",
        "4 Tools",
        "5 Skills",
    ] {
        assert!(flat(&screen).contains(label), "{label}:\n{screen}");
    }
    for gone in ["Activity", "Files", "Voice"] {
        assert!(!screen.contains(gone), "{gone}:\n{screen}");
    }
    assert!(screen.contains("1–5 ←→ section"), "the key hint:\n{screen}");
    // A click on a section switches to it (its title, its highlight).
    let screen = h.click_on("4 Tools");
    assert!(screen.contains("▸4 Tools"), "{screen}");
    assert!(
        screen.contains("All tools") || screen.contains("Loading tools…"),
        "the Tools section:\n{screen}"
    );
    // ← wraps among the five: 3 Workspaces; 1 then ← = 5 Skills.
    let screen = h.keys(b"\x1b[D");
    assert!(screen.contains("▸3 Workspaces"), "{screen}");
    h.keys(b"1");
    let screen = h.keys(b"\x1b[D");
    assert!(screen.contains("▸5 Skills"), "{screen}");
    // 6–8 do nothing here.
    let screen = h.keys(b"8");
    assert!(screen.contains("▸5 Skills"), "{screen}");
}

/// 120x40 screens of the three fixes (fixture automation "Inbox triage",
/// 7 runs): the definition panel at its top and scrolled to its end, the
/// automation's sections, and the runs opened as a chat (newest, then the
/// oldest). `AUTOFIX_CAPTURE_DIR=<dir>` writes them as text.
#[test]
fn autofix2_screens_at_120x40() {
    let dir = std::env::var("AUTOFIX_CAPTURE_DIR").ok();
    let shot = |name: &str, screen: &str| {
        if let Some(d) = &dir {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(format!("{d}/{name}.txt"), screen).unwrap();
        }
    };
    let mut h = edit_inbox_small(Size::new(120, 40));
    let screen = h.turn();
    assert!(screen.contains("Sections (1–5)"), "{screen}");
    shot("a2-01-edit-panel-top", &screen);
    let screen = h.keys(b"\x1b[F");
    assert!(
        screen.contains("Title and limits") && screen.contains("Mailbox"),
        "{screen}"
    );
    shot("a2-02-edit-panel-end-mailbox-title-and-limits", &screen);
    h.keys(b"\x1b[H");
    h.keys(b"\x1b[B\x1b[B\x1b[B");
    let screen = h.turn();
    assert!(screen.contains("Max growing context (tokens)"), "{screen}");
    shot("a2-03-edit-panel-growing-budget", &screen);
    let screen = h.click_on("2 Model and limits");
    assert!(screen.contains("▸2 Model and limits"), "{screen}");
    shot("a2-04-rail-section-clicked", &screen);
    let mut h = harness_sized(Size::new(120, 40));
    open_inbox(&mut h);
    let screen = h.keys(b"o");
    assert!(screen.contains("· as chat · 7 runs"), "{screen}");
    shot("a2-05-open-as-chat-newest", &screen);
    let screen = h.keys(b"\x1b[H");
    assert!(screen.contains("#1 · completed"), "{screen}");
    shot("a2-06-open-as-chat-oldest", &screen);
}
