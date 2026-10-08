//! R17.1 screens without a gateway (Code TUI automation parity, W1): the
//! status-line chip "Automations · N waiting" and its light poll, the
//! Workflow panel's "Default for new conversations" (the account's choice
//! kept by the gateway), and the `/sessions` search — rendered at 80×24 and
//! 120×40 from RECORDED route answers (`tests/fixtures/automations/`,
//! `tests/fixtures/account_prefs/`), applied to the store exactly as the
//! lanes' posted closures apply them. The commands the screens SEND are read
//! off the runner channel. With `R17W1_CAPTURE_DIR` set, each screen is
//! written there (`<w>x<h>/<name>.txt`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;
use serde_json::Value;

use abstractcode::account_prefs::{self as ap, State};
use abstractcode::automations as auto;
use abstractcode::config::Prefs;
use abstractcode::gateway::automations::AutoCmd;
use abstractcode::gateway::preferences::PrefCmd;
use abstractcode::runner::Cmd;
use abstractcode::store::{Conn, SessionIndex, SessionRow, SessionState, Store, Workflow};
use abstractcode::ui::{self, UiCtx};

const AUTOS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/automations");
const PREFS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/account_prefs");
const INBOX: &str = "53443dd0-25c4-5fa8-bdad-e1ac3fdfff8e";
const NEWS: &str = "fddce731-4abf-54d3-81b9-15856efbfd7a";
const SESSION: &str = "acode-r17-session";

fn read(dir: &str, name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(format!("{dir}/{name}")).unwrap()).unwrap()
}

struct H {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    rx: mpsc::Receiver<Cmd>,
    prefs: Rc<RefCell<Prefs>>,
    size: Size,
    clock: Rc<Cell<Instant>>,
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
    let prefs_ctx = prefs.clone();
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
            prefs: prefs_ctx.clone(),
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
    let mut driver = Driver::new(&mut app, &mut term, cfg).expect("driver");
    // A virtual clock: the poll's 15 s cadence is driven by hand.
    let clock = Rc::new(Cell::new(Instant::now()));
    let c = clock.clone();
    driver.set_clock(move || c.get());
    let store = slot.borrow().expect("store");
    let mut h = H {
        app,
        term,
        driver,
        store,
        rx,
        prefs,
        size,
        clock,
    };
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
        self.advance(Duration::from_millis(45));
        std::thread::sleep(Duration::from_millis(45));
        self.turn();
        self.turn()
    }
    fn advance(&mut self, d: Duration) {
        self.clock.set(self.clock.get() + d);
        self.turn();
    }
    fn cmds(&mut self) -> Vec<Cmd> {
        std::iter::from_fn(|| self.rx.try_recv().ok()).collect()
    }
    fn shot(&mut self, name: &str) -> String {
        let screen = self.turn();
        for line in screen.lines() {
            assert!(
                line.chars().count() <= self.size.w as usize,
                "a row overflows {}:\n{screen}",
                self.size.w
            );
        }
        if let Ok(dir) = std::env::var("R17W1_CAPTURE_DIR") {
            let d = std::path::Path::new(&dir).join(format!("{}x{}", self.size.w, self.size.h));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("{name}.txt")), &screen).unwrap();
        }
        screen
    }
    /// The gateway is reachable and signed in (the boot edge).
    fn connect(&mut self) {
        self.store.conn.set(Conn::Ok);
        self.turn();
    }
    /// `GET /automations` answered (as the attention lane posts it).
    /// `GET /automations` answered — POSTED into the loop like the lane's
    /// closure, so effects (the poll's timer) arm on the turn's clock.
    fn answer_list(&mut self, list: &Value) {
        let page = auto::parse_list_page(list).unwrap();
        let store = self.store;
        abstracttui::reactive::wake_handle()
            .post(move || store.automations.update(|v| v.apply_list(page.items)));
        self.turn();
        self.turn();
    }
    fn answer_prefs(&mut self, fixture: &str) {
        let row = ap::row(&read(PREFS, fixture)).unwrap();
        self.store.account_workflow.update(|v| {
            v.state = State::Ok(row);
            v.busy = false;
        });
        self.turn();
    }
    fn status_line(&mut self) -> String {
        let s = self.turn();
        s.lines().last().unwrap_or("").to_string()
    }
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
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

