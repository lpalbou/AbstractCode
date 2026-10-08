//! The account preferences lane (R17.1): one short-lived thread per action,
//! results posted to `store.account_workflow` through the wake handle
//! (worker threads never touch signals).
//!
//! Route — the SAME one the Code web, the Assistant and the console call
//! (R14 PREFERENCES API, R14-W2): `GET/PUT /accounts/me/preferences`. A
//! change is ONE PUT of this app's interface only; a refusal (400
//! `{detail: {reason: "preference_refused", message}}`) becomes "Not saved.
//! <message>" and nothing changes. 404 = a gateway older than the route.

use abstracttui::reactive::WakeHandle;
use serde_json::Value;

use crate::account_prefs::{self as ap, State};
use crate::gateway::{err_from_ureq, GatewayClient};
use crate::store::Store;

/// One command of the lane (sent through `Cmd::AccountPrefs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefCmd {
    /// Read the row; `device` = the old choice saved on this computer
    /// (`bundle:flow`), uploaded once when the account has none.
    Load { device: Option<String> },
    /// One change (`None` = the gateway default).
    Save { value: Option<String> },
}

/// A failed call: the HTTP status (when the gateway answered) and the
/// gateway's sentence (or the transport reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefError {
    pub status: Option<u16>,
    pub sentence: String,
}

impl GatewayClient {
    /// `GET /accounts/me/preferences` (`payload = None`) or one `PUT`.
    pub fn account_preferences(&self, payload: Option<&Value>) -> Result<Value, PrefError> {
        let path = ap::PATH;
        let method = if payload.is_some() { "PUT" } else { "GET" };
        let req = self.with_auth(
            self.agent
                .request(method, &self.url(path))
                .set("Accept", "application/json"),
        );
        let resp = match payload {
            Some(b) => req
                .set("Content-Type", "application/json")
                .send_string(&b.to_string()),
            None => req.call(),
        };
        match resp {
            Ok(r) => {
                let text = r.into_string().map_err(|e| PrefError {
                    status: None,
                    sentence: format!("{path}: read failed: {e}"),
                })?;
                serde_json::from_str(&text).map_err(|e| PrefError {
                    status: Some(200),
                    sentence: format!("{path}: invalid JSON: {e}"),
                })
            }
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                let sentence = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| crate::workspaces::error_sentence(&v))
                    .unwrap_or_else(|| {
                        let t = text.trim();
                        if t.is_empty() {
                            format!("{path} failed (HTTP {code})")
                        } else {
                            t.to_string()
                        }
                    });
                Err(PrefError {
                    status: Some(code),
                    sentence,
                })
            }
            Err(e) => Err(PrefError {
                status: None,
                sentence: err_from_ureq(path, e).compact_reason(),
            }),
        }
    }

    /// The account's row, read and checked (`Ok(None)` = the route is
    /// missing: a gateway older than 0.13.1).
    pub fn account_workflow_row(&self) -> Result<Option<ap::Row>, String> {
        match self.account_preferences(None) {
            Ok(v) => ap::row(&v).map(Some),
            Err(e) if e.status == Some(404) => Ok(None),
            Err(e) => Err(e.sentence),
        }
    }
}

/// What a load ends with: the row state and whether the old device choice
/// is settled (removed here).
pub fn load(client: &GatewayClient, device: Option<&str>) -> (State, bool) {
    let mut row = match client.account_preferences(None) {
        Ok(v) => match ap::row(&v) {
            Ok(r) => r,
            Err(e) => return (State::Error(e), false),
        },
        Err(e) if e.status == Some(404) => return (State::Unsupported, false),
        Err(e) => return (State::Error(e.sentence), false),
    };
    let mut clear = false;
    if let Some(device) = device.filter(|d| !d.is_empty()) {
        clear = true;
        if row.value.is_none() {
            match client
                .account_preferences(Some(&ap::put_body(Some(device))))
                .map_err(|e| (e.status, e.sentence))
                .and_then(|v| ap::row(&v).map_err(|e| (None, e)))
            {
                Ok(r) => row = r,
                // Refused: the old choice no longer runs — removed too.
                // Any other failure keeps it for the next start.
                Err((status, _)) => clear = status == Some(400),
            }
        }
    }
    (State::Ok(row), clear)
}

pub fn spawn(client: &GatewayClient, wake: WakeHandle, store: Store, cmd: PrefCmd) {
    let client = client.clone();
    let post = wake.clone();
    crate::runner::spawn_host_thread("preferences", wake, store, move || {
        run(&client, &post, store, cmd)
    });
}

