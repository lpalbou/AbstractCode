//! The person's event stream client (backlog 1047) against a loopback fake:
//! the catch-up page route and the SSE tail resumed with `Last-Event-ID`.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use abstractcode::gateway::GatewayClient;
use serde_json::Value;

/// Each request's path and Last-Event-ID header.
type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// Serves connections; records each request's first line + Last-Event-ID.
fn fake(seen: Seen) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut last_id = None;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("last-event-id:") {
                    last_id = Some(v.trim().to_string());
                }
            }
            let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
            seen.lock().unwrap().push((path.clone(), last_id.clone()));
            if path.starts_with("/api/gateway/me/events/stream") {
                let from: u64 = last_id.as_deref().unwrap_or("0").parse().unwrap();
                let mut body = String::from(": stream user:default:alice\n\n");
                for s in (from + 1)..=(from + 2) {
                    body.push_str(&format!(
                        "id: {s}\nevent: attention.raised\ndata: {{\"type\":\"attention.raised\",\"sequence\":{s},\"data\":{{\"automation_id\":\"a\",\"seq\":{s}}}}}\n\n: keep-alive\n\n"
                    ));
                }
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            } else if path.starts_with("/api/gateway/me/events") {
                let body = r#"{"stream":"user:default:alice","events":[{"type":"run.started","sequence":1,"data":{"run_id":"r1"}},{"type":"run.finished","sequence":2,"data":{"run_id":"r1","status":"completed"}}],"next_after":2,"head":2,"more":false}"#;
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            } else {
                let _ = write!(
                    stream,
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
            }
        }
    });
    port
}

#[test]
fn catch_up_then_the_tail_resumes_after_the_last_event_read() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let port = fake(seen.clone());
    let client = GatewayClient::new(&format!("http://127.0.0.1:{port}"), Some("t"));
    let page = client.my_events(0, 1000).unwrap();
    let seqs: Vec<u64> = page["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["sequence"].as_u64().unwrap())
        .collect();
    assert_eq!(seqs, vec![1, 2]);
    let mut got: Vec<Value> = Vec::new();
    client
        .stream_my_events(2, || true, |e| got.push(e))
        .unwrap();
    assert_eq!(
        got.iter()
            .map(|e| e["sequence"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![3, 4]
    );
    let log = seen.lock().unwrap().clone();
    assert!(
        log[0]
            .0
            .starts_with("/api/gateway/me/events?after=0&limit=1000"),
        "{log:?}"
    );
    assert_eq!(
        log[1].1.as_deref(),
        Some("2"),
        "the tail resumes with Last-Event-ID: {log:?}"
    );
    // keep() false: the tail returns at once, nothing delivered.
    let mut none = 0;
    client.stream_my_events(4, || false, |_| none += 1).unwrap();
    assert_eq!(none, 0);
    std::thread::sleep(Duration::from_millis(10));
}

#[test]
fn an_older_gateway_answers_404() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}");
        }
    });
    let client = GatewayClient::new(&format!("http://127.0.0.1:{port}"), None);
    let err = client.my_events(0, 10).unwrap_err();
    assert_eq!(err.status, Some(404));
}
