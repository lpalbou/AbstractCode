//! Headless one-shot runs: `abstractcode-tui exec "<prompt>"`.
//!
//! Prints transcript items as they fold, resolves waits from CLI policy
//! (`--approve-all` approves tool batches; ask-user waits get an honest
//! refusal so unattended runs never stall), and exits 0/1/124.
//!
//! This path follows runs by POLLING the REST ledger (the TUI uses SSE), so
//! the two clients between them exercise both transports.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use abstracttui::reactive::Backoff;
use serde_json::json;

use crate::cli::Args;
use crate::config;
use crate::discovery::{
    agent_workflow_ids_from_bundles, agent_workflows_from_bundles, choose_workflow,
    tools_from_discovery,
};
use crate::gateway::GatewayClient;
use crate::run_input::{build_input_data, StartOpts};
use crate::tool_policy::{self, ToolClass};
use crate::transcript::{Fold, FoldEffect, Item, ToolStatus, WaitKind};

const ASK_REFUSAL: &str =
    "No interactive user is present (headless run). Proceed with your best judgment and finish the task.";

/// Decide how headless `exec` should answer an ask-user wait.
///
/// Most asks are genuine operator questions and must still get the honest
/// headless refusal. One narrow exception is the conclude gate emitted by
/// review/coding workflows: it does not ask for missing information, it asks
/// whether the operator accepts the already-produced answer or wants to steer
/// another cycle. Refusing that prompt headlessly creates a client-side loop
/// that keeps feeding the same refusal back as steering.
///
/// So headless `exec` auto-accepts ONLY the conclude-confirmation shape and
/// keeps refusing every other ask. The classifier is intentionally narrow: it
/// requires an explicit `'accept' to finish` instruction plus another
/// conclude-gate cue, so an unrelated user question never gets a false
/// "accept".
pub fn resolve_headless_ask(prompt: &str) -> (&'static str, &'static str) {
    let lower = prompt.to_ascii_lowercase();
    let names_accept = lower.contains("reply 'accept' to finish")
        || lower.contains("reply \"accept\" to finish")
        || lower.contains("type 'accept' to finish")
        || lower.contains("type \"accept\" to finish");
    let looks_like_conclude_gate = lower.contains("believes the task is done")
        || lower.contains("task is done after")
        || lower.contains("fed straight into the next cycle")
        || lower.contains("operator steering");
    if names_accept && looks_like_conclude_gate {
        (
            "accept",
            "answering with the headless auto-accept for the conclude gate",
        )
    } else {
        (ASK_REFUSAL, "answering with the headless refusal")
    }
}

/// The resolution of one approval wait under headless policy: whether it
/// is approved, the resume payload, and a one-line human log naming WHY
/// (the bridge lesson — a controller that gates must say why, so the
/// model reads the real rule, not a canned hint). Extracted as a PURE
/// function so the deny-reason path is testable without a live gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalResolution {
    pub approved: bool,
    pub payload: serde_json::Value,
    pub log: String,
}

