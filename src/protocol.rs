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

/// Tool calls embedded in a wait (`details.tool_calls`, with the
/// evidence-list fallback used by the reference clients).
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

/// Sub-run id for a `subworkflow` wait: `details.sub_run_id`, else the
/// `subworkflow:<id>` wait-key form.
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
/// `evt:{scope}:{scope_id}:{name}` (the name may itself contain dots and
/// colons never appear in names) — the NAME is everything after the third
/// colon. Live example: `evt:run:<run_id>:abstract.status`.
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
}

fn pick_textish(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(x)) => x.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
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
        });
    }
    if !out0.is_object() {
        return None;
    }
    let msg = ["answer", "response", "message", "text", "content"]
        .iter()
        .map(|k| pick_textish(out0.get(*k)))
        .find(|v| !v.is_empty())
        .unwrap_or_default();
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
    if msg.is_empty() && !has_meta {
        return None;
    }
    Some(FlowOutput {
        response: msg,
        meta: if has_meta { Some(meta) } else { None },
    })
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
pub fn usage_from_record(rec: &Value) -> Option<UsageDelta> {
    if effect_type(rec) != "llm_call" || record_status(rec) != "completed" {
        return None;
    }
    let result = rec.get("result")?;
    for key in ["usage", "token_usage", "tokens"] {
        if let Some(u) = result.get(key) {
            if u.is_object() {
                return Some(parse_usage(u));
            }
        }
    }
    if let Some(out) = result.get("output") {
        for key in ["usage", "token_usage", "tokens"] {
            if let Some(u) = out.get(key) {
                if u.is_object() {
                    return Some(parse_usage(u));
                }
            }
        }
    }
    Some(UsageDelta::default())
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
}
