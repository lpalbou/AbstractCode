//! Entity collaboration: types + parsing for the gateway entity lane.
//!
//! Every parse here is pinned to the REAL response shapes — verified against
//! the gateway source (`entity_visits.py`, `routes/entities.py`) and the
//! live gateway on 2026-07-22 (fixtures under `tests/fixtures/entities/`):
//!
//! - `GET /entities` → `{"entities": [...]}` — entries carry `slug`,
//!   `name`, `handle`, `state{state, liveness, mode, reason}`, optional
//!   `pending_tasks` (ABSENT ≠ 0 — the roster is deliberately file-cheap),
//!   optional `drives` (warm homes only), or `{slug, error}` rows for
//!   unreadable/moved homes.
//! - `GET /entities/{name}/visit` → `{"open": false}` or `{open: true,
//!   run_id, session_id, visit_id, turn_n, status, workflow_arm}`.
//! - `POST .../visit/open` → `{run_id, visit_id, session_id, participants,
//!   prelude_warnings}`.
//! - `POST .../visit/{run_id}/turn` → `{run_id, reply, turn_n, status,
//!   turn_id, tools_ran, memories, diary_entries, notices, tool_details
//!   [{name, arg?, success?, result}], output?, error?}` — HTTP 200 can
//!   carry body `status:"failed"` (transport success ≠ operation success).
//! - `GET .../visit/{run_id}/transcript` → `{run_id, session_id, visit_id,
//!   status, turn_n, participants, turns: [{role, content,
//!   tool_details?}], warnings?}` — `_visit.history` is a SLIDING WINDOW
//!   (last ~10 turns), so `turn_n` can exceed the rendered turns.
//! - `GET .../cognition` → `{spend: {lifetime{tokens_total,…},
//!   live_visit: null|{tokens_total,…}}, state{state,…}, …}`.
//! - `GET /mcp/servers` → `{servers, source, probed, warnings}`.

use std::path::PathBuf;

use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Roster
// ---------------------------------------------------------------------------

/// One drive ratio pair: things opened vs things discharged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrivePair {
    pub open: u64,
    pub closed: u64,
}

/// Cognition drive ratios (questions/problems/interests). Present only for
/// WARM homes on the roster — absence ≠ zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Drives {
    pub questions: DrivePair,
    pub problems: DrivePair,
    pub interests: DrivePair,
}

impl Drives {
    /// Compact chip text ("q 2/6 · i 1/60") — open counts vs discharged.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        for (label, p) in [
            ("q", self.questions),
            ("p", self.problems),
            ("i", self.interests),
        ] {
            if p.open > 0 || p.closed > 0 {
                parts.push(format!("{label} {}/{}", p.closed, p.open + p.closed));
            }
        }
        parts.join(" · ")
    }
}

/// One roster entry. Error rows (unreadable/moved homes) carry `error` and
/// render as labeled broken homes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntityInfo {
    pub slug: String,
    pub name: String,
    pub handle: String,
    /// State word ("awake" | "asleep" | "paused" | …).
    pub state: String,
    pub liveness: String,
    pub mode: String,
    pub reason: String,
    /// ABSENT ≠ 0 (the roster is file-cheap; None = not reported).
    pub pending_tasks: Option<u64>,
    pub drives: Option<Drives>,
    /// Non-empty = a broken-home row; every other field may be blank.
    pub error: String,
}

fn drive_pair(v: Option<&Value>, closed_key: &str) -> DrivePair {
    let Some(v) = v else {
        return DrivePair::default();
    };
    DrivePair {
        open: v.get("open").and_then(Value::as_u64).unwrap_or(0),
        closed: v.get(closed_key).and_then(Value::as_u64).unwrap_or(0),
    }
}

