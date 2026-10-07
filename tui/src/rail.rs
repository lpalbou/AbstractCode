//! The settings rail of the Code WUI, in the terminal: pure rules.
//!
//! The web's right rail holds eight panels — Activity, Files, Model,
//! Workflow, Workspace, Tools, Skills, Voice — bound to what is selected:
//! a conversation (its run settings) or an automation (its DEFINITION:
//! every change is saved through the gateway as a new revision). This
//! module holds what both renderings share and what the tests pin:
//!
//! - the panel list ([`Panel`]) and the binding ([`Binding`]);
//! - an automation definition's run settings read from / written to its
//!   `target.input_data` with the SAME keys a conversation turn writes
//!   (the web's `automation_settings.ts`, key for key: an unset value is
//!   REMOVED, never written as "", so the gateway default applies again);
//! - the strings of the revision line, the workspace policy, the voice
//!   routes and the Activity groups.
//!
//! No HTTP and no signals here: the gateway half is `gateway::rail`, the
//! screen is `ui::rail_view`.

use serde_json::{json, Map, Value};

use crate::transcript::{Item, ToolStatus};

// ---------------------------------------------------------------------------
// What the gateway answered (store.rail)
// ---------------------------------------------------------------------------

/// One automation run's Activity: its id and, once read, its items.
pub type RunActivity = (String, Option<Result<Vec<Item>, String>>);

/// The rail's and the conversations board's gateway facts. Written only by
/// posted closures of the `gateway::rail` lane (UI thread).
#[derive(Debug, Clone, PartialEq)]
pub struct RailData {
    /// `GET /workspace/policy`; `None` until read.
    pub policy: Option<Result<Value, String>>,
    /// Automation runs' Activity: (run id, items once read).
    pub run_activity: Vec<RunActivity>,
    /// The archived conversations (rows under `Archived · N`).
    pub archived: crate::store::SessionIndex,
    /// The board's `Archived · N` line is open.
    pub archived_open: bool,
    /// An archive/unarchive in flight.
    pub archive_busy: bool,
    /// "Not archived: …" / "Not unarchived: …".
    pub board_error: String,
    /// Set when an archive succeeded: (archived id, the conversation to
    /// open if it was the open one). Taken by `ui::rail_view::wire_rail`.
    pub handover: Option<(String, Option<String>)>,
    /// The automation settings' save state (the revision line).
    pub save: SaveState,
    /// The panel the rail opened on last (reopened after a picker).
    pub panel: Option<Panel>,
    /// Reopen the rail here once the picker it opened closes.
    pub return_to: Option<(Binding, Panel)>,
    /// The selected row of `panel` (restored on reopen).
    pub cursor: usize,
}