/// The screen as one line of words: rows joined, the settings rail's panel
/// column and the scrollbar removed (wrapped sentences read whole).
fn flat(screen: &str) -> String {
    let rows: Vec<String> = screen
        .lines()
        .map(|l| {
            let mut l = l.trim_end().trim_end_matches('█').trim_end().to_string();
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

fn per_size(body: impl Fn(Size) + Send + Sync + 'static) {
    let body = std::sync::Arc::new(body);
    for size in [Size::new(80, 24), Size::new(120, 40)] {
        let b = body.clone();
        if let Err(e) = std::thread::spawn(move || b(size)).join() {
            std::panic::resume_unwind(e);
        }
    }
}

/// The recorded list with every attention count set to zero.
fn quiet_list() -> Value {
    let mut v = read(AUTOS, "list.json");
    for row in v["items"].as_array_mut().unwrap() {
        row["attention"]["pending_waits"] = 0.into();
        row["attention"]["unseen_count"] = 0.into();
        row["attention"]["items"] = Value::Array(vec![]);
        row["attention"]["waits"] = Value::Array(vec![]);
    }
    v
}

/// The web header's sum, computed from the raw JSON (not by the code under test).
fn web_sum(v: &Value) -> u64 {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            r["attention"]["pending_waits"].as_u64().unwrap()
                + r["attention"]["unseen_count"].as_u64().unwrap()
        })
        .sum()
}

fn attention_reads(cmds: &[Cmd]) -> usize {
    cmds.iter()
        .filter(|c| matches!(c, Cmd::AutomationAttention { .. }))
        .count()
}

// -- the chip -------------------------------------------------------------------

#[test]
fn the_chip_is_the_web_headers_sum_and_opens_the_waiting_automation() {
    per_size(|size| {
        let mut h = harness(size);
        h.connect();
        let cmds = h.cmds();
        assert_eq!(attention_reads(&cmds), 1, "one read at the reachable edge");
        assert!(
            h.status_line().find("waiting").is_none(),
            "no answer yet: no chip"
        );

        // Inbox triage: 2 waits + 2 unseen; the others 0 → 4 (web: Σ pending + unseen).
        let mut list = read(AUTOS, "list.json");
        // Row 0: 2 waits, no unseen; row 1: 1 unseen — the sum is over ALL
        // rows, and the one with a WAIT is opened even though it has no unseen.
        list["items"][0]["attention"]["unseen_count"] = 0.into();
        list["items"][1]["attention"]["unseen_count"] = 1.into();
        let n = web_sum(&list);
        assert_eq!(n, 3);
        h.answer_list(&list);
        let line = h.status_line();
        assert!(line.contains("Automations · 3 waiting"), "{line}");
        h.shot("chip-status-line");

        // Enter on an empty prompt opens the detail of the one WITH A WAIT
        // (Inbox triage, row 0), even though row 1 has unseen items.
        h.cmds();
        let screen = h.keys(b"\r");
        let cmds = h.cmds();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, Cmd::Automations(AutoCmd::Open { id }) if id == INBOX)),
            "{cmds:?}"
        );
        assert!(screen.contains("Automations / Inbox triage"), "{screen}");
        h.shot("chip-enter-opens-detail");
        h.esc();
        h.esc();

        // Only unseen items left (no waits): it opens THAT one.
        let mut list = quiet_list();
        list["items"][1]["attention"]["unseen_count"] = 2.into();
        h.answer_list(&list);
        assert!(h.status_line().contains("Automations · 2 waiting"));
        h.cmds();
        h.keys(b"\r");
        let cmds = h.cmds();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, Cmd::Automations(AutoCmd::Open { id }) if id == NEWS)),
            "{cmds:?}"
        );
        h.esc();
        h.esc();

        // All zero: no chip, and Enter on an empty prompt opens nothing.
        h.answer_list(&quiet_list());
        let line = h.status_line();
        assert!(!line.contains("waiting"), "nothing waits: no chip ({line})");
        h.cmds();
        h.keys(b"\r");
        assert!(
            !h.cmds()
                .iter()
                .any(|c| matches!(c, Cmd::Automations(AutoCmd::Open { .. }))),
            "nothing to open"
        );
    });
}

