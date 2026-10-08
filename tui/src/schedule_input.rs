//! `/schedule`'s workflow inputs (R17.2): the Code web's `buildInput`, in the
//! terminal. Pure: no HTTP, no signals.
//!
//! The web builds a new automation's `target.input_data` in three moves
//! (abstractcode web `app.tsx` `NewAutomationDialog buildInput` +
//! `currentWorkflowInput`, `catalog.ts` `buildWorkflowInput`,
//! `input_schema.ts`):
//!
//! 1. the chosen workflow's input schema (`GET /bundles/{b}/flows/{f}/input_schema`,
//!    normalised like [`normalize_input_schema`]; a v1 descriptor with
//!    required pins is reconciled with its VisualFlow, [`reconcile_visualflow_schema`])
//!    gives the DEFAULTS ([`schema_defaults`] — the served `tools` default
//!    included);
//! 2. the conversation's run settings ([`Conversation`]: provider/model,
//!    reasoning, speculation, stream, iteration and token limits, its tool
//!    choice, its skills) are applied the way an agent turn applies them
//!    ([`automation_input`]), minus what an automation owns itself
//!    (workspace, consent policy);
//! 3. the result is checked against the schema ([`validate_workflow_inputs`],
//!    the web's sentences) before anything is sent.
//!
//! The dialog's tool selection then rides `tools` + `_runtime.allowed_tools`
//! ([`with_automation_tools`], the kit's `withAutomationTools`), and the
//! Workspaces section's value rides `workspace`.

use serde_json::{json, Map, Value};

/// The conversation's run settings an automation inherits (the web's
/// `preferences` + `toolPermissions` + `preferences.skills`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Conversation {
    pub provider: String,
    pub model: String,
    /// `_runtime.thinking` ("" = absent).
    pub reasoning: String,
    pub speculation: Option<Value>,
    /// `_runtime.stream` (None = the gateway decides).
    pub stream: Option<bool>,
    /// `_limits.max_iterations` + the flat `max_iterations` (0 = absent).
    pub max_iterations: u64,
    /// `_limits.max_tokens` (0 = absent).
    pub max_tokens: u64,
    /// The conversation's tool choice: `None` = untouched (the workflow's
    /// own tools), `Some(list)` = the exact list (`/tools` customised).
    pub tools: Option<Vec<String>>,
    pub skills: Vec<String>,
}

fn record(v: Option<&Value>) -> Map<String, Value> {
    v.and_then(Value::as_object).cloned().unwrap_or_default()
}

/// The web's `text()`: a trimmed non-empty string, else `None`.
fn text(v: &str) -> Option<String> {
    let t = v.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn clean_strings(list: &[String]) -> Vec<String> {
    list.iter().filter_map(|s| text(s)).collect()
}

// ---------------------------------------------------------------------------
// Schema (input_schema.ts)
// ---------------------------------------------------------------------------

/// The web's `normalizeInputSchema`: the gateway's `input_data_schema`
/// (or `input_schema` / `schema` / the value itself) with each declared pin
/// (`inputs[]`: schema, type, label, default) and the `defaults` map folded
/// into `properties`. `None` for a null answer.
pub fn normalize_input_schema(value: &Value) -> Option<Value> {
    if value.is_null() {
        return None;
    }
    let raw = record(
        value
            .get("input_data_schema")
            .or_else(|| value.get("input_schema"))
            .or_else(|| value.get("schema"))
            .filter(|v| !v.is_null())
            .or(Some(value)),
    );
    let mut properties: Map<String, Value> = record(raw.get("properties"))
        .into_iter()
        .map(|(k, field)| (k, Value::Object(record(Some(&field)))))
        .collect();
    for pin in value
        .get("inputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = pin
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let mut field = record(pin.get("schema"));
        field.extend(record(properties.get(id)));
        if let Some(t) = pin.get("type").filter(|t| truthy(t)) {
            field.insert("x-abstract-type".into(), t.clone());
        }
        if let Some(l) = pin.get("label").filter(|l| truthy(l)) {
            field.insert("title".into(), l.clone());
        }
        if let Some(d) = pin.as_object().and_then(|o| o.get("default")) {
            field.insert("default".into(), d.clone());
        }
        properties.insert(id.to_string(), Value::Object(field));
    }
    for (key, default) in record(value.get("defaults")) {
        let mut field = record(properties.get(&key));
        field.insert("default".into(), default);
        properties.insert(key, Value::Object(field));
    }
    let mut out = raw;
    out.insert("properties".into(), Value::Object(properties));
    Some(Value::Object(out))
}

/// JS truthiness for the values a descriptor carries.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true,
    }
}

