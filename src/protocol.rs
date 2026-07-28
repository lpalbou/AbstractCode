//! Pure extraction over gateway ledger records.
//!
//! A ledger record is a JSON object:
//! `{run_id, node_id, status: started|completed|waiting|failed, effect:
//! {type, payload}, result: {content?, reasoning?, usage?, wait?, output?,
//! results?}, started_at, ended_at, error}`.
//!
//! These functions are a faithful port of the reference thin clients
//! (`abstractcode/web/src/lib/*` and the AbstractAssistant gateway adapter):
//! the canonical wait location is `result.wait`, tool approvals are
//! `details.mode == "approval_required"` (or embedded `details.tool_calls`),
//! flow output lives on `result.output`, and usage rides completed
//! `llm_call` results.

use serde_json::Value;

fn s(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

pub fn record_run_id(rec: &Value) -> String {
    s(rec, "run_id")
}

pub fn record_node_id(rec: &Value) -> String {
    s(rec, "node_id")
}

pub fn record_status(rec: &Value) -> String {
    s(rec, "status").to_lowercase()
}

pub fn effect_type(rec: &Value) -> String {
    rec.get("effect")
        .and_then(|e| e.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

// ---------------------------------------------------------------------------
// Waits
// ---------------------------------------------------------------------------

/// The canonical wait location is `record.result.wait` (written by
/// `StepRecord.finish_waiting` in abstractruntime).
pub fn extract_wait(rec: &Value) -> Option<&Value> {
    let wait = rec.get("result")?.get("wait")?;
    if wait.is_object() {
        Some(wait)
    } else {
        None
    }
}

pub fn wait_reason(wait: &Value) -> String {
    // Runtime enums may serialize as {"value": "user"}; accept both.
    match wait.get("reason") {
        Some(Value::String(x)) => x.trim().to_string(),
        Some(other) => other
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
        None => String::new(),
    }
}

pub fn wait_key(wait: &Value) -> String {
    s(wait, "wait_key")
}

pub fn wait_prompt(wait: &Value) -> String {
    s(wait, "prompt")
}

/// Tool calls embedded in a wait — both keys are runtime-written fields
/// on the SAME details assembly (effect_handlers.py TOOL_CALLS wait
/// branch): `details.tool_calls` always; `details.tool_calls_for_evidence`
/// when pre-executed/blocked calls were split out of the approval batch.
pub fn tool_calls_from_wait(wait: &Value) -> Vec<Value> {
    let details = match wait.get("details") {
        Some(d) if d.is_object() => d,
        _ => return Vec::new(),
    };
    for key in ["tool_calls", "tool_calls_for_evidence"] {
        if let Some(Value::Array(items)) = details.get(key) {
            return items.clone();
        }
    }
    Vec::new()
}

/// Every branch is a runtime-minted contract, not a text heuristic
/// (verified against abstractruntime source, 2026-07-23):
/// - `details.mode == "approval_required"` is the CANONICAL discriminator
///   — the wait details are assembled as `{mode, tool_calls, executor?}`
///   (integrations/abstractcore/effect_handlers.py, TOOL_CALLS wait
///   branch), and the runtime's own resume path keys on exactly this
///   check (core/runtime.py, thin-client approval resume).
/// - the `tool_approval:` wait-key prefix is the ApprovalToolExecutor's
///   key factory (`f"tool_approval:{uuid4().hex}"`,
///   integrations/abstractcore/tool_executor.py).
/// - `details.executor.kind == "tool_approval"` is the executor's own
///   detail dict (`{"kind": "tool_approval", ...}`), nested under
///   `executor` by the same effect-handler assembly.
pub fn is_tool_approval_wait(wait: &Value) -> bool {
    let wk = wait_key(wait).to_lowercase();
    if wk.starts_with("tool_approval") {
        return true;
    }
    let details = match wait.get("details") {
        Some(d) if d.is_object() => d,
        _ => return false,
    };
    if s(details, "mode").eq_ignore_ascii_case("approval_required") {
        return true;
    }
    if let Some(executor) = details.get("executor") {
        if s(executor, "kind").eq_ignore_ascii_case("tool_approval") {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Subworkflow spawns (the parent ledger's declaration of its child)
// ---------------------------------------------------------------------------

/// The facts a parent's subworkflow WAIT record declares about its child
/// — the structural currency for answer-source binding (the ledger knows
/// which workflow the child runs; nothing needs to be inferred from the
/// child's later behavior).
///
/// Field sources (abstractruntime core/runtime.py,
/// `_handle_start_subworkflow` — identical on the sync and async wait
/// shapes; live-verified against gateway run 76fc3fcb… 2026-07-23):
/// - `sub_run_id`: `result.wait.details.sub_run_id` / the wait-key form.
/// - `workflow_id`: `result.wait.details.sub_workflow_id`, with
///   `effect.payload.workflow_id` as the belt — the handler REQUIRES the
///   payload field ("start_subworkflow requires payload.workflow_id"),
///   so every spawn record carries it even where the details predate
///   `sub_workflow_id`. Empty only on pre-contract ledgers.
/// - `wrap_as_tool_result`: stamped into the wait details when the child
///   runs in TOOL MODE (`delegate_agent` and friends): such a child is a
///   tool observation for its parent BY CONTRACT — never the parent's
///   answer source. Load-bearing exclusion: a delegate child runs its
///   PARENT'S OWN workflow id (abstractagent react_runtime.py,
///   delegate_agent payload), so without this flag a root-level agent's
///   delegate would look answer-shaped by workflow id alone.
#[derive(Debug, Clone, PartialEq)]
pub struct SubworkflowSpawn {
    pub sub_run_id: String,
    pub workflow_id: String,
    pub wrap_as_tool_result: bool,
}

/// Read the spawn declaration off a subworkflow WAIT record. `None` when
/// the record is not a subworkflow wait carrying a child run id.
pub fn subworkflow_spawn(rec: &Value) -> Option<SubworkflowSpawn> {
    let wait = extract_wait(rec)?;
    let sub_run_id = subworkflow_run_id(wait)?;
    let details = wait.get("details").filter(|d| d.is_object());
    let payload = if effect_type(rec) == "start_subworkflow" {
        rec.get("effect")
            .and_then(|e| e.get("payload"))
            .filter(|p| p.is_object())
    } else {
        None
    };
    let workflow_id = details
        .map(|d| s(d, "sub_workflow_id"))
        .filter(|v| !v.is_empty())
        .or_else(|| {
            payload
                .map(|p| s(p, "workflow_id"))
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_default();
    let wrap_as_tool_result = details
        .and_then(|d| d.get("wrap_as_tool_result"))
        .or_else(|| payload.and_then(|p| p.get("wrap_as_tool_result")))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(SubworkflowSpawn {
        sub_run_id,
        workflow_id,
        wrap_as_tool_result,
    })
}

/// Sub-run id for a `subworkflow` wait: `details.sub_run_id`, else the
/// `subworkflow:<id>` wait-key form — both minted by the runtime's
/// `_handle_start_subworkflow` (`wait_key=f"subworkflow:{sub_run_id}"`,
/// abstractruntime core/runtime.py, sync + async wait sites).
pub fn subworkflow_run_id(wait: &Value) -> Option<String> {
    if !wait_reason(wait).eq_ignore_ascii_case("subworkflow") {
        return None;
    }
    if let Some(details) = wait.get("details") {
        let sub = s(details, "sub_run_id");
        if !sub.is_empty() {
            return Some(sub);
        }
    }
    let wk = wait_key(wait);
    if let Some(rest) = wk.strip_prefix("subworkflow:") {
        let sub = rest.trim();
        if !sub.is_empty() {
            return Some(sub.to_string());
        }
    }
    None
}

/// Event name from a wait key. The canonical runtime shape is
/// `evt:{scope}:{scope_id}:{name}` (minted by ONE function:
/// abstractruntime `core/event_keys.py` — the name may itself contain
/// dots and colons never appear in names) — the NAME is everything after
/// the third colon. Live example: `evt:run:<run_id>:abstract.status`.
pub fn event_name_from_wait_key(wk: &str) -> String {
    let wk = wk.trim();
    if let Some(rest) = wk.strip_prefix("evt:") {
        let mut parts = rest.splitn(3, ':');
        let _scope = parts.next();
        let _scope_id = parts.next();
        return parts.next().unwrap_or("").trim().to_string();
    }
    wk.to_string()
}

/// Normalize UI event names to the canonical `abstract.*` namespace.
pub fn normalize_ui_event_name(name: &str) -> String {
    let raw = name.trim();
    if let Some(rest) = raw.strip_prefix("abstractcode.") {
        return format!("abstract.{rest}");
    }
    raw.to_string()
}

/// True when a wait is an ask-the-user prompt (reason `user`, or reason
/// `event` on the `abstract.ask` event).
pub fn is_ask_user_wait(wait: &Value) -> bool {
    let reason = wait_reason(wait).to_lowercase();
    if reason == "user" {
        return true;
    }
    if reason == "event" {
        let name = normalize_ui_event_name(&event_name_from_wait_key(&wait_key(wait)));
        return name == "abstract.ask";
    }
    false
}

// ---------------------------------------------------------------------------
// Tool calls / results (completed `tool_calls` records)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallView {
    pub name: String,
    pub call_id: String,
    pub arguments: Option<Value>,
}

pub fn tool_call_view(tc: &Value) -> Option<ToolCallView> {
    let name = s(tc, "name");
    if name.is_empty() {
        return None;
    }
    let call_id = ["call_id", "id", "runtime_call_id"]
        .iter()
        .map(|k| s(tc, k))
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    Some(ToolCallView {
        name,
        call_id,
        arguments: tc.get("arguments").cloned(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResultView {
    pub name: String,
    pub call_id: String,
    pub arguments: Option<Value>,
    pub success: Option<bool>,
    pub error: String,
    pub output: Option<Value>,
}

/// Pair `effect.payload.tool_calls[]` with `result.results[]` by call id.
/// When ids are missing on both sides, pair by order (providers without
/// call ids exist; order is the only correlation left).
///
/// The payload call list is NOT guaranteed on terminal records: the
/// runtime's ledger slimming (abstractruntime 0067-M) replaces oversized
/// payload fields on WAITING/COMPLETED records with a `$slim` marker
/// pointing at the STARTED record — so a completed `tool_calls` record
/// for a big write_file carries `payload.tool_calls = {"$slim": …}`.
/// Result rows self-describe (`name`, `call_id`, `success`, `output`,
/// `error`), so when the payload list is unavailable the views build from
/// `result.results` directly — otherwise every slimmed tool completion
/// was invisible and restored cards froze at "awaiting approval".
pub fn tool_results_from_record(rec: &Value) -> Vec<ToolResultView> {
    if effect_type(rec) != "tool_calls" || record_status(rec) != "completed" {
        return Vec::new();
    }
    let calls: Vec<Value> = rec
        .get("effect")
        .and_then(|e| e.get("payload"))
        .and_then(|p| p.get("tool_calls"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let results: Vec<Value> = rec
        .get("result")
        .and_then(|r| r.get("results"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if calls.is_empty() {
        // Payload copy unavailable (slimmed / offloaded / absent): the
        // result rows are the source of truth. Rows with neither a name
        // nor a call id are uncorrelatable and skip (same posture as
        // nameless payload calls below).
        return results
            .iter()
            .filter_map(|r| {
                let name = s(r, "name");
                let call_id = ["call_id", "id"]
                    .iter()
                    .map(|k| s(r, k))
                    .find(|v| !v.is_empty())
                    .unwrap_or_default();
                if name.is_empty() && call_id.is_empty() {
                    return None;
                }
                Some(ToolResultView {
                    name,
                    call_id,
                    arguments: r.get("arguments").cloned(),
                    success: r.get("success").and_then(Value::as_bool),
                    error: s(r, "error"),
                    output: r
                        .get("output")
                        .or_else(|| r.get("result"))
                        .or_else(|| r.get("response"))
                        .cloned(),
                })
            })
            .collect();
    }

    let mut out = Vec::new();
    for (i, tc) in calls.iter().enumerate() {
        let view = match tool_call_view(tc) {
            Some(v) => v,
            None => continue,
        };
        let matched = if !view.call_id.is_empty() {
            results.iter().find(|r| {
                let rid = ["call_id", "id"]
                    .iter()
                    .map(|k| s(r, k))
                    .find(|v| !v.is_empty())
                    .unwrap_or_default();
                rid == view.call_id
            })
        } else {
            results.get(i)
        };
        let (success, error, output) = match matched {
            Some(r) => (
                r.get("success").and_then(Value::as_bool),
                s(r, "error"),
                r.get("output")
                    .or_else(|| r.get("result"))
                    .or_else(|| r.get("response"))
                    .cloned(),
            ),
            None => (None, String::new(), None),
        };
        out.push(ToolResultView {
            name: view.name,
            call_id: view.call_id,
            arguments: view.arguments,
            success,
            error,
            output,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// emit_event effects
// ---------------------------------------------------------------------------

pub struct EmitEvent {
    pub name: String,
    pub payload: Option<Value>,
}

pub fn extract_emit_event(rec: &Value) -> Option<EmitEvent> {
    let eff = rec.get("effect")?;
    if s(eff, "type") != "emit_event" {
        return None;
    }
    let payload = eff.get("payload")?;
    let name = {
        let n = s(payload, "name");
        if n.is_empty() {
            s(payload, "event_name")
        } else {
            n
        }
    };
    if name.is_empty() {
        return None;
    }
    Some(EmitEvent {
        name: normalize_ui_event_name(&name),
        payload: payload.get("payload").cloned(),
    })
}

/// The `abstract.status` payload -> status text ("" clears).
pub fn status_text_from_payload(payload: Option<&Value>) -> String {
    match payload {
        Some(Value::String(x)) => x.trim().to_string(),
        Some(v) if v.is_object() => {
            let t = s(v, "text");
            if t.is_empty() {
                s(v, "value")
            } else {
                t
            }
        }
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Flow output (final answer)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct FlowOutput {
    pub response: String,
    pub meta: Option<Value>,
    /// Set when the flow's final output (or its text key) was OFFLOADED
    /// by the runtime's ledger offloader: outputs over the inline cap
    /// (256 KB default) are replaced at persist time with
    /// `{"$artifact": id}`, and the read surface deliberately serves the
    /// ref unresolved (abstractruntime 0067-M: rehydrating list() was a
    /// 113x read amplification). Live consequence before this field: a
    /// heavy agent turn's final answer (answer + messages + scratchpad >
    /// 256 KB) folded as NOTHING — `finished` never flipped and the
    /// composer stayed captured for hours (run c61e4ac9…/0f2d487c…,
    /// 2026-07-22 — the maintainer's "never finishes" P0). The client
    /// must fetch `/runs/{run}/artifacts/{id}/content` for the words.
    pub offload_artifact: Option<String>,
}

fn pick_textish(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(x)) => x.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// The conventional flow-output text keys, in precedence order — THE
/// PROTOCOL CONTRACT for reading answer text out of a flow output, kept
/// deliberately as a documented ladder because the ledger genuinely
/// lacks a structural discriminator here: `result.output` is the flow's
/// AUTHORED output object, passed verbatim by the runtime's completion
/// writers (`_append_completion_record` call sites serialize
/// `run.output` untouched). The abstractagent ReAct workflow declares
/// `{"answer", "report", "iterations", "outcome", …}` (react_runtime.py
/// done/max_iterations nodes — `answer` is its text key), while wrapper
/// bundles author their own shapes; nothing in `abstractcode.agent.v1`
/// pins an output text key (recorded as a contract ask —
/// docs/roadmap/conformance-ledger-asks.md).
///
/// "report" is LAST deliberately: agent flows emit {answer, report}
/// (answer wins), but wrapper bundles like coding-agent/coder end with
/// {report, passed, delivered, …} and NO conventional text key — the
/// report IS the turn's answer there (live run b7d86e08…, 2026-07-22;
/// without it the fold never saw a final answer and the turn only
/// concluded via the terminal-status poll, silently).
const OUTPUT_TEXT_KEYS: [&str; 6] = ["answer", "response", "message", "text", "content", "report"];

/// Inline text from a flow-output OBJECT: the conventional keys in
/// precedence order, then ONE wrapper level down — the runtime's
/// resume/job completion records wrap the real output as
/// `{"success": true, "result": …}` (runtime.py `_append_completion_record`
/// call sites), so `output.result` is tried as a string or as an object
/// carrying the same conventional keys.
fn output_inline_text(out0: &Value) -> String {
    let flat = OUTPUT_TEXT_KEYS
        .iter()
        .map(|k| pick_textish(out0.get(*k)))
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    if !flat.is_empty() {
        return flat;
    }
    match out0.get("result") {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(v) if v.is_object() => OUTPUT_TEXT_KEYS
            .iter()
            .map(|k| pick_textish(v.get(*k)))
            .find(|t| !t.is_empty())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// The artifact id when `v` is the runtime offloader's ref shape
/// (`{"$artifact": id}`; extra keys tolerated — mirrors the runtime's
/// own `is_artifact_ref`, which checks key presence only).
fn offload_ref_id(v: &Value) -> Option<String> {
    let id = v.as_object()?.get("$artifact")?.as_str()?.trim();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// The offloaded-answer artifact id of a flow-output object: the whole
/// output replaced (`output == {"$artifact": id}` — the live c61e4ac9
/// shape), a conventional text key replaced (a >256 KB answer alone), or
/// the one-level `result` wrapper replaced.
fn output_offload_artifact(out0: &Value) -> Option<String> {
    if let Some(id) = offload_ref_id(out0) {
        return Some(id);
    }
    for k in OUTPUT_TEXT_KEYS {
        if let Some(v) = out0.get(k) {
            if let Some(id) = offload_ref_id(v) {
                return Some(id);
            }
        }
    }
    if let Some(res) = out0.get("result") {
        if let Some(id) = offload_ref_id(res) {
            return Some(id);
        }
    }
    None
}

/// True for a run-COMPLETION record: the runtime's terminal ledger write
/// carries `result.completed == true` (`_append_completion_record`, all
/// three call sites — normal flow end, job completion, resume
/// completion). No other record kind writes that key (`resume` results
/// say `{"resumed": true}`, `wait_until` says `{"ready": true}`, emits
/// say `{"emitted": true}`), so this is the key-independent "the run
/// itself ended" signal — the fold's honest fallback when the final
/// output carries no recognizable text.
pub fn is_flow_end_record(rec: &Value) -> bool {
    record_status(rec) == "completed"
        && rec
            .get("result")
            .and_then(|r| r.get("completed"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

/// True when a completed record's `result.output` is the RUN'S OWN final
/// output — the answer-eligibility gate.
///
/// The runtime's terminal writer (`_append_completion_record`) appends
/// with `effect=None` and stamps `result.completed == true`; that marker
/// is authoritative. But EFFECT records can also complete carrying a
/// `result.output`: a SYNC `start_subworkflow`'s completion result is
/// `{"sub_run_id": …, "output": <the CHILD's output>}` (core/runtime.py
/// `_handle_start_subworkflow`, completed branch) — the child's words on
/// the parent's ledger, which must never read as the parent's final
/// answer (the parent keeps executing after it). Acceptance:
/// - the completion marker → the run's own end, always;
/// - no marker: accepted when `output` is present and the result does
///   NOT self-identify as a subworkflow effect result (`sub_run_id`) —
///   a labeled `#FALLBACK` for distilled captures and ledgers that
///   predate the marker (reference-client parity; every LIVE flow end
///   checked on this gateway carries the marker, 2026-07-23).
pub fn is_run_output_record(rec: &Value) -> bool {
    if record_status(rec) != "completed" {
        return false;
    }
    let result = match rec.get("result") {
        Some(r) if r.is_object() => r,
        _ => return false,
    };
    if result
        .get("completed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    // #FALLBACK: pre-marker terminal shape (completed + output, not an
    // effect result that names a child run).
    result.get("output").is_some() && result.get("sub_run_id").is_none()
}

/// Extract the flow-end response from a record (`result.output` as string or
/// as an object with answer/response/message/text/content + artifact meta).
pub fn extract_flow_output(rec: &Value) -> Option<FlowOutput> {
    let out0 = rec.get("result")?.get("output")?;
    if let Value::String(x) = out0 {
        let response = x.trim().to_string();
        if response.is_empty() {
            return None;
        }
        return Some(FlowOutput {
            response,
            meta: None,
            offload_artifact: None,
        });
    }
    if !out0.is_object() {
        return None;
    }
    let msg = output_inline_text(out0);
    // Inline text wins; the offload ref is only consulted when the record
    // carries no readable words (the offloader replaces either the whole
    // output or individual oversized leaves).
    let offload_artifact = if msg.is_empty() {
        output_offload_artifact(out0)
    } else {
        None
    };
    let mut meta = out0
        .get("meta")
        .filter(|m| m.is_object())
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    for key in [
        "$artifact",
        "artifact",
        "image_artifact",
        "video_artifact",
        "audio_artifact",
        "music_artifact",
        "artifact_ref",
        "artifact_id",
        "content_type",
        "outputs",
        "resources",
        "success",
        "scratchpad",
        "report",
    ] {
        if let Some(v) = out0.get(key) {
            if !v.is_null() {
                meta[key] = v.clone();
            }
        }
    }
    // Content-type promotion (reference parity): a generic artifact with an
    // image/video/audio content type is promoted to its typed slot so the
    // renderer needs one lookup.
    let artifact = meta.get("artifact").filter(|a| a.is_object()).cloned();
    if let Some(artifact) = artifact {
        let content_type = artifact
            .get("content_type")
            .and_then(Value::as_str)
            .or_else(|| meta.get("content_type").and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_lowercase();
        for (prefix, slot) in [
            ("image/", "image_artifact"),
            ("video/", "video_artifact"),
            ("audio/", "audio_artifact"),
        ] {
            if content_type.starts_with(prefix) && meta.get(slot).is_none() {
                meta[slot] = artifact.clone();
            }
        }
    }
    let has_meta = meta.as_object().map(|m| !m.is_empty()).unwrap_or(false);
    if msg.is_empty() && !has_meta && offload_artifact.is_none() {
        return None;
    }
    Some(FlowOutput {
        response: msg,
        meta: if has_meta { Some(meta) } else { None },
        offload_artifact,
    })
}

/// Extract the answer TEXT from a fetched offloaded-output artifact
/// (`/runs/{run}/artifacts/{id}/content`). The offloader stores either
/// the serialized output subtree (`kind=json`, content-type
/// application/json) or a bare oversized string leaf (`kind=text`,
/// text/plain) — so the bytes are tried as JSON first (string value or
/// an object read through the SAME text-key precedence as a live flow
/// output), then served as plain text. Returns None only for
/// undecodable bytes or a JSON object with no readable text — callers
/// label that honestly instead of inventing.
pub fn answer_text_from_artifact(bytes: &[u8], content_type: &str) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let looks_json = content_type.to_ascii_lowercase().contains("json")
        || trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('"');
    if looks_json {
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            match &v {
                Value::String(s) => {
                    let s = s.trim();
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
                obj if obj.is_object() => {
                    let msg = output_inline_text(obj);
                    if !msg.is_empty() {
                        return Some(msg);
                    }
                    return None; // an object with no readable text: label, never invent
                }
                _ => {}
            }
        }
    }
    Some(trimmed.to_string())
}

// ---------------------------------------------------------------------------
// Usage (completed `llm_call` results)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageDelta {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    /// Prompt tokens served from the provider's cache (0 when the provider
    /// does not report cache hits — absence of evidence, not evidence of a
    /// cold cache).
    pub cached_tokens: u64,
}

fn as_u64(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().map(|f| f.max(0.0) as u64).unwrap_or(0),
        Some(Value::String(x)) => x.trim().parse::<u64>().unwrap_or(0),
        _ => 0,
    }
}

pub fn parse_usage(usage: &Value) -> UsageDelta {
    if !usage.is_object() {
        return UsageDelta::default();
    }
    let input = [
        "input_tokens",
        "prompt_tokens",
        "in_tokens",
        "prompt_eval_count",
    ]
    .iter()
    .map(|k| as_u64(usage.get(*k)))
    .find(|v| *v > 0)
    .unwrap_or(0);
    let output = [
        "output_tokens",
        "completion_tokens",
        "out_tokens",
        "eval_count",
    ]
    .iter()
    .map(|k| as_u64(usage.get(*k)))
    .find(|v| *v > 0)
    .unwrap_or(0);
    let total = ["total_tokens", "tokens"]
        .iter()
        .map(|k| as_u64(usage.get(*k)))
        .find(|v| *v > 0)
        .unwrap_or(input + output);
    // AbstractCore providers normalize cache hits to `cached_input_tokens`
    // (openai/anthropic/openai-compatible); raw provider spellings kept as
    // fallbacks for ledgers that carry the raw usage block.
    let cached = [
        "cached_input_tokens",
        "cache_read_input_tokens",
        "cached_tokens",
    ]
    .iter()
    .map(|k| as_u64(usage.get(*k)))
    .find(|v| *v > 0)
    .unwrap_or(0);
    // Raw provider blocks nest cache hits one level down: the Responses
    // API reports `input_tokens_details.cached_tokens`, Chat Completions
    // `prompt_tokens_details.cached_tokens` (live ledger, coder run
    // 0312b41d…: {"input_tokens_details": {"cached_tokens": 2560}, …}).
    let cached = if cached > 0 {
        cached
    } else {
        ["input_tokens_details", "prompt_tokens_details"]
            .iter()
            .filter_map(|k| usage.get(*k))
            .map(|d| as_u64(d.get("cached_tokens")))
            .find(|v| *v > 0)
            .unwrap_or(0)
    };
    UsageDelta {
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
        cached_tokens: cached,
    }
}

/// The model that actually served a completed llm_call (the result's own
/// `model` field — the resolved truth even when the run started with no
/// override, i.e. "gateway defaults").
pub fn model_from_record(rec: &Value) -> Option<String> {
    if effect_type(rec) != "llm_call" || record_status(rec) != "completed" {
        return None;
    }
    let result = rec.get("result")?;
    let model = result
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())?;
    Some(model.to_string())
}

/// Usage from a completed llm_call record (`result.usage | token_usage |
/// tokens`, with the nested `result.output.*` fallback the web client uses).
///
/// SPLITLESS REPAIR (live coder run 0312b41d…, gpt-5.6-sol via a proxy):
/// some provider paths normalize usage to `{input: 0, output: 0,
/// total: N}` while the SAME record's `result.raw_response.usage` carries
/// the provider's real split (`{"input_tokens": 2904, "output_tokens":
/// 276, …}`). When the normalized block parses splitless, the raw block
/// on the record fills the split — numbers from the provider's own usage
/// receipt on the same ledger record, never an estimate. Absent/`$slim`'d
/// raw stays splitless-honest (the total-only display path).
pub fn usage_from_record(rec: &Value) -> Option<UsageDelta> {
    if effect_type(rec) != "llm_call" || record_status(rec) != "completed" {
        return None;
    }
    let result = rec.get("result")?;
    let repair = |u: UsageDelta| -> UsageDelta {
        if u.input_tokens > 0 || u.output_tokens > 0 {
            return u;
        }
        match raw_response_usage(result) {
            Some(raw) if raw.input_tokens > 0 || raw.output_tokens > 0 => UsageDelta {
                input_tokens: raw.input_tokens,
                output_tokens: raw.output_tokens,
                // The normalized total is already the fold's honest
                // number when present; the raw total backfills a
                // zero-total block.
                total_tokens: if u.total_tokens > 0 {
                    u.total_tokens
                } else {
                    raw.total_tokens
                },
                cached_tokens: u.cached_tokens.max(raw.cached_tokens),
            },
            _ => u,
        }
    };
    for key in ["usage", "token_usage", "tokens"] {
        if let Some(u) = result.get(key) {
            if u.is_object() {
                return Some(repair(parse_usage(u)));
            }
        }
    }
    if let Some(out) = result.get("output") {
        for key in ["usage", "token_usage", "tokens"] {
            if let Some(u) = out.get(key) {
                if u.is_object() {
                    return Some(repair(parse_usage(u)));
                }
            }
        }
    }
    Some(UsageDelta::default())
}

/// The provider's own usage block from `result.raw_response` — served as
/// an object or as a JSON-string body depending on the provider path
/// (both shapes live-verified). None when absent, `$slim`'d, or
/// unparseable — callers keep the normalized numbers.
fn raw_response_usage(result: &Value) -> Option<UsageDelta> {
    let raw = result.get("raw_response")?;
    let usage_owned: Value;
    let usage: &Value = match raw {
        Value::Object(_) => raw.get("usage")?,
        Value::String(body) => {
            let parsed: Value = serde_json::from_str(body).ok()?;
            usage_owned = parsed.get("usage")?.clone();
            &usage_owned
        }
        _ => return None,
    };
    if usage.is_object() {
        Some(parse_usage(usage))
    } else {
        None
    }
}

/// Generation time of a completed llm_call in MILLISECONDS
/// (`result.gen_time` — the abstractcore contract: "Generation time in
/// milliseconds", types.py). None when absent or non-positive.
pub fn gen_time_ms_from_record(rec: &Value) -> Option<f64> {
    if effect_type(rec) != "llm_call" || record_status(rec) != "completed" {
        return None;
    }
    let t = rec.get("result")?.get("gen_time")?.as_f64()?;
    if t > 0.0 {
        Some(t)
    } else {
        None
    }
}

/// A record's own `started_at` as epoch milliseconds (via
/// [`parse_rfc3339_utc`]). None when the record carries no parseable
/// timestamp — callers must treat that as "unknown", never "now".
pub fn started_at_epoch_ms(rec: &Value) -> Option<u64> {
    let raw = rec.get("started_at").and_then(Value::as_str)?;
    let st = parse_rfc3339_utc(raw)?;
    let dur = st.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as u64)
}

// ---------------------------------------------------------------------------
// LLM cycle content (completed `llm_call` records)
// ---------------------------------------------------------------------------

pub struct CycleResult {
    pub content: String,
    pub reasoning: String,
}

/// The model's actual output for a cycle lives on the COMPLETED record's
/// result — never the STARTED payload, whose `messages` are the conversation
/// INTO the model.
pub fn cycle_result_from_record(rec: &Value) -> Option<CycleResult> {
    if effect_type(rec) != "llm_call" || record_status(rec) != "completed" {
        return None;
    }
    let result = rec.get("result")?;
    let content = s(result, "content");
    let mut reasoning = s(result, "reasoning");
    if reasoning.is_empty() {
        if let Some(meta) = result.get("metadata") {
            reasoning = s(meta, "reasoning");
        }
    }
    if content.is_empty() && reasoning.is_empty() {
        return None;
    }
    Some(CycleResult { content, reasoning })
}

// ---------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------

/// Parse a gateway RFC3339 timestamp ("2026-07-22T11:00:53.600081+00:00",
/// "…Z", offset ±HH:MM) into a `SystemTime`. Sub-second precision is
/// dropped (elapsed displays are whole seconds). Returns None on any
/// malformed input — callers fall back to "now" honestly.
///
/// Hand-rolled on purpose: the crate's dependency budget has no chrono,
/// and the only consumer is run-elapsed back-dating on reattach.
pub fn parse_rfc3339_utc(s: &str) -> Option<std::time::SystemTime> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[13] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { s.get(r)?.parse::<i64>().ok() };
    let year = num(0..4)?;
    let month = num(5..7)?;
    let day = num(8..10)?;
    // 'T' or ' ' separator both appear in the wild.
    if !matches!(bytes[10], b'T' | b't' | b' ') {
        return None;
    }
    let hour = num(11..13)?;
    let minute = num(14..16)?;
    let second = num(17..19)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..61).contains(&second)
    {
        return None;
    }
    // Skip fractional seconds; then parse the offset.
    let mut idx = 19;
    if bytes.get(idx) == Some(&b'.') {
        idx += 1;
        while bytes.get(idx).map(u8::is_ascii_digit).unwrap_or(false) {
            idx += 1;
        }
    }
    let offset_secs: i64 = match bytes.get(idx) {
        Some(b'Z') | Some(b'z') => 0,
        Some(sign @ (b'+' | b'-')) => {
            let oh = num(idx + 1..idx + 3)?;
            // "+HH:MM" and "+HHMM" both occur.
            let om_start = if bytes.get(idx + 3) == Some(&b':') {
                idx + 4
            } else {
                idx + 3
            };
            let om = num(om_start..om_start + 2)?;
            let total = oh * 3600 + om * 60;
            if *sign == b'+' {
                total
            } else {
                -total
            }
        }
        None => 0, // naive timestamps read as UTC (gateway convention)
        _ => return None,
    };
    // Days-from-civil (Howard Hinnant): civil date -> days since epoch.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (month + 9) % 12; // Mar=0 … Feb=11
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146097 + doe - 719468;
    let unix = days * 86_400 + hour * 3600 + minute * 60 + second - offset_secs;
    if unix < 0 {
        return None;
    }
    Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(unix as u64))
}

/// Error text from a failed record.
pub fn error_from_record(rec: &Value) -> String {
    let direct = s(rec, "error");
    if !direct.is_empty() {
        return direct;
    }
    if let Some(result) = rec.get("result") {
        let nested = s(result, "error");
        if !nested.is_empty() {
            return nested;
        }
    }
    "step failed".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wait_extraction_canonical_location() {
        let rec = json!({
            "status": "waiting",
            "result": {"wait": {"reason": "user", "wait_key": "ask_1", "prompt": "Name?"}}
        });
        let w = extract_wait(&rec).unwrap();
        assert_eq!(wait_reason(w), "user");
        assert_eq!(wait_key(w), "ask_1");
        assert!(is_ask_user_wait(w));
        assert!(!is_tool_approval_wait(w));
    }

    #[test]
    fn wait_reason_accepts_enum_value_shape() {
        let w = json!({"reason": {"value": "subworkflow"}, "wait_key": "subworkflow:r2"});
        assert_eq!(wait_reason(&w), "subworkflow");
        assert_eq!(subworkflow_run_id(&w).unwrap(), "r2");
    }

    #[test]
    fn tool_approval_detection() {
        let by_mode = json!({"reason": "job", "details": {"mode": "approval_required", "tool_calls": [{"name": "write_file"}]}});
        assert!(is_tool_approval_wait(&by_mode));
        assert_eq!(tool_calls_from_wait(&by_mode).len(), 1);

        let by_key = json!({"reason": "event", "wait_key": "tool_approval:abc"});
        assert!(is_tool_approval_wait(&by_key));

        let by_executor =
            json!({"reason": "job", "details": {"executor": {"kind": "tool_approval"}}});
        assert!(is_tool_approval_wait(&by_executor));
    }

    #[test]
    fn subworkflow_id_from_details_wins() {
        let w = json!({"reason": "subworkflow", "wait_key": "subworkflow:key_form",
                       "details": {"sub_run_id": "details_form"}});
        assert_eq!(subworkflow_run_id(&w).unwrap(), "details_form");
    }

    #[test]
    fn ask_user_via_event_wait() {
        // Canonical key shape: evt:{scope}:{scope_id}:{name}.
        let w = json!({"reason": "event", "wait_key": "evt:run:abc-123:abstractcode.ask", "prompt": "Pick"});
        assert!(is_ask_user_wait(&w));
        let w2 = json!({"reason": "event", "wait_key": "evt:session:sid-9:abstract.ask", "prompt": "Pick"});
        assert!(is_ask_user_wait(&w2));
        // A status event wait is NOT an ask.
        let w3 = json!({"reason": "event", "wait_key": "evt:run:abc-123:abstract.status"});
        assert!(!is_ask_user_wait(&w3));
        assert_eq!(
            event_name_from_wait_key("evt:run:abc-123:abstract.status"),
            "abstract.status"
        );
    }

    #[test]
    fn tool_results_pair_by_call_id_then_order() {
        let rec = json!({
            "status": "completed",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"name": "read_file", "call_id": "c1", "arguments": {"path": "a.txt"}},
                {"name": "list_dir", "arguments": {"path": "."}}
            ]}},
            "result": {"results": [
                {"call_id": "c1", "success": true, "output": "hello"},
                {"success": false, "error": "denied"}
            ]}
        });
        let views = tool_results_from_record(&rec);
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].name, "read_file");
        assert_eq!(views[0].success, Some(true));
        assert_eq!(views[0].output.as_ref().unwrap(), &json!("hello"));
        assert_eq!(views[1].name, "list_dir");
        assert_eq!(views[1].error, "denied");
    }

    #[test]
    fn tool_results_survive_slimmed_payload() {
        // Real terminal-record shape (abstractruntime 0067-M ledger dedup):
        // oversized payload fields on COMPLETED records are replaced by a
        // `$slim` marker naming the STARTED record; the result rows are
        // self-describing and must produce views on their own.
        let rec = json!({
            "status": "completed",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": {
                "$slim": {"v": 1, "kind": "started_payload_field",
                           "step_id": "284e3750", "sha256": "a8c9…", "bytes": 5932,
                           "field": "tool_calls"}}}},
            "result": {"mode": "executed", "results": [
                {"call_id": "784882755", "name": "write_file", "success": true,
                 "output": "✅ Successfully written", "error": null}
            ]}
        });
        let views = tool_results_from_record(&rec);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].name, "write_file");
        assert_eq!(views[0].call_id, "784882755");
        assert_eq!(views[0].success, Some(true));
        assert!(views[0].error.is_empty(), "null error reads as empty");
        // Rows with neither name nor call id are uncorrelatable and skip.
        let junk = json!({
            "status": "completed",
            "effect": {"type": "tool_calls", "payload": {}},
            "result": {"results": [{"success": true}]}
        });
        assert!(tool_results_from_record(&junk).is_empty());
    }

    #[test]
    fn flow_output_string_and_object() {
        let rec1 = json!({"result": {"output": "done."}});
        assert_eq!(extract_flow_output(&rec1).unwrap().response, "done.");

        let rec2 = json!({"result": {"output": {"answer": "The answer", "success": true}}});
        let out = extract_flow_output(&rec2).unwrap();
        assert_eq!(out.response, "The answer");
        assert_eq!(out.meta.unwrap()["success"], json!(true));

        let rec3 = json!({"result": {"output": {"unrelated": 1}}});
        assert!(extract_flow_output(&rec3).is_none());
    }

    #[test]
    fn flow_output_offloaded_shapes() {
        // The runtime ledger offloader replaces oversized outputs with
        // {"$artifact": id} — whole-output (the live c61e4ac9 shape),
        // per-key leaf, or the resume {"result": …} wrapper. Inline text
        // always wins over a ref.
        let whole = json!({"result": {"completed": true,
            "output": {"$artifact": "e3b19ad9e42a2b725048bab40138f975"}}});
        let out = extract_flow_output(&whole).unwrap();
        assert!(out.response.is_empty());
        assert_eq!(
            out.offload_artifact.as_deref(),
            Some("e3b19ad9e42a2b725048bab40138f975")
        );

        let leaf = json!({"result": {"output": {
            "answer": {"$artifact": "leafart"}, "iterations": 3}}});
        assert_eq!(
            extract_flow_output(&leaf)
                .unwrap()
                .offload_artifact
                .as_deref(),
            Some("leafart")
        );

        let wrapped = json!({"result": {"output": {
            "success": true, "result": {"$artifact": "wrapart"}}}});
        assert_eq!(
            extract_flow_output(&wrapped)
                .unwrap()
                .offload_artifact
                .as_deref(),
            Some("wrapart")
        );

        // Inline text wins: no fetch when readable words exist.
        let both = json!({"result": {"output": {
            "answer": "inline words", "report": {"$artifact": "ignored"}}}});
        let out = extract_flow_output(&both).unwrap();
        assert_eq!(out.response, "inline words");
        assert!(out.offload_artifact.is_none());
    }

    #[test]
    fn flow_output_reads_the_result_wrapper_text() {
        // Resume/job completion records wrap the real output as
        // {"success": true, "result": …} (runtime.py completion writers).
        let s = json!({"result": {"output": {"success": true, "result": "plain words"}}});
        assert_eq!(extract_flow_output(&s).unwrap().response, "plain words");
        let o = json!({"result": {"output": {"success": true,
            "result": {"answer": "nested answer"}}}});
        assert_eq!(extract_flow_output(&o).unwrap().response, "nested answer");
    }

    #[test]
    fn flow_end_record_detection() {
        // Only the runtime's completion writers stamp result.completed.
        let done = json!({"status": "completed", "node_id": "done",
            "result": {"completed": true, "output": {"x": 1}}});
        assert!(is_flow_end_record(&done));
        let resume = json!({"status": "completed", "result": {"resumed": true}});
        assert!(!is_flow_end_record(&resume));
        let wait_until = json!({"status": "completed", "result": {"ready": true, "until": "t"}});
        assert!(!is_flow_end_record(&wait_until));
        let emit = json!({"status": "completed", "result": {"emitted": true}});
        assert!(!is_flow_end_record(&emit));
        let waiting = json!({"status": "waiting", "result": {"completed": true}});
        assert!(!is_flow_end_record(&waiting), "status gates the marker");
    }

    #[test]
    fn answer_text_from_artifact_shapes() {
        // The live artifact content shape: the serialized output object.
        let obj = br#"{"answer": "the real words", "report": "task: x", "iterations": 12}"#;
        assert_eq!(
            answer_text_from_artifact(obj, "application/json").as_deref(),
            Some("the real words")
        );
        // A bare JSON string leaf (kind=text offload of a string value
        // serialized as JSON) and plain text both read as the words.
        assert_eq!(
            answer_text_from_artifact(br#""quoted words""#, "application/json").as_deref(),
            Some("quoted words")
        );
        assert_eq!(
            answer_text_from_artifact(b"plain text answer", "text/plain").as_deref(),
            Some("plain text answer")
        );
        // An object with no readable text is honest None (label, never invent).
        assert!(answer_text_from_artifact(br#"{"blob": [1,2,3]}"#, "application/json").is_none());
        assert!(answer_text_from_artifact(b"   ", "text/plain").is_none());
    }

    #[test]
    fn flow_output_report_is_the_answer_fallback() {
        // coding-agent/coder end outputs carry {report, passed, …} and NO
        // conventional text key (live run b7d86e08…) — the report IS the
        // answer there. A conventional key still wins when present.
        let coder_end = json!({"result": {"output": {
            "report": "# Coding agent result\n\nStatus: DELIVERED",
            "passed": false, "delivered": true, "success": false,
            "rounds_used": 1, "open_failures": [], "artifacts": []}}});
        let out = extract_flow_output(&coder_end).unwrap();
        assert!(out.response.starts_with("# Coding agent result"));
        assert_eq!(out.meta.as_ref().unwrap()["report"], out.response);

        let with_answer = json!({"result": {"output": {"answer": "A", "report": "B"}}});
        assert_eq!(extract_flow_output(&with_answer).unwrap().response, "A");
    }

    #[test]
    fn rfc3339_parsing_variants() {
        use std::time::{Duration, UNIX_EPOCH};
        // The gateway's own created_at shape (epoch cross-checked with
        // python datetime: 2026-07-22T11:00:53Z = 1784718053).
        let t = parse_rfc3339_utc("2026-07-22T11:00:53.600081+00:00").unwrap();
        assert_eq!(
            t.duration_since(UNIX_EPOCH).unwrap(),
            Duration::from_secs(1_784_718_053)
        );
        // Z suffix, no fraction.
        assert_eq!(
            parse_rfc3339_utc("2026-07-22T11:00:53Z").unwrap(),
            UNIX_EPOCH + Duration::from_secs(1_784_718_053)
        );
        // A +02:00 offset lands 2h EARLIER in UTC.
        assert_eq!(
            parse_rfc3339_utc("2026-07-22T13:00:53+02:00").unwrap(),
            UNIX_EPOCH + Duration::from_secs(1_784_718_053)
        );
        // Epoch sanity + leap-year date.
        assert_eq!(
            parse_rfc3339_utc("1970-01-01T00:00:00Z").unwrap(),
            UNIX_EPOCH
        );
        assert_eq!(
            parse_rfc3339_utc("2024-02-29T00:00:00Z").unwrap(),
            UNIX_EPOCH + Duration::from_secs(1_709_164_800)
        );
        // Malformed inputs answer None, never panic.
        for bad in [
            "",
            "yesterday",
            "2026-13-01T00:00:00Z",
            "2026-07-22",
            "2026-07-22T25:00:00Z",
        ] {
            assert!(parse_rfc3339_utc(bad).is_none(), "{bad:?} must not parse");
        }
    }

    #[test]
    fn usage_parsing_variants() {
        let a = parse_usage(&json!({"input_tokens": 10, "output_tokens": 3}));
        assert_eq!(a.total_tokens, 13);
        let b =
            parse_usage(&json!({"prompt_tokens": 7, "completion_tokens": 2, "total_tokens": 9}));
        assert_eq!(b.input_tokens, 7);
        assert_eq!(b.total_tokens, 9);
        let rec = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"output": {"usage": {"prompt_tokens": 5, "completion_tokens": 5}}}
        });
        assert_eq!(usage_from_record(&rec).unwrap().total_tokens, 10);
    }

    #[test]
    fn cycle_result_reads_completed_result_only() {
        let rec = json!({
            "status": "completed", "node_id": "reason",
            "effect": {"type": "llm_call", "payload": {"messages": [{"role": "user", "content": "echo me"}]}},
            "result": {"content": "I will list files.", "reasoning": "user wants files"}
        });
        let c = cycle_result_from_record(&rec).unwrap();
        assert_eq!(c.content, "I will list files.");
        assert_eq!(c.reasoning, "user wants files");

        let started = json!({"status": "started", "effect": {"type": "llm_call"}});
        assert!(cycle_result_from_record(&started).is_none());
    }

    #[test]
    fn emit_event_and_status() {
        let rec = json!({
            "effect": {"type": "emit_event", "payload": {"name": "abstractcode.status", "payload": {"text": "Reading files"}}}
        });
        let ev = extract_emit_event(&rec).unwrap();
        assert_eq!(ev.name, "abstract.status");
        assert_eq!(
            status_text_from_payload(ev.payload.as_ref()),
            "Reading files"
        );
    }

    #[test]
    fn error_extraction_fallbacks() {
        assert_eq!(error_from_record(&json!({"error": "boom"})), "boom");
        assert_eq!(
            error_from_record(&json!({"result": {"error": "nested"}})),
            "nested"
        );
        assert_eq!(error_from_record(&json!({})), "step failed");
    }

    #[test]
    fn splitless_usage_repairs_from_raw_response_object_and_string() {
        // Live ledger shape (coder run 0312b41d…, gpt-5.6-sol via proxy):
        // the NORMALIZED usage block is splitless while the SAME record's
        // raw_response.usage carries the provider's real split.
        let rec_obj = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {
                "usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 3180,
                           "prompt_tokens": 0, "completion_tokens": 0},
                "raw_response": {"usage": {
                    "input_tokens": 2904,
                    "input_tokens_details": {"cache_write_tokens": 0, "cached_tokens": 0},
                    "output_tokens": 276,
                    "output_tokens_details": {"reasoning_tokens": 69},
                    "total_tokens": 3180}}}});
        let u = usage_from_record(&rec_obj).unwrap();
        assert_eq!(u.input_tokens, 2904);
        assert_eq!(u.output_tokens, 276);
        assert_eq!(u.total_tokens, 3180);

        // raw_response can arrive as a JSON STRING body (both shapes are
        // live-verified on this gateway).
        let rec_str = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {
                "usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 100},
                "raw_response":
                    "{\"usage\": {\"prompt_tokens\": 60, \"completion_tokens\": 40}}"}});
        let u = usage_from_record(&rec_str).unwrap();
        assert_eq!(u.input_tokens, 60);
        assert_eq!(u.output_tokens, 40);
        assert_eq!(u.total_tokens, 100, "normalized total is kept when present");

        // Absent / $slim'd / unparseable raw stays splitless-honest.
        let rec_slim = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {
                "usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 500},
                "raw_response": {"$slim": {"v": 1}}}});
        let u = usage_from_record(&rec_slim).unwrap();
        assert_eq!(
            (u.input_tokens, u.output_tokens, u.total_tokens),
            (0, 0, 500)
        );

        // A NORMAL split is never touched by the raw block (repair is
        // splitless-only — the normalized numbers stay authoritative).
        let rec_split = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {
                "usage": {"input_tokens": 10, "output_tokens": 5},
                "raw_response": {"usage": {"input_tokens": 999, "output_tokens": 999}}}});
        let u = usage_from_record(&rec_split).unwrap();
        assert_eq!((u.input_tokens, u.output_tokens), (10, 5));
    }

    #[test]
    fn nested_cached_tokens_details_are_read() {
        // Responses-API spelling: input_tokens_details.cached_tokens.
        let u = parse_usage(&json!({
            "input_tokens": 7975, "output_tokens": 307,
            "input_tokens_details": {"cache_write_tokens": 0, "cached_tokens": 2560}}));
        assert_eq!(u.cached_tokens, 2560);
        // Chat-Completions spelling: prompt_tokens_details.cached_tokens.
        let u = parse_usage(&json!({
            "prompt_tokens": 100, "completion_tokens": 10,
            "prompt_tokens_details": {"cached_tokens": 64}}));
        assert_eq!(u.cached_tokens, 64);
        // The flat normalized key still wins when present.
        let u = parse_usage(&json!({
            "input_tokens": 10, "cached_input_tokens": 4,
            "input_tokens_details": {"cached_tokens": 99}}));
        assert_eq!(u.cached_tokens, 4);
    }

    #[test]
    fn gen_time_and_started_at_extraction() {
        // gen_time is MILLISECONDS (abstractcore types.py contract).
        let rec = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"gen_time": 7203.6}});
        assert_eq!(gen_time_ms_from_record(&rec), Some(7203.6));
        // Non-positive / absent / non-llm answer None.
        let zero = json!({"status": "completed",
            "effect": {"type": "llm_call", "payload": {}}, "result": {"gen_time": 0.0}});
        assert_eq!(gen_time_ms_from_record(&zero), None);
        let tool = json!({"status": "completed",
            "effect": {"type": "tool_calls", "payload": {}}, "result": {"gen_time": 5.0}});
        assert_eq!(gen_time_ms_from_record(&tool), None);

        // started_at → epoch ms (whole-second precision by the parser).
        let rec = json!({"started_at": "2026-07-22T11:00:53.600081+00:00"});
        assert_eq!(started_at_epoch_ms(&rec), Some(1_784_718_053_000));
        assert_eq!(started_at_epoch_ms(&json!({})), None);
        assert_eq!(started_at_epoch_ms(&json!({"started_at": "junk"})), None);
    }

    #[test]
    fn splitless_repair_edge_shapes_stay_honest() {
        // Cycle-2 review, attack surface 4 — the repair must never invent
        // and never overwrite. (a) A MALFORMED raw_response JSON string
        // keeps the normalized numbers (no panic, no partial parse).
        let malformed = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {
                "usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 500},
                "raw_response": "{\"usage\": {\"input_tokens\": 60, "}});
        let u = usage_from_record(&malformed).unwrap();
        assert_eq!(
            (u.input_tokens, u.output_tokens, u.total_tokens),
            (0, 0, 500),
            "unparseable raw stays splitless-honest"
        );
        // (b) A raw block that is ITSELF splitless (total only) repairs
        // nothing — a raw total is not a split.
        let raw_total_only = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {
                "usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 500},
                "raw_response": {"usage": {"total_tokens": 480}}}});
        let u = usage_from_record(&raw_total_only).unwrap();
        assert_eq!(
            (u.input_tokens, u.output_tokens, u.total_tokens),
            (0, 0, 500),
            "the normalized total is never displaced by a splitless raw"
        );
        // (c) A PARTIAL normalized split (input > 0, output == 0 — a
        // legitimately empty response) is a REAL split: a disagreeing raw
        // block must not overwrite it (the repair is 0/0-only).
        let partial_split = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {
                "usage": {"input_tokens": 500, "output_tokens": 0},
                "raw_response": {"usage": {"input_tokens": 9999, "output_tokens": 777}}}});
        let u = usage_from_record(&partial_split).unwrap();
        assert_eq!(
            (u.input_tokens, u.output_tokens),
            (500, 0),
            "a real (partial) split is never overwritten by raw numbers"
        );
        // (d) The mirror partial (output > 0, input == 0) holds too.
        let out_only = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {
                "usage": {"input_tokens": 0, "output_tokens": 42},
                "raw_response": {"usage": {"input_tokens": 9999, "output_tokens": 777}}}});
        let u = usage_from_record(&out_only).unwrap();
        assert_eq!((u.input_tokens, u.output_tokens), (0, 42));
        // (e) A raw_response that parses to a NON-object answers None
        // inside the repair path and the normalized numbers stand.
        let raw_scalar = json!({
            "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {
                "usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 9},
                "raw_response": "\"just a string\""}});
        let u = usage_from_record(&raw_scalar).unwrap();
        assert_eq!((u.input_tokens, u.output_tokens, u.total_tokens), (0, 0, 9));
    }

    #[test]
    fn subworkflow_spawn_reads_the_parents_declaration() {
        // The REAL live shape (gateway run 76fc3fcb…, node-2 waiting
        // record): details carry sub_run_id + sub_workflow_id.
        let live = json!({
            "run_id": "root", "node_id": "node-2", "status": "waiting",
            "effect": {"type": "start_subworkflow", "payload": {
                "workflow_id": "visual_react_agent_basic-agent_0_0_3_81795ea9_node-2",
                "async": true, "wait": true}},
            "result": {"wait": {"reason": "subworkflow",
                "wait_key": "subworkflow:9c5cad22",
                "details": {"sub_run_id": "9c5cad22",
                    "sub_workflow_id": "visual_react_agent_basic-agent_0_0_3_81795ea9_node-2",
                    "async": true}}}});
        let spawn = subworkflow_spawn(&live).unwrap();
        assert_eq!(spawn.sub_run_id, "9c5cad22");
        assert_eq!(
            spawn.workflow_id,
            "visual_react_agent_basic-agent_0_0_3_81795ea9_node-2"
        );
        assert!(!spawn.wrap_as_tool_result);

        // Details lacking sub_workflow_id: the effect payload's REQUIRED
        // workflow_id is the belt.
        let payload_belt = json!({
            "run_id": "root", "status": "waiting",
            "effect": {"type": "start_subworkflow",
                        "payload": {"workflow_id": "bundle@1.0.0:flow9"}},
            "result": {"wait": {"reason": "subworkflow",
                "wait_key": "subworkflow:sub1",
                "details": {"sub_run_id": "sub1"}}}});
        let spawn = subworkflow_spawn(&payload_belt).unwrap();
        assert_eq!(spawn.workflow_id, "bundle@1.0.0:flow9");

        // TOOL MODE (delegate_agent shape): wrap_as_tool_result rides the
        // details (runtime stamps it) — and the payload as the belt.
        let tool_mode = json!({
            "run_id": "agent", "status": "waiting",
            "effect": {"type": "start_subworkflow", "payload": {
                "workflow_id": "visual_react_agent_x_node-1",
                "wrap_as_tool_result": true, "tool_name": "delegate_agent"}},
            "result": {"wait": {"reason": "subworkflow",
                "wait_key": "subworkflow:d1",
                "details": {"sub_run_id": "d1",
                    "sub_workflow_id": "visual_react_agent_x_node-1",
                    "wrap_as_tool_result": true}}}});
        assert!(subworkflow_spawn(&tool_mode).unwrap().wrap_as_tool_result);

        // Pre-contract record (no declaration anywhere): empty workflow
        // id, not tool-wrapped — the fold's cycle #FALLBACK covers it.
        let bare = json!({
            "run_id": "root", "status": "waiting",
            "result": {"wait": {"reason": "subworkflow",
                "wait_key": "subworkflow:old1",
                "details": {"sub_run_id": "old1"}}}});
        let spawn = subworkflow_spawn(&bare).unwrap();
        assert!(spawn.workflow_id.is_empty());
        assert!(!spawn.wrap_as_tool_result);

        // A non-subworkflow wait is None.
        let ask = json!({
            "run_id": "root", "status": "waiting",
            "result": {"wait": {"reason": "user", "wait_key": "ask1"}}});
        assert!(subworkflow_spawn(&ask).is_none());
    }

    #[test]
    fn run_output_record_classification() {
        // The runtime's terminal marker is authoritative.
        let terminal = json!({"status": "completed",
            "result": {"completed": true, "output": {"answer": "x"}}});
        assert!(is_run_output_record(&terminal));
        // Marker without an output field is still the run's own end
        // (the no-readable-answer conclusion path needs it).
        let bare_end = json!({"status": "completed", "result": {"completed": true}});
        assert!(is_run_output_record(&bare_end));
        // #FALLBACK: pre-marker terminal shape (distilled captures /
        // older ledgers) — completed + output, no self-identification
        // as an effect result.
        let legacy = json!({"status": "completed", "node_id": "end",
            "result": {"output": {"answer": "y"}}});
        assert!(is_run_output_record(&legacy));
        // A SYNC start_subworkflow completion carries the CHILD's output
        // ({"sub_run_id", "output"} — runtime.py) and must NOT read as
        // the parent's own answer.
        let sync_spawn = json!({"status": "completed",
            "effect": {"type": "start_subworkflow", "payload": {"workflow_id": "w"}},
            "result": {"sub_run_id": "child1", "output": {"answer": "child words"}}});
        assert!(!is_run_output_record(&sync_spawn));
        // Non-terminal results (resume/wait_until/emit) and non-completed
        // statuses never qualify.
        assert!(!is_run_output_record(
            &json!({"status": "completed", "result": {"resumed": true}})
        ));
        assert!(!is_run_output_record(
            &json!({"status": "waiting", "result": {"completed": true}})
        ));
    }
}
