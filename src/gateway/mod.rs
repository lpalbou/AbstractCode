//! Blocking HTTP client for the AbstractGateway control plane.
//!
//! Endpoints used (all under `/api/gateway`): `ping`, `bundles`,
//! `discovery/providers`, `discovery/tools`, `runs/start`, `runs/{id}`,
//! `runs/{id}/ledger[?after&limit]`, `runs/{id}/ledger/stream` (SSE),
//! `runs/{id}/artifacts/{aid}/content`, `runs?session_id=...`, `commands`.
//!
//! This client lives on the worker thread; the UI never blocks on HTTP.

pub mod sse;

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct GwError {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for GwError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(code) => write!(f, "gateway HTTP {code}: {}", self.message),
            None => write!(f, "gateway unreachable: {}", self.message),
        }
    }
}

impl std::error::Error for GwError {}

pub type GwResult<T> = Result<T, GwError>;

fn err_from_ureq(label: &str, e: ureq::Error) -> GwError {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("detail")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| v.get("error").map(|e| e.to_string()))
                })
                .unwrap_or_else(|| {
                    let t = body.trim();
                    if t.is_empty() {
                        format!("{label} failed")
                    } else {
                        t.chars().take(400).collect()
                    }
                });
            GwError {
                status: Some(code),
                message: detail,
            }
        }
        ureq::Error::Transport(t) => GwError {
            status: None,
            message: format!("{label}: {t}"),
        },
    }
}

#[derive(Clone)]
pub struct GatewayClient {
    base_url: String,
    token: Option<String>,
    agent: ureq::Agent,
    stream_agent: ureq::Agent,
}

