//! On/off rows in the terminal (operator rule 2026-09-30, shared contract §3):
//! a persistent on/off setting is a row labelled by the FEATURE with a state
//! marker — `[x] Active` highlighted (accent + bold) when on, `[ ] Active`
//! plain when off, `[-] Active — <reason>` dimmed when it cannot change now.
//! Space switches the selected row; hints say "space switch", never a
//! Pause/Resume verb pair. Driven through the REAL interface (AbstractTUI's
//! capture harness), like `automations_ui.rs`.
//!
//! `STATE_TOGGLE_SNAPSHOT_DIR=<dir> cargo test --test state_toggles_ui snapshots`
//! writes text/ANSI/SVG buffer snapshots (120x40 and 60x30) of every on/off
//! surface; without the variable that test only renders them.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;
use serde_json::Value;

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

fn harness(size: Size) -> Harness {
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
    h.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "settle".into(),
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

    fn auto_cmds(&mut self) -> Vec<AutoCmd> {
        let mut out = Vec::new();
        while let Ok(cmd) = self.rx.try_recv() {
            if let Cmd::Automations(c) = cmd {
                out.push(c);
            }
        }
        out
    }

    fn answer_list(&mut self) {
        let page = auto::parse_list_page(&fixture("list.json")).unwrap();
        self.store.automations.update(|v| {
            v.availability = Some(Ok(()));
            v.apply_list(page.items);
        });
        self.turn();
    }

    fn answer_detail(&mut self, id: &str) -> String {
        let list = auto::parse_list_page(&fixture("list.json")).unwrap();
        let summary = list.items.into_iter().find(|s| s.id == id).unwrap();
        let definition = auto::Definition {
            revision: 3,
            workflow_id: "inbox@1.0.0:triage".into(),
            tool_approval: "ask".into(),
            growing: Default::default(),
            max_attempts: Some(3),
            workspace_root: summary.workspace_root.clone().unwrap_or_default(),
            target: serde_json::Value::Null,
        };
        let page = auto::parse_occurrence_page(&fixture("occurrences.json")).unwrap();
        self.store
            .automations
            .update(|v| v.apply_detail(id, definition, summary, page));
        self.turn();
        self.turn()
    }

    fn set_tools(&mut self) {
        self.store.tools.set(vec![
            abstractcode::store::ToolInfo {
                name: "read_file".into(),
                description: "Read a file".into(),
                toolset: "files".into(),
                ..Default::default()
            },
            abstractcode::store::ToolInfo {
                name: "write_file".into(),
                description: "Write a file".into(),
                toolset: "files".into(),
                ..Default::default()
            },
        ]);
        self.store.disabled_tools.set(vec!["write_file".into()]);
    }

    fn set_skills(&mut self) {
        self.store.skills_catalog.set(vec![
            abstractcode::store::SkillInfo {
                name: "coredoc".into(),
                description: "Documentation discipline".into(),
                trust: "adopted".into(),
                blocked: false,
            },
            abstractcode::store::SkillInfo {
                name: "release".into(),
                description: "Release checklist".into(),
                trust: "adopted".into(),
                blocked: false,
            },
            abstractcode::store::SkillInfo {
                name: "sketchy".into(),
                description: "Not trusted".into(),
                trust: "unknown".into(),
                blocked: true,
            },
        ]);
        self.store.selected_skills.set(vec!["coredoc".into()]);
    }

    /// The screen row holding `needle` and the cell where it starts.
    fn find(&self, needle: &str) -> Option<(i32, i32)> {
        let text = self.term.screen().to_text();
        for (y, line) in text.lines().enumerate() {
            if let Some(byte) = line.find(needle) {
                let x = line[..byte].chars().count() as i32;
                return Some((x, y as i32));
            }
        }
        None
    }

    /// Bold + foreground of the cell at the first character of `needle`.
    fn style_at(&self, needle: &str) -> (bool, Option<Rgba>) {
        let (x, y) = self.find(needle).unwrap_or_else(|| {
            panic!(
                "{needle:?} not on screen:\n{}",
                self.term.screen().to_text()
            )
        });
        let shot = self.term.screen();
        let cell = shot.cell(x, y).expect("cell");
        (
            cell.paint
                .attrs
                .contains(abstracttui::testing::grid::Attrs::BOLD),
            cell.paint.fg,
        )
    }
}

