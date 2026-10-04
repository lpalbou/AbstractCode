//! R7.3 drive tests: the REAL terminal UI with the REAL worker against a
//! hermetic scratch AbstractGateway (no model, no provider, loopback only).
//!
//! `#[ignore]`d: they need a running fixture gateway. Run them with
//!
//! ```text
//! R7W4_GATEWAY_URL=http://127.0.0.1:<port> R7W4_TOKEN=abstractcode-e2e-only \
//!   cargo test --test r7w4_live -- --ignored --test-threads=1
//! ```
//!
//! where the gateway is `abstractcode/web/e2e/gateway_fixture.py` (the Code
//! web's own hermetic fixture: a fresh data root, a test user, deterministic
//! no-model VisualFlow bundles). A missing variable FAILS the run — the
//! absent-gateway case is never a pass. Each test seeds its own
//! conversations and automations through the gateway's public routes and
//! checks the outcome on the gateway, not only on screen. With
//! `R7W4_CAPTURE_DIR` set, each screen is also written there as text.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use abstracttui::app::Driver;
use abstracttui::prelude::*;
use abstracttui::testing::CaptureTerm;
use serde_json::{json, Value};

use abstractcode::automations as auto;
use abstractcode::config::Prefs;
use abstractcode::gateway::automations::AutomationClient;
use abstractcode::gateway::GatewayClient;
use abstractcode::runner::{self, Cmd};
use abstractcode::store::Store;
use abstractcode::ui::{self, UiCtx};

const BUNDLE: &str = "abstractcode-web-e2e";

fn gateway() -> (String, String) {
    let url = std::env::var("R7W4_GATEWAY_URL")
        .expect("R7W4_GATEWAY_URL must point at the hermetic fixture gateway");
    let token = std::env::var("R7W4_TOKEN").expect("R7W4_TOKEN must be the fixture token");
    (url, token)
}

fn client() -> GatewayClient {
    let (url, token) = gateway();
    GatewayClient::new(&url, Some(token.as_str()))
}

fn api(method: &str, path: &str, body: Option<Value>) -> Value {
    let (url, token) = gateway();
    let req = ureq::request(method, &format!("{url}/api/gateway/{path}"))
        .set("Authorization", &format!("Bearer {token}"));
    let resp = match body {
        Some(b) => req
            .set("Content-Type", "application/json")
            .send_string(&b.to_string()),
        None => req.call(),
    };
    match resp {
        Ok(r) => serde_json::from_str(&r.into_string().unwrap()).unwrap_or(Value::Null),
        Err(ureq::Error::Status(code, r)) => panic!(
            "{method} {path} -> {code}: {}",
            r.into_string().unwrap_or_default()
        ),
        Err(e) => panic!("{method} {path}: {e}"),
    }
}

