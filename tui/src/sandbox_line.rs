//! The command sandbox, in the terminal (R12.1 / R13.4 / R14.4). Pure.
//!
//! Two gateway facts, shown verbatim — nothing is inferred from a command,
//! a tool name or the platform:
//!
//! - **Tool cards** — `GET /discovery/tools` marks every process-spawning
//!   tool (execute_command, shell_exec, local_helper_start, execute_python)
//!   with `sandboxed: bool` + `sandbox: "<state sentence>"` ("Sandboxed to
//!   this run's workspaces", "Refused: no command sandbox on this host",
//!   "Not sandboxed: unsandboxed commands allowed (flag)"); the answer's
//!   `command_sandbox.sentence` explains it. [`ToolSandboxState`].
//! - **Run views** — each such tool result records the sandbox it ran under
//!   (`output.sandbox = {kind, label, private_workspace, allowed, refused,
//!   builtin_refused, …}`, kept by the ledger). [`tool_sandbox`] turns it
//!   into ONE line — "Sandbox: macOS sandbox-exec · 3 workspaces enforced",
//!   or "Sandbox: none — refused" — plus the enforced paths: the same
//!   formatter as panel-chat 0.4.1 `toolSandbox` (the Code web's tool-call
//!   detail), word for word.

use serde_json::Value;

/// A process-spawning tool's state on its card (`ToolInfo.sandbox`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSandboxState {
    /// The row's `sandbox` text, verbatim.
    pub label: String,
    /// The answer's `command_sandbox.sentence` (the web's tooltip).
    pub sentence: String,
    /// ok (sandboxed) | warn (refused) | danger (unsandboxed by flag).
    pub tone: Tone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Danger,
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").trim().to_string()
}

/// The state of a tool the gateway marks as process-spawning, or `None` for
/// every other tool (no client-side guess). `command_sandbox` = the answer's
/// object.
pub fn tool_state(item: &Value, command_sandbox: Option<&Value>) -> Option<ToolSandboxState> {
    let label = text(item.get("sandbox"));
    let sandboxed = item.get("sandboxed").and_then(Value::as_bool)?;
    if label.is_empty() {
        return None;
    }
    let tone = if sandboxed {
        Tone::Ok
    } else if text(command_sandbox.and_then(|c| c.get("state"))) == "unsandboxed" {
        Tone::Danger
    } else {
        Tone::Warn
    };
    Some(ToolSandboxState {
        label,
        sentence: text(command_sandbox.and_then(|c| c.get("sentence"))),
        tone,
    })
}

/// One enforced path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxRow {
    pub path: String,
    /// "Read & write" | "Read-only" | "Refused".
    pub mode: &'static str,
    /// The run's private workspace.
    pub private: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxState {
    /// An OS sandbox ran the command.
    Sandboxed,
    /// No sandbox: nothing ran.
    Refused,
    /// The host allowed commands without a sandbox.
    Unsandboxed,
}

/// One tool call's sandbox, from its ledger result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSandbox {
    pub state: SandboxState,
    pub kind: String,
    pub label: String,
    /// The one line shown for the call.
    pub line: String,
    /// The paths the sandbox enforced (empty when refused).
    pub rows: Vec<SandboxRow>,
    /// Built-in protected folders refused on top of the rows (counted, never listed).
    pub builtin_refused: u64,
}

impl ToolSandbox {
    /// The detail lines under the one line: each enforced path with its
    /// mode, then the built-in count (panel-chat `ToolSandboxLine`).
    pub fn detail_lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .rows
            .iter()
            .map(|r| {
                if r.private {
                    format!("{}  {} · this run's folder", r.path, r.mode)
                } else {
                    format!("{}  {}", r.path, r.mode)
                }
            })
            .collect();
        if !self.rows.is_empty() && self.builtin_refused > 0 {
            let n = self.builtin_refused;
            out.push(format!(
                "{n} built-in protected {} refused",
                if n == 1 { "folder" } else { "folders" }
            ));
        }
        out
    }
}

/// The sandbox evidence of one tool result: `sandbox` on the value, or
/// `output.sandbox` — only when it names a `kind`.
pub fn sandbox_evidence(value: &Value) -> Option<&Value> {
    let has_kind = |v: &&Value| !text(v.get("kind")).is_empty();
    value
        .get("sandbox")
        .filter(|v| v.is_object())
        .filter(has_kind)
        .or_else(|| {
            value
                .get("output")
                .and_then(|o| o.get("sandbox"))
                .filter(|v| v.is_object())
                .filter(has_kind)
        })
}

