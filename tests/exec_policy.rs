//! Integration coverage for the headless (`exec`) approval-resolution
//! path (bug (e), 2026-07-22 — the worker left this UNIT-only). The
//! decision + resume payload + human log are a pure function
//! (`exec::resolve_approval`) so the deny path is exercised WITHOUT a live
//! gateway: the crate-external harness proves the DENY payload names the
//! real rule (the bridge lesson — a controller that gates must say why).

use abstractcode_tui::exec::resolve_approval;
use abstractcode_tui::tool_policy::ToolClass;
use serde_json::json;

fn call(name: &str, args: serde_json::Value) -> serde_json::Value {
    json!({"name": name, "arguments": args})
}

#[test]
fn deny_reason_names_the_real_tier_rule() {
    // A shell batch under an accepted 'read' tier: DENIED, and the reason
    // the runtime relays to the model names both tiers + the escalation.
    let batch = vec![call("execute_command", json!({"command": "cargo build"}))];
    let r = resolve_approval(&batch, "read", &[], &[]);
    assert!(!r.approved, "shell above read level denies");
    let reason = r.payload["reason"].as_str().unwrap();
    assert!(
        reason.contains("permissions 'all'"),
        "names the needed level: {reason}"
    );
    assert!(
        reason.contains("accepted level is 'read'"),
        "names the accepted level: {reason}"
    );
    assert!(
        reason.contains("--permissions"),
        "names the escalation gesture: {reason}"
    );
    // R3 (c5028): headless denials are POLICY decisions, stamped so.
    assert_eq!(r.payload["approved_by"].as_str(), Some("policy"));
    // The human log reflects the denial (not a canned hint).
    assert!(r.log.starts_with("DENYING"), "log: {}", r.log);
    assert!(
        r.log.contains("execute_command"),
        "log names tools: {}",
        r.log
    );
}

#[test]
fn approve_within_level_and_at_the_top_level() {
    // read_file under 'read': approved by the level; the payload stamps
    // the POLICY as the approver + the admitting rule (R3, c5028).
    let reads = vec![call("read_file", json!({"path": "a.rs"}))];
    let r = resolve_approval(&reads, "read", &[], &[]);
    assert!(r.approved);
    assert_eq!(r.payload["approved"].as_bool(), Some(true));
    assert_eq!(r.payload["approved_by"].as_str(), Some("policy"));
    assert!(r.log.contains("within permissions 'read'"), "{}", r.log);

    // A shell batch at level 'all' (--permissions all resolves to the
    // same level — the deleted --approve-all's replacement): approved,
    // attributed to the level, never a flag bypass.
    let shell = vec![call("execute_command", json!({"command": "rm -rf x"}))];
    let r = resolve_approval(&shell, "all", &[], &[]);
    assert!(r.approved);
    assert!(r.log.contains("within permissions 'all'"), "{}", r.log);
}

#[test]
fn git_denies_below_all_client_side_the_refiner_owns_the_proof() {
    // c5057: the client git proof is RETIRED — the read-only-git decision
    // is runtime's git_read_only@v1 refiner (declared by core on
    // execute_command's row), which auto-approves proven reads AT THE
    // APPROVAL POINT so no wait reaches this resolver for them. A git
    // batch that DOES reach the headless resolver (pre-refiner gateway)
    // denies below `all` — one prompt is the honest price of not owning
    // the proof.
    let batch = vec![call("execute_command", json!({"command": "git status -s"}))];
    let r = resolve_approval(&batch, "read", &[], &[]);
    assert!(!r.approved, "no client proof: git denies below all");
    let push = vec![call("execute_command", json!({"command": "git push"}))];
    assert!(!resolve_approval(&push, "read", &[], &[]).approved);
    // At `all` the level admits shell like everything else.
    assert!(resolve_approval(&batch, "all", &[], &[]).approved);
}

