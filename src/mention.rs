//! `@name` mention parsing + the '@' completion provider rule.
//!
//! Routing is a SUBMIT-time parse (leading-@ only; a mid-prompt `@` is
//! plain text). The completion provider is deliberately DIFFERENT from the
//! '/' lane: it completes any @-token (leading = routing position,
//! mid-prompt = reference insert), reads only the CACHED roster (never a
//! synchronous fetch), and yields NO candidates for a query that already
//! exactly equals a roster slug — a fully-typed `@castor` submits (and
//! routes) on the first Enter instead of feeding the dropdown.

use crate::entities::EntityInfo;

/// Submit-time routing decision for a composer draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mention {
    /// No leading `@` — plain prompt text.
    None,
    /// Bare `@name`: open/focus the conversation WITHOUT sending.
    Open { slug: String },
    /// `@name <text>`: route `<text>` into the conversation.
    Message { slug: String, text: String },
    /// Leading `@` with a name not on the cached roster: honest notice,
    /// draft preserved (never silently swallowed as a prompt).
    Unknown { name: String },
}

/// Parse a submitted draft against the cached roster (case-insensitive
/// slug match). Only a LEADING `@` routes.
pub fn parse(text: &str, roster: &[EntityInfo]) -> Mention {
    let t = text.trim_start();
    let Some(rest) = t.strip_prefix('@') else {
        return Mention::None;
    };
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("").trim();
    if name.is_empty() {
        // A bare "@" is plain text (someone typing an email fragment).
        return Mention::None;
    }
    let body = parts.next().unwrap_or("").trim().to_string();
    let lower = name.to_lowercase();
    match roster
        .iter()
        .find(|e| e.error.is_empty() && e.slug == lower)
    {
        Some(e) => {
            if body.is_empty() {
                Mention::Open {
                    slug: e.slug.clone(),
                }
            } else {
                Mention::Message {
                    slug: e.slug.clone(),
                    text: body,
                }
            }
        }
        None => Mention::Unknown {
            name: name.to_string(),
        },
    }
}

/// One completion candidate: (label, insert, detail).
pub type Candidate = (String, String, String);

