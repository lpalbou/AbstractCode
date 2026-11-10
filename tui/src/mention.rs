//! `@entity` mentions (the 2026-11-08 contract,
//! `POST /api/gateway/sessions/{session_id}/mentions`): the roster the
//! composer's `@` popup offers, what a submitted message means, the
//! gateway's refusals, and the `author` every turn surface carries.
//!
//! A mention is a STRUCTURED chip: the person picks an entity from the
//! popup (abstracttui `Completion::mentions`) and the composer hands back
//! `ComposerSubmission { text, mentions }`. Nothing here reads `@` out of
//! text: a typed `@castor` that was never picked is plain text, sent like
//! any other prompt.
//!
//! Routing ([`route`]): a message that is ONLY one chip opens (or focuses)
//! that entity's conversation, as the leading `@name` always did. Any
//! other message with chips is the person's turn, sent as before, then one
//! mention call per chip, in order (the runner's `Cmd::Mentions`).
//!
//! Who the message is for (operator 2026-11-10): the popup also offers the
//! conversation's own agent ([`agent_candidate`], sent as the reserved slug
//! [`AGENT_SLUG`] in the start's `mentions`). A message whose chips name
//! entities and not the agent does not reach the agent: the gateway records
//! the person's turn and starts no agent run (`agent: "not_addressed"` on
//! the start's answer); the entities answer as before. With the agent chip,
//! or with no chip, the agent answers as before.

use abstracttui::widgets::{
    AuthorAvatar, AuthorKind, ChatAuthor, ComposerSubmission, MentionCandidate, MentionKind,
};
use serde_json::{json, Value};

use crate::entities::EntityInfo;

/// The one sentence for a gateway without the mention route.
pub const NOT_OFFERED: &str = "This gateway does not offer mentions yet";

/// The reserved slug that names the conversation's own agent in the
/// start's `mentions` (no entity can be called `agent`: the gateway
/// refuses that slug).
pub const AGENT_SLUG: &str = "agent";

/// The popup's row for the conversation's agent: the workflow's display
/// name (`label`, e.g. "Basic agent"; "Agent" when it has none), kind
/// agent, id [`AGENT_SLUG`].
pub fn agent_candidate(label: &str) -> MentionCandidate {
    let name = match label.trim() {
        "" => "Agent",
        named => named,
    };
    MentionCandidate::new(AGENT_SLUG, name, MentionKind::Agent)
}

/// The `@` popup's rows: the conversation's agent first
/// ([`agent_candidate`], named `agent_label`), then every readable home on
/// the cached roster (`GET /entities`, the entities this account may see),
/// in roster order. `id` is the slug (what the mention call names), `name`
/// the display name (what the chip shows), `hint` the state word and
/// pending tasks. Broken-home rows are never offered.
pub fn candidates(roster: &[EntityInfo], agent_label: &str) -> Vec<MentionCandidate> {
    let entities = roster
        .iter()
        .filter(|e| e.error.is_empty() && !e.slug.is_empty())
        .map(|e| {
            let name = if e.name.trim().is_empty() {
                e.slug.clone()
            } else {
                e.name.clone()
            };
            let mut hint = e.state.clone();
            if let Some(n) = e.pending_tasks.filter(|n| *n > 0) {
                if !hint.is_empty() {
                    hint.push_str(" · ");
                }
                hint.push_str(&format!("{n} task(s) pending"));
            }
            let c = MentionCandidate::new(e.slug.clone(), name, MentionKind::Entity);
            if hint.is_empty() {
                c
            } else {
                c.hint(hint)
            }
        });
    std::iter::once(agent_candidate(agent_label))
        .chain(entities)
        .collect()
}

/// One mention call: the entity (its slug), the name the chip showed, and
/// its `mention_index` (the chip's position among the message's chips).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionCall {
    pub entity: String,
    pub name: String,
    pub index: usize,
}

/// What a submitted message means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// No chip: the message is sent exactly as before.
    Plain,
    /// The message is ONLY one chip: open (or focus) that conversation.
    Open { slug: String },
    /// The person's turn, then these mention calls, in order.
    Mentions(Vec<MentionCall>),
}

