//! `abstractcode exec --stream on` end to end against a FAKE gateway
//! (contract S): the binary runs as a child process with a scratch HOME,
//! the fake serves the catalog, capabilities, run start, the REST ledger
//! and the run's SSE stream with live `llm.delta` frames.
//!
//! Pinned: the start body carries `_runtime.stream: true` only when the
//! gateway advertises `streaming.deltas`; the reply prints to stdout as the
//! deltas arrive; the final answer is not printed a second time; against a
//! gateway WITHOUT the capability the flag is refused out loud, the key is
//! withheld, no SSE is opened and the answer prints in full.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const ANSWER: &str = "The answer is 42.";

struct Fake {
    url: String,
    start_body: Arc<Mutex<String>>,
    sse_hits: Arc<AtomicUsize>,
}

/// `tap`: the run under test is expected to open the SSE (the REST ledger
/// then waits for it, so live text provably precedes the answer).
fn fake_gateway(deltas: bool, tap: bool) -> Fake {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", l.local_addr().unwrap());
    let start_body = Arc::new(Mutex::new(String::new()));
    let sse_hits = Arc::new(AtomicUsize::new(0));
    // The REST ledger withholds the answer until the SSE stream has been
    // written, so the live text reaches stdout before the answer folds.
    let sse_done = Arc::new(AtomicBool::new(!tap));
    let answered = Arc::new(AtomicBool::new(false));
    let (sb, hits) = (start_body.clone(), sse_hits.clone());
    std::thread::spawn(move || {
        for sock in l.incoming() {
            let Ok(mut sock) = sock else { continue };
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; len];
            let _ = reader.read_exact(&mut body);
            let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
            let path = path.trim_start_matches("/api/gateway").to_string();
            let json = |sock: &mut std::net::TcpStream, status: &str, body: String| {
                let _ = write!(
                    sock,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            };
            if path.starts_with("/bundles") {
                json(
                    &mut sock,
                    "200 OK",
                    serde_json::json!({"items": [{
                        "bundle_id": "basic-agent",
                        "entrypoints": [{"flow_id": "main", "name": "main",
                                         "interfaces": ["abstractcode.agent.v1"]}]
                    }]})
                    .to_string(),
                );
            } else if path.starts_with("/discovery/capabilities") {
                let caps = if deltas {
                    serde_json::json!({"capabilities": {"streaming": {"deltas": true, "default": false}}})
                } else {
                    serde_json::json!({"capabilities": {}})
                };
                json(&mut sock, "200 OK", caps.to_string());
            } else if path.starts_with("/runs/start") {
                *sb.lock().unwrap() = String::from_utf8_lossy(&body).to_string();
                json(&mut sock, "200 OK", r#"{"run_id": "root1"}"#.into());
            } else if path.starts_with("/runs/root1/ledger/stream") {
                hits.fetch_add(1, Ordering::SeqCst);
                let sse = concat!(
                    "event: llm.delta\ndata: {\"kind\":\"llm.delta\",\"run_id\":\"root1\",\"root_run_id\":\"root1\",\"node_id\":\"reason\",\"call_id\":\"c1\",\"seq\":1,\"text\":\"The answer\",\"channel\":\"content\",\"snapshot\":false}\n\n",
                    "event: llm.delta\ndata: {\"run_id\":\"root1\",\"root_run_id\":\"root1\",\"node_id\":\"reason\",\"call_id\":\"c1\",\"seq\":2,\"text\":\"(thinking)\",\"channel\":\"reasoning\",\"snapshot\":false}\n\n",
                    "event: llm.delta\ndata: {\"run_id\":\"root1\",\"root_run_id\":\"root1\",\"node_id\":\"reason\",\"call_id\":\"c1\",\"seq\":3,\"text\":\" is 42.\",\"channel\":\"content\",\"snapshot\":false}\n\n",
                    "event: llm.delta_end\ndata: {\"run_id\":\"root1\",\"root_run_id\":\"root1\",\"node_id\":\"reason\",\"call_id\":\"c1\",\"seq\":4,\"reason\":\"completed\"}\n\n",
                    "event: done\ndata: {}\n\n",
                );
                let _ = write!(
                    sock,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",
                    sse.len()
                );
                let _ = sock.flush();
                // Let the tap parse + print before the answer can fold.
                std::thread::sleep(std::time::Duration::from_millis(400));
                sse_done.store(true, Ordering::SeqCst);
            } else if path.starts_with("/runs/root1/ledger") {
                let after: u64 = path
                    .split("after=")
                    .nth(1)
                    .and_then(|s| s.split('&').next())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let items = if sse_done.load(Ordering::SeqCst) && after == 0 {
                    answered.store(true, Ordering::SeqCst);
                    serde_json::json!([
                        {"run_id": "root1", "step_id": "c1", "node_id": "reason",
                         "status": "started", "effect": {"type": "llm_call", "payload": {}}},
                        {"run_id": "root1", "step_id": "c1", "node_id": "reason",
                         "status": "completed", "effect": {"type": "llm_call", "payload": {}},
                         "result": {}},
                        {"run_id": "root1", "step_id": "e1", "node_id": "end",
                         "status": "completed", "effect": {"type": "flow", "payload": {}},
                         "result": {"output": {"answer": ANSWER}}}
                    ])
                } else {
                    serde_json::json!([])
                };
                let n = items.as_array().map(Vec::len).unwrap_or(0) as u64;
                json(
                    &mut sock,
                    "200 OK",
                    serde_json::json!({"items": items, "next_after": after + n}).to_string(),
                );
            } else if path.starts_with("/runs/root1") {
                let status = if answered.load(Ordering::SeqCst) {
                    "completed"
                } else {
                    "running"
                };
                json(
                    &mut sock,
                    "200 OK",
                    serde_json::json!({"status": status}).to_string(),
                );
            } else {
                json(&mut sock, "404 Not Found", "{}".into());
            }
        }
    });
    Fake {
        url,
        start_body,
        sse_hits,
    }
}

fn run_exec(url: &str, stream: &str) -> (i32, String, String) {
    // Hermetic: a scratch HOME under the target dir — never the operator's
    // ~/.abstractcode — and no inherited gateway/prefs settings.
    let home = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("exec-stream-home-{}-{stream}", std::process::id()));
    std::fs::create_dir_all(&home).expect("scratch home");
    let out = Command::new(env!("CARGO_BIN_EXE_abstractcode"))
        .args([
            "exec",
            "what is the answer?",
            "--gateway",
            url,
            "--workflow",
            "basic-agent:main",
            "--no-workspace",
            "--no-project-context",
            "--stream",
            stream,
            "--timeout",
            "30",
        ])
        .env("HOME", &home)
        .env_remove("ABSTRACTCODE_PREFS_FILE")
        .env_remove("ABSTRACTCODE_GATEWAY_CONNECTION_FILE")
        .env_remove("ABSTRACTCODE_GATEWAY_TOKEN")
        .env_remove("ABSTRACTGATEWAY_AUTH_TOKEN")
        .env_remove("ABSTRACTFLOW_GATEWAY_AUTH_TOKEN")
        .output()
        .expect("run exec");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn exec_stream_on_prints_the_reply_live_and_the_answer_once() {
    let fake = fake_gateway(true, true);
    let (code, stdout, stderr) = run_exec(&fake.url, "on");
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let body: serde_json::Value =
        serde_json::from_str(&fake.start_body.lock().unwrap()).expect("start body");
    assert_eq!(
        body["input_data"]["_runtime"]["stream"],
        serde_json::json!(true),
        "{body}"
    );
    assert!(
        fake.sse_hits.load(Ordering::SeqCst) >= 1,
        "the tap opened the root stream"
    );
    assert!(
        stdout.contains(&format!("✎ {ANSWER}\n")),
        "the reply printed live:\n{stdout}"
    );
    assert!(
        !stdout.contains("(thinking)"),
        "reasoning never mixes into the reply:\n{stdout}"
    );
    assert_eq!(
        stdout.matches(ANSWER).count(),
        1,
        "the answer is printed ONCE (live), never repeated:\n{stdout}"
    );
    assert!(
        stdout.contains("━━━ answer ━━━ (streamed above)"),
        "{stdout}"
    );
    let live_at = stdout.find(ANSWER).unwrap();
    let block_at = stdout.find("━━━ answer ━━━").unwrap();
    assert!(
        live_at < block_at,
        "live text precedes the answer marker:\n{stdout}"
    );
}

#[test]
fn exec_stream_on_against_a_gateway_without_deltas_says_so_and_sends_nothing() {
    let fake = fake_gateway(false, false);
    let (code, stdout, stderr) = run_exec(&fake.url, "on");
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let body: serde_json::Value =
        serde_json::from_str(&fake.start_body.lock().unwrap()).expect("start body");
    assert!(
        body["input_data"]
            .get("_runtime")
            .and_then(|r| r.get("stream"))
            .is_none(),
        "no stream key for a gateway without the capability: {body}"
    );
    assert!(
        stderr.contains("--stream on") && stderr.contains("does not advertise live replies"),
        "{stderr}"
    );
    assert_eq!(
        fake.sse_hits.load(Ordering::SeqCst),
        0,
        "no tap without the capability"
    );
    assert!(
        stdout.contains(&format!("━━━ answer ━━━\n{ANSWER}")),
        "the answer prints in full:\n{stdout}"
    );
}

#[test]
fn exec_stream_off_sends_false_and_opens_no_stream() {
    let fake = fake_gateway(true, false);
    let (code, stdout, stderr) = run_exec(&fake.url, "off");
    assert_eq!(code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    let body: serde_json::Value =
        serde_json::from_str(&fake.start_body.lock().unwrap()).expect("start body");
    assert_eq!(
        body["input_data"]["_runtime"]["stream"],
        serde_json::json!(false),
        "{body}"
    );
    assert_eq!(fake.sse_hits.load(Ordering::SeqCst), 0);
    assert!(
        stdout.contains(&format!("━━━ answer ━━━\n{ANSWER}")),
        "{stdout}"
    );
}
