//! The workspaces lane (R14.4): one short-lived thread per action, results
//! posted to `store.workspaces` through the wake handle (worker threads
//! never touch signals).
//!
//! Routes — the SAME ones the Code web's kit chooser calls (R11 WORKSPACE
//! API — FINAL), nothing terminal-only:
//! - `GET/PUT /sessions/{id}/workspaces` — this conversation (session level);
//! - `GET/PUT /workspace/policy/me` — "My default workspaces" (account level);
//! - `POST /workspace/effective/me {workspace}` — the run level's dry run
//!   (an automation's workspaces; nothing stored).
//!
//! Each change is ONE request with the full body; a refusal (400
//! `{detail: {reason: "workspace_refused", message}}`) becomes the gateway's
//! sentence + "Not saved." under the control, and nothing changes.

use abstracttui::reactive::WakeHandle;
use serde_json::{json, Value};

use crate::gateway::{err_from_ureq, url_encode, GatewayClient};
use crate::store::Store;
use crate::workspaces::{self as ws, RunValue, Status};

/// What a run-level change does once the gateway's dry run accepts it.
#[derive(Debug, Clone)]
pub enum RunCommit {
    /// A new automation: the value becomes the dialog's draft.
    Draft,
    /// An existing automation: saved as a new revision (`PATCH
    /// /automations/{id}` with `expected_revision`; `changes` already
    /// carry the value).
    Revision {
        id: String,
        command_id: String,
        expected_revision: u64,
        changes: Value,
    },
}

/// One command of the workspaces lane (sent through `Cmd::Workspaces`).
#[derive(Debug, Clone)]
pub enum WsCmd {
    /// `GET /sessions/{id}/workspaces`.
    LoadSession { session_id: String },
    /// `PUT /sessions/{id}/workspaces` with `payload` (control `key`).
    SaveSession {
        session_id: String,
        payload: Value,
        key: String,
    },
    /// `GET /workspace/policy/me`.
    LoadAccount,
    /// `PUT /workspace/policy/me`.
    SaveAccount { payload: Value, key: String },
    /// The run level's dry run for `value` (shown, nothing changes).
    DryRun { value: Option<RunValue> },
    /// A run-level change: dry run `value`; on success commit it.
    RunChange {
        value: Option<RunValue>,
        key: String,
        commit: RunCommit,
    },
}

/// The scope tag of each chooser (statuses never cross choosers).
pub fn session_scope(session_id: &str) -> String {
    format!("session:{session_id}")
}
pub const ACCOUNT_SCOPE: &str = "account";
pub const RUN_SCOPE: &str = "run";

pub fn spawn(client: &GatewayClient, wake: WakeHandle, store: Store, cmd: WsCmd) {
    let client = client.clone();
    let post = wake.clone();
    crate::runner::spawn_host_thread("workspaces", wake, store, move || {
        run(&client, &post, store, cmd)
    });
}

impl GatewayClient {
    /// One workspace request: the parsed answer, or the gateway's sentence
    /// (`detail.message` of a refusal, `detail` of a 403) / the transport
    /// reason.
    fn workspace_call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let mut req = self.with_auth(
            self.agent
                .request(method, &self.url(path))
                .set("Accept", "application/json"),
        );
        let resp = match body {
            Some(b) => {
                req = req.set("Content-Type", "application/json");
                req.send_string(&b.to_string())
            }
            None => req.call(),
        };
        match resp {
            Ok(r) => {
                let text = r
                    .into_string()
                    .map_err(|e| format!("{path}: read failed: {e}"))?;
                serde_json::from_str(&text).map_err(|e| format!("{path}: invalid JSON: {e}"))
            }
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                let sentence = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| ws::error_sentence(&v));
                Err(sentence.unwrap_or_else(|| {
                    let t = text.trim();
                    if t.is_empty() {
                        format!("{path} failed (HTTP {code})")
                    } else {
                        t.to_string()
                    }
                }))
            }
            Err(e) => Err(err_from_ureq(path, e).compact_reason()),
        }
    }

    /// `GET|PUT /sessions/{id}/workspaces`.
    pub fn session_workspaces(&self, session_id: &str, payload: Option<&Value>) -> Result<Value, String> {
        let path = format!("/sessions/{}/workspaces", url_encode(session_id));
        match payload {
            Some(p) => self.workspace_call("PUT", &path, Some(p)),
            None => self.workspace_call("GET", &path, None),
        }
    }

    /// `GET|PUT /workspace/policy/me`.
    pub fn account_workspaces(&self, payload: Option<&Value>) -> Result<Value, String> {
        match payload {
            Some(p) => self.workspace_call("PUT", "/workspace/policy/me", Some(p)),
            None => self.workspace_call("GET", "/workspace/policy/me", None),
        }
    }

    /// `POST /workspace/effective/me {workspace}` — the dry run.
    pub fn workspace_dry_run(&self, value: Option<&RunValue>) -> Result<Value, String> {
        self.workspace_call(
            "POST",
            "/workspace/effective/me",
            Some(&json!({"workspace": ws::run_value_json(value)})),
        )
    }
}