/// Every entity chip of a submission, in text order (an entity picked
/// twice is listed twice): the chips its raw draft holds, one marker each.
pub fn chips(value: &ComposerSubmission) -> Vec<MentionCall> {
    value
        .mentions
        .iter()
        .filter(|m| m.kind == MentionKind::Entity)
        .map(|m| MentionCall {
            entity: m.id.clone(),
            name: m.name.clone(),
            index: m.index,
        })
        .collect()
}

/// Every chip of a submission in text order, the agent's included (as
/// [`AGENT_SLUG`]): one per marker of its raw draft, what
/// [`restore_draft`] walks and what [`start_mentions`] orders.
pub fn draft_chips(value: &ComposerSubmission) -> Vec<MentionCall> {
    value
        .mentions
        .iter()
        .map(|m| MentionCall {
            entity: match m.kind {
                MentionKind::Entity => m.id.clone(),
                MentionKind::Agent => AGENT_SLUG.to_string(),
            },
            name: m.name.clone(),
            index: m.index,
        })
        .collect()
}

/// The start's `mentions` name an entity (the agent's own slug aside): the
/// entities answer for themselves, and an agent asked alongside them may
/// say nothing.
pub fn names_an_entity(mentions: &[String]) -> bool {
    mentions.iter().any(|m| m != AGENT_SLUG)
}

/// Route a submission by its chips (never by its text). An entity picked
/// twice is asked once: its first chip keeps its index.
pub fn route(value: &ComposerSubmission) -> Route {
    let mut calls: Vec<MentionCall> = Vec::new();
    for call in chips(value) {
        if !calls.iter().any(|c| c.entity == call.entity) {
            calls.push(call);
        }
    }
    match calls.as_slice() {
        [] => Route::Plain,
        [only] if value.text.trim() == format!("@{}", only.name) => Route::Open {
            slug: only.entity.clone(),
        },
        _ => Route::Mentions(calls),
    }
}

/// `mentions` of `POST /runs/start` (AbstractGateway c26d38a2): the
/// chips the message holds ([`draft_chips`]), by the slug the mention call
/// names ([`AGENT_SLUG`] for the agent's), in chip order (`index`), each
/// once. Entities without the agent: the gateway starts no agent run
/// (`agent: "not_addressed"`). The agent with entities: the agent is told
/// the entities answer for themselves, and answers only the part addressed
/// to it, or says nothing (an empty reply, which the gateway serves as no
/// answer). Empty when the message holds no chip: the start then carries
/// no `mentions` key.
pub fn start_mentions(calls: &[MentionCall]) -> Vec<String> {
    let mut ordered: Vec<&MentionCall> = calls.iter().collect();
    ordered.sort_by_key(|c| c.index);
    let mut out: Vec<String> = Vec::new();
    for c in ordered {
        if !out.contains(&c.entity) {
            out.push(c.entity.clone());
        }
    }
    out
}

/// First code point of the composer's chip markers (AbstractTUI
/// `widgets/textarea_chips.rs`: one Supplementary Private Use Area-A scalar
/// per chip, U+F0000 + registry slot, in text order).
const CHIP_FIRST: u32 = 0xF_0000;
/// Last chip marker code point (U+FFFFE/U+FFFFF are noncharacters).
const CHIP_LAST: u32 = 0xF_FFFD;

/// The draft a stopped chain puts back into the composer: `raw` (the
/// buffer the message was sent from, one marker per chip, `chips` in the
/// same order, [`draft_chips`]) with the chips of the entities in `keep`
/// and the agent's chip left as chips (a resend asks only entities, and
/// the agent's chip keeps the line above the box honest) and every other
/// chip spelled as its plain name (no `@`: it asks no one, the words still
/// read). The marker scalars are AbstractTUI's: the `mentions_ui` test
/// round-trips this through the real composer.
pub fn restore_draft(raw: &str, chips: &[MentionCall], keep: &[MentionCall]) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut ordinal = 0usize;
    for ch in raw.chars() {
        if !(CHIP_FIRST..=CHIP_LAST).contains(&(ch as u32)) {
            out.push(ch);
            continue;
        }
        match chips.get(ordinal) {
            Some(chip)
                if chip.entity != AGENT_SLUG && !keep.iter().any(|k| k.entity == chip.entity) =>
            {
                out.push_str(&chip.name)
            }
            _ => out.push(ch),
        }
        ordinal += 1;
    }
    out
}