impl Default for RailData {
    fn default() -> RailData {
        RailData {
            policy: None,
            run_activity: Vec::new(),
            archived: crate::store::SessionIndex::Unfetched,
            archived_open: false,
            archive_busy: false,
            board_error: String::new(),
            handover: None,
            save: SaveState::Idle,
            panel: None,
            return_to: None,
            cursor: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Panels and binding
// ---------------------------------------------------------------------------

/// The rail's panels, in the web's order (`right_rail.tsx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Activity,
    Files,
    Model,
    Workflow,
    Workspace,
    Tools,
    Skills,
    Voice,
}

impl Panel {
    pub const ALL: [Panel; 8] = [
        Panel::Activity,
        Panel::Files,
        Panel::Model,
        Panel::Workflow,
        Panel::Workspace,
        Panel::Tools,
        Panel::Skills,
        Panel::Voice,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Panel::Activity => "Activity",
            Panel::Files => "Files",
            Panel::Model => "Model",
            Panel::Workflow => "Workflow",
            Panel::Workspace => "Workspace",
            Panel::Tools => "Tools",
            Panel::Skills => "Skills",
            Panel::Voice => "Voice",
        }
    }

    pub fn index(self) -> usize {
        Panel::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    /// The panel `delta` steps away, wrapping.
    pub fn step(self, delta: i64) -> Panel {
        let n = Panel::ALL.len() as i64;
        Panel::ALL[((self.index() as i64 + delta).rem_euclid(n)) as usize]
    }

    /// `/settings model`, `/settings tools` … (case-insensitive label).
    pub fn parse(word: &str) -> Option<Panel> {
        let w = word.trim().to_ascii_lowercase();
        Panel::ALL
            .into_iter()
            .find(|p| p.label().to_ascii_lowercase() == w)
    }
}

/// What the settings panels edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// The current conversation (this terminal's session).
    Conversation,
    /// One automation's definition, by id.
    Automation(String),
}

// ---------------------------------------------------------------------------
// An automation's run settings (definition.target.input_data)
// ---------------------------------------------------------------------------

/// The run settings an automation definition holds, as the panels show
/// them. Empty / `None` = "Gateway default" (nothing written).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunSettings {
    pub provider: String,
    pub model: String,
    pub reasoning: String,
    pub speculation: Option<Value>,
    pub max_iterations: String,
    pub max_tokens: String,
    pub system: String,
    /// The automation's workspaces (`input_data.workspace`, the R11 run
    /// level); `None` = "Use my default".
    pub workspace: Option<crate::workspaces::RunValue>,
    /// `None` = all tools (no allowlist); `Some(list)` = custom allowlist.
    pub tools: Option<Vec<String>>,
    /// Per-tool approval overrides: (name, "approve" | "ask").
    pub approval: Vec<(String, String)>,
    pub skills: Vec<String>,
}

