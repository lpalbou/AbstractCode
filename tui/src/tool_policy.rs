//! Persistent tool-approval tiers: which tool batches auto-approve.
//!
//! Three ordered tiers form a legible gradient (the abstractcode CLI
//! precedent, `tool_permissions.py` — read-only < write < full-auto):
//!
//!   read  → only PROVEN read-only tools auto-approve (the list/read/
//!           search/skim/analyze name class)
//!   write → read's set PLUS workspace file mutations (write_file/edit_file
//!           — the runtime clamps them to the run's workspace)
//!   all   → everything auto-approves: arbitrary shell, network egress,
//!           and unknown (MCP/future) tools
//!
//! The ACCEPTED level is a persisted preference (`prefs.json:
//! tool_approval.accepted_tier`); a batch whose every call classifies
//! at-or-below it resumes without a prompt — "if the highest tier is
//! accepted, nothing is ever asked" (maintainer, 2026-07-22). The
//! `/permissions` command is the one surface (the c5028 consolidation).
//!
//! Server truth vs the #FALLBACK name table: since the 2026-07-23
//! full-catalog bounce the gateway serves per-tool `approval_default`
//! (auto|ask) + capability `tier` + `risk_rank` on every discovery row —
//! the `_with` variants below PREFER that served truth (rank floors,
//! approval tightens — see `server_tier`), and the name table classifies
//! only tools the server carried no approval fact for (older gateways,
//! or gaps). Served-disabled rows (`enabled: false` + gate) clamp to ask
//! everywhere, above pins. (The read-only-git client override is retired
//! — see the rule below.)
//!
//! Conservative-by-construction rules (ported from the adversary-hardened
//! Python precedent):
//! - `execute_command` is NEVER below `all` in this client. (The former
//!   ONE exception — the two-stage read-only-git proof — RETIRED
//!   2026-07-24, c5057: the decision moved to the runtime approval point
//!   as the `git_read_only@v1` refiner, declared by core on the tool's
//!   inventory row. Proven read-only git auto-approves SERVER-side and
//!   never generates a wait; the prover and the executor are one party.)
//! - Unknown tools (MCP `mcp::*`, future tools) classify as `all`:
//!   auto-approval requires positive knowledge, so they only auto under
//!   an accepted tier of `all` (the explicit eyes-open posture).
//! - `fetch_url` classifies as `all` (network egress with model-controlled
//!   method/body — `remote_write_capable` in core's inventory). NOTE: the
//!   it auto-runs under an operator ruling about core's
//!   in-tool base64 URL screen; here the tool executes on the GATEWAY,
//!   so this client keeps the conservative tier until the gateway serves
//!   its own classification (the #FALLBACK above).

use serde_json::Value;

/// Approval tier, ordered: `Read < Write < All`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Read,
    Write,
    All,
}

impl Tier {
    /// Parse a tier id/label. Accepts the canonical ids plus the obvious
    /// spellings; `None` for anything unknown (command surfaces refuse
    /// loudly instead of silently defaulting).
    pub fn parse(raw: &str) -> Option<Tier> {
        match raw.trim().to_lowercase().replace('-', "_").as_str() {
            "read" | "read_only" | "readonly" => Some(Tier::Read),
            "write" => Some(Tier::Write),
            "all" | "full" | "full_auto" | "fullauto" => Some(Tier::All),
            _ => None,
        }
    }

    /// Parse with the fail-safe default: unknown/empty → the STRICTEST
    /// tier (`read`), never a more permissive guess. Used where a value
    /// arrives from persisted config rather than a typed command.
    pub fn parse_or_default(raw: &str) -> Tier {
        Tier::parse(raw).unwrap_or(Tier::Read)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::Read => "read",
            Tier::Write => "write",
            Tier::All => "all",
        }
    }

    /// One-line description for notices + help.
    pub fn description(self) -> &'static str {
        match self {
            Tier::Read => "read-only tool batches auto-approve; writes/shell/network prompt",
            Tier::Write => "reads + workspace file writes auto-approve; shell/network prompt",
            // The two residual gates are DELIBERATE and the setter's own
            // notice must not lie about them (the fabricated-selection
            // lesson): a per-tool `ask` pin is the user's explicit
            // boundary, and a gateway-disabled row cannot run at all.
            Tier::All => {
                "every tool auto-approves — no prompts (per-tool 'ask' pins and gateway-disabled tools still gate)"
            }
        }
    }
}