#[test]
fn a_click_on_the_chip_opens_the_same_automation() {
    let mut h = harness(Size::new(120, 40));
    h.connect();
    h.answer_list(&read(AUTOS, "list.json"));
    let screen = h.turn();
    let last = screen.lines().count() as u16 - 1;
    let line = screen.lines().last().unwrap();
    let col = line
        .find("Automations")
        .map(|b| line[..b].chars().count())
        .unwrap() as u16
        + 3;
    h.cmds();
    // SGR mouse: press + release on the chip (1-based coordinates).
    h.term
        .push_input(format!("\x1b[<0;{};{}M", col + 1, last + 1).as_bytes());
    h.turn();
    h.term
        .push_input(format!("\x1b[<0;{};{}m", col + 1, last + 1).as_bytes());
    h.turn();
    let screen = h.turn();
    let cmds = h.cmds();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Cmd::Automations(AutoCmd::Open { id }) if id == INBOX)),
        "{cmds:?}\n{screen}"
    );
}

#[test]
fn a_page_without_attention_shows_no_chip_and_never_a_stale_count() {
    let mut h = harness(Size::new(80, 24));
    h.connect();
    h.answer_list(&read(AUTOS, "list.json"));
    assert!(h.status_line().contains("waiting"));
    // The next read fails to parse (`attention` missing): posted like the
    // overlay's failed read — the chip goes, it never keeps the old count.
    let mut bad = read(AUTOS, "list.json");
    bad["items"][0].as_object_mut().unwrap().remove("attention");
    let err = auto::parse_list_page(&bad).unwrap_err();
    h.store.automations.update(|v| v.list = Some(Err(err)));
    let line = h.status_line();
    assert!(!line.contains("waiting"), "{line}");
}

// -- the light poll --------------------------------------------------------------

#[test]
fn the_poll_runs_only_while_something_waits_or_an_active_one_asks() {
    let mut h = harness(Size::new(80, 24));
    h.connect();
    assert_eq!(attention_reads(&h.cmds()), 1);

    // All zero, nobody asks: no poll at all over a minute.
    h.answer_list(&quiet_list());
    for _ in 0..12 {
        h.advance(Duration::from_secs(5));
    }
    assert_eq!(
        attention_reads(&h.cmds()),
        0,
        "quiet: zero requests after the first read"
    );

    // An ACTIVE automation in Ask mode (its definition read once): ≤ 1 / 15 s.
    let items = auto::parse_list_page(&quiet_list()).unwrap().items;
    let store = h.store;
    abstracttui::reactive::wake_handle().post(move || {
        store
            .automation_ask
            .update(|a| a.merge(vec![(NEWS.into(), 1, true)], &items))
    });
    h.turn();
    h.turn();
    let mut per_step = Vec::new();
    for _ in 0..12 {
        h.advance(Duration::from_secs(5));
        per_step.push(attention_reads(&h.cmds()));
    }
    // Light: one read per 15 s (every 3rd step of 5 s) — never per tick.
    // (The FIRST fire may come early in this harness: posted jobs run
    // before the driver publishes the injected clock, so the timer arms on
    // an older time. Production arms on the wall clock.)
    let fires: Vec<usize> = per_step
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 0)
        .map(|(i, _)| i)
        .collect();
    assert!(per_step.iter().all(|n| *n <= 1), "{per_step:?}");
    assert!(
        fires.len() >= 4,
        "it polls while an active one asks: {per_step:?}"
    );
    assert!(
        fires.windows(2).skip(1).all(|w| w[1] - w[0] == 3),
        "every 15 s: {per_step:?}"
    );

    // It stops asking (paused): the poll stops.
    let mut paused = quiet_list();
    paused["items"][1]["status"] = "paused".into();
    h.answer_list(&paused);
    for _ in 0..12 {
        h.advance(Duration::from_secs(5));
    }
    assert_eq!(
        attention_reads(&h.cmds()),
        0,
        "a paused automation cannot stop for approval"
    );

    // Attention again (e.g. the overlay's refresh shows unseen items): resumes.
    let mut seen = quiet_list();
    seen["items"][0]["attention"]["unseen_count"] = 1.into();
    h.answer_list(&seen);
    let mut per_step = Vec::new();
    for _ in 0..7 {
        h.advance(Duration::from_secs(5));
        per_step.push(attention_reads(&h.cmds()));
    }
    let n: usize = per_step.iter().sum();
    assert!(
        (2..=3).contains(&n),
        "it resumes, ≤ 1 per 15 s: {per_step:?}"
    );
}

