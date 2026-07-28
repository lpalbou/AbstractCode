//! Catalog / discovery parsing — PURE functions over gateway JSON.
//!
//! One home for every shape-tolerant reader of the gateway's catalog and
//! discovery payloads (bundles, providers, tools, skills, MCP servers,
//! models, capability routes). No state, no HTTP, no signals: callers
//! (the TUI worker in `runner`, headless `exec`, the CLI doctor) hand in
//! the parsed `serde_json::Value` and render what comes back. Extracted
//! from `runner.rs` (consolidation survey P2-4, 2026-07-24): headless
//! exec importing its parsers from the TUI worker module was a
//! wrong-direction dependency, and the section was already banner-marked
//! as a separate concern.
//!
//! Parsing discipline (test-pinned): tolerant of BOTH historical wire
//! shapes wherever two exist, blank fields read as absent (never as
//! suppressors), and unknown enum values fail toward the safe reading.

use serde_json::Value;

use crate::store::{McpServer, ProviderInfo, SkillInfo, ToolInfo, Workflow};

pub const AGENT_INTERFACE_V1: &str = "abstractcode.agent.v1";
/// The goal-agent workflow interface (plan item 3). Bundles carrying only
/// this marker stay OUT of `/workflow` — the two catalogs are disjoint by
/// interface, one parser serves both.
pub const GOAL_INTERFACE_V1: &str = "abstractcode.goal.v1";

pub fn agent_workflows_from_bundles(v: &Value) -> Vec<Workflow> {
    workflows_with_interface(v, AGENT_INTERFACE_V1)
}