/// Tools PROVEN read-only by core's builtin inventory facts
/// (`mutating=False` AND `remote_write_capable=False` in
/// `abstractcore/tools/inventory.py`). Names outside this table fail
/// closed to `All`.
const READ_TOOLS: &[&str] = &[
    "analyze_code",
    "analyze_media", // read-only in effect (nested LLM call = cost, not mutation)
    "list_files",
    "read_file",
    "search_files",
    "skim_files",
    "skim_folders",
    "skim_url",
    "skim_websearch",
    "web_search",
];

/// Workspace file mutations (`mutating=True`, workspace-clamped by the
/// runtime's path resolution).
const WRITE_TOOLS: &[&str] = &["write_file", "edit_file"];

// ---------------------------------------------------------------------------
// Server-served classification (facts UPDATE 2, commons 4356/4360): the
// gateway's `discovery/tools` items MAY carry per-tool `tier` + an
// approval dial (live spelling `approval_default`; legacy `approval` —
// render-when-present). When present, the served truth WINS over the
// `#FALLBACK` name table below; when absent, the name table applies
// unchanged. Both states remain parseable; the post-bounce gateway
// serves the dial on every row (live-verified 2026-07-23: 50/50,
// 12 auto / 38 ask, disabled rows clamped to ask server-side).
// ---------------------------------------------------------------------------

/// A minimal per-tool classification fact from `discovery/tools`.
///
/// `approval` is the finer dial ("auto" | "ask"); `tier` is informational
/// (server semantics: ALL core-registry tools are `tier2_world` by the
/// 2026-07-06 boundary ruling, with `approval` as the real discriminator —
/// so the tier string carries no auto/ask signal and is kept only for the
/// modal/inspection). `approval == None` means the field was not served:
/// the caller falls back to the name table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolClass {
    pub name: String,
    pub approval: Option<String>,
    pub tier: Option<String>,
    /// Served `enabled: false` (the full-catalog surfacing fix): the row
    /// exists but a gate disables it. The clamp rule (F3, client side):
    /// a served-disabled tool NEVER auto-approves — not by tier, not by
    /// a (possibly stale) `auto` pin — and the run-policy expansion
    /// force-asks it instead of auto-approving. What a disabled row
    /// would do if granted is a question that should never be
    /// answerable. `false` (the derive default) = enabled.
    pub served_disabled: bool,
    /// The named gate for a served-disabled row (approval-card teaching;
    /// empty when the row is enabled or the gateway named no gate).
    pub enable_gate: String,
    /// Served `risk_rank` (core's band ladder: observe=1, act=2,
    /// outreach=3, destroy=4). When present it FLOORS the tier mapping
    /// (`server_tier`): the band decides the level and `approval` can
    /// only TIGHTEN, never loosen — the transitional-belt rule from the
    /// converged contract (c5028 finding 3): enabled comms rows serve
    /// `approval_default: "auto"` (the 2026-02-21 comms decision) while
    /// the ladder says outreach asks below full trust; deriving from
    /// approval alone would auto outreach sends at accepted tier `read`.
    pub risk_rank: Option<u8>,
}

/// True when the inventory serves `name` as a DISABLED row.
fn served_disabled(name: &str, classes: &[ToolClass]) -> bool {
    classes.iter().any(|c| c.name == name && c.served_disabled)
}

/// Map the server's `approval` field onto the client's accepted-tier
/// gradient (the fact-#2 mapping expressed as a `Tier`, so the uniform
/// `needed <= accepted` comparison keeps working):
/// - `approval == "auto"` → `Read` (auto-approves at read and above —
///   "'read' accepts approval:auto tools");
/// - `approval == "ask"` on the `write_file`/`edit_file` class → `Write`
///   ("'write' additionally accepts the write_file/edit_file class"; the
///   client name-check elevates these two out of `All` even though the
///   server keeps them approval:ask);
/// - anything else at `ask` (shell, network, MCP) → `All` ("'all'
///   accepts everything"), which is also the fail-toward-asking default
///   for an unknown `approval` value.
///
/// RANK-BAND FLOOR (converged contract c5028, finding 3): when the row
/// serves `risk_rank`, the band sets a FLOOR the approval dial can never
/// lower — observe(1) → `Read`, act(2) → `Write`, outreach(3)+ → `All`.
/// The two signals combine as `max(band, approval)`: a rank-2 tool served
/// `ask` (analyze_media's model_cost refinement) still asks below `all`,
/// and an outreach row served `auto` (the comms carve-out) still
/// classifies `All` — approval tightens, never loosens. Rank-less rows
/// (older gateways) keep the approval-only mapping unchanged.
pub fn server_tier(name: &str, approval: &str, risk_rank: Option<u8>) -> Tier {
    let approval_tier = match approval.trim().to_lowercase().as_str() {
        "auto" => Tier::Read,
        _ if name == "write_file" || name == "edit_file" => Tier::Write,
        _ => Tier::All,
    };
    let band_floor = match risk_rank {
        Some(1) => Tier::Read,
        Some(2) => Tier::Write,
        Some(_) => Tier::All, // outreach(3)/destroy(4)/unknown ranks
        None => Tier::Read,   // no band signal: approval alone decides
    };
    approval_tier.max(band_floor)
}