#[test]
fn the_poll_sends_the_known_revisions_so_definitions_are_read_once() {
    let mut h = harness(Size::new(80, 24));
    h.connect();
    h.cmds();
    let items = auto::parse_list_page(&read(AUTOS, "list.json"))
        .unwrap()
        .items;
    h.store.automation_ask.update(|a| {
        a.merge(
            vec![(INBOX.into(), 1, false), (NEWS.into(), 1, true)],
            &items,
        )
    });
    h.answer_list(&read(AUTOS, "list.json"));
    h.advance(Duration::from_secs(16));
    let cmds = h.cmds();
    let known = cmds
        .iter()
        .find_map(|c| match c {
            Cmd::AutomationAttention { known } => Some(known.clone()),
            _ => None,
        })
        .expect("a poll");
    assert!(known.contains(&(INBOX.to_string(), 1)) && known.contains(&(NEWS.to_string(), 1)));
}

// -- Default for new conversations -------------------------------------------------

#[test]
fn the_workflow_panel_reads_and_writes_the_account_default() {
    per_size(|size| {
        let mut h = harness(size);
        h.connect();
        let cmds = h.cmds();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, Cmd::AccountPrefs(PrefCmd::Load { device: None }))),
            "read at the reachable edge (no device choice to upload): {cmds:?}"
        );
        h.answer_prefs("get_default.json");
        let screen = h.command("/settings workflow");
        let f = flat(&screen);
        assert!(f.contains("Default for new conversations"), "{screen}");
        assert!(
            f.contains("Gateway default (Basic agent)"),
            "verbatim from the route:\n{screen}"
        );
        assert!(f.contains(&squash(ap::HELP)), "{screen}");
        h.shot("workflow-panel-account-default");

        // Enter on the row: the gateway's default first, then its choices.
        h.keys(b"\x1b[B");
        let screen = h.keys(b"\r");
        assert!(
            screen.contains("Default for new conversations —"),
            "{screen}"
        );
        let pos = |s: &str, n: &str| s.find(n).unwrap_or_else(|| panic!("{n} missing:\n{s}"));
        assert!(
            pos(&screen, "Gateway default (Basic agent)") < pos(&screen, "Coding agent (chat)")
        );
        h.shot("workflow-panel-account-default-picker");
        h.cmds();
        for _ in 0..3 {
            h.keys(b"\x1b[B");
        }
        let screen = h.keys(b"\r");
        let saves: Vec<_> = h
            .cmds()
            .into_iter()
            .filter(|c| matches!(c, Cmd::AccountPrefs(PrefCmd::Save { .. })))
            .collect();
        assert_eq!(saves.len(), 1, "ONE PUT per change");
        assert!(matches!(
            &saves[0],
            Cmd::AccountPrefs(PrefCmd::Save { value: Some(v) }) if v == "coding-agent:coder"
        ));
        assert!(flat(&screen).contains("Saving…"), "{screen}");

        // Refused: the gateway's sentence after "Not saved."; the previous
        // value still shown (no optimistic flip).
        let refused = read(PREFS, "put_refused_400.json");
        let sentence = refused["detail"]["message"].as_str().unwrap().to_string();
        h.store.account_workflow.update(|v| {
            v.busy = false;
            v.note = Some(ap::change_note(&Err(sentence.clone())));
        });
        let screen = h.turn();
        let f = flat(&screen);
        assert!(
            f.contains(&squash(&format!("Not saved. {sentence}"))),
            "{screen}"
        );
        assert!(f.contains("Gateway default (Basic agent)"), "{screen}");
        h.shot("workflow-panel-account-default-refused");

        // Saved: the new value, "Saved.".
        h.answer_prefs("put_coder.json");
        h.store
            .account_workflow
            .update(|v| v.note = Some(ap::change_note(&Ok(()))));
        let f = flat(&h.turn());
        assert!(
            f.contains("Coding agent (chat)") && f.contains("Saved."),
            "{f}"
        );
    });
}

