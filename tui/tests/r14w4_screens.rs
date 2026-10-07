//! R14.4 screens without a gateway: the Code TUI's workspace chooser
//! (this conversation, My default workspaces, an automation, the
//! new-automation dialog's Workspaces + Title and limits steps), the tool
//! cards' command-sandbox state and the sandbox line per command in the run
//! views — rendered at 80×24 and 120×40 from the RECORDED route answers in
//! `tests/fixtures/workspaces/` (a scratch gateway at origin/main, alice),
//! applied to the store exactly as the workspaces lane's posted closures
//! apply them. Asserts the kit's words and that no row overflows. With
//! `R14W4_CAPTURE_DIR` set, each screen is written there
//! (`<w>x<h>/<name>.txt`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;
use serde_json::{json, Value};

use abstractcode::automations as auto;
use abstractcode::config::Prefs;
use abstractcode::gateway::workspaces::WsCmd;
use abstractcode::runner::Cmd;
use abstractcode::store::{Store, ToolInfo, Workflow};
use abstractcode::transcript::{Fold, Item};
use abstractcode::ui::{self, UiCtx};
use abstractcode::workspaces as ws;

const WS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/workspaces");
const AUTOS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/automations");
const INBOX: &str = "53443dd0-25c4-5fa8-bdad-e1ac3fdfff8e";
const SESSION: &str = "acode-screens-session";

fn read(dir: &str, name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(format!("{dir}/{name}")).unwrap()).unwrap()
}

struct H {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    rx: mpsc::Receiver<Cmd>,
    size: Size,
}

fn harness(size: Size) -> H {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, rx) = mpsc::channel::<Cmd>();
    let slot: Rc<RefCell<Option<Store>>> = Rc::new(RefCell::new(None));
    let out = slot.clone();
    let prefs = Rc::new(RefCell::new(Prefs::default()));
    let actions = app.actions();
    app.mount(move |cx| {
        let store = Store::create(cx);
        *out.borrow_mut() = Some(store);
        store.session_id.set(SESSION.into());
        let ctx = UiCtx {
            tx,
            client: abstractcode::gateway::GatewayClient::new("http://127.0.0.1:1", None),
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            prefs: prefs.clone(),
            workspace_root: Some("/srv/gateway/data/workspaces/session-acode".into()),
            max_iterations_explicit: false,
            max_iterations: 0,
            no_project_context: true,
            no_prompt_cache: false,
            replay_turns: 2,
            gateway_label: "fixture".into(),
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
    let store = slot.borrow().expect("store");
    let mut h = H {
        app,
        term,
        driver,
        store,
        rx,
        size,
    };
    h.store.workflow.set(Workflow {
        bundle_id: "basic-agent".into(),
        flow_id: "agent".into(),
        version: "9.9.9".into(),
        ..Workflow::default()
    });
    for _ in 0..3 {
        h.turn();
    }
    h
}

impl H {
    fn turn(&mut self) -> String {
        self.driver
            .turn(&mut self.app, &mut self.term)
            .expect("turn");
        self.term.screen().to_text()
    }
    fn keys(&mut self, b: &[u8]) -> String {
        self.term.push_input(b);
        self.turn();
        self.turn()
    }
    fn command(&mut self, text: &str) -> String {
        self.term.push_input(text.as_bytes());
        self.turn();
        self.keys(b"\r")
    }
    /// A bare Esc (the 30 ms disambiguation deadline, then dispatch).
    fn esc(&mut self) -> String {
        self.term.push_input(&[0x1b]);
        self.turn();
        std::thread::sleep(std::time::Duration::from_millis(45));
        self.turn();
        self.turn()
    }
    fn cmds(&mut self) -> Vec<Cmd> {
        let mut out = Vec::new();
        while let Ok(c) = self.rx.try_recv() {
            out.push(c);
        }
        out
    }
    /// Write the screen when asked; assert no row is wider than the screen.
    fn shot(&mut self, name: &str) -> String {
        let screen = self.turn();
        for line in screen.lines() {
            assert!(
                line.chars().count() <= self.size.w as usize,
                "a row overflows {}:\n{screen}",
                self.size.w
            );
        }
        if let Ok(dir) = std::env::var("R14W4_CAPTURE_DIR") {
            let d = std::path::Path::new(&dir).join(format!("{}x{}", self.size.w, self.size.h));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("{name}.txt")), &screen).unwrap();
        }
        screen
    }
    /// The session level answered (`GET /sessions/{id}/workspaces`).
    fn answer_session(&mut self, fixture: &str) {
        let state = ws::as_state(&read(WS, fixture)).unwrap();
        self.store.workspaces.update(|w| {
            w.loading.clear();
            w.session = Some((SESSION.into(), Ok(state)));
        });
        self.turn();
    }
    fn answer_account(&mut self) {
        let state = ws::as_state(&read(WS, "account_get.json")).unwrap();
        self.store.workspaces.update(|w| {
            w.loading.clear();
            w.account = Some(Ok(state));
        });
        self.turn();
    }
    fn answer_dry_run(&mut self, key: &str, fixture: &str) {
        let e = ws::as_effective(&read(WS, fixture)).unwrap();
        self.store.workspaces.update(|w| {
            w.loading.clear();
            w.put_run(key.into(), Ok(e));
        });
        self.turn();
    }
    /// Every needle is on the screen, read as wrapped text: rows joined,
    /// the rail's panel column and hanging indents removed.
    fn assert_rows(&self, screen: &str, needles: &[&str]) {
        let flat = flatten(screen);
        for n in needles {
            assert!(
                flat.contains(&squash(n)),
                "{n:?} missing at {}x{}:\n{screen}",
                self.size.w,
                self.size.h
            );
        }
    }
    /// Scroll the open card list to the end (the effective line is last).
    fn scroll_end(&mut self) -> String {
        for _ in 0..30 {
            self.keys(b"\x1b[B");
        }
        self.turn()
    }
    fn scroll_top(&mut self) -> String {
        for _ in 0..30 {
            self.keys(b"\x1b[A");
        }
        self.turn()
    }
}

