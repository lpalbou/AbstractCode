//! Ambient automation attention (R17.1): the status-line chip
//! "Automations · N waiting", outside `/automations`.
//!
//! N is the Code web's Automations header badge, the same sum over the same
//! list (`web/src/workspace/automations_view.tsx:110`):
//! `items.reduce((n, s) => n + s.attention.pending_waits + s.attention.unseen_count, 0)`
//! over `GET /api/gateway/automations` (archived automations are not in
//! `items`). Enter on an empty prompt (or a click on the chip) opens the
//! automation that waits — the first one with a pending wait, else the first
//! with unseen items, in the gateway's order.
//!
//! Since backlog 1047 the chip follows the person's own event stream
//! (`GET /api/gateway/me/events?after=` catch-up, `/me/events/stream` SSE):
//! [`EventAttention`] folds it (raised minus seen over automation results and
//! the approvals/questions that wait for you), the SSE tail runs only while
//! the terminal is in front ([`foreground`]: someone typed or clicked in the
//! last [`FOREGROUND_IDLE`]), otherwise one catch-up every [`CATCH_UP_EVERY`].
//! An automation event refreshes the list (the chip's Enter opens from it).
//! The list poll below is kept ONLY for a gateway without the stream (404).
//!
//! The poll is light: one read every [`POLL`] ONLY while it can change what
//! the chip says without you — an active automation runs in Ask mode (it can
//! stop for a tool approval at any time) or something is already waiting.
//! Otherwise nothing polls; the list is read again when `/automations` is
//! used or the gateway comes back. Whether an automation asks is its
//! definition's `tool_approval` (not in the list row): read once per
//! revision ([`AskModes`]), never again until the revision changes.

use crate::automations::Summary;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The stream's live tail runs while someone used the terminal this recently.
pub const FOREGROUND_IDLE: Duration = Duration::from_secs(5 * 60);
/// Away from the terminal: one catch-up read this often (no open stream).
pub const CATCH_UP_EVERY: Duration = Duration::from_secs(60);

/// Seconds since the epoch of the last key, click or paste (0 = none yet).
static LAST_INPUT: AtomicU64 = AtomicU64::new(0);

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Called by the root's capture handler on every key, click and paste.
pub fn note_input() {
    LAST_INPUT.store(now_s(), Ordering::Relaxed);
}

/// The terminal is in front: someone typed or clicked within
/// [`FOREGROUND_IDLE`]. (The engine does not hand terminal focus events to
/// the app; input is the proof someone is there.)
pub fn foreground() -> bool {
    foreground_at(LAST_INPUT.load(Ordering::Relaxed), now_s())
}

pub fn foreground_at(last_input_s: u64, now_s: u64) -> bool {
    last_input_s > 0 && now_s.saturating_sub(last_input_s) < FOREGROUND_IDLE.as_secs()
}

/// What the stream says waits for you (raised minus seen), folded in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventAttention {
    /// The last sequence folded (the resume point).
    pub cursor: u64,
    /// Open approvals/questions: (run id, wait key, step id) -> session id
    /// (the same node asking again is a new wait).
    pub waits: BTreeMap<(String, String, String), String>,
    /// Per automation: raised item seqs above the latest seen seq.
    pub raised: BTreeMap<String, Vec<u64>>,
    pub seen: BTreeMap<String, u64>,
    /// Record keys seen (a seen folded before its raise keeps it closed).
    pub resolved: Vec<String>,
    /// Bumped by every automation-shaped event: the list is read again.
    pub automation_epoch: u64,
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str).filter(|x| !x.is_empty())
}

impl EventAttention {
    /// Approvals and questions waiting.
    pub fn needs_you(&self) -> u64 {
        self.waits.len() as u64
    }

    /// Automation results not seen yet.
    pub fn new_results(&self) -> u64 {
        self.raised
            .iter()
            .map(|(a, seqs)| {
                seqs.iter()
                    .filter(|q| **q > *self.seen.get(a).unwrap_or(&0))
                    .count() as u64
            })
            .sum()
    }

