//! MANUAL live gate (never CI): one full visit cycle against `doorcheck`,
//! the sanctioned fixture entity (door/gate checking is its purpose).
//!
//! Rules (plan cycle-2, live-test gating): doorcheck ONLY — castor,
//! mnemosyne, hypnos, ephemeral are real lives and never test targets.
//! Opening WAKES an asleep doorcheck (B1); the close (closed_by=operator)
//! restores the prior state server-side. One visit per gate run.
//!
//! Run explicitly:
//! ```sh
//! ABSTRACTCODE_GATEWAY_TOKEN=... cargo test --test live_doorcheck_gate -- --ignored --nocapture
//! ```

use abstractcode_tui::convo::{self, ConvoStatus, EntityConvo};
use abstractcode_tui::entities::{
    close_from_response, cognition_from_response, transcript_from_response, turn_from_response,
    visit_open_from_response, visit_status_from_response,
};
use abstractcode_tui::gateway::entities::EntityClient;
use abstractcode_tui::transcript::Item;

#[test]
#[ignore = "manual live gate: spends tokens on the sanctioned doorcheck entity"]
fn one_visit_cycle_against_doorcheck() {
    let base = std::env::var("ABSTRACTCODE_GATEWAY_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let token = std::env::var("ABSTRACTCODE_GATEWAY_TOKEN").ok();
    let client = EntityClient::new(&base, token.as_deref());

    // Prior state (for the close-restores honesty check).
    let before = cognition_from_response(&client.cognition("doorcheck").expect("cognition"));
    println!("doorcheck state before: {}", before.state);

    // The client-side conversation, driven through the SAME pure folds
    // the UI uses — the live gate proves the whole lane, not just HTTP.
    let mut c = EntityConvo::opening("doorcheck", &before.state);

    // OPEN (or adopt a leftover live visit via the structured 409 path).
    let run_id = match client.visit_open("doorcheck") {
        Ok(v) => {
            let open = visit_open_from_response(&v);
            println!(
                "opened visit run={} visit_id={}",
                open.run_id, open.visit_id
            );
            convo::fold_open_success(&mut c, &open);
            open.run_id
        }
        Err(e) if e.status == Some(409) => {
            let sv = client.visit_status("doorcheck").expect("visit status");
            let status = visit_status_from_response(&sv);
            assert!(status.open, "non-adoptable 409: {}", e.message);
            let tv = client
                .visit_transcript("doorcheck", &status.run_id)
                .expect("transcript");
            let transcript = transcript_from_response(&tv);
            println!(
                "adopted live visit run={} turn_n={}",
                status.run_id, status.turn_n
            );
            convo::fold_adopt(&mut c, &status, &transcript);
            status.run_id
        }
        Err(e) => panic!("open failed: {e}"),
    };
    assert!(matches!(c.status, ConvoStatus::Ready | ConvoStatus::Parked));

    // ONE tiny turn.
    let epoch = convo::fold_send_turn(&mut c, "hello, connectivity check from the TUI build");
    let tv = client
        .visit_turn(
            "doorcheck",
            &run_id,
            "hello, connectivity check from the TUI build",
        )
        .expect("turn HTTP");
    let resp = turn_from_response(&tv);
    println!(
        "turn status={} turn_n={} tools_ran={:?} reply={:?}",
        resp.status, resp.turn_n, resp.tools_ran, resp.reply
    );
    assert_ne!(resp.status, "failed", "turn failed: {}", resp.error);
    assert!(!resp.reply.trim().is_empty(), "an answer came back");
    let held = convo::fold_turn_reply(&mut c, &resp);
    assert_eq!(held, None);
    assert_eq!(c.status, ConvoStatus::Parked);
    assert_eq!(c.turn_epoch, epoch, "no epoch drift through the cycle");
    assert!(
        c.items.iter().any(|i| matches!(
            i,
            Item::Assistant {
                final_answer: true,
                ..
            }
        )),
        "the reply rendered as the final assistant card"
    );

    // CLOSE with closed_by=operator — reflection runs; prior state restores.
    let cv = client
        .visit_close("doorcheck", &run_id, "abstractcode-tui live gate")
        .expect("close HTTP");
    let close = close_from_response(&cv);
    println!("close status={} summary={:?}", close.status, close.summary);
    assert_eq!(close.status, "completed", "close completed");
    convo::fold_close(&mut c, &close);
    assert_eq!(c.status, ConvoStatus::Closed);

    // The visit is gone and the prior state is restored (B1: close
    // restores the operator's pre-visit word — asleep stays asleep).
    let after_status =
        visit_status_from_response(&client.visit_status("doorcheck").expect("status"));
    assert!(!after_status.open, "no live visit remains");
    let after = cognition_from_response(&client.cognition("doorcheck").expect("cognition"));
    println!("doorcheck state after: {}", after.state);
    assert_eq!(
        after.state, before.state,
        "close restored the pre-visit state"
    );
}