fn obj(v: Option<&Value>) -> Map<String, Value> {
    v.and_then(Value::as_object).cloned().unwrap_or_default()
}

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The kit's `automationToolSelection`: `tools` (or the `_runtime`
/// ceiling), filtered by the ceiling; `None` = no allowlist.
fn tool_selection(input: &Map<String, Value>) -> Option<Vec<String>> {
    let runtime = obj(input.get("_runtime"));
    let ceiling = runtime
        .get("allowed_tools")
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

/// Read the panels' settings from a definition's `input_data`.
pub fn read_settings(input: &Value) -> RunSettings {
    let data = obj(Some(input));
    let runtime = obj(data.get("_runtime"));
    let limits = obj(data.get("_limits"));
    let mut provider = text(data.get("provider"));
    if provider.is_empty() {
        provider = text(runtime.get("provider"));
    }
    let mut model = text(data.get("model"));
    if model.is_empty() {
        model = text(runtime.get("model"));
    }
    let both = !provider.is_empty() && !model.is_empty();
    let policy = obj(runtime.get("tool_policy"));
    let mut approval = Vec::new();
    for name in strings(policy.get("auto_approve_tools")) {
        approval.push((name, "approve".to_string()));
    }
    for name in strings(policy.get("require_approval_tools")) {
        approval.push((name, "ask".to_string()));
    }
    RunSettings {
        provider: if both { provider } else { String::new() },
        model: if both { model } else { String::new() },
        reasoning: text(runtime.get("thinking")),
        speculation: runtime.get("speculation").cloned(),
        max_iterations: text(limits.get("max_iterations")),
        max_tokens: text(limits.get("max_tokens")),
        system: text(runtime.get("system_prompt_extra")),
        workspace: crate::workspaces::run_value_from(input),
        tools: tool_selection(&data),
        approval,
        skills: strings(data.get("skills")),
    }
}

fn set_or_delete(target: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    match value {
        None => {
            target.remove(key);
        }
        Some(Value::String(s)) if s.is_empty() => {
            target.remove(key);
        }
        Some(Value::Array(a)) if a.is_empty() => {
            target.remove(key);
        }
        Some(Value::Null) => {
            target.remove(key);
        }
        Some(v) => {
            target.insert(key.to_string(), v);
        }
    }
}

fn positive_int(s: &str) -> Option<Value> {
    s.trim()
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .map(|n| json!(n))
}

/// The definition's `input_data` with `s` applied — everything else kept
/// (the web's `withAutomationRunPreferences`).
pub fn apply_settings(input: &Value, s: &RunSettings) -> Value {
    let mut next = obj(Some(input));
    let mut runtime = obj(next.get("_runtime"));
    let mut limits = obj(next.get("_limits"));
    let both = !s.provider.is_empty() && !s.model.is_empty();
    let route = |v: &str| both.then(|| json!(v));
    set_or_delete(&mut next, "provider", route(&s.provider));
    set_or_delete(&mut next, "model", route(&s.model));
    set_or_delete(&mut runtime, "provider", route(&s.provider));
    set_or_delete(&mut runtime, "model", route(&s.model));
    set_or_delete(&mut runtime, "thinking", Some(json!(s.reasoning)));
    set_or_delete(&mut runtime, "speculation", s.speculation.clone());
    set_or_delete(
        &mut runtime,
        "system_prompt_extra",
        Some(json!(s.system.trim())),
    );
    set_or_delete(
        &mut limits,
        "max_iterations",
        positive_int(&s.max_iterations),
    );
    set_or_delete(&mut limits, "max_tokens", positive_int(&s.max_tokens));
    let approve: Vec<&str> = s
        .approval
        .iter()
        .filter(|(_, v)| v == "approve")
        .map(|(k, _)| k.as_str())
        .collect();
    let ask: Vec<&str> = s
        .approval
        .iter()
        .filter(|(_, v)| v == "ask")
        .map(|(k, _)| k.as_str())
        .collect();
    let policy = if approve.is_empty() && ask.is_empty() {
        None
    } else {
        let mut p = Map::new();
        if !approve.is_empty() {
            p.insert("auto_approve_tools".into(), json!(approve));
        }
        if !ask.is_empty() {
            p.insert("require_approval_tools".into(), json!(ask));
        }
        Some(Value::Object(p))
    };
    set_or_delete(&mut runtime, "tool_policy", policy);
    // The kit's `withAutomationTools`: an allowlist is `tools` AND the
    // `_runtime.allowed_tools` ceiling; "all tools" removes both.
    match &s.tools {
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
    if limits.is_empty() {
        next.remove("_limits");
    } else {
        next.insert("_limits".into(), Value::Object(limits));
    }
    apply_workspace(&mut next, s.workspace.as_ref());
    set_or_delete(&mut next, "skills", Some(json!(s.skills)));
    Value::Object(next)
}

/// The workspaces part of the web's `withAutomationRunPreferences`: the
/// access mode is retired; a chosen payload replaces the R9 list; "Use my
/// default" removes the payload (and the list the gateway derived from it).
fn apply_workspace(next: &mut Map<String, Value>, value: Option<&crate::workspaces::RunValue>) {
    next.remove("workspace_access_mode");
    match value {
        None => {
            if next.get("workspace").is_some_and(Value::is_object) {
                next.remove("workspace_allowed_paths");
            }
            next.remove("workspace");
        }
        Some(v) => {
            next.insert("workspace".into(), v.to_json());
            next.remove("workspace_allowed_paths");
        }
    }
}

/// The PATCH `changes` that set the automation's workspaces (one revision),
/// or `None` when nothing would change.
pub fn workspace_changes(target: &Value, value: Option<&crate::workspaces::RunValue>) -> Option<Value> {
    let before = target.get("input_data").cloned().unwrap_or(json!({}));
    let mut after = obj(Some(&before));
    apply_workspace(&mut after, value);
    let after = Value::Object(after);
    if Value::Object(obj(Some(&before))) == after {
        return None;
    }
    Some(json!({"target": {
        "bundle_ref": target.get("bundle_ref").cloned().unwrap_or(Value::Null),
        "flow_id": target.get("flow_id").cloned().unwrap_or(Value::Null),
        "input_data": after,
    }}))
}

/// The PATCH `changes` for new settings — `{target: {bundle_ref, flow_id,
/// input_data}}` — or `None` when the input would not change.
pub fn settings_changes(target: &Value, s: &RunSettings) -> Option<Value> {
    let before = target.get("input_data").cloned().unwrap_or(json!({}));
    let after = apply_settings(&before, s);
    let before_obj = Value::Object(obj(Some(&before)));
    if before_obj == after {
        return None;
    }
    Some(json!({"target": {
        "bundle_ref": target.get("bundle_ref").cloned().unwrap_or(Value::Null),
        "flow_id": target.get("flow_id").cloned().unwrap_or(Value::Null),
        "input_data": after,
    }}))
}

/// The PATCH `changes` that point the automation at another workflow
/// (its run settings kept).
pub fn workflow_changes(target: &Value, bundle_ref: &str, flow_id: &str) -> Value {
    json!({"target": {
        "bundle_ref": bundle_ref,
        "flow_id": flow_id,
        "input_data": target.get("input_data").cloned().unwrap_or(json!({})),
    }})
}

// ---------------------------------------------------------------------------
// The revision line (web `AutomationBinding`)
// ---------------------------------------------------------------------------

/// Where a save stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SaveState {
    #[default]
    Idle,
    Saving,
    /// Saved as this revision.
    Saved(u64),
    /// Refused: the gateway's sentence.
    Refused(String),
    /// `revision_conflict`: someone else saved first.
    Conflict,
}

/// The status line under "Revision N", verbatim from the web.
pub fn save_line(state: &SaveState) -> String {
    match state {
        SaveState::Idle => {
            "Changes are saved as a new revision and apply from the next run.".into()
        }
        SaveState::Saving => "Saving…".into(),
        SaveState::Saved(n) => format!("Saved as revision {n}; applies from the next run."),
        SaveState::Conflict => "Not saved: the automation changed elsewhere. The latest revision is shown; make the change again.".into(),
        SaveState::Refused(why) => format!("Not saved: {why}"),
    }
}

// ---------------------------------------------------------------------------
// Values as the panels word them
// ---------------------------------------------------------------------------

/// "Gateway default: provider · model." / the generic sentence when the
/// gateway reports no default text route.
pub fn model_default_line(default_route: &(String, String)) -> String {
    if default_route.0.is_empty() || default_route.1.is_empty() {
        "The gateway picks its configured default provider and model for this task.".into()
    } else {
        format!(
            "Gateway default: {} · {}.",
            default_route.0, default_route.1
        )
    }
}

/// `Gateway default` | `Off` | `Depth N` for an MTP value.
pub fn speculation_label(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "Gateway default".into(),
        Some(Value::Bool(false)) => "Off".into(),
        Some(v) => match v
            .get("depth")
            .and_then(Value::as_u64)
            .or_else(|| v.as_u64())
        {
            Some(0) => "Off".into(),
            Some(n) => format!("Depth {n}"),
            None if v.get("enabled").and_then(Value::as_bool) == Some(false) => "Off".into(),
            None => v.to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// Activity: one group per iteration (web `activity_groups.ts`)
// ---------------------------------------------------------------------------

/// A group's status, most urgent first: running > waiting > failed > done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroupStatus {
    Running,
    Waiting,
    Failed,
    Done,
}

/// One foldable group: `Start` (before the first model step) or `Step N`.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub title: String,
    /// The model step's own line (empty for Start).
    pub summary: String,
    pub status: GroupStatus,
    /// "Waiting for you" when an approval or a question waits on you.
    pub waiting_for_you: bool,
    /// The group's rows: tool calls and approvals, as one line each.
    pub lines: Vec<String>,
}

impl Group {
    /// `Running` | `Waiting for you` | `Failed` | `Done`.
    pub fn status_label(&self) -> &'static str {
        match self.status {
            GroupStatus::Running => "Running",
            GroupStatus::Waiting if self.waiting_for_you => "Waiting for you",
            GroupStatus::Waiting => "Waiting",
            GroupStatus::Failed => "Failed",
            GroupStatus::Done => "Done",
        }
    }
}

