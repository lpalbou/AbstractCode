//! R7.3 screens without a gateway: every Code TUI parity screen rendered at
//! 80×24 (the minimum) and 120×40 from the canonical fixtures, asserting the
//! Code WUI's wording and that nothing is cut. With `R7W4_CAPTURE_DIR` set,
//! each screen is written there as text (`<w>x<h>/<name>.txt`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;
use serde_json::{json, Value};

use abstractcode::automations as auto;
use abstractcode::config::Prefs;
use abstractcode::runner::Cmd;
use abstractcode::store::{SessionIndex, SessionRow, SessionState, Store, ToolInfo};
use abstractcode::transcript::{Item, ToolStatus};
use abstractcode::ui::{self, UiCtx};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/automations");
const INBOX: &str = "53443dd0-25c4-5fa8-bdad-e1ac3fdfff8e";

fn fixture(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(format!("{FIXTURES}/{name}")).unwrap()).unwrap()
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
        store.session_id.set("acode-screens-session".into());
        let ctx = UiCtx {
            tx,
            client: abstractcode::gateway::GatewayClient::new("http://127.0.0.1:1", None),
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            prefs: prefs.clone(),
            workspace_root: Some("/home/me/projects/parser".into()),
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
    h.store.fold.update(|f| {
        f.push_item(Item::User {
            text: "fix the parser".into(),
        });
        f.push_item(Item::Thinking {
            iteration: 1,
            content: "Reading the parser".into(),
            reasoning: String::new(),
            call: Default::default(),
        });
        f.push_item(Item::Tool {
            key: "r:n:0:a".into(),
            name: "read_file".into(),
            args_preview: "src/parser.rs".into(),
            args_full: String::new(),
            status: ToolStatus::Ok,
            result: String::new(),
            error: String::new(),
            sandbox: None,
        });
        f.push_item(Item::Thinking {
            iteration: 2,
            content: "Fixing the off-by-one".into(),
            reasoning: String::new(),
            call: Default::default(),
        });
        f.push_item(Item::Tool {
            key: "r:n:1:b".into(),
            name: "edit_file".into(),
            args_preview: "src/parser.rs".into(),
            args_full: String::new(),
            status: ToolStatus::Ok,
            result: String::new(),
            error: String::new(),
            sandbox: None,
        });
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
    fn drain(&mut self) {
        while self.rx.try_recv().is_ok() {}
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
        if let Ok(dir) = std::env::var("R7W4_CAPTURE_DIR") {
            let d = std::path::Path::new(&dir).join(format!("{}x{}", self.size.w, self.size.h));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("{name}.txt")), &screen).unwrap();
        }
        screen
    }
    fn answer_automations(&mut self, archived: bool) {
        let page = auto::parse_list_page(&fixture("list.json")).unwrap();
        let mut gone = page.items[2].clone();
        gone.status = "archived".into();
        gone.title = "Old weekly digest".into();
        gone.capabilities = vec!["unarchive".into()];
        self.store.automations.update(|v| {
            v.availability = Some(Ok(()));
            v.archived_count = if archived { 1 } else { 0 };
            v.archived = Some(Ok(vec![gone]));
            v.apply_list(page.items);
        });
        self.turn();
    }
    fn answer_detail(&mut self) {
        let list = auto::parse_list_page(&fixture("list.json")).unwrap();
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
                                          "provider": "lmstudio", "model": "qwen3-8b",
                                          "_runtime": {"provider": "lmstudio", "model": "qwen3-8b", "thinking": "high"},
                                          "_limits": {"max_iterations": 12},
                                          "skills": ["pdf"]}}),
            notify: serde_json::Value::Null,
        };
        let page = auto::parse_occurrence_page(&fixture("occurrences.json")).unwrap();
        self.store
            .automations
            .update(|v| v.apply_detail(INBOX, definition, summary, page));
        self.turn();
    }
    fn answer_panels(&mut self) {
        self.store
            .default_route
            .set(("lmstudio".into(), "qwen3-8b".into()));
        self.store.tools.set(vec![
            ToolInfo {
                name: "read_file".into(),
                ..Default::default()
            },
            ToolInfo {
                name: "write_file".into(),
                ..Default::default()
            },
        ]);
        self.store
            .skills_catalog
            .set(vec![abstractcode::store::SkillInfo {
                name: "pdf".into(),
                description: "Read and fill PDF forms.".into(),
                trust: "reviewed".into(),
                blocked: false,
            }]);
        self.store.skills_shelf.set(Some(Default::default()));
        self.store.rail.update(|r| {
            r.policy = Some(Ok(
                json!({"ok": true, "policy": {"client_workspace_scope_overrides": false,
                "allowed_access_modes": ["workspace_only", "workspace_or_allowed"], "mounts": []}}),
            ));
        });
        self.store
            .voice
            .defaults
            .set(abstractcode::voice::DefaultsState::Loaded(Box::new(
                abstractcode::voice::VoiceDefaults::from_json(&json!({
                    "tts": {"configured": true, "provider": "supertonic", "model": "supertonic-3"},
                    "stt": {"configured": true, "provider": "faster-whisper", "model": "large-v3"}})),
            )));
        self.turn();
    }
}