/// The '@' provider rule (engine `anchored_completion` mechanics: a token
/// triggers when its FIRST cluster is '@' and the token is whitespace-
/// delimited, so `castor@10.0.0.215` never triggers):
/// 1. NO whole-draft guard — leading and mid-prompt @-tokens both
///    complete; both insert `"@{slug} "`.
/// 2. A query that already EXACTLY equals a roster slug (case-insensitive)
///    yields NO candidates, closing the dropdown so Enter submits.
/// 3. Cached roster only; empty cache = no dropdown.
pub fn candidates(query: &str, roster: &[EntityInfo]) -> Vec<Candidate> {
    let q = query.trim().to_lowercase();
    if roster.iter().any(|e| e.error.is_empty() && e.slug == q) {
        return Vec::new(); // rule 2: fully-typed name submits on Enter
    }
    roster
        .iter()
        .filter(|e| e.error.is_empty() && e.slug.starts_with(&q))
        .map(|e| {
            let mut detail = e.state.clone();
            if let Some(n) = e.pending_tasks {
                if n > 0 {
                    detail.push_str(&format!(" · {n} task(s) pending"));
                }
            }
            (e.slug.clone(), format!("@{} ", e.slug), detail)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Vec<EntityInfo> {
        let mk = |slug: &str, state: &str, tasks: Option<u64>| EntityInfo {
            slug: slug.into(),
            name: slug.into(),
            state: state.into(),
            pending_tasks: tasks,
            ..Default::default()
        };
        let mut r = vec![
            mk("castor", "asleep", None),
            mk("doorcheck", "asleep", Some(2)),
        ];
        r.push(EntityInfo {
            slug: "lost-home".into(),
            error: "home unreadable".into(),
            ..Default::default()
        });
        r
    }

    #[test]
    fn leading_mention_routes_and_bare_opens() {
        let r = roster();
        assert_eq!(
            parse("@castor", &r),
            Mention::Open {
                slug: "castor".into()
            }
        );
        assert_eq!(
            parse("@Castor  hello there", &r),
            Mention::Message {
                slug: "castor".into(),
                text: "hello there".into()
            },
            "case-insensitive match routes to the canonical slug"
        );
        assert_eq!(
            parse("  @doorcheck hi", &r),
            Mention::Message {
                slug: "doorcheck".into(),
                text: "hi".into()
            },
            "leading whitespace does not defeat the parse"
        );
    }

    #[test]
    fn mid_prompt_and_bare_at_are_plain_text() {
        let r = roster();
        assert_eq!(parse("ask @castor about doors", &r), Mention::None);
        assert_eq!(parse("@", &r), Mention::None);
        assert_eq!(parse("mail me at x@y.z", &r), Mention::None);
    }

    #[test]
    fn adversarial_drafts_parse_predictably() {
        let r = roster();
        // Alt+Enter leaves a trailing newline on the draft.
        assert_eq!(
            parse("@doorcheck\n", &r),
            Mention::Open {
                slug: "doorcheck".into()
            },
            "trailing newline still opens"
        );
        // A newline as the first separator: the rest is the message.
        assert_eq!(
            parse("@doorcheck\nhello\nworld", &r),
            Mention::Message {
                slug: "doorcheck".into(),
                text: "hello\nworld".into()
            },
            "multiline bodies route whole"
        );
        // Non-breaking space is whitespace to the parse.
        assert_eq!(
            parse("@doorcheck\u{a0}hi", &r),
            Mention::Message {
                slug: "doorcheck".into(),
                text: "hi".into()
            }
        );
        // Tabs separate like spaces.
        assert_eq!(
            parse("@doorcheck\thi", &r),
            Mention::Message {
                slug: "doorcheck".into(),
                text: "hi".into()
            }
        );
        // Unicode names match case-insensitively.
        let mut r2 = r.clone();
        r2.push(EntityInfo {
            slug: "ünïcorn".into(),
            name: "ünïcorn".into(),
            state: "awake".into(),
            ..Default::default()
        });
        assert_eq!(
            parse("@Ünïcorn hi", &r2),
            Mention::Message {
                slug: "ünïcorn".into(),
                text: "hi".into()
            }
        );
        // Punctuation glued to the name is NOT a match — honest notice,
        // never a guessed route.
        assert_eq!(
            parse("@doorcheck, hi", &r),
            Mention::Unknown {
                name: "doorcheck,".into()
            }
        );
        // An @ inside the name token never matches a slug.
        assert_eq!(
            parse("@castor@10.0.0.215 hi", &r),
            Mention::Unknown {
                name: "castor@10.0.0.215".into()
            }
        );
    }

    #[test]
    fn unknown_name_is_reported_not_swallowed() {
        let r = roster();
        assert_eq!(
            parse("@ghost hello", &r),
            Mention::Unknown {
                name: "ghost".into()
            }
        );
        // Broken-home rows never route (their slug is not a live door).
        assert_eq!(
            parse("@lost-home hi", &r),
            Mention::Unknown {
                name: "lost-home".into()
            }
        );
    }

    #[test]
    fn provider_prefix_matches_and_details() {
        let r = roster();
        let cands = candidates("c", &r);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].0, "castor");
        assert_eq!(cands[0].1, "@castor ");
        assert_eq!(cands[0].2, "asleep");
        let with_tasks = candidates("door", &r);
        assert!(with_tasks[0].2.contains("2 task(s) pending"));
        // Case-insensitive prefix.
        assert_eq!(candidates("CAS", &r).len(), 1);
    }

    #[test]
    fn provider_exact_slug_yields_no_candidates() {
        let r = roster();
        assert!(
            candidates("castor", &r).is_empty(),
            "a fully-typed @castor submits on the first Enter"
        );
        assert!(candidates("CASTOR", &r).is_empty());
    }

    #[test]
    fn provider_empty_cache_and_broken_rows_stay_silent() {
        assert!(candidates("c", &[]).is_empty());
        let r = roster();
        assert!(
            candidates("lost", &r).is_empty(),
            "error rows never complete"
        );
    }
}