/// One tool call's sandbox, or `None` when its result carries no sandbox evidence.
pub fn tool_sandbox(value: &Value) -> Option<ToolSandbox> {
    let sb = sandbox_evidence(value)?;
    let kind = text(sb.get("kind"));
    let label = {
        let l = text(sb.get("label"));
        if l.is_empty() {
            kind.clone()
        } else {
            l
        }
    };
    let builtin_refused = sb
        .get("builtin_refused")
        .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0) as u64)))
        .unwrap_or(0);
    if kind == "none" {
        return Some(ToolSandbox {
            state: SandboxState::Refused,
            line: format!("Sandbox: {label} — refused"),
            kind,
            label,
            rows: Vec::new(),
            builtin_refused: 0,
        });
    }
    let mut rows = Vec::new();
    let private = text(sb.get("private_workspace"));
    if !private.is_empty() {
        rows.push(SandboxRow {
            path: private,
            mode: "Read & write",
            private: true,
        });
    }
    for item in sb.get("allowed").and_then(Value::as_array).into_iter().flatten() {
        let path = text(item.get("path"));
        if !path.is_empty() {
            rows.push(SandboxRow {
                path,
                mode: if text(item.get("mode")) == "ro" {
                    "Read-only"
                } else {
                    "Read & write"
                },
                private: false,
            });
        }
    }
    for item in sb.get("refused").and_then(Value::as_array).into_iter().flatten() {
        let path = item.as_str().unwrap_or("").trim().to_string();
        if !path.is_empty() {
            rows.push(SandboxRow {
                path,
                mode: "Refused",
                private: false,
            });
        }
    }
    if kind == "unsandboxed" {
        return Some(ToolSandbox {
            state: SandboxState::Unsandboxed,
            line: format!("Sandbox: {label}"),
            kind,
            label,
            rows: Vec::new(),
            builtin_refused: 0,
        });
    }
    let n = rows.len();
    Some(ToolSandbox {
        state: SandboxState::Sandboxed,
        line: format!(
            "Sandbox: {label} · {n} {} enforced",
            if n == 1 { "workspace" } else { "workspaces" }
        ),
        kind,
        label,
        rows,
        builtin_refused,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const LEDGER: &str = include_str!("../tests/fixtures/workspaces/sandbox_ledger.json");
    const TOOLS: &str =
        include_str!("../tests/fixtures/workspaces/discovery_tools_command_sandbox.json");

    fn results() -> Vec<Value> {
        let v: Value = serde_json::from_str(LEDGER).unwrap();
        v["records"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r["result"]["results"].as_array().cloned().unwrap_or_default())
            .collect()
    }

    #[test]
    fn a_sandboxed_command_is_one_line_plus_its_paths() {
        let r = results();
        let exec = r.iter().find(|x| x["call_id"].as_str().unwrap().contains("309e")).unwrap();
        let sb = tool_sandbox(exec).unwrap();
        assert_eq!(sb.state, SandboxState::Sandboxed);
        assert_eq!(sb.line, "Sandbox: macOS sandbox-exec · 4 workspaces enforced");
        let lines = sb.detail_lines();
        assert_eq!(lines.len(), 5);
        assert!(lines[0].ends_with("  Read & write · this run's folder"));
        assert!(lines[1].ends_with("/Users/ada/home/work/project  Read & write"));
        assert!(lines[2].ends_with("/Users/ada  Read-only"));
        assert!(lines[3].ends_with("/Users/ada/home  Refused"));
        assert_eq!(lines[4], "12 built-in protected folders refused");
    }

    #[test]
    fn a_refused_command_says_none_refused_and_lists_nothing() {
        let r = results();
        let refused = r.iter().find(|x| x["call_id"] == json!("rtcall_refused_1")).unwrap();
        let sb = tool_sandbox(refused).unwrap();
        assert_eq!(sb.state, SandboxState::Refused);
        assert_eq!(sb.line, "Sandbox: none — refused");
        assert!(sb.detail_lines().is_empty());
    }

    #[test]
    fn a_tool_without_evidence_has_no_line() {
        let r = results();
        let plain: Vec<_> = r.iter().filter(|x| x["name"] == json!("read_file")).collect();
        assert!(!plain.is_empty());
        assert!(plain.iter().all(|x| tool_sandbox(x).is_none()));
        assert!(tool_sandbox(&json!({"output": {"sandbox": {"label": "x"}}})).is_none());
        let one = tool_sandbox(&json!({"sandbox": {"kind": "k", "label": "L", "allowed": [{"path": "/a", "mode": "ro"}]}})).unwrap();
        assert_eq!(one.line, "Sandbox: L · 1 workspace enforced");
        let un = tool_sandbox(&json!({"sandbox": {"kind": "unsandboxed", "label": "unsandboxed (flag)"}})).unwrap();
        assert_eq!(un.state, SandboxState::Unsandboxed);
        assert_eq!(un.line, "Sandbox: unsandboxed (flag)");
    }

    #[test]
    fn tool_cards_take_the_gateway_state_verbatim() {
        let v: Value = serde_json::from_str(TOOLS).unwrap();
        let cs = v.get("command_sandbox");
        let items = v["items"].as_array().unwrap();
        let state = |name: &str| {
            let item = items.iter().find(|t| t["name"] == json!(name)).unwrap();
            tool_state(item, cs)
        };
        let exec = state("execute_command").unwrap();
        assert_eq!(exec.label, "Sandboxed to this run's workspaces");
        assert_eq!(exec.tone, Tone::Ok);
        assert_eq!(
            exec.sentence,
            "Every command a run starts is confined by the operating system to that run's workspaces."
        );
        assert!(state("read_file").is_none(), "unmarked tools show nothing");
        let refused = tool_state(
            &json!({"name": "execute_command", "sandboxed": false, "sandbox": "Refused: no command sandbox on this host"}),
            Some(&json!({"state": "refused", "sentence": "s"})),
        )
        .unwrap();
        assert_eq!(refused.tone, Tone::Warn);
        let flag = tool_state(
            &json!({"name": "execute_command", "sandboxed": false, "sandbox": "Not sandboxed: unsandboxed commands allowed (flag)"}),
            Some(&json!({"state": "unsandboxed"})),
        )
        .unwrap();
        assert_eq!(flag.tone, Tone::Danger);
        assert_eq!(flag.label, "Not sandboxed: unsandboxed commands allowed (flag)");
    }
}
