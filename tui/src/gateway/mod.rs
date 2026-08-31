//! Blocking HTTP client for the AbstractGateway control plane.
//!
//! Endpoints used (all under `/api/gateway`): `ping`, `bundles`,
//! `discovery/providers`, `discovery/tools`, `runs/start`, `runs/{id}`,
//! `runs/{id}/ledger[?after&limit]`, `runs/{id}/ledger/stream` (SSE),
//! `runs/{id}/artifacts/{aid}/content`, `runs?session_id=...`, `commands`.
//!
//! This client lives on the worker thread; the UI never blocks on HTTP.

pub mod entities;
pub mod gpu;
pub mod sse;

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

/// Evidence class of a gateway error — WHAT the failure proves about the
/// gateway, not just that one failed. The class decides both the honest
/// wording (`Display`) and the connection-orb policy (`runner`): only
/// `Unreachable` is evidence the gateway is GONE; a timeout is a gateway
/// that did not answer IN TIME (loaded/wedged/idle), and an HTTP status
/// of any code is proof the gateway is reachable (the `doctor` command's
/// own rule). Before this class existed, every status-less error rendered
/// as "gateway unreachable" — read timeouts against a merely BUSY gateway
/// included — which is exactly the false claim the operator reported
/// (2026-07-23: health answering in ~1ms while the app said unreachable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GwErrorKind {
    /// The gateway answered with an HTTP status (it is reachable).
    Http,
    /// Connect-level failure: refused, DNS, TLS/proxy, unroutable — the
    /// TCP/naming layer itself says nobody is there.
    Unreachable,
    /// The request timed out (connect fine; the answer never came in
    /// time) — a busy or wedged gateway, not proof it is gone.
    Timeout,
    /// The response BODY exceeded the client's reader ceiling — the
    /// gateway is fine (it answered); the client refused to buffer an
    /// unbounded body. NEVER gateway-down evidence, NEVER retried
    /// (deterministic), NEVER truncated (ADR: truncation is a
    /// violation — refuse loudly instead).
    BodyTooLarge,
    /// Other transport trouble: reset mid-read, truncated body, garbage
    /// JSON — the gateway (or a middlebox) SPOKE, then broke.
    Transport,
}

#[derive(Debug, Clone)]
pub struct GwError {
    pub status: Option<u16>,
    pub kind: GwErrorKind,
    pub message: String,
}

impl GwError {
    pub fn http(code: u16, message: impl Into<String>) -> GwError {
        GwError {
            status: Some(code),
            kind: GwErrorKind::Http,
            message: message.into(),
        }
    }

    pub fn unreachable(message: impl Into<String>) -> GwError {
        GwError {
            status: None,
            kind: GwErrorKind::Unreachable,
            message: message.into(),
        }
    }

    pub fn timeout(message: impl Into<String>) -> GwError {
        GwError {
            status: None,
            kind: GwErrorKind::Timeout,
            message: message.into(),
        }
    }

    pub fn body_too_large(message: impl Into<String>) -> GwError {
        GwError {
            status: None,
            kind: GwErrorKind::BodyTooLarge,
            message: message.into(),
        }
    }

    pub fn transport(message: impl Into<String>) -> GwError {
        GwError {
            status: None,
            kind: GwErrorKind::Transport,
            message: message.into(),
        }
    }

    /// Classify an `io::Error` from reading a response BODY (headers
    /// already arrived, so this is never connect-level evidence on its
    /// own — except the io layer itself naming an unreachable peer).
    pub fn from_io_read(message: String, e: &std::io::Error) -> GwError {
        match e.kind() {
            // macOS/Linux socket read timeouts surface as WouldBlock
            // (EAGAIN); ureq normalizes some paths to TimedOut. Both are
            // the same fact: no bytes in time.
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                GwError::timeout(message)
            }
            std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::HostUnreachable
            | std::io::ErrorKind::NetworkUnreachable => GwError::unreachable(message),
            _ => GwError::transport(message),
        }
    }

    /// Connect-level evidence that the gateway is actually GONE — the
    /// only single-error class allowed to flip the orb `Conn::Down`.
    pub fn is_gone(&self) -> bool {
        self.kind == GwErrorKind::Unreachable
    }

    /// Worth ONE same-id retry: transport-level failures where the
    /// request may never have arrived (or the response was lost). HTTP
    /// statuses are the server SPEAKING — never retried here (the
    /// durable command store's dedup makes the retry safe either way,
    /// but a 4xx/5xx repeat would just repeat). BodyTooLarge is
    /// deterministic — a retry would just re-refuse.
    pub fn is_transient(&self) -> bool {
        self.status.is_none() && self.kind != GwErrorKind::BodyTooLarge
    }

    /// A short, URL-free reason for text that TRAVELS — transcript labels
    /// that later ride `context.messages` as assistant words. The full
    /// `Display` (path + transport detail, request URL included via
    /// ureq's error text) is for toasts and logs only: the 2026-07-23
    /// incident proved a URL-bearing failure label plus an operator
    /// "try again" reads as an instruction kit — the model fetch_url'd
    /// the gateway's own artifact endpoint from "its own" prior message.
    pub fn compact_reason(&self) -> String {
        match (self.status, self.kind) {
            (Some(code), _) => format!("HTTP {code}"),
            (None, GwErrorKind::Unreachable) => "gateway unreachable".into(),
            (None, GwErrorKind::Timeout) => "request timed out".into(),
            (None, GwErrorKind::BodyTooLarge) => "response exceeded the reader cap".into(),
            (None, _) => "transient transport failure".into(),
        }
    }
}

impl std::fmt::Display for GwError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.status, self.kind) {
            (Some(code), _) => write!(f, "gateway HTTP {code}: {}", self.message),
            (None, GwErrorKind::Unreachable) => {
                write!(f, "gateway unreachable: {}", self.message)
            }
            (None, GwErrorKind::Timeout) => write!(f, "gateway timed out: {}", self.message),
            (None, GwErrorKind::BodyTooLarge) => {
                write!(f, "response too large: {}", self.message)
            }
            // Transport (and a mis-built status-less Http) read honestly:
            // the request failed — no reachability claim either way.
            (None, _) => write!(f, "gateway request failed: {}", self.message),
        }
    }
}

