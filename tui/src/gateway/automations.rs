//! Automations lane: the HTTP half of `/automations` (one short-lived
//! thread per action, so a slow gateway never starves the runner loop).
//!
//! Every route is built by `crate::automations` (`*_request`), so the wire
//! the contract tests pin is the wire sent here. Results reach the UI thread
//! only through `WakeHandle::post`, which applies them to
//! `Store::automations` (the engine rule: worker threads never touch
//! signals). Errors are the gateway's typed envelope (`ApiError`); a
//! transport failure keeps the action's id so a retry is idempotent.

use std::time::Duration;

use abstracttui::reactive::WakeHandle;
use serde_json::Value;

use crate::automations::{self as auto, ApiError, Request, Wait};
use crate::gateway::GatewayClient;
use crate::store::Store;

/// Re-reads after a command: the gateway ACCEPTS a command when it is queued
/// and the controller applies it moments later, so the first re-read may
/// still show the old state.
const FOLLOW_UPS: [Duration; 2] = [Duration::from_millis(1500), Duration::from_millis(2500)];

/// One command of the automations lane (sent through `Cmd::Automations`).
#[derive(Debug, Clone)]
pub enum AutoCmd {
    /// The whole list (every page) + the open automation, if any.
    Refresh {
        open: Option<String>,
    },
    /// Open one automation: its detail and newest occurrence page.
    Open {
        id: String,
    },
    /// The next older occurrence page.
    More {
        id: String,
        cursor: String,
    },
    /// `automation.*` command.
    Command {
        id: String,
        command_id: String,
        command_type: String,
    },
    Revise {
        id: String,
        command_id: String,
        expected_revision: Option<u64>,
        changes: Value,
    },
    Create {
        body: Value,
    },
    Discuss {
        id: String,
        request_id: String,
        index: u64,
        prompt: String,
    },
    Seen {
        id: String,
        cursor: String,
    },
    Answer {
        id: String,
        command_id: String,
        wait: Box<Wait>,
        payload: Value,
    },
}

#[derive(Clone)]
pub struct AutomationClient {
    base_url: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl AutomationClient {
    pub fn from_gateway(client: &GatewayClient) -> AutomationClient {
        let (base_url, token) = client.connection();
        // `#[WARNING:TIMEOUT]` automations control plane (ADR-0027 §4):
        // short JSON reads/writes only — never a model call (occurrences run
        // on the gateway; this lane only reads their records).
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(60))
            .timeout_write(Duration::from_secs(30))
            .build();
        AutomationClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            agent,
        }
    }

    /// Send one request; a 2xx answer must be a JSON object.
    pub fn send(&self, r: &Request) -> Result<Value, ApiError> {
        let url = format!("{}{}", self.base_url, r.path);
        let mut req = self
            .agent
            .request(r.method, &url)
            .set("Accept", "application/json");
        if let Some(t) = &self.token {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        let result = match &r.body {
            Some(body) => req
                .set("Content-Type", "application/json")
                .send_string(&body.to_string()),
            None => req.call(),
        };
        match result {
            Ok(resp) => {
                let body =
                    crate::gateway::read_body_capped_for_tests(resp, &r.path).map_err(|e| {
                        ApiError {
                            status: None,
                            code: "invalid_response".into(),
                            message: e.to_string(),
                            field: None,
                        }
                    })?;
                match serde_json::from_str::<Value>(&body) {
                    Ok(v) if v.is_object() => Ok(v),
                    _ => Err(ApiError {
                        status: Some(200),
                        code: "invalid_response".into(),
                        message: format!("{} {} answered without a JSON object", r.method, r.path),
                        field: None,
                    }),
                }
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(auto::parse_api_error(code, &body))
            }
            Err(ureq::Error::Transport(t)) => Err(ApiError {
                status: None,
                code: "unreachable".into(),
                message: format!("{} {}: {t}", r.method, r.path),
                field: None,
            }),
        }
    }

    fn capability(&self) -> Result<(), String> {
        let r = Request {
            method: "GET",
            path: "/api/gateway/discovery/capabilities".into(),
            body: None,
        };
        let v = self.send(&r).map_err(|e| auto::api_error_text(&e))?;
        auto::capability_from_discovery(&v)
    }

    /// Every page of the list (v1 has no change cursor: full pages are polled).
    pub fn list_all(&self) -> Result<Vec<auto::Summary>, String> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..200 {
            let v = self
                .send(&auto::list_request(cursor.as_deref()))
                .map_err(|e| auto::api_error_text(&e))?;
            let page = auto::parse_list_page(&v)?;
            out.extend(page.items);
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => return Ok(out),
            }
        }
        Err("GET /api/gateway/automations kept returning next_cursor after 200 pages".into())
    }

    #[allow(clippy::type_complexity)]
    pub fn detail(
        &self,
        id: &str,
    ) -> Result<
        (
            auto::Definition,
            auto::Summary,
            auto::Page<auto::Occurrence>,
        ),
        String,
    > {
        let d = self
            .send(&auto::detail_request(id))
            .map_err(|e| auto::api_error_text(&e))?;
        let (definition, summary) = auto::parse_detail(&d)?;
        let o = self
            .send(&auto::occurrences_request(id, None))
            .map_err(|e| auto::api_error_text(&e))?;
        Ok((definition, summary, auto::parse_occurrence_page(&o)?))
    }
}