/// Decide an approval wait for a headless run: the PERSISTED permissions
/// level decides (server truth preferred via `classes`, else the name
/// table); `--permissions all` is just the level at its top. Above-level
/// batches DENY with the real rule named (a headless run has no one to
/// ask), and every payload stamps `approved_by: "policy"` + the rule —
/// the R3 ledger-honesty convention (c5028).
pub fn resolve_approval(
    tool_calls: &[serde_json::Value],
    accepted_raw: &str,
    overrides: &[(String, String)],
    classes: &[ToolClass],
) -> ApprovalResolution {
    let accepted = tool_policy::Tier::parse_or_default(accepted_raw);
    let names: Vec<String> = tool_calls
        .iter()
        .filter_map(|tc| tc.get("name").and_then(|v| v.as_str()).map(str::to_string))
        .collect();
    // HONEST-REASON GAP 1 (Lane A): an empty/nameless batch used to deny
    // with a self-contradictory "needs tier 'all', accepted 'all'" —
    // name the real cause.
    if names.is_empty() {
        let reason = "Denied: unreadable tool batch (no named calls) — denied fail-closed";
        return ApprovalResolution {
            approved: false,
            payload: json!({"approved": false, "approved_by": "policy",
                             "rule": "fail-closed on unreadable batch", "reason": reason}),
            log: format!("DENYING (unreadable batch: no named calls) tool batch [{names:?}]"),
        };
    }
    // HONEST-REASON GAP 2 (Lane A): a served-disabled tool denies naming
    // ITS GATE, never a tier arithmetic that reads as contradiction.
    let disabled: Vec<(String, String)> = names
        .iter()
        .filter_map(|n| {
            classes
                .iter()
                .find(|c| &c.name == n && c.served_disabled)
                .map(|c| (n.clone(), c.enable_gate.clone()))
        })
        .collect();
    if !disabled.is_empty() {
        let detail = disabled
            .iter()
            .map(|(n, g)| {
                if g.is_empty() {
                    n.clone()
                } else {
                    format!("{n} (gate: {g})")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let reason = format!(
            "Denied: {detail} — disabled on this gateway; enabling is the operator's \
             gate, not an approval question"
        );
        return ApprovalResolution {
            approved: false,
            payload: json!({"approved": false, "approved_by": "policy",
                             "rule": "served-disabled clamp", "reason": reason}),
            log: format!(
                "DENYING (served-disabled: {detail}) tool batch [{}]",
                names.join(", ")
            ),
        };
    }
    let approved =
        tool_policy::batch_auto_approves_with(tool_calls, accepted_raw, overrides, classes);
    let needed = tool_policy::batch_tier_with(tool_calls, classes);
    // Distinguish the two denial causes so the reason is HONEST: a level
    // gap (the batch needs a higher level than accepted) vs a per-tool
    // 'ask' pin (the level would admit it, but the user pinned it to
    // always ask — headlessly there is no one to ask, so it denies; the
    // pin is a deliberate boundary that even --permissions all does not
    // silently dissolve). Citing a level gap when the real cause is a
    // pin would contradict itself (needed <= accepted, yet denied).
    let ask_pinned: Vec<&str> = names
        .iter()
        .filter(|n| overrides.iter().any(|(name, d)| name == *n && d == "ask"))
        .map(String::as_str)
        .collect();
    let denied_by_pin = !approved && needed <= accepted && !ask_pinned.is_empty();
    let rule = if approved {
        format!("within permissions '{}'", accepted.label())
    } else if denied_by_pin {
        format!("per-tool 'ask' pin: {}", ask_pinned.join(", "))
    } else {
        format!(
            "batch needs '{}', permissions '{}'",
            needed.label(),
            accepted.label()
        )
    };
    let mut log = format!(
        "{} tool batch [{}]",
        if approved {
            format!("approving ({rule})")
        } else {
            format!("DENYING ({rule})")
        },
        names.join(", ")
    );
    // Classification-source honesty (thin-client conformance, class ii):
    // when the LEVEL decided (not a pin) and any call's tier came from
    // the client's #FALLBACK name table rather than gateway-served
    // approval facts, the log says so — a decision from client
    // heuristics must never read as server truth.
    if !denied_by_pin {
        let fallback = tool_policy::batch_name_table_names(tool_calls, classes);
        if !fallback.is_empty() {
            log.push_str(&format!(
                " · tier from the client name table for: {} (#FALLBACK — no gateway approval facts served)",
                fallback.join(", ")
            ));
        }
    }
    let payload = if approved {
        json!({"approved": true, "approved_by": "policy", "rule": rule})
    } else if denied_by_pin {
        json!({"approved": false, "approved_by": "policy", "rule": rule, "reason": format!(
            "Denied: {} pinned to always ask, and this is a headless run with no \
             interactive user (prefs.json tool_approval overrides; remove the pin \
             or run interactively)",
            ask_pinned.join(", ")
        )})
    } else {
        json!({"approved": false, "approved_by": "policy", "rule": rule, "reason": format!(
            "Denied: this batch needs permissions '{}' but the accepted level is '{}' \
             (headless policy from prefs.json tool_approval; --permissions <level> raises it)",
            needed.label(), accepted.label()
        )})
    };
    ApprovalResolution {
        approved,
        payload,
        log,
    }
}

/// Refusal message when an EXPLICITLY requested `--workflow` doesn't match
/// what `choose_workflow` resolved (i.e. the request fell through to the
/// basic-agent/first fallback). Pure so the headless refusal is testable
/// without a gateway. `None` = the chosen workflow satisfies the request.
///
/// Match rule mirrors the resolver: `bundle` alone matches any flow in the
/// bundle; `bundle:flow` requires both halves.
pub fn explicit_workflow_mismatch(
    requested_raw: &str,
    chosen: &crate::store::Workflow,
    available: &[crate::store::Workflow],
) -> Option<String> {
    explicit_workflow_mismatch_diagnosed(requested_raw, chosen, available, &[])
}

/// `explicit_workflow_mismatch` with the full catalog for DIAGNOSIS: the
/// `(bundle, flow, interfaces)` rows from `all_entrypoints_from_bundles`.
///
/// The refusal is unchanged in verdict, only in truthfulness. Three cases
/// the old single sentence collapsed into "not found on this gateway":
///  * the flow IS installed but behind a non-agent interface (e.g. the
///    `abstractcode.coding.v1` pipeline entrypoints) — say which interface;
///  * the bundle is installed and holds several agent flows, so a
///    bundle-only ref is AMBIGUOUS — list them and ask for `bundle:flow`;
///  * genuinely absent — the original message.
pub fn explicit_workflow_mismatch_diagnosed(
    requested_raw: &str,
    chosen: &crate::store::Workflow,
    available: &[crate::store::Workflow],
    catalog: &[(String, String, Vec<String>)],
) -> Option<String> {
    let (b, f) = crate::cli::split_workflow_ref(requested_raw);
    let satisfied = chosen.bundle_id == b && f.as_deref().is_none_or(|flow| chosen.flow_id == flow);
    if satisfied {
        return None;
    }
    // Installed-but-wrong-interface: the exact ref exists in the catalog and
    // simply does not carry the agent interface this client runs.
    if let Some((_, _, ifs)) = catalog
        .iter()
        .find(|(cb, cf, _)| cb == &b && f.as_deref().is_some_and(|flow| cf == flow))
    {
        let ifs_txt = if ifs.is_empty() {
            "none declared".to_string()
        } else {
            ifs.join(", ")
        };
        let mut msg = format!(
            "✗ workflow '{requested_raw}' IS installed but does not carry the agent interface \
             '{iface}' — refusing to run a different agent\n  its interfaces: {ifs_txt}",
            iface = crate::discovery::AGENT_INTERFACE_V1
        );
        let siblings: Vec<&crate::store::Workflow> =
            crate::discovery::flows_in_bundle(available, &b);
        if !siblings.is_empty() {
            msg.push_str("\n  agent entrypoints in this bundle:");
            for w in siblings {
                msg.push_str(&format!("\n    {}:{}", w.bundle_id, w.flow_id));
            }
        }
        return Some(msg);
    }
    // Bundle-only ref naming an INSTALLED bundle that simply has no agent
    // entrypoint. Without this the commonest spelling (`--workflow coder`)
    // reported "not found on this gateway" for a bundle sitting right there —
    // the same lie the diagnosed refusal exists to end, just one input shape
    // further along.
    if f.is_none() && crate::discovery::flows_in_bundle(available, &b).is_empty() {
        let eps: Vec<&(String, String, Vec<String>)> =
            catalog.iter().filter(|(cb, _, _)| cb == &b).collect();
        if !eps.is_empty() {
            let mut msg = format!(
                "✗ bundle '{b}' is installed but has no '{iface}' entrypoint —                  refusing to run a different agent\n  its entrypoints:",
                iface = crate::discovery::AGENT_INTERFACE_V1
            );
            for (_, flow, ifs) in eps {
                let i = if ifs.is_empty() {
                    "none declared".to_string()
                } else {
                    ifs.join(", ")
                };
                msg.push_str(&format!("\n    {b}:{flow}  ({i})"));
            }
            return Some(msg);
        }
    }
    // Ambiguous bundle-only ref: the bundle holds several agent flows and
    // none is named after it, so no flow is the operator's evident intent.
    if f.is_none() {
        let siblings = crate::discovery::flows_in_bundle(available, &b);
        if siblings.len() > 1 {
            let mut msg = format!(
                "✗ workflow '{requested_raw}' is ambiguous — bundle '{b}' has {n} agent \
                 entrypoints and none is named after the bundle; name one as 'bundle:flow'",
                n = siblings.len()
            );
            for w in siblings {
                msg.push_str(&format!("\n    {}:{}", w.bundle_id, w.flow_id));
            }
            return Some(msg);
        }
    }
    let mut msg = format!(
        "✗ workflow '{requested_raw}' not found on this gateway — refusing to run a different agent"
    );
    if available.is_empty() {
        msg.push_str("\n  (no agent workflows available)");
    } else {
        msg.push_str("\n  available:");
        for w in available.iter().take(12) {
            msg.push_str(&format!("\n    {}:{}", w.bundle_id, w.flow_id));
        }
        if available.len() > 12 {
            msg.push_str(&format!("\n    … and {} more", available.len() - 12));
        }
    }
    Some(msg)
}

pub fn run(args: &Args) -> i32 {
    let prompt = match args.prompt.as_deref() {
        Some(p) if !p.trim().is_empty() => p.trim().to_string(),
        _ => {
            eprintln!("exec needs a prompt: abstractcode-tui exec \"<prompt>\"");
            return 2;
        }
    };
    let conn = config::resolve_connection(args.gateway.as_deref(), args.token.as_deref());
    let client = GatewayClient::new(&conn.base_url, conn.token.as_deref());

    // Resolve the workflow: flag > prefs > basic-agent > first agent flow.
    let prefs = config::Prefs::load();
    let (pref_bundle, pref_flow) = match args.workflow.as_deref() {
        Some(raw) => {
            let (b, f) = crate::cli::split_workflow_ref(raw);
            (Some(b), f)
        }
        None => (prefs.bundle_id.clone(), prefs.flow_id.clone()),
    };
    let bundles = match client.list_bundles() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("✗ catalog: {e}");
            if e.status == Some(401) || e.status == Some(403) {
                eprintln!("  hint: token rejected — run `abstractcode-tui login` (or check ABSTRACTGATEWAY_AUTH_TOKEN)");
            }
            return 1;
        }
    };
    let workflows = agent_workflows_from_bundles(&bundles);
    let workflow = match choose_workflow(&workflows, pref_bundle.as_deref(), pref_flow.as_deref()) {
        Some(w) => w,
        None => {
            eprintln!("✗ no agent workflows (interface abstractcode.agent.v1) on this gateway");
            return 1;
        }
    };
    // An EXPLICIT --workflow that doesn't exist must refuse, never silently
    // substitute basic-agent (final-verifier finding 1, 2026-07-23): a
    // script pinning a specific agent would otherwise run a different one
    // with exit 0 and no warning. The prefs lane keeps the fallback — a
    // stale saved preference degrading to the default is the interactive
    // contract, and the header names what ran.
    if let Some(raw) = args.workflow.as_deref() {
        let catalog = crate::discovery::all_entrypoints_from_bundles(&bundles);
        if let Some(msg) =
            explicit_workflow_mismatch_diagnosed(raw, &workflow, &workflows, &catalog)
        {
            eprintln!("{msg}");
            return 2;
        }
    }

    // Ungated safety guard (design + adversary): an unattended run that
    // skips the workflow's human pauses must NOT also run with an
    // unstated tool posture — that is exactly when an unwatched tool
    // could touch the machine. Refuse `--ungated` unless the operator
    // chose a tool posture explicitly on the SAME command line, and
    // print an unattended banner so the combination is never silent.
    if args.ungated {
        if args.permissions.is_none() {
            eprintln!(
                "✗ --ungated requires an explicit --permissions <read|write|all>: an unattended \
                 run skips the workflow's approval pauses, so the tool posture must be chosen, \
                 never defaulted."
            );
            return 2;
        }
        eprintln!(
            "⚠ UNATTENDED: gating_mode=auto (no human approval pauses) with permissions={}. \
             Every shell step runs under that posture without asking.",
            args.permissions.as_deref().unwrap_or("")
        );
    }

    let session_id = args.session.clone().unwrap_or_else(config::mint_session_id);
    // Tool inventory for the policy expansion + the wait-loop classifier
    // (server truth preferred, else the name table). Best-effort: a failed
    // discovery just leaves the name table + an empty server-side policy;
    // the client-side wait loop still enforces the tier.
    // ONE projection (`From<&ToolInfo>` in store.rs) — the served-disabled
    // clamp and the rank-band floor reach exec runs through the same
    // mapping the TUI uses; a field added there reaches here for free.
    let classes: Vec<ToolClass> = match client.discovery_tools() {
        Ok(v) => tools_from_discovery(&v)
            .iter()
            .map(ToolClass::from)
            .collect(),
        Err(_) => Vec::new(),
    };
    // ONE resolved level feeds BOTH halves (Lane A / c5028): the
    // server-side expansion below AND the wait-loop resolver. Resolution:
    // --permissions flag > prefs baseline (the same file /permissions
    // writes). Flag `--require-approval` names merge over prefs pins as
    // `ask` (flag wins per name) and flow into require_approval_tools +
    // the resolver — a deliberate gate even at --permissions all.
    let policy_tier = args
        .permissions
        .clone()
        .unwrap_or_else(|| prefs.tool_accepted_tier.clone());
    let mut effective_overrides: Vec<(String, String)> = prefs
        .tool_overrides
        .iter()
        .filter(|(n, _)| !args.require_approval.contains(n))
        .cloned()
        .collect();
    for name in &args.require_approval {
        effective_overrides.push((name.clone(), "ask".to_string()));
    }
    let tool_policy = tool_policy::expand_run_policy(&classes, &policy_tier, &effective_overrides);
    // `--attach` uploads BEFORE the run exists: any failure exits 1 with
    // nothing spent. Typed args accept relative paths (explicit intent —
    // resolved against cwd by canonicalize), `~`, quotes, file://.
    // Uploads run INSIDE the --timeout budget (the deadline is minted
    // before this loop and threaded to the wait loop below) — N uploads
    // must not stretch a bounded invocation before its clock starts.
    // `#[WARNING:TIMEOUT]` exec wall-clock safeguard (ADR-0014/0027).
    // `--timeout 0` = NO client-side cap: a run that should never be
    // interrupted gets a deadline ~10 years out rather than a special case at
    // every comparison below. Note the ordering — `.max(10)` must not apply to
    // 0, or "unlimited" would silently become the shortest cap in the program.
    let exec_deadline = if args.timeout_secs == 0 {
        Instant::now() + Duration::from_secs(315_360_000)
    } else {
        Instant::now() + Duration::from_secs(args.timeout_secs.max(10))
    };
    let mut attachment_refs: Vec<serde_json::Value> = Vec::new();
    for raw in &args.attach {
        if Instant::now() > exec_deadline {
            eprintln!("✗ attach: --timeout exhausted before all uploads completed");
            return 1;
        }
        let expanded = crate::paths::expand_path_spelling(raw);
        let canon = match std::fs::canonicalize(&expanded) {
            Ok(p) => p,
            Err(_) => {
                eprintln!("✗ attach: no such file: {expanded}");
                return 1;
            }
        };
        if !canon.is_file() {
            eprintln!(
                "✗ attach: {} is not a regular file (directories attach individually)",
                canon.display()
            );
            return 1;
        }
        let name = canon
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| expanded.clone());
        // Safety ceiling (the TUI's attach-time rule): the read buffers
        // whole + one multipart copy — a mis-typed huge path must not
        // allocate gigabytes before the server can refuse it.
        match std::fs::metadata(&canon) {
            Ok(m) if m.len() > crate::ui::attachments::CLIENT_SAFETY_CEILING_BYTES => {
                eprintln!(
                    "✗ attach: {name} is {} — over the client's {} safety ceiling",
                    crate::paths::human_size(m.len()),
                    crate::paths::human_size(crate::ui::attachments::CLIENT_SAFETY_CEILING_BYTES)
                );
                return 1;
            }
            _ => {}
        }
        let bytes = match std::fs::read(&canon) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("✗ attach: {name}: {e}");
                return 1;
            }
        };
        let size = crate::paths::human_size(bytes.len() as u64);
        match client.upload_attachment(&session_id, &name, &bytes) {
            Ok(r) => {
                let id = r
                    .get("$artifact")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("?");
                let id8 = id.get(..8).unwrap_or(id);
                eprintln!("attached {name} ({size}) as {id8}…");
                attachment_refs.push(r);
            }
            Err(e) => {
                eprintln!("✗ attach: {name}: upload failed — {e}");
                return 1;
            }
        }
    }
    let workspace_root = if args.no_workspace {
        None
    } else {
        args.workspace.clone().or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.display().to_string())
        })
    };
    // Project instructions (AGENTS.md) for the run's workspace — parity with
    // the Python client, which has always injected them. Scoped to the
    // workspace: `--no-workspace` runs have no project to read conventions
    // from. `--no-project-context` opts out for byte-exact scripted runs.
    let project_context = crate::project_context::resolve_project_context(
        workspace_root.as_deref(),
        args.no_project_context,
        |line| eprintln!("⚠ {line}"),
        |sources, chars| eprintln!("project context: {sources} ({chars} chars)"),
    );
    let opts = StartOpts {
        attachments: attachment_refs,
        provider: args.provider.clone().unwrap_or_default(),
        model: args.model.clone().unwrap_or_default(),
        // Headless parity: --reasoning rides verbatim (validated at
        // parse); prefs deliberately NOT consulted here — exec runs are
        // scripted, and a sticky TUI preference silently changing a
        // script's reasoning posture is the ambient-config class.
        reasoning: args.reasoning.clone().unwrap_or_default(),
        // --ungated -> gating_mode=auto (guarded above); absent = the
        // workflow's default (gated).
        gating_mode: if args.ungated {
            "auto".to_string()
        } else {
            String::new()
        },
        workspace_root: workspace_root.clone(),
        // Config-first for headless runs: flags win, prefs.json fills in
        // (the same file the TUI's /workspace edits).
        workspace_mode: args
            .workspace_mode
            .clone()
            .or_else(|| prefs.workspace_mode.clone()),
        workspace_allowed: prefs.workspace_allowed.clone(),
        max_iterations: args.max_iterations,
        max_iterations_explicit: args.max_iterations_explicit,
        system: String::new(),
        system_prompt_extra: project_context,
        // Verifier-before-conclude: on unless `--no-review`. Headless runs
        // are exactly where nobody is watching for a too-early "done", so
        // the posture is always STATED on the wire, never left to whichever
        // server default happens to be in force.
        review_mode: Some(args.review.unwrap_or(crate::cli::DEFAULT_REVIEW_MODE)),
        review_capable: crate::discovery::workflow_is_review_capable(&workflow.bundle_id),
        review_max_rounds: args.review_rounds,
        // One-shot runs have no prior client transcript; cross-invocation
        // continuity rides the server-side session seed.
        messages: Vec::new(),
        // Headless runs take the workflow's own tool defaults; the /tools
        // and /skills selections are interactive-session preferences.
        tools: None,
        skills: Vec::new(),
        // Goal runs are a TUI surface (/goal); exec stays one-shot.
        goal: None,
        tool_policy,
        // CTX-0 declared window: flag wins, prefs.json fills in (the
        // headless config-first rule above); 0 = undeclared, key omitted.
        context_window: if args.max_tokens > 0 {
            args.max_tokens
        } else {
            prefs.context_window
        },
        // `--no-prompt-cache` states the OFF posture; absent = server truth.
        prompt_cache: if args.no_prompt_cache {
            Some(false)
        } else {
            None
        },
    };
    let mut input = build_input_data(&prompt, &opts);
    // `--param` pins ride input_data top-level — on_flow_start resolves
    // declared pins input-first, so a key matching a workflow's start pin
    // reaches it; unknown keys are inert. Scalars are typed (a `number`
    // pin never sees the string "16"); everything else rides verbatim.
    for (k, v) in &args.params {
        let t = v.trim();
        let val = if t.eq_ignore_ascii_case("true") {
            serde_json::Value::Bool(true)
        } else if t.eq_ignore_ascii_case("false") {
            serde_json::Value::Bool(false)
        } else if let Ok(n) = t.parse::<i64>() {
            serde_json::json!(n)
        } else if let Ok(fl) = t.parse::<f64>() {
            serde_json::json!(fl)
        } else {
            serde_json::Value::String(v.clone())
        };
        input[k.as_str()] = val;
    }
    let run_id = match client.start_run(
        &workflow.flow_id,
        Some(&workflow.bundle_id),
        Some(&session_id),
        input,
    ) {
        Ok(rid) => rid,
        Err(e) => {
            eprintln!("✗ start: {e}");
            if e.status == Some(401) || e.status == Some(403) {
                eprintln!("  hint: token rejected — run `abstractcode-tui login` (or check ABSTRACTGATEWAY_AUTH_TOKEN)");
            }
            return 1;
        }
    };
    eprintln!(
        "run {run_id} · workflow {}:{} · session {session_id}",
        workflow.bundle_id, workflow.flow_id
    );

    let mut fold = Fold::new();
    // Declare the catalog's agent entrypoint ids (the lane-1 fold contract)
    // BEFORE folding any record. The TUI worker does this at every catalog
    // load; headless exec never did, so `is_agent_workflow` fell back to
    // matching the `visual_react_agent_` prefix alone and a wrapper bundle
    // spawning a catalogued agent child degraded to the labeled-cycle
    // #FALLBACK instead of recognizing its answer source.
    fold.set_agent_workflows(agent_workflow_ids_from_bundles(&bundles));
    fold.begin_run(&run_id);
    fold.push_item(Item::User { text: prompt });

    let mut cursors: HashMap<String, u64> = HashMap::new();
    cursors.insert(run_id.clone(), 0);
    let mut printed = 0usize;
    let mut printed_tool_state: HashMap<usize, ToolStatus> = HashMap::new();
    // Minted BEFORE the attach uploads (P2-7): one budget bounds the
    // whole invocation, uploads included.
    let deadline = exec_deadline;
    // Gateway-error pacing: the same jittered exponential the TUI's
    // stream threads use (engine 0.2.6 `reactive::Backoff`), replacing
    // three fixed 800ms sleeps — fleet `exec` (the documented swarm
    // bridges) is the real multi-PROCESS thundering herd, and the
    // per-instance entropy seed decorrelates processes too. Reset on
    // every successful ledger read; the 300ms loop cadence below is a
    // POLL cadence, not error backoff, and stays fixed. Every draw is
    // CLAMPED to the remaining --timeout budget (cycle-2 adversary
    // P2-1: three unclamped ≤30s draws could land after the deadline
    // passed and stretch `--timeout 30` to ~90s before the loop-top
    // check returned 124).
    let mut backoff = Backoff::default();
    let backoff_sleep = move |backoff: &mut Backoff, deadline: Instant| {
        let draw = backoff.next_delay();
        let budget = deadline.saturating_duration_since(Instant::now());
        std::thread::sleep(draw.min(budget));
    };

    print_new(&fold, &mut printed, &mut printed_tool_state);

    loop {
        if Instant::now() > deadline {
            eprintln!(
                "✗ timeout after {}s (the run stays durable on the gateway: {run_id})",
                args.timeout_secs
            );
            return 124;
        }
        // Drain ledger pages for every followed run.
        let run_ids: Vec<String> = cursors.keys().cloned().collect();
        for rid in run_ids {
            if Instant::now() > deadline {
                break;
            }
            let after = *cursors.get(&rid).unwrap_or(&0);
            match client.get_ledger(&rid, after, 500) {
                Ok((items, next)) => {
                    backoff.reset();
                    cursors.insert(rid.clone(), next);
                    for rec in items {
                        for fx in fold.apply(&rid, &rec) {
                            match fx {
                                FoldEffect::FollowRun(sub) => {
                                    cursors.entry(sub).or_insert(0);
                                }
                                FoldEffect::FetchImage { artifact_id, .. } => {
                                    eprintln!("· image artifact {artifact_id} (view it in the TUI or the web UI)");
                                }
                                FoldEffect::FetchAnswer {
                                    run_id: art_run,
                                    artifact_id,
                                } => {
                                    // Offloaded final answer (>256 KB output):
                                    // fetch synchronously — headless output must
                                    // print the words, not a placeholder. Shared
                                    // retry helper (Lane B fix): one transport
                                    // blip must not cost the printed answer, and
                                    // failure text stays URL-free (exec output is
                                    // routinely piped back into agents).
                                    let outcome = crate::runner::fetch_answer_with_retry(
                                        &client,
                                        &art_run,
                                        &artifact_id,
                                    );
                                    fold.resolve_offloaded_answer(&artifact_id, outcome);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("· gateway read failed ({e}); retrying");
                    backoff_sleep(&mut backoff, deadline);
                }
            }
        }
        print_new(&fold, &mut printed, &mut printed_tool_state);

        // Resolve pending waits per policy: --approve-all approves
        // everything; otherwise the PERSISTED tier policy decides
        // (prefs.json tool_approval — the same knob as the TUI's
        // `/tools tier`). Above-tier batches DENY with the real rule
        // named, so the model reads why (bridge lesson: controllers
        // that gate must say why).
        if let Some(wait) = fold.pending_wait.clone() {
            match &wait.kind {
                WaitKind::Approval { tool_calls } => {
                    let resolution =
                        resolve_approval(tool_calls, &policy_tier, &effective_overrides, &classes);
                    eprintln!("{}", resolution.log);
                    match client.resume(&wait.run_id, &wait.wait_key, resolution.payload) {
                        Ok(_) => {
                            fold.wait_answered(&wait.wait_key, &wait.step_id);
                            fold.mark_wait_tools(resolution.approved);
                        }
                        Err(e) => {
                            eprintln!("· resume failed ({e}); retrying");
                            backoff_sleep(&mut backoff, deadline);
                        }
                    }
                }
                WaitKind::Ask { prompt } => {
                    eprintln!("agent asks: {prompt}");
                    let (response, log) = resolve_headless_ask(prompt);
                    eprintln!("{log}");
                    match client.resume(&wait.run_id, &wait.wait_key, json!({"response": response}))
                    {
                        Ok(_) => {
                            fold.wait_answered(&wait.wait_key, &wait.step_id);
                        }
                        Err(e) => {
                            eprintln!("· resume failed ({e}); retrying");
                            backoff_sleep(&mut backoff, deadline);
                        }
                    }
                }
            }
        }

        // The answer landing finishes the turn — wrapper bundles may keep
        // helper subflows (status watchers) polling long after the agent
        // answered, so root-terminal alone is not the finish line.
        if fold.finished {
            let stats = &fold.stats;
            let failed = fold.failed;
            let budget = fold.budget_exhausted;
            // Name the outcome honestly on the ONE line a piped caller reads.
            // "done" for a run the agent was cut off mid-task is the same lie
            // the chrome line told (transcript.rs `push_done_summary`).
            let head = match budget {
                Some(n) if n > 0 => format!("stopped: iteration budget ({n})"),
                Some(_) => "stopped: iteration budget".to_string(),
                None => "done".to_string(),
            };
            eprintln!(
                "{head} · {} llm calls · {} tools · {} (run {run_id} finalizes on the gateway)",
                stats.llm_calls,
                stats.tool_calls,
                fmt_stats_tokens(stats)
            );
            if failed {
                return 1;
            }
            return if budget.is_some() {
                EXIT_ITERATION_BUDGET
            } else {
                0
            };
        }

        // Terminal check on the ANSWER-SOURCE agent subrun (the
        // failed-agent P0, live tree 76fc3fcb…/9c5cad22…): the wrapper
        // root ABSORBS the agent's terminal failure and parks forever on
        // its status poller, so the root check below never fires. Drain
        // that run's ledger first so a late conclusion record folds
        // before the terminal verdict; `subrun_terminal` no-ops when the
        // drain already concluded the turn.
        if !fold.finished {
            if let Some(agent_rid) = fold.answer_run_id().map(str::to_string) {
                if let Ok(v) = client.get_run(&agent_rid) {
                    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
                    if matches!(status, "completed" | "failed" | "cancelled") {
                        let mut fully_drained = true;
                        loop {
                            let after = *cursors.get(&agent_rid).unwrap_or(&0);
                            match client.get_ledger(&agent_rid, after, 1000) {
                                Ok((items, next)) => {
                                    cursors.insert(agent_rid.clone(), next);
                                    let short = items.len() < 1000;
                                    for rec in items {
                                        let _ = fold.apply(&agent_rid, &rec);
                                    }
                                    if short {
                                        break;
                                    }
                                }
                                Err(_) => {
                                    fully_drained = false;
                                    break;
                                }
                            }
                        }
                        // The "completed without a readable final answer"
                        // verdict requires the WHOLE ledger — a drain cut
                        // by a network blip may have missed the real
                        // conclusion record, and nothing re-reads after
                        // `finished` (cycle-2 review F3). Skip the verdict
                        // this sweep and retry; failed/cancelled verdicts
                        // are status-truth and need no ledger.
                        if status != "completed" || fully_drained {
                            let was_finished = fold.finished;
                            fold.subrun_terminal(&agent_rid, status);
                            print_new(&fold, &mut printed, &mut printed_tool_state);
                            // Exit-code truth (cycle-2 review F2): a turn
                            // CONCLUDED BY a cancelled answer-source is a
                            // cancel (130, root-cancel parity) — the fold
                            // deliberately keeps `failed == false` for
                            // cancels, so the finished branch's 0 would
                            // read "answer produced" to scripts. A drain
                            // that folded the real answer (finished before
                            // the verdict) still reports 0/1 normally.
                            if fold.finished && !was_finished && status == "cancelled" {
                                let stats = &fold.stats;
                                eprintln!(
                                    "done: cancelled · {} llm calls · {} tools · {}",
                                    stats.llm_calls,
                                    stats.tool_calls,
                                    fmt_stats_tokens(stats)
                                );
                                return exit_code_for_status("cancelled");
                            }
                            if fold.finished {
                                continue; // the finished branch above reports
                            }
                        }
                    }
                }
            }
        }

        // Terminal check on the root (covers failures and cancellation).
        match client.get_run(&run_id) {
            Ok(v) => {
                let status = v
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
                    // Final drain so the answer is not lost to a race.
                    let run_ids: Vec<String> = cursors.keys().cloned().collect();
                    for rid in run_ids {
                        loop {
                            let after = *cursors.get(&rid).unwrap_or(&0);
                            match client.get_ledger(&rid, after, 1000) {
                                Ok((items, next)) => {
                                    cursors.insert(rid.clone(), next);
                                    let short = items.len() < 1000;
                                    for rec in items {
                                        let _ = fold.apply(&rid, &rec);
                                    }
                                    if short {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    fold.run_terminal(&status);
                    print_new(&fold, &mut printed, &mut printed_tool_state);
                    let stats = &fold.stats;
                    eprintln!(
                        "done: {status} · {} llm calls · {} tools · {}",
                        stats.llm_calls,
                        stats.tool_calls,
                        fmt_stats_tokens(stats)
                    );
                    return exit_code_for_status_with_verdict(
                        &status,
                        fold.budget_exhausted.is_some(),
                    );
                }
            }
            Err(e) => eprintln!("· status read failed ({e}); retrying"),
        }

        std::thread::sleep(Duration::from_millis(300));
    }
}

/// Exit code for a run that STOPPED on its iteration budget instead of
/// finishing. Distinct from both success and failure: nothing failed, and
/// nothing finished — the agent was interrupted with work outstanding.
///
/// A dedicated code exists because `0` here is actively harmful. Every
/// harness in `scripts/` scores success as `exit_code == 0`, so a
/// budget-truncated Zelda run used to be recorded as a PASS — measuring an
/// interrupted agent as a competent one. 125 is outside the shell's signal
/// range and unused by this binary.
pub const EXIT_ITERATION_BUDGET: i32 = 125;

/// The documented exec exit-code truth table for a run/answer-source
/// reaching a terminal status: completed → 0, cancelled → 130 (script
/// convention: a cancel is neither success nor failure), anything else
/// (failed, unexpected) → 1. ONE authority for both terminal branches —
/// pre-fix, a CANCELLED answer-source subrun concluded through the
/// generic finished branch and exited 0 (cycle-2 review F2).
///
/// `budget_exhausted` overrides a `completed` status: the RUN completed, the
/// TURN did not (see `EXIT_ITERATION_BUDGET`).
pub fn exit_code_for_status_with_verdict(status: &str, budget_exhausted: bool) -> i32 {
    if status == "completed" && budget_exhausted {
        return EXIT_ITERATION_BUDGET;
    }
    match status {
        "completed" => 0,
        "cancelled" => 130,
        _ => 1,
    }
}

/// Status-only door for callers with no fold in hand (kept so the truth
/// table has one home).
pub fn exit_code_for_status(status: &str) -> i32 {
    exit_code_for_status_with_verdict(status, false)
}

/// Token summary line: honest fallback to the cumulative total when the
/// provider reports no input/output split (bug (e): the coder-run
/// provider fills only `total_tokens`).
fn fmt_stats_tokens(stats: &crate::transcript::Stats) -> String {
    if stats.input_tokens == 0 && stats.output_tokens == 0 && stats.total_tokens > 0 {
        format!("{} tk total", stats.total_tokens)
    } else {
        format!("{}↑ {}↓ tk", stats.input_tokens, stats.output_tokens)
    }
}

fn print_new(fold: &Fold, printed: &mut usize, tool_state: &mut HashMap<usize, ToolStatus>) {
    // Newly appended items.
    for (i, item) in fold.items.iter().enumerate().skip(*printed) {
        print_item(item);
        if let Item::Tool { status, .. } = item {
            tool_state.insert(i, *status);
        }
    }
    *printed = fold.items.len();
    // Status transitions on already-printed tool cards.
    for (i, item) in fold.items.iter().enumerate().take(*printed) {
        if let Item::Tool {
            status,
            name,
            result,
            error,
            ..
        } = item
        {
            let prev = tool_state.get(&i).copied();
            if prev.is_some() && prev != Some(*status) {
                tool_state.insert(i, *status);
                let line = match status {
                    ToolStatus::Ok => format!("  ✓ {name} done{}", preview_suffix(result)),
                    ToolStatus::Failed => format!("  ✗ {name} failed: {error}"),
                    ToolStatus::Denied => format!("  ⊘ {name} denied"),
                    ToolStatus::Running => format!("  » {name} running"),
                    ToolStatus::AwaitingApproval => format!("  ? {name} awaiting approval"),
                    ToolStatus::Interrupted => format!("  ◌ {name} interrupted"),
                };
                println!("{line}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_headless_ask, ASK_REFUSAL};

    #[test]
    fn conclude_gate_auto_accepts_in_headless_exec() {
        let prompt = "The coding agent believes the task is done after 2 cycle(s).\n\n\
                      WORKFLOW_CHECK\n\n\
                      Reply 'accept' to finish, or describe what still needs to change \
                      (your words are fed straight into the next cycle as operator steering).";
        let (response, log) = resolve_headless_ask(prompt);
        assert_eq!(response, "accept");
        assert!(
            log.contains("auto-accept"),
            "the log names the special-case resolution: {log}"
        );
    }

    #[test]
    fn ordinary_user_question_keeps_the_headless_refusal() {
        let (response, log) = resolve_headless_ask("Which one?");
        assert_eq!(response, ASK_REFUSAL);
        assert!(
            log.contains("refusal"),
            "ordinary asks must still refuse headlessly: {log}"
        );
    }
}

/// One-line tool-result tail for the headless stream.
///
/// ADR-0026: display bound, but NEVER a silent one. This stream is captured by
/// the bench harnesses and piped into orchestrating agents, so a cut that
/// leaves no trace reads downstream as "the tool returned exactly this". The
/// marker names what was dropped; the full result stays in the gateway ledger.
fn preview_suffix(preview: &str) -> String {
    let first = preview.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        return String::new();
    }
    let total = preview.chars().count();
    let more_lines = preview.lines().count() > 1;
    let capped: String = first.chars().take(100).collect();
    if capped.chars().count() < first.chars().count() || more_lines {
        //[WARNING:TRUNCATION] headless one-line tool preview; full result in the ledger
        format!(" — {capped}… [#TRUNCATION: first line, 100 chars of {total}; full result in the run ledger]")
    } else {
        format!(" — {capped}")
    }
}

fn print_item(item: &Item) {
    match item {
        Item::User { text } => println!("❯ {text}"),
        Item::Steer { text } => println!("↪ steer: {text}"),
        Item::Thinking {
            iteration,
            content,
            reasoning,
            ..
        } => {
            let body = if content.trim().is_empty() {
                reasoning
            } else {
                content
            };
            let first: String = body.lines().take(3).collect::<Vec<_>>().join(" | ");
            let capped: String = first.chars().take(240).collect();
            // ADR-0026: the cycle line is progress chrome, but a SILENT cut in a
            // piped stream reads as the model's whole thought. Say so when it cuts.
            if capped.chars().count() < first.chars().count() || body.lines().count() > 3 {
                //[WARNING:TRUNCATION] headless cycle preview; full reasoning in the ledger
                println!(
                    "∴ cycle {iteration}: {capped}… [#TRUNCATION: first 3 lines, 240 chars of {}; full text in the run ledger]",
                    body.chars().count()
                );
            } else {
                println!("∴ cycle {iteration}: {capped}");
            }
        }
        Item::Tool {
            name,
            args_preview,
            status,
            ..
        } => {
            let glyph = match status {
                ToolStatus::AwaitingApproval => "?",
                ToolStatus::Running => "»",
                ToolStatus::Ok => "✓",
                ToolStatus::Failed => "✗",
                ToolStatus::Denied => "⊘",
                ToolStatus::Interrupted => "◌",
            };
            println!("{glyph} {name} {args_preview}");
        }
        Item::Assistant { text, final_answer } => {
            if *final_answer {
                println!("\n━━━ answer ━━━\n{text}\n");
            } else {
                println!("✦ {text}");
            }
        }
        Item::Image {
            artifact_id, label, ..
        } => println!("▦ image: {label} ({artifact_id})"),
        Item::Info { text } => println!("· {text}"),
        Item::Error { text } => println!("✗ {text}"),
        // Entity-lane probe cards (worker-2 lane): headless prints the
        // title + first body line, nothing interactive.
        Item::Probe { title, body } => {
            let first = body.lines().next().unwrap_or("").trim();
            if first.is_empty() {
                println!("◈ {title}");
            } else if body.lines().count() > 1 {
                //[WARNING:TRUNCATION] headless probe card shows the first body line only
                println!(
                    "◈ {title} — {first} [#TRUNCATION: first of {} lines; full probe body in the run ledger]",
                    body.lines().count()
                );
            } else {
                println!("◈ {title} — {first}");
            }
        }
    }
}