/// The message part of its calls' `mention_id` (AbstractGateway f0f23bb0:
/// a resend with the same id gets the first answer back, the entity is
/// never asked twice): the person's own turn (`repeat_of`) when the words
/// started one, else an id minted now. Taken ONCE per message and held with
/// it ([`crate::gateway::mentions::Unsent`]), so a resend names the same.
pub fn message_key(repeat_of: Option<&str>) -> String {
    match repeat_of.map(str::trim).filter(|r| !r.is_empty()) {
        Some(run_id) => run_id.to_string(),
        None => mint_message_id(),
    }
}

/// A fresh message id, `msg-<12 hex>-<n>`: the session-id mint (time, pid)
/// plus a process counter, so two messages never share one, even within
/// one clock tick.
pub fn mint_message_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!(
        "msg-{}-{n}",
        crate::config::mint_session_id().trim_start_matches("acode-")
    )
}

/// One call's `mention_id`: `<message>:<entity>:<mention_index>` — the same
/// on every resend of that mention, another for a new message.
pub fn mention_id(message: &str, call: &MentionCall) -> String {
    format!("{}:{}:{}", message.trim(), call.entity, call.index)
}

/// The mention call's body: the message as typed, chips spelled `@name`,
/// `repeat_of` — the run id of the person's own turn that carries the
/// same words (the agent turn this message started), so every client shows
/// those words once; absent when the message started no turn (a steer, a
/// refused start) — and its `mention_id` from the message's key
/// ([`message_key`]).
pub fn request_body(
    text: &str,
    call: &MentionCall,
    repeat_of: Option<&str>,
    message: &str,
) -> Value {
    let mut body = json!({"entity": call.entity, "text": text, "mention_index": call.index,
        "mention_id": mention_id(message, call)});
    if let Some(run_id) = repeat_of.filter(|r| !r.is_empty()) {
        body["repeat_of"] = json!(run_id);
    }
    body
}

/// Restored turns in reading order: the gateway's order (when each
/// question was asked), except that a turn repeating another turn's words
/// (`repeat_of`, an `@entity` mention) is read right after the turn it
/// repeats, the answers to the same words in the order they came
/// (`updated_at`). A repeat whose turn is not among these keeps its place.
/// `key` gives `(run_id, repeat_of, updated_at)` for a turn.
pub fn order_restored<T>(
    turns: Vec<T>,
    key: impl Fn(&T) -> (String, Option<String>, String),
) -> Vec<T> {
    let keys: Vec<(String, Option<String>, String)> = turns.iter().map(&key).collect();
    let is_root = |run_id: &str| {
        keys.iter()
            .any(|(id, rep, _)| id == run_id && rep.as_deref().is_none_or(|r| r == id))
    };
    // A repeat is placed under its turn only when that turn is here and is
    // itself no repeat (one level: an answer never repeats an answer).
    let parent = |i: usize| -> Option<&str> {
        let (id, rep, _) = &keys[i];
        rep.as_deref().filter(|r| *r != id && is_root(r))
    };
    let mut order: Vec<usize> = Vec::with_capacity(keys.len());
    for i in 0..keys.len() {
        if parent(i).is_some() {
            continue;
        }
        order.push(i);
        let mut answers: Vec<usize> = (0..keys.len())
            .filter(|j| parent(*j) == Some(keys[i].0.as_str()))
            .collect();
        answers.sort_by(|a, b| keys[*a].2.cmp(&keys[*b].2));
        order.extend(answers);
    }
    let mut slots: Vec<Option<T>> = turns.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

/// A mention call that did not answer 200: the HTTP status (None = no
/// answer at all), the error `code` (body `code` or the
/// `X-Gateway-Error-Code` header) and the gateway's `detail` sentence.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MentionFailure {
    pub status: Option<u16>,
    pub code: String,
    pub detail: String,
}