    /// Fold one CloudEvents event (`{type, data, sequence}`); a sequence at
    /// or below the cursor was folded already and is skipped.
    pub fn apply(&mut self, ev: &Value) {
        let seq = ev.get("sequence").and_then(Value::as_u64).unwrap_or(0);
        if seq > 0 {
            if seq <= self.cursor {
                return;
            }
            self.cursor = seq;
        }
        let typ = ev.get("type").and_then(Value::as_str).unwrap_or("");
        let empty = Value::Null;
        let data = ev.get("data").unwrap_or(&empty);
        match typ {
            "attention.raised" => {
                if let (Some(run), Some(key)) = (s(data, "run_id"), s(data, "wait_key")) {
                    let step = s(data, "step_id").unwrap_or("");
                    let step_mark = format!("step:{run}:{key}:{step}");
                    if s(data, "record_key").is_some_and(|r| self.resolved.iter().any(|x| x == r))
                        || (!step.is_empty() && self.resolved.contains(&step_mark))
                    {
                        return;
                    }
                    self.waits.insert(
                        (run.to_string(), key.to_string(), step.to_string()),
                        s(data, "session_id").unwrap_or("").to_string(),
                    );
                } else if let (Some(aid), Some(q)) = (
                    s(data, "automation_id"),
                    data.get("seq").and_then(Value::as_u64),
                ) {
                    let seen = *self.seen.get(aid).unwrap_or(&0);
                    let list = self.raised.entry(aid.to_string()).or_default();
                    if q > seen && !list.contains(&q) {
                        list.push(q);
                    }
                    self.automation_epoch += 1;
                }
            }
            "attention.seen" => {
                if let Some(r) = s(data, "record_key") {
                    self.resolved.push(r.to_string());
                }
                // The gateway's seen of a wait: `{kind, run_id, wait_key, step_id, outcome}`.
                if let (Some(run), Some(key)) = (s(data, "run_id"), s(data, "wait_key")) {
                    let step = s(data, "step_id").unwrap_or("");
                    if !step.is_empty() {
                        self.resolved.push(format!("step:{run}:{key}:{step}"));
                    }
                    self.waits.retain(|(r, k, st), _| {
                        !(r == run && k == key && (step.is_empty() || st.is_empty() || st == step))
                    });
                }
                while self.resolved.len() > 500 {
                    self.resolved.remove(0);
                }
                if let Some(aid) = s(data, "automation_id") {
                    let q = data.get("seq").and_then(Value::as_u64).unwrap_or(0);
                    let seen = self.seen.entry(aid.to_string()).or_insert(0);
                    *seen = (*seen).max(q);
                    let seen = *seen;
                    if let Some(list) = self.raised.get_mut(aid) {
                        list.retain(|x| *x > seen);
                    }
                    self.automation_epoch += 1;
                }
            }
            "run.finished" | "run.started" => {
                if typ == "run.finished" {
                    if let Some(run) = s(data, "run_id") {
                        self.waits.retain(|(r, _, _), _| r != run);
                    }
                }
                let trigger =
                    data.pointer("/origin/kind").and_then(Value::as_str) == Some("trigger");
                if trigger {
                    self.automation_epoch += 1;
                }
            }
            "trigger.event" | "wake.scheduled" | "wake.cancelled" => self.automation_epoch += 1,
            _ => {}
        }
    }
}

/// The chip from the stream; `None` when nothing waits.
pub fn chip_events(needs_you: u64, new_results: u64) -> Option<String> {
    match (needs_you, new_results) {
        (0, 0) => None,
        (0, m) => Some(format!("Automations · {m} new")),
        (n, 0) => Some(format!("{n} waiting for you")),
        (n, m) => Some(format!("{n} waiting for you · {m} new")),
    }
}

/// Where the person's event stream stands for this terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventsState {
    /// Not asked yet (the list poll rules, as before).
    #[default]
    Unknown,
    /// Caught up (catch-up reads or the live tail): the chip follows it.
    Following,
    /// The gateway has no `/me/events` (404): the list poll stays.
    Unsupported,
}

/// The poll cadence while the chip matters.
pub const POLL: std::time::Duration = std::time::Duration::from_secs(15);

/// The waiting count of one automation (web: `pending_waits + unseen_count`).
pub fn waiting(s: &Summary) -> u64 {
    s.attention.pending_waits + s.attention.unseen_count
}

/// The chip's N: the sum over the list (the web header's badge).
pub fn waiting_total(items: &[Summary]) -> u64 {
    items.iter().map(waiting).sum()
}

/// The chip's text; `None` when nothing waits (no chip at all).
pub fn chip(total: u64) -> Option<String> {
    (total > 0).then(|| format!("Automations · {total} waiting"))
}

/// The automation the chip opens: the first with a pending wait (it is
/// stopped until you answer), else the first with unseen items.
pub fn first_waiting(items: &[Summary]) -> Option<String> {
    items
        .iter()
        .find(|s| s.attention.pending_waits > 0)
        .or_else(|| items.iter().find(|s| s.attention.unseen_count > 0))
        .map(|s| s.id.clone())
}