#[test]
fn d_puts_the_account_default_back_to_the_gateway_default() {
    let mut h = harness(Size::new(120, 40));
    h.connect();
    h.answer_prefs("put_coder.json");
    h.command("/settings workflow");
    h.keys(b"\x1b[B");
    h.cmds();
    h.keys(b"d");
    let cmds = h.cmds();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Cmd::AccountPrefs(PrefCmd::Save { value: None }))),
        "{cmds:?}"
    );
}

#[test]
fn an_older_gateway_keeps_the_device_choice_and_says_so() {
    per_size(|size| {
        let mut h = harness(size);
        h.connect();
        h.store
            .account_workflow
            .update(|v| v.state = State::Unsupported);
        let screen = h.command("/settings workflow");
        assert!(flat(&screen).contains(&squash(ap::UNSUPPORTED)), "{screen}");
        h.shot("workflow-panel-older-gateway");
    });
}

#[test]
fn the_device_choice_is_offered_once_then_removed_here() {
    let mut h = harness(Size::new(80, 24));
    h.prefs
        .borrow_mut()
        .set_explicit_workflow("coding-agent", "coder");
    h.connect();
    let cmds = h.cmds();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            Cmd::AccountPrefs(PrefCmd::Load { device: Some(d) }) if d == "coding-agent:coder"
        )),
        "{cmds:?}"
    );
    // The lane settled the migration → the old choice leaves this computer.
    h.answer_prefs("put_coder.json");
    h.store.account_workflow.update(|v| v.clear_device = true);
    h.turn();
    assert!(h.prefs.borrow().uses_gateway_default_workflow());
    // A later reachable edge offers nothing.
    h.store.conn.set(Conn::Down("gone".into(), true));
    h.turn();
    h.connect();
    let cmds = h.cmds();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Cmd::AccountPrefs(PrefCmd::Load { device: None }))),
        "{cmds:?}"
    );
}

#[test]
fn a_fresh_conversation_starts_on_the_account_default_and_workflow_saves_nothing_here() {
    let mut h = harness(Size::new(80, 24));
    let basic = Workflow {
        bundle_id: "basic-agent".into(),
        flow_id: "81795ea9".into(),
        name: "Basic agent".into(),
        gateway_default: true,
        ..Workflow::default()
    };
    let coder = Workflow {
        bundle_id: "coding-agent".into(),
        flow_id: "coder".into(),
        name: "Coding agent (chat)".into(),
        ..Workflow::default()
    };
    h.store.gateway_default_workflow.set(Some(basic.clone()));
    h.store.workflows.set(vec![
        Workflow {
            gateway_default: false,
            ..basic.clone()
        },
        coder.clone(),
    ]);
    h.store.workflow.set(basic.clone());
    h.connect();
    h.answer_prefs("put_coder.json");
    let w = h.store.workflow.get_untracked();
    assert_eq!(
        (w.bundle_id.as_str(), w.flow_id.as_str()),
        ("coding-agent", "coder")
    );

    // /workflow picks THIS conversation's workflow; nothing saved here —
    // neither an explicit pick nor the Gateway default row.
    let before = h.prefs.borrow().workflow_preference();
    assert_eq!(before, (None, None));
    let screen = h.command("/workflow");
    assert!(
        flat(&screen).contains("This conversation's workflow."),
        "{screen}"
    );
    h.keys(b"\x1b[A");
    h.keys(b"\r"); // the row above the current one = Basic agent (explicit)
    let w = h.store.workflow.get_untracked();
    assert!(!w.gateway_default && w.bundle_id == "basic-agent", "{w:?}");
    assert_eq!(
        h.prefs.borrow().workflow_preference(),
        before,
        "an explicit pick saves nothing here"
    );
    h.prefs
        .borrow_mut()
        .set_explicit_workflow("coding-agent", "coder");
    h.command("/workflow");
    for _ in 0..4 {
        h.keys(b"\x1b[A");
    }
    h.keys(b"\r"); // the first row = Gateway default
    assert!(h.store.workflow.get_untracked().gateway_default);
    assert_eq!(
        h.prefs.borrow().workflow_preference(),
        (Some("coding-agent".into()), Some("coder".into())),
        "the Gateway default row saves nothing here either"
    );

    // A run started: a change of the default no longer moves it.
    h.store.run_id.set("run-1".into());
    h.answer_prefs("get_default.json");
    assert!(h.store.workflow.get_untracked().gateway_default);
}