/// Catalog-declared agent workflow IDS for the fold's answer-source
/// recognition (`Fold::set_agent_workflows`, lane-1 contract): every
/// `GET /bundles` entrypoint carrying `abstractcode.agent.v1` contributes
/// its run-facing `workflow_id` (`{bundle}@{version}:{flow}` — the exact
/// string spawn records cite as `sub_workflow_id`). DEPRECATED
/// entrypoints are deliberately INCLUDED: this set recognizes agent
/// children in ledgers (past runs used now-deprecated entrypoints), it
/// never selects what to run — `workflows_with_interface` keeps the
/// selection filter. Absent/empty `workflow_id` fields contribute
/// nothing (older serializations; the structural id contract covers them).
pub fn agent_workflow_ids_from_bundles(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for b in v
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        for ep in b
            .get("entrypoints")
            .and_then(Value::as_array)
            .unwrap_or(&Vec::new())
        {
            let is_agent = ep
                .get("interfaces")
                .and_then(Value::as_array)
                .map(|ifs| ifs.iter().any(|i| i.as_str() == Some(AGENT_INTERFACE_V1)))
                .unwrap_or(false);
            if !is_agent {
                continue;
            }
            let id = ep
                .get("workflow_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if !id.is_empty() {
                out.push(id.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Bundle entrypoints whose `interfaces[]` carry `interface_id` — the one
/// catalog filter, generalized over the interface parameter (agent.v1 for
/// `/workflow`, goal.v1 for `/goal`) instead of a second parser copy.
pub fn workflows_with_interface(v: &Value, interface_id: &str) -> Vec<Workflow> {
    let mut out = Vec::new();
    let items = match v.get("items").and_then(Value::as_array) {
        Some(i) => i,
        None => return out,
    };
    for b in items {
        let bundle_id = b
            .get("bundle_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if bundle_id.is_empty() {
            continue;
        }
        for ep in b
            .get("entrypoints")
            .and_then(Value::as_array)
            .unwrap_or(&Vec::new())
        {
            let interfaces = ep
                .get("interfaces")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let matches = interfaces.iter().any(|i| i.as_str() == Some(interface_id));
            if !matches {
                continue;
            }
            if ep
                .get("deprecated")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let flow_id = ep
                .get("flow_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if flow_id.is_empty() {
                continue;
            }
            out.push(Workflow {
                bundle_id: bundle_id.to_string(),
                flow_id: flow_id.to_string(),
                name: ep
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(flow_id)
                    .trim()
                    .to_string(),
                description: ep
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            });
        }
    }
    out.sort_by_key(|a| a.label());
    out
}

pub fn choose_workflow(
    workflows: &[Workflow],
    preferred_bundle: Option<&str>,
    preferred_flow: Option<&str>,
) -> Option<Workflow> {
    if let (Some(b), Some(f)) = (preferred_bundle, preferred_flow) {
        if let Some(w) = workflows
            .iter()
            .find(|w| w.bundle_id == b && w.flow_id == f)
        {
            return Some(w.clone());
        }
    }
    if let Some(w) = workflows.iter().find(|w| w.bundle_id == "basic-agent") {
        return Some(w.clone());
    }
    workflows.first().cloned()
}

pub fn providers_from_discovery(v: &Value) -> Vec<ProviderInfo> {
    let mut out = Vec::new();
    let items = v
        .get("providers")
        .and_then(Value::as_array)
        .or_else(|| v.get("items").and_then(Value::as_array));
    for p in items.unwrap_or(&Vec::new()) {
        let name = p
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| p.get("id").and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let mut models = Vec::new();
        if let Some(list) = p.get("models").and_then(Value::as_array) {
            for m in list {
                let id = match m {
                    Value::String(s) => s.trim().to_string(),
                    other => other
                        .get("id")
                        .or_else(|| other.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                };
                if !id.is_empty() {
                    models.push(id);
                }
            }
        }
        out.push(ProviderInfo { name, models });
    }
    out
}

pub fn tools_from_discovery(v: &Value) -> Vec<ToolInfo> {
    let mut out = Vec::new();
    let items = v
        .get("tools")
        .and_then(Value::as_array)
        .or_else(|| v.get("items").and_then(Value::as_array));
    for t in items.unwrap_or(&Vec::new()) {
        let name = t
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let description = t
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        let toolset = ["toolset", "group", "category"]
            .iter()
            .find_map(|k| t.get(*k).and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_string();
        // Server tier/approval (facts UPDATE 2): read-when-present. Absent
        // fields stay None → the #FALLBACK name table classifies. An empty
        // string is treated as absent (a served-but-blank field is no
        // signal). Both states stay valid PERMANENTLY: post-bounce
        // gateways serve the dial on every row, older gateways serve
        // none — render-when-present is the contract, not a transition.
        let str_field = |k: &str| -> Option<String> {
            t.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        // Full-catalog surfacing (tool-tiers item H): disabled rows are
        // stamped `enabled: false` + their gate; ABSENT `enabled` means
        // enabled (only disabled rows carry the field; the post-bounce
        // build also stamps `enabled: true` explicitly on enabled rows).
        let served_disabled = t.get("enabled").and_then(Value::as_bool) == Some(false);
        out.push(ToolInfo {
            name,
            description,
            toolset,
            tier: str_field("tier"),
            // The per-tool approval dial's LIVE spelling is
            // `approval_default` (live-verified 2026-07-23 vs process
            // 11759: auto 12 / ask 38 on all 50 rows, disabled rows
            // clamped to ask server-side — reading only the older
            // `approval` spelling left server truth silently unread and
            // the name-table #FALLBACK classifying everything). Legacy
            // `approval` stays as the fallback spelling for older
            // gateways.
            approval: str_field("approval_default").or_else(|| str_field("approval")),
            // Served band rank (core's observe/act/outreach/destroy =
            // 1..4) — floors the client tier mapping so approval:auto on
            // an outreach row (comms carve-out) never classifies Read
            // (converged contract c5028, finding 3). Absent on older
            // gateways: approval-only mapping applies unchanged.
            risk_rank: t
                .get("risk_rank")
                .and_then(Value::as_u64)
                .and_then(|r| u8::try_from(r).ok()),
            served_disabled,
            enable_gate: str_field("enable_gate").unwrap_or_default(),
            why_disabled: str_field("why_disabled").unwrap_or_default(),
        });
    }
    // Group order first (files/web/system read naturally), name within.
    out.sort_by(|a, b| (&a.toolset, &a.name).cmp(&(&b.toolset, &b.name)));
    out
}

pub fn skills_from_response(v: &Value) -> Vec<SkillInfo> {
    let mut out = Vec::new();
    let items = v
        .get("skills")
        .and_then(Value::as_array)
        .or_else(|| v.get("items").and_then(Value::as_array));
    for s in items.unwrap_or(&Vec::new()) {
        let name = s
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        out.push(SkillInfo {
            name,
            description: s
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            trust: s
                .get("trust_level")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            blocked: s.get("blocked").and_then(Value::as_bool).unwrap_or(false),
        });
    }
    out.sort_by_key(|s| s.name.clone());
    out
}

/// MCP registry -> (servers, note). The note carries the gateway's own
/// empty-state guidance (how to declare a registry) or stays empty.
pub fn mcp_from_response(v: &Value) -> (Vec<McpServer>, String) {
    let mut out = Vec::new();
    for s in v
        .get("servers")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        let name = s
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        out.push(McpServer {
            name,
            url: s
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            description: s
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            auth_required: s
                .get("auth_required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    let note = v
        .get("warnings")
        .and_then(Value::as_array)
        .map(|w| {
            w.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();
    (out, note)
}

/// Model ids from the per-provider models route (`{models: [...]}`; entries
/// are strings, tolerating `{id|name}` objects like the bulk route).
pub fn models_from_provider_models(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for m in v
        .get("models")
        .and_then(Value::as_array)
        .or_else(|| v.get("items").and_then(Value::as_array))
        .unwrap_or(&Vec::new())
    {
        let id = match m {
            Value::String(s) => s.trim().to_string(),
            other => other
                .get("id")
                .or_else(|| other.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
        };
        if !id.is_empty() {
            out.push(id);
        }
    }
    out
}

/// The gateway's default text route: `output.text` first (what generates
/// replies), `input.text` as the fallback — mirroring the server's own
/// `resolve_gateway_provider_model` order.
pub fn default_text_route(v: &Value) -> Option<(String, String)> {
    let routes = v.get("routes").and_then(Value::as_array)?;
    for key in ["output.text", "input.text"] {
        for r in routes {
            if r.get("key").and_then(Value::as_str) != Some(key) {
                continue;
            }
            let provider = r
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let model = r
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if !provider.is_empty() && !model.is_empty() {
                return Some((provider, model));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn agent_workflows_filter_interface_and_deprecated() {
        let v = json!({"items": [
            {"bundle_id": "basic-agent", "entrypoints": [
                {"flow_id": "81795ea9", "name": "basic-agent", "interfaces": ["abstractcode.agent.v1"]}]},
            {"bundle_id": "coding-agent", "entrypoints": [
                {"flow_id": "coding-agent", "interfaces": ["abstractcode.coding.v1"]},
                {"flow_id": "coder", "name": "coder", "interfaces": ["abstractcode.agent.v1"]}]},
            {"bundle_id": "old", "entrypoints": [
                {"flow_id": "x", "interfaces": ["abstractcode.agent.v1"], "deprecated": true}]}
        ]});
        let flows = agent_workflows_from_bundles(&v);
        assert_eq!(flows.len(), 2);
        assert!(flows
            .iter()
            .any(|w| w.bundle_id == "coding-agent" && w.flow_id == "coder"));
    }

    #[test]
    fn interface_parameter_separates_goal_and_agent_catalogs() {
        // One parser, two interfaces (plan item 3): a goal bundle carrying
        // ONLY abstractcode.goal.v1 stays out of /workflow, and the agent
        // list never leaks into /goal. A dual-interface entrypoint appears
        // in both (interfaces are markers, not partitions).
        let v = json!({"items": [
            {"bundle_id": "basic-agent", "entrypoints": [
                {"flow_id": "81795ea9", "name": "basic-agent",
                 "interfaces": ["abstractcode.agent.v1"]}]},
            {"bundle_id": "goal-agent", "entrypoints": [
                {"flow_id": "goal-loop", "name": "goal-loop",
                 "interfaces": ["abstractcode.goal.v1"]}]},
            {"bundle_id": "dual", "entrypoints": [
                {"flow_id": "both", "name": "both",
                 "interfaces": ["abstractcode.agent.v1", "abstractcode.goal.v1"]}]},
            {"bundle_id": "dead-goal", "entrypoints": [
                {"flow_id": "x", "interfaces": ["abstractcode.goal.v1"],
                 "deprecated": true}]}
        ]});
        let agents = workflows_with_interface(&v, AGENT_INTERFACE_V1);
        let goals = workflows_with_interface(&v, GOAL_INTERFACE_V1);
        assert_eq!(
            agents
                .iter()
                .map(|w| w.bundle_id.as_str())
                .collect::<Vec<_>>(),
            vec!["basic-agent", "dual"],
            "goal-only bundles stay out of /workflow"
        );
        assert_eq!(
            goals
                .iter()
                .map(|w| w.bundle_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dual", "goal-agent"],
            "deprecated goal entrypoints drop; agent-only bundles stay out"
        );
    }

    #[test]
    fn agent_workflow_ids_collect_run_facing_ids_including_deprecated() {
        // Lane-1 fold contract wiring: the ids are the entrypoints' own
        // `workflow_id` fields (live shape verified 2026-07-23:
        // "basic-agent@0.0.3:81795ea9"), agent.v1 only, deprecated KEPT
        // (recognition of past ledgers, not selection), absent fields
        // skipped, deduped + sorted.
        let v = json!({"items": [
            {"bundle_id": "basic-agent", "entrypoints": [
                {"flow_id": "81795ea9", "workflow_id": "basic-agent@0.0.3:81795ea9",
                 "interfaces": ["abstractcode.agent.v1"]}]},
            {"bundle_id": "old-agent", "entrypoints": [
                {"flow_id": "x", "workflow_id": "old-agent@0.0.1:x",
                 "interfaces": ["abstractcode.agent.v1"], "deprecated": true}]},
            {"bundle_id": "not-an-agent", "entrypoints": [
                {"flow_id": "y", "workflow_id": "not-an-agent@1:y",
                 "interfaces": ["abstractreview.adversarial.v1"]}]},
            {"bundle_id": "pre-id-era", "entrypoints": [
                {"flow_id": "z", "interfaces": ["abstractcode.agent.v1"]}]},
            {"bundle_id": "dup", "entrypoints": [
                {"flow_id": "a", "workflow_id": "dup@1:a",
                 "interfaces": ["abstractcode.agent.v1"]},
                {"flow_id": "a", "workflow_id": "dup@1:a",
                 "interfaces": ["abstractcode.agent.v1"]}]}
        ]});
        assert_eq!(
            agent_workflow_ids_from_bundles(&v),
            vec![
                "basic-agent@0.0.3:81795ea9".to_string(),
                "dup@1:a".to_string(),
                "old-agent@0.0.1:x".to_string(),
            ]
        );
        assert!(agent_workflow_ids_from_bundles(&json!({"items": []})).is_empty());
    }

    #[test]
    fn workflow_choice_prefers_saved_then_basic_agent() {
        let flows = vec![
            Workflow {
                bundle_id: "coding-agent".into(),
                flow_id: "coder".into(),
                name: "coder".into(),
                description: String::new(),
            },
            Workflow {
                bundle_id: "basic-agent".into(),
                flow_id: "81795ea9".into(),
                name: "basic-agent".into(),
                description: String::new(),
            },
        ];
        let picked = choose_workflow(&flows, Some("coding-agent"), Some("coder")).unwrap();
        assert_eq!(picked.bundle_id, "coding-agent");
        let fallback = choose_workflow(&flows, None, None).unwrap();
        assert_eq!(fallback.bundle_id, "basic-agent");
        let missing_pref = choose_workflow(&flows, Some("gone"), Some("x")).unwrap();
        assert_eq!(missing_pref.bundle_id, "basic-agent");
    }

    #[test]
    fn provider_parsing_accepts_both_shapes() {
        let v = json!({"providers": [
            {"name": "lmstudio", "models": ["qwen3-4b", {"id": "gpt-oss-120b"}]},
            {"id": "ollama", "models": []}
        ]});
        let providers = providers_from_discovery(&v);
        assert_eq!(providers.len(), 2);
        assert_eq!(
            providers[0].models,
            vec!["qwen3-4b".to_string(), "gpt-oss-120b".to_string()]
        );
        assert_eq!(providers[1].name, "ollama");
    }

    #[test]
    fn tool_parsing_takes_first_description_line() {
        let v = json!({"tools": [
            {"name": "write_file", "description": "Write a file.\nLong details.", "toolset": "files"},
            {"name": "read_file"}
        ]});
        let tools = tools_from_discovery(&v);
        // Ungrouped ("") sorts before "files".
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[1].name, "write_file");
        assert_eq!(tools[1].description, "Write a file.");
        assert_eq!(tools[1].toolset, "files");
        // Dial-less shape (older gateways): no tier/approval fields.
        assert!(tools[1].tier.is_none() && tools[1].approval.is_none());
    }

    #[test]
    fn tool_parsing_reads_server_tier_and_approval_when_present() {
        // Post-bounce shape (live sample from the gateway receipt):
        // execute_command tier2_world/ask, read_file tier2_world/auto.
        // The LIVE spelling is `approval_default` (process 11759);
        // legacy `approval` still parses for older gateways, and the
        // new spelling wins when both are present.
        let v = json!({"tools": [
            {"name": "execute_command", "toolset": "system",
             "tier": "tier2_world", "approval_default": "ask"},
            {"name": "read_file", "toolset": "files",
             "tier": "tier2_world", "approval_default": "auto"},
            // Legacy spelling alone still reads.
            {"name": "list_files", "toolset": "files", "approval": "auto"},
            // Both present: the live spelling wins.
            {"name": "web_search", "toolset": "web",
             "approval_default": "ask", "approval": "auto"},
            // A served-but-blank field is treated as absent (no signal).
            {"name": "write_file", "toolset": "files", "tier": "", "approval_default": "  "},
            // Blank live spelling + present legacy: blank is NO SIGNAL
            // (the documented convention), so the legacy value reads —
            // blank must not act as a suppressor.
            {"name": "edit_file", "toolset": "files",
             "approval_default": " ", "approval": "ask"}
        ]});
        let tools = tools_from_discovery(&v);
        let by = |n: &str| tools.iter().find(|t| t.name == n).unwrap();
        assert_eq!(by("execute_command").approval.as_deref(), Some("ask"));
        assert_eq!(by("execute_command").tier.as_deref(), Some("tier2_world"));
        assert_eq!(by("read_file").approval.as_deref(), Some("auto"));
        assert_eq!(by("list_files").approval.as_deref(), Some("auto"));
        assert_eq!(by("web_search").approval.as_deref(), Some("ask"));
        assert!(by("write_file").tier.is_none() && by("write_file").approval.is_none());
        assert_eq!(
            by("edit_file").approval.as_deref(),
            Some("ask"),
            "blank live spelling falls through to the legacy value"
        );
    }

    #[test]
    fn tool_parsing_reads_served_disabled_rows_with_their_gate() {
        // Full-catalog surfacing shape (tool-tiers item H): disabled rows
        // are stamped `enabled: false` + gate + reason; ABSENT `enabled`
        // means enabled (only disabled rows carry the field).
        let v = json!({"tools": [
            {"name": "read_file", "toolset": "files"},
            {"name": "send_email", "toolset": "comms", "enabled": false,
             "enable_gate": "ABSTRACT_ENABLE_COMMS_TOOLS",
             "why_disabled": "registered but disabled on this gateway"},
            // Defensive: an explicit `enabled: true` is also "enabled".
            {"name": "write_file", "toolset": "files", "enabled": true}
        ]});
        let tools = tools_from_discovery(&v);
        let by = |n: &str| tools.iter().find(|t| t.name == n).unwrap();
        assert!(!by("read_file").served_disabled, "absent enabled = enabled");
        assert!(!by("write_file").served_disabled, "true = enabled");
        let email = by("send_email");
        assert!(email.served_disabled);
        assert_eq!(email.enable_gate, "ABSTRACT_ENABLE_COMMS_TOOLS");
        assert_eq!(
            email.why_disabled,
            "registered but disabled on this gateway"
        );
    }

    #[test]
    fn skills_parsing_carries_trust_and_blocked() {
        let v = json!({"skills": [
            {"name": "coredoc", "description": "Docs.\nMore.", "trust_level": "adopted"},
            {"name": "sketchy", "trust_level": "unknown", "blocked": true},
            {"name": ""}
        ]});
        let skills = skills_from_response(&v);
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "coredoc");
        assert_eq!(skills[0].trust, "adopted");
        assert!(!skills[0].blocked);
        assert!(skills[1].blocked);
    }

    #[test]
    fn mcp_parsing_keeps_servers_and_empty_state_note() {
        let (servers, note) = mcp_from_response(&json!({
            "servers": [{"name": "kb", "url": "http://x/mcp", "auth_required": true}],
            "warnings": []
        }));
        assert_eq!(servers.len(), 1);
        assert!(servers[0].auth_required);
        assert!(note.is_empty());
        let (none, hint) = mcp_from_response(&json!({
            "servers": [],
            "warnings": ["no MCP server registry declared (create mcp_servers.json)"]
        }));
        assert!(none.is_empty());
        assert!(hint.contains("mcp_servers.json"));
    }

    #[test]
    fn default_route_prefers_output_text_then_input_text() {
        let v = json!({"routes": [
            {"key": "input.text", "provider": "lmstudio", "model": "ornith-1.0-35b"},
            {"key": "output.image.text_to_image", "provider": "mlx-gen", "model": "flux"}
        ]});
        assert_eq!(
            default_text_route(&v),
            Some(("lmstudio".into(), "ornith-1.0-35b".into()))
        );
        let with_out = json!({"routes": [
            {"key": "output.text", "provider": "openai", "model": "gpt-5.2"},
            {"key": "input.text", "provider": "lmstudio", "model": "ornith-1.0-35b"}
        ]});
        assert_eq!(
            default_text_route(&with_out),
            Some(("openai".into(), "gpt-5.2".into()))
        );
        assert_eq!(default_text_route(&json!({"routes": []})), None);
    }
}