/// The served `(approval, risk_rank)` for `name`, if the inventory
/// carried an approval fact (the rank rides along when present — it can
/// exist only on rows that also serve the dial, so approval presence
/// stays the one served-truth gate).
fn server_facts<'a>(name: &str, classes: &'a [ToolClass]) -> Option<(&'a str, Option<u8>)> {
    classes
        .iter()
        .find(|c| c.name == name)
        .and_then(|c| c.approval.as_deref().map(|a| (a, c.risk_rank)))
}

/// Classify one tool call into the tier it NEEDS. Unknown names fail
/// closed to `All` (auto requires proof — the Python precedent's rule).
/// Name-table only (#FALLBACK); see [`classify_call_with`] for the
/// server-truth-preferring path.
pub fn classify_call(name: &str) -> Tier {
    let name = name.trim();
    if READ_TOOLS.contains(&name) {
        return Tier::Read;
    }
    if WRITE_TOOLS.contains(&name) {
        return Tier::Write;
    }
    // execute_command / shell_exec / shell_write_stdin / fetch_url / MCP /
    // anything future: the top tier. The former ONE below-`all` shell
    // exception — the client's 330-line read-only-git PROOF — is RETIRED
    // (c5057, 2026-07-24): the decision moved server-side as runtime's
    // `git_read_only@v1` refiner, declared by core on execute_command's
    // inventory row. The prover and the executor are one party again
    // (lane-1 finding 3); a proven read-only git call is auto-approved AT
    // THE APPROVAL POINT and never generates a wait, so this table's
    // fail-closed `All` only decides on pre-refiner gateways — where one
    // prompt is the honest price of not owning the proof.
    Tier::All
}

/// Classify one tool call, PREFERRING the gateway's served truth when the
/// inventory carried an `approval` field for the tool. When no server
/// class matches `name`, this is exactly [`classify_call`].
pub fn classify_call_with(name: &str, classes: &[ToolClass]) -> Tier {
    let name = name.trim();
    if let Some((approval, rank)) = server_facts(name, classes) {
        return server_tier(name, approval, rank);
    }
    classify_call(name)
}

/// WHERE a call's tier classification came from — the honesty label for
/// approval surfaces (thin-client conformance, class ii): the client name
/// table is a `#FALLBACK` that only ever applies when the gateway served
/// no approval fact for the tool, and any surface rendering the tier must
/// be able to SAY which authority produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassSource {
    /// The gateway's served `approval` fact decided (server truth).
    Server,
    /// The client's `#FALLBACK` name table decided (no server facts for
    /// this name — includes unknown/unnamed calls failing closed).
    NameTable,
}

/// The classification source for one call, mirroring
/// [`classify_call_with`]'s decision order exactly (server fact → name
/// table; the GitProof source retired with the client proof — c5057).
/// Kept beside it so the two can never drift.
pub fn classify_source(name: &str, classes: &[ToolClass]) -> ClassSource {
    let name = name.trim();
    if server_facts(name, classes).is_some() {
        return ClassSource::Server;
    }
    ClassSource::NameTable
}