/// Spawn the thread for one lane command.
pub fn spawn(client: &GatewayClient, wake: WakeHandle, store: Store, cmd: AutoCmd) {
    let client = AutomationClient::from_gateway(client);
    let panic_wake = wake.clone();
    let _ = std::thread::Builder::new()
        .name("automations".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                run(&client, &wake, store, cmd);
            }));
            if let Err(payload) = result {
                let msg = crate::runner::panic_text(payload.as_ref());
                panic_wake.post(move || {
                    store.automations.update(|v| {
                        v.busy = false;
                        v.loading = false;
                        v.error = format!("automations thread died: {msg}");
                    });
                });
            }
        });
}

/// Read the list (+ the open automation) and post it.
fn refresh(client: &AutomationClient, wake: &WakeHandle, store: Store, open: Option<String>) {
    let list = client.list_all();
    let detail = open.map(|id| (id.clone(), client.detail(&id)));
    wake.post(move || {
        store.automations.update(|v| {
            match list {
                Ok(items) => v.apply_list(items),
                Err(e) => {
                    v.list = Some(Err(e));
                    v.loading = false;
                }
            }
            if let Some((id, d)) = detail {
                match d {
                    Ok((def, summary, page)) => v.apply_detail(&id, def, summary, page),
                    Err(e) => {
                        if let Some(det) = v.detail.as_mut().filter(|det| det.id == id) {
                            det.error = e;
                        }
                    }
                }
            }
        });
    });
}

/// Post an action's outcome: settle its id, clear busy, say what happened.
fn settle(wake: &WakeHandle, store: Store, outcome: Result<String, ApiError>) {
    wake.post(move || {
        store.automations.update(|v| {
            v.busy = false;
            match outcome {
                Ok(notice) => {
                    v.ids.settle(false);
                    v.notice = notice;
                    v.error.clear();
                }
                Err(e) => {
                    v.ids.settle(e.is_transport());
                    v.error = auto::api_error_text(&e);
                }
            }
        });
    });
}