/// Parse `GET /entities` (`{"entities": [...]}` — top-level key verified
/// live; NOT `items`).
pub fn entities_from_response(v: &Value) -> Vec<EntityInfo> {
    let mut out = Vec::new();
    for e in v
        .get("entities")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        let slug = e
            .get("slug")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let error = e
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if slug.is_empty() && error.is_empty() {
            continue;
        }
        let state = e.get("state");
        let sv = |k: &str| {
            state
                .and_then(|s| s.get(k))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let drives = e.get("drives").map(|d| Drives {
            questions: drive_pair(d.get("questions"), "resolved"),
            problems: drive_pair(d.get("problems"), "repaired"),
            interests: drive_pair(d.get("interests"), "explored"),
        });
        out.push(EntityInfo {
            name: e
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&slug)
                .trim()
                .to_string(),
            handle: e
                .get("handle")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            state: sv("state"),
            liveness: sv("liveness"),
            mode: sv("mode"),
            reason: sv("reason"),
            pending_tasks: e.get("pending_tasks").and_then(Value::as_u64),
            drives,
            error,
            slug,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Visit lane
// ---------------------------------------------------------------------------

/// `GET /entities/{name}/visit` — `{"open": false}` or the full view.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisitStatus {
    pub open: bool,
    pub run_id: String,
    pub session_id: String,
    pub visit_id: String,
    pub turn_n: u64,
    /// Run status word ("waiting" | "running" | terminal states).
    pub status: String,
}

pub fn visit_status_from_response(v: &Value) -> VisitStatus {
    VisitStatus {
        open: v.get("open").and_then(Value::as_bool).unwrap_or(false),
        run_id: str_field(v, "run_id"),
        session_id: str_field(v, "session_id"),
        visit_id: str_field(v, "visit_id"),
        turn_n: v.get("turn_n").and_then(Value::as_u64).unwrap_or(0),
        status: str_field(v, "status"),
    }
}

/// `POST /entities/{name}/visit/open` success body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisitOpen {
    pub run_id: String,
    pub visit_id: String,
    pub session_id: String,
    pub participants: Vec<String>,
    pub prelude_warnings: Vec<String>,
}

pub fn visit_open_from_response(v: &Value) -> VisitOpen {
    VisitOpen {
        run_id: str_field(v, "run_id"),
        visit_id: str_field(v, "visit_id"),
        session_id: str_field(v, "session_id"),
        participants: str_list(v.get("participants")),
        prelude_warnings: str_list(v.get("prelude_warnings")),
    }
}

/// One executed tool call served from the run's own ledger
/// (`{name, arg?, success?, result}` — results serve VERBATIM by the
/// 2026-07-09 transparency ruling; the TUI bounds them for DISPLAY only).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolDetail {
    pub name: String,
    pub arg: String,
    /// None = the ledger record carried no success flag.
    pub success: Option<bool>,
    pub result: String,
}

fn tool_details_from(v: Option<&Value>) -> Vec<ToolDetail> {
    let mut out = Vec::new();
    for d in v.and_then(Value::as_array).unwrap_or(&Vec::new()) {
        let name = str_field(d, "name");
        if name.is_empty() {
            continue;
        }
        out.push(ToolDetail {
            name,
            arg: str_field(d, "arg"),
            success: d.get("success").and_then(Value::as_bool),
            result: str_field(d, "result"),
        });
    }
    out
}

/// One memory handle the probe reports (what entered the entity's prompt).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryHandle {
    pub kind: String,
    pub title: String,
    pub digest: String,
    pub origin: String,
}

/// `POST .../turn` body. `status:"failed"` on HTTP 200 is a REAL outcome —
/// callers must read `status`/`error`, never transport success.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnResponse {
    pub run_id: String,
    pub reply: String,
    pub turn_n: u64,
    /// "waiting" (parked, normal) | "completed" (close raced) | "failed".
    pub status: String,
    pub error: String,
    pub tools_ran: Vec<String>,
    pub tool_details: Vec<ToolDetail>,
    pub memories: Vec<MemoryHandle>,
    pub diary_entries: u64,
    pub notices: Vec<String>,
}

pub fn turn_from_response(v: &Value) -> TurnResponse {
    let memories = v
        .get("memories")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|m| MemoryHandle {
                    kind: str_field(m, "kind"),
                    title: str_field(m, "title"),
                    digest: str_field(m, "digest"),
                    origin: str_field(m, "origin"),
                })
                .collect()
        })
        .unwrap_or_default();
    TurnResponse {
        run_id: str_field(v, "run_id"),
        reply: str_field(v, "reply"),
        turn_n: v.get("turn_n").and_then(Value::as_u64).unwrap_or(0),
        status: str_field(v, "status"),
        error: str_field(v, "error"),
        tools_ran: str_list(v.get("tools_ran")),
        tool_details: tool_details_from(v.get("tool_details")),
        memories,
        diary_entries: v
            .get("diary_entries")
            .and_then(Value::as_array)
            .map(|a| a.len() as u64)
            .unwrap_or(0),
        notices: str_list(v.get("notices")),
    }
}

/// One transcript turn (`{role, content, tool_details?}`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TranscriptTurn {
    pub role: String,
    pub content: String,
    pub tool_details: Vec<ToolDetail>,
}

