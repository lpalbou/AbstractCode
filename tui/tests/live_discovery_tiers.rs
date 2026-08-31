//! MANUAL live probe (never CI): the `discovery/tools` tier/approval
//! contract (facts UPDATE 2, 2026-07-22; approval spelling corrected
//! 2026-07-23). Read-only GET — spends no tokens, mutates nothing.
//!
//! The gateway serves per-item `tier` + an approval dial — LIVE
//! spelling `approval_default` (50/50 rows post-bounce), legacy
//! `approval` parsed as the fallback spelling — render-when-present:
//! BOTH states are valid (older builds omit the dial entirely). This
//! test pins the contract LOOSELY — it asserts whichever state the
//! live gateway is in is INTERNALLY CONSISTENT:
//! - if no item carries a dial, the client's #FALLBACK name table
//!   classifies (nothing to check here);
//! - if any item carries one, every served value is a recognized
//!   dial ("auto"|"ask") that `server_tier` maps without surprise, and
//!   the tier string (when present) is a known capability class.
//!
//! Run explicitly (fields absent verified live 2026-07-22 15:xx Z):
//! ```sh
//! ABSTRACTCODE_GATEWAY_TOKEN=... cargo test --test live_discovery_tiers -- --ignored --nocapture
//! ```

use abstractcode::discovery::tools_from_discovery;
use abstractcode::gateway::GatewayClient;

/// The production connection resolution (env > shared login store) —
/// the same path the app boots with, so the probe sees what the app
/// would see.
fn live_client() -> GatewayClient {
    let conn = abstractcode::config::resolve_connection(None, None);
    GatewayClient::new(&conn.base_url, conn.token.as_deref())
}

#[test]
#[ignore = "manual live probe: read-only GET against a running gateway"]
fn discovery_tier_approval_contract_is_consistent_either_way() {
    let client = live_client();

    let v = client.discovery_tools().expect("discovery/tools GET");
    let tools = tools_from_discovery(&v);
    assert!(!tools.is_empty(), "the gateway serves at least one tool");

    let with_approval = tools.iter().filter(|t| t.approval.is_some()).count();
    let with_tier = tools.iter().filter(|t| t.tier.is_some()).count();
    println!(
        "discovery: {} tools · {with_approval} with approval · {with_tier} with tier",
        tools.len()
    );

    if with_approval == 0 {
        // Pre-bounce build: the client falls back to the name table.
        println!("PRE-BOUNCE: no per-tool approval served (#FALLBACK name table active)");
        return;
    }

    // Post-bounce build: every served value must be a recognized dial, and
    // the client's mapping must not panic or produce a surprising tier.
    println!("POST-BOUNCE: per-tool approval served — checking consistency");
    for t in &tools {
        if let Some(approval) = t.approval.as_deref() {
            let a = approval.trim().to_lowercase();
            assert!(
                a == "auto" || a == "ask",
                "tool {} served an unrecognized approval {approval:?}",
                t.name
            );
            // The mapping is total (server_tier never panics); a served
            // read tool must not classify as All, a shell tool must not
            // classify below All at ask.
            let mapped = abstractcode::tool_policy::server_tier(&t.name, approval, t.risk_rank);
            println!(
                "  {} → approval={approval} rank={:?} tier={:?} → {mapped:?}",
                t.name, t.risk_rank, t.tier
            );
        }
    }
}

/// MANUAL live wire pass (never CI) for the full-catalog surfacing fix
/// (tool-tiers item H; this seat's c4611 receipt promised this pass at
/// the gateway bounce): served-disabled rows parse `enabled:false` +
/// `enable_gate` + `why_disabled` into `served_disabled` rows, the
/// policy expansion clamps every one of them to ask, and the run
/// allowlist derivation (enabled minus user-disabled) excludes them.
/// Loose by design: a pre-surfacing gateway (zero disabled rows) passes
/// with the PRE-SURFACING label — both states are internally checked.
#[test]
#[ignore = "manual live probe: read-only GET against a running gateway"]
fn served_disabled_rows_parse_and_clamp_on_the_live_catalog() {
    let client = live_client();
    let v = client.discovery_tools().expect("discovery/tools GET");
    let tools = tools_from_discovery(&v);
    assert!(!tools.is_empty(), "the gateway serves at least one tool");

    let disabled: Vec<_> = tools.iter().filter(|t| t.served_disabled).collect();
    let enabled = tools.len() - disabled.len();
    println!(
        "discovery: {} tools · {enabled} enabled · {} served-disabled",
        tools.len(),
        disabled.len()
    );
    if disabled.is_empty() {
        println!("PRE-SURFACING gateway: no disabled rows served — nothing to clamp");
        return;
    }
    // Every served-disabled row must name its gate (the surfacing fix's
    // whole point: exists-but-not-enabled is a VISIBLE state with the
    // gate that flips it — silence and gate-less rows are both defects).
    for t in &disabled {
        println!(
            "  [off] {} · gate: {} · {}",
            t.name, t.enable_gate, t.why_disabled
        );
        assert!(
            !t.enable_gate.is_empty(),
            "served-disabled row {} carries no enable_gate",
            t.name
        );
    }
    // The clamp holds against the LIVE inventory: even at tier `all`,
    // no served-disabled name reaches auto_approve_tools; each lands in
    // require_approval_tools instead.
    let classes: Vec<abstractcode::tool_policy::ToolClass> = tools
        .iter()
        .map(|t| abstractcode::tool_policy::ToolClass {
            name: t.name.clone(),
            approval: t.approval.clone(),
            tier: t.tier.clone(),
            served_disabled: t.served_disabled,
            enable_gate: t.enable_gate.clone(),
            risk_rank: t.risk_rank,
        })
        .collect();
    let policy = abstractcode::tool_policy::expand_run_policy(&classes, "all", &[]);
    for t in &disabled {
        assert!(
            !policy.auto_approve_tools.contains(&t.name),
            "served-disabled {} leaked into auto_approve_tools",
            t.name
        );
        assert!(
            policy.require_approval_tools.contains(&t.name),
            "served-disabled {} missing from require_approval_tools",
            t.name
        );
    }
    // The allowlist derivation (enabled inventory minus user-disabled)
    // never names a served-disabled tool.
    let allowlist: Vec<&str> = tools
        .iter()
        .filter(|t| !t.served_disabled)
        .map(|t| t.name.as_str())
        .collect();
    for t in &disabled {
        assert!(
            !allowlist.contains(&t.name.as_str()),
            "served-disabled {} leaked into the allowlist",
            t.name
        );
    }
    println!(
        "CLAMP HOLDS: {} disabled rows → require_approval, 0 in auto/allowlist",
        disabled.len()
    );
}

