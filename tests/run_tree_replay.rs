//! Replay a REAL captured run tree (root + helper subflows + agent subrun)
//! through the fold exactly as the runner/exec interleave it: root records
//! first, subruns discovered through FollowRun effects, then round-robin.
//!
//! Captured live from basic-agent@0.0.2 on 2026-07-21. The load-bearing
//! assertion: the AGENT subrun's answer finishes the turn even though the
//! root run keeps waiting on a helper poller subflow.

use std::collections::HashMap;

use serde_json::{json, Value};

use abstractcode_tui::transcript::{Fold, FoldEffect, Item};

#[test]
fn real_run_tree_reaches_the_answer() {
    let raw = include_str!("fixtures/run_tree_basic_agent.json");
    let tree: Value = serde_json::from_str(raw).expect("fixture parses");
    let root_id = tree["root"]["run_id"].as_str().unwrap().to_string();

    let mut ledgers: HashMap<String, Vec<Value>> = HashMap::new();
    for name in ["root", "helper1", "agent", "helper2"] {
        let run_id = tree[name]["run_id"].as_str().unwrap().to_string();
        let records = tree[name]["records"].as_array().unwrap().clone();
        ledgers.insert(run_id, records);
    }

    let mut fold = Fold::new();
    fold.begin_run(&root_id);

    // Round-robin the ledgers the way exec's polling loop does: one page per
    // followed run per sweep, following new runs as the fold discovers them.
    let mut cursors: HashMap<String, usize> = HashMap::new();
    cursors.insert(root_id.clone(), 0);
    for _sweep in 0..60 {
        if fold.finished {
            break;
        }
        let run_ids: Vec<String> = cursors.keys().cloned().collect();
        for rid in run_ids {
            let cur = *cursors.get(&rid).unwrap_or(&0);
            let records = match ledgers.get(&rid) {
                Some(r) => r,
                None => continue,
            };
            let page: Vec<Value> = records.iter().skip(cur).take(5).cloned().collect();
            cursors.insert(rid.clone(), cur + page.len());
            for rec in &page {
                for fx in fold.apply(&rid, rec) {
                    if let FoldEffect::FollowRun(sub) = fx {
                        cursors.entry(sub).or_insert(0);
                    }
                }
            }
        }
    }

    assert!(
        fold.finished,
        "the agent answer must finish the turn; items: {:#?}",
        fold.items
    );
    let answer = fold.items.iter().rev().find_map(|i| match i {
        Item::Assistant {
            text,
            final_answer: true,
        } => Some(text.clone()),
        _ => None,
    });
    let answer = answer.expect("final assistant answer present");
    assert!(
        answer.contains(".abstractgateway-workspace.json"),
        "answer carries the real content: {answer}"
    );
    assert!(fold.stats.llm_calls >= 3, "usage folded: {:?}", fold.stats);
}

