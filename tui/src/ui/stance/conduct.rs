//! `conduct` — a faithful Rust port of AbstractUIC's
//! `ui-kit/src/cognition_conduct_core.ts` (the framework-free half of
//! `AfConductGauge`, slot (b) of the Cognitive Monitor pair).
//!
//! FOUR READS, each an axis, all grade-A mechanical and labeled:
//!
//! - **EFF effort** — think time + output volume, against the session's
//!   own running baseline (a RELATIVE read, never an absolute one).
//! - **ACT action** — tool rounds/calls this turn (+ failure ticks).
//! - **ATT attention** — memories recalled into context (+ formed).
//! - **RIG rigor** — the verification-SHAPED share of call NAMES plus
//!   retry-after-failure. Never "verified truth" — a name match is a
//!   vocabulary reading and is labeled as one.
//!
//! HONESTY RULES, carried over verbatim because they are the point:
//! an absent fact is an absent arc (never a zero-faked reading); no
//! baseline yet means value text without a filled arc; baselines are the
//! CONSUMER's own session history (this module never invents one).
//!
//! Kept a pure port — same field names, same thresholds, same wording —
//! so the two implementations can be diffed against each other. The one
//! deliberate difference is color: the kit's four hex hues become the
//! theme's audited categorical chart ramp, since a terminal app must
//! render on 26 palettes.

/// Mechanical facts for ONE turn. Every field optional: a missing fact
/// downgrades its axis to text-only with a printed reason.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Facts {
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    /// Client-measured wall time is acceptable; consumers label it.
    pub think_ms: Option<f64>,
    pub tool_rounds: Option<u32>,
    pub memories_recalled: Option<u32>,
    pub memories_formed: Option<u32>,
}

/// One tool call, as the reads see it: a name and whether it succeeded.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub name: String,
    /// `None` = still running / unknown; `Some(false)` is a failure.
    pub ok: Option<bool>,
}

/// Session-relative baselines — medians of the CONSUMER's own turn
/// history. All optional: a missing baseline downgrades that axis.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Baseline {
    pub think_ms: Option<f64>,
    pub tokens_out: Option<f64>,
    pub tool_rounds: Option<f64>,
    pub memories_recalled: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisId {
    Effort,
    Action,
    Attention,
    Rigor,
}

