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

use crate::store::{
    HostContracts, HostFacts, McpServer, ProviderInfo, ResidencyRow, SessionCacheRow, SkillInfo,
    ToolInfo, Workflow,
};

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

/// EVERY catalogued entrypoint as `(bundle_id, flow_id, interfaces)` — the
/// DIAGNOSTIC view, no interface filter and deprecated rows included.
///
/// Selection never uses this (`workflows_with_interface` owns that): it
/// exists so a refusal can tell the truth about WHY a ref was rejected. A
/// flow that is installed but carries, say, `abstractcode.coding.v1` used to
/// be reported as "not found on this gateway", which sent the operator
/// hunting for a missing bundle that was in fact sitting right there behind
/// a different interface.
pub fn all_entrypoints_from_bundles(v: &Value) -> Vec<(String, String, Vec<String>)> {
    let mut out = Vec::new();
    for b in v
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
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
            let flow_id = ep
                .get("flow_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if flow_id.is_empty() {
                continue;
            }
            let interfaces = ep
                .get("interfaces")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            out.push((bundle_id.to_string(), flow_id.to_string(), interfaces));
        }
    }
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

/// Flows belonging to `bundle_id` — the bundle-only resolution set.
pub fn flows_in_bundle<'a>(workflows: &'a [Workflow], bundle_id: &str) -> Vec<&'a Workflow> {
    workflows
        .iter()
        .filter(|w| w.bundle_id == bundle_id)
        .collect()
}

/// Resolve a BUNDLE-ONLY reference (`--workflow react-agent`, a saved pref
/// carrying no flow) to one flow inside that bundle.
///
/// Determinism first (the operator's orchestration contract): a bundle with
/// exactly one agent flow resolves to it; a multi-flow bundle resolves ONLY
/// on an unambiguous name match (`flow_id` or `name` equal to the bundle id
/// — the conventional "chat entry" spelling). Anything else returns `None`
/// so the caller refuses and names the choices, rather than picking a flow
/// the operator did not ask for.
///
/// Before this existed, `choose_workflow` required BOTH halves and a
/// bundle-only ref fell through to the basic-agent fallback — so
/// `--workflow react-agent` (a bundle that IS installed) resolved to
/// basic-agent, which headless `exec` then refused as "not found on this
/// gateway", while `--workflow basic-agent` worked only by coinciding with
/// the fallback. Verified live 2026-07-30 against the running gateway.
pub fn resolve_bundle_only(workflows: &[Workflow], bundle_id: &str) -> Option<Workflow> {
    let in_bundle = flows_in_bundle(workflows, bundle_id);
    match in_bundle.len() {
        0 => None,
        1 => Some(in_bundle[0].clone()),
        _ => in_bundle
            .iter()
            .find(|w| w.flow_id == bundle_id || w.name == bundle_id)
            .map(|w| (*w).clone()),
    }
}

/// Resolve the workflow to run: exact `bundle:flow` > bundle-only >
/// coding-agent:coder (the benchmark-verified default) > basic-agent >
/// first agent flow.
///
/// The trailing fallbacks serve the PREFS lane (a stale saved preference
/// degrading to the default is the interactive contract). Callers acting on
/// an EXPLICIT request must re-check the result — see
/// `exec::explicit_workflow_mismatch` — because a fallback silently running
/// a different agent breaks deterministic orchestration.
/// Whether a bundle's loop has review nodes at all.
///
/// memact does not: `MemActAgent` deprecation-warns on `review_mode` /
/// `review_max_rounds` (`abstractagent/agents/memact.py:88-96`) and the Python
/// client withholds them for that agent kind (`react_shell.py:775-779`).
/// Match that rather than making the server warn about a posture it ignores.
pub fn workflow_is_review_capable(bundle_id: &str) -> bool {
    !bundle_id.starts_with("memact")
}