/// What the person reads for a failed mention, and what happens next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// 409 `in_another_conversation`: "<name> is in another conversation",
    /// then the next mention.
    Elsewhere,
    /// Any other refusal the gateway explains (the visit door's own 409s,
    /// a `repeat_of` the gateway does not know): its sentence, then the
    /// next mention.
    Refused(String),
    /// 404 `entity_not_found` (a missing entity, or one this account may
    /// not see — the same answer): "<name> cannot be mentioned here.",
    /// then the next mention.
    NotFound,
    /// 503 `entity_cannot_answer`: the gateway's sentence; the chain stops,
    /// and the message goes back into the composer with only the chips not
    /// answered (a resend asks only those).
    CannotAnswer(String),
    /// The route does not exist on this gateway: one sentence, mentions
    /// off for the rest of the session, the remaining calls dropped.
    NotOffered,
    /// No answer, or an unexplained error: an error row; the chain stops
    /// and the message goes back, as for [`Refusal::CannotAnswer`].
    Failed(String),
}

/// Map a failed mention onto what the person sees. Read from the error
/// code, never the sentence: the route's own 404 carries
/// `entity_not_found`; a 404 WITHOUT a code is the route missing.
pub fn classify(f: &MentionFailure) -> Refusal {
    match (f.status, f.code.as_str()) {
        (Some(409), "in_another_conversation") => Refusal::Elsewhere,
        (Some(503), "entity_cannot_answer") => Refusal::CannotAnswer(f.detail.clone()),
        (Some(404), "entity_not_found") => Refusal::NotFound,
        // Every refusal of the door is coded: an uncoded 404 or 405 is the
        // route itself missing (an older gateway).
        (Some(404 | 405), "") => Refusal::NotOffered,
        (Some(404 | 409 | 422), _) if !f.detail.is_empty() => Refusal::Refused(f.detail.clone()),
        (Some(code), _) => Refusal::Failed(if f.detail.is_empty() {
            format!("gateway HTTP {code}")
        } else {
            format!("gateway HTTP {code}: {}", f.detail)
        }),
        (None, _) => Refusal::Failed(f.detail.clone()),
    }
}

/// The workflow id of a mention's session turn: a completed run with no
/// ledger, whose reply is its `output.answer`.
pub const MENTION_WORKFLOW_ID: &str = "entity-mention";