/// THE "never finishes" P0, distilled from the LIVE run the maintainer
/// watched hang for five hours (root c61e4ac9…, agent subrun 0f2d487c…,
/// gateway 2026-07-22): the agent ANSWERED at minute 3, but its final
/// output exceeded the runtime's 256 KB inline cap, so the ledger
/// persisted `result.output = {"$artifact": id}` (offloader; the read
/// surface serves refs unresolved). The fold saw no text → `finished`
/// never flipped — and the root can never conclude on its own because the
/// wrapper keeps a status-poller subflow looping wait_until forever
/// (poller 8fe2acc9 was still looping 18h later). The fix: the offloaded
/// flow end CONCLUDES the turn on a placeholder card + FetchAnswer effect;
/// the fetched artifact content swaps the real words in.
#[test]
fn offloaded_answer_tree_concludes_and_fetches_the_words() {
    let raw = include_str!("fixtures/offloaded_answer_tree.json");
    let tree: Value = serde_json::from_str(raw).expect("fixture parses");
    let root_id = tree["root_run_id"].as_str().unwrap().to_string();
    let agent_id = tree["agent_run_id"].as_str().unwrap().to_string();
    let artifact_id = tree["artifact_id"].as_str().unwrap().to_string();
    let ledgers = tree["ledgers"].as_object().unwrap();

    let mut fold = Fold::new();
    fold.begin_run(&root_id);

    // Round-robin replay (the runner's stream shape), collecting fetch
    // effects the way stream_run forwards them to the command loop.
    let mut cursors: HashMap<String, usize> = HashMap::new();
    cursors.insert(root_id.clone(), 0);
    let mut fetches: Vec<(String, String)> = Vec::new();
    for _sweep in 0..60 {
        let run_ids: Vec<String> = cursors.keys().cloned().collect();
        let mut progressed = false;
        for rid in run_ids {
            let cur = *cursors.get(&rid).unwrap_or(&0);
            let records = match ledgers.get(&rid).and_then(Value::as_array) {
                Some(r) => r,
                None => continue,
            };
            let page: Vec<Value> = records.iter().skip(cur).take(5).cloned().collect();
            progressed |= !page.is_empty();
            cursors.insert(rid.clone(), cur + page.len());
            for rec in &page {
                for fx in fold.apply(&rid, rec) {
                    match fx {
                        FoldEffect::FollowRun(sub) => {
                            cursors.entry(sub).or_insert(0);
                        }
                        FoldEffect::FetchAnswer {
                            run_id,
                            artifact_id,
                        } => fetches.push((run_id, artifact_id)),
                        FoldEffect::FetchImage { .. } => {}
                    }
                }
            }
        }
        if !progressed {
            break;
        }
    }

    assert_eq!(
        cursors.len(),
        3,
        "root + agent + poller all followed: {:?}",
        cursors.keys().collect::<Vec<_>>()
    );
    assert!(
        fold.finished,
        "the offloaded flow end must conclude the turn; items: {:#?}",
        fold.items
    );
    assert!(!fold.failed, "a concluded answer is a success outcome");
    assert_eq!(
        fetches,
        vec![(agent_id.clone(), artifact_id.clone())],
        "exactly one answer fetch, addressed to the agent run's artifact"
    );
    // Splitless usage still folds (this run's provider reports totals only).
    assert!(
        fold.stats.total_tokens > 0,
        "usage folded: {:?}",
        fold.stats
    );

    // The placeholder card is the final answer until the fetch lands…
    let placeholder = abstractcode_tui::transcript::offload_placeholder(&artifact_id);
    assert!(
        fold.items.iter().any(
            |i| matches!(i, Item::Assistant { text, final_answer: true } if *text == placeholder)
        ),
        "placeholder rendered final: {:#?}",
        fold.items
    );
    // …then the fetched artifact content (the REAL bytes' shape: the
    // serialized output object) swaps the words in.
    let content = br#"{"answer": "You were right. I tested the game myself in headless Chrome.", "report": "task: ...", "iterations": 12}"#;
    let text = abstractcode_tui::protocol::answer_text_from_artifact(content, "application/json")
        .expect("artifact carries the answer");
    fold.resolve_offloaded_answer(&artifact_id, Ok(text));
    assert!(
        fold.items.iter().any(|i| matches!(
            i,
            Item::Assistant { text, final_answer: true } if text.starts_with("You were right.")
        )),
        "the real words replace the placeholder: {:#?}",
        fold.items
    );
}