impl std::error::Error for GwError {}

pub type GwResult<T> = Result<T, GwError>;

pub(crate) fn err_from_ureq(label: &str, e: ureq::Error) -> GwError {
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
                        // ADR-0026: a non-JSON error body is the only account
                        // the operator gets of WHY the call failed — so it
                        // arrives WHOLE (2026-08-20). The 400-char cut that
                        // used to live here fired before any view existed,
                        // and its `[#TRUNCATION …]` marker then rode into
                        // `/details`, which by contract shortens nothing.
                        // Bounding an HTML error page to a screenful is the
                        // FOLDED view's job, where the cut is labeled.
                        t.to_string()
                    }
                });
            GwError::http(code, detail)
        }
        ureq::Error::Transport(t) => {
            let kind = classify_transport(&t);
            GwError {
                status: None,
                kind,
                message: format!("{label}: {t}"),
            }
        }
    }
}

/// Map ureq's own transport classification onto the evidence classes.
/// This reads STRUCTURE (`ureq::ErrorKind` + the io source's kind), never
/// message text — the previous design collapsed all of these into one
/// "unreachable" label and the app repeated it for mere read timeouts.
fn classify_transport(t: &ureq::Transport) -> GwErrorKind {
    use ureq::ErrorKind as K;
    match t.kind() {
        // Connect/naming/TLS-level: nobody (correctly) answering at the
        // configured address — genuine "gone or misconfigured" evidence.
        K::ConnectionFailed
        | K::Dns
        | K::InvalidUrl
        | K::UnknownScheme
        | K::InsecureRequestHttpsOnly
        | K::ProxyConnect
        | K::ProxyUnauthorized
        | K::InvalidProxyUrl => GwErrorKind::Unreachable,
        // Io covers everything after the connection exists; the io
        // source's kind separates "no bytes in time" from "broke mid-way".
        K::Io => {
            let io_kind = std::error::Error::source(t)
                .and_then(|s| s.downcast_ref::<std::io::Error>())
                .map(std::io::Error::kind);
            match io_kind {
                Some(std::io::ErrorKind::TimedOut) | Some(std::io::ErrorKind::WouldBlock) => {
                    GwErrorKind::Timeout
                }
                Some(std::io::ErrorKind::ConnectionRefused)
                | Some(std::io::ErrorKind::HostUnreachable)
                | Some(std::io::ErrorKind::NetworkUnreachable) => GwErrorKind::Unreachable,
                _ => GwErrorKind::Transport,
            }
        }
        // The server spoke (badly), or redirect games: reachable-ish.
        K::BadStatus | K::BadHeader | K::TooManyRedirects | K::HTTP => GwErrorKind::Transport,
    }
}

/// Body-reader ceiling for JSON lanes. ureq's `into_string()` silently
/// enforced a 10 MiB limit that made every >10 MiB history_bundle (a
/// single tool-heavy TURN measures 10-14 MB today) deterministically
/// unreadable — the replay-integrity incident (2026-07-25). This is a
/// CEILING against unbounded bodies, not a size model: uniform across
/// all JSON lanes (per-lane size models are what rotted), ~18× today's
/// worst observed bundle. On exceed the typed `BodyTooLarge` error
/// names path, cap, and observed size — NEVER truncation (ADR
/// violation; laurent's ruling 2026-07-25), never gateway-down
/// evidence, never retried.
const MAX_JSON_BODY_BYTES: u64 = 256 * 1024 * 1024;

/// Read a response body BOUNDED: up to `cap` bytes, then one probe byte
/// to distinguish exactly-at-cap from over-cap. The honest replacement
/// for `into_string()`'s hidden 10 MiB limit.
fn read_body_capped(resp: ureq::Response, path: &str, cap: u64) -> GwResult<String> {
    use std::io::Read;
    let len_hint = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut buf: Vec<u8> = Vec::with_capacity(len_hint.unwrap_or(64 * 1024).min(cap) as usize);
    let mut reader = resp.into_reader().take(cap + 1);
    reader
        .read_to_end(&mut buf)
        .map_err(|e| GwError::from_io_read(format!("{path}: read failed: {e}"), &e))?;
    if buf.len() as u64 > cap {
        return Err(GwError::body_too_large(format!(
            "{path}: response body exceeds the {} MiB reader ceiling{} — refusing to buffer (never truncating)",
            cap / (1024 * 1024),
            len_hint
                .map(|l| format!(" (Content-Length {l} bytes)"))
                .unwrap_or_default()
        )));
    }
    String::from_utf8(buf)
        .map_err(|e| GwError::transport(format!("{path}: body is not UTF-8: {e}")))
}

