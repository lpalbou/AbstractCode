//! The launch preflight (operator, fresh macOS install, 2026-09-28): a
//! TUI started without a credential opened anyway and said "no workflow
//! yet" and "session history not restored (HTTP 401) — retrying". The
//! binary now asks the gateway first: a refused credential ends the launch
//! with the exact way to sign in, on stderr, exit 1 — before any terminal
//! check, so this test needs no pty.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Command;

/// A gateway answering every request with `status_line`.
fn fake_gateway(status_line: &'static str) -> String {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for sock in l.incoming() {
            let Ok(mut sock) = sock else { continue };
            let mut reader = BufReader::new(sock.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            let body = r#"{"ok":true}"#;
            let _ = write!(
                sock,
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

/// Runs the TUI entry (no subcommand) hermetically: scratch HOME, no
/// inherited gateway/token settings. `extra` adds flags.
fn launch(url: &str, tag: &str, extra: &[&str]) -> (i32, String) {
    let home = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("signin-preflight-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&home).expect("scratch home");
    let mut args = vec!["--gateway-url", url, "--animation", "off"];
    args.extend_from_slice(extra);
    let out = Command::new(env!("CARGO_BIN_EXE_abstractcode"))
        .args(&args)
        .env("HOME", &home)
        .env_remove("ABSTRACTCODE_PREFS_FILE")
        .env_remove("ABSTRACTCODE_GATEWAY_CONNECTION_FILE")
        .env_remove("ABSTRACTCODE_GATEWAY_URL")
        .env_remove("ABSTRACTFLOW_GATEWAY_URL")
        .env_remove("ABSTRACTGATEWAY_URL")
        .env_remove("ABSTRACTCODE_GATEWAY_TOKEN")
        .env_remove("ABSTRACTGATEWAY_AUTH_TOKEN")
        .env_remove("ABSTRACTFLOW_GATEWAY_AUTH_TOKEN")
        .output()
        .expect("run abstractcode");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn a_gateway_refusing_the_credential_ends_the_launch_with_the_way_to_sign_in() {
    let url = fake_gateway("401 Unauthorized");
    let (code, stderr) = launch(&url, "401", &[]);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains(&format!(
            "abstractcode: not signed in to {url} (HTTP 401: this client has no token for it)."
        )),
        "{stderr}"
    );
    // Loopback gateway: the gateway's own no-token sign-in comes first.
    assert!(
        stderr.contains("  abstractgateway apps tui-command code"),
        "{stderr}"
    );
    // The URL came from the flag, so the login line keeps it.
    assert!(
        stderr.contains(&format!(
            "  abstractcode login --gateway-url {url} --token <value>"
        )),
        "{stderr}"
    );
    assert!(
        !stderr.contains("needs an interactive terminal"),
        "{stderr}"
    );
}

#[test]
fn a_refused_token_is_named_as_refused() {
    let url = fake_gateway("403 Forbidden");
    let (code, stderr) = launch(&url, "403", &["--token", "agw_wrong"]);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains("(HTTP 403: it refused the token this client sent)"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("agw_wrong"),
        "the token is never echoed: {stderr}"
    );
}

/// The absent case: a gateway that ACCEPTS the ping passes the preflight
/// (here the launch then stops at the terminal check — no pty in tests).
#[test]
fn a_signed_in_launch_passes_the_preflight() {
    let url = fake_gateway("200 OK");
    let (code, stderr) = launch(&url, "200", &[]);
    assert!(!stderr.contains("not signed in"), "{stderr}");
    assert_eq!(code, 2, "stopped at the tty check: {stderr}");
    assert!(stderr.contains("needs an interactive terminal"), "{stderr}");
}