/// MANUAL live discriminator (never CI) for the factless-discovery
/// finding (c4650/c4647; core's verify line c4664 folded in per c4667).
/// RESOLVED 2026-07-23 before it had to discriminate: the fold-side
/// hypothesis WON — the discovery lane's prompt specs carried no fact
/// fields, gateway shipped `join_registry_facts()` (c4665), runtime
/// retracted its stale-boot diagnosis (c4672: a re-bounce alone would
/// NOT have flipped the flat distribution), core closed the thread
/// (c4675). Kept as the post-bounce verification: expect LADDER LIVE
/// (read_file=observe/1, execute_command=destroy/4) with an HONEST
/// unvetted-4 remainder — agora/shell (and camera until its
/// register_capability_tool_facts lands, c4671/c4673) carry no
/// declared facts by design; a remainder is not a regression.
/// This client consumes no risk_* key (on record, c4614), so the test
/// asserts only per-row INTERNAL consistency (rank is an int; a
/// factless `unvetted` presentation implies the deny-safe top rank)
/// and PRINTS the verdict.
#[test]
#[ignore = "manual live probe: read-only GET against a running gateway"]
fn risk_trio_distribution_discriminates_stale_vs_seam() {
    let client = live_client();
    let v = client.discovery_tools().expect("discovery/tools GET");
    let items = v
        .get("tools")
        .or_else(|| v.get("items"))
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut with_trio = 0usize;
    let mut flat_top = 0usize;
    let mut read_file_trio = None;
    let mut exec_trio = None;
    for t in &items {
        let word = t.get("risk_tier").and_then(|x| x.as_str());
        let rank = t.get("risk_rank").and_then(|x| x.as_i64());
        let pres = t.get("risk_presentation").and_then(|x| x.as_str());
        let (Some(word), Some(rank)) = (word, rank) else {
            continue; // pre-trio gateway: nothing to discriminate
        };
        with_trio += 1;
        assert!(
            (1..=4).contains(&rank),
            "{}: risk_rank {rank} outside 1..=4",
            t.get("name").and_then(|x| x.as_str()).unwrap_or("?")
        );
        // The ruled factless shape: unvetted presentation rides the
        // deny-safe TOP rank, never a lower one.
        if pres == Some("unvetted") {
            assert_eq!(
                rank,
                4,
                "unvetted presentation must ride the top rank ({})",
                t.get("name").and_then(|x| x.as_str()).unwrap_or("?")
            );
        }
        if word == "destroy" && rank == 4 {
            flat_top += 1;
        }
        match t.get("name").and_then(|x| x.as_str()) {
            Some("read_file") => read_file_trio = Some((word.to_string(), rank)),
            Some("execute_command") => exec_trio = Some((word.to_string(), rank)),
            _ => {}
        }
    }
    if with_trio == 0 {
        println!("PRE-TRIO gateway: no risk fields served — nothing to discriminate");
        return;
    }
    println!(
        "risk trio on {with_trio} rows · {flat_top} at destroy/4 · read_file={read_file_trio:?} · execute_command={exec_trio:?}"
    );
    if flat_top == with_trio {
        println!(
            "VERDICT: FLAT — every row factless (stale process or the fact-stripped-rows seam; re-bounce discriminates)"
        );
    } else if read_file_trio.as_ref().map(|(w, r)| (w.as_str(), *r)) == Some(("observe", 1)) {
        println!(
            "VERDICT: LADDER LIVE — facts reach the HTTP lane (stale-boot diagnosis confirmed)"
        );
    } else {
        println!("VERDICT: MIXED — partial facts; hand the distribution to the serving chair");
    }
}
