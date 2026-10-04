//! Conversation cards (R7.3): the Code WUI sidebar's wording, in the
//! terminal — pure and test-pinned.
//!
//! A card is its title (the opening prompt, else `Conversation <8 chars>`)
//! and one quiet line `Oct 2 · 2 turns · 7 tools`: the day of the newest
//! turn (month + day in local time, never a year), the turns (root runs),
//! and the tool calls across them — the gateway's per-turn `tool_calls`
//! summed (`include_metrics=true`), shown only when above zero and only
//! when every turn reported a number.

/// The Archive question under a card, verbatim from the Code web.
pub const ARCHIVE_QUESTION: &str =
    "Archive this conversation? It stays searchable and auditable; it just leaves this list.";

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The card title: the opening prompt on one line, else
/// `Conversation <id>` — the id's readable TAIL when long (this
/// client's ids all begin `acode-`, so the web's first 8 characters would
/// name nothing; ids differ at the end).
pub fn card_title(label: &str, id: &str) -> String {
    let one = label
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if one.is_empty() {
        {
            let n = id.chars().count();
            let tail = if n > 22 {
                format!("…{}", id.chars().skip(n - 21).collect::<String>())
            } else {
                id.to_string()
            };
            format!("Conversation {tail}")
        }
    } else {
        one.to_string()
    }
}

/// "Oct 2" for an RFC3339 stamp shifted by `offset_secs` (the viewer's
/// UTC offset); `None` when the stamp does not parse.
pub fn day_label(stamp: &str, offset_secs: i64) -> Option<String> {
    let t = crate::protocol::parse_rfc3339_utc(stamp)?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    let (_, m, d) = crate::config::civil_from_days((t + offset_secs).div_euclid(86_400));
    Some(format!("{} {d}", MONTHS[(m as usize).clamp(1, 12) - 1]))
}

/// `Oct 2 · 2 turns · 7 tools` (singular at exactly 1; no tool figure at 0
/// or when unknown; `Saved conversation` when the day is unknown).
/// `floor` = the listing was truncated, so the turn count is a floor
/// ("3+ turns") — never a page-bounded number passed off as a total.
pub fn meta_line(
    last_at: &str,
    turns: usize,
    tools: Option<u64>,
    floor: bool,
    offset_secs: i64,
) -> String {
    let mut parts =
        vec![day_label(last_at, offset_secs).unwrap_or_else(|| "Saved conversation".into())];
    if turns > 0 {
        let plus = if floor { "+" } else { "" };
        parts.push(format!(
            "{turns}{plus} {}",
            if turns == 1 && !floor {
                "turn"
            } else {
                "turns"
            }
        ));
    }
    if let Some(n) = tools.filter(|n| *n > 0) {
        parts.push(format!("{n} {}", if n == 1 { "tool" } else { "tools" }));
    }
    parts.join(" · ")
}

/// The conversation to open after archiving `archived` when it was the
/// open one: the next in the list, else the previous; `None` = start a new
/// conversation (the web's `nextConversationAfterArchive`).
pub fn next_after_archive(ids: &[String], archived: &str) -> Option<String> {
    let i = ids.iter().position(|id| id == archived)?;
    ids.get(i + 1)
        .or_else(|| i.checked_sub(1).and_then(|p| ids.get(p)))
        .cloned()
}

/// The viewer's UTC offset in seconds at `t` (unix seconds): local time on
/// Unix through `localtime_r`, UTC elsewhere.
pub fn local_offset_secs(t: i64) -> i64 {
    #[cfg(unix)]
    {
        let tt = t as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: both pointers are valid for the call; `localtime_r` is the
        // re-entrant form and writes only into `tm`.
        let ok = unsafe { !libc::localtime_r(&tt, &mut tm).is_null() };
        if ok {
            return tm.tm_gmtoff as i64;
        }
        0
    }
    #[cfg(not(unix))]
    {
        let _ = t;
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meta_line_is_the_web_card_line() {
        assert_eq!(
            meta_line("2026-10-02T09:00:00Z", 2, Some(7), false, 0),
            "Oct 2 · 2 turns · 7 tools"
        );
        assert_eq!(
            meta_line("2026-10-02T09:00:00Z", 1, Some(1), false, 0),
            "Oct 2 · 1 turn · 1 tool"
        );
        // No tool figure at 0 or when a turn reported none.
        assert_eq!(
            meta_line("2026-10-02T09:00:00Z", 3, Some(0), false, 0),
            "Oct 2 · 3 turns"
        );
        assert_eq!(
            meta_line("2026-10-02T09:00:00Z", 3, None, false, 0),
            "Oct 2 · 3 turns"
        );
        // The viewer's day, not UTC's.
        assert_eq!(
            meta_line("2026-10-02T23:30:00Z", 1, None, false, 3600),
            "Oct 3 · 1 turn"
        );
        assert_eq!(meta_line("", 0, None, false, 0), "Saved conversation");
        assert_eq!(
            meta_line("2026-10-02T09:00:00Z", 1, None, true, 0),
            "Oct 2 · 1+ turns"
        );
    }

    #[test]
    fn titles_and_handover() {
        assert_eq!(card_title("fix the parser\nplease", "x"), "fix the parser");
        assert_eq!(
            card_title("", "acode-never-seen"),
            "Conversation acode-never-seen"
        );
        assert_eq!(
            card_title("", "acode-0123456789abcdef0123456789"),
            "Conversation …56789abcdef0123456789"
        );
        let ids: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(next_after_archive(&ids, "b").as_deref(), Some("c"));
        assert_eq!(next_after_archive(&ids, "c").as_deref(), Some("b"));
        assert_eq!(next_after_archive(&ids[..1], "a"), None);
    }
}