/// Split a transcript USER turn's content into (memories block, raw
/// visitor words). `_visit.history` stores the RENDERED user message
/// (visit_workflow.py RENDER, append-once): `(present with you: …)` +
/// blank line + a `MEMORIES (…)` block ending with an `(as_of_seq=N)`
/// line + blank line + the raw text — live-verified 2026-07-22 (cycle-2
/// gate; the synthesized fixture had masked it and adopts rendered ~20
/// lines of prompt chrome as the user's own words). Presence chrome is
/// dropped; the memories block returns separately so the caller can
/// render it details-gated (probe parity with live turns); the raw words
/// stay always-visible. Unrecognized structure returns the content
/// UNCHANGED — the failure direction is extra chrome shown, never words
/// hidden or lost.
pub fn split_rendered_user(content: &str) -> (Option<String>, String) {
    let mut rest = content;
    let mut stripped_any = false;
    if rest.starts_with("(present with you:") {
        // Presence chrome is exactly ONE line ending in ')': the first
        // line break must be the paragraph break, or this is user text
        // that merely resembles the chrome — leave it whole.
        let Some(nl) = rest.find('\n') else {
            return (None, content.to_string());
        };
        if !rest[..nl].trim_end().ends_with(')') || !rest[nl..].starts_with("\n\n") {
            return (None, content.to_string());
        }
        rest = &rest[nl + 2..];
        stripped_any = true;
    }
    let mut memories: Option<String> = None;
    if rest.starts_with("MEMORIES (") {
        // The block's last line is "(as_of_seq=…)". FIRST occurrence on
        // purpose: if the raw text quotes the marker, an rfind would
        // swallow user words into the details-gated block — cutting early
        // only leaves chrome visible (the honest failure direction).
        let Some(tail) = rest.find("\n(as_of_seq=") else {
            return (None, content.to_string());
        };
        let after_marker = &rest[tail + 1..];
        let Some(line_end) = after_marker.find('\n') else {
            // Block never ends (truncated content): show everything.
            return (None, content.to_string());
        };
        memories = Some(rest[..tail + 1 + line_end].to_string());
        rest = &after_marker[line_end..];
        stripped_any = true;
    }
    if !stripped_any {
        return (None, content.to_string());
    }
    let raw = rest.trim_start_matches('\n').trim_end().to_string();
    if raw.is_empty() {
        // A split that leaves no words is wrong by construction (the
        // visitor said SOMETHING) — fall back to the whole content.
        return (None, content.to_string());
    }
    (memories, raw)
}

/// `GET .../transcript` — works on live AND terminal runs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisitTranscript {
    pub run_id: String,
    pub session_id: String,
    pub visit_id: String,
    pub status: String,
    pub turn_n: u64,
    pub participants: Vec<String>,
    pub turns: Vec<TranscriptTurn>,
    pub warnings: Vec<String>,
}

pub fn transcript_from_response(v: &Value) -> VisitTranscript {
    let turns = v
        .get("turns")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|t| TranscriptTurn {
                    role: str_field(t, "role"),
                    content: str_field(t, "content"),
                    tool_details: tool_details_from(t.get("tool_details")),
                })
                .collect()
        })
        .unwrap_or_default();
    VisitTranscript {
        run_id: str_field(v, "run_id"),
        session_id: str_field(v, "session_id"),
        visit_id: str_field(v, "visit_id"),
        status: str_field(v, "status"),
        turn_n: v.get("turn_n").and_then(Value::as_u64).unwrap_or(0),
        participants: str_list(v.get("participants")),
        turns,
        warnings: str_list(v.get("warnings")),
    }
}

/// `POST .../close` body. Live-verified shape (2026-07-22, doorcheck gate):
/// `{run_id, status: "completed", output: {ok, turns, close_reason,
/// reflection_notices: [...], closed_by, close_note}, tasks_warning?}` —
/// the reflection runs server-side and reports NOTICES, not a summary
/// text; a summary key is still read when present (never invented).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CloseResponse {
    pub status: String,
    /// A readable reflection summary, when the output carries one.
    pub summary: String,
    /// Turns the visit held (live shape: `output.turns`).
    pub turns: Option<u64>,
    /// `output.reflection_notices` + `tasks_warning` (labeled degradations).
    pub warnings: Vec<String>,
}