/// The web's `fetchWorkflowSchema` rule: an older v1 descriptor (not a
/// native loop) whose `required` list is non-empty inferred `required =
/// !hasDefault`, so its VisualFlow must be read to tell an author
/// requirement from an optional runtime setting.
pub fn needs_visualflow(raw: &Value, schema: &Value) -> bool {
    !(raw.get("native_loop_factory").is_some_and(truthy)
        || raw.get("version") != Some(&json!(1))
        || !raw.get("inputs").is_some_and(Value::is_array)
        || !raw.get("input_data_schema").is_some_and(truthy)
        || !schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|r| !r.is_empty()))
}

/// The web's `reconcileVisualFlowSchema`: required = the start node's pins
/// flagged `required: true` (plus required names that are not pins); pin
/// schema, type, label and `pinDefaults` folded into `properties`.
pub fn reconcile_visualflow_schema(schema: &Value, flow: &Value) -> Result<Value, String> {
    let Some(nodes) = flow.get("nodes").and_then(Value::as_array) else {
        return Err("The workflow input declarations could not be verified. Please retry.".into());
    };
    let start = nodes.iter().find(|n| {
        n.pointer("/data/nodeType")
            .filter(|v| truthy(v))
            .or_else(|| n.get("type"))
            .and_then(Value::as_str)
            == Some("on_flow_start")
    });
    let Some(outputs) = start
        .and_then(|s| s.pointer("/data/outputs"))
        .and_then(Value::as_array)
    else {
        return Err("The workflow start inputs could not be verified. Please retry.".into());
    };
    let pins: Vec<&Value> = outputs
        .iter()
        .filter(|p| {
            let id = p.get("id").and_then(Value::as_str).unwrap_or("");
            !id.is_empty()
                && p.get("type").and_then(Value::as_str) != Some("execution")
                && id != "exec"
                && id != "exec-out"
        })
        .collect();
    let pin_ids: Vec<&str> = pins
        .iter()
        .filter_map(|p| p.get("id").and_then(Value::as_str))
        .collect();
    let mut required: Vec<String> = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|id| !pin_ids.contains(id))
        .map(str::to_string)
        .collect();
    let mut properties = record(schema.get("properties"));
    let defaults = record(start.and_then(|s| s.pointer("/data/pinDefaults")));
    for pin in pins {
        let id = pin
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if pin.get("required") == Some(&json!(true)) && !required.contains(&id) {
            required.push(id.clone());
        }
        let mut field = record(properties.get(&id));
        field.extend(record(pin.get("schema")));
        if let Some(t) = pin.get("type").filter(|t| truthy(t)) {
            field.insert("x-abstract-type".into(), t.clone());
        }
        if let Some(l) = pin.get("label").filter(|l| truthy(l)) {
            field.insert("title".into(), l.clone());
        }
        if let Some(d) = defaults.get(&id) {
            field.insert("default".into(), d.clone());
        }
        properties.insert(id, Value::Object(field));
    }
    let mut out = record(Some(schema));
    out.insert("properties".into(), Value::Object(properties));
    out.insert("required".into(), json!(required));
    Ok(Value::Object(out))
}

/// The web's `schemaDefaults`: every property's `default`.
pub fn schema_defaults(schema: Option<&Value>) -> Map<String, Value> {
    record(schema.and_then(|s| s.get("properties")))
        .into_iter()
        .filter_map(|(k, field)| field.get("default").cloned().map(|d| (k, d)))
        .collect()
}