/// Which automations ask before each tool call, keyed by revision: an
/// entry is re-read only when the list shows another revision.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AskModes {
    /// (automation id, revision, asks).
    pub known: Vec<(String, u64, bool)>,
}

impl AskModes {
    /// `(id, revision)` of every entry already read (sent with the poll so
    /// the lane skips their definitions).
    pub fn revisions(&self) -> Vec<(String, u64)> {
        self.known.iter().map(|(i, r, _)| (i.clone(), *r)).collect()
    }

    pub fn asks(&self, id: &str) -> bool {
        self.known.iter().any(|(i, _, a)| i == id && *a)
    }

    /// Fold the lane's answers in (and forget automations no longer listed).
    pub fn merge(&mut self, read: Vec<(String, u64, bool)>, listed: &[Summary]) {
        for (id, rev, asks) in read {
            self.known.retain(|(i, _, _)| *i != id);
            self.known.push((id, rev, asks));
        }
        self.known
            .retain(|(i, _, _)| listed.iter().any(|s| s.id == *i));
    }
}

/// The definitions to read: active, non-legacy automations whose revision
/// is not known yet (paused/completed ones cannot stop for an approval).
pub fn to_read(items: &[Summary], known: &[(String, u64)]) -> Vec<(String, u64)> {
    items
        .iter()
        .filter(|s| s.status == "active" && !s.legacy)
        .filter_map(|s| {
            let rev = s.revision.unwrap_or(0);
            (!known.iter().any(|(i, r)| *i == s.id && *r == rev)).then(|| (s.id.clone(), rev))
        })
        .collect()
}

