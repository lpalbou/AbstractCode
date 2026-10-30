use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Instant,
};
fn serve(replies: Vec<(u16, Value)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let mut paths = Vec::new();
        for (status, value) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0u8; 8192];
            let n = stream.read(&mut bytes).unwrap();
            paths.push(
                String::from_utf8_lossy(&bytes[..n])
                    .lines()
                    .next()
                    .unwrap()
                    .to_string(),
            );
            let body = value.to_string();
            write!(stream,"HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        paths
    });
    (url, handle)
}
fn wait(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        abstracttui::reactive::drain_posted();
        if done() {
            return;
        }
        assert!(Instant::now() < deadline, "sessions worker did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn metadata_preview_survives_a_failed_completeness_pass() {
    let (url, server) = serve(vec![
        (
            200,
            serde_json::json!({"items":[{"run_id":"r","session_id":"s","status":"completed","created_at":"2026-10-30"}],"has_more":true}),
        ),
        (503, serde_json::json!({"detail":"busy"})),
    ]);
    let (root, ()) = abstracttui::reactive::create_root(|cx| {
        let store = Store::create(cx);
        store.phase.set(Phase::Running);
        store.run_id.set("active-run".into());
        spawn_load_sessions(
            GatewayClient::new(&url, None),
            abstracttui::reactive::wake_handle(),
            store,
            5000,
        );
        wait(|| !store.session_refresh_error.get_untracked().is_empty());
        assert!(!store.session_refreshing.get_untracked());
        match store.session_index.get_untracked() {
            SessionIndex::Loaded {
                rows, truncated, ..
            } => {
                assert_eq!(rows.len(), 1);
                assert!(truncated);
            }
            other => panic!("preview lost: {other:?}"),
        }
        assert_eq!(store.run_id.get_untracked(), "active-run");
        assert_eq!(store.phase.get_untracked(), Phase::Running);
    });
    root.dispose();
    let paths = server.join().unwrap();
    assert_eq!(paths.len(), 2);
    assert!(paths[0].contains("limit=200"));
    assert!(paths[1].contains("limit=5000"));
    assert!(paths.iter().all(|p| p.contains("include_metrics=false")));
}
#[test]
fn a_failed_refresh_retains_the_previous_gateway_snapshot() {
    let (url, server) = serve(vec![(503, serde_json::json!({"detail":"busy"}))]);
    let (root, ()) = abstracttui::reactive::create_root(|cx| {
        let store = Store::create(cx);
        store.session_index.set(SessionIndex::Loaded {
            rows: vec![],
            truncated: false,
            labeled: 0,
            archived: 7,
        });
        spawn_load_sessions(
            GatewayClient::new(&url, None),
            abstracttui::reactive::wake_handle(),
            store,
            5000,
        );
        wait(|| !store.session_refresh_error.get_untracked().is_empty());
        assert!(matches!(
            store.session_index.get_untracked(),
            SessionIndex::Loaded { archived: 7, .. }
        ));
        assert!(!store.session_refreshing.get_untracked());
    });
    root.dispose();
    server.join().unwrap();
}