fn one_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

fn tool_line(name: &str, args: &str, status: ToolStatus) -> (String, GroupStatus, bool) {
    let (word, st, you) = match status {
        ToolStatus::AwaitingApproval => ("approval needed", GroupStatus::Waiting, true),
        ToolStatus::Running => ("running", GroupStatus::Running, false),
        ToolStatus::Ok => ("done", GroupStatus::Done, false),
        ToolStatus::Failed => ("failed", GroupStatus::Failed, false),
        ToolStatus::Denied => ("denied", GroupStatus::Done, false),
        ToolStatus::Interrupted => ("interrupted", GroupStatus::Done, false),
    };
    let args = one_line(args);
    let line = if args.is_empty() {
        format!("{name} · {word}")
    } else {
        format!("{name} {args} · {word}")
    };
    (line, st, you)
}

/// Group a transcript's items per model step. `live` = the run is still
/// going (its newest group is Running unless it waits); `waiting` = a
/// question or approval waits on you in the newest group.
pub fn activity_groups(items: &[Item], live: bool, waiting: bool) -> Vec<Group> {
    let mut groups: Vec<Group> = vec![Group {
        title: "Start".into(),
        summary: String::new(),
        status: GroupStatus::Done,
        waiting_for_you: false,
        lines: Vec::new(),
    }];
    let mut step = 0;
    for item in items {
        match item {
            Item::Thinking { content, .. } => {
                step += 1;
                groups.push(Group {
                    title: format!("Step {step}"),
                    summary: one_line(content),
                    status: GroupStatus::Done,
                    waiting_for_you: false,
                    lines: Vec::new(),
                });
            }
            Item::Tool {
                name,
                args_preview,
                status,
                ..
            } => {
                let (line, st, you) = tool_line(name, args_preview, *status);
                let g = groups.last_mut().expect("Start exists");
                g.lines.push(line);
                g.status = g.status.min(st);
                g.waiting_for_you |= you;
            }
            Item::Error { text } => {
                let g = groups.last_mut().expect("Start exists");
                g.lines.push(format!("error · {}", one_line(text)));
                g.status = g.status.min(GroupStatus::Failed);
            }
            Item::User { text } => {
                let g = groups.last_mut().expect("Start exists");
                g.lines.push(format!("request · {}", one_line(text)));
            }
            Item::Assistant { text, .. } => {
                let g = groups.last_mut().expect("Start exists");
                g.lines.push(format!("answer · {}", one_line(text)));
            }
            _ => {}
        }
    }
    // A Start group with nothing in it is not shown once steps exist.
    if groups.len() > 1 && groups[0].lines.is_empty() {
        groups.remove(0);
    }
    if let Some(last) = groups.last_mut() {
        if waiting {
            last.status = GroupStatus::Waiting;
            last.waiting_for_you = true;
        } else if live && last.status == GroupStatus::Done {
            last.status = GroupStatus::Running;
        }
    }
    if groups.len() == 1 && groups[0].lines.is_empty() && groups[0].title == "Start" {
        return Vec::new();
    }
    groups
}