const PANELS: [&str; 8] = [
    "Activity",
    "Files",
    "Model",
    "Workflow",
    "Workspace",
    "Tools",
    "Skills",
    "Voice",
];

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The screen as one line of words (wrapped sentences read whole).
fn flatten(screen: &str) -> String {
    let rows: Vec<String> = screen
        .lines()
        .map(|l| {
            let mut l = l.trim_end().to_string();
            for (i, p) in PANELS.iter().enumerate() {
                for mark in ["▸", " "] {
                    let tail = format!("{mark}{} {p}", i + 1);
                    if l.ends_with(&tail) {
                        l.truncate(l.len() - tail.len());
                    }
                }
            }
            l
        })
        .collect();
    squash(&rows.join(" "))
}

/// One fresh thread per size (the reactive runtime is thread-local: a
/// toast timer of one harness must never fire into the next).
fn per_size(body: impl Fn(Size) + Send + Sync + 'static) {
    let body = std::sync::Arc::new(body);
    for size in [Size::new(80, 24), Size::new(120, 40)] {
        let b = body.clone();
        if let Err(e) = std::thread::spawn(move || b(size)).join() {
            std::panic::resume_unwind(e);
        }
    }
}

#[test]
fn the_conversation_workspace_panel_is_the_session_chooser() {
    per_size(|size| {
        let mut h = harness(size);
        let screen = h.command("/workspace");
        assert!(screen.contains("Current workspace"), "{screen}");
        assert!(
            screen.contains("session-acode"),
            "the private workspace line: {screen}"
        );
        assert!(
            h.cmds().iter().any(|c| matches!(c,
                Cmd::Workspaces(WsCmd::LoadSession { session_id }) if session_id == SESSION)),
            "GET /sessions/{{id}}/workspaces"
        );
        h.shot("workspace-session-loading");
        // Following the default: what applies, read-only.
        h.answer_session("session_get_default.json");
        let screen = h.shot("workspace-session-default");
        h.assert_rows(
            &screen,
            &[
                "Workspaces",
                "The workspaces this conversation uses, among the eligible ones.",
                "Gateway: Allow everything, refuse listed workspaces (rw) · /Users/ada/home/work",
                "[x] Use my default",
            ],
        );
        let screen = h.scroll_end();
        h.shot("workspace-session-default-end");
        h.assert_rows(&screen, &["My default workspaces"]);
        // A configured conversation: rows with modes, caps, add, effective line.
        h.answer_session("session_get_configured.json");
        let screen = h.scroll_top();
        h.shot("workspace-session-configured");
        h.assert_rows(&screen, &["[ ] Use my default"]);
        let screen = h.keys(b"\x1b[B");
        h.shot("workspace-session-configured-posture");
        h.assert_rows(
            &screen,
            &[
                "Workspaces agents may use",
                "Deny everything, allow listed workspaces",
                "Agents may only work in the listed workspaces.",
            ],
        );
        let screen = h.keys(b"\x1b[B");
        h.shot("workspace-session-configured-rows");
        h.assert_rows(
            &screen,
            &[
                "Allowed workspaces",
                "/Users/ada/home/Pictures",
                "(-) Read & write (•) Read-only ( ) Refused",
                "The gateway allows this workspace read-only",
            ],
        );
        let screen = h.scroll_end();
        h.shot("workspace-session-configured-end");
        h.assert_rows(
            &screen,
            &[
                "Add a workspace path",
                "The private workspace of each run is always available, read & write.",
                "Deny everything, allow listed workspaces · /Users/ada/home/Pictures (ro) ·",
            ],
        );
    });
}