/// Crate-internal door to the bounded reader at the default ceiling
/// (entities lane + tests).
pub(crate) fn read_body_capped_for_tests(resp: ureq::Response, path: &str) -> GwResult<String> {
    read_body_capped(resp, path, MAX_JSON_BODY_BYTES)
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
        // `#[WARNING:TIMEOUT]` REST control-plane agent (ADR-0027 §4).
        // These bound SHORT request/response JSON calls only — catalog reads,
        // run start, ledger pages, wait resolutions — never a model call and
        // never the ledger stream (which has its own agent below). A stalled
        // 60s read here is a transport fault, not a slow agent: run duration
        // is unaffected, because the run lives durably on the gateway and the
        // next poll picks it back up.
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(60))
            .timeout_write(Duration::from_secs(30))
            .build();
        // `#[WARNING:TIMEOUT]` SSE ledger-stream agent (ADR-0027 §4).
        // NOT a run deadline: the stream idles between records and the gateway
        // heartbeats with keep-alive comments, so this read timeout is an IDLE
        // WATCHDOG. On expiry the runner polls run status and reconnects from
        // its cursor (`runner.rs:2566`) — an agent thinking for an hour keeps
        // its run; only the socket is replaced. Must stay above the gateway's
        // heartbeat interval, or every quiet stretch costs a reconnect.
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

    /// Connection identity for sibling clients (the entity lane builds its
    /// own agents with different timeouts over the same connection).
    pub(crate) fn connection(&self) -> (String, Option<String>) {
        (self.base_url.clone(), self.token.clone())
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
        let body = read_body_capped(resp, path, MAX_JSON_BODY_BYTES)?;
        serde_json::from_str(&body)
            .map_err(|e| GwError::transport(format!("{path}: invalid JSON: {e}")))
    }

    fn post_json(&self, path: &str, payload: &Value) -> GwResult<Value> {
        self.post_json_via(&self.agent, path, payload)
    }

    /// `post_json` over a CALLER-CHOSEN agent. The retry path passes a
    /// throwaway agent so its attempt cannot land on a pooled socket:
    /// ureq returns a stream to the pool as soon as the body read hits
    /// EOF (`pool.rs::PoolReturnRead`), including the EOF that ENDED a
    /// failed read, so retrying through the shared agent can pick the
    /// very socket that just broke.
    fn post_json_via(&self, agent: &ureq::Agent, path: &str, payload: &Value) -> GwResult<Value> {
        let req = self.with_auth(
            agent
                .post(&self.url(path))
                .set("Accept", "application/json")
                .set("Content-Type", "application/json"),
        );
        let resp = req
            .send_string(&payload.to_string())
            .map_err(|e| err_from_ureq(path, e))?;
        let body = read_body_capped(resp, path, MAX_JSON_BODY_BYTES)?;
        if body.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body)
            .map_err(|e| GwError::transport(format!("{path}: invalid JSON: {e}")))
    }

    // -- probes --------------------------------------------------------------

    pub fn ping(&self) -> GwResult<Value> {
        self.get_json("/ping")
    }

    // -- attachments ----------------------------------------------------------

    /// Upload one file as a session attachment
    /// (`POST /attachments/upload`, multipart). Returns the WHOLE ref
    /// object (`$artifact`/`artifact_id`/`content_type`/`modality`/…) —
    /// callers forward it verbatim as `context.attachments[i]`. ureq 2.x
    /// has no multipart support, so the body is hand-encoded (the
    /// assistant's `_encode_multipart` precedent).
    pub fn upload_attachment(
        &self,
        session_id: &str,
        filename: &str,
        bytes: &[u8],
    ) -> GwResult<Value> {
        let path = "/attachments/upload";
        // Boundary from the id-minting entropy; never collides with file
        // bytes in practice (and multipart survives collision poorly, so
        // keep it long + random).
        let boundary = format!("acodeb{}", crate::config::mint_session_id());
        let body = encode_multipart(&boundary, session_id, filename, bytes);
        let req = self.with_auth(
            self.agent
                .post(&self.url(path))
                .set("Accept", "application/json")
                .set(
                    "Content-Type",
                    &format!("multipart/form-data; boundary={boundary}"),
                ),
        );
        let resp = req.send_bytes(&body).map_err(|e| err_from_ureq(path, e))?;
        let body = read_body_capped(resp, path, MAX_JSON_BODY_BYTES)?;
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| GwError::transport(format!("{path}: invalid JSON: {e}")))?;
        attachment_ref_from_response(&v)
            .ok_or_else(|| GwError::transport(format!("{path}: response carried no artifact ref")))
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

    /// Gateway-host GPU utilization (`/gpu` meter, OBS-6). Live shape
    /// (verified 2026-07-22, Apple silicon host):
    /// `{ts, supported: bool, source: "ioreg", utilization_gpu_pct: f64,
    ///   gpus: [{index, name, utilization_gpu_pct, ...}]}`.
    /// `supported:false` is an honest answer, not an error.
    pub fn host_gpu_metrics(&self) -> GwResult<Value> {
        self.get_json("/host/metrics/gpu")
    }

    /// Gateway capability contracts (`/discovery/capabilities`) — the
    /// server-declared contract set (`contracts.common.model_residency`
    /// / `host_state` / `session_caches`). The `/resources` surface is
    /// GATED on this: absent contract = the view says so, never guesses.
    pub fn discovery_capabilities(&self) -> GwResult<Value> {
        self.get_json("/discovery/capabilities")
    }

    /// Full host state (`/host/state`): memory, GPU, resident models
    /// (row_v1), session caches, totals, degraded lanes. SLOW by
    /// contract — fetched on modal open + explicit refresh only, never
    /// polled.
    pub fn host_state(&self) -> GwResult<Value> {
        self.get_json("/host/state")
    }

    /// Unload one resident model (`POST /models/unload`, admin). A
    /// locked model answers HTTP 409 `{error:"model_locked"}` — the
    /// caller offers `force` then.
    pub fn unload_model(&self, provider: &str, model: &str, force: bool) -> GwResult<Value> {
        self.post_json(
            "/models/unload",
            &json!({"provider": provider, "model": model, "force": force}),
        )
    }

    /// Lock a model resident (`POST /models/lock`, admin). The gateway
    /// ADOPTS externally-loaded models here — a model LM Studio or
    /// ollama put in memory (the residency sweep's rows, which arrive
    /// with `lockable: null`) becomes this gateway's to hold. So a null
    /// `lockable` is an UNKNOWN for the server to answer, never a
    /// refusal this client should make on its behalf.
    pub fn lock_model(&self, provider: &str, model: &str) -> GwResult<Value> {
        self.post_json(
            "/models/lock",
            &json!({"provider": provider, "model": model}),
        )
    }

    /// Release a residency lock (`POST /models/unlock`, admin).
    pub fn unlock_model(&self, provider: &str, model: &str) -> GwResult<Value> {
        self.post_json(
            "/models/unlock",
            &json!({"provider": provider, "model": model}),
        )
    }

    /// Context-fit estimate for one route
    /// (`GET /models/context_estimate`): `{confidence, predicted_max_context,
    /// notes[]}`. `context_length` rides along when the caller knows it.
    pub fn context_estimate(
        &self,
        provider: &str,
        model: &str,
        context_length: Option<u64>,
    ) -> GwResult<Value> {
        let mut path = format!(
            "/models/context_estimate?provider={}&model={}",
            url_encode(provider),
            url_encode(model)
        );
        if let Some(n) = context_length {
            path.push_str(&format!("&context_length={n}"));
        }
        self.get_json(&path)
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
            return Err(GwError::transport(format!(
                "runs/start: missing run_id in {resp}"
            )));
        }
        Ok(run_id)
    }

    pub fn get_run(&self, run_id: &str) -> GwResult<Value> {
        self.get_json(&format!("/runs/{}", url_encode(run_id)))
    }

    pub fn list_runs(&self, session_id: &str, limit: u32) -> GwResult<Value> {
        // include_ledger_len=false (session-reload investigation,
        // 2026-08-19): the gateway defaults the flag ON and, on
        // file-backed stores, LINE-READS every listed run's whole
        // ledger JSONL to compute it — a per-reload cost linear in the
        // session's ledger bytes, for a field this client never reads.
        // Pre-flag gateways ignore the unknown query param.
        self.get_json(&format!(
            "/runs?limit={limit}&session_id={}&root_only=true&include_ledger_len=false",
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
            .map_err(|e| GwError::from_io_read(format!("artifact read failed: {e}"), &e))?;
        if bytes.len() > max_bytes {
            return Err(GwError::transport(format!(
                "artifact larger than {max_bytes} bytes; not rendering inline"
            )));
        }
        Ok((bytes, content_type))
    }

    // -- commands ---------------------------------------------------------------

    pub fn submit_command(&self, run_id: &str, typ: &str, payload: Value) -> GwResult<Value> {
        self.submit_command_with_id(&mint_command_id(), run_id, typ, payload)
    }

    /// Command submission with a CALLER-MINTED id: the durable command
    /// store dedups by `command_id` (runtime receipt c5541 — a repeat
    /// returns the original seq, never a double-apply), so retries MUST
    /// reuse the same id. The quit lane's dedicated send mints at the
    /// choice site and passes it here.
    pub fn submit_command_with_id(
        &self,
        command_id: &str,
        run_id: &str,
        typ: &str,
        payload: Value,
    ) -> GwResult<Value> {
        let body = json!({
            "command_id": command_id,
            "run_id": run_id,
            "type": typ,
            "payload": payload,
            "client_id": "abstractcode-tui",
        });
        self.post_json("/commands", &body)
    }

    /// Submit a command with ONE same-id retry on a transient transport
    /// failure, and report whether any attempt was AMBIGUOUS.
    ///
    /// This is the single owner of the command retry policy. It exists
    /// because the policy was implemented in the quit lane and nowhere
    /// else, so a steer that hit a reset socket was simply lost — the
    /// live failure of 2026-08-20 ("steer not delivered: … Connection
    /// reset by peer"). ureq will not cover this for us: it re-opens
    /// only when the PRELUDE write fails on a recycled connection, and
    /// its post-response retry is restricted to idempotent methods with
    /// an empty body (`unit.rs::is_retryable`) — a POST with a JSON body
    /// is neither, so a reset arriving after the prelude reaches us raw.
    ///
    /// The retry is exactly-once by construction: the durable command
    /// store dedups on `command_id` (runtime receipt c5541), so a repeat
    /// of an accepted command returns the original seq instead of
    /// applying twice. That is why the id is minted by the CALLER and
    /// reused verbatim here.
    ///
    /// `ambiguous` = some attempt may have LEFT and only its answer was
    /// lost (timeout or body-level transport). A connect-level failure
    /// (nobody there) and an HTTP status (the server spoke) are both
    /// unambiguous. It is sticky across attempts: a first attempt that
    /// may have landed makes the final verdict non-definitive even if
    /// the retry fails cleanly.
    pub fn submit_command_retried(
        &self,
        command_id: &str,
        run_id: &str,
        typ: &str,
        payload: Value,
    ) -> (GwResult<Value>, bool) {
        let ambiguous = |e: &GwError| e.status.is_none() && !e.is_gone();
        let first = self.submit_command_with_id(command_id, run_id, typ, payload.clone());
        let Err(e) = &first else {
            return (first, false);
        };
        let saw_ambiguous = ambiguous(e);
        if !e.is_transient() {
            return (first, saw_ambiguous);
        }
        let second = self.submit_command_on_a_fresh_connection(command_id, run_id, typ, payload);
        let saw_ambiguous = saw_ambiguous || second.as_ref().err().is_some_and(ambiguous);
        (second, saw_ambiguous)
    }

    /// The retry attempt: same command id, guaranteed-new socket.
    fn submit_command_on_a_fresh_connection(
        &self,
        command_id: &str,
        run_id: &str,
        typ: &str,
        payload: Value,
    ) -> GwResult<Value> {
        let body = json!({
            "command_id": command_id,
            "run_id": run_id,
            "type": typ,
            "payload": payload,
            "client_id": "abstractcode-tui",
        });
        // A throwaway agent owns an empty pool, so this cannot reuse the
        // socket the first attempt just broke. Same timeouts as the
        // control-plane agent (ADR-0027 §4).
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(60))
            .timeout_write(Duration::from_secs(30))
            .build();
        self.post_json_via(&agent, "/commands", &body)
    }

    /// Per-model reasoning capability probe (first-citizen picker stage 3):
    /// the gateway's model-capabilities lookup — runtime discovery facade
    /// over core's registry. Reads `thinking_support` + `reasoning_levels`
    /// (+ `capability_source` when core ships the provenance ask).
    pub fn model_capabilities(&self, provider: &str, model: &str) -> GwResult<Value> {
        let name = if provider.is_empty() {
            model.to_string()
        } else {
            format!("{provider}/{model}")
        };
        self.get_json(&format!(
            "/discovery/models/capabilities?model_name={}",
            url_encode(&name)
        ))
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

    /// Steer a live run. Returns the outcome plus whether the guidance
    /// may have landed despite the error (see
    /// [`GatewayClient::submit_command_retried`]) — the operator is told
    /// "not delivered" only when that is actually known.
    pub fn steer(&self, run_id: &str, guidance: &str) -> (GwResult<Value>, bool) {
        self.submit_command_retried(
            &mint_command_id(),
            run_id,
            "inject_guidance",
            json!({ "guidance": guidance }),
        )
    }

    /// The gateway's OWN view of recent work, across every session
    /// (`/sessions` discovery, 2026-08-28). No `session_id` filter: the
    /// answer carries `session_id` on each row, so grouping it yields
    /// the sessions this GATEWAY knows about — which is the only
    /// honest source when the client is pointed at a remote gateway it
    /// has never talked to before. The picker used to render a local
    /// prefs file, so a fresh client saw an empty list against a
    /// gateway holding a hundred live sessions.
    ///
    /// `root_only=true` (a session's turns are its root runs) and
    /// `include_ledger_len=false` (the gateway line-reads every listed
    /// run's whole ledger for that field on file-backed stores — a
    /// per-call cost linear in ledger bytes, for a field nothing here
    /// reads). One request, at the gesture, never polled.
    pub fn list_recent_runs(&self, limit: u32) -> GwResult<Value> {
        self.get_json(&Self::session_listing_path(limit))
    }

    /// The exact query the session board sends. Extracted so it can be
    /// PINNED: the gateway REFUSES unknown query parameters with a 400
    /// (`routes/gateway.py`, the c5253 filter-typo fix), so a renamed
    /// or mistyped param does not degrade — it kills the board. A
    /// sabotage of this string left the whole suite green.
    pub(crate) fn session_listing_path(limit: u32) -> String {
        // NO client-side ceiling. The route documents "No server
        // ceiling — an explicit larger ask is served", and a self-
        // imposed clamp of 500 was the bug behind the board's two
        // worst lies (2026-08-29): the page covers the most recent N
        // ROOT RUNS across all sessions, so on a store with 119
        // sessions it missed most of them entirely — every missing one
        // rendered "not on the gateway" — and it saw only a slice of
        // each surviving session's turns, so every count read "1+".
        // Asking for the whole store makes the counts TOTALS and makes
        // absence provable; `has_more` says when it still is not.
        format!(
            "/runs?limit={}&root_only=true&include_ledger_len=false",
            limit.max(1)
        )
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
        // detail=replay (runtime R2, live-verified 2026-07-25): drops
        // request-side prompts + observability metadata the fold never
        // reads — measured 4.1x smaller on the 14.3MB incident bundle
        // with ZERO read-set mismatches across 1,890 records (c5645
        // receipt). Pre-R2 gateways ignore the unknown query param.
        let mut path = format!("/runs/{}/history_bundle?detail=replay", url_encode(run_id));
        if include_session {
            path.push_str(&format!(
                "&include_session=true&session_turn_limit={}",
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
    /// running cursor reported through `on_cursor`. Malformed `step`
    /// events (undecodable JSON, or a valid envelope with no record
    /// object) are SKIPPED and COUNTED through `on_skipped(n)` per read
    /// (F7 — they used to vanish silently); the good records around them
    /// keep folding. Returns:
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
        mut on_skipped: impl FnMut(usize),
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
                    // Timeouts already returned above; what remains is a
                    // mid-stream break (reset/abort) — transport-class,
                    // never single-handed "unreachable" evidence.
                    return Err(GwError::from_io_read(
                        format!("stream read failed: {e}"),
                        &e,
                    ));
                }
            };
            let mut records: Vec<Value> = Vec::new();
            let mut skipped = 0usize;
            let mut saw_done = false;
            for ev in parser.push(&buf[..n]) {
                match ev.event.as_str() {
                    "step" => match parse_step_data(&ev.data) {
                        Ok((cursor, record)) => {
                            on_cursor(cursor);
                            match record {
                                Some(r) => records.push(r),
                                None => skipped += 1,
                            }
                        }
                        Err(()) => skipped += 1,
                    },
                    "done" => saw_done = true,
                    _ => {}
                }
            }
            if !records.is_empty() {
                on_batch(records);
            }
            if skipped > 0 {
                on_skipped(skipped);
            }
            if saw_done {
                return Ok(true);
            }
        }
    }
}

/// Parse one SSE `step` event's data (the `{cursor, record}` envelope).
/// `Ok((cursor, Some(record)))` = a well-formed record; `Ok((cursor,
/// None))` = a valid envelope carrying NO record object (counts as a
/// skip, but the cursor still advances so a poisoned record cannot
/// wedge reconnect loops — the gateway ledger keeps the byte truth);
/// `Err(())` = undecodable JSON (counts as a skip; no cursor to trust).
pub(crate) fn parse_step_data(data: &str) -> Result<(u64, Option<Value>), ()> {
    let v: Value = serde_json::from_str(data).map_err(|_| ())?;
    let cursor = v.get("cursor").and_then(Value::as_u64).unwrap_or(0);
    let record = v.get("record").filter(|r| r.is_object()).cloned();
    Ok((cursor, record))
}

/// Hand-rolled multipart/form-data body for the attachment upload
/// (ureq 2.x has no multipart). Parts: `session_id`, `filename` (the
/// server-preferred name field), `file` (bytes). CRLF discipline per
/// RFC 2046; the filename inside the part HEADER is sanitized against
/// header injection (CR/LF/`"` stripped) — the dedicated `filename`
/// field carries the real name.
pub(crate) fn encode_multipart(
    boundary: &str,
    session_id: &str,
    filename: &str,
    bytes: &[u8],
) -> Vec<u8> {
    // Sanitize BOTH filename copies: the part header against quote/CRLF
    // injection, and the dedicated field against control bytes — the
    // field value lands verbatim in artifact metadata and later renders
    // into the "Stored session attachments" SYSTEM message, so a
    // newline in one's own filename would be prompt injection by
    // filename (legal in POSIX names).
    let field_name: String = filename.chars().filter(|c| !c.is_control()).collect();
    let header_name: String = field_name.chars().filter(|c| *c != '"').collect();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + 512);
    let text_part = |name: &str, value: &str, out: &mut Vec<u8>| {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        out.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    };
    text_part("session_id", session_id, &mut out);
    text_part("filename", &field_name, &mut out);
    // Honest content type (extension-guessed): the server derives the
    // artifact's MODALITY from it — octet-stream would demote a .md to
    // modality "file" and skip the text-inlining lane.
    let ctype = guess_content_type(filename);
    text_part("content_type", ctype, &mut out);
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{header_name}\"\r\n\
             Content-Type: {ctype}\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(bytes);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

/// Extension-based content-type guess for uploads (conservative: the
/// kinds the runtime treats specially — text-likes inline, images ride
/// the VLM media path, PDF extracts server-side; everything else is an
/// honest octet-stream).
pub(crate) fn guess_content_type(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "log" | "text" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "js" | "mjs" => "text/javascript",
        "ts" | "tsx" | "jsx" | "py" | "rs" | "go" | "java" | "c" | "h" | "cpp" | "hpp" | "sh"
        | "bash" | "zsh" | "toml" | "ini" | "cfg" | "sql" | "rb" | "php" | "swift" | "kt"
        | "tex" | "rst" => "text/plain",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        // Both tiff spellings (core's canonical raster set, c5574 —
        // mimetypes.guess_type answers image/tiff for both; the old
        // octet-stream fallback made the server classify a declared
        // image as modality "file").
        "tif" | "tiff" => "image/tiff",
        // Deliberately NOT image/svg+xml: the server derives modality
        // from this, and the image modality rides the provider VLM
        // media path where raster decoders reject SVG — as XML text it
        // inlines READABLY instead.
        "svg" => "application/xml",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// Extract the attachment ref from the upload response: `artifact`
/// first, `attachment` fallback (the route returns BOTH — the same
/// object; tonight's probe saw `artifact`, the assistant reads
/// `attachment`). A ref must carry `$artifact` to be usable.
pub(crate) fn attachment_ref_from_response(v: &Value) -> Option<Value> {
    for key in ["artifact", "attachment"] {
        if let Some(r) = v.get(key) {
            if r.get("$artifact").and_then(Value::as_str).is_some() {
                return Some(r.clone());
            }
        }
    }
    None
}

/// Mint one durable-command id (the gateway's dedup key). Minted ONCE
/// per logical command; every retry of that command reuses it.
pub fn mint_command_id() -> String {
    format!(
        "cmd_{}",
        crate::config::mint_session_id().trim_start_matches("acode-")
    )
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

    /// The session board's query is PINNED, param by param.
    ///
    /// The gateway REFUSES unknown query parameters with a 400 rather
    /// than ignoring them (its own filter-typo fix), so a renamed or
    /// camel-cased param does not degrade the board — it kills it. A
    /// sabotage that rewrote this string to `?rootOnly=true&sort=desc`
    /// left the whole suite green (adversarial review 2026-08-29), so
    /// the exact spelling is asserted here.
    #[test]
    fn the_session_listing_query_is_exactly_what_the_gateway_accepts() {
        let p = GatewayClient::session_listing_path(200);
        assert_eq!(p, "/runs?limit=200&root_only=true&include_ledger_len=false");
        // No session filter: the whole point is discovering sessions
        // this client has never heard of.
        assert!(!p.contains("session_id"));
        // Only the FLOOR is clamped (`ge=1` on the route). There is
        // deliberately no ceiling: the route documents that an
        // explicit larger ask is served, and a self-imposed cap of 500
        // was what made the board miss most of a 119-session gateway
        // and report every turn count as "1+".
        assert!(GatewayClient::session_listing_path(0).contains("limit=1"));
        assert!(GatewayClient::session_listing_path(5000).contains("limit=5000"));
    }

    #[test]
    fn capped_reader_parses_big_bodies_and_refuses_over_cap_honestly() {
        // The replay-integrity class: ureq's into_string() silently
        // capped at 10 MiB and every >10 MiB history_bundle turn was
        // deterministically unreplayable. The bounded reader must (a)
        // pass bodies FAR beyond that, (b) refuse over-ceiling bodies
        // with a TYPED error naming the cap — never truncation.
        let big = format!("{{\"pad\": \"{}\"}}", "x".repeat(12 * 1024 * 1024));
        let resp = ureq::Response::new(200, "OK", &big).expect("test response");
        let body = read_body_capped(resp, "/test", MAX_JSON_BODY_BYTES)
            .expect("12 MiB passes the 256 MiB ceiling");
        assert_eq!(body.len(), big.len(), "no silent truncation");
        assert!(serde_json::from_str::<Value>(&body).is_ok());

        let resp = ureq::Response::new(200, "OK", &big).expect("test response");
        let err = read_body_capped(resp, "/test", 1024 * 1024).expect_err("over-cap refuses");
        assert_eq!(err.kind, GwErrorKind::BodyTooLarge);
        assert!(
            err.to_string().contains("1 MiB") && err.to_string().contains("never truncating"),
            "the error names the ceiling: {err}"
        );
        assert!(!err.is_gone(), "a big body is never gateway-down evidence");
        assert!(!err.is_transient(), "deterministic — never retried");
    }

    #[test]
    fn multipart_encoding_is_byte_exact_and_sanitizes_header_names() {
        let body = encode_multipart("BB", "sid-1", "re\"po\nrt.pdf", b"DATA");
        let s = String::from_utf8_lossy(&body);
        // Golden frame: four parts + terminator, CRLF discipline. The
        // content type is extension-guessed (server derives modality
        // from it); BOTH filename copies sanitize — control bytes
        // stripped from the field (prompt-injection-by-filename), the
        // header additionally drops quotes (header injection).
        assert_eq!(
            s,
            "--BB\r\nContent-Disposition: form-data; name=\"session_id\"\r\n\r\nsid-1\r\n\
             --BB\r\nContent-Disposition: form-data; name=\"filename\"\r\n\r\nre\"port.pdf\r\n\
             --BB\r\nContent-Disposition: form-data; name=\"content_type\"\r\n\r\napplication/pdf\r\n\
             --BB\r\nContent-Disposition: form-data; name=\"file\"; filename=\"report.pdf\"\r\n\
             Content-Type: application/pdf\r\n\r\nDATA\r\n--BB--\r\n"
        );
        assert_eq!(guess_content_type("notes.MD"), "text/markdown");
        assert_eq!(guess_content_type("photo.jpeg"), "image/jpeg");
        assert_eq!(guess_content_type("blob"), "application/octet-stream");
        // Both tiff spellings map to image/tiff (core c5574 — the
        // octet-stream gap made a declared image ride modality "file").
        assert_eq!(guess_content_type("scan.tif"), "image/tiff");
        assert_eq!(guess_content_type("scan.TIFF"), "image/tiff");
    }

    #[test]
    fn attachment_ref_reads_artifact_first_then_attachment_and_demands_id() {
        let both = serde_json::json!({
            "artifact": {"$artifact": "a1", "filename": "x"},
            "attachment": {"$artifact": "a2"}
        });
        assert_eq!(
            attachment_ref_from_response(&both).unwrap()["$artifact"],
            "a1"
        );
        let alias_only = serde_json::json!({"attachment": {"$artifact": "a2"}});
        assert_eq!(
            attachment_ref_from_response(&alias_only).unwrap()["$artifact"],
            "a2"
        );
        // A ref without $artifact is unusable — refuse, never guess.
        let bad = serde_json::json!({"artifact": {"artifact_id": "a3"}});
        assert!(attachment_ref_from_response(&bad).is_none());
    }

    #[test]
    fn url_encoding_is_conservative() {
        assert_eq!(url_encode("run-1_2.3~x"), "run-1_2.3~x");
        assert_eq!(url_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(url_encode("é"), "%C3%A9");
    }

    #[test]
    fn step_parse_separates_good_records_from_counted_skips() {
        // F7: a well-formed envelope folds; a corrupted one COUNTS as a
        // skip instead of vanishing silently.
        let good = r#"{"cursor": 7, "record": {"run_id": "r1", "status": "completed"}}"#;
        let (cursor, rec) = parse_step_data(good).expect("well-formed parses");
        assert_eq!(cursor, 7);
        assert_eq!(rec.expect("record present").get("run_id").unwrap(), "r1");

        // Undecodable JSON (truncated mid-write): a skip, no cursor.
        assert!(parse_step_data(r#"{"cursor": 8, "record": {"run_id"#).is_err());

        // Valid envelope, no record object: a skip — but the cursor still
        // advances so a poisoned record cannot wedge reconnect loops.
        let (cursor, rec) = parse_step_data(r#"{"cursor": 9}"#).expect("envelope parses");
        assert_eq!(cursor, 9);
        assert!(rec.is_none());
        // A non-object record (null/string) is malformed too.
        let (_, rec) = parse_step_data(r#"{"cursor": 10, "record": null}"#).unwrap();
        assert!(rec.is_none());
    }

    /// The false-"unreachable" regression (operator report 2026-07-23:
    /// the app repeated "gateway unreachable" while `/api/health`
    /// answered in ~1ms). Classification must come from REAL transport
    /// errors, so this test manufactures both shapes on live sockets:
    ///
    /// - connect REFUSED (a just-freed port): genuine gone-evidence —
    ///   kind `Unreachable`, `is_gone()`, worded "gateway unreachable".
    /// - read TIMEOUT (a bound listener that never answers — the kernel
    ///   completes the TCP handshake via the backlog, so connect
    ///   succeeds and the response never comes): kind `Timeout`, NOT
    ///   `is_gone()`, worded "gateway timed out", never "unreachable".
    #[test]
    fn transport_classification_separates_refused_from_timeout() {
        // Refused: bind to grab a free port, then drop the listener.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            l.local_addr().unwrap().port()
        };
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(500))
            .timeout_read(Duration::from_millis(500))
            .build();
        let err = agent
            .get(&format!("http://127.0.0.1:{port}/api/gateway/ping"))
            .call()
            .expect_err("nothing listens on a dropped port");
        let gw = err_from_ureq("/ping", err);
        assert_eq!(gw.kind, GwErrorKind::Unreachable, "refused = gone");
        assert!(gw.is_gone());
        assert!(
            gw.to_string().starts_with("gateway unreachable: "),
            "refused wording: {gw}"
        );

        // Timeout: a listener that accepts (kernel backlog) but never
        // answers — the reachable-but-silent gateway shape.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let err = agent
            .get(&format!("http://127.0.0.1:{port}/api/gateway/ping"))
            .call()
            .expect_err("no response ever comes");
        let gw = err_from_ureq("/ping", err);
        assert_eq!(
            gw.kind,
            GwErrorKind::Timeout,
            "silence is a timeout, never gone-evidence: {gw}"
        );
        assert!(!gw.is_gone());
        assert!(
            gw.to_string().starts_with("gateway timed out: "),
            "timeout wording must not claim unreachable: {gw}"
        );
        drop(listener);
    }

    /// Read ONE complete HTTP request (headers + `Content-Length` body).
    /// A server that closes with the request body still unread makes the
    /// kernel emit RST, which shows up as spurious client-side errors —
    /// the tests below must fail for the reason they are testing.
    #[cfg(test)]
    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        let mut raw: Vec<u8> = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            raw.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&raw).to_string();
            let Some(head_end) = text.find("\r\n\r\n") else {
                continue;
            };
            let want: usize = text[..head_end]
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    (k.trim().eq_ignore_ascii_case("content-length"))
                        .then(|| v.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if raw.len() >= head_end + 4 + want {
                break;
            }
        }
        String::from_utf8_lossy(&raw).to_string()
    }

    /// The live steer failure of 2026-08-20: "steer not delivered:
    /// /commands: … Network Error: Connection reset by peer (os error 54)".
    ///
    /// ureq protects only part of this class. It silently re-opens when
    /// the PRELUDE write fails on a recycled connection (so a socket
    /// already known-dead is handled), and it retries a closed response
    /// only for idempotent methods with an empty body
    /// (`unit.rs::is_retryable`). A POST carrying a JSON body — every
    /// command this client sends — is NEITHER, so a reset that lands
    /// after the prelude went out reaches the caller untouched.
    ///
    /// This builds that exact shape: the second connection is accepted
    /// and then closed with the request still unread, which makes the
    /// kernel answer with RST. The command must still be delivered,
    /// because `submit_command_with_id` retries once with the SAME
    /// command id and the durable store dedups on it.
    #[test]
    fn a_reset_after_the_prelude_still_delivers_the_command() {
        use std::io::Write;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(AtomicUsize::new(0));
        let seen_srv = seen.clone();
        let server = std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let Ok(mut stream) = stream else { break };
                let n = seen_srv.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    // First connection: accept the request, then close it
                    // WITHOUT reading — unread bytes in the receive queue
                    // make close() emit RST, which is exactly what the
                    // operator saw (os error 54).
                    std::thread::sleep(Duration::from_millis(60));
                    drop(stream);
                    continue;
                }
                // The retry's connection: behave.
                let _ = read_http_request(&mut stream);
                let body = "{\"ok\":true}";
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
                let _ = stream.flush();
            }
        });

        let client = GatewayClient::new(&format!("http://127.0.0.1:{port}"), None);
        let (delivered, ambiguous) = client.steer("run-1", "please use AbstractTUI");
        // Never join: without the retry the server is still parked on
        // accept(), and a hung test says nothing.
        drop(server);
        assert_eq!(
            seen.load(Ordering::SeqCst),
            2,
            "the client must try again after the reset"
        );
        assert!(
            delivered.is_ok(),
            "a steer must survive one reset — the command store dedups the \
             same-id retry, so losing the operator's words here is pure loss: {:?}",
            delivered.err()
        );
        assert!(
            ambiguous,
            "the first attempt may have landed before the reset — the caller \
             must be told the outcome is not definitive"
        );
    }

    /// The retry is EXACTLY-ONCE or it is a bug: the durable command
    /// store dedups on `command_id`, so both attempts must carry the
    /// same one. A fresh id per attempt would double-apply a steer that
    /// actually landed — which is precisely why `steer` mints once and
    /// hands the id to the retry policy.
    #[test]
    fn both_attempts_carry_the_same_command_id() {
        use std::io::Write;
        use std::sync::mpsc;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().take(2).enumerate() {
                let Ok(mut stream) = stream else { break };
                let _ = tx.send(read_http_request(&mut stream));
                if n == 0 {
                    // A response that dies mid-body: deterministic
                    // transport failure (no socket-state race), which is
                    // what the policy retries. The point of THIS test is
                    // the id, not the flavour of the break.
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort",
                    );
                    let _ = stream.flush();
                    drop(stream);
                } else {
                    let body = "{}";
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    );
                    let _ = stream.flush();
                }
            }
        });

        let client = GatewayClient::new(&format!("http://127.0.0.1:{port}"), None);
        let (outcome, _) = client.steer("run-1", "keep going");
        assert!(outcome.is_ok(), "the retry lands: {:?}", outcome.err());
        let first = rx.recv_timeout(Duration::from_secs(5)).expect("attempt 1");
        let second = rx.recv_timeout(Duration::from_secs(5)).expect("attempt 2");
        let id_of = |req: &str| -> String {
            let body = req.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
            serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("command_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default()
        };
        let (a, b) = (id_of(&first), id_of(&second));
        assert!(!a.is_empty(), "attempt 1 carries a command id: {first}");
        assert_eq!(
            a, b,
            "the retry MUST reuse the id — dedup is what makes it safe"
        );
    }

    #[test]
    fn http_status_errors_prove_reachability() {
        // An HTTP answer of ANY code is proof the gateway is there — the
        // doctor's own rule (cli.rs reachability check). Wording keeps
        // the status; is_gone() is false.
        let resp = ureq::Response::new(500, "Internal Server Error", "boom").unwrap();
        let gw = err_from_ureq("/ping", ureq::Error::Status(500, resp));
        assert_eq!(gw.status, Some(500));
        assert_eq!(gw.kind, GwErrorKind::Http);
        assert!(!gw.is_gone());
        assert!(gw.to_string().starts_with("gateway HTTP 500: "), "{gw}");
    }

    #[test]
    fn io_read_classification_covers_both_unix_timeout_kinds() {
        // Body-read timeouts arrive as WouldBlock on unix sockets
        // (EAGAIN) and TimedOut where ureq normalizes — both must read
        // as Timeout; a reset stays transport-class ("request failed").
        for k in [std::io::ErrorKind::TimedOut, std::io::ErrorKind::WouldBlock] {
            let e = std::io::Error::new(k, "slow");
            let gw = GwError::from_io_read("read failed".into(), &e);
            assert_eq!(gw.kind, GwErrorKind::Timeout, "{k:?}");
        }
        let reset = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset");
        let gw = GwError::from_io_read("read failed".into(), &reset);
        assert_eq!(gw.kind, GwErrorKind::Transport);
        assert!(gw.to_string().starts_with("gateway request failed: "));
        let refused = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        assert!(GwError::from_io_read("read failed".into(), &refused).is_gone());
    }
}