fn status(scope: &str, key: &str, out: &Result<(), String>) -> Status {
    match out {
        Ok(()) => Status {
            scope: scope.to_string(),
            key: key.to_string(),
            text: ws::SAVED.to_string(),
            error: false,
        },
        Err(sentence) => Status {
            scope: scope.to_string(),
            key: key.to_string(),
            text: ws::refusal(sentence),
            error: true,
        },
    }
}

fn run(client: &GatewayClient, wake: &WakeHandle, store: Store, cmd: WsCmd) {
    match cmd {
        WsCmd::LoadSession { session_id } => {
            let out = client
                .session_workspaces(&session_id, None)
                .and_then(|v| ws::as_state(&v))
                .map_err(|e| ws::load_error(&e));
            wake.post(move || {
                store.workspaces.update(|w| {
                    let key = format!("session:{session_id}");
                    w.loading.retain(|k| *k != key);
                    w.session = Some((session_id, out));
                })
            });
        }
        WsCmd::SaveSession {
            session_id,
            payload,
            key,
        } => {
            let scope = session_scope(&session_id);
            let out = client
                .session_workspaces(&session_id, Some(&payload))
                .and_then(|v| ws::as_state(&v));
            let st = status(&scope, &key, &out.as_ref().map(|_| ()).map_err(Clone::clone));
            wake.post(move || {
                store.workspaces.update(|w| {
                    if let Ok(state) = out {
                        w.session = Some((session_id, Ok(state)));
                    }
                    w.busy = None;
                    w.status = Some(st);
                })
            });
        }
        WsCmd::LoadAccount => {
            let out = client
                .account_workspaces(None)
                .and_then(|v| ws::as_state(&v))
                .map_err(|e| ws::load_error(&e));
            wake.post(move || {
                store.workspaces.update(|w| {
                    w.loading.retain(|k| k != "account");
                    w.account = Some(out);
                })
            });
        }
        WsCmd::SaveAccount { payload, key } => {
            let out = client
                .account_workspaces(Some(&payload))
                .and_then(|v| ws::as_state(&v));
            let st = status(ACCOUNT_SCOPE, &key, &out.as_ref().map(|_| ()).map_err(Clone::clone));
            let changed = out.is_ok();
            wake.post(move || {
                store.workspaces.update(|w| {
                    if let Ok(state) = out {
                        w.account = Some(Ok(state));
                    }
                    if changed {
                        // The session's "Use my default" view follows the account default.
                        w.account_tick += 1;
                        w.session = None;
                    }
                    w.busy = None;
                    w.status = Some(st);
                })
            });
        }
        WsCmd::DryRun { value } => {
            let key = ws::run_value_json(value.as_ref()).to_string();
            let out = client
                .workspace_dry_run(value.as_ref())
                .and_then(|v| ws::as_effective(&v))
                .map_err(|e| ws::load_error(&e));
            wake.post(move || {
                store.workspaces.update(|w| {
                    let loading = format!("run:{key}");
                    w.loading.retain(|k| *k != loading);
                    w.run = Some((key, out));
                })
            });
        }
        WsCmd::RunChange { value, key, commit } => {
            let value_key = ws::run_value_json(value.as_ref()).to_string();
            let dry = client
                .workspace_dry_run(value.as_ref())
                .and_then(|v| ws::as_effective(&v));
            let effective = match dry {
                Ok(e) => e,
                Err(sentence) => {
                    let st = status(RUN_SCOPE, &key, &Err(sentence));
                    wake.post(move || {
                        store.workspaces.update(|w| {
                            w.busy = None;
                            w.status = Some(st);
                        })
                    });
                    return;
                }
            };
            match commit {
                RunCommit::Draft => {
                    let st = status(RUN_SCOPE, &key, &Ok(()));
                    wake.post(move || {
                        store.workspaces.update(|w| {
                            w.run = Some((value_key, Ok(effective)));
                            w.draft = value;
                            w.busy = None;
                            w.status = Some(st);
                        })
                    });
                }
                RunCommit::Revision {
                    id,
                    command_id,
                    expected_revision,
                    changes,
                } => {
                    let saved = crate::gateway::rail::save_revision_now(
                        client,
                        wake,
                        store,
                        &id,
                        &command_id,
                        expected_revision,
                        changes,
                    );
                    let st = status(RUN_SCOPE, &key, &saved);
                    wake.post(move || {
                        store.workspaces.update(|w| {
                            if st.error {
                                // The stored value stands: its dry run is re-read by the view.
                            } else {
                                w.run = Some((value_key, Ok(effective)));
                            }
                            w.busy = None;
                            w.status = Some(st);
                        })
                    });
                }
            }
        }
    }
}
