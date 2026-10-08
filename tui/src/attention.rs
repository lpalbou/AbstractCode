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
//! The poll is light: one read every [`POLL`] ONLY while it can change what
//! the chip says without you — an active automation runs in Ask mode (it can
//! stop for a tool approval at any time) or something is already waiting.
//! Otherwise nothing polls; the list is read again when `/automations` is
//! used or the gateway comes back. Whether an automation asks is its
//! definition's `tool_approval` (not in the list row): read once per
//! revision ([`AskModes`]), never again until the revision changes.

use crate::automations::Summary;

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