/// `Run #3 · 2 h ago · completed` — an automation occurrence's group head.
pub fn run_group_title(index: u64, fired_at: &str, status: &str, now: i64) -> String {
    let when = crate::protocol::parse_rfc3339_utc(fired_at)
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            format!(
                "{} ago",
                crate::automations::compact_duration((now - d.as_secs() as i64).max(0))
            )
        })
        .unwrap_or_default();
    let mut parts = vec![format!("Run #{index}")];
    if !when.is_empty() {
        parts.push(when);
    }
    parts.push(status.to_string());
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_with_the_web_keys() {
        let input = json!({
            "prompt": "Summarise the inbox",
            "provider": "lmstudio", "model": "qwen3",
            "_runtime": {"provider": "lmstudio", "model": "qwen3", "thinking": "high",
                          "allowed_tools": ["read_file", "list_files"],
                          "tool_policy": {"require_approval_tools": ["write_file"]}},
            "tools": ["read_file"],
            "_limits": {"max_iterations": 12},
            "skills": ["pdf"],
            "workspace_access_mode": "workspace_only"
        });
        let s = read_settings(&input);
        assert_eq!(s.provider, "lmstudio");
        assert_eq!(s.reasoning, "high");
        assert_eq!(s.tools, Some(vec!["read_file".to_string()]));
        assert_eq!(s.approval, vec![("write_file".into(), "ask".into())]);
        assert_eq!(s.max_iterations, "12");
        // Unchanged settings = no change at all (no revision for nothing).
        let target = json!({"bundle_ref": "b@1", "flow_id": "f", "input_data": input});
        let same = apply_settings(target.get("input_data").unwrap(), &s);
        assert_eq!(same.get("prompt"), Some(&json!("Summarise the inbox")));
        // Back to the gateway default: the keys are REMOVED, not "".
        let cleared = RunSettings {
            tools: None,
            ..RunSettings::default()
        };
        let ch = settings_changes(&target, &cleared).expect("a change");
        let data = &ch["target"]["input_data"];
        assert!(
            data.get("provider").is_none() && data.get("_runtime").is_none(),
            "{data}"
        );
        assert!(
            data.get("tools").is_none() && data.get("_limits").is_none(),
            "{data}"
        );
        assert_eq!(data["prompt"], json!("Summarise the inbox"));
        assert_eq!(ch["target"]["bundle_ref"], json!("b@1"));
    }

    #[test]
    fn no_change_means_no_revision() {
        let target = json!({"bundle_ref": "b@1", "flow_id": "f", "input_data": {"prompt": "x"}});
        let s = read_settings(&target["input_data"]);
        assert_eq!(settings_changes(&target, &s), None);
    }

    #[test]
    fn save_lines_are_the_web_sentences() {
        assert_eq!(
            save_line(&SaveState::Idle),
            "Changes are saved as a new revision and apply from the next run."
        );
        assert_eq!(
            save_line(&SaveState::Saved(4)),
            "Saved as revision 4; applies from the next run."
        );
    }

    #[test]
    fn activity_groups_per_model_step() {
        let items = vec![
            Item::User {
                text: "fix it".into(),
            },
            Item::Thinking {
                iteration: 1,
                content: "Reading the file".into(),
                reasoning: String::new(),
                call: Default::default(),
            },
            Item::Tool {
                key: "k".into(),
                name: "read_file".into(),
                args_preview: "src/main.rs".into(),
                args_full: String::new(),
                status: ToolStatus::Ok,
                result: String::new(),
                error: String::new(),
            },
            Item::Thinking {
                iteration: 2,
                content: "Writing".into(),
                reasoning: String::new(),
                call: Default::default(),
            },
            Item::Tool {
                key: "k2".into(),
                name: "write_file".into(),
                args_preview: "a.txt".into(),
                args_full: String::new(),
                status: ToolStatus::AwaitingApproval,
                result: String::new(),
                error: String::new(),
            },
        ];
        let g = activity_groups(&items, true, false);
        let titles: Vec<_> = g.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, vec!["Start", "Step 1", "Step 2"]);
        assert_eq!(g[1].status_label(), "Done");
        assert_eq!(g[2].status_label(), "Waiting for you");
        assert_eq!(g[1].lines, vec!["read_file src/main.rs · done"]);
    }
}