impl GatewayClient {
    pub fn new(base_url: &str, token: Option<&str>) -> GatewayClient {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(60))
            .timeout_write(Duration::from_secs(30))
            .build();
        // The SSE stream idles between records; the gateway heartbeats with
        // keep-alive comments, so a long read timeout doubles as the idle
        // watchdog (on timeout the runner polls run status and reconnects).
        let stream_agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(75))
            .build();
        GatewayClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(str::to_string).filter(|t| !t.is_empty()),
            agent,
            stream_agent,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/gateway{}", self.base_url, path)
    }

    fn with_auth(&self, req: ureq::Request) -> ureq::Request {
        match &self.token {
            Some(t) => req.set("Authorization", &format!("Bearer {t}")),
            None => req,
        }
    }

    fn get_json(&self, path: &str) -> GwResult<Value> {
        let req = self.with_auth(
            self.agent
                .get(&self.url(path))
                .set("Accept", "application/json"),
        );
        let resp = req.call().map_err(|e| err_from_ureq(path, e))?;
        let body = resp.into_string().map_err(|e| GwError {
            status: None,
            message: format!("{path}: read failed: {e}"),
        })?;
        serde_json::from_str(&body).map_err(|e| GwError {
            status: None,
            message: format!("{path}: invalid JSON: {e}"),
        })
    }

    fn post_json(&self, path: &str, payload: &Value) -> GwResult<Value> {
        let req = self.with_auth(
            self.agent
                .post(&self.url(path))
                .set("Accept", "application/json")
                .set("Content-Type", "application/json"),
        );
        let resp = req
            .send_string(&payload.to_string())
            .map_err(|e| err_from_ureq(path, e))?;
        let body = resp.into_string().map_err(|e| GwError {
            status: None,
            message: format!("{path}: read failed: {e}"),
        })?;
        if body.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body).map_err(|e| GwError {
            status: None,
            message: format!("{path}: invalid JSON: {e}"),
        })
    }

    // -- probes --------------------------------------------------------------

    pub fn ping(&self) -> GwResult<Value> {
        self.get_json("/ping")
    }

    // -- catalog / discovery --------------------------------------------------

    pub fn list_bundles(&self) -> GwResult<Value> {
        self.get_json("/bundles?all_versions=false&include_deprecated=false")
    }

    pub fn discovery_providers(&self, include_models: bool) -> GwResult<Value> {
        self.get_json(&format!(
            "/discovery/providers?include_models={}",
            if include_models { "true" } else { "false" }
        ))
    }

    pub fn discovery_tools(&self) -> GwResult<Value> {
        self.get_json("/discovery/tools")
    }

    pub fn workspace_policy(&self) -> GwResult<Value> {
        self.get_json("/workspace/policy")
    }

    /// The gateway's skill shelf (attachable per run via `input_data.skills`).
    pub fn skills(&self) -> GwResult<Value> {
        self.get_json("/skills")
    }

    /// The gateway's MCP server registry (gateway-side configuration).
    pub fn mcp_servers(&self) -> GwResult<Value> {
        self.get_json("/mcp/servers")
    }

    /// Capability routes (which provider/model serves each modality) — the
    /// source of truth for what "gateway defaults" resolves to.
    pub fn capability_defaults(&self) -> GwResult<Value> {
        self.get_json("/config/capability-defaults")
    }

    /// Per-provider model list — the gateway route its own console uses.
    /// Needed for provider-endpoint profiles whose models the BULK discovery
    /// route served as [] on gateways predating the 2026-07-22 fix (the
    /// gateway's stated contract keeps this fallback correct and harmless).
    pub fn provider_models(&self, provider: &str) -> GwResult<Value> {
        self.get_json(&format!(
            "/discovery/providers/{}/models",
            url_encode(provider)
        ))
    }

    /// Prompt-cache capability for one provider/model route.
    pub fn prompt_cache_capabilities(&self, provider: &str, model: &str) -> GwResult<Value> {
        self.get_json(&format!(
            "/prompt_cache/capabilities?provider={}&model={}",
            url_encode(provider),
            url_encode(model)
        ))
    }

    // -- runs ------------------------------------------------------------------

    pub fn start_run(
        &self,
        flow_id: &str,
        bundle_id: Option<&str>,
        session_id: Option<&str>,
        input_data: Value,
    ) -> GwResult<String> {
        let mut body = json!({ "flow_id": flow_id, "input_data": input_data });
        if let Some(b) = bundle_id {
            if !b.trim().is_empty() {
                body["bundle_id"] = json!(b.trim());
            }
        }
        if let Some(s) = session_id {
            if !s.trim().is_empty() {
                body["session_id"] = json!(s.trim());
            }
        }
        let resp = self.post_json("/runs/start", &body)?;
        let run_id = resp
            .get("run_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if run_id.is_empty() {
            return Err(GwError {
                status: None,
                message: format!("runs/start: missing run_id in {resp}"),
            });
        }
        Ok(run_id)
    }

    pub fn get_run(&self, run_id: &str) -> GwResult<Value> {
        self.get_json(&format!("/runs/{}", url_encode(run_id)))
    }

    pub fn list_runs(&self, session_id: &str, limit: u32) -> GwResult<Value> {
        self.get_json(&format!(
            "/runs?limit={limit}&session_id={}&root_only=true",
            url_encode(session_id)
        ))
    }

    pub fn get_ledger(&self, run_id: &str, after: u64, limit: u32) -> GwResult<(Vec<Value>, u64)> {
        let v = self.get_json(&format!(
            "/runs/{}/ledger?after={after}&limit={limit}",
            url_encode(run_id)
        ))?;
        let items = v
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let next = v
            .get("next_after")
            .and_then(Value::as_u64)
            .unwrap_or(after + items.len() as u64);
        Ok((items, next))
    }

    pub fn artifact_bytes(
        &self,
        run_id: &str,
        artifact_id: &str,
        max_bytes: usize,
    ) -> GwResult<(Vec<u8>, String)> {
        let path = format!(
            "/runs/{}/artifacts/{}/content",
            url_encode(run_id),
            url_encode(artifact_id)
        );
        let req = self.with_auth(self.agent.get(&self.url(&path)));
        let resp = req.call().map_err(|e| err_from_ureq(&path, e))?;
        let content_type = resp.content_type().to_string();
        let mut bytes = Vec::new();
        resp.into_reader()
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| GwError {
                status: None,
                message: format!("artifact read failed: {e}"),
            })?;
        if bytes.len() > max_bytes {
            return Err(GwError {
                status: None,
                message: format!("artifact larger than {max_bytes} bytes; not rendering inline"),
            });
        }
        Ok((bytes, content_type))
    }

    // -- commands ---------------------------------------------------------------

    pub fn submit_command(&self, run_id: &str, typ: &str, payload: Value) -> GwResult<Value> {
        let body = json!({
            "command_id": format!("cmd_{}", crate::config::mint_session_id().trim_start_matches("acode-")),
            "run_id": run_id,
            "type": typ,
            "payload": payload,
            "client_id": "abstractcode-tui",
        });
        self.post_json("/commands", &body)
    }

    pub fn resume(&self, run_id: &str, wait_key: &str, payload: Value) -> GwResult<Value> {
        self.submit_command(
            run_id,
            "resume",
            json!({ "wait_key": wait_key, "payload": payload }),
        )
    }

    pub fn cancel(&self, run_id: &str) -> GwResult<Value> {
        self.submit_command(run_id, "cancel", json!({}))
    }

    /// Pause the run TREE durably (gateway `pause` command; the runner stops
    /// ticking every run in the tree at its next step boundary).
    pub fn pause(&self, run_id: &str) -> GwResult<Value> {
        self.submit_command(run_id, "pause", json!({}))
    }

    /// Resume a PAUSED run tree. Deliberately carries NO `payload` key —
    /// its presence is the gateway's discriminator between "answer a
    /// waiting run" and "unpause the tree" (runner.py `_apply_command`).
    pub fn resume_paused(&self, run_id: &str) -> GwResult<Value> {
        self.submit_command(run_id, "resume", json!({}))
    }

    pub fn steer(&self, run_id: &str, guidance: &str) -> GwResult<Value> {
        self.submit_command(run_id, "inject_guidance", json!({ "guidance": guidance }))
    }

    /// One document for a whole run TREE: `input_data` (the prompt),
    /// `ledgers` keyed by run id, and the root `run` record. The gateway
    /// documents this route as THE thin-client replay surface ("render/
    /// replay without stitching multiple endpoints"). With
    /// `include_session`, the bundle also carries the ordered session turn
    /// list (`session.turns`: run_id, prompt, status per root run).
    pub fn history_bundle(
        &self,
        run_id: &str,
        include_session: bool,
        turn_limit: u32,
    ) -> GwResult<Value> {
        let mut path = format!("/runs/{}/history_bundle", url_encode(run_id));
        if include_session {
            path.push_str(&format!(
                "?include_session=true&session_turn_limit={}",
                turn_limit.clamp(1, 500)
            ));
        }
        self.get_json(&path)
    }

    /// The run's original input_data (prompt, context, …).
    pub fn input_data(&self, run_id: &str) -> GwResult<Value> {
        self.get_json(&format!("/runs/{}/input_data", url_encode(run_id)))
    }

    // -- SSE ledger streaming -----------------------------------------------------

    /// Stream ledger records from `after`. Calls `on_batch(records)` once
    /// per network read (records must reach the UI at ARRIVAL cadence —
    /// buffering across reads would hold live activity hostage), with the
    /// running cursor reported through `on_cursor`. Returns:
    /// - `Ok(true)` when the gateway closed with `event: done` (run terminal),
    /// - `Ok(false)` when the stream ended/idled without a done event
    ///   (caller should poll run status and maybe reconnect from the cursor).
    pub fn stream_ledger(
        &self,
        run_id: &str,
        after: u64,
        stop: &Arc<AtomicBool>,
        mut on_cursor: impl FnMut(u64),
        mut on_batch: impl FnMut(Vec<Value>),
    ) -> GwResult<bool> {
        let path = format!("/runs/{}/ledger/stream?after={after}", url_encode(run_id));
        let req = self.with_auth(
            self.stream_agent
                .get(&self.url(&path))
                .set("Accept", "text/event-stream"),
        );
        let resp = req.call().map_err(|e| err_from_ureq(&path, e))?;
        let mut reader = resp.into_reader();
        let mut parser = sse::SseParser::new();
        let mut buf = [0u8; 16 * 1024];
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(false);
            }
            let n = match reader.read(&mut buf) {
                Ok(0) => return Ok(false), // server closed without done
                Ok(n) => n,
                Err(e) => {
                    let kind = e.kind();
                    if matches!(
                        kind,
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) {
                        // Idle past the read timeout: hand control back so the
                        // runner can poll status and reconnect from the cursor.
                        return Ok(false);
                    }
                    return Err(GwError {
                        status: None,
                        message: format!("stream read failed: {e}"),
                    });
                }
            };
            let mut records: Vec<Value> = Vec::new();
            let mut saw_done = false;
            for ev in parser.push(&buf[..n]) {
                match ev.event.as_str() {
                    "step" => {
                        if let Ok(v) = serde_json::from_str::<Value>(&ev.data) {
                            let cursor = v.get("cursor").and_then(Value::as_u64).unwrap_or(0);
                            on_cursor(cursor);
                            if let Some(record) = v.get("record") {
                                records.push(record.clone());
                            }
                        }
                    }
                    "done" => saw_done = true,
                    _ => {}
                }
            }
            if !records.is_empty() {
                on_batch(records);
            }
            if saw_done {
                return Ok(true);
            }
        }
    }
}

/// Percent-encode a path segment (conservative: alphanumerics and -_.~ pass).
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encoding_is_conservative() {
        assert_eq!(url_encode("run-1_2.3~x"), "run-1_2.3~x");
        assert_eq!(url_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(url_encode("é"), "%C3%A9");
    }
}