/// The screen's text as one line of words (wrapped sentences rejoin).
fn flat(screen: &str) -> String {
    screen
        .lines()
        .map(|l| l.trim_end().trim_end_matches('█').trim())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn sizes() -> [Size; 2] {
    [Size::new(80, 24), Size::new(120, 40)]
}

/// Run `body` once per size, each on its own thread: the reactive runtime
/// is per thread, and a second App on the same thread would run the first
/// one's pending timers (toasts) against its disposed scopes.
fn per_size(body: impl Fn(Size) + Send + Sync + 'static) {
    let body = std::sync::Arc::new(body);
    for size in sizes() {
        let b = body.clone();
        let out = std::thread::spawn(move || b(size)).join();
        if let Err(e) = out {
            std::panic::resume_unwind(e);
        }
    }
}

fn row(
    id: &str,
    state: SessionState,
    at: &str,
    turns: usize,
    tools: Option<u64>,
    prompt: &str,
) -> SessionRow {
    SessionRow {
        id: id.into(),
        state,
        last_at: at.into(),
        turns,
        first_run: String::new(),
        prompt: Some(prompt.into()),
        tools,
    }
}

#[test]
fn the_conversations_board_reads_like_the_web_sidebar() {
    per_size(move |size| {
        let mut h = harness(size);
        h.command("/sessions");
        let cmds: Vec<Cmd> = std::iter::from_fn(|| h.rx.try_recv().ok()).collect();
        assert!(cmds.iter().any(|c| matches!(c, Cmd::LoadSessions { .. })));
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                Cmd::Rail(abstractcode::gateway::rail::RailCmd::LoadArchived)
            )),
            "the archived ones are read too"
        );
        h.store.session_index.set(SessionIndex::Loaded {
            rows: vec![
                row(
                    "acode-a",
                    SessionState::Running,
                    "2026-10-02T09:00:00Z",
                    2,
                    Some(7),
                    "Port the tests to rstest",
                ),
                row(
                    "acode-b",
                    SessionState::Done,
                    "2026-10-01T09:00:00Z",
                    1,
                    Some(0),
                    "Summarise the release notes",
                ),
            ],
            truncated: false,
            labeled: 2,
            archived: 3,
        });
        let screen = h.shot("conversations");
        assert!(screen.contains("Port the tests to rstest"), "{screen}");
        assert!(screen.contains("2 turns · 7 tools"), "{screen}");
        assert!(screen.contains("1 turn"), "{screen}");
        assert!(
            !screen.contains("0 tools"),
            "no tool figure at 0:\n{screen}"
        );
        assert!(
            screen.contains("running"),
            "the live state at the right:\n{screen}"
        );
        assert!(screen.contains("Archived · 3"), "{screen}");
        let screen = h.keys(b"a");
        assert!(
            flat(&screen).contains("Archive this conversation? It stays searchable and auditable; it just leaves this list."),
            "{screen}"
        );
        assert!(screen.contains("y Archive · n Cancel"), "{screen}");
        h.shot("conversations-archive-confirm");
        h.drain();
        h.keys(b"y");
        let sent: Vec<Cmd> = std::iter::from_fn(|| h.rx.try_recv().ok()).collect();
        assert!(
            sent.iter().any(|c| matches!(c, Cmd::Rail(abstractcode::gateway::rail::RailCmd::SetArchived { session_id, archive: true, .. }) if session_id == "acode-a")),
            "y archives through the gateway: {sent:?}"
        );
        // The Archived · N line opens the archived rows, each with Unarchive.
        h.store.rail.update(|r| {
            r.archive_busy = false;
            r.archived = SessionIndex::Loaded {
                rows: vec![row(
                    "acode-z",
                    SessionState::Done,
                    "2026-09-30T09:00:00Z",
                    4,
                    Some(2),
                    "Old migration",
                )],
                truncated: false,
                labeled: 1,
                archived: 1,
            };
        });
        h.keys(b"\x1b[B\x1b[B");
        let screen = h.keys(b"\r");
        assert!(screen.contains("Old migration"), "{screen}");
        assert!(screen.contains("Unarchive"), "{screen}");
        h.shot("conversations-archived-open");
        h.drain();
        h.keys(b"\x1b[B");
        h.keys(b"u");
        let sent: Vec<Cmd> = std::iter::from_fn(|| h.rx.try_recv().ok()).collect();
        assert!(
            sent.iter().any(|c| matches!(c, Cmd::Rail(abstractcode::gateway::rail::RailCmd::SetArchived { session_id, archive: false, .. }) if session_id == "acode-z")),
            "{sent:?}"
        );
    });
}