fn accent() -> Option<Rgba> {
    Some(abstracttui::app::current_theme().tokens.accent)
}

fn command_types(cmds: Vec<AutoCmd>) -> Vec<String> {
    cmds.into_iter()
        .filter_map(|c| match c {
            AutoCmd::Command { command_type, .. } => Some(command_type),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Automations: the "Active" switch
// ---------------------------------------------------------------------------

#[test]
fn automation_cards_carry_their_active_switch() {
    let mut h = harness(Size::new(160, 40));
    h.command("/automations");
    h.answer_list();
    let screen = h.turn();
    // Each card: its name, then `↻ cadence · last`, then `next …` with the
    // `[x] Active` switch at the right — ON highlighted, OFF plain, the
    // legacy one unavailable (dimmed).
    for title in ["AI news monitor", "Weekly journal monitor", "echo"] {
        assert!(screen.contains(title), "{screen}");
    }
    // Off the first card (the selection inks its own rows).
    let screen = h.keys(b"\x1b[B");
    assert_eq!(
        h.style_at("[x] Active"),
        (true, accent()),
        "ON = accent + bold"
    );
    let (bold, fg) = h.style_at("[ ] Active");
    assert!(!bold && fg != accent(), "OFF = plain");
    let (bold, fg) = h.style_at("[-] Active");
    assert!(
        !bold && fg == Some(abstracttui::app::current_theme().tokens.text_faint),
        "UNAVAILABLE = dimmed"
    );
    assert!(screen.contains("[x] Active"), "{screen}");
    assert!(screen.contains("[ ] Active"), "{screen}");
    assert!(screen.contains("[-] Active"), "{screen}");
    assert!(screen.contains("space Active"), "{screen}");
    assert!(
        !screen.contains("pause/resume"),
        "no verb pair in the hints:\n{screen}"
    );
}

#[test]
fn space_switches_active_pause_when_on_resume_when_off() {
    let mut h = harness(Size::new(160, 40));
    h.command("/automations");
    h.answer_list();
    h.auto_cmds();
    // Second row: AI news monitor, active → pause.
    h.keys(b"\x1b[B");
    h.keys(b" ");
    assert_eq!(command_types(h.auto_cmds()), vec!["automation.pause"]);
    let notice = h.store.automations.with_untracked(|v| v.notice.clone());
    assert_eq!(notice, "Pausing…");
    // Third row: Weekly journal monitor, paused → resume.
    h.store.automations.update(|v| v.busy = false);
    h.keys(b"\x1b[B");
    h.keys(b" ");
    assert_eq!(command_types(h.auto_cmds()), vec!["automation.resume"]);
    // Fourth row: Morning briefing (schedule@2 daily), active → pause.
    h.store.automations.update(|v| v.busy = false);
    h.keys(b"\x1b[B");
    h.keys(b" ");
    assert_eq!(command_types(h.auto_cmds()), vec!["automation.pause"]);
    // Fifth row: the legacy one cannot change; it says why and sends nothing.
    h.store.automations.update(|v| v.busy = false);
    h.keys(b"\x1b[B");
    let screen = h.keys(b" ");
    assert!(command_types(h.auto_cmds()).is_empty());
    assert!(screen.contains("Active: Legacy schedule"), "{screen}");
}

#[test]
fn automation_detail_shows_the_active_switch_and_space_switches_it() {
    let mut h = harness(Size::new(160, 40));
    h.command("/automations");
    h.answer_list();
    h.keys(b"\x1b[B\x1b[B\r");
    let paused = auto::parse_list_page(&fixture("list.json")).unwrap().items[2]
        .id
        .clone();
    let screen = h.answer_detail(&paused);
    assert!(
        screen.contains("[ ] Active — paused: scheduled runs are skipped"),
        "{screen}"
    );
    assert!(screen.contains("space Active"), "{screen}");
    assert!(!screen.contains("pause/resume"), "{screen}");
    h.auto_cmds();
    h.keys(b" ");
    assert_eq!(command_types(h.auto_cmds()), vec!["automation.resume"]);

    let mut h = harness(Size::new(160, 40));
    h.command("/automations");
    h.answer_list();
    h.keys(b"\x1b[B\r");
    let active = auto::parse_list_page(&fixture("list.json")).unwrap().items[1]
        .id
        .clone();
    let screen = h.answer_detail(&active);
    assert!(
        screen.contains("[x] Active — runs on its schedule"),
        "{screen}"
    );
    assert_eq!(h.style_at("[x] Active"), (true, accent()));
}

// ---------------------------------------------------------------------------
// /tools and /skills
// ---------------------------------------------------------------------------

#[test]
fn tools_rows_are_switches_labelled_by_the_tool() {
    let mut h = harness(Size::new(120, 40));
    h.set_tools();
    let screen = h.command("/tools");
    // Move the cursor off the first row so its own ink shows.
    h.keys(b"\x1b[B");
    assert!(screen.contains("[x] read_file"), "{screen}");
    assert!(screen.contains("[ ] write_file"), "{screen}");
    assert!(!screen.contains("[✓]"), "{screen}");
    assert_eq!(h.style_at("[x] read_file"), (true, accent()));
    let (bold, fg) = h.style_at("[ ] write_file");
    assert!(
        !bold && fg != accent() && fg != Some(abstracttui::app::current_theme().tokens.text_faint),
        "OFF is plain, not dimmed"
    );
    assert!(screen.contains("space switch"), "{screen}");
    assert!(!screen.contains("Space toggles"), "{screen}");
}

#[test]
fn skills_rows_are_switches_and_a_blocked_skill_says_why() {
    let mut h = harness(Size::new(120, 40));
    h.set_skills();
    let screen = h.command("/skills");
    h.keys(b"\x1b[B");
    assert!(screen.contains("[x] coredoc"), "{screen}");
    assert!(screen.contains("[ ] release"), "{screen}");
    assert!(
        screen.contains("[-] sketchy — blocked by the gateway"),
        "{screen}"
    );
    assert_eq!(h.style_at("[x] coredoc"), (true, accent()));
    let (bold, _) = h.style_at("[-] sketchy");
    assert!(!bold);
    assert!(screen.contains("space switch"), "{screen}");
}

// ---------------------------------------------------------------------------
// Snapshots (BEFORE/AFTER evidence)
// ---------------------------------------------------------------------------

fn write_snapshot(h: &Harness, dir: Option<&std::path::Path>, name: &str) {
    let Some(dir) = dir else { return };
    std::fs::create_dir_all(dir).unwrap();
    let shot = h.term.screen().screenshot();
    shot.write_text(dir.join(format!("{name}.txt"))).unwrap();
    shot.write_ansi(dir.join(format!("{name}.ansi"))).unwrap();
    shot.write_svg(dir.join(format!("{name}.svg"))).unwrap();
}

#[test]
fn snapshots() {
    let dir = std::env::var_os("STATE_TOGGLE_SNAPSHOT_DIR").map(std::path::PathBuf::from);
    for (w, hgt) in [(120, 40), (60, 30)] {
        let sz = format!("{w}x{hgt}");
        let d = dir.as_deref();
        // The list: an active (running), an active, a paused and a legacy row.
        let mut h = harness(Size::new(w, hgt));
        h.command("/automations");
        h.answer_list();
        write_snapshot(&h, d, &format!("automations-list.{sz}"));
        // One automation, active, then paused.
        h.keys(b"\r");
        h.answer_detail(INBOX);
        write_snapshot(&h, d, &format!("automation-active.{sz}"));
        let mut h = harness(Size::new(w, hgt));
        h.command("/automations");
        h.answer_list();
        h.keys(b"\x1b[B\x1b[B\r");
        let paused = auto::parse_list_page(&fixture("list.json")).unwrap().items[2]
            .id
            .clone();
        h.answer_detail(&paused);
        write_snapshot(&h, d, &format!("automation-paused.{sz}"));
        // Tools and skills.
        let mut h = harness(Size::new(w, hgt));
        h.set_tools();
        h.command("/tools");
        write_snapshot(&h, d, &format!("tools.{sz}"));
        let mut h = harness(Size::new(w, hgt));
        h.set_skills();
        h.command("/skills");
        write_snapshot(&h, d, &format!("skills.{sz}"));
    }
}
