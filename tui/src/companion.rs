//! Client-owned companion choices and a pure, change-driven advice gate.
//!
//! No timer, transport, model, microphone or UI state lives here. The host feeds
//! observations and monotonic elapsed time, then owns each request's lifecycle.
use crate::{store::Phase, transcript::Item, voice::VoicePrefs};
use serde_json::{json, Value};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
};

pub const ADVICE_INTERVAL: Duration = Duration::from_secs(60);
pub const NO_ADVICE: &str = "NO_ADVICE";
const REVISION_BYTES: usize = 32_768;
const MAX_ADVICE_CHARS: usize = 800;

/// Local presentation choices. Future shared identity resolution stays outside
/// these client preferences; `voice` is an explicit per-client override.
#[derive(Clone, Debug, PartialEq)]
pub struct CompanionPrefs {
    pub enabled: bool,
    pub style: String,
    pub animate: bool,
    pub advice: bool,
    pub read_aloud: bool,
    pub voice: VoicePrefs,
}
impl Default for CompanionPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            style: "halo".into(),
            animate: true,
            advice: false,
            read_aloud: false,
            voice: VoicePrefs::default(),
        }
    }
}
impl CompanionPrefs {
    pub fn from_json(value: Option<&Value>) -> Self {
        let mut p = Self::default();
        let Some(v) = value.filter(|v| v.is_object()) else {
            return p;
        };
        let boolean = |name, default| v.get(name).and_then(Value::as_bool).unwrap_or(default);
        p.enabled = boolean("enabled", p.enabled);
        p.animate = boolean("animate", p.animate);
        p.advice = boolean("advice", p.advice);
        p.read_aloud = boolean("read_aloud", p.read_aloud);
        if let Some(style) = v
            .get("style")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            // Preserve an unknown future style; the renderer can choose its
            // supported representation without rewriting stored identity.
            p.style = style.to_owned();
        }
        p.voice = VoicePrefs::from_json(v.get("voice"));
        p
    }
    pub fn to_json(&self) -> Value {
        self.merge_into(None)
    }

    /// Replace fields owned by this version while retaining unknown settings,
    /// including unknown nested voice metadata from another client version.
    pub fn merge_into(&self, existing: Option<&Value>) -> Value {
        let mut out = existing
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (key, value) in [
            ("enabled", json!(self.enabled)),
            ("style", json!(self.style)),
            ("animate", json!(self.animate)),
            ("advice", json!(self.advice)),
            ("read_aloud", json!(self.read_aloud)),
        ] {
            out.insert(key.into(), value);
        }
        let mut voice = out
            .get("voice")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        // Empty/default values clear our old overrides instead of accidentally
        // retaining a previous provider/voice triple.
        for key in [
            "provider",
            "model",
            "voice",
            "voice_label",
            "profile",
            "speed",
            "quality_preset",
            "instructions",
            "stt_provider",
            "stt_model",
            "stt_language",
            "output_device",
            "input_device",
            "input_gain",
            "reply_volume",
            "read_aloud",
            "auto_send_voice",
            "space_to_talk",
        ] {
            voice.remove(key);
        }
        if let Value::Object(known) = self.voice.to_json() {
            voice.extend(known);
        }
        out.insert("voice".into(), Value::Object(voice));
        Value::Object(out)
    }
}

/// A presentation-relevant snapshot identity, independent of ledger heartbeats,
/// tool payload sizes, private reasoning, and the companion's own conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub session: String,
    pub run: String,
    pub digest: u64,
    pub has_messages: bool,
}

