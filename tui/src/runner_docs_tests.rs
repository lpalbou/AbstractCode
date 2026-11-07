use super::*;
use crate::store::Store;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Instant,
};

#[derive(Clone, Debug)]
struct Request {
    method: String,
    path: String,
    body: Value,
}
struct Server {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new(reply: impl Fn(&Request, usize) -> (u16, Value) + Send + 'static) -> Self {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let url = format!("http://{}", socket.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let ending = stop.clone();
        let join = thread::spawn(move || {
            while !ending.load(Ordering::Acquire) {
                let Ok((mut stream, _)) = socket.accept() else {
                    thread::sleep(Duration::from_millis(3));
                    continue;
                };
                // macOS (BSD) hands out accepted sockets that inherit the listener's non-blocking
                // mode; Linux does not. Reads below must block (with the timeout), or a request
                // that has not fully arrived fails with WouldBlock on a Mac.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':') {
                        if key.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap();
                        }
                    }
                }
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).unwrap();
                let words: Vec<_> = first.split_whitespace().collect();
                let req = Request {
                    method: words[0].into(),
                    path: words[1].into(),
                    body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
                };
                let index = {
                    let mut r = observed.lock().unwrap();
                    let n = r.len();
                    r.push(req.clone());
                    n
                };
                let (status, value) = reply(&req, index);
                let body = value.to_string();
                let _=write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
            }
        });
        Self {
            url,
            requests,
            stop,
            join: Some(join),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.join.take().unwrap().join().unwrap();
    }
}
fn drain_until(done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        abstracttui::reactive::drain_posted();
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}
fn start(server: &Server, inspect: impl FnOnce(Store)) {
    let (root, ()) = abstracttui::reactive::create_root(|cx| {
        let store = Store::create(cx);
        store.phase.set(crate::store::Phase::Running);
        store.run_id.set("coding-root".into());
        store.session_id.set("coding-session".into());
        store.docs_chat.update(|s| {
            s.session_id = "code-docs-assistant:test".into();
            s.operation = 7;
            s.busy = true;
            s.messages
                .push(("user".into(), "How do I open Tools?".into()));
        });
        ask(
            GatewayClient::new(&server.url, Some("fixture-token")),
            abstracttui::reactive::wake_handle(),
            store.docs_chat,
            "code-docs-assistant:test".into(),
            7,
            "How do I open Tools?".into(),
            crate::docs_chat::QuestionReference {
                corpus: None,
                conversation: "User: Main conversation question".into(),
            },
        );
        inspect(store);
        assert_eq!(store.run_id.get_untracked(), "coding-root");
        assert_eq!(store.session_id.get_untracked(), "coding-session");
        assert_eq!(store.phase.get_untracked(), crate::store::Phase::Running);
    });
    root.dispose();
}
fn normal(req: &Request) -> (u16, Value) {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/gateway/docs/corpus?app=code") => (
            200,
            json!({"text":"The Tools menu groups Tools, Skills and MCP."}),
        ),
        ("POST", "/api/gateway/runs/start") => (200, json!({"run_id":"docs-root"})),
        ("GET", "/api/gateway/runs/docs-root") => (
            200,
            json!({"status":"completed","output":{"response":"Open Tools in the top bar."}}),
        ),
        _ => (404, json!({"detail":"Unknown fixture route"})),
    }
}
#[test]
fn docs_use_the_shared_workflow_contract_and_do_not_change_the_coding_run() {
    let server = Server::new(|r, _| normal(r));
    start(&server, |store| {
        assert!(drain_until(|| !store.docs_chat.with_untracked(|s| s.busy)));
        let chat = store.docs_chat.get_untracked();
        assert_eq!(
            chat.messages.last().unwrap().1,
            "Open Tools in the top bar."
        );
        assert!(chat.error.is_empty());
        assert_eq!(chat.run_id.as_deref(), Some("docs-root"));
    });
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    let body = &requests[1].body;
    assert!(body["input_data"]["docs"]
        .as_str()
        .unwrap()
        .contains("Main conversation question"));
    assert!(body["input_data"]["docs"]
        .as_str()
        .unwrap()
        .contains("Tools menu groups"));
    assert_eq!(body["kind"], "docs");
    assert_eq!(body["bundle_id"], "docs-qa");
    assert_eq!(body["bundle_version"], "0.1.1");
    assert_eq!(body["flow_id"], "docsqa001");
    assert_eq!(body["session_id"], "code-docs-assistant:test");
    assert_eq!(body["input_data"]["use_session_history"], true);
    assert!(body["input_data"].get("tools").is_none());
    assert!(body["input_data"].get("workspace_root").is_none());
}
#[test]
fn missing_corpus_refuses_to_start_ungrounded_inference() {
    let server = Server::new(|_, _| (200, json!({"text":""})));
    start(&server, |store| {
        assert!(drain_until(|| !store.docs_chat.with_untracked(|s| s.busy)));
        assert!(store
            .docs_chat
            .with_untracked(|s| s.error.contains("no AbstractCode documentation")));
    });
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}
#[test]
fn failed_docs_run_is_visible_and_does_not_become_an_answer() {
    let server = Server::new(|r, _| {
        if r.path.ends_with("/docs-root") {
            (200, json!({"status":"failed"}))
        } else {
            normal(r)
        }
    });
    start(&server, |store| {
        assert!(drain_until(|| !store.docs_chat.with_untracked(|s| s.busy)));
        let chat = store.docs_chat.get_untracked();
        assert!(chat.error.contains("failed"));
        assert_eq!(chat.messages.len(), 1);
    });
}
#[test]
fn temporary_poll_failure_follows_the_same_run_without_another_inference() {
    let server = Server::new(|r, index| {
        if r.path.ends_with("/docs-root") && index == 2 {
            (503, json!({"detail":"restart"}))
        } else {
            normal(r)
        }
    });
    start(&server, |store| {
        assert!(drain_until(|| !store.docs_chat.with_untracked(|s| s.busy)));
        assert!(store
            .docs_chat
            .with_untracked(|s| s.error.is_empty() && s.messages.len() == 2));
    });
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == "POST")
            .count(),
        1
    );
}
#[test]
fn a_late_reply_cannot_replace_a_new_docs_conversation() {
    let server = Server::new(|r, _| {
        if r.path.ends_with("/docs-root") {
            thread::sleep(Duration::from_millis(100));
        }
        normal(r)
    });
    start(&server, |store| {
        store.docs_chat.set(Default::default());
        let fresh = store.docs_chat.get_untracked().session_id;
        assert!(drain_until(|| server.requests.lock().unwrap().len() == 3));
        for _ in 0..30 {
            thread::sleep(Duration::from_millis(10));
            abstracttui::reactive::drain_posted();
        }
        assert_eq!(store.docs_chat.get_untracked().session_id, fresh);
        assert!(store
            .docs_chat
            .with_untracked(|s| s.messages.is_empty() && s.run_id.is_none()));
    });
}
#[test]
fn identity_uses_only_gateway_principal_facts() {
    let account = crate::store::AccountIdentity::from_response(
        &json!({"principal":{"user_id":"admin","tenant_id":"default","token_fingerprint":"private"},"auth":{"token":"secret"}}),
    );
    assert_eq!(account.user_id, "admin");
    assert_eq!(account.tenant_id, "default");
    assert!(account.error.is_empty());
    assert!(!format!("{account:?}").contains("private"));
    assert!(!format!("{account:?}").contains("secret"));
}
