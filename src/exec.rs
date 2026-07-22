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

use serde_json::json;

use crate::cli::Args;
use crate::config;
use crate::gateway::GatewayClient;
use crate::run_input::{build_input_data, StartOpts};
use crate::runner::{agent_workflows_from_bundles, choose_workflow};
use crate::transcript::{Fold, FoldEffect, Item, ToolStatus, WaitKind};

const ASK_REFUSAL: &str =
    "No interactive user is present (headless run). Proceed with your best judgment and finish the task.";

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

    let session_id = args.session.clone().unwrap_or_else(config::mint_session_id);
    let opts = StartOpts {
        provider: args.provider.clone().unwrap_or_default(),
        model: args.model.clone().unwrap_or_default(),
        workspace_root: if args.no_workspace {
            None
        } else {
            args.workspace.clone().or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|p| p.display().to_string())
            })
        },
        workspace_mode: args.workspace_mode.clone(),
        max_iterations: args.max_iterations,
        system: String::new(),
        // One-shot runs have no prior client transcript; cross-invocation
        // continuity rides the server-side session seed.
        messages: Vec::new(),
        // Headless runs take the workflow's own tool defaults; the /tools
        // and /skills selections are interactive-session preferences.
        tools: None,
        skills: Vec::new(),
    };
    let input = build_input_data(&prompt, &opts);
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
    fold.begin_run(&run_id);
    fold.push_item(Item::User { text: prompt });

    let mut cursors: HashMap<String, u64> = HashMap::new();
    cursors.insert(run_id.clone(), 0);
    let mut printed = 0usize;
    let mut printed_tool_state: HashMap<usize, ToolStatus> = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(args.timeout_secs.max(10));
    let mut answered: Vec<String> = Vec::new();

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
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("· ledger read failed ({e}); retrying");
                    std::thread::sleep(Duration::from_millis(800));
                }
            }
        }
        print_new(&fold, &mut printed, &mut printed_tool_state);

        // Resolve pending waits per policy.
        if let Some(wait) = fold.pending_wait.clone() {
            match &wait.kind {
                WaitKind::Approval { tool_calls } => {
                    let approved = args.approve_all;
                    let names: Vec<String> = tool_calls
                        .iter()
                        .filter_map(|tc| {
                            tc.get("name").and_then(|v| v.as_str()).map(str::to_string)
                        })
                        .collect();
                    eprintln!(
                        "{} tool batch [{}]",
                        if approved {
                            "approving"
                        } else {
                            "DENYING (no --approve-all)"
                        },
                        names.join(", ")
                    );
                    let payload = if approved {
                        json!({"approved": true})
                    } else {
                        json!({"approved": false, "reason": "Denied: headless run without --approve-all"})
                    };
                    match client.resume(&wait.run_id, &wait.wait_key, payload) {
                        Ok(_) => {
                            fold.wait_answered(&wait.wait_key, &wait.step_id);
                            fold.mark_wait_tools(&wait.wait_key, approved);
                            answered.push(wait.wait_key.clone());
                        }
                        Err(e) => {
                            eprintln!("· resume failed ({e}); retrying");
                            std::thread::sleep(Duration::from_millis(800));
                        }
                    }
                }
                WaitKind::Ask { prompt } => {
                    eprintln!("agent asks: {prompt}");
                    eprintln!("answering with the headless refusal");
                    match client.resume(
                        &wait.run_id,
                        &wait.wait_key,
                        json!({"response": ASK_REFUSAL}),
                    ) {
                        Ok(_) => {
                            fold.wait_answered(&wait.wait_key, &wait.step_id);
                            answered.push(wait.wait_key.clone());
                        }
                        Err(e) => {
                            eprintln!("· resume failed ({e}); retrying");
                            std::thread::sleep(Duration::from_millis(800));
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
            eprintln!(
                "done · {} llm calls · {} tools · {}↑ {}↓ tk (run {run_id} finalizes on the gateway)",
                stats.llm_calls, stats.tool_calls, stats.input_tokens, stats.output_tokens
            );
            return if failed { 1 } else { 0 };
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
                        "done: {status} · {} llm calls · {} tools · {}↑ {}↓ tk",
                        stats.llm_calls, stats.tool_calls, stats.input_tokens, stats.output_tokens
                    );
                    return match status.as_str() {
                        "completed" => 0,
                        "cancelled" => 130,
                        _ => 1,
                    };
                }
            }
            Err(e) => eprintln!("· status read failed ({e}); retrying"),
        }

        std::thread::sleep(Duration::from_millis(300));
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
            result_preview,
            error,
            ..
        } = item
        {
            let prev = tool_state.get(&i).copied();
            if prev.is_some() && prev != Some(*status) {
                tool_state.insert(i, *status);
                let line = match status {
                    ToolStatus::Ok => format!("  ✓ {name} done{}", preview_suffix(result_preview)),
                    ToolStatus::Failed => format!("  ✗ {name} failed: {error}"),
                    ToolStatus::Denied => format!("  ⊘ {name} denied"),
                    ToolStatus::Running => format!("  » {name} running"),
                    ToolStatus::AwaitingApproval => format!("  ? {name} awaiting approval"),
                };
                println!("{line}");
            }
        }
    }
}

fn preview_suffix(preview: &str) -> String {
    let first = preview.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        String::new()
    } else {
        let capped: String = first.chars().take(100).collect();
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
        } => {
            let body = if content.trim().is_empty() {
                reasoning
            } else {
                content
            };
            let first: String = body.lines().take(3).collect::<Vec<_>>().join(" | ");
            let capped: String = first.chars().take(240).collect();
            println!("∴ cycle {iteration}: {capped}");
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
    }
}