/// The web's `validateWorkflowInputs`, sentence for sentence.
pub fn validate_workflow_inputs(
    schema: Option<&Value>,
    supplied: &Map<String, Value>,
) -> Vec<String> {
    let Some(schema) = schema else {
        return Vec::new();
    };
    let mut values = schema_defaults(Some(schema));
    values.extend(supplied.clone());
    let props = record(schema.get("properties"));
    let title_of = |name: &str| -> String {
        props
            .get(name)
            .and_then(|f| f.get("title"))
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .unwrap_or(name)
            .to_string()
    };
    let mut errors = Vec::new();
    for name in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !values.contains_key(name) {
            errors.push(format!("{} is required.", title_of(name)));
        }
    }
    for (name, value) in &values {
        let Some(field) = props.get(name) else {
            continue;
        };
        let title = title_of(name);
        if let Some(choices) = field.get("enum").and_then(Value::as_array) {
            if !choices.iter().any(|c| c == value) {
                errors.push(format!("{title} must be one of the available choices."));
            }
        }
        let types: Vec<String> = match field.get("type") {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
            Some(Value::String(t)) if !t.is_empty() => vec![t.clone()],
            _ => Vec::new(),
        };
        if value.is_null() && (types.iter().any(|t| t == "null") || types.is_empty()) {
            continue;
        }
        let actual = match value {
            Value::Array(_) => "array",
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Object(_) => "object",
        };
        let ty: String = if types.iter().any(|t| t == actual) {
            actual.to_string()
        } else if actual == "number" && types.iter().any(|t| t == "integer") {
            "integer".to_string()
        } else {
            types.first().cloned().unwrap_or_default()
        };
        let num = value.as_f64();
        if ty == "number" || ty == "integer" {
            let bad = match num {
                None => true,
                Some(f) => !f.is_finite() || (ty == "integer" && f.fract() != 0.0),
            };
            if bad {
                errors.push(format!(
                    "{title} must be {}.",
                    if ty == "integer" {
                        "a whole number"
                    } else {
                        "a number"
                    }
                ));
            }
        }
        if let Some(n) = num {
            if let Some(min) = field.get("minimum").and_then(Value::as_f64) {
                if n < min {
                    errors.push(format!(
                        "{title} must be at least {}.",
                        js_number(&field["minimum"])
                    ));
                }
            }
            if let Some(max) = field.get("maximum").and_then(Value::as_f64) {
                if n > max {
                    errors.push(format!(
                        "{title} must be at most {}.",
                        js_number(&field["maximum"])
                    ));
                }
            }
        }
        if ty == "string" && !value.is_string() {
            errors.push(format!("{title} must be text."));
        }
        if let (Some(s), Some(min)) = (
            value.as_str(),
            field.get("minLength").and_then(Value::as_u64),
        ) {
            if (s.encode_utf16().count() as u64) < min {
                errors.push(format!("{title} must contain at least {min} characters."));
            }
        }
        if ty == "boolean" && !value.is_boolean() {
            errors.push(format!("{title} must be true or false."));
        }
        if ty == "array" && !value.is_array() {
            errors.push(format!("{title} must be a JSON array."));
        }
        if ty == "object" && !value.is_object() {
            errors.push(format!("{title} must be a JSON object."));
        }
    }
    errors
}

/// A JSON number as JavaScript prints it (`5`, not `5.0`).
fn js_number(v: &Value) -> String {
    match v.as_f64() {
        Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
        Some(f) => format!("{f}"),
        None => v.to_string(),
    }
}

// ---------------------------------------------------------------------------
// The input (catalog.ts buildWorkflowInput, agent interface + app.tsx
// currentWorkflowInput(…, forAutomation = true))
// ---------------------------------------------------------------------------

/// The provider/model rule every client shares.
pub const PROVIDER_MODEL_PAIR: &str =
    "Choose both a provider and a model, or use the workflow / Gateway defaults.";

