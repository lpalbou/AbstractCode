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
        /// What the session knows of the round-16 schedule API: `Unknown`
        /// probes it first (one preview), `Missing` sends `schedule@1`.
        schedule_api: auto::ScheduleApi,
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
    /// `/schedule`'s When step: the gateway's words for a trigger (nothing stored).
    Preview {
        trigger: Value,
    },
    /// `/schedule` opens: the account's email status, the executable
    /// workflows for the picker, and the conversation workflow's input
    /// schema (bundle, version, flow).
    Prepare {
        schema: Option<(String, String, String)>,
    },
    /// The account's email status only (the definition panel's Mailbox rows).
    EmailStatus,
    /// One workflow's input schema (a pick in `/schedule`).
    Schema {
        bundle: String,
        version: String,
        flow: String,
    },
}

/// `GET /api/gateway/me/email`.
pub fn email_status_request() -> Request {
    Request {
        method: "GET",
        path: "/api/gateway/me/email".into(),
        body: None,
    }
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

    /// `GET /me/email` → the dialog's email gate (`None` when it cannot be
    /// read: unknown is not usable, never an automations error).
    pub fn email_status(&self) -> Option<crate::automation_email::EmailStatus> {
        self.send(&email_status_request())
            .ok()
            .and_then(|v| crate::automation_email::EmailStatus::parse(&v).ok())
    }

    /// The executable workflows for the picker (the kit's parser: a
    /// gateway that ignores the contract is an error, shown as such).
    pub fn executable(&self) -> Result<crate::workflow_picker::Executable, String> {
        let r = Request {
            method: "GET",
            path: crate::workflow_picker::path(auto::CODE_AGENT_INTERFACE),
            body: None,
        };
        let v = self.send(&r).map_err(|e| auto::api_error_text(&e))?;
        crate::workflow_picker::parse(&v, auto::CODE_AGENT_INTERFACE)
    }

    /// One workflow's input schema, as the web's `fetchWorkflowSchema` reads
    /// it: the `input_schema` route, normalised; an older v1 descriptor with
    /// required pins reconciled with its VisualFlow.
    pub fn schema(&self, bundle: &str, version: &str, flow: &str) -> Result<Value, String> {
        use crate::schedule_input as si;
        let get = |path: String| {
            self.send(&Request {
                method: "GET",
                path,
                body: None,
            })
            .map_err(|e| auto::api_error_text(&e))
        };
        let raw = get(si::input_schema_path(bundle, flow, version))?;
        let schema = si::normalize_input_schema(&raw).ok_or_else(|| {
            "This workflow does not report its input requirements. Choose another workflow."
                .to_string()
        })?;
        if !si::needs_visualflow(&raw, &schema) {
            return Ok(schema);
        }
        let version = if version.is_empty() {
            raw.get("bundle_version")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        } else {
            version.to_string()
        };
        if version.is_empty() {
            return Err(
                "The Gateway did not identify the workflow version. Refresh workflows and retry."
                    .into(),
            );
        }
        si::assert_selection(&raw, bundle, &version, flow)?;
        let source = get(si::flow_source_path(bundle, flow, &version))?;
        si::assert_selection(&source, bundle, &version, flow)?;
        si::reconcile_visualflow_schema(&schema, source.get("flow").unwrap_or(&Value::Null))
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
        self.list_pages(auto::list_request).map(|(items, _)| items)
    }

    /// Every page of a listing + the first page's `archived_automations`.
    fn list_pages(
        &self,
        request: fn(Option<&str>) -> Request,
    ) -> Result<(Vec<auto::Summary>, u64), String> {
        let mut out = Vec::new();
        let mut archived = 0;
        let mut cursor: Option<String> = None;
        for page_no in 0..200 {
            let v = self
                .send(&request(cursor.as_deref()))
                .map_err(|e| auto::api_error_text(&e))?;
            if page_no == 0 {
                archived = v
                    .get("archived_automations")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
            }
            let page = auto::parse_list_page(&v)?;
            out.extend(page.items);
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => return Ok((out, archived)),
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

/// The ambient attention read (R17.1, `crate::attention`): every list page,
/// then whether each active automation asks before each tool call — the
/// row's `policy.tool_approval` when the list carries it, else its
/// definition, read once per revision (`known` = already read). A failed
/// list read is posted like the overlay's (the chip then shows nothing,
/// never a stale count); a failed definition read counts as "does not ask"
/// for that revision (no re-read every poll).
pub fn spawn_attention(
    client: &GatewayClient,
    wake: WakeHandle,
    store: Store,
    known: Vec<(String, u64)>,
) {
    let client = AutomationClient::from_gateway(client);
    crate::runner::spawn_host_thread("attention", wake.clone(), store, move || {
        let list = client.list_with_policy();
        let (items, listed_policy) = match list {
            Ok(v) => v,
            Err(e) => {
                wake.post(move || {
                    store.automations.update(|v| {
                        v.list = Some(Err(e));
                    })
                });
                return;
            }
        };
        let read: Vec<(String, u64, bool)> = crate::attention::to_read(&items, &known)
            .into_iter()
            .map(|(id, rev)| {
                if let Some((_, asks)) = listed_policy.iter().find(|(i, _)| *i == id) {
                    return (id, rev, *asks);
                }
                let asks = client
                    .send(&auto::detail_request(&id))
                    .ok()
                    .and_then(|d| auto::parse_detail(&d).ok())
                    .is_some_and(|(def, _)| def.tool_approval == "ask");
                (id, rev, asks)
            })
            .collect();
        wake.post(move || {
            store.automation_ask.update(|a| a.merge(read, &items));
            store.automations.update(|v| v.apply_list(items));
        });
    });
}

impl AutomationClient {
    /// Every list page (like `list_all`) + `(id, asks)` for the rows that
    /// carry `policy.tool_approval`.
    #[allow(clippy::type_complexity)]
    fn list_with_policy(&self) -> Result<(Vec<auto::Summary>, Vec<(String, bool)>), String> {
        let mut out = Vec::new();
        let mut policy = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..200 {
            let v = self
                .send(&auto::list_request(cursor.as_deref()))
                .map_err(|e| auto::api_error_text(&e))?;
            for row in v
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let (Some(id), Some(ta)) = (
                    row.get("automation_id").and_then(Value::as_str),
                    row.pointer("/policy/tool_approval").and_then(Value::as_str),
                ) {
                    policy.push((id.to_string(), ta == "ask"));
                }
            }
            let page = auto::parse_list_page(&v)?;
            out.extend(page.items);
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => return Ok((out, policy)),
            }
        }
        Err("GET /api/gateway/automations kept returning next_cursor after 200 pages".into())
    }
}

/// Read the list (+ the open automation) and post it.
fn refresh(client: &AutomationClient, wake: &WakeHandle, store: Store, open: Option<String>) {
    let list = client.list_pages(auto::list_request);
    // The archived ones are read only when the gateway counts some: the
    // `Archived · N` line opens onto them without another round trip.
    let archived = match &list {
        Ok((_, n)) if *n > 0 => Some(
            client
                .list_pages(auto::archived_list_request)
                .map(|(items, _)| items),
        ),
        Ok(_) => Some(Ok(Vec::new())),
        Err(_) => None,
    };
    let detail = open.map(|id| (id.clone(), client.detail(&id)));
    wake.post(move || {
        store.automations.update(|v| {
            if let Some(a) = archived {
                v.archived = Some(a);
            }
            match list {
                Ok((items, archived_count)) => {
                    v.archived_count = archived_count;
                    v.apply_list(items)
                }
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
                    if let Some(state) = auto::Control::from_command_type(&command_type)
                        .and_then(auto::Control::accepted_notice)
                    {
                        return state.to_string();
                    }
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
            // Accepted is not done: the row stays pending until a re-read
            // shows the new state (or the follow-up reads end).
            match out {
                Ok(notice) => {
                    wake.post(move || {
                        store.automations.update(|v| {
                            v.ids.settle(false);
                            v.error.clear();
                            v.accept_pending(notice);
                        })
                    });
                    follow_up(client, wake, store, Some(id));
                    wake.post(move || store.automations.update(|v| v.finish_pending()));
                }
                Err(e) => {
                    wake.post(move || store.automations.update(|v| v.fail_pending()));
                    settle(wake, store, Err(e));
                }
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
        AutoCmd::Create { body, schedule_api } => {
            let body = match schedule_create_body(client, wake, store, body, schedule_api) {
                Ok(b) => b,
                Err(e) => {
                    settle(wake, store, Err(e));
                    return;
                }
            };
            create(client, wake, store, body)
        }
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
        AutoCmd::Preview { trigger } => {
            let (state, api) = preview_state(client.send(&auto::preview_request(trigger.clone())));
            wake.post(move || {
                store.automations.update(|v| {
                    v.schedule_api = api;
                    v.apply_preview(&trigger, state)
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
        AutoCmd::Prepare { schema } => {
            let email = client.email_status();
            let executable = client.executable();
            wake.post(move || {
                store.automations.update(|v| {
                    v.email = email;
                    v.executable = Some(executable);
                })
            });
            if let Some(key) = schema {
                post_schema(client, wake, store, key);
            }
        }
        AutoCmd::EmailStatus => {
            let email = client.email_status();
            wake.post(move || store.automations.update(|v| v.email = email));
        }
        AutoCmd::Schema {
            bundle,
            version,
            flow,
        } => post_schema(client, wake, store, (bundle, version, flow)),
    }
}

/// `POST /automations` and what follows (the new automation opens).
fn create(client: &AutomationClient, wake: &WakeHandle, store: Store, body: Value) {
    match client.send(&auto::create_request(body)) {
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
    }
}

/// The create body for THIS gateway (posts what a probe learned).
fn schedule_create_body(
    client: &AutomationClient,
    wake: &WakeHandle,
    store: Store,
    body: Value,
    known: auto::ScheduleApi,
) -> Result<Value, ApiError> {
    let (out, learned) = create_body_for(client, body, known);
    if let Some(api) = learned {
        wake.post(move || store.automations.update(|v| v.schedule_api = api));
    }
    out
}

/// The create body for THIS gateway: a `schedule@2` trigger on a gateway
/// without the round-16 schedule API is sent as `schedule@1` (Repeat, Once
/// at… in UTC); a calendar rule there is refused with the one sentence.
/// When the session has not probed yet (`Unknown`), the trigger's own
/// preview is the probe — once; the second value is what it learned.
pub fn create_body_for(
    client: &AutomationClient,
    body: Value,
    known: auto::ScheduleApi,
) -> (Result<Value, ApiError>, Option<auto::ScheduleApi>) {
    let trigger = body.get("trigger").cloned().unwrap_or(Value::Null);
    if !auto::needs_schedule_api(&trigger) {
        return (Ok(body), None);
    }
    let (api, learned) = match known {
        auto::ScheduleApi::Unknown => {
            let api = match client.send(&auto::preview_request(trigger)) {
                Err(e) if auto::is_missing_route(&e) => auto::ScheduleApi::Missing,
                // No answer: the probe learned nothing — send the shape every
                // gateway accepts (schedule@1), never a schedule@2 guess; a
                // calendar rule has no such shape, so it is not sent.
                Err(e) if e.is_transport() => {
                    return (auto::legacy_create_body(&body).map_err(|_| e), None);
                }
                _ => auto::ScheduleApi::Served,
            };
            (api, Some(api))
        }
        known => (known, None),
    };
    if api != auto::ScheduleApi::Missing {
        return (Ok(body), learned);
    }
    let out = auto::legacy_create_body(&body).map_err(|m| ApiError {
        status: Some(422),
        code: "unsupported_feature".into(),
        message: m,
        field: None,
    });
    (out, learned)
}

/// A preview answer as the When line shows it, and what it says about the
/// round-16 schedule API: a 404/405 (no such route: AbstractGateway 0.13.x)
/// is `Unavailable` + `Missing` — the sentence in place, never the route's
/// error, and no further preview is asked this session.
pub fn preview_state(answer: Result<Value, ApiError>) -> (auto::PreviewState, auto::ScheduleApi) {
    match answer {
        Ok(v) => (
            match auto::parse_schedule_preview(&v) {
                Ok(p) => auto::PreviewState::Ready(p),
                Err(e) => auto::PreviewState::Failed(e),
            },
            auto::ScheduleApi::Served,
        ),
        Err(e) if auto::is_missing_route(&e) => {
            (auto::PreviewState::Unavailable, auto::ScheduleApi::Missing)
        }
        // No answer: one plain line (never the transport text), nothing
        // learned, and Continue is not stopped.
        Err(e) if e.is_transport() => (auto::PreviewState::Unreached, auto::ScheduleApi::Unknown),
        Err(e) => (
            auto::PreviewState::Failed(auto::api_error_text(&e)),
            auto::ScheduleApi::Served,
        ),
    }
}

fn post_schema(
    client: &AutomationClient,
    wake: &WakeHandle,
    store: Store,
    (bundle, version, flow): (String, String, String),
) {
    let answer = client.schema(&bundle, &version, &flow);
    let key = auto::schema_key(&bundle, &version, &flow);
    wake.post(move || store.automations.update(|v| v.put_schema(key, answer)));
}

fn follow_up(client: &AutomationClient, wake: &WakeHandle, store: Store, open: Option<String>) {
    refresh(client, wake, store, open.clone());
    for pause in FOLLOW_UPS {
        std::thread::sleep(pause);
        refresh(client, wake, store, open.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Read, Write};

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

    fn client(url: &str) -> AutomationClient {
        AutomationClient::from_gateway(&GatewayClient::new(url, Some("t")))
    }

    /// What AbstractGateway 0.13.1 answers for `POST /automations/schedule-preview`
    /// (measured on the released package: the route does not exist there).
    const METHOD_NOT_ALLOWED: &str =
        r#"{"detail": {"reason_code": "invalid_request", "message": "Method Not Allowed"}}"#;

    fn repeat_v2() -> Value {
        json!({"source_id": "schedule", "source_version": 2,
               "config": {"kind": "every", "every": "24h", "count": 3}})
    }

    fn body_with(trigger: Value) -> Value {
        json!({"request_id": "r1", "title": "t", "target": {"flow_id": "f"}, "trigger": trigger})
    }

    #[test]
    fn a_405_or_404_preview_is_the_sentence_and_marks_the_api_missing() {
        for status in ["405 Method Not Allowed", "404 Not Found"] {
            let (url, rx) = server(vec![(status, METHOD_NOT_ALLOWED)]);
            let answer = client(&url).send(&auto::preview_request(repeat_v2()));
            let (state, api) = preview_state(answer);
            assert_eq!(state, auto::PreviewState::Unavailable, "{status}");
            assert_eq!(api, auto::ScheduleApi::Missing, "{status}");
            assert_eq!(
                auto::preview_lines(&state, false),
                vec![auto::NEEDS_NEWER_GATEWAY.to_string()]
            );
            let (line, _) = rx.recv().unwrap();
            assert!(
                line.starts_with("POST /api/gateway/automations/schedule-preview"),
                "{line}"
            );
        }
    }

    #[test]
    fn a_refused_rule_is_the_gateways_sentence_and_the_api_is_served() {
        let (url, _rx) = server(vec![(
            "422 Unprocessable Entity",
            r#"{"detail": {"reason_code": "invalid_definition", "message": "Once at… is in the past."}}"#,
        )]);
        let (state, api) = preview_state(client(&url).send(&auto::preview_request(repeat_v2())));
        assert_eq!(api, auto::ScheduleApi::Served);
        match state {
            auto::PreviewState::Failed(e) => assert!(e.contains("Once at… is in the past."), "{e}"),
            other => panic!("{other:?}"),
        }
        // No gateway answer at all: one plain line, nothing learned.
        let (state, api) =
            preview_state(client("http://127.0.0.1:9").send(&auto::preview_request(repeat_v2())));
        assert_eq!(
            (state, api),
            (auto::PreviewState::Unreached, auto::ScheduleApi::Unknown)
        );
    }

    #[test]
    fn an_unprobed_create_probes_once_then_sends_schedule_v1() {
        let (url, rx) = server(vec![("405 Method Not Allowed", METHOD_NOT_ALLOWED)]);
        let (out, learned) = create_body_for(
            &client(&url),
            body_with(repeat_v2()),
            auto::ScheduleApi::Unknown,
        );
        assert_eq!(learned, Some(auto::ScheduleApi::Missing));
        assert_eq!(
            out.unwrap()["trigger"],
            json!({"source_id": "schedule", "source_version": 1, "config": {"every": "24h", "count": 3}})
        );
        assert!(rx.recv().unwrap().0.contains("schedule-preview"));
        assert!(rx.try_recv().is_err(), "one probe only");
    }

    #[test]
    fn a_known_missing_api_never_probes_and_refuses_a_calendar_rule() {
        // No server at all: any request would fail the test with a transport error.
        let c = client("http://127.0.0.1:9");
        let (out, learned) =
            create_body_for(&c, body_with(repeat_v2()), auto::ScheduleApi::Missing);
        assert_eq!(learned, None);
        assert_eq!(out.unwrap()["trigger"]["source_version"], 1);
        let daily = json!({"source_id": "schedule", "source_version": 2, "config": {"kind": "daily", "at": "08:00"}});
        let (out, _) = create_body_for(&c, body_with(daily), auto::ScheduleApi::Missing);
        let e = out.unwrap_err();
        assert_eq!(auto::api_error_text(&e), auto::NEEDS_NEWER_GATEWAY);
        // A round-16 gateway: the body goes as built (schedule@2).
        let (out, learned) = create_body_for(&c, body_with(repeat_v2()), auto::ScheduleApi::Served);
        assert_eq!(
            (out.unwrap()["trigger"].clone(), learned),
            (repeat_v2(), None)
        );
    }

    #[test]
    fn an_unprobed_create_without_any_answer_sends_schedule_v1() {
        let c = client("http://127.0.0.1:9");
        let (out, learned) =
            create_body_for(&c, body_with(repeat_v2()), auto::ScheduleApi::Unknown);
        assert_eq!(learned, None, "nothing learned from no answer");
        assert_eq!(
            out.unwrap()["trigger"],
            json!({"source_id": "schedule", "source_version": 1, "config": {"every": "24h", "count": 3}})
        );
        // A calendar rule has no schedule@1 shape: not sent, the transport sentence.
        let daily = json!({"source_id": "schedule", "source_version": 2, "config": {"kind": "daily", "at": "08:00"}});
        let (out, _) = create_body_for(&c, body_with(daily), auto::ScheduleApi::Unknown);
        assert!(out.unwrap_err().is_transport());
    }

    #[test]
    fn a_served_preview_keeps_schedule_v2() {
        let (url, _rx) = server(vec![(
            "200 OK",
            r#"{"trigger": {}, "time_zone": "Europe/Paris", "schedule_rule_text": "Every day at 08:00 (Europe/Paris)",
                "schedule_text": "x", "first_run_sentence": "Runs every day at 08:00 (Europe/Paris), first run Sat 10 Oct 08:00."}"#,
        )]);
        let (out, learned) = create_body_for(
            &client(&url),
            body_with(repeat_v2()),
            auto::ScheduleApi::Unknown,
        );
        assert_eq!(learned, Some(auto::ScheduleApi::Served));
        assert_eq!(out.unwrap()["trigger"], repeat_v2());
    }
}