/// The batch's names whose tier came from the `#FALLBACK` name table —
/// what an approval surface must label when non-empty (sorted, deduped;
/// unnamed calls report as `(unnamed)`: they fail closed client-side and
/// that too is a client decision worth naming).
pub fn batch_name_table_names(tool_calls: &[Value], classes: &[ToolClass]) -> Vec<String> {
    let mut out: Vec<String> = tool_calls
        .iter()
        .filter_map(|tc| {
            let name = tc
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if name.is_empty() {
                return Some("(unnamed)".to_string());
            }
            match classify_source(&name, classes) {
                ClassSource::NameTable => Some(name),
                ClassSource::Server => None,
            }
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The tier a whole batch needs = the max over its calls. An empty or
/// malformed batch needs `All` (fail closed — never auto-approve what
/// cannot be read). Name-table only; see [`batch_tier_with`].
pub fn batch_tier(tool_calls: &[Value]) -> Tier {
    batch_tier_with(tool_calls, &[])
}

/// [`batch_tier`] preferring server truth (the discovery inventory).
pub fn batch_tier_with(tool_calls: &[Value], classes: &[ToolClass]) -> Tier {
    if tool_calls.is_empty() {
        return Tier::All;
    }
    tool_calls
        .iter()
        .map(|tc| {
            let name = tc.get("name").and_then(Value::as_str).unwrap_or("");
            if name.is_empty() {
                return Tier::All; // nameless call: unreadable, fail closed
            }
            classify_call_with(name, classes)
        })
        .max()
        .unwrap_or(Tier::All)
}

/// Per-call decision under the accepted tier + per-tool override pins
/// (`name → "auto" | "ask"`) + optional server truth. An `ask` pin always
/// prompts (even reads); an `auto` pin always auto-approves (an explicit
/// user act — documented as such in prefs). Unknown override values are
/// ignored (fail to the tier decision, never to auto).
///
/// SERVED-DISABLED CLAMP (checked FIRST, above pins): a tool the gateway
/// serves `enabled: false` never auto-approves — a stale persisted
/// `auto` pin from when the tool was enabled must not silently lift a
/// row the operator's gate turned off.
pub fn call_auto_approves(
    name: &str,
    accepted: Tier,
    overrides: &[(String, String)],
    classes: &[ToolClass],
) -> bool {
    if served_disabled(name, classes) {
        return false;
    }
    if let Some((_, decision)) = overrides.iter().find(|(n, _)| n == name) {
        match decision.as_str() {
            "auto" => return true,
            "ask" => return false,
            _ => {} // unknown decision: fall through to the tier
        }
    }
    classify_call_with(name, classes) <= accepted
}

/// Whole-batch decision: every call must individually auto-approve.
/// `accepted_raw` is the persisted string ("" / unknown → `read`,
/// the strictest — see `Tier::parse_or_default`). Name-table only; see
/// [`batch_auto_approves_with`].
pub fn batch_auto_approves(
    tool_calls: &[Value],
    accepted_raw: &str,
    overrides: &[(String, String)],
) -> bool {
    batch_auto_approves_with(tool_calls, accepted_raw, overrides, &[])
}

/// [`batch_auto_approves`] preferring server truth (the discovery
/// inventory). Empty `classes` reproduces the name-table behavior exactly.
pub fn batch_auto_approves_with(
    tool_calls: &[Value],
    accepted_raw: &str,
    overrides: &[(String, String)],
    classes: &[ToolClass],
) -> bool {
    let accepted = Tier::parse_or_default(accepted_raw);
    if tool_calls.is_empty() {
        return false; // unreadable batch: a human decides
    }
    tool_calls.iter().all(|tc| {
        let name = tc.get("name").and_then(Value::as_str).unwrap_or("");
        !name.is_empty() && call_auto_approves(name, accepted, overrides, classes)
    })
}

/// The server-side run tool-policy name lists (`input_data._runtime.
/// tool_policy`). The runtime consumer executes `auto_approve_tools` with
/// NO wait round-trip and force-asks `require_approval_tools` (facts #1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunToolPolicy {
    pub auto_approve_tools: Vec<String>,
    pub require_approval_tools: Vec<String>,
}

impl RunToolPolicy {
    pub fn is_empty(&self) -> bool {
        self.auto_approve_tools.is_empty() && self.require_approval_tools.is_empty()
    }
}

/// Expand the accepted tier + per-tool pins into server-side run-policy
/// name lists over the CURRENT inventory (`classes`).
///
/// - `auto_approve_tools` = inventory names the accepted tier admits
///   (server truth preferred, else the name table), PLUS `auto`-pinned
///   names, MINUS `ask`-pinned names.
/// - `require_approval_tools` = explicit `ask` pins that exist in the
///   inventory. These are listed so the server force-asks them even
///   against its own auto-default (a pure exclusion from
///   `auto_approve_tools` would let a server-side auto-default silently
///   override the user's `ask` pin).
///
/// Classification is name-level, so `execute_command` only lands in
/// `auto_approve_tools` at tier `all` (or an explicit `auto` pin);
/// proven read-only git is approved server-side by the
/// `git_read_only@v1` refiner (c5057). An empty inventory yields an
/// empty policy (send nothing).
pub fn expand_run_policy(
    classes: &[ToolClass],
    accepted_raw: &str,
    overrides: &[(String, String)],
) -> RunToolPolicy {
    let accepted = Tier::parse_or_default(accepted_raw);
    let mut auto: Vec<String> = Vec::new();
    let mut require: Vec<String> = Vec::new();
    for c in classes {
        let name = c.name.trim();
        if name.is_empty() {
            continue;
        }
        // Served-disabled clamp (F3, client side — checked ABOVE pins):
        // a gate-disabled row lands in require_approval_tools, never in
        // auto — even under a stale `auto` pin or tier `all`. Should the
        // run somehow carry such a call, the server-side policy then
        // force-asks it (defense in depth with the runtime's own clamp).
        if c.served_disabled {
            require.push(name.to_string());
            continue;
        }
        let pin = overrides
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d.as_str());
        match pin {
            Some("auto") => auto.push(name.to_string()),
            Some("ask") => require.push(name.to_string()),
            _ => {
                if classify_call_with(name, classes) <= accepted {
                    auto.push(name.to_string());
                }
            }
        }
    }
    // Deterministic wire order (testable payloads; also dedups a tool
    // that appears twice in a drifting inventory).
    auto.sort();
    auto.dedup();
    require.sort();
    require.dedup();
    RunToolPolicy {
        auto_approve_tools: auto,
        require_approval_tools: require,
    }
}

// ---------------------------------------------------------------------------
// The execute_command read-only-git PROOF (330 lines, two-stage, the
// adversary attack corpus beside it) lived here until 2026-07-24. It is
// RETIRED: the decision moved to the runtime approval point as the
// `git_read_only@v1` refiner (send_email_recipient@v1 precedent), declared
// by core on execute_command's inventory row (commons c5042/c5057). The
// Python twin (abstractcode/tool_permissions.py classify_execute_command)
// retires with it — one proof, at the executor, not in every client.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------
    // Tier vocabulary
    // -----------------------------------------------------------------

    #[test]
    fn tier_order_is_the_legibility_gradient() {
        assert!(Tier::Read < Tier::Write);
        assert!(Tier::Write < Tier::All);
    }

    #[test]
    fn tier_parse_accepts_labels_refuses_garbage() {
        assert_eq!(Tier::parse("read"), Some(Tier::Read));
        assert_eq!(Tier::parse("read-only"), Some(Tier::Read));
        assert_eq!(Tier::parse("WRITE"), Some(Tier::Write));
        assert_eq!(Tier::parse("all"), Some(Tier::All));
        assert_eq!(Tier::parse("full-auto"), Some(Tier::All));
        assert_eq!(Tier::parse("garbage"), None);
        assert_eq!(Tier::parse(""), None);
        // Persisted-config lane: unknown falls to the STRICTEST tier.
        assert_eq!(Tier::parse_or_default("garbage"), Tier::Read);
        assert_eq!(Tier::parse_or_default(""), Tier::Read);
    }

    // -----------------------------------------------------------------
    // Classification
    // -----------------------------------------------------------------

    #[test]
    fn read_tools_classify_read_writes_write_rest_all() {
        for name in [
            "read_file",
            "list_files",
            "search_files",
            "web_search",
            "skim_url",
        ] {
            assert_eq!(classify_call(name), Tier::Read, "{name}");
        }
        for name in ["write_file", "edit_file"] {
            assert_eq!(classify_call(name), Tier::Write, "{name}");
        }
        for name in [
            "fetch_url",
            "shell_exec",
            "shell_write_stdin",
            "shell_close",
        ] {
            assert_eq!(classify_call(name), Tier::All, "{name}");
        }
    }

    #[test]
    fn unknown_tools_fail_closed_to_all() {
        // MCP / future tools: auto requires proof (the Python rule).
        assert_eq!(classify_call("mcp::server::delete_everything"), Tier::All);
        assert_eq!(classify_call("brand_new_tool"), Tier::All);
        assert_eq!(classify_call(""), Tier::All);
    }

    #[test]
    fn execute_command_is_always_all_in_the_name_table() {
        // The client git proof is RETIRED (c5057): read-only git is
        // decided at the runtime approval point (git_read_only@v1
        // refiner on execute_command's row) — server-side, before a
        // wait ever reaches this client. The name table fails every
        // shell invocation closed to All (classification is NAME-only
        // now; per-call arguments no longer exist in these signatures);
        // against a pre-refiner gateway one prompt is the honest price
        // of not owning the proof.
        assert_eq!(classify_call("execute_command"), Tier::All);
        assert_eq!(classify_call("shell_exec"), Tier::All);
    }

    // -----------------------------------------------------------------
    // Batch decisions + overrides
    // -----------------------------------------------------------------

    fn call(name: &str) -> Value {
        json!({"name": name, "arguments": {}})
    }

    #[test]
    fn batch_tier_is_the_max_over_calls() {
        assert_eq!(batch_tier(&[call("read_file")]), Tier::Read);
        assert_eq!(
            batch_tier(&[call("read_file"), call("write_file")]),
            Tier::Write
        );
        assert_eq!(
            batch_tier(&[call("read_file"), call("execute_command")]),
            Tier::All
        );
        // Empty/malformed: fail closed.
        assert_eq!(batch_tier(&[]), Tier::All);
        assert_eq!(batch_tier(&[json!({"arguments": {}})]), Tier::All);
    }

    #[test]
    fn batch_auto_respects_accepted_tier() {
        let none: &[(String, String)] = &[];
        assert!(batch_auto_approves(&[call("read_file")], "read", none));
        assert!(!batch_auto_approves(&[call("write_file")], "read", none));
        assert!(batch_auto_approves(&[call("write_file")], "write", none));
        assert!(!batch_auto_approves(&[call("fetch_url")], "write", none));
        assert!(batch_auto_approves(
            &[call("fetch_url"), call("execute_command")],
            "all",
            none
        ));
        // One above-tier call prompts the WHOLE batch.
        assert!(!batch_auto_approves(
            &[call("read_file"), call("write_file")],
            "read",
            none
        ));
        // Unknown persisted tier falls to read (strictest).
        assert!(!batch_auto_approves(&[call("write_file")], "banana", none));
        assert!(batch_auto_approves(&[call("read_file")], "", none));
        // A nameless call is unreadable: never auto.
        assert!(!batch_auto_approves(&[json!({})], "all", none));
        assert!(!batch_auto_approves(&[], "all", none));
    }

    #[test]
    fn git_batches_never_auto_approve_client_side_below_all() {
        // Post-retirement posture (c5057): the client holds NO git
        // proof — a read-only git batch that somehow reaches the
        // client belt prompts below `all` (the runtime refiner
        // normally approves it server-side first, so this belt only
        // fires against pre-refiner gateways).
        let batch = vec![json!({"name": "execute_command",
                                "arguments": {"command": "git log --oneline -n 20"}})];
        let none: &[(String, String)] = &[];
        assert!(!batch_auto_approves(&batch, "read", none));
        assert!(!batch_auto_approves(&batch, "write", none));
        assert!(batch_auto_approves(&batch, "all", none));
        let push = vec![json!({"name": "execute_command",
                               "arguments": {"command": "git push"}})];
        assert!(!batch_auto_approves(&push, "write", none));
    }

    #[test]
    fn overrides_pin_per_tool_decisions() {
        let pins = vec![
            ("fetch_url".to_string(), "auto".to_string()),
            ("read_file".to_string(), "ask".to_string()),
            ("write_file".to_string(), "nonsense".to_string()),
        ];
        // "auto" pin lifts an all-tier tool under a read acceptance.
        assert!(batch_auto_approves(&[call("fetch_url")], "read", &pins));
        // "ask" pin forces a prompt even for a read tool.
        assert!(!batch_auto_approves(&[call("read_file")], "all", &pins));
        // Unknown decision falls through to the tier (write ≤ write).
        assert!(batch_auto_approves(&[call("write_file")], "write", &pins));
    }

    // -----------------------------------------------------------------
    // Server-served classification (facts UPDATE 2): approval field wins
    // -----------------------------------------------------------------

    fn cls(name: &str, approval: &str) -> ToolClass {
        ToolClass {
            name: name.into(),
            approval: Some(approval.into()),
            tier: Some("tier2_world".into()),
            ..Default::default()
        }
    }

    #[test]
    fn server_tier_maps_approval_onto_the_gradient() {
        // read accepts approval:auto; write adds the write_file/edit_file
        // class; all accepts everything (also the fail-toward-ask default).
        assert_eq!(server_tier("read_file", "auto", None), Tier::Read);
        assert_eq!(server_tier("read_file", "AUTO", None), Tier::Read); // case-insensitive
        assert_eq!(server_tier("write_file", "ask", None), Tier::Write);
        assert_eq!(server_tier("edit_file", "ask", None), Tier::Write);
        assert_eq!(server_tier("execute_command", "ask", None), Tier::All);
        assert_eq!(server_tier("mystery", "ask", None), Tier::All);
        assert_eq!(server_tier("mystery", "garbage", None), Tier::All); // fail toward ask
    }

    #[test]
    fn rank_band_floors_the_served_tier_approval_only_tightens() {
        // The converged-contract transitional rule (c5028 finding 3):
        // the band decides the level; approval can tighten, never loosen.
        // The comms hole: an OUTREACH row served approval:auto (the
        // 2026-02-21 comms decision) must NOT classify Read — deriving
        // from approval alone would auto telegram/agora sends at
        // accepted tier read.
        assert_eq!(server_tier("telegram_send", "auto", Some(3)), Tier::All);
        assert_eq!(server_tier("agora_post", "auto", Some(3)), Tier::All);
        // The analyze_media shape: act(2) band + served ask (model_cost
        // refinement) stays above Write — the server's own ask verdict
        // is never downgraded by the band floor.
        assert_eq!(server_tier("analyze_media", "ask", Some(2)), Tier::All);
        // Plain band mapping: observe autos at read; act autos at write;
        // destroy asks below all.
        assert_eq!(server_tier("read_file", "auto", Some(1)), Tier::Read);
        assert_eq!(server_tier("write_file", "ask", Some(2)), Tier::Write);
        assert_eq!(server_tier("execute_command", "ask", Some(4)), Tier::All);
        // Unknown future ranks fail toward asking.
        assert_eq!(server_tier("mystery", "auto", Some(9)), Tier::All);
    }

    #[test]
    fn server_truth_beats_the_name_table() {
        // A tool the NAME TABLE would call All (unknown) but the server
        // serves approval:auto classifies Read — server truth wins.
        let classes = vec![cls("mcp::search", "auto"), cls("execute_command", "ask")];
        assert_eq!(classify_call_with("mcp::search", &classes), Tier::Read);
        // Name table alone still fails it closed to All.
        assert_eq!(classify_call("mcp::search"), Tier::All);
        // With server truth, an mcp:auto batch auto-approves at read tier.
        assert!(batch_auto_approves_with(
            &[call("mcp::search")],
            "read",
            &[],
            &classes
        ));
        assert!(!batch_auto_approves(&[call("mcp::search")], "read", &[]));
    }

    #[test]
    fn empty_classes_reproduce_the_name_table_exactly() {
        // The delegation invariant: batch_auto_approves_with(&[]) == the
        // name-table batch_auto_approves for the same inputs.
        for (calls, tier) in [
            (vec![call("read_file")], "read"),
            (vec![call("write_file")], "read"),
            (vec![call("write_file")], "write"),
            (vec![call("fetch_url")], "all"),
        ] {
            assert_eq!(
                batch_auto_approves_with(&calls, tier, &[], &[]),
                batch_auto_approves(&calls, tier, &[]),
                "delegation mismatch at {tier}"
            );
        }
    }

    // -----------------------------------------------------------------
    // Server-side run-policy expansion (facts #1)
    // -----------------------------------------------------------------

    fn inventory() -> Vec<ToolClass> {
        // Pre-bounce shape: NO server fields (approval None) — the name
        // table drives the tier decision.
        [
            "read_file",
            "list_files",
            "write_file",
            "edit_file",
            "execute_command",
            "fetch_url",
        ]
        .iter()
        .map(|n| ToolClass {
            name: (*n).into(),
            ..Default::default()
        })
        .collect()
    }

    #[test]
    fn expand_run_policy_by_tier_over_the_name_table() {
        let none: &[(String, String)] = &[];
        // read: only proven read-only names auto (no per-call args, so
        // execute_command is NOT auto — the belt proves git per call).
        let read = expand_run_policy(&inventory(), "read", none);
        assert_eq!(read.auto_approve_tools, vec!["list_files", "read_file"]);
        assert!(read.require_approval_tools.is_empty());
        // write: adds the workspace mutations.
        let write = expand_run_policy(&inventory(), "write", none);
        assert_eq!(
            write.auto_approve_tools,
            vec!["edit_file", "list_files", "read_file", "write_file"]
        );
        // all: everything auto-approves — nothing ever asks.
        let all = expand_run_policy(&inventory(), "all", none);
        assert_eq!(
            all.auto_approve_tools,
            vec![
                "edit_file",
                "execute_command",
                "fetch_url",
                "list_files",
                "read_file",
                "write_file"
            ]
        );
    }

    #[test]
    fn expand_run_policy_rides_overrides_both_directions() {
        // auto pin lifts fetch_url under read; ask pin drops read_file and
        // force-asks it (present in require_approval_tools).
        let pins = vec![
            ("fetch_url".to_string(), "auto".to_string()),
            ("read_file".to_string(), "ask".to_string()),
        ];
        let p = expand_run_policy(&inventory(), "read", &pins);
        assert!(p.auto_approve_tools.contains(&"fetch_url".to_string()));
        assert!(!p.auto_approve_tools.contains(&"read_file".to_string()));
        assert_eq!(p.require_approval_tools, vec!["read_file"]);
        // list_files still auto by tier.
        assert!(p.auto_approve_tools.contains(&"list_files".to_string()));
    }

    #[test]
    fn expand_run_policy_prefers_server_truth_when_served() {
        // Post-bounce shape: an MCP tool served approval:auto joins the
        // read-tier auto set that the name table would have excluded.
        let mut inv = inventory();
        inv.push(cls("mcp::search", "auto"));
        let p = expand_run_policy(&inv, "read", &[]);
        assert!(p.auto_approve_tools.contains(&"mcp::search".to_string()));
    }

    /// The served-disabled clamp (full-catalog surfacing, F3 client
    /// side): a row the gateway serves `enabled: false` NEVER
    /// auto-approves — not at tier `all`, not through a served
    /// `approval: auto` fact, not through a stale persisted `auto` pin —
    /// and the run-policy expansion force-asks it instead.
    #[test]
    fn served_disabled_rows_never_auto_approve() {
        let disabled_row = |name: &str| ToolClass {
            name: name.into(),
            // Runtime's pre-tiers fold carries auto rows for some comms
            // tools — the clamp must beat a served auto fact too.
            approval: Some("auto".into()),
            tier: Some("tier2_world".into()),
            served_disabled: true,
            enable_gate: "SOME_GATE".into(),
            ..Default::default()
        };
        let classes = vec![cls("read_file", "auto"), disabled_row("send_email")];
        // Tier all + served approval:auto: still ask.
        assert!(!batch_auto_approves_with(
            &[call("send_email")],
            "all",
            &[],
            &classes
        ));
        // A stale persisted auto pin must not lift the gate either.
        let stale_pin = vec![("send_email".to_string(), "auto".to_string())];
        assert!(!batch_auto_approves_with(
            &[call("send_email")],
            "all",
            &stale_pin,
            &classes
        ));
        // One disabled call prompts the WHOLE batch.
        assert!(!batch_auto_approves_with(
            &[call("read_file"), call("send_email")],
            "all",
            &[],
            &classes
        ));
        // The expansion: disabled rows land in require, never in auto —
        // even under the stale auto pin.
        let p = expand_run_policy(&classes, "all", &stale_pin);
        assert!(!p.auto_approve_tools.contains(&"send_email".to_string()));
        assert_eq!(p.require_approval_tools, vec!["send_email"]);
        assert!(p.auto_approve_tools.contains(&"read_file".to_string()));
    }

    #[test]
    fn expand_run_policy_empty_inventory_is_empty() {
        let p = expand_run_policy(&[], "all", &[]);
        assert!(p.is_empty());
    }

    // -----------------------------------------------------------------
    // Classification-source honesty (thin-client conformance, lane 2):
    // the name table is a #FALLBACK — approval surfaces label it, so the
    // source accessor must mirror classify_call_with's decision order.
    // (The GitProof source retired with the client proof, c5057 — the
    // read-only-git decision is runtime's git_read_only@v1 refiner now.)
    // -----------------------------------------------------------------

    #[test]
    fn classify_source_mirrors_the_decision_order() {
        let classes = vec![cls("mcp::search", "auto"), cls("execute_command", "ask")];
        // Server fact present → Server, and the tier is the served one.
        assert_eq!(
            classify_source("mcp::search", &classes),
            ClassSource::Server
        );
        // A git command with a served execute_command class: the server
        // fact decides (no client proof exists — the refiner approves
        // proven reads server-side before a wait ever reaches us).
        assert_eq!(
            classify_source("execute_command", &classes),
            ClassSource::Server
        );
        assert_eq!(
            classify_source("execute_command", &classes),
            ClassSource::Server
        );
        // No server fact → the #FALLBACK name table (known or unknown name).
        assert_eq!(
            classify_source("read_file", &classes),
            ClassSource::NameTable
        );
        assert_eq!(
            classify_source("brand_new_tool", &[]),
            ClassSource::NameTable
        );
    }

    #[test]
    fn batch_name_table_names_lists_only_fallback_classified_calls() {
        let classes = vec![cls("mcp::search", "auto")];
        let batch = vec![
            call("mcp::search"), // server fact
            call("read_file"),   // name table
            call("read_file"),   // duplicate: deduped
            json!({"name": "execute_command",
                   "arguments": {"command": "git status"}}), // name table now (proof retired)
            json!({"arguments": {}}), // unnamed: fail-closed client decision
        ];
        assert_eq!(
            batch_name_table_names(&batch, &classes),
            vec![
                "(unnamed)".to_string(),
                "execute_command".to_string(),
                "read_file".to_string()
            ]
        );
        // A fully server-classified batch labels nothing.
        assert!(batch_name_table_names(&[call("mcp::search")], &classes).is_empty());
        // No inventory at all: every named call is name-table classified.
        assert_eq!(
            batch_name_table_names(&[call("write_file")], &[]),
            vec!["write_file".to_string()]
        );
    }
}