#[test]
fn the_automations_list_reads_like_the_web_cards() {
    per_size(move |size| {
        let mut h = harness(size);
        h.command("/automations");
        h.answer_automations(true);
        let screen = h.shot("automations");
        assert!(screen.contains("↻ Every 8 hours (UTC) · last"), "{screen}");
        assert!(screen.contains("[x] Active"), "{screen}");
        assert!(screen.contains("e Edit"), "{screen}");
        let screen = h.keys(b"a");
        assert!(
            flat(&screen).contains(
                "Archive “Inbox triage”? It will not run again; its history stays readable."
            ),
            "{screen}"
        );
        h.shot("automations-archive-confirm");
        h.keys(b"n");
        // The quiet Archived · N line ends the list (scrolled into view).
        for _ in 0..5 {
            h.keys(b"\x1b[B");
        }
        let screen = h.keys(b"\r");
        assert!(screen.contains("Archived · 1"), "{screen}");
        assert!(screen.contains("Old weekly digest"), "{screen}");
        assert!(screen.contains("Unarchive"), "{screen}");
        h.shot("automations-archived-open");
    });
}

#[test]
fn the_automation_screen_has_the_web_header() {
    per_size(move |size| {
        let mut h = harness(size);
        h.command(&format!("/automations {INBOX}"));
        h.answer_detail();
        let screen = h.shot("automation-detail");
        assert!(screen.contains("Automations / Inbox triage"), "{screen}");
        assert!(screen.contains("[x] Active"), "{screen}");
        assert!(
            flat(&screen).contains("Every 30 minutes (UTC) · waiting since"),
            "{screen}"
        );
        assert!(screen.contains("g Run now"), "{screen}");
        assert!(screen.contains("x Stop"), "{screen}");
    });
}

#[test]
fn every_rail_panel_on_the_conversation() {
    per_size(move |size| {
        let mut h = harness(size);
        h.command("/settings");
        h.answer_panels();
        h.drain();
        let expect: [(&str, &[&str]); 8] = [
            (
                "activity",
                &[
                    "▸ Step 1",
                    "▾ Step 2",
                    "edit_file src/parser.rs · done",
                    "Done",
                ],
            ),
            ("files", &["Files", "Enter opens"]),
            (
                "model",
                &[
                    "Gateway default: lmstudio · qwen3-8b.",
                    "Reasoning effort",
                    "MTP depth",
                ],
            ),
            ("workflow", &["Workflow"]),
            (
                "workspace",
                &[
                    "Current workspace",
                    "parser",
                    // R14.4: the kit chooser (session level), loading from the gateway.
                    "Workspaces",
                    "Loading…",
                ],
            ),
            ("tools", &["Permissions", "2 / 2 enabled", "[x] read_file"]),
            ("skills", &["Skills", "[ ] pdf"]),
            (
                "voice",
                &[
                    "Gateway default · supertonic / supertonic-3",
                    "Gateway default · faster-whisper / large-v3",
                ],
            ),
        ];
        for (i, (name, needles)) in expect.iter().enumerate() {
            let screen = h.keys(format!("{}", i + 1).as_bytes());
            let screen = if needles.iter().all(|n| screen.contains(n)) {
                screen
            } else {
                h.turn()
            };
            for n in *needles {
                assert!(
                    screen.contains(n),
                    "{name}: {n:?} missing at {}x{}:\n{screen}",
                    size.w,
                    size.h
                );
            }
            assert!(screen.contains("Conversation fix the parser"), "{screen}");
            h.shot(&format!("rail-{name}"));
        }
    });
}