fn run(client: &AutomationClient, wake: &WakeHandle, store: Store, cmd: AutoCmd) {
    match cmd {
        AutoCmd::Refresh { open } => {
            let availability = client.capability();
            let ok = availability.is_ok();
            wake.post(move || {
                store
                    .automations
                    .update(|v| v.availability = Some(availability))
            });
            if ok {
                refresh(client, wake, store, open);
            } else {
                wake.post(move || store.automations.update(|v| v.loading = false));
            }
        }
        AutoCmd::Open { id } => {
            let d = client.detail(&id);
            wake.post(move || {
                store.automations.update(|v| match d {
                    Ok((def, summary, page)) => v.apply_detail(&id, def, summary, page),
                    Err(e) => {
                        if let Some(det) = v.detail.as_mut().filter(|det| det.id == id) {
                            det.error = e;
                        }
                    }
                })
            });
        }
        AutoCmd::More { id, cursor } => {
            let page = client
                .send(&auto::occurrences_request(&id, Some(&cursor)))
                .map_err(|e| auto::api_error_text(&e))
                .and_then(|v| auto::parse_occurrence_page(&v));
            wake.post(move || {
                store.automations.update(|v| match page {
                    Ok(p) => v.apply_more(&id, p),
                    Err(e) => v.error = e,
                })
            });
        }
        AutoCmd::Command {
            id,
            command_id,
            command_type,
        } => {
            let out = client
                .send(&auto::command_request(&id, &command_id, &command_type))
                .map(|r| {
                    let dup = r.get("duplicate").and_then(Value::as_bool).unwrap_or(false);
                    let verb = command_type
                        .trim_start_matches("automation.")
                        .replace('_', " ");
                    if dup {
                        format!(
                            "{verb}: already sent (the gateway answered the retry as a duplicate)"
                        )
                    } else {
                        format!("{verb} sent — the gateway applies it in a moment")
                    }
                });
            let ok = out.is_ok();
            settle(wake, store, out);
            if ok {
                follow_up(client, wake, store, Some(id));
            }
        }
        AutoCmd::Revise {
            id,
            command_id,
            expected_revision,
            changes,
        } => {
            let out = client
                .send(&auto::revise_request(
                    &id,
                    &command_id,
                    expected_revision,
                    changes,
                ))
                .map(|_| "revision sent — it applies from the next run".to_string());
            let ok = out.is_ok();
            settle(wake, store, out);
            if ok {
                follow_up(client, wake, store, Some(id));
            }
        }
        AutoCmd::Create { body } => match client.send(&auto::create_request(body)) {
            Ok(r) => {
                let id = r
                    .get("automation_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if id.is_empty() {
                    settle(
                        wake,
                        store,
                        Err(ApiError {
                            status: Some(200),
                            code: "invalid_response".into(),
                            message: "the create answer carried no automation_id".into(),
                            field: None,
                        }),
                    );
                    return;
                }
                let title = r
                    .pointer("/summary/title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                settle(wake, store, Ok(format!("automation created: {title}")));
                let created = id.clone();
                wake.post(move || store.automations.update(|v| v.created = Some(created)));
                // The first occurrence is admitted moments after the answer.
                follow_up(client, wake, store, Some(id));
            }
            Err(e) => settle(wake, store, Err(e)),
        },
        AutoCmd::Discuss {
            id,
            request_id,
            index,
            prompt,
        } => {
            let out = client
                .send(&auto::discuss_request(&id, &request_id, index, &prompt))
                .and_then(|v| {
                    auto::parse_discuss(&v).map_err(|m| ApiError {
                        status: Some(200),
                        code: "invalid_response".into(),
                        message: m,
                        field: None,
                    })
                });
            match out {
                Ok(d) => {
                    let notice = auto::discussion_notice(index, &d);
                    settle(wake, store, Ok(notice));
                    wake.post(move || {
                        store
                            .automations
                            .update(|v| v.discussion = Some((index, d)))
                    });
                }
                Err(e) => settle(wake, store, Err(e)),
            }
        }
        AutoCmd::Seen { id, cursor } => {
            // Acknowledged only after the gateway accepted it; a failure is
            // retried on the next refresh that brings the items again.
            let ok = client.send(&auto::seen_request(&id, &cursor)).is_ok();
            wake.post(move || {
                store.automations.update(|v| {
                    if ok {
                        v.mark_acked(&id, &cursor)
                    } else {
                        v.ack_failed()
                    }
                })
            });
        }
        AutoCmd::Answer {
            id,
            command_id,
            wait,
            payload,
        } => {
            let out = client
                .send(&auto::wait_answer_request(&command_id, &wait, payload))
                .map(|_| {
                    format!(
                        "answer sent to run #{}",
                        wait.index.map(|i| i.to_string()).unwrap_or_default()
                    )
                });
            let ok = out.is_ok();
            settle(wake, store, out);
            if ok {
                follow_up(client, wake, store, Some(id));
            }
        }
    }
}

fn follow_up(client: &AutomationClient, wake: &WakeHandle, store: Store, open: Option<String>) {
    refresh(client, wake, store, open.clone());
    for pause in FOLLOW_UPS {
        std::thread::sleep(pause);
        refresh(client, wake, store, open.clone());
    }
}