impl AxisId {
    pub fn code(self) -> &'static str {
        match self {
            AxisId::Effort => "EFF",
            AxisId::Action => "ACT",
            AxisId::Attention => "ATT",
            AxisId::Rigor => "RIG",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AxisId::Effort => "effort",
            AxisId::Action => "action",
            AxisId::Attention => "attention",
            AxisId::Rigor => "rigor",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Axis {
    pub id: AxisId,
    /// Fill in 0..=1, or `None` when unreadable (absent fact, or no
    /// baseline where one is required).
    pub value: Option<f32>,
    /// Human value text ("4.2s · 380tk", "3 rounds · 1 fail", "—").
    pub text: String,
    /// COMPACT value for an always-visible legend ("4.2s", "3·1✕",
    /// "12+2", "2/3", "—"). The kit's own finding: motion and alignment
    /// encodings need a static numeric channel beside them.
    pub short: String,
    /// Present when `value` is None — the reason, rendered not hidden.
    pub reason: Option<String>,
    /// Extra marks (failure ticks, retries).
    pub marks: u32,
}

/// Read/check-shaped call names — the deeds-lane vocabulary. Matched on
/// snake_case word boundaries, not as a prefix: `web_search`, the most
/// common lookup on live entities, missed the old `^search_` rule and
/// made a ten-search turn read as zero rigor.
const VERIFY_VERBS: [&str; 15] = [
    "read", "list", "search", "get", "fetch", "skim", "head", "stat", "check", "verify", "analyze",
    "open", "lookup", "query", "probe",
];

/// Whether one call NAME is verification-shaped. Exported so renderers
/// can mark individual calls with the SAME rule the share is computed
/// from (the kit's ringed stroke tips).
pub fn is_verify_shaped(name: &str) -> bool {
    name.split('_')
        .any(|seg| VERIFY_VERBS.contains(&seg.to_ascii_lowercase().as_str()))
}

/// The relative read: a value against twice its median sits at 1.0.
/// No usable median = no reading at all (never a fabricated 0.5).
fn rel(v: f64, med: Option<f64>) -> Option<f32> {
    match med {
        Some(m) if m > 0.0 => Some(((v / (2.0 * m)) as f32).clamp(0.0, 1.0)),
        _ => None,
    }
}

fn fmt_ms(ms: f64) -> String {
    if ms < 1000.0 {
        return format!("{}ms", ms.round() as i64);
    }
    let s = ms / 1000.0;
    if s < 60.0 {
        format!("{s:.1}s")
    } else {
        format!(
            "{}m{}s",
            (s / 60.0).floor() as i64,
            (s % 60.0).round() as i64
        )
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

/// The four axes for one turn. Order is stable: EFF, ACT, ATT, RIG.
pub fn axes(facts: &Facts, tools: &[ToolCall], baseline: &Baseline) -> Vec<Axis> {
    let mut out = Vec::with_capacity(4);

    // EFF — think time + output volume vs the session baseline.
    {
        let has = facts.think_ms.is_some() || facts.tokens_out.is_some();
        if !has {
            out.push(absent(
                AxisId::Effort,
                "—",
                "—",
                "no timing/volume fact this turn",
            ));
        } else {
            let mut parts: Vec<Option<f32>> = Vec::new();
            if let Some(ms) = facts.think_ms {
                parts.push(rel(ms, baseline.think_ms));
            }
            if let Some(tk) = facts.tokens_out {
                parts.push(rel(tk as f64, baseline.tokens_out));
            }
            let usable: Vec<f32> = parts.into_iter().flatten().collect();
            let text = [
                facts.think_ms.map(fmt_ms),
                facts.tokens_out.map(|tk| format!("{tk}tk")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            let short = match facts.think_ms {
                Some(ms) => fmt_ms(ms),
                None => format!("{}tk", facts.tokens_out.unwrap_or(0)),
            };
            out.push(if usable.is_empty() {
                Axis {
                    id: AxisId::Effort,
                    value: None,
                    text,
                    short,
                    reason: Some("first turns — no session baseline yet".into()),
                    marks: 0,
                }
            } else {
                Axis {
                    id: AxisId::Effort,
                    value: Some(usable.iter().sum::<f32>() / usable.len() as f32),
                    text,
                    short,
                    reason: None,
                    marks: 0,
                }
            });
        }
    }

    // ACT — tool rounds/calls (+ failure ticks).
    {
        let calls = tools.len() as u64;
        let fails = tools.iter().filter(|t| t.ok == Some(false)).count() as u32;
        if facts.tool_rounds.is_none() && calls == 0 {
            // Reachable only with NO tool fact at all: a present-but-zero
            // `tool_rounds` is a number, so the honest zero renders below.
            out.push(absent(AxisId::Action, "—", "—", "no tool facts this turn"));
        } else {
            let n = facts.tool_rounds.map(u64::from).unwrap_or(calls);
            let v = rel(n as f64, baseline.tool_rounds);
            let mut text = format!("{n} {}", plural(n, "round", "rounds"));
            if calls > 0 {
                text.push_str(&format!(" · {calls} {}", plural(calls, "call", "calls")));
            }
            if fails > 0 {
                text.push_str(&format!(" · {fails} fail"));
            }
            let short = if fails > 0 {
                format!("{n}·{fails}✕")
            } else {
                n.to_string()
            };
            out.push(Axis {
                id: AxisId::Action,
                value: v,
                text,
                short,
                reason: v
                    .is_none()
                    .then(|| "first turns — no session baseline yet".to_string()),
                marks: fails,
            });
        }
    }

    // ATT — memories recalled (+ formed).
    {
        match facts.memories_recalled {
            None => out.push(absent(
                AxisId::Attention,
                "—",
                "—",
                "no recall fact this turn",
            )),
            Some(recalled) => {
                let v = rel(recalled as f64, baseline.memories_recalled);
                let formed = facts.memories_formed.unwrap_or(0);
                let mut text = format!("{recalled} recalled");
                if formed > 0 {
                    text.push_str(&format!(" · +{formed} formed"));
                }
                let short = if formed > 0 {
                    format!("{recalled}+{formed}")
                } else {
                    recalled.to_string()
                };
                out.push(Axis {
                    id: AxisId::Attention,
                    value: v,
                    text,
                    short,
                    reason: v
                        .is_none()
                        .then(|| "first turns — no session baseline yet".to_string()),
                    marks: 0,
                });
            }
        }
    }

    // RIG — verification-shaped share of calls + retry-after-failure.
    {
        if tools.is_empty() {
            out.push(absent(
                AxisId::Rigor,
                "no calls to read",
                "—",
                "rigor reads call names — zero calls this turn",
            ));
        } else {
            let verify = tools.iter().filter(|t| is_verify_shaped(&t.name)).count();
            let retries = tools
                .windows(2)
                .filter(|w| w[0].ok == Some(false) && w[1].name == w[0].name)
                .count() as u32;
            let share = verify as f32 / tools.len() as f32;
            let mut text = format!("{verify}/{} verify-shaped", tools.len());
            if retries > 0 {
                text.push_str(&format!(
                    " · {retries} {}",
                    plural(retries as u64, "retry", "retries")
                ));
            }
            out.push(Axis {
                id: AxisId::Rigor,
                value: Some(share),
                text,
                short: format!("{verify}/{}", tools.len()),
                reason: None,
                marks: retries,
            });
        }
    }

    out
}

fn absent(id: AxisId, text: &str, short: &str, reason: &str) -> Axis {
    Axis {
        id,
        value: None,
        text: text.into(),
        short: short.into(),
        reason: Some(reason.into()),
        marks: 0,
    }
}

/// Running-median helper for building session baselines: the median of
/// the last `window` finite values, or `None` when there are none.
pub fn running_median(values: &[f64], window: usize) -> Option<f64> {
    let mut xs: Vec<f64> = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .rev()
        .take(window)
        .collect();
    if xs.is_empty() {
        return None;
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    Some(xs[xs.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, ok: Option<bool>) -> ToolCall {
        ToolCall {
            name: name.into(),
            ok,
        }
    }

    /// The vocabulary matches on snake_case word boundaries, anywhere in
    /// the name — the `web_search` lesson from the kit.
    #[test]
    fn verify_shaped_matches_on_word_boundaries() {
        for yes in [
            "read_file",
            "web_search",
            "list_files",
            "fetch_url",
            "probe_entity",
            "check",
        ] {
            assert!(is_verify_shaped(yes), "{yes} is verification-shaped");
        }
        for no in [
            "edit_file",
            "write_file",
            "execute_command",
            "readfile",
            "summon",
        ] {
            assert!(!is_verify_shaped(no), "{no} is not");
        }
    }

    /// An absent fact is an ABSENT reading with a printed reason — never
    /// a zero-faked arc. This is the rule the whole widget rests on.
    #[test]
    fn absent_facts_read_as_absent_not_zero() {
        let a = axes(&Facts::default(), &[], &Baseline::default());
        assert_eq!(a.len(), 4);
        for axis in &a {
            assert_eq!(axis.value, None, "{:?} must be unreadable", axis.id);
            assert!(axis.reason.is_some(), "{:?} must say why", axis.id);
            assert_eq!(axis.short, "—");
        }
        assert_eq!(a[3].text, "no calls to read");
    }

    /// A fact WITH no baseline keeps its text and loses only the fill —
    /// the "first turns" state, not an error.
    #[test]
    fn a_fact_without_a_baseline_keeps_its_text() {
        let facts = Facts {
            think_ms: Some(4200.0),
            tokens_out: Some(380),
            ..Facts::default()
        };
        let a = axes(&facts, &[], &Baseline::default());
        assert_eq!(a[0].value, None);
        assert_eq!(a[0].text, "4.2s · 380tk");
        assert_eq!(a[0].short, "4.2s");
        assert!(a[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("no session baseline"));
    }

    /// With a baseline the read is RELATIVE: at the median it sits at
    /// half, at twice the median it saturates.
    #[test]
    fn the_reading_is_relative_to_the_sessions_own_median() {
        let base = Baseline {
            think_ms: Some(4000.0),
            tokens_out: Some(400.0),
            ..Baseline::default()
        };
        let at_median = axes(
            &Facts {
                think_ms: Some(4000.0),
                tokens_out: Some(400),
                ..Facts::default()
            },
            &[],
            &base,
        );
        assert!((at_median[0].value.unwrap() - 0.5).abs() < 1e-6);
        let hard = axes(
            &Facts {
                think_ms: Some(40_000.0),
                tokens_out: Some(4000),
                ..Facts::default()
            },
            &[],
            &base,
        );
        assert_eq!(hard[0].value, Some(1.0), "saturates, never exceeds");
    }

    /// ACT counts, marks failures, and reads an honest ZERO as zero —
    /// distinct from "no tool facts at all".
    #[test]
    fn action_separates_an_honest_zero_from_no_facts() {
        let none = axes(&Facts::default(), &[], &Baseline::default());
        assert_eq!(none[1].reason.as_deref(), Some("no tool facts this turn"));
        let zero = axes(
            &Facts {
                tool_rounds: Some(0),
                ..Facts::default()
            },
            &[],
            &Baseline {
                tool_rounds: Some(3.0),
                ..Baseline::default()
            },
        );
        assert_eq!(zero[1].value, Some(0.0));
        assert_eq!(zero[1].text, "0 rounds");
        let busy = axes(
            &Facts {
                tool_rounds: Some(3),
                ..Facts::default()
            },
            &[
                call("read_file", Some(true)),
                call("edit_file", Some(false)),
            ],
            &Baseline {
                tool_rounds: Some(3.0),
                ..Baseline::default()
            },
        );
        assert_eq!(busy[1].text, "3 rounds · 2 calls · 1 fail");
        assert_eq!(busy[1].short, "3·1✕");
        assert_eq!(busy[1].marks, 1);
    }

    /// RIG is a share of NAMES plus retry-after-failure, and it says so.
    #[test]
    fn rigor_reads_names_and_counts_retries() {
        let tools = [
            call("read_file", Some(true)),
            call("edit_file", Some(false)),
            call("edit_file", Some(true)),
            call("web_search", Some(true)),
        ];
        let a = axes(&Facts::default(), &tools, &Baseline::default());
        assert_eq!(a[3].value, Some(0.5), "2 of 4 names are verify-shaped");
        assert_eq!(a[3].text, "2/4 verify-shaped · 1 retry");
        assert_eq!(a[3].short, "2/4");
        assert_eq!(a[3].marks, 1);
    }

    #[test]
    fn running_median_takes_the_last_window() {
        assert_eq!(running_median(&[], 12), None);
        assert_eq!(running_median(&[5.0], 12), Some(5.0));
        assert_eq!(running_median(&[1.0, 2.0, 3.0], 12), Some(2.0));
        // Only the last `window` values count.
        assert_eq!(running_median(&[100.0, 1.0, 1.0, 1.0], 3), Some(1.0));
        assert_eq!(running_median(&[f64::NAN, 4.0], 12), Some(4.0));
    }
}