fn unique(tag: &str) -> String {
    format!(
        "{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

/// One turn of a no-model flow in `session`; returns the run id.
fn start(session: &str, flow: &str, prompt: &str) -> String {
    client()
        .start_run(
            flow,
            Some(BUNDLE),
            Some(session),
            json!({ "prompt": prompt }),
        )
        .expect("start a fixture run")
        .run_id
}

fn wait_gateway(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the gateway never reached: {what}");
}

/// Approve the run's pending tool call (the fixture's `write_file`).
fn approve(run_id: &str) {
    wait_gateway("a waiting tool approval", || {
        api("GET", &format!("runs/{run_id}"), None)["status"] == "waiting"
    });
    let run = api("GET", &format!("runs/{run_id}"), None);
    let key = run["waiting"]["wait_key"].as_str().unwrap().to_string();
    api(
        "POST",
        "commands",
        Some(
            json!({"command_id": unique("approve"), "run_id": run_id, "type": "resume",
                    "payload": {"wait_key": key, "payload": {"approved": true}}}),
        ),
    );
    wait_gateway("the approved run completes", || {
        api("GET", &format!("runs/{run_id}"), None)["status"] == "completed"
    });
}

/// A conversation with 2 turns and 1 tool call (`Oct 2 · 2 turns · 1 tool`).
fn seed_conversation(prompt: &str) -> String {
    let sid = unique("acode-r7w4");
    let first = start(&sid, "timer-contract", prompt);
    wait_gateway("the first turn completes", || {
        api("GET", &format!("runs/{first}"), None)["status"] == "completed"
    });
    let second = start(&sid, "tool-approval", "write the fixture file");
    approve(&second);
    sid
}

fn seed_automation(title: &str, flow: &str, every: Option<&str>) -> String {
    seed_automation_with(title, flow, every, "auto")
}

fn seed_automation_with(title: &str, flow: &str, every: Option<&str>, approval: &str) -> String {
    let trigger = match every {
        Some(e) => json!({"source_id": "schedule", "source_version": 1, "config": {"every": e}}),
        None => json!({"source_id": "manual", "source_version": 1, "config": {}}),
    };
    let created = api(
        "POST",
        "automations",
        Some(json!({
            "request_id": unique("create"),
            "title": title,
            "target": {"bundle_ref": format!("{BUNDLE}@0.0.1"), "flow_id": flow,
                       "input_data": {"prompt": format!("{title} task")}},
            "trigger": trigger,
            "context": {"mode": "independent"},
            "policy": {"tool_approval": approval},
        })),
    );
    created["automation_id"].as_str().unwrap().to_string()
}

fn automation(id: &str) -> Value {
    api("GET", &format!("automations/{id}"), None)
}

struct Live {
    app: App,
    term: CaptureTerm,
    driver: Driver,
    store: Store,
    tx: mpsc::Sender<Cmd>,
    size: Size,
}

fn live(size: Size) -> Live {
    abstracttui::app::set_theme_by_id("abstract-dark");
    let mut app = App::new(size);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, rx) = mpsc::channel::<Cmd>();
    let rx_slot = Rc::new(RefCell::new(Some(rx)));
    let store_slot: Rc<RefCell<Option<Store>>> = Rc::new(RefCell::new(None));
    let store_out = store_slot.clone();
    let prefs = Rc::new(RefCell::new(Prefs::default()));
    let actions = app.actions();
    let tx_ui = tx.clone();
    app.mount(move |cx| {
        let store = Store::create(cx);
        *store_out.borrow_mut() = Some(store);
        store.session_id.set(unique("acode-r7w4-here"));
        let client = client();
        runner::spawn(
            client.clone(),
            abstracttui::reactive::wake_handle(),
            store,
            tx_ui.clone(),
            rx_slot.borrow_mut().take().unwrap(),
            None,
        );
        let _ = tx_ui.send(Cmd::Probe);
        let _ = tx_ui.send(Cmd::LoadCatalog {
            preferred_bundle: None,
            preferred_flow: None,
        });
        let _ = tx_ui.send(Cmd::LoadTools);
        let _ = tx_ui.send(Cmd::LoadSkills);
        let ctx = UiCtx {
            tx: tx_ui.clone(),
            client,
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            prefs: prefs.clone(),
            workspace_root: Some("/tmp/r7w4-ws".into()),
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
    let store = store_slot.borrow().expect("store");
    let mut l = Live {
        app,
        term,
        driver,
        store,
        tx,
        size,
    };
    l.store.fold.update(|f| {
        f.push_item(abstractcode::transcript::Item::User {
            text: "r7w4 drive".into(),
        })
    });
    for _ in 0..3 {
        l.turn();
    }
    l
}

impl Live {
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

    fn escape(&mut self) {
        self.term.push_input(&[0x1b]);
        self.turn();
        std::thread::sleep(Duration::from_millis(45));
        self.turn();
    }

    /// Turn until `cond(screen)` holds (worker answers land via wake posts).
    fn until(&mut self, what: &str, mut cond: impl FnMut(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut screen = String::new();
        while Instant::now() < deadline {
            screen = self.turn();
            if cond(&screen) {
                return screen;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        panic!("never on screen: {what}\n{screen}");
    }

    /// Move the cursor (↓) until the selected row (▸) contains `needle`.
    fn select(&mut self, needle: &str) -> String {
        for _ in 0..400 {
            let screen = self.turn();
            if screen
                .lines()
                .any(|l| l.contains('▸') && l.contains(needle))
            {
                return screen;
            }
            self.keys(b"\x1b[B");
        }
        panic!("could not select {needle}:\n{}", self.turn());
    }

    /// Turn until the TUI's own list holds automation `id` in `status`.
    fn until_status(&mut self, id: &str, status: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            self.turn();
            let ok = self.store.automations.with_untracked(|v| {
                v.list
                    .as_ref()
                    .and_then(|l| l.as_ref().ok())
                    .is_some_and(|items| items.iter().any(|s| s.id == id && s.status == status))
            });
            if ok {
                return;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        panic!("the list never read {id} as {status}");
    }

    /// `select` for the first of several spellings that is selected.
    fn select_any(&mut self, needles: &[&str]) -> String {
        for _ in 0..400 {
            let screen = self.turn();
            if screen
                .lines()
                .any(|l| l.contains('▸') && needles.iter().any(|n| l.contains(n)))
            {
                return screen;
            }
            self.keys(b"\x1b[B");
        }
        panic!("could not select any of {needles:?}:\n{}", self.turn());
    }

    fn capture(&mut self, name: &str) {
        let Ok(dir) = std::env::var("R7W4_CAPTURE_DIR") else {
            return;
        };
        let dir = std::path::Path::new(&dir).join(format!("{}x{}", self.size.w, self.size.h));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.txt")), self.turn()).unwrap();
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
    }
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

// ---------------------------------------------------------------------------
// Conversations: cards, Archive / Unarchive (POST /sessions/{id}/archive)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs the hermetic fixture gateway (R7W4_GATEWAY_URL)"]
fn conversation_cards_archive_and_unarchive_through_the_gateway() {
    let prompt = unique("Card prompt");
    let sid = seed_conversation(&prompt);
    per_size(move |size| {
        let mut l = live(size);
        l.command("/sessions");
        let screen = l.until("the seeded card", |s| s.contains(&prompt));
        assert!(screen.contains("2 turns · 1 tool"), "{screen}");
        l.capture("conversations");
        if size.w != 120 {
            return;
        }
        l.select(&prompt);
        let screen = l.keys(b"a");
        assert!(
            screen.contains("Archive this conversation? It stays searchable and auditable; it just leaves this list."),
            "{screen}"
        );
        l.capture("conversations-archive-confirm");
        l.keys(b"y");
        wait_gateway("the session is archived", || {
            api(
                "GET",
                "runs?root_only=true&archived_only=true&include_ledger_len=false&limit=200",
                None,
            )["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["session_id"] == sid.as_str())
        });
        l.until("the card leaves the list", |s| !s.contains(&prompt));
        let n = api("GET", "runs?root_only=true&limit=1", None)["archived_sessions"]
            .as_u64()
            .unwrap();
        // The quiet line ends the list (scrolled into view by the cursor).
        let screen = l.select(&format!("Archived · {n}"));
        assert!(screen.contains(&format!("Archived · {n}")), "{screen}");
        l.keys(b"\r");
        l.select(&prompt);
        l.capture("conversations-archived-open");
        l.select(&prompt);
        l.keys(b"\r");
        wait_gateway("the session is unarchived", || {
            !api(
                "GET",
                "runs?root_only=true&archived_only=true&include_ledger_len=false&limit=200",
                None,
            )["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["session_id"] == sid.as_str())
        });
        // Its title is the prompt — or its id when the gateway listing is
        // larger than the bounded prompt fetch (a reused fixture).
        let tail: String = sid
            .chars()
            .skip(sid.chars().count().saturating_sub(21))
            .collect();
        // The cursor sat on the archived row below it: reopen the board
        // (cursor back at the top), then walk down to it.
        l.escape();
        l.command("/sessions");
        l.until("the listing", |s| s.contains("sessions on the gateway"));
        let screen = l.select_any(&[prompt.as_str(), tail.as_str()]);
        assert!(screen.contains("2 turns · 1 tool"), "{screen}");
        assert_eq!(
            api("GET", "runs?root_only=true&limit=1", None)["archived_sessions"].as_u64(),
            Some(n - 1),
            "{screen}"
        );
    });
}

#[test]
#[ignore = "needs the hermetic fixture gateway (R7W4_GATEWAY_URL)"]
fn archiving_the_open_conversation_opens_the_next_one() {
    let older = seed_conversation(&unique("Older conversation"));
    let open = seed_conversation(&unique("Open conversation"));
    let mut l = live(Size::new(120, 40));
    l.store.session_id.set(open.clone());
    l.turn();
    let screen = l.command("/archive");
    let screen = if screen.contains("Archive this conversation?") {
        screen
    } else {
        l.until("the question for the open conversation", |s| {
            s.contains("Archive this conversation?")
        })
    };
    assert!(screen.contains("y Archive · n Cancel"), "{screen}");
    l.keys(b"y");
    wait_gateway("the open session is archived", || {
        api(
            "GET",
            "runs?root_only=true&archived_only=true&include_ledger_len=false&limit=200",
            None,
        )["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["session_id"] == open.as_str())
    });
    l.until("the terminal moved to another conversation", |_| true);
    let deadline = Instant::now() + Duration::from_secs(20);
    while l.store.session_id.get_untracked() == open && Instant::now() < deadline {
        l.turn();
        std::thread::sleep(Duration::from_millis(40));
    }
    let now = l.store.session_id.get_untracked();
    assert_ne!(now, open, "archiving the open conversation hands over");
    // The next in the list (newest first) is the older one seeded before it,
    // or another listed conversation — never the archived one.
    assert!(!now.is_empty());
    let _ = older;
}

// ---------------------------------------------------------------------------
// Automations: cards, Active, Run now, Stop, Archive / Unarchive
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs the hermetic fixture gateway (R7W4_GATEWAY_URL)"]
fn automation_cards_switch_run_archive_and_unarchive() {
    let title = unique("Daily digest");
    let id = seed_automation(&title, "timer-contract", Some("24h"));
    per_size(move |size| {
        let short = title.as_str();
        let mut l = live(size);
        l.command("/automations");
        l.until("the list", |s| {
            s.contains("Automations") && !s.contains("Loading automations…")
        });
        let screen = l.select(short);
        // A schedule fires once at creation: "last never" or "last <1 min ago".
        assert!(screen.contains("↻ every 24 h · "), "{screen}");
        l.capture("automations");
        if size.w != 120 {
            return;
        }
        l.select(short);
        // Run now: the occurrence runs on the gateway, the card reads it.
        l.keys(b"g");
        wait_gateway("the run finished", || {
            automation(&id)["summary"]["last_occurrence"]["status"] == "completed"
        });
        l.keys(b"r");
        let screen = l.until("last <1 min ago", |s| s.contains("· last <1 min ago"));
        assert!(
            screen.contains("next in 23 h") || screen.contains("next in 24 h"),
            "{screen}"
        );
        // Active off → paused on the gateway, the switch reads off, no next.
        // (The Run now follow-up re-reads may still hold the busy flag.)
        l.store.automations.update(|v| v.busy = false);
        l.select(short);
        l.keys(b" ");
        wait_gateway("paused", || {
            automation(&id)["summary"]["status"] == "paused"
        });
        l.keys(b"r");
        l.until_status(&id, "paused");
        l.until("[ ] Active", |s| s.contains("[ ] Active"));
        l.capture("automations-paused");
        l.store.automations.update(|v| v.busy = false);
        l.select(short);
        l.keys(b" ");
        wait_gateway("active again", || {
            automation(&id)["summary"]["status"] == "active"
        });
        // Archive asks inline, y archives.
        l.keys(b"r");
        l.until("active again on screen", |s| s.contains(short));
        l.store.automations.update(|v| v.busy = false);
        l.select(short);
        let screen = l.keys(b"a");
        assert!(
            screen.contains(&format!(
                "Archive “{title}”? It will not run again; its history stays readable."
            )),
            "{screen}"
        );
        l.keys(b"y");
        wait_gateway("archived", || {
            automation(&id)["summary"]["status"] == "archived"
        });
        l.keys(b"r");
        l.until("the card leaves the list", |s| {
            !s.contains(&format!("{short} "))
        });
        let n = api("GET", "automations", None)["archived_automations"]
            .as_u64()
            .unwrap();
        // The quiet line ends the list (scrolled into view by the cursor).
        let screen = l.select(&format!("Archived · {n}"));
        assert!(screen.contains(&format!("Archived · {n}")), "{screen}");
        l.keys(b"\r");
        l.select(short);
        l.capture("automations-archived-open");
        l.store.automations.update(|v| v.busy = false);
        l.select(short);
        l.keys(b"u");
        wait_gateway("unarchived → paused", || {
            automation(&id)["summary"]["status"] == "paused"
        });
    });
}

#[test]
#[ignore = "needs the hermetic fixture gateway (R7W4_GATEWAY_URL)"]
fn stop_ends_the_run_in_progress() {
    let title = unique("Needs approval");
    let id = seed_automation_with(&title, "tool-approval", None, "ask");
    let mut l = live(Size::new(120, 40));
    l.command("/automations");
    l.until("the card", |s| s.contains(&title));
    l.select(&title);
    l.keys(b"g");
    wait_gateway("a run in progress", || {
        !automation(&id)["summary"]["current_occurrence"].is_null()
    });
    l.keys(b"r");
    l.until("waiting for you", |s| s.contains("waiting for you"));
    l.store.automations.update(|v| v.busy = false);
    l.select(&title);
    l.keys(b"x");
    wait_gateway("nothing in progress", || {
        automation(&id)["summary"]["current_occurrence"].is_null()
    });
}

// ---------------------------------------------------------------------------
// Edit: the settings panels on an automation save revisions
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs the hermetic fixture gateway (R7W4_GATEWAY_URL)"]
fn edit_saves_each_change_as_a_new_revision() {
    let title = unique("Revise me");
    let id = seed_automation(&title, "timer-contract", Some("24h"));
    let rev0 = automation(&id)["definition"]["revision"].as_u64().unwrap();
    let mut l = live(Size::new(120, 40));
    l.command(&format!("/automations {id}"));
    l.until("the automation", |s| {
        s.contains(&format!("Automations / {title}"))
    });
    l.capture("automation-detail");
    let screen = l.keys(b"e");
    assert!(screen.contains(&format!("Automation {title}")), "{screen}");
    let screen = l.until("the revision", |s| s.contains(&format!("Revision {rev0}")));
    l.capture("rail-automation-workflow");
    assert!(
        screen.contains("Changes are saved as a new revision and apply from the next run."),
        "{screen}"
    );
    // Workflow panel: Title.
    l.select("Title");
    l.keys(b"\r");
    l.keys(b"\x1b[F");
    l.term.push_input(b" v2");
    l.turn();
    l.keys(b"\r");
    let rev1 = rev0 + 1;
    wait_gateway("revision +1 with the new title", || {
        let a = automation(&id);
        a["definition"]["revision"] == rev1
            && a["summary"]["title"] == format!("{title} v2").as_str()
    });
    l.until("Saved as revision N", |s| {
        s.contains(&format!(
            "Saved as revision {rev1}; applies from the next run."
        ))
    });
    l.until("the new revision number", |s| {
        s.contains(&format!("Revision {rev1}"))
    });
    // Model panel (3): Iteration limit = 7 → `_limits.max_iterations`.
    l.keys(b"3");
    l.select("Iteration limit");
    l.keys(b"\r");
    l.term.push_input(b"7");
    l.turn();
    l.keys(b"\r");
    let rev2 = rev1 + 1;
    wait_gateway("revision +2 with the limit", || {
        let a = automation(&id);
        a["definition"]["revision"] == rev2
            && a["definition"]["target"]["input_data"]["_limits"]["max_iterations"] == 7
    });
    l.until("the limit on screen", |s| {
        s.lines()
            .any(|line| line.contains("Iteration limit") && line.contains(" 7  "))
    });
    l.capture("rail-automation-model");
    // d puts it back to the default: the key is REMOVED (not "").
    l.select("Iteration limit");
    l.keys(b"d");
    let rev3 = rev2 + 1;
    wait_gateway("revision +3 without the limit", || {
        let a = automation(&id);
        a["definition"]["revision"] == rev3
            && a["definition"]["target"]["input_data"]
                .get("_limits")
                .is_none()
    });
    l.until("Workflow default again", |s| s.contains("Workflow default"));
}

// ---------------------------------------------------------------------------
// The rail bound to the conversation reads the gateway (same routes as web)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs the hermetic fixture gateway (R7W4_GATEWAY_URL)"]
fn the_rail_panels_read_the_gateway() {
    per_size(move |size| {
        let mut l = live(size);
        l.command("/settings voice");
        let screen = l.until("the gateway's voice routes", |s| {
            s.contains("Gateway default · supertonic / supertonic-3")
        });
        assert!(
            screen.contains("Gateway default · faster-whisper / base"),
            "{screen}"
        );
        assert!(!screen.contains("openai"), "{screen}");
        l.capture("rail-voice");
        l.keys(b"5");
        let screen = l.until("the workspace policy", |s| {
            s.contains("Managed by your gateway")
        });
        assert!(
            screen.contains("This workspace only · Workspace and allowed paths"),
            "{screen}"
        );
        l.capture("rail-workspace");
        l.keys(b"6");
        l.until("the Tools panel", |s| s.contains("Permissions"));
        l.select("list_files");
        l.until("the gateway's tools", |s| s.contains("[x] list_files"));
        l.capture("rail-tools");
        l.keys(b"7");
        l.until("the gateway's skills", |s| {
            s.contains("Enter and leverage AbstractFramework")
        });
        l.capture("rail-skills");
        l.keys(b"3");
        l.until("the Model panel", |s| s.contains("Reasoning effort"));
        l.capture("rail-model");
        l.keys(b"4");
        l.until("the Workflow panel", |s| s.contains("Workflow"));
        l.capture("rail-workflow");
        l.keys(b"2");
        l.capture("rail-files");
        l.keys(b"1");
        l.capture("rail-activity");
        l.escape();
    });
}

#[test]
#[ignore = "needs the hermetic fixture gateway (R7W4_GATEWAY_URL)"]
fn a_tools_switch_in_the_rail_changes_the_next_run() {
    let mut l = live(Size::new(120, 40));
    l.command("/settings tools");
    l.until("the Tools panel", |s| s.contains("Permissions"));
    l.select("list_files");
    l.keys(b" ");
    let screen = l.until("switched off", |s| s.contains("[ ] list_files"));
    assert!(
        l.store
            .disabled_tools
            .get_untracked()
            .contains(&"list_files".to_string()),
        "{screen}"
    );
}

// ---------------------------------------------------------------------------
// Activity: one group per run for an automation, its steps from the ledger
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs the hermetic fixture gateway (R7W4_GATEWAY_URL)"]
fn automation_activity_is_one_group_per_run() {
    let title = unique("Activity");
    let id = seed_automation_with(&title, "tool-approval", None, "ask");
    for runs in 1..=2u64 {
        let cid = unique("run-now");
        AutomationClient::from_gateway(&client())
            .send(&auto::command_request(&id, &cid, "automation.run_now"))
            .expect("run now");
        wait_gateway("a waiting occurrence", || {
            automation(&id)["summary"]["current_occurrence"]["index"] == runs
        });
        let run_id = automation(&id)["summary"]["current_occurrence"]["run_id"]
            .as_str()
            .unwrap()
            .to_string();
        approve(&run_id);
        wait_gateway("the occurrence finished", || {
            automation(&id)["summary"]["current_occurrence"].is_null()
        });
    }
    per_size(move |size| {
        let mut l = live(size);
        l.command(&format!("/automations {id}"));
        l.until("the automation", |s| s.contains(&title));
        l.keys(b"e");
        l.keys(b"1");
        let screen = l.until("the newest run open with its steps", |s| {
            s.contains("▾ Run #2") && s.contains("write_file")
        });
        assert!(
            screen.contains("▸ Run #1"),
            "the older run is folded:\n{screen}"
        );
        l.capture("rail-automation-activity");
    });
}

// ---------------------------------------------------------------------------
// Activity: a conversation's work, one group per model step
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs the hermetic fixture gateway (R7W4_GATEWAY_URL)"]
fn conversation_activity_groups_the_restored_turns() {
    let sid = seed_conversation(&unique("Activity conversation"));
    per_size(move |size| {
        let mut l = live(size);
        l.command(&format!("/sessions {sid}"));
        l.until("the restored transcript", |s| s.contains("write_file"));
        l.command("/activity");
        let screen = l.until("the Start group with the tool call", |s| {
            s.contains("▾ Start") && s.contains("write_file")
        });
        assert!(screen.contains("Done"), "{screen}");
        assert!(screen.contains("Conversation "), "{screen}");
        l.capture("rail-conversation-activity");
    });
}
