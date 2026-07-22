//! Replay a REAL captured run tree (root + helper subflows + agent subrun)
//! through the fold exactly as the runner/exec interleave it: root records
//! first, subruns discovered through FollowRun effects, then round-robin.
//!
//! Captured live from basic-agent@0.0.2 on 2026-07-21. The load-bearing
//! assertion: the AGENT subrun's answer finishes the turn even though the
//! root run keeps waiting on a helper poller subflow.

use std::collections::HashMap;

use serde_json::Value;

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