#[test]
fn a_mode_above_the_cap_is_never_sent_and_a_refusal_reads_not_saved() {
    per_size(|size| {
        let mut h = harness(size);
        h.command("/workspace");
        h.answer_session("session_get_configured.json");
        h.cmds();
        // Follow, Posture, then the first row (Pictures, cap ro).
        h.keys(b"\x1b[B\x1b[B");
        let screen = h.keys(b"\r");
        h.shot("workspace-mode-picker");
        h.assert_rows(
            &screen,
            &[
                "Read & write — The gateway allows this workspace read-only",
                "Read-only",
                "Refused",
                "Remove /Users/ada/home/Pictures",
            ],
        );
        // Pick "Read & write" (above the cap): nothing is sent.
        h.keys(b"\x1b[A");
        h.keys(b"\r");
        assert!(
            !h.cmds()
                .iter()
                .any(|c| matches!(c, Cmd::Workspaces(WsCmd::SaveSession { .. }))),
            "a mode above the cap is never sent"
        );
        // The gateway refuses a change: its sentence + "Not saved." under the row.
        let refusal = read(WS, "session_put_above_cap.json");
        let sentence = ws::error_sentence(&refusal).unwrap();
        h.store.workspaces.update(|w| {
            w.busy = None;
            w.status = Some(ws::Status {
                scope: format!("session:{SESSION}"),
                key: "/Users/ada/home/Pictures".into(),
                text: ws::refusal(&sentence),
                error: true,
            })
        });
        let screen = h.turn();
        h.shot("workspace-session-refusal");
        assert!(screen.contains("Not saved."), "{screen}");
        // Refused rows are offered: Refused is sent as a full replacement.
        h.keys(b"\r");
        h.keys(b"\x1b[B");
        h.keys(b"\r");
        let sent: Vec<Value> = h
            .cmds()
            .into_iter()
            .filter_map(|c| match c {
                Cmd::Workspaces(WsCmd::SaveSession { payload, key, .. }) => {
                    assert_eq!(key, "/Users/ada/home/Pictures");
                    Some(payload)
                }
                _ => None,
            })
            .collect();
        assert_eq!(sent.len(), 1, "ONE PUT per change");
        assert_eq!(
            sent[0],
            json!({"configured": true, "posture": "allowed_only", "default_mode": "rw",
                   "folders": [{"path": "/Users/ada/home/Pictures", "mode": "deny"},
                               {"path": "/Users/ada/home/work/project", "mode": "rw"}]})
        );
    });
}