// -- /sessions search ----------------------------------------------------------------

fn row(id: &str, at: &str, prompt: &str) -> SessionRow {
    SessionRow {
        id: id.into(),
        state: SessionState::Done,
        last_at: at.into(),
        turns: 1,
        first_run: String::new(),
        prompt: Some(prompt.into()),
        tools: None,
    }
}

fn board(h: &mut H) {
    h.command("/sessions");
    h.store.session_index.set(SessionIndex::Loaded {
        rows: vec![
            row(
                "acode-7f3a91",
                "2026-10-02T10:00:00Z",
                "Port the tests to rstest",
            ),
            row(
                "acode-c0ffee",
                "2026-10-01T09:00:00Z",
                "Summarise the RELEASE notes",
            ),
            row(
                "acode-b1d2e3",
                "2026-09-30T09:00:00Z",
                "Fix the release script",
            ),
        ],
        truncated: false,
        labeled: 3,
        archived: 2,
    });
    h.turn();
}

#[test]
fn sessions_search_filters_on_title_and_id_like_the_web() {
    per_size(|size| {
        let mut h = harness(size);
        board(&mut h);
        let screen = h.shot("sessions-board");
        assert!(
            screen.contains("Search conversations"),
            "the hint names `/`:\n{screen}"
        );
        // `/` then typing: case-insensitive on the title.
        h.keys(b"/");
        let screen = h.keys(b"release");
        assert!(screen.contains("Summarise the RELEASE notes"), "{screen}");
        assert!(screen.contains("Fix the release script"), "{screen}");
        assert!(!screen.contains("Port the tests"), "{screen}");
        assert!(
            flat(&screen).contains("Search conversations: release▏ · 2 of 3"),
            "{screen}"
        );
        assert!(
            screen.contains("Archived · 2"),
            "the archived line stays reachable:\n{screen}"
        );
        h.shot("sessions-search-title");
        // Letters type while searching: `a` never asks to archive.
        assert!(!flat(&screen).contains("Archive this conversation?"));
        // Backspace narrows back.
        for _ in 0..7 {
            h.keys(b"\x7f");
        }
        let screen = h.keys(b"C0FFEE");
        // An id fragment matches a card whose title does not contain it.
        assert!(screen.contains("Summarise the RELEASE notes"), "{screen}");
        assert!(!screen.contains("Fix the release script"), "{screen}");
        h.shot("sessions-search-id");
        // No match: the web's empty state.
        let screen = h.keys(b"zz");
        assert!(
            screen.contains("No conversations match your search."),
            "{screen}"
        );
        h.shot("sessions-search-empty");
        // Esc clears the search first, then closes.
        let screen = h.esc();
        assert!(screen.contains("Port the tests to rstest"), "{screen}");
        assert!(!screen.contains("Search conversations:"), "{screen}");
        let screen = h.esc();
        assert!(!screen.contains("Conversations"), "closed:\n{screen}");
    });
}

#[test]
fn enter_continues_the_filtered_conversation() {
    let mut h = harness(Size::new(120, 40));
    board(&mut h);
    h.keys(b"/");
    h.keys(b"rstest");
    h.cmds();
    h.keys(b"\r");
    assert_eq!(h.store.session_id.get_untracked(), "acode-7f3a91");
}