/// Whether the poll runs: something waits, or an active automation asks.
pub fn should_poll(items: &[Summary], ask: &AskModes) -> bool {
    items
        .iter()
        .any(|s| waiting(s) > 0 || (s.status == "active" && ask.asks(&s.id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automations::parse_list_page;

    const LIST: &str = include_str!("../tests/fixtures/automations/list.json");

    fn items() -> Vec<Summary> {
        parse_list_page(&serde_json::from_str(LIST).unwrap())
            .unwrap()
            .items
    }

    #[test]
    fn the_total_is_the_web_headers_sum() {
        let items = items();
        // Hand-summed from the fixture: pending_waits + unseen_count per row.
        let raw: serde_json::Value = serde_json::from_str(LIST).unwrap();
        let expected: u64 = raw["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                r["attention"]["pending_waits"].as_u64().unwrap()
                    + r["attention"]["unseen_count"].as_u64().unwrap()
            })
            .sum();
        assert!(expected > 0, "the fixture has attention");
        assert_eq!(waiting_total(&items), expected);
        assert_eq!(
            chip(expected).as_deref(),
            Some(format!("Automations · {expected} waiting").as_str())
        );
        assert_eq!(chip(0), None, "nothing waits: no chip");
    }

    #[test]
    fn the_chip_opens_the_one_with_a_pending_wait_first() {
        let mut items = items();
        for s in &mut items {
            s.attention.pending_waits = 0;
            s.attention.unseen_count = 0;
        }
        assert_eq!(first_waiting(&items), None);
        let last = items.len() - 1;
        items[0].attention.unseen_count = 3;
        items[last].attention.pending_waits = 1;
        assert_eq!(first_waiting(&items), Some(items[last].id.clone()));
        items[last].attention.pending_waits = 0;
        assert_eq!(first_waiting(&items), Some(items[0].id.clone()));
    }

    #[test]
    fn the_poll_runs_only_while_something_waits_or_an_active_one_asks() {
        let mut items = items();
        for s in &mut items {
            s.attention.pending_waits = 0;
            s.attention.unseen_count = 0;
        }
        let mut ask = AskModes::default();
        assert!(
            !should_poll(&items, &ask),
            "quiet list, no Ask mode: no poll"
        );
        let active = items.iter().position(|s| s.status == "active").unwrap();
        let id = items[active].id.clone();
        ask.merge(vec![(id.clone(), 1, true)], &items);
        assert!(should_poll(&items, &ask), "an active automation asks");
        items[active].status = "paused".into();
        assert!(
            !should_poll(&items, &ask),
            "a paused one cannot stop for approval"
        );
        items[0].attention.unseen_count = 1;
        assert!(should_poll(&items, &ask), "unseen items keep it polling");
    }

    fn ev(seq: u64, typ: &str, data: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"specversion": "1.0", "id": format!("{typ}:{seq}"), "type": typ, "data": data, "sequence": seq})
    }

    #[test]
    fn the_stream_folds_raised_minus_seen() {
        use serde_json::json;
        let mut a = EventAttention::default();
        a.apply(&ev(1, "attention.raised", json!({"automation_id": "x", "seq": 1, "kind": "notify", "record_key": "automation:completed:x:1"})));
        a.apply(&ev(
            2,
            "attention.raised",
            json!({"automation_id": "x", "seq": 2, "kind": "failure"}),
        ));
        // The gateway's wait shapes (L3 item 4b): seen names the step, no record_key.
        a.apply(&ev(3, "attention.raised", json!({"kind": "approval", "wait_kind": "tool_approval", "run_id": "r1", "session_id": "s", "wait_key": "k1", "step_id": "a", "record_key": "wait:r1:k1:a"})));
        a.apply(&ev(4, "attention.raised", json!({"kind": "question", "wait_kind": "ask_user", "run_id": "r2", "session_id": "s", "wait_key": "k2", "step_id": "b", "record_key": "wait:r2:k2:b"})));
        assert_eq!((a.needs_you(), a.new_results()), (2, 2));
        assert_eq!(
            chip_events(a.needs_you(), a.new_results()).as_deref(),
            Some("2 waiting for you · 2 new")
        );
        let epoch = a.automation_epoch;
        a.apply(&ev(
            5,
            "attention.seen",
            json!({"automation_id": "x", "seq": 1}),
        ));
        a.apply(&ev(
            6,
            "attention.seen",
            json!({"kind": "approval", "run_id": "r1", "wait_key": "k1", "step_id": "a", "outcome": "approved"}),
        ));
        a.apply(&ev(7, "run.finished", json!({"run_id": "r2", "status": "completed", "origin": {"kind": "trigger", "ref": {"automation_id": "x"}}})));
        assert_eq!((a.needs_you(), a.new_results()), (0, 1));
        assert!(
            a.automation_epoch > epoch,
            "automation events re-read the list"
        );
        assert_eq!(chip_events(0, 1).as_deref(), Some("Automations · 1 new"));
        // A frame read twice (a reconnect) folds once; a seen item never returns.
        a.apply(&ev(
            7,
            "attention.raised",
            json!({"automation_id": "x", "seq": 9}),
        ));
        a.apply(&ev(
            8,
            "attention.raised",
            json!({"automation_id": "x", "seq": 1}),
        ));
        assert_eq!(a.new_results(), 1);
        a.apply(&ev(
            9,
            "attention.seen",
            json!({"automation_id": "x", "seq": 2}),
        ));
        assert_eq!(chip_events(a.needs_you(), a.new_results()), None);
        // The same node asking again is a new wait; an older step's seen leaves it open.
        a.apply(&ev(
            10,
            "attention.raised",
            json!({"kind": "approval", "run_id": "r3", "wait_key": "k", "step_id": "s2"}),
        ));
        a.apply(&ev(11, "attention.seen", json!({"kind": "approval", "run_id": "r3", "wait_key": "k", "step_id": "s1", "outcome": "approved"})));
        assert_eq!(a.needs_you(), 1);
        a.apply(&ev(12, "attention.seen", json!({"kind": "approval", "run_id": "r3", "wait_key": "k", "step_id": "s2", "outcome": "denied"})));
        assert_eq!(a.needs_you(), 0);
    }

    #[test]
    fn the_live_tail_runs_only_while_someone_is_at_the_terminal() {
        assert!(!foreground_at(0, 1_000), "no input yet: not in front");
        assert!(foreground_at(1_000, 1_000 + 299));
        assert!(
            !foreground_at(1_000, 1_000 + 300),
            "five quiet minutes: catch-up only"
        );
    }

    #[test]
    fn definitions_are_read_once_per_revision() {
        let items = items();
        let active: Vec<_> = items
            .iter()
            .filter(|s| s.status == "active" && !s.legacy)
            .collect();
        let first = to_read(&items, &[]);
        assert_eq!(first.len(), active.len());
        assert!(
            to_read(&items, &first).is_empty(),
            "known revisions are skipped"
        );
        let bumped: Vec<_> = first.iter().map(|(i, r)| (i.clone(), r + 1)).collect();
        assert_eq!(
            to_read(&items, &bumped).len(),
            active.len(),
            "a new revision is re-read"
        );
        let mut ask = AskModes::default();
        ask.merge(vec![("gone".into(), 1, true)], &items);
        assert!(
            ask.known.is_empty(),
            "an automation no longer listed is forgotten"
        );
    }
}