#[test]
fn ask_override_denies_even_at_permissions_all_naming_the_pin() {
    // An 'ask' pin forces a prompt/deny even for a read tool the level
    // would admit — the headless resolver treats "must ask" as "deny"
    // (no interactive user), and the pin gates EVEN AT `all` (the
    // pin-vs-flag ruling: --permissions all never silently dissolves a
    // deliberate boundary the old --approve-all bypassed). The reason
    // names the pin and the remedy.
    let pins = vec![("read_file".to_string(), "ask".to_string())];
    let reads = vec![call("read_file", json!({"path": "a.rs"}))];
    let r = resolve_approval(&reads, "all", &pins, &[]);
    assert!(!r.approved, "ask pin denies headlessly even at level all");
    let reason = r.payload["reason"].as_str().unwrap();
    assert!(
        reason.contains("read_file") && reason.contains("pinned"),
        "the deny names the pin, not a level contradiction: {reason}"
    );
}

#[test]
fn unreadable_and_served_disabled_batches_deny_with_honest_reasons() {
    // Honest-reason gap 1 (Lane A): an empty batch used to produce the
    // self-contradictory "needs tier 'all', accepted 'all'".
    let r = resolve_approval(&[], "all", &[], &[]);
    assert!(!r.approved, "empty batches fail closed");
    assert!(
        r.payload["reason"].as_str().unwrap().contains("unreadable"),
        "names the real cause: {}",
        r.payload
    );

    // Honest-reason gap 2: a served-disabled tool denies naming ITS GATE.
    let classes = vec![ToolClass {
        name: "send_email".into(),
        approval: Some("auto".into()),
        served_disabled: true,
        enable_gate: "ABSTRACT_ENABLE_COMMS_TOOLS".into(),
        ..Default::default()
    }];
    let batch = vec![call("send_email", json!({"to": "x@y.z"}))];
    let r = resolve_approval(&batch, "all", &[], &classes);
    assert!(!r.approved, "served-disabled denies even at all");
    let reason = r.payload["reason"].as_str().unwrap();
    assert!(
        reason.contains("ABSTRACT_ENABLE_COMMS_TOOLS"),
        "the deny names the gate: {reason}"
    );
}

#[test]
fn server_truth_is_preferred_over_the_name_table() {
    // An MCP tool the NAME TABLE calls All (unknown) but the gateway
    // served approval:auto is approved at read tier — server truth wins.
    let classes = vec![ToolClass {
        name: "mcp::search".into(),
        approval: Some("auto".into()),
        tier: Some("tier2_world".into()),
        ..Default::default()
    }];
    let batch = vec![call("mcp::search", json!({"q": "x"}))];
    assert!(
        resolve_approval(&batch, "read", &[], &classes).approved,
        "server approval:auto lifts an mcp tool to read tier"
    );
    // Without the server class, the same batch denies at read (name table).
    assert!(!resolve_approval(&batch, "read", &[], &[]).approved);
}

#[test]
fn exit_codes_match_the_documented_truth_table() {
    // Cycle-2 review F2: a CANCELLED answer-source subrun used to conclude
    // through the generic finished branch (fold keeps failed==false for
    // cancels) and exec exited 0 — scripts read "answer produced" for a
    // cancelled run. One authority now serves both terminal branches:
    // completed → 0, cancelled → 130, anything else → 1 (timeout stays
    // 124 at its own site; missing prompt stays 2).
    use abstractcode_tui::exec::exit_code_for_status;
    assert_eq!(exit_code_for_status("completed"), 0);
    assert_eq!(exit_code_for_status("cancelled"), 130);
    assert_eq!(exit_code_for_status("failed"), 1);
    assert_eq!(exit_code_for_status("unknown"), 1);
    assert_eq!(exit_code_for_status(""), 1);
}

