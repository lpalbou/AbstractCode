//! Boot-restore regression: fold a REAL history_bundle through
//! `rehydrate_run_into` and require every tool card to reach its FINAL
//! state (operator report 2026-07-22: a relaunched session showed ALL
//! tool cards as "awaiting approval" although they were approved and
//! completed in the live session).
//!
//! The fixture is a sanitized slice of a live bundle
//! (GET /api/gateway/runs/{id}/history_bundle, gateway 2026-07-22,
//! session acode-3d7cd0ef54a9) and pins the two REAL shapes that broke
//! the restore:
//!
//! 1. Bundle ledger items are `{cursor, record}` ENVELOPES (the same
//!    wire shape as SSE `step` events) — the fold used to receive the
//!    envelope and every status/effect/result read missed.
//! 2. Terminal `tool_calls` records may carry
//!    `effect.payload.tool_calls = {"$slim": …}` (abstractruntime 0067-M
//!    ledger dedup replaces oversized payload fields on WAITING/COMPLETED
//!    records with a marker pointing at the STARTED record). The card
//!    finisher used to pair results against that payload list and found
//!    nothing — the approval wait's flip was the last state standing.

use serde_json::Value;

use abstractcode_tui::runner::rehydrate_run_into;
use abstractcode_tui::transcript::{Fold, Item, ToolStatus};

fn fold_fixture() -> Fold {
    let raw = include_str!("fixtures/history_bundle_restore.json");
    let bundle: Value = serde_json::from_str(raw).expect("fixture parses");
    let root = bundle["root_run_id"].as_str().expect("root id").to_string();
    let mut fold = Fold::new();
    let mut fx = Vec::new();
    let contributed = rehydrate_run_into(&mut fold, &root, &bundle, true, &mut fx);
    assert!(contributed, "the bundle must contribute transcript items");
    fold
}

#[test]
fn restored_tool_cards_reach_final_states() {
    let fold = fold_fixture();
    let tools: Vec<(&String, &ToolStatus)> = fold
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Tool { name, status, .. } => Some((name, status)),
            _ => None,
        })
        .collect();
    assert!(
        tools.len() >= 3,
        "the fixture carries two tool batches (3 calls): {tools:?}"
    );
    // THE regression: no restored card may sit in a live-only state.
    for (name, status) in &tools {
        assert!(
            matches!(status, ToolStatus::Ok | ToolStatus::Failed),
            "restored card '{name}' stuck in non-final state {status:?}"
        );
    }
}

#[test]
fn slimmed_completion_still_finishes_the_card() {
    // The write_file batch in the fixture has payload.tool_calls slimmed
    // to a `$slim` marker on BOTH its waiting and completed records —
    // the completion must still flip the card and carry the result.
    let fold = fold_fixture();
    let write_file = fold
        .items
        .iter()
        .find_map(|i| match i {
            Item::Tool {
                name,
                status,
                result_preview,
                ..
            } if name == "write_file" => Some((status, result_preview)),
            _ => None,
        })
        .expect("write_file card present");
    assert_eq!(*write_file.0, ToolStatus::Ok);
    assert!(
        write_file.1.contains("Successfully written"),
        "result preview from the slimmed record's results: {}",
        write_file.1
    );
}

#[test]
fn per_result_success_flags_are_honored() {
    // First batch (unslimmed): list_files failed, execute_command
    // succeeded — statuses come from each result row, not the batch.
    let fold = fold_fixture();
    let status_of = |wanted: &str| {
        fold.items.iter().find_map(|i| match i {
            Item::Tool { name, status, .. } if name == wanted => Some(*status),
            _ => None,
        })
    };
    assert_eq!(status_of("list_files"), Some(ToolStatus::Failed));
    assert_eq!(status_of("execute_command"), Some(ToolStatus::Ok));
}

#[test]
fn restore_counts_slimmed_tool_calls_and_holds_no_wait() {
    let fold = fold_fixture();
    assert!(
        fold.pending_wait.is_none(),
        "a prior run's answered waits must never prompt after restore"
    );
    // 2 calls in the unslimmed batch + 1 in the slimmed batch: the count
    // must not drop to 2 because the slimmed payload list is a marker.
    assert_eq!(fold.stats.tool_calls, 3);
    assert!(fold.stats.llm_calls >= 2);
}