#[test]
fn my_default_workspaces_is_the_account_level() {
    per_size(|size| {
        let mut h = harness(size);
        h.command("/workspace");
        h.answer_session("session_get_default.json");
        h.scroll_end();
        h.cmds();
        let screen = h.keys(b"\r");
        assert!(screen.contains("My default workspaces"), "{screen}");
        assert!(
            h.cmds()
                .iter()
                .any(|c| matches!(c, Cmd::Workspaces(WsCmd::LoadAccount))),
            "GET /workspace/policy/me"
        );
        h.answer_account();
        let screen = h.shot("workspace-account");
        h.assert_rows(
            &screen,
            &[
                "The workspaces this account's agents use, among the eligible ones.",
                "[x] Follow the gateway policy",
            ],
        );
        // Esc goes back to the conversation's panel.
        let screen = h.esc();
        assert!(
            screen.contains("▸5 Workspace"),
            "back on the rail's Workspace panel: {screen}"
        );
        assert!(screen.contains("My default workspaces"), "{screen}");
    });
}

fn open_automation_workspace(h: &mut H) -> String {
    h.command(&format!("/automations {INBOX}"));
    let list = auto::parse_list_page(&read(AUTOS, "list.json")).unwrap();
    let summary = list.items.into_iter().find(|s| s.id == INBOX).unwrap();
    let definition = auto::Definition {
        revision: 3,
        workflow_id: "inbox@1.0.0:triage".into(),
        tool_approval: "ask".into(),
        growing: Default::default(),
        max_attempts: Some(3),
        workspace_root: summary.workspace_root.clone().unwrap(),
        target: json!({"bundle_ref": "inbox@1.0.0", "flow_id": "triage",
            "input_data": {"prompt": "List the new emails that need a reply today.",
                           "workspace": {"posture": "allowed_only", "default_mode": "rw",
                                         "folders": [{"path": "/Users/ada/home/work", "mode": "ro"}]}}}),
    };
    let page = auto::parse_occurrence_page(&read(AUTOS, "occurrences.json")).unwrap();
    h.store
        .automations
        .update(|v| v.apply_detail(INBOX, definition, summary, page));
    h.turn();
    h.keys(b"e");
    h.keys(b"5")
}

#[test]
fn an_automation_workspace_panel_is_the_run_level() {
    per_size(|size| {
        let mut h = harness(size);
        open_automation_workspace(&mut h);
        let key = json!({"posture": "allowed_only", "default_mode": "rw",
                         "folders": [{"path": "/Users/ada/home/work", "mode": "ro"}]})
        .to_string();
        assert!(
            h.cmds()
                .iter()
                .any(|c| matches!(c, Cmd::Workspaces(WsCmd::DryRun { value: Some(_) }))),
            "POST /workspace/effective/me {{workspace}}"
        );
        h.answer_dry_run(&key, "dryrun_payload.json");
        let screen = h.shot("workspace-automation");
        h.assert_rows(
            &screen,
            &[
                "Automation Inbox triage",
                "The workspaces this run uses, among the eligible ones.",
                "[ ] Use my default",
            ],
        );
        if size.h >= 40 {
            h.assert_rows(&screen, &["Runs work in the automation folder"]);
        }
        let screen = h.scroll_end();
        h.shot("workspace-automation-end");
        h.assert_rows(
            &screen,
            &["Deny everything, allow listed workspaces · /Users/ada/home/work (ro)"],
        );
        assert!(!screen.contains("My default workspaces"), "{screen}");
    });
}

