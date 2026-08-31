//! Human-readable rendering model for the tool-approval modal.
//!
//! Pure data (no widgets): a tool-call batch becomes per-call cards —
//! headline (tool name + tier), a one-line intent summary, the COMMAND
//! string first-class for `execute_command` (it is the thing being
//! approved), and aligned `key: value` parameter rows with honest
//! truncation notes. The raw JSON stays one keypress away in the modal
//! (`f`), and the run ledger always holds the full arguments.

use serde_json::Value;

use crate::tool_policy::{classify_call_with, Tier, ToolClass};

/// Cap for one parameter VALUE on a row (the modal is ~72 cols; the key
/// column takes up to 18). Longer values truncate with a note.
const VALUE_CAP: usize = 110;
/// Nested-object children flattened per parameter before summarizing.
const CHILD_CAP: usize = 6;
/// Array items previewed before "…".
const ARRAY_PREVIEW: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct CallView {
    pub name: String,
    pub tier: Tier,
    /// One-line intent ("write src/main.rs", "run a shell command", …).
    pub summary: String,
    /// `execute_command` only: the command string, shown first-class.
    pub command: Option<String>,
    /// Aligned parameter rows (key, formatted value), pre-capped.
    pub params: Vec<(String, String)>,
    /// Any value was shortened (the "values shortened — f shows the
    /// full JSON" note renders when true).
    pub truncated: bool,
    /// The inventory serves this tool `enabled: false` (gate-disabled).
    /// The belt clamps such calls to ask regardless of tier/pins — the
    /// card must SAY so instead of rendering a tier line that implies
    /// approvability (cycle-2 adversary P2-1: a disabled call reaching
    /// a wait is exactly the defense-in-depth case, and the modal was
    /// blind to it).
    pub served_disabled: bool,
    /// The named gate, when the inventory carried one (render aid).
    pub enable_gate: String,
}

/// Build the per-call views for a batch (name-table classification —
/// used by tests and any caller without the live inventory).
pub fn build_call_views(tool_calls: &[Value]) -> Vec<CallView> {
    build_call_views_with(tool_calls, &[])
}

/// Build the per-call views PREFERRING the gateway's served tier/approval
/// when the inventory carried it, so the card's "needs: <tier>" line
/// matches the belt's auto-approve decision for ENABLED tools (both go
/// through `classify_call_with`). Served-DISABLED tools diverge by
/// design: the belt clamps them to ask above tiers and pins, so their
/// card carries `served_disabled` + the gate and the render must show
/// the gate line, not the tier as an approvability claim. Empty
/// `classes` reproduces the name table.
pub fn build_call_views_with(tool_calls: &[Value], classes: &[ToolClass]) -> Vec<CallView> {
    tool_calls.iter().map(|tc| call_view(tc, classes)).collect()
}

fn call_view(tc: &Value, classes: &[ToolClass]) -> CallView {
    let name = tc
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("(unnamed tool)")
        .to_string();
    let args = tc.get("arguments");
    let tier = classify_call_with(&name, classes);
    let disabled_class = classes.iter().find(|c| c.name == name && c.served_disabled);
    let served_disabled = disabled_class.is_some();
    let mut truncated = false;

    // execute_command: extract the command string first-class; the rest
    // of the arguments (cwd, timeout, …) stay as secondary rows.
    let mut command: Option<String> = None;
    let mut params: Vec<(String, String)> = Vec::new();
    match args {
        Some(Value::Object(map)) => {
            for (k, v) in map {
                if name == "execute_command" && k == "command" {
                    command = Some(match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    });
                    continue;
                }
                push_param(&mut params, k, v, &mut truncated);
            }
        }
        // Non-object argument payloads (string blobs, null, …) render as
        // one honest row rather than pretending to be structured.
        Some(Value::Null) | None => {}
        Some(Value::String(s)) if s.trim().is_empty() => {}
        Some(other) => {
            let (text, cut) = format_scalar(other, VALUE_CAP);
            truncated |= cut;
            params.push(("arguments".into(), text));
        }
    }

    let summary = intent_summary(&name, args, &params);
    CallView {
        name,
        tier,
        summary,
        command,
        params,
        truncated,
        served_disabled,
        enable_gate: disabled_class
            .map(|c| c.enable_gate.clone())
            .unwrap_or_default(),
    }
}

/// One parameter row; nested objects flatten ONE level (`key.child`),
/// deeper structures + arrays summarize compactly.
fn push_param(out: &mut Vec<(String, String)>, key: &str, value: &Value, truncated: &mut bool) {
    match value {
        Value::Object(child) => {
            for (i, (ck, cv)) in child.iter().enumerate() {
                if i == CHILD_CAP {
                    out.push((
                        format!("{key}.…"),
                        format!("+{} more", child.len() - CHILD_CAP),
                    ));
                    *truncated = true;
                    break;
                }
                let (text, cut) = format_scalar(cv, VALUE_CAP);
                *truncated |= cut;
                out.push((format!("{key}.{ck}"), text));
            }
            if child.is_empty() {
                out.push((key.to_string(), "{}".into()));
            }
        }
        _ => {
            let (text, cut) = format_scalar(value, VALUE_CAP);
            *truncated |= cut;
            out.push((key.to_string(), text));
        }
    }
}