/// Bug (e) regression, distilled from the LIVE bug run itself (coder run
/// 5f810f81…, gateway 2026-07-22): the coder wrapper nests agent runs at
/// depth 2-3 (coder > coding-agent > builder/verify-gates > verifier),
/// and its provider (gpt-5.6-sol) reports SPLITLESS usage —
/// `{"input_tokens": 0, "output_tokens": 0, "total_tokens": N}`. The live
/// symptom: "0↑ 0↓ tk" against "23 tools" for five hours (the trimmed
/// half-tree alone spent 513k tokens), with a sticky "Done" activity from
/// per-round helper status events (unit-tested in transcript.rs — the
/// round-robin end state here is interleave-dependent).
#[test]
fn coder_tree_folds_splitless_usage_and_concludes_on_the_report() {
    let raw = include_str!("fixtures/coder_run_tree.json");
    let tree: Value = serde_json::from_str(raw).expect("fixture parses");
    let root_id = tree["root_run_id"].as_str().unwrap().to_string();
    let ledgers = tree["ledgers"].as_object().unwrap();

    let mut fold = Fold::new();
    fold.begin_run(&root_id);

    // Round-robin replay, following subruns as the fold discovers them
    // (the runner's stream shape). Discovery must recurse to depth 3.
    // Two trimmed rounds are discoverable-but-recordless (mid-capture
    // view): their cursors exist, their ledgers are absent.
    let mut cursors: HashMap<String, usize> = HashMap::new();
    cursors.insert(root_id.clone(), 0);
    for _sweep in 0..200 {
        let run_ids: Vec<String> = cursors.keys().cloned().collect();
        let mut progressed = false;
        for rid in run_ids {
            let cur = *cursors.get(&rid).unwrap_or(&0);
            let records = match ledgers.get(&rid).and_then(Value::as_array) {
                Some(r) => r,
                None => continue,
            };
            let page: Vec<Value> = records.iter().skip(cur).take(5).cloned().collect();
            progressed |= !page.is_empty();
            cursors.insert(rid.clone(), cur + page.len());
            for rec in &page {
                for fx in fold.apply(&rid, rec) {
                    if let FoldEffect::FollowRun(sub) = fx {
                        cursors.entry(sub).or_insert(0);
                    }
                }
            }
        }
        if !progressed {
            break;
        }
    }
    assert_eq!(
        cursors.len(),
        7,
        "discovery recurses the whole depth-3 tree (root + 6 subruns): {:?}",
        cursors.keys().collect::<Vec<_>>()
    );

    // Splitless usage folded as totals (the live numbers of the kept
    // ledgers — recomputed from the fixture at distillation time).
    assert_eq!(fold.stats.llm_calls, 20, "every followed run's llm calls");
    assert_eq!(fold.stats.input_tokens, 0, "the provider reports no split");
    assert_eq!(fold.stats.output_tokens, 0);
    assert_eq!(
        fold.stats.total_tokens, 513_671,
        "total_tokens is the only honest number for this provider"
    );
    assert_eq!(fold.stats.tool_calls, 55, "tool cards from every depth");
    assert!(
        fold.stats.output_series.iter().any(|v| *v > 0.0),
        "the sparkline substitutes per-call totals for splitless usage"
    );

    // Deep cycling runs are NOT the answer lane (the builder/verifier
    // agents are grandchildren): nothing in the tree so far may finish
    // the turn — the conclusion belongs to the coder ROOT's own end.
    assert!(!fold.finished, "no grandchild output may conclude the turn");

    // The coder ROOT's end record concludes the turn. The live run had
    // not completed at capture (root still waiting), so this record is
    // synthesized from the flow definition wiring (artifact 1b7ec590…:
    // get_report.value -> end.response, meta_obj.result -> end.meta) and
    // the real report text of completed sibling run b7d86e08.
    let root_end = json!({
        "run_id": root_id, "node_id": "end", "status": "completed",
        "result": {"output": {
            "response": "# Coding agent result\n\nStatus: DELIVERED — NOT VERIFIABLE HERE\nRounds used: 1\n",
            "success": false,
            "meta": {"passed": false, "delivered": true, "rounds_used": 1}}}
    });
    fold.apply(&root_id, &root_end);
    assert!(fold.finished, "the coder root's end concludes the turn");
    // The done summary follows the answer — skip terminal markers to
    // reach the content tail.
    let tail = fold
        .items
        .iter()
        .rev()
        .find(|i| {
            !matches!(i, Item::Info { text }
                if text.starts_with("✓ ") || text.starts_with("✗ ") || text.starts_with("⊘ "))
        })
        .expect("items");
    match tail {
        Item::Assistant {
            text,
            final_answer: true,
        } => {
            assert!(
                text.contains("Coding agent result"),
                "the report is the answer: {text}"
            );
        }
        other => panic!("unexpected tail item: {other:?}"),
    }
}