#[test]
fn explicit_missing_workflow_refuses_instead_of_silently_substituting() {
    // Final-verifier finding 1 (2026-07-23): `exec --workflow <nonexistent>`
    // silently ran basic-agent with exit 0 — automation pinning a specific
    // agent never noticed a DIFFERENT one ran. The refusal is a pure
    // predicate over (request, resolved, catalog).
    use abstractcode_tui::exec::explicit_workflow_mismatch;
    use abstractcode_tui::store::Workflow;

    let wf = |b: &str, f: &str| Workflow {
        bundle_id: b.into(),
        flow_id: f.into(),
        name: String::new(),
        description: String::new(),
    };
    let catalog = vec![wf("basic-agent", "main"), wf("coding-agent", "coder")];
    let fallback = catalog[0].clone();

    // Nonexistent bundle: the resolver fell back to basic-agent — refuse,
    // naming the request and the real catalog.
    let msg = explicit_workflow_mismatch("definitely-not-a-workflow", &fallback, &catalog)
        .expect("mismatch must refuse");
    assert!(msg.contains("definitely-not-a-workflow"), "{msg}");
    assert!(
        msg.contains("coding-agent:coder"),
        "lists the catalog: {msg}"
    );

    // Wrong flow inside a real bundle: still a refusal.
    assert!(
        explicit_workflow_mismatch("coding-agent:nope", &fallback, &catalog).is_some(),
        "bundle:flow must match both halves"
    );

    // Exact match and bundle-only match: no refusal.
    assert!(explicit_workflow_mismatch("coding-agent:coder", &catalog[1], &catalog).is_none());
    assert!(
        explicit_workflow_mismatch("coding-agent", &catalog[1], &catalog).is_none(),
        "bundle alone accepts any flow within it"
    );
}

// ---------------------------------------------------------------------------
// Thin-client conformance (lane 2, 2026-07-23): classification-source
// honesty in the headless log — a tier decided by the client's #FALLBACK
// name table must say so; gateway-served facts and explicit user acts
// (--approve-all, pins) must not carry the label.
// ---------------------------------------------------------------------------

#[test]
fn tier_decisions_from_the_name_table_carry_the_fallback_label() {
    // No inventory served at all: the tier decision rests entirely on the
    // client name table — the log names the tools and the #FALLBACK.
    let reads = vec![call("read_file", json!({"path": "a.rs"}))];
    let r = resolve_approval(&reads, "read", &[], &[]);
    assert!(r.approved);
    assert!(
        r.log.contains("#FALLBACK") && r.log.contains("read_file"),
        "name-table classification is labeled: {}",
        r.log
    );

    // A deny decided by the tier gap carries the label too (the cited
    // "needs tier" came from client classification).
    let shell = vec![call("execute_command", json!({"command": "cargo build"}))];
    let denied = resolve_approval(&shell, "read", &[], &[]);
    assert!(!denied.approved);
    assert!(denied.log.contains("#FALLBACK"), "{}", denied.log);
}

#[test]
fn server_facts_and_explicit_user_acts_never_carry_the_fallback_label() {
    // Gateway-served approval facts: the decision is server truth.
    let classes = vec![ToolClass {
        name: "read_file".into(),
        approval: Some("auto".into()),
        tier: Some("tier2_world".into()),
        ..Default::default()
    }];
    let reads = vec![call("read_file", json!({"path": "a.rs"}))];
    let r = resolve_approval(&reads, "read", &[], &classes);
    assert!(r.approved);
    assert!(
        !r.log.contains("#FALLBACK"),
        "server-classified decisions are unlabeled: {}",
        r.log
    );

    // At level `all` with NO served facts, the name table still
    // classified (read_file → Read) — the label ATTACHES (Lane A's
    // recommendation: the label follows the classification source, no
    // flag-shaped special case remains — the old --approve-all bypass
    // that suppressed it is deleted).
    let r = resolve_approval(&reads, "all", &[], &[]);
    assert!(r.approved);
    assert!(
        r.log.contains("#FALLBACK"),
        "name-table classification stays labeled at every level: {}",
        r.log
    );

    // An 'ask' pin deny: the user's explicit act decided, not the table.
    let pins = vec![("read_file".to_string(), "ask".to_string())];
    let r = resolve_approval(&reads, "all", &pins, &[]);
    assert!(!r.approved);
    assert!(
        !r.log.contains("#FALLBACK"),
        "pin denials carry no classification label: {}",
        r.log
    );

    // A git command with no served facts classifies via the NAME TABLE
    // now (the client proof retired, c5057) — denied below all, and the
    // label attaches because the table decided.
    let git = vec![call("execute_command", json!({"command": "git status -s"}))];
    let r = resolve_approval(&git, "read", &[], &[]);
    assert!(!r.approved);
    assert!(
        r.log.contains("#FALLBACK"),
        "name-table git classification is labeled: {}",
        r.log
    );
}