/// The new automation's `input_data` before the dialog's tool selection and
/// workspaces: the schema defaults, the task, and the conversation's run
/// settings. The agent interface's conventions apply (every workflow the
/// terminal can schedule is an `abstractcode.agent.v1` agent). What an
/// automation owns itself is never inherited: the workspace fields and the
/// conversation's consent policy (`_runtime.tool_policy`).
pub fn automation_input(
    prompt: &str,
    defaults: &Map<String, Value>,
    conv: &Conversation,
) -> Result<Value, String> {
    let mut output = defaults.clone();
    // promptProperty = "prompt" for agents; the agent contract then sets
    // `prompt` whatever its text.
    output.insert("prompt".into(), json!(prompt));
    let mut context = record(output.get("context"));
    context.insert("task".into(), json!(prompt));
    context.remove("messages");
    context.remove("media");
    context.remove("attachments");
    output.insert("use_context".into(), json!(false));
    output.remove("attachments");
    output.remove("media");
    output.insert("context".into(), Value::Object(context));
    output.insert("use_session_history".into(), json!(true));

    // assignCommonRuntimeInputs(output, options, mayOverwrite = true)
    let provider = text(&conv.provider);
    let model = text(&conv.model);
    if provider.is_some() != model.is_some() {
        return Err(PROVIDER_MODEL_PAIR.into());
    }
    if let (Some(p), Some(m)) = (&provider, &model) {
        output.insert("provider".into(), json!(p));
        output.insert("model".into(), json!(m));
    }
    if let Some(tools) = &conv.tools {
        output.insert("tools".into(), json!(clean_strings(tools)));
    }
    let skills = clean_strings(&conv.skills);
    if !skills.is_empty() {
        output.insert("skills".into(), json!(skills));
    }
    let mut runtime = record(output.get("_runtime"));
    if let (Some(p), Some(m)) = (&provider, &model) {
        runtime.insert("provider".into(), json!(p));
        runtime.insert("model".into(), json!(m));
    }
    if let Some(r) = text(&conv.reasoning) {
        runtime.insert("thinking".into(), json!(r));
    }
    if let Some(s) = &conv.speculation {
        runtime.insert("speculation".into(), s.clone());
    }
    if let Some(stream) = conv.stream {
        runtime.insert("stream".into(), json!(stream));
    }
    if let Some(tools) = &conv.tools {
        let requested = clean_strings(tools);
        let allowed = match runtime.get("allowed_tools").and_then(Value::as_array) {
            Some(authored) => requested
                .into_iter()
                .filter(|n| authored.iter().any(|a| a.as_str() == Some(n)))
                .collect(),
            None => requested,
        };
        runtime.insert("allowed_tools".into(), json!(allowed));
    }
    let had_runtime = !runtime.is_empty();
    if had_runtime {
        output.insert("_runtime".into(), Value::Object(runtime));
    }
    let mut limits = record(output.get("_limits"));
    if conv.max_iterations >= 1 {
        limits.insert("max_iterations".into(), json!(conv.max_iterations));
    }
    if conv.max_tokens >= 1 {
        limits.insert("max_tokens".into(), json!(conv.max_tokens));
    }
    if !limits.is_empty() {
        output.insert("_limits".into(), Value::Object(limits));
    }
    if conv.max_iterations >= 1 {
        output.insert("max_iterations".into(), json!(conv.max_iterations));
    }
    // forAutomation: the automation owns its workspace and consent policy.
    for key in [
        "workspace_root",
        "workspace_access_mode",
        "workspace_allowed_paths",
        "workspace",
    ] {
        output.remove(key);
    }
    if let Some(Value::Object(rt)) = output.get_mut("_runtime") {
        rt.remove("tool_policy");
    }
    Ok(Value::Object(output))
}

/// The kit's `automationToolSelection`: `tools` (or the `_runtime`
/// ceiling), filtered by the ceiling; `None` = the workflow's own tools.
pub fn automation_tool_selection(input: &Value) -> Option<Vec<String>> {
    let ceiling = input
        .pointer("/_runtime/allowed_tools")
        .and_then(Value::as_array)
        .cloned();
    let tools = input
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| ceiling.clone())?;
    Some(
        tools
            .iter()
            .filter_map(Value::as_str)
            .filter(|t| {
                ceiling
                    .as_ref()
                    .is_none_or(|c| c.iter().any(|x| x.as_str() == Some(t)))
            })
            .map(str::to_string)
            .collect(),
    )
}

/// The kit's `withAutomationTools`: a selection is `tools` AND the
/// `_runtime.allowed_tools` ceiling; `None` removes both (and an emptied
/// `_runtime`).
pub fn with_automation_tools(input: &Value, tools: Option<&[String]>) -> Value {
    let mut next = record(Some(input));
    let mut runtime = record(input.get("_runtime"));
    match tools {
        None => {
            next.remove("tools");
            runtime.remove("allowed_tools");
        }
        Some(list) => {
            next.insert("tools".into(), json!(list));
            runtime.insert("allowed_tools".into(), json!(list));
        }
    }
    if runtime.is_empty() {
        next.remove("_runtime");
    } else {
        next.insert("_runtime".into(), Value::Object(runtime));
    }
    Value::Object(next)
}