#[test]
fn every_rail_panel_on_an_automation() {
    per_size(move |size| {
        let mut h = harness(size);
        h.command(&format!("/automations {INBOX}"));
        h.answer_detail();
        h.keys(b"e");
        h.answer_panels();
        h.answer_detail();
        let expect: [(&str, &[&str]); 7] = [
            ("activity", &["▾ Run #7", "▸ Run #6"]),
            ("files", &["Automation folder"]),
            ("model", &["Custom", "lmstudio · qwen3-8b", "high", "12"]),
            ("workflow", &["Title", "Inbox triage", "Task"]),
            (
                "workspace",
                &["Runs work in the automation folder", "Workspaces"],
            ),
            (
                "tools",
                &["All tools", "Choose Custom to pick tools one by one."],
            ),
            ("skills", &["[x] pdf"]),
        ];
        for (i, (name, needles)) in expect.iter().enumerate() {
            let screen = h.keys(format!("{}", i + 1).as_bytes());
            for n in *needles {
                assert!(
                    screen.contains(n),
                    "{name}: {n:?} missing at {}x{}:\n{screen}",
                    size.w,
                    size.h
                );
            }
            assert!(screen.contains("Automation Inbox triage"), "{screen}");
            assert!(screen.contains("Revision 3"), "{screen}");
            h.shot(&format!("rail-automation-{name}"));
            if *name == "workflow" {
                // The rest of the definition form, scrolled into view.
                // (R17.2 added the limits and Mailbox rows below Tools: the
                // rows are read while walking down.)
                let mut seen = String::new();
                let mut screen = String::new();
                for _ in 0..9 {
                    screen = h.keys(b"\x1b[B");
                    seen.push_str(&flat(&screen));
                }
                for n in [
                    "Repeat every (UTC)",
                    "Every 30 minutes (UTC)",
                    "Context",
                    "Ask before each tool call",
                    "Stop after this many runs",
                    "Stop at (UTC)",
                    "Mailbox",
                    "Email result",
                ] {
                    assert!(
                        seen.contains(n),
                        "{n:?} at {}x{}:\n{screen}",
                        size.w,
                        size.h
                    );
                }
                h.shot("rail-automation-workflow-end");
                h.keys(b"4");
            }
        }
        // A switch saves a revision: the skill off → `skills` removed.
        h.drain();
        h.keys(b" ");
        let sent: Vec<Cmd> = std::iter::from_fn(|| h.rx.try_recv().ok()).collect();
        let changes = sent
            .iter()
            .find_map(|c| match c {
                Cmd::Rail(abstractcode::gateway::rail::RailCmd::SaveRevision {
                    expected_revision: 3,
                    changes,
                    ..
                }) => Some(changes.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("space saves a revision: {sent:?}"));
        assert!(
            changes["target"]["input_data"].get("skills").is_none(),
            "{changes}"
        );
        assert_eq!(
            changes["target"]["input_data"]["prompt"],
            "List the new emails that need a reply today."
        );
    });
}

/// Round 16 (R16.1) captures at three widths (wide / medium / narrow): the
/// list with a `schedule@2` daily card (served rule, served next run), the
/// When step's calendar kinds, the weekly `[x]` day toggles, the gateway's
/// preview (time-zone line + `first_run_sentence`) and Title and limits.
/// `R7W4_CAPTURE_DIR=<dir>` writes them as text.
#[test]
fn r16_calendar_when_and_served_cards_fit_every_width() {
    for size in [Size::new(160, 44), Size::new(104, 36), Size::new(48, 30)] {
        std::thread::spawn(move || {
            let mut h = harness(size);
            h.command("/automations");
            h.answer_automations(false);
            let screen = h.shot("r16-automations-list");
            assert!(flat(&screen).contains("Every 30 minutes (UTC)"), "{screen}");
            if size.w >= 104 {
                assert!(flat(&screen).contains("Morning briefing"), "{screen}");
                assert!(screen.contains("Every day at"), "{screen}");
            }
            let mut h = harness(size);
            h.command(&format!("/automations {INBOX}"));
            h.answer_detail();
            let screen = h.shot("r16-automation-detail");
            // The key-hint notes sit under the runs; a 30-row phone-width
            // terminal gives them no room (as before round 16).
            if size.w >= 104 {
                assert!(
                    flat(&screen).contains("Next scheduled run: 2026-09-27 09:00 Europe/Paris."),
                    "{screen}"
                );
            }
            let mut h = harness(size);
            h.store.workflow.update(|w| w.gateway_default = true);
            h.command("/schedule morning briefing");
            h.keys(b"\r"); // What: Continue
            let screen = h.shot("r16-when-kinds");
            assert!(screen.contains("Once at…"), "{screen}");
            // From Continue (the 15th selectable card under Repeat) up to Weekly
            // (on a 30-row terminal the list scrolls to it).
            let mut screen = String::new();
            for _ in 0..12 {
                screen = h.keys(b"\x1b[A");
            }
            assert!(screen.contains("( ) Weekly"), "{screen}");
            h.keys(b"\r"); // Weekly (Mon picked)
            for _ in 0..7 {
                h.keys(b"\x1b[A");
            }
            h.keys(b"\r"); // + Tue
            let screen = h.shot("r16-weekly-days");
            assert!(screen.contains("[x] Tue"), "{screen}");
            if size.h >= 36 {
                assert!(screen.contains("[x] Mon"), "{screen}");
            }
            let mut trigger = None;
            while let Ok(cmd) = h.rx.try_recv() {
                if let Cmd::Automations(abstractcode::gateway::automations::AutoCmd::Preview {
                    trigger: t,
                }) = cmd
                {
                    trigger = Some(t);
                }
            }
            let trigger = trigger.expect("schedule-preview asked");
            assert_eq!(
                trigger["config"],
                json!({"kind": "weekly", "days": ["mon", "tue"], "at": "08:00"})
            );
            let answer = json!({
                "trigger": trigger,
                "time_zone": "Europe/Paris",
                "schedule_rule_text": "Every Mon and Tue at 08:00 (Europe/Paris)",
                "schedule_text": "Every Mon and Tue at 08:00 (Europe/Paris) · next Mon 12 Oct 08:00",
                "next_run_at": "2026-10-12T06:00:00+00:00",
                "next_run_local": "2026-10-12T08:00:00+02:00",
                "first_run_sentence": "Runs every Mon and Tue at 08:00 (Europe/Paris), first run Mon 12 Oct 08:00."
            });
            let p = auto::parse_schedule_preview(&answer).unwrap();
            h.store
                .automations
                .update(|v| v.apply_preview(&trigger, auto::PreviewState::Ready(p)));
            let screen = h.shot("r16-when-served-preview");
            assert!(
                flat(&screen).contains("in Europe/Paris (your account's time zone)"),
                "{screen}"
            );
            h.keys(b"\r"); // When: Continue
            h.keys(b"\r"); // Context: Continue
            h.keys(b"\r"); // Tools: Continue
            h.keys(b"\r"); // Workspaces: Continue
            let screen = h.keys(b"\r"); // Mailbox → Title and limits
            assert!(screen.contains("7/7 Title and limits"), "{screen}");
            let screen = h.shot("r16-title-and-limits");
            assert!(
                flat(&screen).contains("first run Mon 12 Oct 08:00."),
                "{screen}"
            );
        })
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e));
    }
}