/// The gateway's OWN default entrypoint, when it marks one.
///
/// Operator ruling 2026-08-21: *"there is the default set by the gateway, but
/// it can be overridden on new turns by a client. During a turn, a
/// workflow/agent can't be changed."* Which agent runs is the single largest
/// determinant of what a run means, so it must not be a client ruling: this
/// TUI picking `coding-agent:coder` from its own benchmark while a web build
/// or a bridge picks "the first agent flow" gives one durable session two
/// different loops across turns.
///
/// Read from where the gateway already serves it: `bundles[].is_default`
/// (the platform default) narrowed by `bundles[].default_entrypoint` (that
/// bundle's default flow). Today no bundle carries `is_default` on this
/// gateway, so this returns `None` and `choose_workflow` degrades to its
/// labelled client fallback — the seam is here for the day the catalog marks
/// one, and no client edit is needed then.
pub fn served_default_workflow(v: &Value, interface_id: &str) -> Option<(String, String)> {
    // Same array the interface filter walks (`items`); `bundles` tolerated
    // because the console renders that spelling.
    let bundles = v
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| v.get("bundles").and_then(Value::as_array))?;
    for b in bundles {
        if !b
            .get("is_default")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let bundle_id = b.get("bundle_id").and_then(Value::as_str)?.trim();
        let entry = b
            .get("default_entrypoint")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if bundle_id.is_empty() || entry.is_empty() {
            continue;
        }
        // Only when that entrypoint really declares the interface we run.
        let declares = b
            .get("entrypoints")
            .and_then(Value::as_array)
            .map(|eps| {
                eps.iter().any(|ep| {
                    ep.get("flow_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        == entry
                        && ep
                            .get("interfaces")
                            .and_then(Value::as_array)
                            .map(|ifs| {
                                ifs.iter().any(|i| {
                                    i.as_str()
                                        .map(|x| x.trim() == interface_id)
                                        .unwrap_or(false)
                                })
                            })
                            .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if declares {
            return Some((bundle_id.to_string(), entry.to_string()));
        }
    }
    None
}

/// `choose_workflow` with the gateway's served default consulted before the
/// client's own fallback. Pass the raw bundles payload; everything else is
/// unchanged, so an operator preference still wins over both.
pub fn choose_workflow_with_served_default(
    workflows: &[Workflow],
    preferred_bundle: Option<&str>,
    preferred_flow: Option<&str>,
    bundles: &Value,
    interface_id: &str,
) -> Option<Workflow> {
    if preferred_bundle.is_none() {
        if let Some((b, f)) = served_default_workflow(bundles, interface_id) {
            if let Some(w) = workflows
                .iter()
                .find(|w| w.bundle_id == b && w.flow_id == f)
            {
                return Some(w.clone());
            }
        }
    }
    choose_workflow(workflows, preferred_bundle, preferred_flow)
}

pub fn choose_workflow(
    workflows: &[Workflow],
    preferred_bundle: Option<&str>,
    preferred_flow: Option<&str>,
) -> Option<Workflow> {
    if let Some(b) = preferred_bundle {
        match preferred_flow {
            Some(f) => {
                if let Some(w) = workflows
                    .iter()
                    .find(|w| w.bundle_id == b && w.flow_id == f)
                {
                    return Some(w.clone());
                }
            }
            // Bundle-only: resolve WITHIN the bundle before any fallback.
            None => {
                if let Some(w) = resolve_bundle_only(workflows, b) {
                    return Some(w);
                }
            }
        }
    }
    // #FALLBACK — no bundle is marked default on this gateway. Below is a
    // CLIENT ruling (2026-08-01, benchmark-backed): the verified coding
    // workflow. Across a ~70-run campaign, `coding-agent:coder` had the
    // highest quality floor of every loop design (0.795; the only heavy arm
    // with no sub-0.6 run in any era) and the best calls-to-artifact ratio —
    // builder + independent verifier + deterministic gates. It is a client
    // ruling, which means another client picks differently for the same
    // session: the benchmark is a fact about SERVER-installed bundles and
    // belongs in the catalog as `is_default`. Filed in the conformance audit.
    if let Some(w) = workflows
        .iter()
        .find(|w| w.bundle_id == "coding-agent" && w.flow_id == "coder")
    {
        return Some(w.clone());
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

// ---------------------------------------------------------------------------
// Host state + capability contracts (`/resources`)
// ---------------------------------------------------------------------------
//
// Honesty contract (mirrors `gateway::gpu::meter_from_response`): an
// absent or non-numeric field reads as UNKNOWN (`None`/empty), never as
// a fabricated zero; `resident` is tri-state by wire contract (null =
// the gateway does not know).

/// `/discovery/capabilities` → which of the host/resource contracts this
/// gateway declares (`contracts.common.{model_residency,host_state,
/// session_caches}`) plus the modality display labels
/// (`model_residency.modality_ui.colors` — hex colors dropped: theme
/// inks only, modality distinguished by label TEXT).
pub fn contracts_from_capabilities(v: &Value) -> HostContracts {
    // The live endpoint wraps everything in a `capabilities` envelope
    // (`{"capabilities": {"contracts": {"common": ...}}}`); tolerate the
    // unwrapped shape too so a fixture or proxy that strips the envelope
    // still parses.
    let root = v.get("capabilities").filter(|c| c.is_object()).unwrap_or(v);
    let common = root
        .get("contracts")
        .and_then(|c| c.get("common"))
        .cloned()
        .unwrap_or(Value::Null);
    let has = |key: &str| common.get(key).is_some_and(Value::is_object);
    let mut modality_labels: Vec<(String, String)> = Vec::new();
    if let Some(colors) = common
        .get("model_residency")
        .and_then(|m| m.get("modality_ui"))
        .and_then(|u| u.get("colors"))
        .and_then(Value::as_object)
    {
        for (task, entry) in colors {
            let label = entry
                .get("label")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .unwrap_or(task)
                .to_string();
            modality_labels.push((task.clone(), label));
        }
        modality_labels.sort();
    }
    HostContracts {
        model_residency: has("model_residency"),
        host_state: has("host_state"),
        session_caches: has("session_caches"),
        modality_labels,
    }
}

/// Read a string field tolerantly ("" when absent/blank).
fn str_of(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Read a numeric field as `Option<u64>` — junk (negative, string,
/// float-NaN) reads as UNKNOWN, never as zero.
fn u64_of(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

/// Epoch seconds → a UTC "HH:MM:SS" clock string (the same clock the GPU
/// lane's ISO timestamps yield through `short_ts`). Callers guard
/// finiteness/positivity; this only does the modular arithmetic.
fn clock_from_epoch(secs: f64) -> String {
    let s = secs as u64 % 86_400;
    format!("{:02}:{:02}:{:02}", s / 3_600, (s % 3_600) / 60, s % 60)
}

/// `models[]` (row_v1) from a `/host/state` response. Tolerates an
/// `items` spelling; rows without both provider and model are dropped
/// (nothing actionable to render or unload).
pub fn residency_rows_v1(v: &Value) -> Vec<ResidencyRow> {
    let mut out = Vec::new();
    let items = v
        .get("models")
        .and_then(Value::as_array)
        .or_else(|| v.get("items").and_then(Value::as_array));
    for m in items.unwrap_or(&Vec::new()) {
        let provider = str_of(m, "provider");
        let model = str_of(m, "model");
        if provider.is_empty() && model.is_empty() {
            continue;
        }
        out.push(ResidencyRow {
            runtime_id: str_of(m, "runtime_id"),
            task: str_of(m, "task"),
            provider,
            model,
            source: str_of(m, "source"),
            // TRI-STATE: null/absent = unknown — the view renders it
            // distinct; folding to `false` here would fabricate a "no".
            resident: m.get("resident").and_then(Value::as_bool),
            state: str_of(m, "state"),
            locked: m.get("locked").and_then(Value::as_bool).unwrap_or(false),
            // TRI-STATE like `resident`: null/absent = the gateway did
            // not say. Folding it to `false` here would refuse the lock
            // on exactly the rows lock now ADOPTS (sweep/externally
            // loaded models arrive with a null `lockable`).
            lockable: m.get("lockable").and_then(Value::as_bool),
            modalities: m
                .get("modalities")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            size_bytes: u64_of(m, "size_bytes"),
            size_vram_bytes: u64_of(m, "size_vram_bytes"),
            // Additive row_v1 fields — nullable by contract, so absence
            // stays absence (`display_size` coalesces, marking the
            // estimate; `cache_bytes` renders as its own second figure).
            est_weights_bytes: u64_of(m, "est_weights_bytes"),
            cache_bytes: u64_of(m, "cache_bytes"),
            context_length: u64_of(m, "context_length"),
            calibrated_context_length: u64_of(m, "calibrated_context_length"),
            context_calibrated: m
                .get("context_calibrated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            is_default: m.get("default").and_then(Value::as_bool).unwrap_or(false),
            loaded_at: str_of(m, "loaded_at"),
            last_used_at: str_of(m, "last_used_at"),
        });
    }
    out
}

/// Fold one `/host/state` response into [`HostFacts`]. Pure and
/// tolerant: every section is optional; a missing section means its
/// rows/numbers simply do not exist (rendered as omission).
pub fn host_state_from_response(v: &Value) -> HostFacts {
    let memory = v.get("memory").cloned().unwrap_or(Value::Null);
    let ram = memory.get("ram").cloned().unwrap_or(Value::Null);
    let device = memory.get("device").cloned().unwrap_or(Value::Null);
    let gpu = v.get("gpu").cloned().unwrap_or(Value::Null);
    let mut caches = Vec::new();
    for c in v
        .get("session_caches")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        caches.push(SessionCacheRow {
            key: str_of(c, "key"),
            provider: str_of(c, "provider"),
            model: str_of(c, "model"),
            session_id: str_of(c, "session_id"),
            bytes: u64_of(c, "bytes"),
            token_count: u64_of(c, "token_count"),
        });
    }
    let mut totals: Vec<(String, u64)> = v
        .get("totals")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, val)| val.as_u64().map(|n| (k.clone(), n)))
                .collect()
        })
        .unwrap_or_default();
    totals.sort();
    // Degraded lanes carry their reason when `reasons{}` names one.
    let reasons = v.get("reasons").cloned().unwrap_or(Value::Null);
    let degraded: Vec<String> = v
        .get("degraded")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(
                    |lane| match reasons.get(lane).and_then(Value::as_str).map(str::trim) {
                        Some(why) if !why.is_empty() => format!("{lane}: {why}"),
                        _ => lane.to_string(),
                    },
                )
                .collect()
        })
        .unwrap_or_default();
    let gpu_supported = gpu
        .get("supported")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    HostFacts {
        // The live route serves `ts` as an epoch FLOAT (`time.time()`);
        // an ISO string is tolerated for older/other emitters. A float is
        // pre-formatted to a UTC clock here so the modal's `short_ts`
        // (which extracts HH:MM:SS from ISO strings and passes anything
        // else through) renders it unchanged.
        ts: match v.get("ts") {
            Some(Value::Number(n)) => n
                .as_f64()
                .filter(|s| s.is_finite() && *s > 0.0)
                .map(clock_from_epoch)
                .unwrap_or_default(),
            _ => str_of(v, "ts"),
        },
        host_name: memory
            .get("host")
            .map(|h| str_of(h, "host_name"))
            .unwrap_or_default(),
        ram_total: u64_of(&ram, "total_bytes"),
        ram_used: u64_of(&ram, "used_bytes"),
        ram_available: u64_of(&ram, "available_bytes"),
        // Range-checked: a percent is only a percent inside 0..=100 —
        // junk (250, NaN, negatives) reads as UNKNOWN, so the footer and
        // the modal bar can never disagree over a clamped fabrication.
        ram_percent: ram
            .get("percent")
            .and_then(Value::as_f64)
            .filter(|p| p.is_finite() && (0.0..=100.0).contains(p)),
        process_rss: memory
            .get("process")
            .and_then(|p| p.get("rss_bytes"))
            .and_then(Value::as_u64),
        device_backend: device
            .get("backend")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
        device_allocated: u64_of(&device, "allocated_bytes"),
        device_total: u64_of(&device, "total_bytes"),
        device_free: u64_of(&device, "free_bytes"),
        device_host_in_use: u64_of(&device, "host_in_use_bytes"),
        device_wired_limit: u64_of(&device, "wired_limit_bytes"),
        gpu_supported,
        // A number is only a number when the host SUPPORTS the meter —
        // `supported:false` with a stray field must not resurrect it.
        gpu_util_pct: if gpu_supported {
            gpu.get("utilization_gpu_pct").and_then(Value::as_f64)
        } else {
            None
        },
        models: residency_rows_v1(v),
        caches,
        totals,
        degraded,
    }
}

/// One display line for a `/models/context_estimate` answer:
/// `confidence · predicted max context N · notes`. Unknowns are omitted,
/// never zero-filled.
pub fn context_estimate_line(v: &Value) -> String {
    // An in-band failure (`{ok:false, error:...}`, e.g. facade
    // unavailable) carries no confidence — surface the served error
    // instead of a bare "unknown" that reads like an estimator verdict.
    if v.get("ok").and_then(Value::as_bool) == Some(false) {
        let why = str_of(v, "error");
        if !why.is_empty() {
            return format!("estimate unavailable: {why}");
        }
    }
    let confidence = v
        .get("confidence")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .trim()
        .to_string();
    let mut parts = vec![confidence];
    if let Some(n) = v.get("predicted_max_context").and_then(Value::as_u64) {
        parts.push(format!("predicted max context {n}"));
    }
    let notes: Vec<&str> = v
        .get("notes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if !notes.is_empty() {
        parts.push(notes.join("; "));
    }
    parts.join(" · ")
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

    /// Which agent runs is the largest determinant of what a run means, so
    /// the GATEWAY decides it: "there is the default set by the gateway, but
    /// it can be overridden on new turns by a client" (operator, 2026-08-21).
    /// A client ruling here gives one durable session two different loops
    /// depending on which app opened the next turn.
    #[test]
    fn the_gateways_served_default_outranks_the_clients_own_pick() {
        let bundles = json!({"items": [
            {"bundle_id": "coding-agent", "default_entrypoint": "coder", "entrypoints": [
                {"flow_id": "coder", "name": "coder", "interfaces": [AGENT_INTERFACE_V1]}]},
            {"bundle_id": "house-agent", "is_default": true, "default_entrypoint": "house",
             "entrypoints": [{"flow_id": "house", "name": "house", "interfaces": [AGENT_INTERFACE_V1]}]}
        ]});
        let flows = agent_workflows_from_bundles(&bundles);

        assert_eq!(
            served_default_workflow(&bundles, AGENT_INTERFACE_V1),
            Some(("house-agent".to_string(), "house".to_string()))
        );
        let picked =
            choose_workflow_with_served_default(&flows, None, None, &bundles, AGENT_INTERFACE_V1)
                .unwrap();
        assert_eq!(picked.bundle_id, "house-agent", "the server's default wins");

        // An operator preference is an override and still wins over both.
        let overridden = choose_workflow_with_served_default(
            &flows,
            Some("coding-agent"),
            Some("coder"),
            &bundles,
            AGENT_INTERFACE_V1,
        )
        .unwrap();
        assert_eq!(overridden.bundle_id, "coding-agent");

        // No bundle marked default (today's gateway): unchanged behaviour,
        // and the client fallback is reached exactly as before.
        let unmarked = json!({"items": [
            {"bundle_id": "coding-agent", "default_entrypoint": "coder", "entrypoints": [
                {"flow_id": "coder", "name": "coder", "interfaces": [AGENT_INTERFACE_V1]}]}
        ]});
        assert_eq!(served_default_workflow(&unmarked, AGENT_INTERFACE_V1), None);
        let flows2 = agent_workflows_from_bundles(&unmarked);
        assert_eq!(
            choose_workflow_with_served_default(&flows2, None, None, &unmarked, AGENT_INTERFACE_V1)
                .unwrap()
                .bundle_id,
            "coding-agent"
        );

        // A default whose entrypoint does not declare our interface is not
        // ours to run.
        let wrong_iface = json!({"items": [
            {"bundle_id": "other", "is_default": true, "default_entrypoint": "x",
             "entrypoints": [{"flow_id": "x", "name": "x", "interfaces": ["something.else.v1"]}]}
        ]});
        assert_eq!(
            served_default_workflow(&wrong_iface, AGENT_INTERFACE_V1),
            None
        );
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
        // Default ruling 2026-08-01: coder is the default when installed
        // (highest quality floor of the loop-design benchmark); basic-agent
        // is the fallback when it is not.
        let fallback = choose_workflow(&flows, None, None).unwrap();
        assert_eq!(fallback.bundle_id, "coding-agent");
        let missing_pref = choose_workflow(&flows, Some("gone"), Some("x")).unwrap();
        assert_eq!(missing_pref.bundle_id, "coding-agent");
        let no_coder: Vec<Workflow> = flows
            .iter()
            .filter(|w| w.bundle_id != "coding-agent")
            .cloned()
            .collect();
        let basic = choose_workflow(&no_coder, None, None).unwrap();
        assert_eq!(basic.bundle_id, "basic-agent", "fallback without coder");
    }

    /// A BUNDLE-ONLY reference resolves inside its own bundle — it must never
    /// fall through to the basic-agent default.
    ///
    /// Live regression (2026-07-30, gateway 127.0.0.1:8080): `--workflow
    /// react-agent` exited 2 with "not found on this gateway" while
    /// `react-agent:react` ran GREEN, because the old resolver required BOTH
    /// halves and a bundle-only ref landed on basic-agent, which headless
    /// `exec` then correctly refused. `--workflow basic-agent` appeared to
    /// work only because it coincided with the fallback.
    #[test]
    fn bundle_only_reference_resolves_within_its_bundle() {
        let flows = vec![
            Workflow {
                bundle_id: "react-agent".into(),
                flow_id: "react".into(),
                name: "react".into(),
                description: String::new(),
            },
            Workflow {
                bundle_id: "multiagent-coding".into(),
                flow_id: "multiagent-coder".into(),
                name: "Multi-agent coder — chat entry".into(),
                description: String::new(),
            },
            Workflow {
                bundle_id: "basic-agent".into(),
                flow_id: "81795ea9".into(),
                name: "basic-agent".into(),
                description: String::new(),
            },
        ];
        // The single agent flow in the bundle is the evident intent.
        let react = choose_workflow(&flows, Some("react-agent"), None).unwrap();
        assert_eq!(
            (react.bundle_id.as_str(), react.flow_id.as_str()),
            ("react-agent", "react")
        );
        // Including when the flow id is nothing like the bundle id.
        let mc = choose_workflow(&flows, Some("multiagent-coding"), None).unwrap();
        assert_eq!(mc.flow_id, "multiagent-coder");
        // A bundle that genuinely is not installed still degrades (the prefs
        // lane contract); explicit callers re-check via
        // `exec::explicit_workflow_mismatch`.
        assert_eq!(
            choose_workflow(&flows, Some("gone"), None)
                .unwrap()
                .bundle_id,
            "basic-agent"
        );
    }

    /// A multi-flow bundle resolves ONLY on an unambiguous name match; an
    /// ambiguous bundle-only ref returns None so the caller can refuse and
    /// list the choices. Picking one silently would run an agent the
    /// operator did not name — the orchestration-determinism failure.
    #[test]
    fn ambiguous_bundle_only_reference_refuses_to_guess() {
        let two = vec![
            Workflow {
                bundle_id: "dual".into(),
                flow_id: "alpha".into(),
                name: "alpha".into(),
                description: String::new(),
            },
            Workflow {
                bundle_id: "dual".into(),
                flow_id: "beta".into(),
                name: "beta".into(),
                description: String::new(),
            },
        ];
        assert!(
            resolve_bundle_only(&two, "dual").is_none(),
            "two candidates, no name match → the caller must ask"
        );
        // A flow named after its bundle IS the evident chat entry.
        let mut named = two.clone();
        named.push(Workflow {
            bundle_id: "dual".into(),
            flow_id: "dual".into(),
            name: "dual".into(),
            description: String::new(),
        });
        assert_eq!(resolve_bundle_only(&named, "dual").unwrap().flow_id, "dual");
        assert!(resolve_bundle_only(&two, "absent").is_none());
    }

    /// The diagnostic view reports EVERY entrypoint with its interfaces —
    /// including the `abstractcode.coding.v1` pipelines that the agent-only
    /// selection filter hides, so a refusal can say "installed, different
    /// interface" instead of the false "not found on this gateway".
    #[test]
    fn diagnostic_catalog_reports_every_interface() {
        let v = json!({"items": [
            {"bundle_id": "multiagent-coding", "entrypoints": [
                {"flow_id": "multiagent-coder", "interfaces": ["abstractcode.agent.v1"]},
                {"flow_id": "multiagent-coding", "interfaces": ["abstractcode.coding.v1"]}]},
            {"bundle_id": "coder", "entrypoints": [{"flow_id": "b4c6f107", "interfaces": []}]},
            {"bundle_id": "", "entrypoints": [{"flow_id": "x", "interfaces": []}]}
        ]});
        let all = all_entrypoints_from_bundles(&v);
        assert!(all.contains(&(
            "multiagent-coding".to_string(),
            "multiagent-coding".to_string(),
            vec!["abstractcode.coding.v1".to_string()]
        )));
        // Interface-less rows are still VISIBLE to diagnosis (they are the
        // hardest refusals to explain), blank bundle ids are not.
        assert!(all
            .iter()
            .any(|(b, f, i)| b == "coder" && f == "b4c6f107" && i.is_empty()));
        assert!(all.iter().all(|(b, _, _)| !b.is_empty()));
        // Selection stays agent-only — diagnosis must not widen it.
        let selectable = agent_workflows_from_bundles(&v);
        assert_eq!(selectable.len(), 1);
        assert_eq!(selectable[0].flow_id, "multiagent-coder");
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

    // --- host state + capability contracts (`/resources`) ---------------

    #[test]
    fn capability_contracts_read_presence_and_modality_labels() {
        let v = json!({"contracts": {"common": {
            "model_residency": {
                "endpoints": {"loaded": "/models/loaded", "unload": "/models/unload"},
                "row_schema": "row_v1",
                "modality_ui": {"version": 1, "colors": {
                    "text-generation": {"color": "#7aa2f7", "label": "LLM"},
                    "image-generation": {"color": "#f7768e", "label": "IMG"},
                    "label-less": {"color": "#000000"}
                }}
            },
            "host_state": {"endpoints": {"state": "/host/state"}},
            "session_caches": {"endpoints": {"list": "/x", "clear_all": "/y"}}
        }}});
        let c = contracts_from_capabilities(&v);
        assert!(c.model_residency && c.host_state && c.session_caches);
        assert_eq!(c.label_for("text-generation"), "LLM");
        assert_eq!(c.label_for("image-generation"), "IMG");
        // A label-less color entry falls back to the task id; unknown
        // tasks read as themselves (never invented).
        assert_eq!(c.label_for("label-less"), "label-less");
        assert_eq!(c.label_for("audio"), "audio");

        // An old gateway (no contracts at all): everything false — the
        // gate then says "not supported", never a fabricated view.
        let old = contracts_from_capabilities(&json!({"flows": []}));
        assert!(!old.model_residency && !old.host_state && !old.session_caches);
        assert!(old.modality_labels.is_empty());
    }

    #[test]
    fn capability_contracts_read_the_live_capabilities_envelope() {
        // The REAL endpoint wraps everything in `capabilities` — captured
        // live from `GET /api/gateway/discovery/capabilities` (2026-08-28).
        // The bare-`contracts` fixture above is the tolerated unwrapped
        // shape; THIS is the wire truth, and reading only the top level
        // regresses to "not supported by this gateway" on every real
        // gateway.
        let v = json!({"capabilities": {
            "abstractcore": {"version": "2.13.40"},
            "contracts": {"version": 1, "abstractcode": {}, "common": {
                "model_residency": {
                    "endpoints": {"loaded": "/api/gateway/models/loaded"},
                    "row_schema": "model_residency_row_v1",
                    "modality_ui": {"version": 1, "colors": {
                        "text_generation": {"color": "#00D2FF", "label": "Text"}
                    }}
                },
                "host_state": {
                    "route_available": true, "available": true, "memory_available": true,
                    "endpoints": {"state": "/api/gateway/host/state",
                                   "memory": "/api/gateway/host/metrics/memory",
                                   "gpu": "/api/gateway/host/metrics/gpu"}
                },
                "session_caches": {"endpoints": {"list": "/a", "clear_all": "/b"}}
            }}
        }});
        let c = contracts_from_capabilities(&v);
        assert!(c.model_residency && c.host_state && c.session_caches);
        assert_eq!(c.label_for("text_generation"), "Text");

        // A NON-OBJECT `capabilities` key (a stray flag on an unwrapped
        // payload) must not hijack the root: fall back to the top level.
        let odd = contracts_from_capabilities(
            &json!({"capabilities": true, "contracts": {"common": {"host_state": {}}}}),
        );
        assert!(
            odd.host_state,
            "non-object capabilities falls back to top level"
        );
    }

    fn full_host_state() -> Value {
        json!({
            // Live wire truth: `ts` is an epoch FLOAT (`time.time()`),
            // captured 2026-08-28. 1787894010.297934 % 86400 → 05:13:30 UTC.
            "ok": true, "ts": 1787894010.297934f64,
            "memory": {
                "ram": {"total_bytes": 137438953472u64, "available_bytes": 52200000000u64,
                         "used_bytes": 85238953472u64, "percent": 62.0},
                "process": {"rss_bytes": 512000000u64},
                // The live device block, verbatim: `allocated_bytes` is
                // PROCESS-LOCAL and reads 0 while ~98 GB of weights are
                // resident. `host_in_use_bytes` / `wired_limit_bytes`
                // are the all-processes pair the meter must prefer.
                "device": {"backend": "mlx", "allocated_bytes": 0u64,
                            "total_bytes": 137438953472u64, "free_bytes": 31694962688u64,
                            "host_in_use_bytes": 105743990784u64,
                            "wired_limit_bytes": 115343360000u64},
                "host": {"host_name": "studio.local"}
            },
            "gpu": {"supported": true, "utilization_gpu_pct": 21.0},
            "models": [
                {"runtime_id": "rt-1", "task": "text-generation", "provider": "lmstudio",
                 "model": "qwen3-4b", "source": "config", "resident": true,
                 "state": "loaded", "locked": true, "lockable": true,
                 "modalities": ["text"], "size_bytes": 4508876800u64,
                 "size_vram_bytes": 4508876800u64, "est_weights_bytes": 4400000000u64,
                 "cache_bytes": 268435456u64, "context_length": 32768,
                 "calibrated_context_length": 28672, "context_calibrated": true,
                 "default": true, "loaded_at": "2026-08-27T09:00:00Z",
                 "last_used_at": "2026-08-27T09:59:00Z"},
                {"task": "image-generation", "provider": "mlx-gen", "model": "flux",
                 "resident": null, "state": null, "locked": null, "lockable": false,
                 "size_bytes": null, "est_weights_bytes": null, "cache_bytes": null,
                 "context_length": null},
                // The host-sweep row EXACTLY as the wire emits it: LM
                // Studio loaded it, so the gateway stamps
                // `source: "provider_server"` and `lockable: true`, and a
                // lock ADOPTS it. No measured size — only an ESTIMATE.
                {"runtime_id": null, "task": "text-generation", "provider": "lmstudio",
                 "model": "glm-4.6-gguf", "source": "provider_server", "resident": true,
                 "state": "provider_loaded", "locked": false, "lockable": true,
                 "size_bytes": null, "size_vram_bytes": null,
                 "est_weights_bytes": 99857989632u64, "cache_bytes": 2147483648u64,
                 "context_length": 131072, "default": false}
            ],
            "session_caches": [
                {"key": "k1", "provider": "lmstudio", "model": "qwen3-4b",
                 "session_id": "acode-abc", "bytes": 1048576, "token_count": 2100}
            ],
            "totals": {"model_bytes": 4508876800u64, "models_resident": 2,
                        "cache_bytes_models": 2415919104u64,
                        "session_cache_bytes": 1048576u64},
            "degraded": [],
            "reasons": {}
        })
    }

    #[test]
    fn host_state_parses_the_full_shape() {
        let f = host_state_from_response(&full_host_state());
        assert_eq!(f.ts, "05:13:30", "epoch-float ts formatted to a UTC clock");
        // An ISO-string ts (older/other emitters) still carries verbatim
        // for `short_ts` to trim; junk numbers read as absent.
        let iso = host_state_from_response(&json!({"ts": "2026-08-27T10:00:00Z"}));
        assert_eq!(iso.ts, "2026-08-27T10:00:00Z");
        let junk = host_state_from_response(&json!({"ts": -5.0}));
        assert_eq!(
            junk.ts, "",
            "negative epoch reads as absent, never fabricated"
        );
        assert_eq!(f.host_name, "studio.local");
        assert_eq!(f.ram_total, Some(137438953472));
        assert_eq!(f.ram_percent, Some(62.0));
        assert_eq!(f.process_rss, Some(512000000));
        assert_eq!(f.device_backend, "mlx");
        assert_eq!(f.device_allocated, Some(0), "the wire really serves 0");
        assert_eq!(f.device_host_in_use, Some(105743990784));
        assert_eq!(f.device_wired_limit, Some(115343360000));
        assert!(f.gpu_supported);
        assert_eq!(f.gpu_util_pct, Some(21.0));
        assert_eq!(f.models.len(), 3);
        let m = &f.models[0];
        assert_eq!(m.resident, Some(true));
        assert!(m.locked && m.lockable == Some(true) && m.context_calibrated && m.is_default);
        assert_eq!(m.calibrated_context_length, Some(28672));
        // The additive row_v1 numbers fold, and the coalesce prefers the
        // MEASURED size over the estimate that rides beside it.
        assert_eq!(m.est_weights_bytes, Some(4400000000));
        assert_eq!(m.cache_bytes, Some(268435456));
        assert_eq!(m.display_size(), Some((4508876800, false)));
        let sweep = &f.models[2];
        assert_eq!(sweep.size_bytes, None, "the sweep row measured nothing");
        assert_eq!(
            sweep.display_size(),
            Some((99857989632, true)),
            "…so the ESTIMATE renders, flagged as one"
        );
        assert_eq!(
            sweep.source, "provider_server",
            "the wire's ONE sweep marker — the adopt selector keys on it"
        );
        assert_eq!(
            sweep.lockable,
            Some(true),
            "the wave stamps sweep rows lockable:true — so `lockable` can\
             never be the adopt selector"
        );
        assert_eq!(f.caches.len(), 1);
        assert_eq!(f.caches[0].bytes, Some(1048576));
        assert_eq!(
            f.totals,
            vec![
                ("cache_bytes_models".to_string(), 2415919104),
                ("model_bytes".to_string(), 4508876800),
                ("models_resident".to_string(), 2),
                ("session_cache_bytes".to_string(), 1048576),
            ]
        );
        assert!(f.degraded.is_empty());
    }

    /// THE METAL BUG: `allocated_bytes` is process-local and reads 0
    /// with ~98 GB resident. The meter prefers the ALL-PROCESSES pair and
    /// names its scope; the process-local pair is the labelled fallback.
    #[test]
    fn device_meter_prefers_the_all_processes_figure_over_the_process_local_zero() {
        use crate::store::DeviceScope;
        let f = host_state_from_response(&full_host_state());
        assert_eq!(
            f.device_meter(),
            Some((105743990784, 115343360000, DeviceScope::AllProcesses))
        );
        // Spec PART A3: the scope words are exactly these two. No
        // spelling of "host" survives as a scope name for this figure.
        assert_eq!(DeviceScope::AllProcesses.label(), "all processes");
        assert_eq!(DeviceScope::Process.label(), "this process only");

        // No host figure → the process-local pair, LABELLED as such.
        let older = host_state_from_response(&json!({
            "memory": {"device": {"backend": "cuda", "allocated_bytes": 5000,
                                   "total_bytes": 50000}}
        }));
        assert_eq!(
            older.device_meter(),
            Some((5000, 50000, DeviceScope::Process))
        );
        // An all-processes figure with no wired limit still outranks it.
        let no_limit = host_state_from_response(&json!({
            "memory": {"device": {"allocated_bytes": 0, "total_bytes": 50000,
                                   "host_in_use_bytes": 40000}}
        }));
        assert_eq!(
            no_limit.device_meter(),
            Some((40000, 50000, DeviceScope::AllProcesses))
        );
        // Nothing known → nothing drawn.
        assert_eq!(HostFacts::default().device_meter(), None);
        // A used figure with NO ceiling is still a fact — it just cannot
        // draw a bar (spec PART B2 renders it as the bare `used`).
        let no_ceiling = host_state_from_response(&json!({
            "memory": {"device": {"backend": "metal", "host_in_use_bytes": 4096}}
        }));
        assert_eq!(no_ceiling.device_meter(), None, "no denominator, no bar");
        assert_eq!(
            no_ceiling.accelerator_figure(),
            Some((4096, None, DeviceScope::AllProcesses))
        );
    }

    /// Refinement 1 + spec PART D1: every RESIDENT line offers the lock
    /// verb, sweep rows included (`POST /models/lock` adopts them), and
    /// the ADOPT wording keys on `source == "provider_server"` — the one
    /// marker the wire actually stamps. Locked outranks residency; a
    /// non-resident row gets no lock and no unload, with the reason named.
    #[test]
    fn lock_action_follows_residency_and_adopts_provider_server_rows() {
        use crate::store::{lock_action, unload_refusal, LockAction, ResidencyRow};
        let f = host_state_from_response(&full_host_state());
        assert_eq!(lock_action(&f.models[0]), LockAction::Unlock, "locked row");
        assert_eq!(
            lock_action(&f.models[2]),
            LockAction::Lock { adopt: true },
            "the wire's `source: provider_server` row locks by ADOPTING"
        );
        // `lockable: true` rides on the sweep row too, so it can never be
        // the adopt selector — and no other source string adopts.
        assert_eq!(
            lock_action(&ResidencyRow {
                resident: Some(true),
                lockable: Some(true),
                source: "config".into(),
                ..Default::default()
            }),
            LockAction::Lock { adopt: false },
            "a gateway-loaded row locks WITHOUT adopting"
        );
        for other in ["sweep", "external", "PROVIDER_SERVER", ""] {
            assert_eq!(
                lock_action(&ResidencyRow {
                    resident: Some(true),
                    source: other.into(),
                    ..Default::default()
                }),
                LockAction::Lock { adopt: false },
                "only the exact `provider_server` adopts, not {other:?}"
            );
        }
        assert!(matches!(
            lock_action(&f.models[1]),
            LockAction::Refused(_)
        ));
        assert!(unload_refusal(&f.models[2]).is_none());
        assert!(
            unload_refusal(&f.models[1]).is_none(),
            "resident:null is UNKNOWN, not a 'no' — the gateway decides"
        );

        let evicted = ResidencyRow {
            locked: true,
            resident: Some(false),
            ..Default::default()
        };
        assert_eq!(
            lock_action(&evicted),
            LockAction::Unlock,
            "a locked-but-evicted row keeps its Unlock"
        );
        let cold = ResidencyRow {
            resident: Some(false),
            ..Default::default()
        };
        assert!(matches!(lock_action(&cold), LockAction::Refused(_)));
        assert!(unload_refusal(&cold).is_some());
        let refused = ResidencyRow {
            resident: Some(true),
            lockable: Some(false),
            ..Default::default()
        };
        assert!(matches!(lock_action(&refused), LockAction::Refused(_)));
        // THE GATE OUTRANKS THE WORDING. This is the live shape on this
        // machine — LM Studio's row is `provider_server` AND
        // `lockable: false` — so no lock affordance renders at all and
        // the adopt wording never gets the chance to. The two selectors
        // stay independent; the gate is unchanged by spec PART D1.
        let gated = ResidencyRow {
            resident: Some(true),
            source: "provider_server".into(),
            lockable: Some(false),
            ..Default::default()
        };
        assert!(
            matches!(lock_action(&gated), LockAction::Refused(_)),
            "a provider_server row the gateway calls not-lockable gets NO lock verb"
        );
    }

    #[test]
    fn resident_null_stays_unknown_never_a_fabricated_no() {
        // The tri-state contract: null resident is UNKNOWN. Folding it to
        // false would render "no" for a state the gateway itself does not
        // claim to know.
        let f = host_state_from_response(&full_host_state());
        let m = &f.models[1];
        assert_eq!(m.resident, None, "null = unknown, not false");
        assert_eq!(m.est_weights_bytes, None, "null size fields stay unknown");
        assert_eq!(m.cache_bytes, None);
        assert_eq!(m.display_size(), None, "nothing known: no size at all");
        assert!(m.state.is_empty(), "null state reads as absent");
        assert!(
            !m.locked,
            "null locked reads as not-locked (no 🔒 invented)"
        );
        assert_eq!(m.size_bytes, None);
        assert_eq!(m.context_length, None);
    }

    #[test]
    fn degraded_gpu_and_missing_sections_read_as_absence() {
        let v = json!({
            "ok": true,
            "memory": {"ram": {"total_bytes": 1000, "used_bytes": 620}},
            "gpu": {"supported": false, "utilization_gpu_pct": 55.0},
            "models": [],
            "degraded": ["gpu", "device"],
            "reasons": {"gpu": "ioreg unavailable"}
        });
        let f = host_state_from_response(&v);
        assert!(!f.gpu_supported);
        // A stray number under supported:false must NOT resurrect the
        // meter (the gpu.rs honesty rule applied here).
        assert_eq!(f.gpu_util_pct, None);
        assert_eq!(f.ram_percent, None, "absent percent stays unknown");
        assert_eq!(f.process_rss, None);
        assert!(f.device_backend.is_empty());
        assert_eq!(
            f.degraded,
            vec!["gpu: ioreg unavailable".to_string(), "device".to_string()],
            "degraded lanes fold with their reason when named"
        );
    }

    #[test]
    fn junk_percents_read_as_unknown_never_clamped_truth() {
        // A percent outside 0..=100 is junk, not a measurement — clamping
        // it would fabricate a number the host never reported.
        for junk in [-3.0f64, 100.1, 250.0, f64::NAN] {
            let v = json!({"memory": {"ram": {"percent": junk}}});
            assert_eq!(
                host_state_from_response(&v).ram_percent,
                None,
                "{junk} must read unknown"
            );
        }
        // The boundaries are real values.
        for ok in [0.0f64, 62.0, 100.0] {
            let v = json!({"memory": {"ram": {"percent": ok}}});
            assert_eq!(host_state_from_response(&v).ram_percent, Some(ok));
        }
    }

    #[test]
    fn junk_sizes_read_as_unknown_never_zero() {
        let v = json!({"models": [
            {"provider": "p", "model": "m", "size_bytes": -5,
             "size_vram_bytes": "big", "context_length": 3.7,
             "calibrated_context_length": null},
            {"provider": "", "model": ""}
        ]});
        let rows = residency_rows_v1(&v);
        assert_eq!(rows.len(), 1, "a row with no identity is dropped");
        let r = &rows[0];
        assert_eq!(r.size_bytes, None, "negative = junk = unknown");
        assert_eq!(r.size_vram_bytes, None, "string = junk = unknown");
        assert_eq!(r.context_length, None, "non-integer = unknown");
        assert_eq!(r.calibrated_context_length, None);
    }

    #[test]
    fn context_estimate_line_renders_known_parts_only() {
        assert_eq!(
            context_estimate_line(&json!({
                "confidence": "calibrated", "predicted_max_context": 28672,
                "notes": ["measured on this host", "vram-bound"]
            })),
            "calibrated · predicted max context 28672 · measured on this host; vram-bound"
        );
        // Unknown answer: no invented number.
        assert_eq!(
            context_estimate_line(&json!({"confidence": "unknown"})),
            "unknown"
        );
        assert_eq!(context_estimate_line(&json!({})), "unknown");
        // In-band failure surfaces the served error, never a bare
        // "unknown" masquerading as an estimator verdict.
        assert_eq!(
            context_estimate_line(&json!({"ok": false, "error": "context_estimate_unavailable"})),
            "estimate unavailable: context_estimate_unavailable"
        );
        // ok:false with no error text falls through to the honest default.
        assert_eq!(context_estimate_line(&json!({"ok": false})), "unknown");
    }
}