/// The dialog's first tool selection (the web's `initialTools`): the
/// conversation's own list when it customised its tools, else the
/// workflow's served default (`schemaDefaults(schema).tools`), else `None`
/// ("Use workflow default tools").
pub fn initial_tools(defaults: &Map<String, Value>, conv: &Conversation) -> Option<Vec<String>> {
    let mut inputs = Value::Object(defaults.clone());
    if let Some(list) = &conv.tools {
        inputs["tools"] = json!(list);
    }
    automation_tool_selection(&inputs)
}

/// Everything after the dialog: the built input, the dialog's tool
/// selection (`with_automation_tools`), then the Workspaces section's value
/// (the web's `withAutomationWorkspace`).
pub fn finish_input(
    built: &Value,
    tools: Option<&[String]>,
    workspace: Option<&crate::workspaces::RunValue>,
) -> Value {
    let tooled = with_automation_tools(built, tools);
    let mut out = record(Some(&tooled));
    match workspace {
        None => {
            out.remove("workspace");
        }
        Some(ws) => {
            out.remove("workspace_allowed_paths");
            out.remove("workspace_access_mode");
            out.insert("workspace".into(), ws.to_json());
        }
    }
    Value::Object(out)
}

/// The `input_schema` route of a workflow (the web's `fetchWorkflowSchema`
/// path for the private registry).
pub fn input_schema_path(bundle_id: &str, flow_id: &str, version: &str) -> String {
    let enc = crate::gateway::url_encode;
    let mut p = format!(
        "/api/gateway/bundles/{}/flows/{}/input_schema",
        enc(bundle_id),
        enc(flow_id)
    );
    if !version.trim().is_empty() {
        p.push_str(&format!("?bundle_version={}", enc(version.trim())));
    }
    p
}

/// The VisualFlow source route (`?bundle_version=` always: the web reads
/// the version it validated).
pub fn flow_source_path(bundle_id: &str, flow_id: &str, version: &str) -> String {
    let enc = crate::gateway::url_encode;
    format!(
        "/api/gateway/bundles/{}/flows/{}?bundle_version={}",
        enc(bundle_id),
        enc(flow_id),
        enc(version)
    )
}