/// Recursion guard for `format_scalar`: nested arrays are the only path
/// that deepens (objects render as a flat one-liner). serde_json already
/// caps parse depth ~128, but the summary never needs more than a couple
/// of levels — a hostile deeply-nested array must not drive stack depth
/// with the input (robust regardless of any parser limit).
const MAX_PREVIEW_DEPTH: usize = 4;

/// Scalar/compact formatting: strings UNQUOTED (first line only, with an
/// honest `(+N lines)` marker), arrays as `[N items] a, b, …`, nested
/// values as capped compact JSON. Returns (text, was_truncated).
fn format_scalar(value: &Value, cap: usize) -> (String, bool) {
    format_scalar_depth(value, cap, 0)
}

fn format_scalar_depth(value: &Value, cap: usize, depth: usize) -> (String, bool) {
    let mut cut = false;
    let text = match value {
        Value::String(s) => {
            let mut lines = s.lines();
            let first = lines.next().unwrap_or("").to_string();
            let extra = lines.count();
            if extra > 0 {
                cut = true;
                format!(
                    "{first} (+{extra} more line{})",
                    if extra == 1 { "" } else { "s" }
                )
            } else {
                first
            }
        }
        // Past the depth guard, a nested container is summarized by shape
        // rather than recursed into (hostile deep nesting can never grow
        // the call stack with the input).
        Value::Array(_) | Value::Object(_) if depth >= MAX_PREVIEW_DEPTH => {
            cut = true;
            match value {
                Value::Array(items) => format!("[{} items]", items.len()),
                _ => "{…}".into(),
            }
        }
        Value::Array(items) => {
            let previews: Vec<String> = items
                .iter()
                .take(ARRAY_PREVIEW)
                .map(|v| format_scalar_depth(v, 24, depth + 1).0)
                .collect();
            let ellipsis = if items.len() > ARRAY_PREVIEW {
                cut = true;
                ", …"
            } else {
                ""
            };
            format!(
                "[{} items] {}{}",
                items.len(),
                previews.join(", "),
                ellipsis
            )
        }
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Object(_) => value.to_string(), // compact one-liner
    };
    if text.chars().count() > cap {
        let capped: String = text.chars().take(cap.saturating_sub(1)).collect();
        (format!("{capped}…"), true)
    } else {
        (text, cut)
    }
}

