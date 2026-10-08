//! Automations v1 contract checks against the canonical wire fixtures.
//!
//! `tests/fixtures/automations/*.json` are BYTE-IDENTICAL copies of the
//! ui-kit's canonical fixtures (`abstractuic/ui-kit/scripts/fixtures/
//! automations`, the same copies the Assistant vendors); `CHECKSUMS.sha256`
//! comes with them and is verified here, so a local edit of a copy fails
//! loudly instead of drifting from the other clients.

use serde_json::{json, Value};

use abstractcode::automations as auto;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/automations");
const FILES: [&str; 6] = [
    "attention.json",
    "commands.json",
    "errors.json",
    "list.json",
    "occurrences.json",
    "trigger-sources.json",
];

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{DIR}/{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn load(name: &str) -> Value {
    serde_json::from_slice(&read(name)).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

/// SHA-256 (FIPS 180-4) — the crate carries no hashing dependency, and the
/// checksum file is the canonical side's own format (`shasum -a 256`).
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate().take(16) {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[test]
fn sha256_matches_a_known_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn vendored_fixtures_are_byte_identical_to_the_canonical_checksums() {
    let recorded = String::from_utf8(read("CHECKSUMS.sha256")).unwrap();
    let computed: String = FILES
        .iter()
        .map(|f| format!("{}  {f}\n", sha256_hex(&read(f))))
        .collect();
    assert_eq!(
        recorded, computed,
        "a vendored fixture drifted from the canonical ui-kit copy"
    );
}

fn summaries() -> Vec<auto::Summary> {
    auto::parse_list_page(&load("list.json"))
        .expect("list.json parses")
        .items
}

fn by_title(title: &str) -> auto::Summary {
    summaries()
        .into_iter()
        .find(|s| s.title == title)
        .expect(title)
}

#[test]
fn every_list_row_parses_and_reads_state_from_the_gateway() {
    let rows = summaries();
    assert_eq!(rows.len(), 5);
    let inbox = by_title("Inbox triage");
    // In progress = `current_occurrence`; next = the served `next_run_at` /
    // `next_run_local` (also while running), cut — never computed.
    assert_eq!(
        auto::current_label(&inbox).as_deref(),
        Some("Run #7 running")
    );
    let now = auto::now_unix();
    assert!(auto::next_label(&inbox, now).starts_with("2026-09-27 09:00 Europe/Paris ("));
    assert_eq!(
        auto::attention_label(&inbox),
        "2 unseen · 2 waiting for you"
    );
    assert_eq!(
        auto::attention_ack_cursor(&inbox).as_deref(),
        Some("att1:2")
    );
    assert_eq!(
        auto::control_state(&inbox, auto::Control::RunNow, false),
        Err("An occurrence is in progress.".into())
    );
    assert_eq!(
        auto::control_state(&inbox, auto::Control::StopCurrent, false),
        Ok(())
    );
    assert!(inbox
        .workspace_root
        .as_deref()
        .unwrap()
        .contains("session-automation-53443dd0"));
    let row = auto::row_line(&inbox, now);
    assert!(row.starts_with("Inbox triage · Active ▶ · 2 unseen · 2 waiting for you · Every 30 minutes (UTC) · now: Run #7 running · next: "), "{row}");

    let paused = by_title("Weekly journal monitor");
    assert_eq!(auto::status_label(&paused.status), "Paused ⏸");
    assert_eq!(auto::next_label(&paused, now), "none while paused");
    assert_eq!(
        auto::control_state(&paused, auto::Control::RunNow, false),
        Ok(())
    );
    assert_eq!(
        auto::control_state(&paused, auto::Control::Resume, false),
        Ok(())
    );

    // The schedule@2 daily row: the served rule on the card, the served next run.
    let daily = by_title("Morning briefing");
    assert_eq!(daily.trigger.source_version, 2);
    let (line1, _) = auto::card_lines(&daily, now);
    assert!(
        line1.starts_with("↻ Every day at 08:00 (Europe/Paris) · last "),
        "{line1}"
    );
    assert!(auto::next_label(&daily, now).starts_with("2026-09-28 08:00 Europe/Paris ("));
    assert!(auto::row_line(&daily, now)
        .contains(" · Every day at 08:00 (Europe/Paris) · next: 2026-09-28 08:00 Europe/Paris ("));

    let legacy = rows.iter().find(|s| s.legacy).unwrap();
    assert!(legacy.current.is_none());
    for c in [
        auto::Control::Pause,
        auto::Control::RunNow,
        auto::Control::Archive,
        auto::Control::Discuss,
    ] {
        assert!(auto::control_state(legacy, c, false).is_err());
    }
}

/// A summary without the served schedule fields is a broken R16.1 seam: it
/// fails loudly instead of showing a next run or a sentence made up here.
#[test]
fn a_row_without_the_served_schedule_fields_is_refused() {
    let mut v = load("list.json");
    v["items"][0]
        .as_object_mut()
        .unwrap()
        .remove("schedule_rule_text");
    let err = auto::parse_list_page(&v).unwrap_err();
    assert!(err.contains("schedule_rule_text"), "{err}");
}

/// The vendored wording (`assets/automation_controls.json`) is BYTE-IDENTICAL
/// to the ui-kit's canonical file: its SHA-256 is pinned in
/// `assets/automation_controls.sha256` (the kit's own `shasum -a 256` line).
#[test]
fn vendored_controls_wording_is_byte_identical_to_the_kit() {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
    let json = std::fs::read(format!("{assets}/automation_controls.json")).unwrap();
    let pinned = std::fs::read_to_string(format!("{assets}/automation_controls.sha256")).unwrap();
    assert_eq!(
        pinned,
        format!("{}  automation_controls.json\n", sha256_hex(&json)),
        "assets/automation_controls.json drifted from the canonical ui-kit copy"
    );
    // The When step's words come from this file (the `schedule` block).
    let spec: serde_json::Value = serde_json::from_slice(&json).unwrap();
    for key in [
        "kind_every",
        "kind_daily",
        "kind_weekly",
        "kind_monthly",
        "kind_once",
        "time_zone_line",
        "time_zone_hint",
        "describing",
        "error_at",
        "error_days",
        "error_day",
        "error_once",
    ] {
        assert_eq!(
            spec["schedule"][key].as_str(),
            Some(auto::schedule_text(key)),
            "{key}"
        );
    }
    assert_eq!(auto::day_label("mon"), "Mon");
}

#[test]
fn occurrences_parse_as_chat_pairs_with_badges() {
    let page = auto::parse_occurrence_page(&load("occurrences.json")).unwrap();
    let rows = auto::merge_occurrences(&[], &page.items);
    assert_eq!(
        rows.iter().map(|o| o.index).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 7]
    );
    let badge = |i: u64| auto::occurrence_badge(rows.iter().find(|o| o.index == i).unwrap());
    assert_eq!(badge(7), "Waiting for you");
    assert_eq!(badge(5), "Failed after 3 attempts");
    assert_eq!(badge(2), "Notified");
    assert_eq!(badge(1), "");
    let third = rows.iter().find(|o| o.index == 3).unwrap();
    assert!(auto::occurrence_header(third).starts_with("#3 · completed after 2 attempts"));
    assert!(!auto::can_discuss(
        rows.iter().find(|o| o.index == 7).unwrap()
    ));
    assert!(auto::can_discuss(third));
}

#[test]
fn every_request_matches_the_fixture_wire() {
    let fx = load("commands.json");
    let item = |name: &str| -> Value {
        fx["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["name"] == name)
            .unwrap_or_else(|| panic!("{name}"))
            .clone()
    };
    let check = |name: &str, built: auto::Request| {
        let want = item(name)["request"].clone();
        assert_eq!(built.method, want["method"], "{name}");
        assert_eq!(built.path, want["path"], "{name}");
        assert_eq!(built.body.unwrap_or(Value::Null), want["body"], "{name}");
    };
    for (name, ty) in [
        ("pause", "automation.pause"),
        ("run_now while paused", "automation.run_now"),
        ("resume", "automation.resume"),
        ("stop_current", "automation.stop_current"),
        ("archive", "automation.archive"),
    ] {
        let body = &item(name)["request"]["body"];
        let path = item(name)["request"]["path"].as_str().unwrap().to_string();
        let id = path
            .trim_start_matches("/api/gateway/automations/")
            .trim_end_matches("/commands")
            .to_string();
        check(
            name,
            auto::command_request(&id, body["command_id"].as_str().unwrap(), ty),
        );
    }
    check(
        "seen",
        auto::seen_request("53443dd0-25c4-5fa8-bdad-e1ac3fdfff8e", "att1:2"),
    );
    check(
        "discuss",
        auto::discuss_request(
            "53443dd0-25c4-5fa8-bdad-e1ac3fdfff8e",
            "38d02117-24d3-4277-8309-684992fc255e",
            2,
            "Draft the reply to Clara.",
        ),
    );
    // Revise: the changes come from the revise rule applied to the listed row.
    let rev = item("revise");
    let id = "fddce731-4abf-54d3-81b9-15856efbfd7a";
    let s = summaries()
        .into_iter()
        .find(|s| s.id == id)
        .expect("revised row is listed");
    let mut form = auto::revise_form_from(&s);
    form.every = Some("6h".into());
    let changes = auto::revise_changes(&s, &form).unwrap().unwrap();
    check(
        "revise",
        auto::revise_request(
            id,
            rev["request"]["body"]["command_id"].as_str().unwrap(),
            s.revision,
            changes,
        ),
    );
    // Wait answers: the payload follows the wait kind; only the client id differs.
    let waits = by_title("Inbox triage").attention.waits;
    for (name, kind, answer) in [
        ("answer ask_user wait", "ask_user", "Reply: Tuesday works"),
        ("answer tool_approval wait", "tool_approval", "approve"),
    ] {
        let want = item(name)["request"].clone();
        let w = waits.iter().find(|w| w.kind == kind).unwrap();
        let payload = auto::wait_answer_payload(kind, answer).unwrap();
        let mut built =
            auto::wait_answer_request(want["body"]["command_id"].as_str().unwrap(), w, payload);
        built.body.as_mut().unwrap()["client_id"] = json!("web_pwa");
        check(name, built);
    }
    // The discuss answer parses (both folders named).
    let d = auto::parse_discuss(&item("discuss")["response"]).unwrap();
    assert!(d.session_id.starts_with("discussion-session:"));
    assert!(!d.mounted_workspace.is_empty() && !d.workspace_root.is_empty());
}

#[test]
fn every_error_body_maps_to_its_code_and_one_sentence() {
    let fx = load("errors.json");
    for item in fx["items"].as_array().unwrap() {
        let status = item["status"].as_u64().unwrap() as u16;
        let body = item["body"].to_string();
        let e = auto::parse_api_error(status, &body);
        assert_eq!(
            e.code, item["body"]["detail"]["reason_code"],
            "{}",
            item["name"]
        );
        assert!(
            !auto::api_error_text(&e).contains("refused the request ("),
            "{}: unmapped code {}",
            item["name"],
            e.code
        );
    }
}

#[test]
fn trigger_sources_list_both_v1_sources() {
    let fx = load("trigger-sources.json");
    let ids: Vec<&str> = fx["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert!(
        ids.contains(&"schedule") && ids.contains(&"manual"),
        "{ids:?}"
    );
}