pub fn semantic_revision(session: &str, run: &str, items: &[Item], phase: Phase) -> Revision {
    let mut hash = DefaultHasher::new();
    session.hash(&mut hash);
    run.hash(&mut hash);
    match phase {
        Phase::Idle => 0u8,
        Phase::Starting => 1,
        Phase::Running => 2,
    }
    .hash(&mut hash);
    let mut budget = REVISION_BYTES;
    let mut has_messages = false;
    for item in items.iter().rev() {
        let (role, text) = match item {
            Item::User { text } => (0u8, text),
            Item::Steer { text } => (1, text),
            Item::Assistant { text, .. } => (2, text),
            Item::Error { text } => (3, text),
            _ => continue,
        };
        if text.trim().is_empty() {
            continue;
        }
        has_messages = true;
        if budget == 0 {
            break;
        }
        role.hash(&mut hash);
        text.len().hash(&mut hash);
        let mut start = text.len().saturating_sub(budget);
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text[start..].hash(&mut hash);
        budget = budget.saturating_sub(text.len() - start);
    }
    Revision {
        session: session.into(),
        run: run.into(),
        digest: hash.finish(),
        has_messages,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdviceTicket {
    pub id: u64,
    pub revision: Revision,
    generation: u64,
}

/// One gate per TUI, retained across session switches. `eligible` should mean
/// enabled + advice opted in + visible + not restoring. `live=false` consumes
/// replay as a baseline rather than treating old records as fresh activity.
#[derive(Debug, Default)]
pub struct AdviceGate {
    observed: Option<Revision>,
    pending: Option<Revision>,
    eligible: bool,
    generation: u64,
    sequence: u64,
    last_attempt: Option<Duration>,
    in_flight: Option<AdviceTicket>,
    last_suggestion: Option<(String, String)>,
}
impl AdviceGate {
    pub fn observe(&mut self, revision: Revision, eligible: bool, live: bool) {
        let same_session = self
            .observed
            .as_ref()
            .is_some_and(|r| r.session == revision.session);
        let changed = self.observed.as_ref() != Some(&revision);
        if !same_session || (self.eligible && !eligible) || !live {
            self.generation = self.generation.wrapping_add(1);
            self.pending = None;
        }
        if same_session && self.eligible && eligible && live && changed && revision.has_messages {
            self.pending = Some(revision.clone());
        }
        if !eligible || !live {
            self.pending = None;
        }
        self.observed = Some(revision);
        self.eligible = eligible;
    }

    /// Delay to the next eligible attempt. `None` means no timer is needed.
    /// A busy request will be reconsidered by its completion, not polling.
    pub fn next_due(&self, now: Duration) -> Option<Duration> {
        if !self.eligible || self.pending.is_none() || self.in_flight.is_some() {
            return None;
        }
        Some(
            self.last_attempt
                .map(|t| t.saturating_add(ADVICE_INTERVAL).saturating_sub(now))
                .unwrap_or(Duration::ZERO),
        )
    }
    pub fn try_begin(&mut self, now: Duration) -> Option<AdviceTicket> {
        if self.next_due(now)? != Duration::ZERO {
            return None;
        }
        let revision = self.pending.take()?;
        self.sequence = self.sequence.wrapping_add(1);
        let ticket = AdviceTicket {
            id: self.sequence,
            revision,
            generation: self.generation,
        };
        self.last_attempt = Some(now);
        self.in_flight = Some(ticket.clone());
        Some(ticket)
    }
    pub fn in_flight(&self) -> bool {
        self.in_flight.is_some()
    }
    pub fn dirty(&self) -> bool {
        self.pending.is_some()
    }

    /// Call for every finished request, even after disabling/switching sessions.
    /// Releasing stale work here keeps the one-request rule truthful. Failures
    /// pass `None`; they consume the attempt but do not automatically retry.
    pub fn finish(&mut self, ticket: AdviceTicket, suggestion: Option<&str>) -> Option<String> {
        if self.in_flight.as_ref().map(|t| t.id) != Some(ticket.id) {
            return None;
        }
        self.in_flight = None;
        if !self.eligible
            || ticket.generation != self.generation
            || self.observed.as_ref() != Some(&ticket.revision)
        {
            return None;
        }
        let text = suggestion?.trim();
        let normalized = normalized(text);
        if normalized.is_empty() || is_no_advice(text) {
            return None;
        }
        if self
            .last_suggestion
            .as_ref()
            .is_some_and(|(session, old)| session == &ticket.revision.session && old == &normalized)
        {
            return None;
        }
        self.last_suggestion = Some((ticket.revision.session, normalized));
        if text.chars().count() > MAX_ADVICE_CHARS {
            Some(format!(
                "{}\n[TRUNCATION: advice shortened]",
                text.chars().take(MAX_ADVICE_CHARS).collect::<String>()
            ))
        } else {
            Some(text.into())
        }
    }
}
fn normalized(text: &str) -> String {
    text.split_whitespace()
        .flat_map(str::chars)
        .take(MAX_ADVICE_CHARS)
        .flat_map(char::to_lowercase)
        .collect()
}
fn is_no_advice(text: &str) -> bool {
    // docs-qa asks for section citations without prescribing their syntax.
    // Accept a leading marker plus citation groups, never surrounding prose.
    let Some(line) = text.lines().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    let decoration = |c: char| c.is_whitespace() || matches!(c, '`' | '*' | '_' | '.' | '"' | ':');
    let line = line.trim_start_matches(decoration);
    let Some(prefix) = line.get(..NO_ADVICE.len()) else {
        return false;
    };
    if !prefix.eq_ignore_ascii_case(NO_ADVICE) {
        return false;
    }
    let mut tail = line[NO_ADVICE.len()..].trim_matches(decoration);
    while !tail.is_empty() {
        let open = tail.chars().next().unwrap();
        let close = match open {
            '(' => ')',
            '[' => ']',
            _ => return false,
        };
        let mut depth = 0usize;
        let mut end = None;
        for (at, c) in tail.char_indices() {
            if c == open {
                depth += 1;
            }
            if c == close {
                depth -= 1;
                if depth == 0 {
                    end = Some(at + c.len_utf8());
                    break;
                }
            }
        }
        let Some(end) = end else { return false };
        if tail[open.len_utf8()..end - close.len_utf8()]
            .trim()
            .is_empty()
        {
            return false;
        }
        tail = tail[end..].trim_matches(decoration);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(session: &str, digest: u64) -> Revision {
        Revision {
            session: session.into(),
            run: "run".into(),
            digest,
            has_messages: true,
        }
    }
    fn t(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }
    fn ready() -> AdviceGate {
        let mut g = AdviceGate::default();
        g.observe(r("one", 0), true, true);
        g
    }
    #[test]
    fn no_advice_accepts_source_decorations_but_not_real_comment_prose() {
        for marker in [
            "NO_ADVICE (Current conversation snapshot)",
            "**NO_ADVICE** [source]",
            "`NO_ADVICE` [1](https://example.invalid/reference).",
            "no_advice (Conversation (recent)) [2]",
            "NO_ADVICE\n\nSources: Current conversation snapshot",
        ] {
            assert!(is_no_advice(marker), "{marker}");
        }
        for comment in [
            "The run returned NO_ADVICE but should have answered.",
            "NO_ADVICE means no suggestion, but inspect the parser.",
            "NO_ADVICE (Current conversation snapshot) Check the parser.",
            "NO_ADVICES [source]",
            "NO_ADVICE [unfinished",
            "NO_ADVICE []",
        ] {
            assert!(!is_no_advice(comment), "{comment}");
        }
    }

    #[test]
    fn defaults_are_opt_in_and_round_trip_unknown_fields() {
        let original = json!({"enabled":true,"future":"keep","voice":{"provider":"piper","model":"m","voice":"v","future_voice":"keep"}});
        let mut p = CompanionPrefs::from_json(Some(&original));
        assert!(!p.advice && !p.read_aloud && p.animate);
        p.voice = VoicePrefs::default();
        let merged = p.merge_into(Some(&original));
        assert_eq!(merged["future"], "keep");
        assert_eq!(merged["voice"]["future_voice"], "keep");
        assert!(merged["voice"].get("provider").is_none());
        assert_eq!(CompanionPrefs::from_json(Some(&p.to_json())), p);
        assert!(!CompanionPrefs::default().enabled);
        assert!(
            !CompanionPrefs::from_json(Some(&json!({"enabled":"true","advice":"true"}))).enabled
        );
    }
    #[test]
    fn semantic_revision_ignores_tool_and_private_reasoning_changes() {
        let mut items = vec![
            Item::User {
                text: "question".into(),
            },
            Item::Assistant {
                text: "answer".into(),
                final_answer: true,
            },
        ];
        let a = semantic_revision("s", "r", &items, Phase::Running);
        items.push(Item::Thinking {
            iteration: 2,
            content: "cycle".into(),
            reasoning: "private".into(),
            call: Default::default(),
        });
        items.push(Item::Tool {
            key: "k".into(),
            name: "tool".into(),
            args_preview: "preview".into(),
            args_full: "secret args".into(),
            status: crate::transcript::ToolStatus::Ok,
            result: "large result".repeat(1000),
            error: String::new(),
            sandbox: None,
        });
        assert_eq!(a, semantic_revision("s", "r", &items, Phase::Running));
        items.push(Item::Error {
            text: "visible failure".into(),
        });
        assert_ne!(a, semantic_revision("s", "r", &items, Phase::Running));
        assert_ne!(a, semantic_revision("s", "r", &items, Phase::Idle));
    }
    #[test]
    fn unchanged_and_replayed_content_never_arms_a_timer() {
        let mut g = ready();
        assert_eq!(g.next_due(t(0)), None);
        g.observe(r("one", 0), true, true);
        assert_eq!(g.next_due(t(100)), None);
        g.observe(r("one", 1), true, false);
        assert_eq!(g.next_due(t(100)), None);
        g.observe(r("one", 1), true, true);
        assert_eq!(g.next_due(t(100)), None);
        g.observe(r("one", 2), true, true);
        assert_eq!(g.next_due(t(100)), Some(Duration::ZERO));
    }
    #[test]
    fn changes_coalesce_and_failures_do_not_retry_or_overlap() {
        let mut g = ready();
        g.observe(r("one", 1), true, true);
        let first = g.try_begin(t(10)).unwrap();
        let old_completion = first.clone();
        assert!(g.in_flight());
        g.observe(r("one", 2), true, true);
        g.observe(r("one", 3), true, true);
        assert_eq!(g.next_due(t(100)), None);
        assert!(g.try_begin(t(100)).is_none());
        assert_eq!(g.finish(first, None), None);
        assert_eq!(g.next_due(t(20)), Some(t(50)));
        let next = g.try_begin(t(70)).unwrap();
        assert_eq!(g.finish(old_completion, Some("duplicate completion")), None);
        assert!(
            g.in_flight(),
            "a duplicate old completion cannot release newer work"
        );
        assert_eq!(next.revision.digest, 3);
        assert_eq!(g.finish(next, None), None);
        assert_eq!(g.next_due(t(1000)), None);
    }
    #[test]
    fn cooldown_survives_disable_and_session_switch_and_stale_work_is_discarded() {
        let mut g = ready();
        g.observe(r("one", 1), true, true);
        let old = g.try_begin(t(0)).unwrap();
        g.observe(r("two", 0), true, true);
        g.observe(r("two", 1), true, true);
        assert_eq!(g.finish(old, Some("old session advice")), None);
        assert_eq!(g.next_due(t(5)), Some(t(55)));
        g.observe(r("two", 1), false, true);
        g.observe(r("two", 1), true, true);
        assert_eq!(g.next_due(t(100)), None);
        g.observe(r("two", 2), true, true);
        let ticket = g.try_begin(t(100)).unwrap();
        g.observe(r("two", 2), false, true);
        assert!(g.in_flight());
        assert_eq!(g.finish(ticket, Some("disabled advice")), None);
        assert!(!g.in_flight());
    }
    #[test]
    fn suggestions_are_current_deduplicated_bounded_and_no_advice_is_silent() {
        let mut g = ready();
        g.observe(r("one", 1), true, true);
        let ticket = g.try_begin(t(0)).unwrap();
        assert_eq!(
            g.finish(ticket, Some("  Check the parser.  ")).as_deref(),
            Some("Check the parser.")
        );
        g.observe(r("one", 2), true, true);
        let ticket = g.try_begin(t(60)).unwrap();
        assert_eq!(g.finish(ticket, Some("check  THE parser.")), None);
        g.observe(r("one", 3), true, true);
        let ticket = g.try_begin(t(120)).unwrap();
        assert_eq!(
            g.finish(ticket, Some("**NO_ADVICE**\n\nSources: conversation")),
            None
        );
        g.observe(r("one", 4), true, true);
        let ticket = g.try_begin(t(180)).unwrap();
        let huge = g.finish(ticket, Some(&"語".repeat(2000))).unwrap();
        assert!(huge.chars().count() < 850 && huge.contains("TRUNCATION"));
        g.observe(r("one", 5), true, true);
        let ticket = g.try_begin(t(240)).unwrap();
        g.observe(r("one", 6), true, true);
        assert_eq!(g.finish(ticket, Some("outdated revision")), None);
    }
}