fn run(client: &GatewayClient, wake: &WakeHandle, store: Store, cmd: PrefCmd) {
    match cmd {
        PrefCmd::Load { device } => {
            let (state, clear) = load(client, device.as_deref());
            wake.post(move || {
                store.account_workflow.update(|v| {
                    v.state = state;
                    if clear {
                        v.clear_device = true;
                    }
                })
            });
        }
        PrefCmd::Save { value } => {
            let out = client
                .account_preferences(Some(&ap::put_body(value.as_deref())))
                .map_err(|e| e.sentence)
                .and_then(|v| ap::row(&v));
            let note = ap::change_note(&out.as_ref().map(|_| ()).map_err(Clone::clone));
            wake.post(move || {
                store.account_workflow.update(|v| {
                    if let Ok(row) = out {
                        v.state = State::Ok(row);
                    }
                    v.busy = false;
                    v.note = Some(note);
                })
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Read, Write};

    const GET: &str = include_str!("../../tests/fixtures/account_prefs/get_default.json");
    const PUT_OK: &str = include_str!("../../tests/fixtures/account_prefs/put_coder.json");
    const REFUSED: &str = include_str!("../../tests/fixtures/account_prefs/put_refused_400.json");

    /// A local HTTP server answering `answers` in order (status, body);
    /// hands back every (request line, body) that went on the wire.
    fn server(
        answers: Vec<(&'static str, &'static str)>,
    ) -> (String, std::sync::mpsc::Receiver<(String, String)>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", l.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (status, body) in answers {
                let (mut sock, _) = l.accept().expect("accept");
                let mut reader = BufReader::new(sock.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
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
                let mut buf = vec![0u8; len];
                reader.read_exact(&mut buf).unwrap();
                let _ = tx.send((
                    first.trim_end().to_string(),
                    String::from_utf8(buf).unwrap(),
                ));
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes());
                let _ = sock.write_all(body.as_bytes());
            }
        });
        (url, rx)
    }

    #[test]
    fn the_row_is_read_from_the_account_route() {
        let (url, rx) = server(vec![("200 OK", GET)]);
        let c = GatewayClient::new(&url, Some("t"));
        let (state, clear) = load(&c, None);
        assert!(matches!(state, State::Ok(ref r) if r.value.is_none()));
        assert!(!clear);
        let (line, body) = rx.recv().unwrap();
        assert_eq!(line, "GET /api/gateway/accounts/me/preferences HTTP/1.1");
        assert!(body.is_empty());
        assert!(
            rx.try_recv().is_err(),
            "one request, no upload without a device choice"
        );
    }

    #[test]
    fn a_change_is_one_put_of_this_apps_interface() {
        let (url, rx) = server(vec![("200 OK", PUT_OK)]);
        let c = GatewayClient::new(&url, Some("t"));
        let v = c
            .account_preferences(Some(&ap::put_body(Some("coding-agent:coder"))))
            .unwrap();
        assert_eq!(
            ap::row(&v).unwrap().value.as_deref(),
            Some("coding-agent:coder")
        );
        let (line, body) = rx.recv().unwrap();
        assert_eq!(line, "PUT /api/gateway/accounts/me/preferences HTTP/1.1");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({"default_workflow": {"abstractcode.agent.v1": "coding-agent:coder"}})
        );
    }

    #[test]
    fn a_refusal_is_the_gateways_sentence() {
        let (url, _rx) = server(vec![("400 Bad Request", REFUSED)]);
        let c = GatewayClient::new(&url, Some("t"));
        let e = c
            .account_preferences(Some(&ap::put_body(Some("nope-bundle:nope"))))
            .unwrap_err();
        assert_eq!(e.status, Some(400));
        assert_eq!(
            ap::change_note(&Err(e.sentence)).1,
            "Not saved. default_workflow.abstractcode.agent.v1 = 'nope-bundle:nope' refused: workflow bundle 'nope-bundle' is not on this gateway."
        );
    }

    #[test]
    fn an_older_gateway_is_unsupported() {
        let (url, _rx) = server(vec![("404 Not Found", r#"{"detail":"Not Found"}"#)]);
        let c = GatewayClient::new(&url, Some("t"));
        assert_eq!(
            load(&c, Some("coding-agent:coder")),
            (State::Unsupported, false)
        );
    }

    #[test]
    fn the_device_choice_is_uploaded_once_when_the_account_has_none() {
        let (url, rx) = server(vec![("200 OK", GET), ("200 OK", PUT_OK)]);
        let c = GatewayClient::new(&url, Some("t"));
        let (state, clear) = load(&c, Some("coding-agent:coder"));
        assert!(clear, "uploaded → removed from this computer");
        assert!(
            matches!(state, State::Ok(ref r) if r.value.as_deref() == Some("coding-agent:coder"))
        );
        let _get = rx.recv().unwrap();
        let (line, body) = rx.recv().unwrap();
        assert_eq!(line, "PUT /api/gateway/accounts/me/preferences HTTP/1.1");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({"default_workflow": {"abstractcode.agent.v1": "coding-agent:coder"}})
        );
    }

    #[test]
    fn an_account_choice_made_elsewhere_wins_and_the_device_choice_goes() {
        let (url, rx) = server(vec![("200 OK", PUT_OK)]);
        let c = GatewayClient::new(&url, Some("t"));
        let (state, clear) = load(&c, Some("basic-agent:81795ea9"));
        assert!(clear);
        assert!(
            matches!(state, State::Ok(ref r) if r.value.as_deref() == Some("coding-agent:coder"))
        );
        let _get = rx.recv().unwrap();
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "no PUT"
        );
    }

    #[test]
    fn a_refused_upload_removes_the_device_choice_a_network_failure_keeps_it() {
        let (url, _rx) = server(vec![("200 OK", GET), ("400 Bad Request", REFUSED)]);
        let c = GatewayClient::new(&url, Some("t"));
        let (state, clear) = load(&c, Some("nope-bundle:nope"));
        assert!(clear);
        assert!(matches!(state, State::Ok(ref r) if r.value.is_none()));

        let (url, _rx) = server(vec![("200 OK", GET), ("503 Service Unavailable", "")]);
        let c = GatewayClient::new(&url, Some("t"));
        let (_, clear) = load(&c, Some("coding-agent:coder"));
        assert!(!clear, "kept for the next start");
    }
}