/// A mention turn's reply from its run snapshot (`GET /runs/{id}`:
/// `output.answer` + `author`); None when the snapshot holds no answer.
pub fn reply_from_run(v: &Value) -> Option<(String, Option<ChatAuthor>)> {
    let answer = v
        .get("output")
        .and_then(|o| o.get("answer"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|a| !a.is_empty())?;
    Some((answer.to_string(), author_from_value(v.get("author"))))
}

/// The 200 answer: the reply text and who wrote it.
pub fn reply_from_response(v: &Value) -> (String, Option<ChatAuthor>) {
    let reply = v
        .get("reply")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    (reply, author_from_value(v.get("author")))
}

/// The contract's `author` object (`{kind, name, entity_id, identity_ref,
/// avatar: {style, color} | null}`), or None when absent, null or of an
/// unknown kind (an older gateway: the row renders exactly as before).
pub fn author_from_value(v: Option<&Value>) -> Option<ChatAuthor> {
    let v = v?.as_object()?;
    let kind = match v.get("kind").and_then(Value::as_str)?.trim() {
        "entity" => AuthorKind::Entity,
        "agent" => AuthorKind::Agent,
        "person" => AuthorKind::Person,
        _ => return None,
    };
    let text = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let mut author = ChatAuthor::new(kind, text("name").unwrap_or_default());
    author.entity_id = text("entity_id");
    author.identity_ref = text("identity_ref");
    author.avatar = v.get("avatar").and_then(Value::as_object).map(|a| {
        let field = |key: &str| {
            a.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        AuthorAvatar {
            style: field("style"),
            color: field("color"),
        }
    });
    Some(author)
}

#[cfg(test)]
mod tests {
    use super::*;
    use abstracttui::widgets::Mention;

    fn roster() -> Vec<EntityInfo> {
        let mk = |slug: &str, name: &str, state: &str, tasks: Option<u64>| EntityInfo {
            slug: slug.into(),
            name: name.into(),
            state: state.into(),
            pending_tasks: tasks,
            ..Default::default()
        };
        vec![
            mk("castor", "Castor", "asleep", None),
            mk("doorcheck", "", "awake", Some(2)),
            EntityInfo {
                slug: "lost-home".into(),
                error: "home unreadable".into(),
                ..Default::default()
            },
        ]
    }

    fn chip(id: &str, name: &str, index: usize) -> Mention {
        Mention {
            id: id.into(),
            name: name.into(),
            kind: MentionKind::Entity,
            index,
        }
    }

    #[test]
    fn candidates_offer_readable_homes_with_their_state() {
        let all = candidates(&roster(), "Basic agent");
        let c = &all[1..];
        assert_eq!(c.len(), 2, "broken homes are never offered");
        assert_eq!((c[0].id.as_str(), c[0].name.as_str()), ("castor", "Castor"));
        assert_eq!(c[0].hint.as_deref(), Some("asleep"));
        assert_eq!(c[0].kind, MentionKind::Entity);
        assert_eq!(c[1].name, "doorcheck", "a nameless row shows its slug");
        assert_eq!(c[1].hint.as_deref(), Some("awake · 2 task(s) pending"));
        assert_eq!(candidates(&[], "Basic agent").len(), 1, "the agent alone");
    }

    /// Mentions routing (operator 2026-11-10): the popup's first row is the
    /// conversation's agent, named as its workflow ("Agent" without a
    /// name), kind agent, sent as the reserved slug `agent`.
    #[test]
    fn the_popup_offers_the_agent_first_named_as_its_workflow() {
        let c = candidates(&roster(), "Basic agent");
        assert_eq!(
            (c[0].id.as_str(), c[0].name.as_str(), c[0].kind),
            (AGENT_SLUG, "Basic agent", MentionKind::Agent)
        );
        assert_eq!(AGENT_SLUG, "agent");
        assert_eq!(agent_candidate("  ").name, "Agent");
        assert_eq!(
            candidates(&roster(), "")
                .iter()
                .filter(|c| c.kind == MentionKind::Agent)
                .count(),
            1
        );
    }

    fn agent_chip(index: usize) -> Mention {
        Mention {
            id: AGENT_SLUG.into(),
            name: "Basic agent".into(),
            kind: MentionKind::Agent,
            index,
        }
    }

    /// The start's `mentions` hold every chip by slug in pick order, the
    /// agent's as `agent`, each once; the mention calls ask only the
    /// entities; only the agent's chip is no mention call and opens nothing.
    #[test]
    fn the_agent_chip_rides_the_start_in_pick_order_and_is_never_asked() {
        let both = ComposerSubmission {
            text: "@Castor and @Basic agent, does it hold?".into(),
            mentions: vec![chip("castor", "Castor", 0), agent_chip(1)],
        };
        assert_eq!(start_mentions(&draft_chips(&both)), vec!["castor", "agent"]);
        let first = ComposerSubmission {
            text: "@Basic agent and @Castor".into(),
            mentions: vec![agent_chip(0), chip("castor", "Castor", 1)],
        };
        assert_eq!(
            start_mentions(&draft_chips(&first)),
            vec!["agent", "castor"]
        );
        let Route::Mentions(calls) = route(&both) else {
            panic!("chips with words are mentions")
        };
        assert_eq!(calls.len(), 1, "the agent is no mention call: {calls:?}");
        assert_eq!(calls[0].entity, "castor");
        let only = ComposerSubmission {
            text: "@Basic agent ".into(),
            mentions: vec![agent_chip(0)],
        };
        assert_eq!(route(&only), Route::Plain, "the agent's chip opens nothing");
        assert_eq!(start_mentions(&draft_chips(&only)), vec!["agent"]);
        assert!(names_an_entity(&start_mentions(&draft_chips(&both))));
        assert!(!names_an_entity(&["agent".to_string()]));
        assert!(!names_an_entity(&[]));
    }

    /// A stopped chain's draft keeps the agent's chip a chip (its marker
    /// is walked in order, so the entities after it stay aligned).
    #[test]
    fn a_restored_draft_keeps_the_agent_chip_and_its_order() {
        let m = |k: u32| char::from_u32(CHIP_FIRST + k).unwrap();
        let raw = format!("{} {} and {} hold?", m(0), m(1), m(2));
        let value = ComposerSubmission {
            text: String::new(),
            mentions: vec![
                chip("castor", "Castor", 0),
                agent_chip(1),
                chip("pollux", "Pollux", 2),
            ],
        };
        let chips = draft_chips(&value);
        assert_eq!(
            restore_draft(&raw, &chips, &chips[2..]),
            format!("Castor {} and {} hold?", m(1), m(2)),
            "Castor answered: text; the agent and Pollux stay chips"
        );
    }

    /// AbstractGateway c26d38a2: the run start names the message's
    /// entities in chip order, each once; none when it names nobody.
    #[test]
    fn the_start_names_the_mentioned_entities_in_order_each_once() {
        let call = |entity: &str, index: usize| MentionCall {
            entity: entity.into(),
            name: entity.into(),
            index,
        };
        let calls = vec![
            call("vega", 2),
            call("pollux", 1),
            call("castor", 0),
            call("pollux", 3),
        ];
        assert_eq!(start_mentions(&calls), vec!["castor", "pollux", "vega"]);
        let two = ComposerSubmission {
            text: "@Castor and @Pollux, does the plan hold?".into(),
            mentions: vec![chip("castor", "Castor", 0), chip("pollux", "Pollux", 1)],
        };
        let Route::Mentions(routed) = route(&two) else {
            panic!("chips with words are mentions")
        };
        assert_eq!(start_mentions(&routed), vec!["castor", "pollux"]);
        assert!(start_mentions(&[]).is_empty());
    }

    #[test]
    fn a_message_without_chips_is_plain_even_with_a_typed_at() {
        let typed = ComposerSubmission {
            text: "@castor hello".into(),
            mentions: vec![],
        };
        assert_eq!(route(&typed), Route::Plain);
    }

    #[test]
    fn a_message_that_is_only_one_chip_opens_the_conversation() {
        let only = ComposerSubmission {
            text: "@Castor ".into(),
            mentions: vec![chip("castor", "Castor", 0)],
        };
        assert_eq!(
            route(&only),
            Route::Open {
                slug: "castor".into()
            }
        );
    }

    #[test]
    fn chips_with_words_are_mention_calls_in_order() {
        let two = ComposerSubmission {
            text: "@Castor and @Pollux, does the plan hold?".into(),
            mentions: vec![chip("castor", "Castor", 0), chip("pollux", "Pollux", 1)],
        };
        assert_eq!(
            route(&two),
            Route::Mentions(vec![
                MentionCall {
                    entity: "castor".into(),
                    name: "Castor".into(),
                    index: 0
                },
                MentionCall {
                    entity: "pollux".into(),
                    name: "Pollux".into(),
                    index: 1
                },
            ])
        );
        // Two chips and nothing else: still a message to both, not an open.
        let bare = ComposerSubmission {
            text: "@Castor @Pollux".into(),
            mentions: vec![chip("castor", "Castor", 0), chip("pollux", "Pollux", 1)],
        };
        assert!(matches!(route(&bare), Route::Mentions(c) if c.len() == 2));
    }

    #[test]
    fn an_entity_picked_twice_is_asked_once_at_its_first_index() {
        let twice = ComposerSubmission {
            text: "@Castor and again @Castor, @Pollux?".into(),
            mentions: vec![
                chip("castor", "Castor", 0),
                chip("castor", "Castor", 1),
                chip("pollux", "Pollux", 2),
            ],
        };
        assert_eq!(
            route(&twice),
            Route::Mentions(vec![
                MentionCall {
                    entity: "castor".into(),
                    name: "Castor".into(),
                    index: 0
                },
                MentionCall {
                    entity: "pollux".into(),
                    name: "Pollux".into(),
                    index: 2
                },
            ])
        );
        assert_eq!(chips(&twice).len(), 3, "the draft still holds three chips");
    }

    #[test]
    fn a_restored_draft_keeps_only_the_unsent_chips() {
        let m = |k: u32| char::from_u32(CHIP_FIRST + k).unwrap();
        let raw = format!("{} and {} does the plan hold?", m(0), m(1));
        let all = vec![
            MentionCall {
                entity: "castor".into(),
                name: "Castor".into(),
                index: 0,
            },
            MentionCall {
                entity: "pollux".into(),
                name: "Pollux".into(),
                index: 1,
            },
        ];
        assert_eq!(
            restore_draft(&raw, &all, &all[1..]),
            format!("Castor and {} does the plan hold?", m(1)),
            "Castor answered: plain text; Pollux stays a chip"
        );
        assert_eq!(
            restore_draft(&raw, &all, &all),
            raw,
            "nothing answered: unchanged"
        );
        assert_eq!(
            restore_draft(&raw, &all, &[]),
            "Castor and Pollux does the plan hold?"
        );
        // A chip picked twice follows its entity.
        let dup = format!("{} {} {}", m(0), m(1), m(0));
        let chips3 = vec![all[0].clone(), all[1].clone(), all[0].clone()];
        assert_eq!(
            restore_draft(&dup, &chips3, &all[..1]),
            format!("{} Pollux {}", m(0), m(0))
        );
    }

    #[test]
    fn the_body_carries_the_text_as_typed_and_the_index() {
        let call = MentionCall {
            entity: "pollux".into(),
            name: "Pollux".into(),
            index: 1,
        };
        assert_eq!(
            request_body("@Castor and @Pollux?", &call, None, "msg-1"),
            json!({"entity": "pollux", "text": "@Castor and @Pollux?", "mention_index": 1,
                "mention_id": "msg-1:pollux:1"}),
            "no turn carried the words: no repeat_of"
        );
        let body = request_body("@Castor and @Pollux?", &call, Some("run-7"), "run-7");
        assert_eq!(body["repeat_of"], "run-7");
        assert_eq!(body["mention_id"], "run-7:pollux:1");
    }

    /// One answer per mention (AbstractGateway f0f23bb0): the key's message
    /// is the turn the words started, else an id minted for the message —
    /// never empty, never shared by two messages.
    #[test]
    fn the_message_key_is_the_turn_else_a_fresh_id() {
        assert_eq!(message_key(Some("run-7")), "run-7");
        let (a, b) = (message_key(None), message_key(Some("  ")));
        assert!(a.starts_with("msg-") && b.starts_with("msg-"), "{a} {b}");
        assert_ne!(a, b, "two messages, two keys");
        let call = MentionCall {
            entity: "castor".into(),
            name: "Castor".into(),
            index: 0,
        };
        assert_ne!(mention_id("run-7", &call), mention_id("run-8", &call));
        assert_eq!(mention_id(&a, &call), format!("{a}:castor:0"));
        // Unique even within one clock tick.
        let many: std::collections::HashSet<String> =
            (0..20_000).map(|_| mint_message_id()).collect();
        assert_eq!(many.len(), 20_000);
    }

    #[test]
    fn restored_answers_follow_the_turn_they_answer_in_updated_at_order() {
        // (run_id, repeat_of, updated_at), in the gateway's order (when
        // each question was asked): the person's turn a1; Pollux's answer
        // to it (asked second, answered at :12); the person's NEXT turn
        // a2, asked while the answers were still coming; Castor's answer
        // to a1 (answered at :09); a repeat of a turn not restored here.
        let turns = vec![
            ("a1", None, "10:05"),
            ("m2", Some("a1"), "10:12"),
            ("a2", None, "10:20"),
            ("m1", Some("a1"), "10:09"),
            ("m3", Some("gone"), "10:30"),
        ];
        let ordered = order_restored(turns, |t| {
            (t.0.to_string(), t.1.map(str::to_string), t.2.to_string())
        });
        let ids: Vec<&str> = ordered.iter().map(|t| t.0).collect();
        assert_eq!(ids, ["a1", "m1", "m2", "a2", "m3"]);
        // Nothing to reorder: the gateway's order, untouched.
        let plain: Vec<(&str, Option<&str>, &str)> = vec![("a1", None, "2"), ("a2", None, "1")];
        let ids: Vec<&str> = order_restored(plain, |t| (t.0.into(), None, t.2.into()))
            .iter()
            .map(|t| t.0)
            .collect();
        assert_eq!(ids, ["a1", "a2"]);
    }

    #[test]
    fn refusals_map_to_what_the_person_reads() {
        let f = |status: Option<u16>, code: &str, detail: &str| MentionFailure {
            status,
            code: code.into(),
            detail: detail.into(),
        };
        assert_eq!(
            classify(&f(Some(409), "in_another_conversation", "Castor is in …")),
            Refusal::Elsewhere
        );
        assert_eq!(
            classify(&f(Some(409), "entity_paused", "Castor is paused.")),
            Refusal::Refused("Castor is paused.".into()),
            "the visit door's own 409s pass through with their sentence"
        );
        assert_eq!(
            classify(&f(
                Some(503),
                "entity_cannot_answer",
                "Castor cannot answer: no model"
            )),
            Refusal::CannotAnswer("Castor cannot answer: no model".into())
        );
        assert_eq!(
            classify(&f(Some(404), "", "Not Found")),
            Refusal::NotOffered,
            "a 404 without a code: the route is missing"
        );
        assert_eq!(
            classify(&f(Some(404), "", "entity 'ghost' not found")),
            Refusal::NotOffered,
            "the code decides, never the sentence"
        );
        assert_eq!(
            classify(&f(Some(405), "", "Method Not Allowed")),
            Refusal::NotOffered,
            "an uncoded 405 is the route missing too"
        );
        assert_eq!(
            classify(&f(
                Some(404),
                "entity_not_found",
                "entity 'ghost' not found under /x"
            )),
            Refusal::NotFound,
            "missing or hidden: cannot mention this one"
        );
        assert_eq!(
            classify(&f(
                Some(422),
                "repeat_of_unknown",
                "repeat_of 'r9' is not a turn of this conversation"
            )),
            Refusal::Refused("repeat_of 'r9' is not a turn of this conversation".into()),
            "a refused repeat_of is a notice row"
        );
        assert_eq!(
            classify(&f(Some(500), "", "boom")),
            Refusal::Failed("gateway HTTP 500: boom".into())
        );
        assert_eq!(
            classify(&f(None, "", "gateway unreachable: refused")),
            Refusal::Failed("gateway unreachable: refused".into())
        );
    }

    #[test]
    fn the_author_object_maps_field_for_field() {
        let v = json!({"kind": "entity", "name": "Castor", "entity_id": "entity:castor",
            "identity_ref": "entity:castor#3", "avatar": {"style": "watch-halo", "color": "#336699"}});
        let a = author_from_value(Some(&v)).expect("an entity author");
        assert_eq!(a.kind, AuthorKind::Entity);
        assert_eq!(a.name, "Castor");
        assert_eq!(a.entity_id.as_deref(), Some("entity:castor"));
        assert_eq!(a.identity_ref.as_deref(), Some("entity:castor#3"));
        assert_eq!(
            a.avatar,
            Some(AuthorAvatar {
                style: Some("watch-halo".into()),
                color: Some("#336699".into())
            })
        );
        let agent = json!({"kind": "agent", "name": "basic-agent", "entity_id": null,
            "identity_ref": null, "avatar": null});
        let a = author_from_value(Some(&agent)).unwrap();
        assert_eq!(
            (a.kind, a.name.as_str()),
            (AuthorKind::Agent, "basic-agent")
        );
        assert_eq!((a.entity_id, a.avatar), (None, None));
    }

    #[test]
    fn a_mention_turn_reply_comes_from_the_run_output() {
        let v = json!({"status": "completed", "workflow_id": "entity-mention",
            "output": {"answer": "It holds."},
            "author": {"kind": "entity", "name": "Castor"}});
        let (reply, author) = reply_from_run(&v).expect("the reply");
        assert_eq!(reply, "It holds.");
        assert_eq!(author.unwrap().name, "Castor");
        assert_eq!(reply_from_run(&json!({"output": null})), None);
        assert_eq!(reply_from_run(&json!({"output": {"answer": "  "}})), None);
    }

    #[test]
    fn no_author_or_an_unknown_kind_is_none() {
        assert_eq!(author_from_value(None), None);
        assert_eq!(author_from_value(Some(&Value::Null)), None);
        assert_eq!(
            author_from_value(Some(&json!({"kind": "robot", "name": "x"}))),
            None,
            "unknown kinds are refused, never guessed"
        );
        let (reply, author) = reply_from_response(&json!({"reply": " hi "}));
        assert_eq!((reply.as_str(), author), ("hi", None));
    }
}