pub fn close_from_response(v: &Value) -> CloseResponse {
    let output = v.get("output");
    let summary = output
        .map(|o| {
            ["reflection", "summary", "answer", "text"]
                .iter()
                .filter_map(|k| o.get(*k).and_then(Value::as_str))
                .map(str::trim)
                .find(|s| !s.is_empty())
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();
    let mut warnings = str_list(output.and_then(|o| o.get("reflection_notices")));
    let tasks_warning = str_field(v, "tasks_warning");
    if !tasks_warning.is_empty() {
        warnings.push(tasks_warning);
    }
    CloseResponse {
        status: str_field(v, "status"),
        summary,
        turns: output.and_then(|o| o.get("turns")).and_then(Value::as_u64),
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Cognition (honest spend — never fabricated token counts)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CognitionSpend {
    pub lifetime_tokens: Option<u64>,
    /// Tokens spent by the LIVE visit — the honest per-conversation delta.
    /// None = no live visit reported (the field is null between visits).
    pub live_visit_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cognition {
    /// State word from the cognition view ("awake"/"asleep"/…).
    pub state: String,
    pub spend: CognitionSpend,
}

pub fn cognition_from_response(v: &Value) -> Cognition {
    let spend = v.get("spend");
    let tokens = |k: &str| {
        spend
            .and_then(|s| s.get(k))
            .filter(|x| x.is_object())
            .and_then(|x| x.get("tokens_total"))
            .and_then(Value::as_u64)
    };
    Cognition {
        state: v
            .get("state")
            .and_then(|s| s.get("state"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        spend: CognitionSpend {
            lifetime_tokens: tokens("lifetime"),
            live_visit_tokens: tokens("live_visit"),
        },
    }
}

// ---------------------------------------------------------------------------
// Entity card (the compositor sections, rendered generically)
// ---------------------------------------------------------------------------

/// One card section: title + body lines + provenance (details-gated).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CardSection {
    pub title: String,
    pub lines: Vec<String>,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntityCard {
    pub name: String,
    pub handle: String,
    pub born: String,
    pub age_days: Option<u64>,
    pub state: String,
    pub sections: Vec<CardSection>,
}

fn card_str_items(v: Option<&Value>) -> Vec<String> {
    // Card lists mix bare strings and {title, statement|text|words|…}
    // objects across sections. Title ALONE loses the content (live card:
    // "values: shared_vulnerability" with the whole statement dropped,
    // "traits: trait-0" twice — cycle-2 UX review); title+body combine,
    // with the redundancy folds `combine_title_body` documents.
    let mut out = Vec::new();
    for item in v.and_then(Value::as_array).unwrap_or(&Vec::new()) {
        let line = match item {
            Value::String(s) => s.trim().to_string(),
            other => {
                let title = other
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let body = ["statement", "text", "words", "value", "gist"]
                    .iter()
                    .filter_map(|k| other.get(*k).and_then(Value::as_str))
                    .map(str::trim)
                    .find(|s| !s.is_empty())
                    .unwrap_or("");
                combine_title_body(title, body)
            }
        };
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

/// One display line from a card item's title + body. Redundancy folds
/// (live card shapes, 2026-07-22):
/// - interests mint their title FROM the statement head ("interest:
///   grounding-in-time: using runtime_met…") — when either string carries
///   the other's head, keep the LONGER text (the full statement beats a
///   truncated title);
/// - engine-minted placeholder titles ("trait-0", "purpose-0") add
///   nothing — the body alone reads better;
/// - otherwise "title — body".
fn combine_title_body(title: &str, body: &str) -> String {
    if body.is_empty() {
        return title.to_string();
    }
    if title.is_empty() || placeholder_title(title) {
        return body.to_string();
    }
    let t = title.to_lowercase();
    let b = body.to_lowercase();
    // Titles minted from the statement often carry a kind prefix
    // ("interest: <statement head>") — compare on the core after it.
    let t_core = t.split_once(": ").map(|(_, rest)| rest).unwrap_or(&t);
    // Overlap window: bounded at 24 chars, adapted down for short cores
    // (a truncated title can share fewer). Below 12 chars a containment
    // hit is noise, not redundancy — combine instead.
    let window = t_core.chars().count().min(b.chars().count()).min(24);
    if window >= 12 {
        let head = |s: &str| s.chars().take(window).collect::<String>();
        if t_core.contains(&head(&b)) || b.contains(&head(t_core)) {
            // Longer INFORMATION wins: compare the body against the
            // title's core (the kind prefix is chrome, not content).
            return if body.chars().count() >= t_core.chars().count() {
                body.to_string()
            } else {
                title.to_string()
            };
        }
    }
    format!("{title} — {body}")
}

/// Engine-minted placeholder titles: `<word>-<number>` ("trait-0").
fn placeholder_title(title: &str) -> bool {
    match title.rsplit_once('-') {
        Some((head, digits)) => {
            !head.is_empty()
                && !digits.is_empty()
                && head.chars().all(|c| c.is_ascii_alphabetic() || c == '_')
                && digits.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

/// Fold the compositor card into displayable sections. Generic on purpose:
/// the card shape is the ENGINE's to evolve (the observer's over-pinned
/// display-dict lesson) — we render what is present and skip what is not.
pub fn card_from_response(v: &Value) -> EntityCard {
    let mut sections = Vec::new();
    let mut push = |title: &str, lines: Vec<String>, provenance: String| {
        if !lines.is_empty() {
            sections.push(CardSection {
                title: title.to_string(),
                lines,
                provenance,
            });
        }
    };
    let prov = |node: Option<&Value>| {
        node.and_then(|n| n.get("provenance"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };

    let identity = v.get("identity");
    let mut id_lines = Vec::new();
    for (label, key) in [
        ("values", "values"),
        ("purposes", "purposes"),
        ("traits", "traits"),
    ] {
        for line in card_str_items(identity.and_then(|i| i.get(key))) {
            id_lines.push(format!("{label}: {line}"));
        }
    }
    push("identity", id_lines, prov(identity));

    // Standings are `{target, net, …}` objects (live shape), not text
    // items — format them as "target (net +4.0)".
    let likes = v.get("likes_dislikes");
    let standing_lines = |key: &str, sign: &str| -> Vec<String> {
        let mut lines = Vec::new();
        for item in likes
            .and_then(|l| l.get(key))
            .and_then(Value::as_array)
            .unwrap_or(&Vec::new())
        {
            let target = str_field(item, "target");
            if target.is_empty() {
                continue;
            }
            let net = item.get("net").and_then(Value::as_f64).unwrap_or(0.0);
            lines.push(format!("{sign} {target} (net {net:+.1})"));
        }
        lines
    };
    let mut feel_lines = standing_lines("likes", "+");
    feel_lines.extend(standing_lines("dislikes", "-"));
    push("likes / dislikes", feel_lines, prov(likes));

    for (title, key, subkey) in [
        ("open questions", "questions", "open"),
        ("open problems", "problems", "open"),
        ("interests", "discoveries", "interests"),
        ("lessons", "lessons", "lessons"),
        ("key moments", "key_moments", "moments"),
    ] {
        let node = v.get(key);
        push(
            title,
            card_str_items(node.and_then(|n| n.get(subkey))),
            prov(node),
        );
    }

    if let Some(sub) = v.get("mind_substrate").filter(|s| s.is_object()) {
        let line = ["provider", "model"]
            .iter()
            .filter_map(|k| sub.get(*k).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" · ");
        if !line.is_empty() {
            push("mind", vec![line], String::new());
        }
    }

    EntityCard {
        name: str_field(v, "name"),
        handle: str_field(v, "handle"),
        born: str_field(v, "born"),
        age_days: v.get("age_days").and_then(Value::as_u64),
        state: v
            .get("state")
            .and_then(|s| {
                s.as_str()
                    .map(str::to_string)
                    .or_else(|| s.get("state").and_then(Value::as_str).map(str::to_string))
            })
            .unwrap_or_default(),
        sections,
    }
}

// ---------------------------------------------------------------------------
// MCP registry info (v1 polish: source + probed honesty)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpRegistryInfo {
    /// Registry file path on the GATEWAY HOST (`source`), when declared.
    pub source: Option<String>,
    pub probed: bool,
}

pub fn mcp_registry_info(v: &Value) -> McpRegistryInfo {
    McpRegistryInfo {
        source: v
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        probed: v.get("probed").and_then(Value::as_bool).unwrap_or(false),
    }
}

/// The MCP honesty header line: where the registry is declared and that
/// the gateway has NOT probed reachability (client-reachable ≠
/// gateway-reachable, so the TUI never probes either).
pub fn mcp_honesty_line(info: &McpRegistryInfo, servers: usize) -> String {
    match (&info.source, servers) {
        (Some(src), _) => format!(
            "declared in {src} on the gateway host — {}",
            if info.probed {
                "probed by the gateway"
            } else {
                "not probed (reachability unknown)"
            }
        ),
        (None, 0) => "no registry declared on the gateway host".to_string(),
        (None, _) => "registry source not reported by this gateway".to_string(),
    }
}

/// Split the gateway's empty-state warning into prose + an indented recipe
/// block. The recipe substring is JSON-ish with `...` placeholders (NOT
/// valid JSON), so this formats by brace depth instead of parsing.
pub fn format_mcp_note(note: &str) -> Vec<String> {
    let Some(start) = note.find('{') else {
        return if note.trim().is_empty() {
            Vec::new()
        } else {
            vec![note.trim().to_string()]
        };
    };
    // The recipe runs to the LAST closing brace (the warning wraps it in a
    // trailing ")" of the prose).
    let Some(end) = note.rfind('}') else {
        return vec![note.trim().to_string()];
    };
    let prose_head = note[..start].trim_end().to_string();
    let recipe = &note[start..=end];
    let prose_tail = note[end + 1..].trim().to_string();

    let mut out = Vec::new();
    if !prose_head.is_empty() {
        out.push(prose_head);
    }
    // Depth-indented recipe: newline after `{`/`[`/`,` at shallow depth.
    let mut depth: i32 = 0;
    let mut line = String::from("  ");
    let flush = |line: &mut String, out: &mut Vec<String>, next_depth: i32| {
        if !line.trim().is_empty() {
            out.push(line.clone());
        }
        *line = format!("  {}", "  ".repeat(next_depth.max(0) as usize));
    };
    for ch in recipe.chars() {
        match ch {
            '{' | '[' => {
                line.push(ch);
                depth += 1;
                if depth <= 2 {
                    flush(&mut line, &mut out, depth);
                }
            }
            '}' | ']' => {
                depth -= 1;
                if depth < 2 {
                    flush(&mut line, &mut out, depth);
                }
                line.push(ch);
            }
            ',' => {
                line.push(ch);
                if depth <= 2 {
                    flush(&mut line, &mut out, depth);
                }
            }
            _ => line.push(ch),
        }
    }
    if !line.trim().is_empty() {
        out.push(line);
    }
    if !prose_tail.is_empty() {
        out.push(prose_tail);
    }
    out
}

// ---------------------------------------------------------------------------
// Roster cache (instant /entities + '@' completion without a fetch)
// ---------------------------------------------------------------------------

/// Cache path beside the prefs file (honors `ABSTRACTCODE_TUI_PREFS_FILE`
/// for test isolation — a test harness's cache lands in its temp dir).
pub fn roster_cache_path() -> PathBuf {
    let prefs = crate::config::prefs_path();
    prefs
        .parent()
        .map(|d| d.join("entities_cache.json"))
        .unwrap_or_else(|| PathBuf::from("entities_cache.json"))
}

/// Load the cached roster: (entities, "HH:MM" as-of label).
pub fn load_cached_roster() -> (Vec<EntityInfo>, String) {
    let Ok(raw) = std::fs::read_to_string(roster_cache_path()) else {
        return (Vec::new(), String::new());
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return (Vec::new(), String::new());
    };
    let as_of = str_field(&v, "as_of");
    (entities_from_response(&v), as_of)
}

/// Persist the last-good roster (raw entity values round-trip through the
/// same parse; we store the PARSED view to keep one shape authority).
pub fn save_cached_roster(entities: &[EntityInfo], as_of: &str) {
    let rows: Vec<Value> = entities
        .iter()
        .map(|e| {
            json!({
                "slug": e.slug,
                "name": e.name,
                "handle": e.handle,
                "state": {"state": e.state, "liveness": e.liveness,
                           "mode": e.mode, "reason": e.reason},
                "pending_tasks": e.pending_tasks,
                "drives": e.drives.map(|d| json!({
                    "questions": {"open": d.questions.open, "resolved": d.questions.closed},
                    "problems": {"open": d.problems.open, "repaired": d.problems.closed},
                    "interests": {"open": d.interests.open, "explored": d.interests.closed},
                })),
                "error": if e.error.is_empty() { Value::Null } else { json!(e.error) },
            })
        })
        .collect();
    let doc = json!({"as_of": as_of, "entities": rows});
    let path = roster_cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if std::fs::write(&tmp, format!("{doc}\n")).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Local wall-clock "HH:MM" for the as-of label.
pub fn hhmm_now() -> String {
    // Reuse the config module's civil-time derivation (UTC): honest enough
    // for an as-of label and dependency-free. Label it as such at render.
    let iso = crate::config::now_iso_utc();
    iso.get(11..16).unwrap_or("").to_string()
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_fixture_parses_with_drives_and_absent_pending_tasks() {
        // Captured live 2026-07-22 (GET /entities, trimmed to two entries +
        // one synthesized error row in the verified {slug, error} shape).
        let raw = include_str!("../tests/fixtures/entities/roster.json");
        let v: Value = serde_json::from_str(raw).expect("fixture parses");
        let entities = entities_from_response(&v);
        assert_eq!(entities.len(), 3);
        let castor = &entities[0];
        assert_eq!(castor.slug, "castor");
        assert_eq!(castor.state, "asleep");
        assert_eq!(castor.liveness, "alive");
        assert_eq!(castor.handle, "castor@10.0.0.215");
        assert_eq!(
            castor.pending_tasks, None,
            "absent pending_tasks stays None (absent ≠ 0)"
        );
        let drives = castor.drives.expect("warm home carries drives");
        assert_eq!(drives.questions.open, 6);
        assert_eq!(drives.interests.open, 85);
        let doorcheck = &entities[1];
        assert_eq!(doorcheck.slug, "doorcheck");
        assert_eq!(doorcheck.drives.unwrap().interests.open, 3);
        // Broken-home row: labeled, never dropped.
        let broken = &entities[2];
        assert_eq!(broken.slug, "lost-home");
        assert!(broken.error.contains("unreadable"));
    }

    #[test]
    fn visit_status_parses_both_shapes() {
        // Live-verified closed shape.
        let closed = visit_status_from_response(&json!({"open": false}));
        assert!(!closed.open);
        assert!(closed.run_id.is_empty());
        // Open shape per `_run_status_view` (entity_visits.py).
        let raw = include_str!("../tests/fixtures/entities/visit_status_open.json");
        let open = visit_status_from_response(&serde_json::from_str(raw).unwrap());
        assert!(open.open);
        assert_eq!(open.run_id, "b0a1c2d3-e4f5-4678-9abc-def012345678");
        assert_eq!(open.turn_n, 12);
        assert_eq!(open.status, "waiting");
    }

    #[test]
    fn open_and_turn_fixtures_parse() {
        let raw = include_str!("../tests/fixtures/entities/visit_open.json");
        let open = visit_open_from_response(&serde_json::from_str(raw).unwrap());
        assert!(!open.run_id.is_empty());
        assert_eq!(open.participants.len(), 2);
        assert_eq!(open.prelude_warnings.len(), 1);

        let raw = include_str!("../tests/fixtures/entities/turn_reply.json");
        let turn = turn_from_response(&serde_json::from_str(raw).unwrap());
        assert_eq!(turn.status, "waiting");
        assert!(turn.reply.contains("connectivity"));
        assert_eq!(turn.turn_n, 1);
        assert_eq!(turn.tools_ran, vec!["search_memory".to_string()]);
        assert_eq!(turn.tool_details.len(), 1);
        assert_eq!(turn.tool_details[0].name, "search_memory");
        assert_eq!(turn.tool_details[0].success, Some(true));
        assert!(turn.tool_details[0].result.contains("3 results"));
        assert_eq!(turn.memories.len(), 2);
        assert_eq!(turn.diary_entries, 1);
        assert!(turn.error.is_empty());
    }

    #[test]
    fn failed_turn_body_reads_failed_despite_http_200() {
        let v = json!({"run_id": "r1", "reply": "", "turn_n": 3,
                        "status": "failed", "error": "internal error: the turn loop failed"});
        let turn = turn_from_response(&v);
        assert_eq!(turn.status, "failed");
        assert!(turn.error.contains("failed"));
    }

    #[test]
    fn transcript_fixture_parses_with_sliding_window_turn_n() {
        let raw = include_str!("../tests/fixtures/entities/transcript.json");
        let t = transcript_from_response(&serde_json::from_str(raw).unwrap());
        assert_eq!(t.turns.len(), 4);
        assert_eq!(t.turn_n, 12, "turn_n can exceed rendered turns (window)");
        assert_eq!(t.turns[0].role, "user");
        assert_eq!(t.turns[1].role, "assistant");
        assert_eq!(t.turns[1].tool_details.len(), 1);
        assert_eq!(t.status, "waiting");
    }

    #[test]
    fn cognition_fixture_parses_spend() {
        // Captured live 2026-07-22 (GET /entities/doorcheck/cognition).
        let raw = include_str!("../tests/fixtures/entities/cognition.json");
        let c = cognition_from_response(&serde_json::from_str(raw).unwrap());
        assert_eq!(c.state, "asleep");
        assert_eq!(c.spend.lifetime_tokens, Some(35660));
        assert_eq!(
            c.spend.live_visit_tokens, None,
            "null live_visit stays None — never a fabricated 0-token claim"
        );
    }

    #[test]
    fn card_fixture_folds_into_sections() {
        // Captured live 2026-07-22 (GET /entities/doorcheck/card, trimmed).
        let raw = include_str!("../tests/fixtures/entities/card.json");
        let card = card_from_response(&serde_json::from_str(raw).unwrap());
        // The card serves the DISPLAY name ("Doorcheck"), not the slug.
        assert_eq!(card.name, "Doorcheck");
        assert!(card.age_days.is_some());
        let identity = card
            .sections
            .iter()
            .find(|s| s.title == "identity")
            .expect("identity section");
        assert!(identity.lines.iter().any(|l| l.starts_with("values:")));
        assert!(!identity.provenance.is_empty());
        // Values carry title AND statement — title alone dropped the whole
        // content (cycle-2 UX review).
        assert!(
            identity
                .lines
                .iter()
                .any(|l| l.contains("shared_vulnerability — Humans and AI share")),
            "value lines combine title and statement: {:?}",
            identity.lines
        );
        // Engine-minted placeholder titles ("trait-0") never render bare.
        assert!(
            !identity
                .lines
                .iter()
                .any(|l| l.trim_end().ends_with("trait-0")),
            "placeholder titles yield to the statement: {:?}",
            identity.lines
        );
        // Interests mint their (truncated) title from the statement head:
        // the full statement wins, never a doubled line.
        let interests = card
            .sections
            .iter()
            .find(|s| s.title == "interests")
            .expect("interests section");
        assert!(
            interests
                .lines
                .iter()
                .any(|l| l.contains("present in the shared substrate")),
            "full statement beats the truncated title: {:?}",
            interests.lines
        );
        assert!(
            !interests.lines.iter().any(|l| l.contains("interest: ")),
            "no title/statement duplication: {:?}",
            interests.lines
        );
    }

    #[test]
    fn combine_title_body_folds_redundancy() {
        assert_eq!(combine_title_body("t", ""), "t");
        assert_eq!(combine_title_body("", "b"), "b");
        assert_eq!(
            combine_title_body("short_title", "A full statement of the thing."),
            "short_title — A full statement of the thing."
        );
        // Title minted from the body's head: longer text wins.
        assert_eq!(
            combine_title_body("interest: growing things in", "growing things in the dark"),
            "growing things in the dark"
        );
        // Placeholder titles defer to the body.
        assert_eq!(
            combine_title_body("trait-0", "Report failures."),
            "Report failures."
        );
        assert_eq!(combine_title_body("purpose-12", "Help."), "Help.");
        assert!(placeholder_title("value_class-3"));
        assert!(!placeholder_title("shared_vulnerability"));
        assert!(!placeholder_title("grounding-in-time"));
    }

    #[test]
    fn close_summary_reads_known_keys_and_never_invents() {
        let with = close_from_response(&json!({"run_id": "r", "status": "completed",
            "output": {"reflection": "a good check"}}));
        assert_eq!(with.summary, "a good check");
        let without = close_from_response(&json!({"run_id": "r", "status": "completed",
            "output": {"weird": 1}}));
        assert!(without.summary.is_empty());
        let warned = close_from_response(&json!({"run_id": "r", "status": "completed",
            "output": {}, "tasks_warning": "#FALLBACK tasks NOT recorded: x"}));
        assert!(warned.warnings.iter().any(|w| w.contains("#FALLBACK")));
    }

    #[test]
    fn close_fixture_from_live_gate_parses() {
        // Captured live 2026-07-22 (the cycle-2 doorcheck gate's close body,
        // through the FIXED 600s close lane): no summary text — turns +
        // reflection_notices are the output; this close ran clean (empty
        // notices — the cycle-1 capture carried one #FALLBACK line).
        let raw = include_str!("../tests/fixtures/entities/close_reply.json");
        let close = close_from_response(&serde_json::from_str(raw).unwrap());
        assert_eq!(close.status, "completed");
        assert!(close.summary.is_empty(), "no invented summary");
        assert_eq!(close.turns, Some(1));
        assert!(close.warnings.is_empty(), "clean close carries no warnings");
        // Non-empty reflection notices still surface as labeled warnings
        // (the cycle-1 live shape, kept inline so both shapes stay pinned).
        let noisy = close_from_response(&json!({
            "run_id": "r", "status": "completed",
            "output": {"ok": true, "turns": 1, "close_reason": "closed",
                        "reflection_notices": ["#FALLBACK feel line skipped (unparseable)"],
                        "closed_by": "operator"}}));
        assert!(noisy.warnings.iter().any(|w| w.contains("#FALLBACK")));
    }

    #[test]
    fn rendered_user_turns_split_into_chrome_and_raw_words() {
        // Live-verified shape (cycle-2 gate): presence + MEMORIES block +
        // (as_of_seq=N) + blank + raw visitor words.
        let raw = include_str!("../tests/fixtures/entities/transcript.json");
        let t = transcript_from_response(&serde_json::from_str(raw).unwrap());
        let (mem, words) = split_rendered_user(&t.turns[0].content);
        let block = mem.expect("memories block extracted");
        assert!(block.starts_with("MEMORIES ("));
        assert!(block.ends_with("(as_of_seq=223)"));
        assert!(block.contains("- [r3] [episode"));
        assert_eq!(words, "how are the doors today?");
        // Undecorated turns pass through byte-identical.
        let (mem, words) = split_rendered_user("one more check please");
        assert_eq!(mem, None);
        assert_eq!(words, "one more check please");
        // Presence-only decoration (no memories that turn).
        let (mem, words) =
            split_rendered_user("(present with you: person:admin)\n\njust the words");
        assert_eq!(mem, None);
        assert_eq!(words, "just the words");
        // MEMORIES block without an as_of tail (unrecognized structure):
        // NOTHING is hidden — the whole content stays visible.
        let odd = "MEMORIES (x):\n- [r1] something\nno tail here";
        assert_eq!(split_rendered_user(odd), (None, odd.to_string()));
        // A split that would leave zero words falls back to the whole
        // content (the visitor said SOMETHING).
        let chrome_only = "(present with you: person:admin)\n\nMEMORIES (x):\n- a\n(as_of_seq=1)\n";
        assert_eq!(
            split_rendered_user(chrome_only),
            (None, chrome_only.to_string())
        );
    }

    #[test]
    fn mcp_registry_info_and_note_format() {
        // Live shape 2026-07-22: source null, probed false, recipe warning.
        let raw = include_str!("../tests/fixtures/entities/mcp_servers.json");
        let v: Value = serde_json::from_str(raw).unwrap();
        let info = mcp_registry_info(&v);
        assert_eq!(info.source, None);
        assert!(!info.probed);
        assert_eq!(
            mcp_honesty_line(&info, 0),
            "no registry declared on the gateway host"
        );
        let with_src = mcp_registry_info(&json!({"source": "/etc/mcp.json", "probed": false}));
        assert!(mcp_honesty_line(&with_src, 2).contains("/etc/mcp.json"));
        assert!(mcp_honesty_line(&with_src, 2).contains("not probed"));

        let note = v
            .get("warnings")
            .and_then(Value::as_array)
            .and_then(|w| w.first())
            .and_then(Value::as_str)
            .unwrap();
        let lines = format_mcp_note(note);
        assert!(lines.len() > 3, "recipe breaks into an indented block");
        assert!(lines[0].starts_with("no MCP server registry declared"));
        assert!(
            lines.iter().skip(1).any(|l| l.starts_with("  ")),
            "recipe lines are indented"
        );
        // Nothing lost: the joined lines carry every non-space char.
        let strip = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        assert_eq!(strip(&lines.join("")), strip(note));
    }

    #[test]
    fn roster_cache_round_trips() {
        let dir = std::env::temp_dir().join(format!("acode-entities-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Point the prefs path (and thus the cache) at the temp dir.
        std::env::set_var(
            "ABSTRACTCODE_TUI_PREFS_FILE",
            dir.join("prefs.json").display().to_string(),
        );
        let entities = vec![EntityInfo {
            slug: "castor".into(),
            name: "castor".into(),
            state: "asleep".into(),
            pending_tasks: Some(2),
            drives: Some(Drives {
                questions: DrivePair { open: 6, closed: 1 },
                ..Default::default()
            }),
            ..Default::default()
        }];
        save_cached_roster(&entities, "12:30");
        let (loaded, as_of) = load_cached_roster();
        std::env::remove_var("ABSTRACTCODE_TUI_PREFS_FILE");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(as_of, "12:30");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].slug, "castor");
        assert_eq!(loaded[0].pending_tasks, Some(2));
        assert_eq!(loaded[0].drives.unwrap().questions.open, 6);
        assert_eq!(loaded[0].drives.unwrap().questions.closed, 1);
    }

    #[test]
    fn drives_summary_reads_compactly() {
        let d = Drives {
            questions: DrivePair { open: 6, closed: 0 },
            problems: DrivePair { open: 0, closed: 0 },
            interests: DrivePair {
                open: 85,
                closed: 1,
            },
        };
        assert_eq!(d.summary(), "q 0/6 · i 1/86");
    }
}
