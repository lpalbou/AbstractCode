//! Replay the REAL captured tree of the failed-agent P0 (lane A,
//! 2026-07-23): basic-agent@0.0.3 whose AGENT subrun terminally failed
//! ("Model unloaded.") at cycle 1 while the wrapper root absorbed the
//! failure and parked FOREVER on its status-poller subflow.
//!
//! Captured live from gateway runs 76fc3fcb… (root, still `waiting` 15h
//! later) and 9c5cad22… (agent, `failed`) — see
//! docs/roadmap/lane-a-diagnosis.md §1. The load-bearing assertions:
//! records alone must NOT conclude (failed effect records retry/absorb),
//! and the runner's subrun-terminal report MUST (it is the only signal
//! this tree ever produces).

use serde_json::Value;

use abstractcode_tui::transcript::{Fold, FoldEffect, Item};

fn fixture() -> Value {
    let raw = include_str!("fixtures/failed_agent_subrun_tree.json");
    serde_json::from_str(raw).expect("fixture parses")
}

#[test]
fn failed_agent_subrun_tree_concludes_from_the_terminal_report() {
    let tree = fixture();
    let root_id = tree["root_run_id"].as_str().unwrap().to_string();
    let agent_id = tree["agent_run_id"].as_str().unwrap().to_string();
    assert_eq!(tree["agent_run_status"], "failed", "fixture self-check");
    assert_eq!(tree["root_run_status"], "waiting", "fixture self-check");

    let mut fold = Fold::new();
    fold.begin_run(&root_id);

    // Root records first (the runner's root stream), following discovered
    // subruns exactly as Cmd::Follow would.
    let mut followed: Vec<String> = Vec::new();
    for rec in tree["ledgers"][&root_id].as_array().unwrap() {
        for fx in fold.apply(&root_id, rec) {
            if let FoldEffect::FollowRun(sub) = fx {
                followed.push(sub);
            }
        }
    }
    // The root discovers three subruns: helper (node-4), the AGENT
    // (node-2), and the eternal status poller (node-5).
    assert!(
        followed.contains(&agent_id),
        "the agent subrun is discovered from the root's subworkflow wait"
    );

    // The agent subrun's ledger: started llm_call, terminally-failed
    // llm_call ("Model unloaded."), a status emit. NO conclusion record.
    for rec in tree["ledgers"][&agent_id].as_array().unwrap() {
        fold.apply(&agent_id, rec);
    }
    assert!(
        fold.items
            .iter()
            .any(|i| matches!(i, Item::Error { text } if text.contains("Model unloaded"))),
        "the provider failure renders as an error card"
    );
    assert!(
        !fold.finished,
        "records alone never conclude: a failed effect record can retry or absorb"
    );
    assert_eq!(
        fold.answer_run_id(),
        Some(agent_id.as_str()),
        "the cycling agent bound as the answer source (exec polls this)"
    );

    // The runner's subrun-terminal report (stream `done` → get_run →
    // status "failed") — the ONLY conclusion signal this tree produces:
    // the root resumed PAST the failure onto the poller and stays
    // `waiting` forever (live-verified 15h later).
    fold.subrun_terminal(&agent_id, "failed");
    assert!(fold.finished, "the composer is freed");
    assert!(fold.failed, "and the outcome is Failed (exec exits 1)");
    assert!(
        fold.items.iter().rev().any(
            |i| matches!(i, Item::Error { text } if text.contains("the agent run ended: failed"))
        ),
        "the conclusion names what happened (the ✗ done summary follows it)"
    );

    // Helper terminals (the poller eventually being cancelled by hand)
    // arrive after conclusion and change nothing.
    let items = fold.items.len();
    fold.subrun_terminal("97b4dde0-a125-4e8f-bed3-e8732c4054d1", "cancelled");
    assert_eq!(fold.items.len(), items);
}

#[test]
fn agent_dying_before_its_first_cycle_still_concludes() {
    // Conformance lane (2026-07-23): the answer source binds
    // STRUCTURALLY from the ROOT's spawn records (the wait's
    // `details.sub_workflow_id` names the runtime's Agent-node workflow —
    // real captured shape), never from the child's behavior. An agent
    // child that dies BEFORE writing a single record of its own — the
    // documented residual of the cycle-heuristic era — is therefore
    // already bound, and its terminal status concludes the turn.
    let tree = fixture();
    let root_id = tree["root_run_id"].as_str().unwrap().to_string();
    let agent_id = tree["agent_run_id"].as_str().unwrap().to_string();

    let mut fold = Fold::new();
    fold.begin_run(&root_id);
    // ROOT records only: the agent child's ledger stays EMPTY (it
    // crashed before its first reason cycle — no records exist).
    for rec in tree["ledgers"][&root_id].as_array().unwrap() {
        let _ = fold.apply(&root_id, rec);
    }
    assert_eq!(
        fold.answer_run_id(),
        Some(agent_id.as_str()),
        "the agent bound from the ROOT's spawn declaration alone \
         (details.sub_workflow_id = visual_react_agent_…) — no cycle needed"
    );
    assert!(!fold.finished, "nothing concluded yet");

    // The runner observes the recordless child terminally failed.
    fold.subrun_terminal(&agent_id, "failed");
    assert!(fold.finished, "the turn concludes honestly");
    assert!(fold.failed, "as a failure (exec exits 1)");
    assert!(
        fold.items.iter().rev().any(
            |i| matches!(i, Item::Error { text } if text.contains("the agent run ended: failed"))
        ),
        "the conclusion names what happened (the ✗ done summary follows it)"
    );

    // Contrast pin: the HELPER children declared in the same root ledger
    // (basic-agent@0.0.3:15f19f7f — the status flows) never bound, so
    // their terminals never conclude a fresh turn.
    let mut fold2 = Fold::new();
    fold2.begin_run(&root_id);
    for rec in tree["ledgers"][&root_id].as_array().unwrap() {
        fold2.apply(&root_id, rec);
    }
    fold2.subrun_terminal("eee5ff82-cc78-49f3-b55b-94d7026aa23a", "completed");
    fold2.subrun_terminal("97b4dde0-a125-4e8f-bed3-e8732c4054d1", "failed");
    assert!(
        !fold2.finished,
        "helper terminals never conclude — only the declared agent's does"
    );
}