/// One-line intent. Known tool families get a verb phrase; everything
/// else falls back to the primary-argument heuristic (generic — works
/// for MCP/future tools too).
fn intent_summary(name: &str, args: Option<&Value>, params: &[(String, String)]) -> String {
    let arg_str = |key: &str| -> Option<String> {
        args?
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    match name {
        "write_file" => {
            if let Some(p) = arg_str("path").or_else(|| arg_str("file_path")) {
                return format!("write {p}");
            }
        }
        "edit_file" => {
            if let Some(p) = arg_str("path").or_else(|| arg_str("file_path")) {
                return format!("edit {p}");
            }
        }
        "read_file" => {
            if let Some(p) = arg_str("path").or_else(|| arg_str("file_path")) {
                return format!("read {p}");
            }
        }
        "fetch_url" => {
            if let Some(u) = arg_str("url") {
                return format!("fetch {u}");
            }
        }
        "execute_command" => {
            // The command itself renders first-class, not in the summary.
            return match arg_str("cwd") {
                Some(cwd) => format!("run a shell command in {cwd}"),
                None => "run a shell command".into(),
            };
        }
        _ => {}
    }
    // Generic: the first recognizable primary argument names the intent.
    for key in [
        "path",
        "file_path",
        "url",
        "query",
        "pattern",
        "name",
        "command",
    ] {
        if let Some(v) = arg_str(key) {
            let (capped, _) = format_scalar(&Value::String(v), 80);
            return format!("{key}: {capped}");
        }
    }
    match params.len() {
        0 => "no parameters".into(),
        1 => "1 parameter".into(),
        n => format!("{n} parameters"),
    }
}

/// Pretty JSON of the whole batch (the `f` toggle + the ledger's truth).
pub fn full_json(tool_calls: &[Value]) -> String {
    serde_json::to_string_pretty(&Value::Array(tool_calls.to_vec()))
        .unwrap_or_else(|_| format!("{tool_calls:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn write_file_card_reads_like_a_sentence() {
        let calls = vec![json!({
            "name": "write_file",
            "arguments": {
                "path": "src/main.rs",
                "content": "fn main() {\n    println!(\"hi\");\n}\n",
                "overwrite": true
            }
        })];
        let views = build_call_views(&calls);
        assert_eq!(views.len(), 1);
        let v = &views[0];
        assert_eq!(v.name, "write_file");
        assert_eq!(v.tier, Tier::Write);
        assert_eq!(v.summary, "write src/main.rs");
        assert!(v.command.is_none());
        // Params: unquoted values, multi-line content marked honestly.
        let content = v.params.iter().find(|(k, _)| k == "content").unwrap();
        assert!(
            content.1.starts_with("fn main() {") && content.1.contains("(+2 more lines)"),
            "multiline content shows first line + honest marker: {:?}",
            content.1
        );
        let overwrite = v.params.iter().find(|(k, _)| k == "overwrite").unwrap();
        assert_eq!(overwrite.1, "true");
        assert!(v.truncated, "multi-line content flags truncation");
    }

    #[test]
    fn execute_command_promotes_the_command_string() {
        let calls = vec![json!({
            "name": "execute_command",
            "arguments": {"command": "cargo test --lib", "cwd": "/tmp/proj", "timeout": 60}
        })];
        let v = &build_call_views(&calls)[0];
        assert_eq!(v.command.as_deref(), Some("cargo test --lib"));
        assert_eq!(v.tier, Tier::All);
        assert_eq!(v.summary, "run a shell command in /tmp/proj");
        // command is NOT duplicated in params; cwd/timeout are secondary.
        assert!(v.params.iter().all(|(k, _)| k != "command"));
        assert!(v
            .params
            .iter()
            .any(|(k, val)| k == "cwd" && val == "/tmp/proj"));
        assert!(v
            .params
            .iter()
            .any(|(k, val)| k == "timeout" && val == "60"));
    }

    #[test]
    fn git_commands_classify_all_in_the_card_proof_retired() {
        // c5057: the client git proof retired — the card shows the
        // honest client-side classification (All); the runtime's
        // git_read_only@v1 refiner approves proven reads server-side
        // before a wait (and this card) ever exists for them.
        let calls = vec![json!({
            "name": "execute_command",
            "arguments": {"command": "git status -s"}
        })];
        assert_eq!(build_call_views(&calls)[0].tier, Tier::All);
    }

    #[test]
    fn nested_objects_flatten_one_level_and_arrays_summarize() {
        let calls = vec![json!({
            "name": "mystery_tool",
            "arguments": {
                "options": {"depth": 3, "follow": false},
                "targets": ["a.rs", "b.rs", "c.rs", "d.rs"]
            }
        })];
        let v = &build_call_views(&calls)[0];
        assert_eq!(v.tier, Tier::All, "unknown tool fails closed");
        assert!(v
            .params
            .iter()
            .any(|(k, val)| k == "options.depth" && val == "3"));
        assert!(v
            .params
            .iter()
            .any(|(k, val)| k == "options.follow" && val == "false"));
        let targets = v.params.iter().find(|(k, _)| k == "targets").unwrap();
        assert!(
            targets.1.starts_with("[4 items] a.rs, b.rs, c.rs, …"),
            "array preview: {:?}",
            targets.1
        );
    }

    #[test]
    fn long_values_cap_with_ellipsis_and_flag_truncation() {
        let long = "x".repeat(500);
        let calls = vec![json!({"name": "write_file",
                                "arguments": {"path": long, "content": "y"}})];
        let v = &build_call_views(&calls)[0];
        let path = v.params.iter().find(|(k, _)| k == "path").unwrap();
        assert!(path.1.chars().count() <= VALUE_CAP);
        assert!(path.1.ends_with('…'));
        assert!(v.truncated);
    }

    #[test]
    fn malformed_calls_never_panic_and_stay_honest() {
        let calls = vec![
            json!({}),                                  // nameless, argless
            json!({"name": "t", "arguments": null}),    // null args
            json!({"name": "t2", "arguments": "blob"}), // string args
        ];
        let views = build_call_views(&calls);
        assert_eq!(views[0].name, "(unnamed tool)");
        assert_eq!(views[0].tier, Tier::All);
        assert!(views[1].params.is_empty());
        assert_eq!(views[2].params[0].0, "arguments");
        assert_eq!(views[2].params[0].1, "blob");
    }

    #[test]
    fn full_json_is_pretty_and_complete() {
        let calls = vec![json!({"name": "read_file", "arguments": {"path": "a"}})];
        let out = full_json(&calls);
        assert!(out.contains("\"name\": \"read_file\""));
        assert!(out.contains("\"path\": \"a\""));
    }

    // -----------------------------------------------------------------
    // Cycle-2 adversarial (bug (b)): hostile arguments must never panic,
    // never grow unboundedly, and never let hidden content masquerade.
    // The engine strips control clusters at draw AND in text::wrap /
    // truncate_ellipsis (verified in the crate), so the rendered modal
    // cannot be corrupted; these tests pin the DATA-model invariants the
    // card builder owns (bounded output, no panics, honest truncation).
    // -----------------------------------------------------------------

    #[test]
    fn control_chars_in_a_command_never_hide_a_second_line() {
        // A newline-injected command: text::wrap splits on \n (verified),
        // so the second line is a distinct wrapped row — never merged into
        // the first by a vanished control char. The card keeps the raw
        // command; the modal renderer + f-JSON show it truthfully.
        let calls = vec![json!({
            "name": "execute_command",
            "arguments": {"command": "git status\nrm -rf /"}
        })];
        let v = &build_call_views(&calls)[0];
        let cmd = v.command.as_deref().unwrap();
        assert!(cmd.contains("git status") && cmd.contains("rm -rf /"));
        // The full-JSON escapes control bytes into visible \n — truthful.
        let out = full_json(&calls);
        assert!(out.contains("rm -rf /"));
    }

    #[test]
    fn hostile_args_do_not_panic_and_stay_bounded() {
        let big = "x".repeat(10_000);
        // Deeply nested arrays (guard the recursion), arrays of objects,
        // 10KB values, control/unicode in every field.
        let mut nested = json!("leaf");
        for _ in 0..200 {
            nested = json!([nested]);
        }
        let calls = vec![
            json!({
                "name": "execute_command",
                "arguments": {
                    "command": format!("echo \u{1b}[2J{big}\u{7}\r\t\u{202e}drow"),
                    "env": {"A\u{0}B": "v\u{1b}al", "n": 5, "deep": {"x": {"y": {"z": 1}}}},
                }
            }),
            json!({
                "name": "mystery_tool",
                "arguments": {
                    "targets": [{"path": "a"}, {"path": "b"}, {"path": "c"}, {"path": "d"}],
                    "blob": big,
                    "nested": nested,
                }
            }),
        ];
        let views = build_call_views(&calls); // must not panic / recurse away
        assert_eq!(views.len(), 2);
        // Every param VALUE is bounded by VALUE_CAP; the truncation flag is
        // honest for the oversized ones.
        for v in &views {
            for (_, val) in &v.params {
                assert!(
                    val.chars().count() <= VALUE_CAP,
                    "param value exceeds cap: {} chars",
                    val.chars().count()
                );
            }
        }
        assert!(views[1].truncated, "10KB blob flags truncation");
        // The command is kept raw on the card (rendering strips control
        // clusters); it is not silently emptied.
        assert!(views[0].command.as_deref().unwrap().contains("echo"));
        // full_json never panics on any of it.
        let _ = full_json(&calls);
    }

    #[test]
    fn card_prefers_server_truth_for_tier() {
        // An MCP tool the name table calls All classifies Read when the
        // gateway served approval:auto — the card's tier matches the belt.
        let classes = vec![ToolClass {
            name: "mcp::search".into(),
            approval: Some("auto".into()),
            tier: Some("tier2_world".into()),
            ..Default::default()
        }];
        let calls = vec![json!({"name": "mcp::search", "arguments": {"q": "x"}})];
        assert_eq!(build_call_views(&calls)[0].tier, Tier::All); // name table
        assert_eq!(build_call_views_with(&calls, &classes)[0].tier, Tier::Read);
        // server truth
    }

    /// A served-disabled call's card carries the disabled fact + gate
    /// (cycle-2 adversary P2-1): the belt clamps such calls to ask above
    /// tiers and pins, so a card rendering only the tier would imply an
    /// approvability the gateway will refuse — the render needs the
    /// truth on the view model.
    #[test]
    fn card_carries_served_disabled_and_gate() {
        let classes = vec![ToolClass {
            name: "send_email".into(),
            // The defense-in-depth scenario: an older/pre-tiers fold
            // serving auto on a disabled row must not read as auto here.
            approval: Some("auto".into()),
            tier: Some("tier2_world".into()),
            served_disabled: true,
            enable_gate: "ABSTRACT_ENABLE_COMMS_TOOLS".into(),
            ..Default::default()
        }];
        let calls = vec![json!({"name": "send_email", "arguments": {"to": "x@y.z"}})];
        let views = build_call_views_with(&calls, &classes);
        assert!(views[0].served_disabled, "disabled fact reaches the card");
        assert_eq!(views[0].enable_gate, "ABSTRACT_ENABLE_COMMS_TOOLS");
        // Enabled rows carry neither.
        let enabled = build_call_views_with(
            &[json!({"name": "read_file", "arguments": {"path": "a"}})],
            &classes,
        );
        assert!(!enabled[0].served_disabled && enabled[0].enable_gate.is_empty());
    }
}