/// The web's `assertSelection`: the answer names the workflow asked for.
pub fn assert_selection(
    answer: &Value,
    bundle_id: &str,
    version: &str,
    flow_id: &str,
) -> Result<(), String> {
    for (key, expected) in [
        ("bundle_id", bundle_id),
        ("bundle_version", version),
        ("flow_id", flow_id),
    ] {
        if answer.get(key).and_then(Value::as_str) != Some(expected) {
            return Err(
                "The Gateway returned inputs for a different workflow version. Refresh workflows and retry."
                    .into(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC: &str = include_str!("../tests/fixtures/schedule/input_schema_basic_agent.json");
    const REACT: &str = include_str!("../tests/fixtures/schedule/input_schema_react_agent.json");

    fn v(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn defaults_come_from_the_served_schema() {
        let raw = v(BASIC);
        let schema = normalize_input_schema(&raw).unwrap();
        let d = schema_defaults(Some(&schema));
        assert_eq!(d["use_context"], json!(false));
        assert_eq!(d["max_iterations"], json!(20));
        assert!(d["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "read_file"));
        assert!(!needs_visualflow(&raw, &schema), "no required pins");
    }

    #[test]
    fn native_loop_never_reads_the_visualflow_and_requires_the_prompt() {
        let raw = v(REACT);
        let schema = normalize_input_schema(&raw).unwrap();
        assert!(!needs_visualflow(&raw, &schema));
        let none = Map::new();
        assert_eq!(
            validate_workflow_inputs(Some(&schema), &none),
            vec!["Prompt is required.".to_string()]
        );
        let mut ok = Map::new();
        ok.insert("prompt".into(), json!("hi"));
        assert!(validate_workflow_inputs(Some(&schema), &ok).is_empty());
    }

    #[test]
    fn validation_sentences_are_the_webs() {
        let schema = json!({"properties": {
            "n": {"type": "integer", "title": "Rounds", "minimum": 1, "maximum": 5},
            "e": {"type": "string", "enum": ["a", "b"]},
            "b": {"type": "boolean"},
            "s": {"type": "string", "minLength": 3},
            "o": {"type": "object"},
            "l": {"type": "array"}
        }, "required": ["must"]});
        let supplied: Map<String, Value> = serde_json::from_value(json!({
            "n": 7.5, "e": "c", "b": "yes", "s": "ab", "o": [], "l": {}
        }))
        .unwrap();
        let errors = validate_workflow_inputs(Some(&schema), &supplied);
        for want in [
            "must is required.",
            "Rounds must be a whole number.",
            "Rounds must be at most 5.",
            "e must be one of the available choices.",
            "b must be true or false.",
            "s must contain at least 3 characters.",
            "o must be a JSON object.",
            "l must be a JSON array.",
        ] {
            assert!(
                errors.iter().any(|e| e == want),
                "missing {want:?} in {errors:?}"
            );
        }
    }

    #[test]
    fn reconcile_reads_required_from_the_start_pins() {
        let schema = json!({"properties": {"a": {"type": "string"}, "b": {"type": "string"}}, "required": ["a", "b", "x"]});
        let flow = json!({"nodes": [{"type": "on_flow_start", "data": {"nodeType": "on_flow_start",
            "outputs": [{"id": "exec-out", "type": "execution"}, {"id": "a", "type": "string", "required": true},
                        {"id": "b", "type": "string", "label": "Bee"}],
            "pinDefaults": {"b": "dflt"}}}]});
        let out = reconcile_visualflow_schema(&schema, &flow).unwrap();
        assert_eq!(out["required"], json!(["x", "a"]));
        assert_eq!(out["properties"]["b"]["title"], json!("Bee"));
        assert_eq!(out["properties"]["b"]["default"], json!("dflt"));
        assert!(reconcile_visualflow_schema(&schema, &json!({})).is_err());
    }

    #[test]
    fn the_conversation_rides_the_input_and_the_automation_keeps_its_own_consent() {
        let schema = normalize_input_schema(&v(BASIC)).unwrap();
        let conv = Conversation {
            provider: "lmstudio".into(),
            model: "qwen3-4b".into(),
            reasoning: "high".into(),
            tools: Some(vec!["read_file".into()]),
            skills: vec!["coredoc".into()],
            max_iterations: 30,
            max_tokens: 65536,
            ..Conversation::default()
        };
        let input = automation_input("check it", &schema_defaults(Some(&schema)), &conv).unwrap();
        assert_eq!(input["prompt"], json!("check it"));
        assert_eq!(input["context"], json!({"task": "check it"}));
        assert_eq!(input["use_context"], json!(false));
        assert_eq!(input["use_session_history"], json!(true));
        assert_eq!(input["provider"], json!("lmstudio"));
        assert_eq!(input["_runtime"]["model"], json!("qwen3-4b"));
        assert_eq!(input["_runtime"]["thinking"], json!("high"));
        assert_eq!(input["_runtime"]["allowed_tools"], json!(["read_file"]));
        assert_eq!(
            input["_limits"],
            json!({"max_iterations": 30, "max_tokens": 65536})
        );
        assert_eq!(input["max_iterations"], json!(30));
        assert_eq!(input["skills"], json!(["coredoc"]));
        assert!(input.get("workspace_root").is_none());
        assert!(input["_runtime"].get("tool_policy").is_none());
        let half = Conversation {
            provider: "lmstudio".into(),
            ..Conversation::default()
        };
        assert_eq!(
            automation_input("x", &Map::new(), &half).unwrap_err(),
            PROVIDER_MODEL_PAIR
        );
    }

    #[test]
    fn tool_selection_is_the_kits() {
        let built = json!({"tools": ["a", "b"], "_runtime": {"allowed_tools": ["a", "b"]}});
        assert_eq!(
            automation_tool_selection(&built),
            Some(vec!["a".into(), "b".into()])
        );
        let none = with_automation_tools(&built, None);
        assert_eq!(
            none,
            json!({}),
            "no selection removes tools and the emptied _runtime"
        );
        let empty = with_automation_tools(&json!({}), Some(&[]));
        assert_eq!(
            empty,
            json!({"tools": [], "_runtime": {"allowed_tools": []}})
        );
        // Untouched conversation: the served default list is the first selection.
        let d: Map<String, Value> = serde_json::from_value(json!({"tools": ["x", "y"]})).unwrap();
        assert_eq!(
            initial_tools(&d, &Conversation::default()),
            Some(vec!["x".into(), "y".into()])
        );
        assert_eq!(initial_tools(&Map::new(), &Conversation::default()), None);
        let custom = Conversation {
            tools: Some(vec!["z".into()]),
            ..Conversation::default()
        };
        assert_eq!(initial_tools(&d, &custom), Some(vec!["z".into()]));
    }
}