#[test]
fn the_new_automation_dialog_has_visible_workspaces_and_title_and_limits() {
    per_size(|size| {
        let mut h = harness(size);
        h.store.fold.update(|f| {
            f.push_item(Item::User {
                text: "report the free memory".into(),
            })
        });
        h.command("/schedule");
        h.keys(b"\r"); // the task
        h.keys(b"\r"); // every 24 hours
        h.keys(b"\r"); // independent
        let screen = h.keys(b"\r"); // run without asking
        assert!(screen.contains("5/6 Workspaces"), "{screen}");
        h.answer_dry_run("null", "dryrun_default.json");
        let screen = h.shot("schedule-5-workspaces");
        h.assert_rows(&screen, &["Continue — Title and limits"]);
        let screen = h.scroll_top();
        h.shot("schedule-5-workspaces-top");
        h.assert_rows(&screen, &["[x] Use my default", "Gateway: "]);
        h.scroll_end();
        let screen = h.keys(b"\r");
        h.shot("schedule-6-title-and-limits");
        h.assert_rows(
            &screen,
            &[
                "6/6 Title and limits",
                "Defaults to the task's first line",
                "First run at (UTC; empty = now)",
                "Stop after this many runs",
                "Stop at (UTC)",
                "Create automation",
            ],
        );
        assert!(!screen.contains("Advanced"), "{screen}");
    });
}

#[test]
fn tool_cards_show_the_gateway_sandbox_state() {
    per_size(|size| {
        let mut h = harness(size);
        let tools = abstractcode::discovery::tools_from_discovery(&read(
            WS,
            "discovery_tools_command_sandbox.json",
        ));
        assert_eq!(
            tools.iter().filter(|t| t.sandbox.is_some()).count(),
            3,
            "execute_command, local_helper_start, shell_exec"
        );
        let shown: Vec<ToolInfo> = tools
            .into_iter()
            .filter(|t| t.name != "shell_exec")
            .collect();
        h.store.tools.set(shown);
        h.command("/settings tools");
        let screen = h.scroll_end();
        h.shot("tools-sandbox-state");
        h.assert_rows(
            &screen,
            &[
                "execute_command",
                "Sandboxed to this run's workspaces",
                "Every command a run starts is confined by the operating system to that run's",
            ],
        );
        h.esc();
        let screen = h.command("/tools");
        h.shot("tools-modal-sandbox-state");
        h.assert_rows(&screen, &["[Sandboxed to this run's workspaces]"]);
    });
}

/// The recorded ledger folded the way the live stream folds it.
fn fold_sandbox_ledger() -> Vec<Item> {
    let v = read(WS, "sandbox_ledger.json");
    let records = v["records"].as_array().unwrap();
    let run = records[0]["run_id"].as_str().unwrap().to_string();
    let mut fold = Fold::new();
    fold.begin_run(&run);
    for r in records {
        fold.apply(&run, r);
    }
    fold.items
}

#[test]
fn the_run_view_shows_the_sandbox_line_per_command() {
    let items = fold_sandbox_ledger();
    let lines: Vec<String> = items
        .iter()
        .filter_map(|i| match i {
            Item::Tool { name, sandbox, .. } => Some(format!(
                "{name}: {}",
                sandbox.as_ref().map_or("-".into(), |s| s.line.clone())
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        lines,
        vec![
            "execute_command: Sandbox: macOS sandbox-exec · 4 workspaces enforced".to_string(),
            "execute_command: Sandbox: none — refused".to_string(),
            "read_file: -".to_string(),
        ]
    );
    per_size(|size| {
        let mut h = harness(size);
        h.store.fold.update(|f| {
            f.push_item(Item::User {
                text: "list my Desktop".into(),
            });
            for i in fold_sandbox_ledger() {
                f.push_item(i);
            }
        });
        let screen = h.shot("transcript-sandbox-line");
        h.assert_rows(
            &screen,
            &[
                "↳ Sandbox: macOS sandbox-exec · 4 workspaces enforced",
                "↳ Sandbox: none — refused",
            ],
        );
        // The rail's Activity groups carry the same line.
        let screen = h.command("/settings activity");
        h.shot("activity-sandbox-line");
        h.assert_rows(
            &screen,
            &["Sandbox: macOS sandbox-exec · 4 workspaces enforced"],
        );
    });
}
