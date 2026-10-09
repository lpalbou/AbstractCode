//! Automations v1 — the terminal client's pure rules (no UI, no HTTP).
//!
//! The Rust mirror of the ui-kit's `panel_core.ts`
//! (`@abstractframework/ui-kit`, `src/automations/panel_core.ts`) and of the
//! Assistant's `core/automations.py`: the gateway's wire shapes parsed into
//! typed rows, the labels every client prints (cadence, "Active ▶", "Run #7
//! running", "next 2026-09-27 09:00 Europe/Paris (in 25 min)"), which controls apply,
//! occurrences as chat pairs, the exact `POST /api/gateway/automations` and
//! `PATCH` bodies, the typed wait answers and the error sentences.
//!
//! Two framework rules are structural here, never inferred:
//! - a run is in progress only when the gateway says so in
//!   `current_occurrence` (never from `last_occurrence` or the rows);
//! - the next run comes only from the gateway's served `next_run_at` /
//!   `next_run_local` (round 16, R16.1): no client arithmetic, no zone math —
//!   the local wall time is CUT from the served string; a calendar rule reads
//!   as the served `schedule_rule_text`, never a sentence built here.
//!
//! Everything reads STRUCTURE (statuses, `notify` objects, wait kinds,
//! config fields) — never model prose. The canonical wire examples are the
//! ui-kit fixtures, vendored byte-identical under `tests/fixtures/automations`.

use serde_json::{json, Map, Value};

/// The gateway interface whose default agent `@default` targets name.
pub const CODE_AGENT_INTERFACE: &str = "abstractcode.agent.v1";
/// Page size for the list and the occurrences (full pages are polled).
pub const PAGE_LIMIT: u32 = 50;
/// The consent line wherever an automation is created with `tool_approval: "auto"`.
pub const TOOL_APPROVAL_CONSENT: &str =
    "Tools run without asking (you approve them now by creating this automation)";
/// What Discuss does, in the words every client uses.
pub const DISCUSS_HELP: &str = "Discuss forks this automation at the selected occurrence with its full history: a new chat \
in its own writable workspace; the automation's files are mounted read-only for the file tools (shell commands are not \
sandboxed), and nothing is written back into the automation's session.";

// ---------------------------------------------------------------------------
// Wire shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    pub source_id: String,
    pub source_version: u64,
    pub config: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentOccurrence {
    pub index: u64,
    pub run_id: String,
    pub attempt: u64,
    /// `admitted` | `running` | `backoff`.
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastOccurrence {
    pub run_id: String,
    pub index: u64,
    pub status: String,
    pub attempts: u64,
    pub excerpt: String,
    /// When the occurrence fired / finished (RFC3339) — the timing line's
    /// "last 3 h ago" reads `finished_at`, else `fired_at`.
    pub fired_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionItem {
    /// `notify` | `failure`.
    pub kind: String,
    pub run_id: String,
    pub index: u64,
    pub title: String,
    pub body: String,
    pub cursor: String,
}

/// A typed human wait (decision D1): the answer follows `kind`, never the prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct Wait {
    pub run_id: String,
    pub wait_key: String,
    /// `ask_user` | `tool_approval` | `event` (anything else is shown, not answered).
    pub kind: String,
    pub reason: String,
    pub prompt: String,
    pub choices: Vec<String>,
    pub details: Value,
    /// The occurrence it belongs to (summary waits carry it).
    pub index: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attention {
    pub pending_waits: u64,
    pub unseen_count: u64,
    /// At most 20, oldest unseen first.
    pub items: Vec<AttentionItem>,
    pub waits: Vec<Wait>,
}

/// A row of `GET /api/gateway/automations`.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub trigger: Trigger,
    pub context_mode: String,
    pub workspace_root: Option<String>,
    /// Served (R16.1): the next run in UTC (absent when none).
    pub next_run_at: Option<String>,
    /// Served: the same instant as ISO with the offset of `time_zone`.
    pub next_run_local: Option<String>,
    /// Served: the schedule's IANA zone (the owner's for non-v2 triggers).
    pub time_zone: String,
    /// Served: the rule + " · next Thu 9 Oct 08:00" when a next run exists.
    pub schedule_text: String,
    /// Served: the rule alone ("Every day at 08:00 (Europe/Paris)").
    pub schedule_rule_text: String,
    pub current: Option<CurrentOccurrence>,
    pub occurrence_count: u64,
    pub last: Option<LastOccurrence>,
    pub attention: Attention,
    pub legacy: bool,
    pub revision: Option<u64>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub reason_code: String,
    pub message: String,
    pub attempts: u64,
}

/// A row of `GET /api/gateway/automations/{id}/occurrences`.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    pub run_id: String,
    pub index: u64,
    pub attempts: u64,
    pub fired_at: String,
    pub status: String,
    pub trigger_summary: String,
    pub user_turn: String,
    pub answer: String,
    pub notify: Option<(String, String)>,
    pub failure: Option<Failure>,
    pub artifacts: Vec<String>,
    pub waits: Vec<Wait>,
}

/// The parts of `GET /api/gateway/automations/{id}` → `definition` the
/// terminal shows (the gateway's definition is the source of truth: the
/// growing-context settings are printed as the gateway states them).
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub revision: u64,
    pub workflow_id: String,
    pub tool_approval: String,
    pub growing: Map<String, Value>,
    pub max_attempts: Option<u64>,
    pub workspace_root: String,
    /// The definition's whole `target` (`bundle_ref`, `flow_id`,
    /// `input_data`) as the gateway returned it: the settings panels read
    /// the run settings from `input_data` and save a revision of it.
    pub target: Value,
    /// The definition's `notify` (`{channels, recipients?}`; `null` = the
    /// default, in the console): "Email result" and its recipients.
    pub notify: Value,
}

/// `POST …/discuss` answer: the fork's session, its own writable folder and
/// the automation's folder mounted read-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscussResponse {
    pub session_id: String,
    pub run_id: String,
    pub workspace_root: String,
    pub mounted_workspace: String,
}

/// A page of rows plus the cursor of the next (older) page.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsing (loud: a missing required field is an error naming it)
// ---------------------------------------------------------------------------

type Parse<T> = Result<T, String>;

fn req_str(v: &Value, key: &str, what: &str) -> Parse<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{what}: `{key}` is missing or not a string"))
}

fn req_u64(v: &Value, key: &str, what: &str) -> Parse<u64> {
    v.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{what}: `{key}` is missing or not a whole number"))
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn str_list(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_trigger(v: &Value, what: &str) -> Parse<Trigger> {
    let t = v
        .get("trigger")
        .ok_or_else(|| format!("{what}: `trigger` is missing"))?;
    Ok(Trigger {
        source_id: req_str(t, "source_id", "trigger")?,
        source_version: req_u64(t, "source_version", "trigger")?,
        config: t
            .get("config")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
    })
}

pub fn parse_wait(v: &Value) -> Parse<Wait> {
    Ok(Wait {
        run_id: req_str(v, "run_id", "wait")?,
        wait_key: req_str(v, "wait_key", "wait")?,
        kind: req_str(v, "kind", "wait")?,
        reason: opt_str(v, "reason").unwrap_or_default(),
        prompt: opt_str(v, "prompt").unwrap_or_default(),
        choices: str_list(v, "choices"),
        details: v.get("details").cloned().unwrap_or(Value::Null),
        index: v.get("index").and_then(Value::as_u64),
    })
}

fn parse_waits(v: &Value, key: &str) -> Parse<Vec<Wait>> {
    match v.get(key) {
        Some(Value::Array(a)) => a.iter().map(parse_wait).collect(),
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(_) => Err(format!("`{key}` is not a list")),
    }
}

/// The served schedule block of one summary row (R16.1: `next_run_at`,
/// `next_run_local`, `time_zone`, `schedule_text`, `schedule_rule_text`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServedSchedule {
    pub next_run_at: Option<String>,
    pub next_run_local: Option<String>,
    pub time_zone: String,
    pub schedule_text: String,
    pub schedule_rule_text: String,
}

/// True for a UTC ISO timestamp ("…+00:00" / "…Z"), the runtime's form of
/// `next_fire_at`.
fn is_utc_iso(ts: &str) -> bool {
    ts.ends_with("+00:00") || ts.ends_with('Z')
}

/// Every field of the served schedule block is OPTIONAL on read: a gateway
/// before round 16 (0.13.x) serves none of them — only the runtime's
/// `next_fire_at` (UTC) — and a row must never fail the list for it. A
/// missing or non-string field reads as empty (shown as "—", see
/// [`served_rule`]); the next run falls back to the served `next_fire_at`,
/// shown in UTC (its own zone: the string is CUT, no clock arithmetic).
pub fn served_schedule(v: &Value) -> ServedSchedule {
    let text = |key: &str| opt_str(v, key).filter(|s| !s.is_empty());
    let next_run_at = text("next_run_at").or_else(|| text("next_fire_at"));
    let mut next_run_local = text("next_run_local");
    let mut time_zone = text("time_zone").unwrap_or_default();
    if next_run_local.is_none() && time_zone.is_empty() {
        if let Some(at) = next_run_at.as_deref().filter(|at| is_utc_iso(at)) {
            next_run_local = Some(at.to_string());
            time_zone = "UTC".into();
        }
    }
    ServedSchedule {
        next_run_at,
        next_run_local,
        time_zone,
        schedule_text: text("schedule_text").unwrap_or_default(),
        schedule_rule_text: text("schedule_rule_text").unwrap_or_default(),
    }
}

pub fn parse_summary(v: &Value) -> Parse<Summary> {
    let what = "automation summary";
    let att = v
        .get("attention")
        .ok_or_else(|| format!("{what}: `attention` is missing"))?;
    let items = match att.get("items") {
        Some(Value::Array(a)) => a
            .iter()
            .map(|i| {
                Ok(AttentionItem {
                    kind: req_str(i, "kind", "attention item")?,
                    run_id: opt_str(i, "run_id").unwrap_or_default(),
                    index: i.get("index").and_then(Value::as_u64).unwrap_or(0),
                    title: opt_str(i, "title").unwrap_or_default(),
                    body: opt_str(i, "body").unwrap_or_default(),
                    cursor: req_str(i, "cursor", "attention item")?,
                })
            })
            .collect::<Parse<Vec<_>>>()?,
        _ => Vec::new(),
    };
    let current = match v.get("current_occurrence") {
        Some(c) if c.is_object() => Some(CurrentOccurrence {
            index: req_u64(c, "index", "current_occurrence")?,
            run_id: req_str(c, "run_id", "current_occurrence")?,
            attempt: c.get("attempt").and_then(Value::as_u64).unwrap_or(1),
            status: req_str(c, "status", "current_occurrence")?,
        }),
        _ => None,
    };
    let last = match v.get("last_occurrence") {
        Some(l) if l.is_object() => Some(LastOccurrence {
            run_id: req_str(l, "run_id", "last_occurrence")?,
            index: req_u64(l, "index", "last_occurrence")?,
            status: req_str(l, "status", "last_occurrence")?,
            attempts: l.get("attempts").and_then(Value::as_u64).unwrap_or(1),
            excerpt: opt_str(l, "excerpt").unwrap_or_default(),
            fired_at: opt_str(l, "fired_at").filter(|s| !s.is_empty()),
            finished_at: opt_str(l, "finished_at").filter(|s| !s.is_empty()),
        }),
        _ => None,
    };
    if !v.get("capabilities").is_some_and(Value::is_array) {
        return Err(format!("{what}: `capabilities` is missing or not a list"));
    }
    let served = served_schedule(v);
    Ok(Summary {
        id: req_str(v, "automation_id", what)?,
        title: req_str(v, "title", what)?,
        status: req_str(v, "status", what)?,
        trigger: parse_trigger(v, what)?,
        context_mode: opt_str(v, "context_mode").unwrap_or_default(),
        workspace_root: opt_str(v, "workspace_root").filter(|s| !s.is_empty()),
        next_run_at: served.next_run_at,
        next_run_local: served.next_run_local,
        time_zone: served.time_zone,
        schedule_text: served.schedule_text,
        schedule_rule_text: served.schedule_rule_text,
        current,
        occurrence_count: v
            .get("occurrence_count")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        last,
        attention: Attention {
            pending_waits: att
                .get("pending_waits")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            unseen_count: att.get("unseen_count").and_then(Value::as_u64).unwrap_or(0),
            items,
            waits: parse_waits(att, "waits")?,
        },
        legacy: v.get("legacy").and_then(Value::as_bool).unwrap_or(false),
        revision: v.get("revision").and_then(Value::as_u64),
        capabilities: str_list(v, "capabilities"),
    })
}

pub fn parse_occurrence(v: &Value) -> Parse<Occurrence> {
    let what = "occurrence";
    let notify = match v.get("notify") {
        Some(n) if n.is_object() => Some((
            opt_str(n, "title").unwrap_or_default(),
            opt_str(n, "body").unwrap_or_default(),
        )),
        _ => None,
    };
    let failure = match v.get("failure") {
        Some(f) if f.is_object() => Some(Failure {
            reason_code: opt_str(f, "reason_code").unwrap_or_default(),
            message: opt_str(f, "message").unwrap_or_default(),
            attempts: f.get("attempts").and_then(Value::as_u64).unwrap_or(1),
        }),
        _ => None,
    };
    let artifacts = v
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| opt_str(x, "name")).collect())
        .unwrap_or_default();
    Ok(Occurrence {
        run_id: req_str(v, "run_id", what)?,
        index: req_u64(v, "index", what)?,
        attempts: v.get("attempts").and_then(Value::as_u64).unwrap_or(1),
        fired_at: opt_str(v, "fired_at").unwrap_or_default(),
        status: req_str(v, "status", what)?,
        trigger_summary: v
            .get("trigger")
            .and_then(|t| opt_str(t, "summary"))
            .unwrap_or_default(),
        user_turn: opt_str(v, "user_turn").unwrap_or_default(),
        answer: opt_str(v, "answer").unwrap_or_default(),
        notify,
        failure,
        artifacts,
        waits: parse_waits(v, "waits")?,
    })
}

fn parse_page<T>(v: &Value, what: &str, row: fn(&Value) -> Parse<T>) -> Parse<Page<T>> {
    let items = v
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{what}: `items` is missing"))?
        .iter()
        .map(row)
        .collect::<Parse<Vec<T>>>()?;
    Ok(Page {
        items,
        next_cursor: opt_str(v, "next_cursor").filter(|s| !s.is_empty()),
    })
}

pub fn parse_list_page(v: &Value) -> Parse<Page<Summary>> {
    parse_page(v, "automation list", parse_summary)
}

pub fn parse_occurrence_page(v: &Value) -> Parse<Page<Occurrence>> {
    parse_page(v, "occurrence list", parse_occurrence)
}

/// `GET /api/gateway/automations/{id}` → (definition, summary).
pub fn parse_detail(v: &Value) -> Parse<(Definition, Summary)> {
    let d = v
        .get("definition")
        .ok_or("automation detail: `definition` is missing")?;
    let summary = parse_summary(
        v.get("summary")
            .ok_or("automation detail: `summary` is missing")?,
    )?;
    let policy = d.get("policy").cloned().unwrap_or(Value::Null);
    let definition = Definition {
        revision: req_u64(d, "revision", "definition")?,
        workflow_id: d
            .get("target")
            .and_then(|t| opt_str(t, "workflow_id"))
            .unwrap_or_default(),
        tool_approval: opt_str(&policy, "tool_approval").unwrap_or_else(|| "auto".into()),
        growing: d
            .get("context")
            .and_then(|c| c.get("growing"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        max_attempts: policy
            .get("retry")
            .and_then(|r| r.get("max_attempts"))
            .and_then(Value::as_u64),
        workspace_root: opt_str(d, "workspace_root").unwrap_or_default(),
        target: d.get("target").cloned().unwrap_or(Value::Null),
        notify: d.get("notify").cloned().unwrap_or(Value::Null),
    };
    Ok((definition, summary))
}

pub fn parse_discuss(v: &Value) -> Parse<DiscussResponse> {
    let what = "discuss response";
    Ok(DiscussResponse {
        session_id: req_str(v, "session_id", what)?,
        run_id: req_str(v, "run_id", what)?,
        workspace_root: req_str(v, "workspace_root", what)?,
        mounted_workspace: req_str(v, "mounted_workspace", what)?,
    })
}

// ---------------------------------------------------------------------------
// Time (occurrence stamps in UTC; the next run is served, see `served_local`)
// ---------------------------------------------------------------------------

fn unix_secs(ts: &str) -> Option<i64> {
    crate::protocol::parse_rfc3339_utc(ts)
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `2026-09-27T08:00:00Z` → `2026-09-27 08:00 UTC` (seconds only when non-zero).
pub fn format_utc(ts: &str) -> String {
    let Some(secs) = unix_secs(ts) else {
        return ts.to_string();
    };
    let (y, m, d) = crate::config::civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    if ss == 0 {
        format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02} UTC")
    } else {
        format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02} UTC")
    }
}

/// "in 25 min", "in 3 h 5 min", "in 2 d 4 h", "due now" — from the served `next_run_at` only.
pub fn relative_in(ts: &str, now: i64) -> String {
    let Some(t) = unix_secs(ts) else {
        return String::new();
    };
    let min = ((t - now) as f64 / 60.0).round() as i64;
    if min <= 0 {
        return "due now".into();
    }
    if min < 60 {
        return format!("in {min} min");
    }
    let h = min / 60;
    if h < 24 {
        return if min % 60 != 0 {
            format!("in {h} h {} min", min % 60)
        } else {
            format!("in {h} h")
        };
    }
    let d = h / 24;
    if h % 24 != 0 {
        format!("in {d} d {} h", h % 24)
    } else {
        format!("in {d} d")
    }
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

fn parse_duration(every: &str) -> Option<(u64, char)> {
    let unit = every.chars().last()?;
    if !matches!(unit, 's' | 'm' | 'h' | 'd') {
        return None;
    }
    let digits = &every[..every.len() - 1];
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().map(|n| (n, unit))
}

/// An interval in seconds (`None` when it is not one).
pub fn duration_seconds(every: &str) -> Option<u64> {
    let (n, unit) = parse_duration(every)?;
    Some(
        n * match unit {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            _ => 86_400,
        },
    )
}

/// True for the contract's `^[1-9][0-9]*[smhd]$` interval.
pub fn is_duration(every: &str) -> bool {
    parse_duration(every).is_some()
}

/// "every 8 hours" / "every hour" — a fixed interval in words. Schedules
/// read as the gateway's `schedule_rule_text` ([`served_rule`]); this is for
/// the email trigger's check interval only (the kit's `emailTriggerLabel`).
pub fn interval_label(every: &str) -> String {
    let Some((n, unit)) = parse_duration(every) else {
        return format!("every {every}");
    };
    let (one, many) = match unit {
        's' => ("second", "seconds"),
        'm' => ("minute", "minutes"),
        'h' => ("hour", "hours"),
        _ => ("day", "days"),
    };
    if n == 1 {
        format!("every {one}")
    } else {
        format!("every {n} {many}")
    }
}

/// The served `next_run_local` ("2026-10-09T08:00:00+02:00", already in the
/// automation's zone) as "2026-10-09 08:00 Europe/Paris": the date and wall
/// time are CUT from the gateway's string — no clock or zone arithmetic.
pub fn served_local(next_run_local: &str, time_zone: &str) -> String {
    let b = next_run_local.as_bytes();
    let shaped = b.len() >= 16
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15]
            .iter()
            .all(|&i| b[i].is_ascii_digit());
    if !shaped {
        return next_run_local.to_string();
    }
    let mut out = format!("{} {}", &next_run_local[..10], &next_run_local[11..16]);
    if !time_zone.is_empty() {
        out.push(' ');
        out.push_str(time_zone);
    }
    out
}

/// A `schedule@2` trigger (any kind): the gateway words it.
pub fn is_schedule_v2(t: &Trigger) -> bool {
    t.source_id == "schedule" && t.source_version == SCHEDULE_VERSION
}

/// What a row shows for a served field the gateway did not send.
pub const NOT_SERVED: &str = "—";

/// The served rule verbatim (`schedule_rule_text`, every schedule row:
/// `schedule@1` and `schedule@2`, Repeat with its bounds included); a
/// missing one (a gateway before round 16) reads as "—" ([`NOT_SERVED`]),
/// never as a sentence made up here.
pub fn served_rule(s: &Summary) -> String {
    if s.schedule_rule_text.is_empty() {
        NOT_SERVED.to_string()
    } else {
        s.schedule_rule_text.clone()
    }
}

/// The trigger in words for one row: any schedule is the served rule; the
/// other sources keep their fixed words.
pub fn summary_trigger_text(s: &Summary) -> String {
    if s.trigger.source_id == "schedule" {
        served_rule(s)
    } else {
        trigger_summary(&s.trigger)
    }
}

/// A non-schedule trigger in words ("manual runs only"); a schedule is
/// never worded here (see `served_rule`).
pub fn trigger_summary(t: &Trigger) -> String {
    match (t.source_id.as_str(), t.source_version) {
        ("manual", 1) => "manual runs only".into(),
        ("email.received", 1) => crate::automation_email::email_trigger_label(&t.config),
        _ => format!("{}@{}", t.source_id, t.source_version),
    }
}

/// The state as TEXT then ICON — the same label in every client
/// ("Active ▶", "Paused ⏸"): the word carries the meaning, the icon is a cue.
pub fn status_label(status: &str) -> String {
    match status {
        "active" => "Active ▶".into(),
        "paused" => "Paused ⏸".into(),
        "completed" => "Completed ✓".into(),
        "failed" => "Failed ✕".into(),
        "archived" => "Archived ▪".into(),
        other => other.to_string(),
    }
}

pub fn context_label(mode: &str) -> &'static str {
    if mode == "growing" {
        "Growing — each run sees the previous runs"
    } else {
        "Independent — each run starts fresh"
    }
}

/// "Run #7 running", "Run #7 starting", "Run #7 waiting to retry (attempt 2)" —
/// from `current_occurrence` only; `None` when nothing is in flight.
pub fn current_label(s: &Summary) -> Option<String> {
    let c = s.current.as_ref()?;
    let attempt = if c.attempt > 1 {
        format!(" (attempt {})", c.attempt)
    } else {
        String::new()
    };
    Some(match c.status.as_str() {
        "backoff" => format!(
            "Run #{} waiting to retry (attempt {})",
            c.index,
            c.attempt + 1
        ),
        "admitted" => format!("Run #{} starting{attempt}", c.index),
        _ => format!("Run #{} running{attempt}", c.index),
    })
}

/// "2026-09-27 09:00 Europe/Paris (in 25 min)" from the served
/// `next_run_local` + `next_run_at` only; "none while paused" / "none
/// scheduled" otherwise.
pub fn next_label(s: &Summary, now: i64) -> String {
    match (&s.next_run_at, &s.next_run_local) {
        (Some(at), Some(local)) => format!(
            "{} ({})",
            served_local(local, &s.time_zone),
            relative_in(at, now)
        ),
        _ if s.status == "paused" => "none while paused".into(),
        _ => "none scheduled".into(),
    }
}

/// The kit's run-now line with the served next run: "Next scheduled run:
/// 2026-09-27 09:00 Europe/Paris." (`run_now_next_run_line`); `None` when
/// nothing is scheduled.
pub fn run_now_next_line(s: &Summary) -> Option<String> {
    let local = s.next_run_local.as_deref()?;
    Some(spec_str(&["run_now_next_run_line"]).replace("{time}", &served_local(local, &s.time_zone)))
}

/// "2 unseen · 1 waiting for you" / "nothing new".
pub fn attention_label(s: &Summary) -> String {
    let mut parts = Vec::new();
    if s.attention.unseen_count > 0 {
        parts.push(format!("{} unseen", s.attention.unseen_count));
    }
    if s.attention.pending_waits > 0 {
        parts.push(format!("{} waiting for you", s.attention.pending_waits));
    }
    if parts.is_empty() {
        "nothing new".into()
    } else {
        parts.join(" · ")
    }
}

/// One list row: title, state, attention, cadence, what runs now, the next run.
pub fn row_line(s: &Summary, now: i64) -> String {
    let mut parts = vec![s.title.clone(), status_label(&s.status)];
    if s.legacy {
        parts.push("legacy schedule".into());
    }
    if s.attention.unseen_count > 0 || s.attention.pending_waits > 0 {
        parts.push(attention_label(s));
    }
    parts.push(summary_trigger_text(s));
    if let Some(cur) = current_label(s) {
        parts.push(format!("now: {cur}"));
    }
    parts.push(format!("next: {}", next_label(s, now)));
    parts.join(" · ")
}

/// The rows the list shows: never the archived ones — those live under the
/// list's `Archived · N` line (the gateway already leaves them out of the
/// default listing; an older gateway that still includes them is filtered
/// here so the two lists never overlap).
pub fn visible(items: &[Summary]) -> Vec<Summary> {
    items
        .iter()
        .filter(|s| s.status != "archived")
        .cloned()
        .collect()
}

/// `Archived · 6` — the quiet line at the end of a list; `None` at 0 (the
/// line is absent, like the web sidebar's footer).
pub fn archived_line(count: u64) -> Option<String> {
    (count > 0).then(|| format!("Archived · {count}"))
}

// ---------------------------------------------------------------------------
// The compact timing line (kit `automations/timing_line.ts`, same rules)
// ---------------------------------------------------------------------------
//
//   card line 1: "↻ every 24 h · last 3 h ago"   (↻ only for a schedule)
//   card line 2: "next in 20 h"                   (+ the Active switch)
//   header:      "every 24 h · last 3 h ago · next in 14 h"
//
// Deterministic: the caller passes `now` (unix seconds); no year, no seconds.

/// A span as one compact unit, rounded DOWN: "<1 min", "N min" (< 60 min),
/// "N h" (< 48 h, so a day reads "24 h"), "N d" — the kit's `compactDuration`.
pub fn compact_duration(secs: i64) -> String {
    let span = secs.unsigned_abs();
    if span < 60 {
        "<1 min".into()
    } else if span < 3600 {
        format!("{} min", span / 60)
    } else if span < 2 * 86_400 {
        format!("{} h", span / 3600)
    } else {
        format!("{} d", span / 86_400)
    }
}

/// A non-schedule trigger in a word or two: "manual", "on new email" —
/// the kit's `compactCadence` (a schedule reads as its served rule).
pub fn compact_cadence(t: &Trigger) -> String {
    match t.source_id.as_str() {
        "manual" => "manual".into(),
        "email.received" if t.source_version == 1 => "on new email".into(),
        other => other.to_string(),
    }
}

/// "last 3 h ago" / "last <1 min ago" / "running now" / "last never"; while
/// an approval or question waits on you, an occurrence in flight is not
/// running: "waiting since 5 min" (from its fired time), or "" when unknown.
pub fn last_run_text(s: &Summary, now: i64) -> String {
    if let Some(cur) = &s.current {
        if s.attention.pending_waits > 0 {
            let same = s
                .last
                .as_ref()
                .filter(|l| l.index == cur.index)
                .and_then(|l| l.fired_at.as_deref())
                .and_then(unix_secs);
            return match same {
                Some(t) => format!("waiting since {}", compact_duration((now - t).max(0))),
                None => String::new(),
            };
        }
        return "running now".into();
    }
    let t = s.last.as_ref().and_then(|l| {
        l.finished_at
            .as_deref()
            .and_then(unix_secs)
            .or_else(|| l.fired_at.as_deref().and_then(unix_secs))
    });
    match t {
        // A run stamped after `now` (clock skew) reads as just now.
        Some(t) => format!("last {} ago", compact_duration((now - t).max(0))),
        None => "last never".into(),
    }
}

/// "next in 14 h" / "next due now" from the served `next_run_at`; `None`
/// when nothing is scheduled (paused, manual, archived, finished).
pub fn next_run_text(s: &Summary, now: i64) -> Option<String> {
    let t = s.next_run_at.as_deref().and_then(unix_secs)?;
    if t - now < 60 {
        Some("next due now".into())
    } else {
        Some(format!("next in {}", compact_duration(t - now)))
    }
}

/// The cadence of one row: any schedule (Repeat included) is the served
/// `schedule_rule_text` verbatim; other sources keep their word.
pub fn summary_cadence(s: &Summary) -> String {
    if s.trigger.source_id == "schedule" {
        served_rule(s)
    } else {
        compact_cadence(&s.trigger)
    }
}

/// The header's one line: cadence · last · next (empty parts omitted).
pub fn timing_line(s: &Summary, now: i64) -> String {
    let mut parts = vec![summary_cadence(s), last_run_text(s, now)];
    parts.extend(next_run_text(s, now));
    parts.retain(|p| !p.is_empty());
    parts.join(" · ")
}

/// The card's two lines: `↻ every 24 h · last 3 h ago` (↻ for a schedule
/// only) and `next in 20 h` (empty when nothing is scheduled).
pub fn card_lines(s: &Summary, now: i64) -> (String, String) {
    let mut parts = vec![summary_cadence(s), last_run_text(s, now)];
    parts.retain(|p| !p.is_empty());
    let mut first = parts.join(" · ");
    if s.trigger.source_id == "schedule" {
        first = format!("↻ {first}");
    }
    (first, next_run_text(s, now).unwrap_or_default())
}

/// "waiting for you" — the card/header badge while an approval or a
/// question is pending.
pub fn waiting_badge(s: &Summary) -> Option<&'static str> {
    (s.attention.pending_waits > 0).then_some("waiting for you")
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Control {
    Pause,
    Resume,
    RunNow,
    StopCurrent,
    Revise,
    Archive,
    Discuss,
    /// Bring an archived automation back (it returns paused).
    Unarchive,
}

impl Control {
    /// The capability name and, for commands, the command type.
    pub fn capability(self) -> &'static str {
        match self {
            Control::Pause => "pause",
            Control::Resume => "resume",
            Control::RunNow => "run_now",
            Control::StopCurrent => "stop_current",
            Control::Revise => "revise",
            Control::Archive => "archive",
            Control::Discuss => "discuss",
            Control::Unarchive => "unarchive",
        }
    }

    /// `automation.*` command type (None for revise/discuss, which have their own routes).
    pub fn command_type(self) -> Option<&'static str> {
        match self {
            Control::Pause => Some("automation.pause"),
            Control::Resume => Some("automation.resume"),
            Control::RunNow => Some("automation.run_now"),
            Control::StopCurrent => Some("automation.stop_current"),
            Control::Archive => Some("automation.archive"),
            Control::Unarchive => Some("automation.unarchive"),
            Control::Revise | Control::Discuss => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Control::Pause => "pause",
            Control::Resume => "resume",
            Control::RunNow => "run now",
            Control::StopCurrent => "stop current",
            Control::Revise => "revise",
            Control::Archive => "archive",
            Control::Discuss => "discuss",
            Control::Unarchive => "unarchive",
        }
    }

    /// The button's short label as every client shows it (the kit's
    /// `automation_controls.json` `labels`: "Run now", "Stop", "Edit",
    /// "Archive", "Unarchive").
    pub fn button(self) -> &'static str {
        match self {
            // The Code header's own short label (`HEADER_BUTTONS`).
            Control::StopCurrent => "Stop",
            other => spec_str(&["labels", other.capability()]),
        }
    }

    /// What the result line says once the gateway accepted the command —
    /// the NEW state, never the verb (the Code web header's
    /// `HEADER_NOTICES` and the kit panel's unarchive notice).
    pub fn accepted_notice(self) -> Option<&'static str> {
        Some(match self {
            Control::Pause => "Automation paused.",
            Control::Resume => "Automation active.",
            Control::RunNow => "Run requested.",
            Control::StopCurrent => "Stop requested.",
            Control::Archive => "Automation archived.",
            Control::Unarchive => "Unarchived: it is paused until you make it active.",
            Control::Revise | Control::Discuss => return None,
        })
    }

    /// The busy line while the command is in flight (Code web header).
    pub fn busy_notice(self) -> &'static str {
        match self {
            Control::Pause => "Pausing…",
            Control::Resume => "Activating…",
            Control::RunNow => "Starting a run…",
            Control::StopCurrent => "Stopping…",
            Control::Archive => "Archiving…",
            Control::Unarchive => "Unarchiving…",
            Control::Revise => "Saving…",
            Control::Discuss => "Opening the discussion…",
        }
    }

    /// The control a `automation.*` command type belongs to.
    pub fn from_command_type(command_type: &str) -> Option<Control> {
        [
            Control::Pause,
            Control::Resume,
            Control::RunNow,
            Control::StopCurrent,
            Control::Archive,
            Control::Unarchive,
        ]
        .into_iter()
        .find(|c| c.command_type() == Some(command_type))
    }
}

// ---------------------------------------------------------------------------
// Control hints (the shared "Run now" text)
// ---------------------------------------------------------------------------

/// The automation controls' names, hints and run-now glyph: a BYTE-IDENTICAL
/// copy of AbstractUIC's canonical `ui-kit/src/automations/automation_controls.json`
/// (the web panel's `CONTROL_HINTS`, the Observer's rows and the Assistant read
/// the same file). The AbstractFramework root `scripts/check_identity_sync.py`
/// fails when this copy drifts.
pub const AUTOMATION_CONTROLS_JSON: &str = include_str!("../assets/automation_controls.json");

fn controls_spec() -> &'static Value {
    static SPEC: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| {
        serde_json::from_str(AUTOMATION_CONTROLS_JSON)
            .expect("assets/automation_controls.json is the kit's canonical JSON")
    })
}

fn spec_str(path: &[&str]) -> &'static str {
    let mut v = controls_spec();
    for key in path {
        v = &v[*key];
    }
    v.as_str()
        .unwrap_or_else(|| panic!("automation_controls.json has no string at {path:?}"))
}

/// Run now in one line: "Run it once now, without waiting for the schedule;
/// the next scheduled run keeps its time." (the kit's `RUN_NOW_ONE_LINE`).
pub fn run_now_one_line() -> &'static str {
    spec_str(&["run_now_one_line"])
}

/// A control's full hint (the kit's `CONTROL_HINTS[id]`, lines joined by "\n").
pub fn control_hint(control: Control) -> &'static str {
    spec_str(&["hints", control.capability()])
}

/// The terminal's Run now line under the key hints (list and one
/// automation): `g run now: <the kit's one line>`. The screen's facts line
/// already shows the next scheduled time.
pub fn run_now_key_line() -> String {
    format!("g run now: {}", run_now_one_line())
}

/// `Ok(())` when the control applies now, `Err(reason)` otherwise — the kit's
/// `automationControls` rule. The server decides what the principal may do
/// (`capabilities`); the status decides what applies at this moment. Run now
/// stays enabled while paused (it does not resume). Discuss stays available on
/// an archived automation (its history is kept).
pub fn control_state(s: &Summary, control: Control, busy: bool) -> Result<(), String> {
    if busy {
        return Err("Working…".into());
    }
    let caps = |c: &str| s.capabilities.iter().any(|x| x == c);
    if control == Control::Discuss {
        if s.legacy {
            return Err("Legacy schedule: discussion is not available.".into());
        }
        if !caps("discuss") {
            return Err("Discussion is not permitted for this automation.".into());
        }
        return Ok(());
    }
    if s.legacy {
        return Err("Legacy schedule: managed with its existing controls.".into());
    }
    if !caps(control.capability()) {
        return Err("Not permitted for this automation.".into());
    }
    if control == Control::Unarchive {
        return if s.status == "archived" {
            Ok(())
        } else {
            Err("Not archived.".into())
        };
    }
    if s.status == "archived" {
        return Err("Archived: history is kept, nothing runs.".into());
    }
    let running = s.current.is_some();
    let live = s.status == "active" || s.status == "paused";
    let (ok, reason) = match control {
        Control::Pause => (
            s.status == "active",
            if s.status == "paused" {
                "Already paused."
            } else {
                "The automation has ended."
            },
        ),
        Control::Resume => (
            s.status == "paused",
            if s.status == "active" {
                "Already running on schedule."
            } else {
                "The automation has ended."
            },
        ),
        Control::RunNow => (
            live && !running,
            if running {
                "An occurrence is in progress."
            } else {
                "The automation has ended."
            },
        ),
        Control::StopCurrent => (running, "Nothing is running."),
        Control::Revise | Control::Archive | Control::Discuss | Control::Unarchive => (true, ""),
    };
    if ok {
        Ok(())
    } else {
        Err(reason.into())
    }
}

// ---------------------------------------------------------------------------
// The "Active" switch (state-toggles contract §4)
// ---------------------------------------------------------------------------
//
// An automation's schedule is a persistent on/off state, so it is ONE switch
// labelled by the feature ("Active": on = runs on its schedule, off =
// paused), never a Pause/Resume verb pair. Switching sends automation.pause
// or automation.resume (the kit's `activeToggleCommand`); it is unavailable,
// with the reason, once the automation ended, is archived or legacy, while a
// command is in flight, or without the transition's capability.

/// The switch's feature name, from the kit's shared spec (`labels.active`).
pub fn active_label() -> &'static str {
    spec_str(&["labels", "active"])
}

/// The command the Active switch sends from this state: pause when active,
/// resume otherwise (the kit's `activeToggleCommand`).
pub fn active_command(s: &Summary) -> Control {
    if s.status == "active" {
        Control::Pause
    } else {
        Control::Resume
    }
}

/// `Ok(on)` when the Active switch can change now, else `Err(reason)`.
pub fn active_switch(s: &Summary, busy: bool) -> Result<bool, String> {
    if s.legacy {
        return Err("Legacy schedule: managed with its existing controls.".into());
    }
    if s.status == "archived" {
        return Err("Archived: history is kept, nothing runs.".into());
    }
    if s.status != "active" && s.status != "paused" {
        return Err("The automation has ended.".into());
    }
    if busy {
        return Err("Working…".into());
    }
    let cap = active_command(s).capability();
    if !s.capabilities.iter().any(|c| c == cap) {
        return Err("Not permitted for this automation.".into());
    }
    Ok(s.status == "active")
}

/// What the Active switch row says after the feature name.
pub fn active_detail(s: &Summary, busy: bool) -> String {
    match active_switch(s, busy) {
        Ok(true) => "runs on its schedule".into(),
        Ok(false) => "paused: scheduled runs are skipped (Run now still works)".into(),
        Err(why) => why,
    }
}

// ---------------------------------------------------------------------------
// Occurrences as chat pairs
// ---------------------------------------------------------------------------

/// `quiet` | `notified` | `failed` | `waiting` | `running`.
pub fn occurrence_tone(o: &Occurrence) -> &'static str {
    if o.status == "failed" {
        "failed"
    } else if !o.waits.is_empty() || o.status == "waiting" {
        "waiting"
    } else if o.notify.is_some() {
        "notified"
    } else if o.status == "running" || o.status == "backoff" {
        "running"
    } else {
        "quiet"
    }
}

/// The badge beside a run ("Failed after 3 attempts", "Waiting for you", …); "" when quiet.
pub fn occurrence_badge(o: &Occurrence) -> String {
    let attempts = format!(
        "{} {}",
        o.attempts,
        if o.attempts == 1 {
            "attempt"
        } else {
            "attempts"
        }
    );
    match occurrence_tone(o) {
        "failed" => format!("Failed after {attempts}"),
        "waiting" => "Waiting for you".into(),
        "notified" => "Notified".into(),
        "running" => "Running".into(),
        _ => String::new(),
    }
}

/// Discuss is offered once the run has finished.
pub fn can_discuss(o: &Occurrence) -> bool {
    !matches!(o.status.as_str(), "running" | "waiting" | "backoff")
}

/// Oldest first, merged by run id with rows already loaded (newest page wins).
pub fn merge_occurrences(loaded: &[Occurrence], page: &[Occurrence]) -> Vec<Occurrence> {
    let mut out: Vec<Occurrence> = loaded
        .iter()
        .filter(|o| !page.iter().any(|p| p.run_id == o.run_id))
        .cloned()
        .collect();
    out.extend(page.iter().cloned());
    out.sort_by_key(|o| o.index);
    out
}

/// The run's header line: "#5 · failed · Failed after 3 attempts · schedule: … · 2026-… UTC".
pub fn occurrence_header(o: &Occurrence) -> String {
    let status = if o.status == "completed" && o.attempts > 1 {
        format!("completed after {} attempts", o.attempts)
    } else {
        o.status.clone()
    };
    let mut parts = vec![format!("#{}", o.index), status];
    let badge = occurrence_badge(o);
    if !badge.is_empty() && occurrence_tone(o) != "running" {
        parts.push(badge);
    }
    if !o.trigger_summary.is_empty() {
        parts.push(o.trigger_summary.clone());
    }
    if !o.fired_at.is_empty() {
        parts.push(format_utc(&o.fired_at));
    }
    parts.join(" · ")
}

// ---------------------------------------------------------------------------
// Attention
// ---------------------------------------------------------------------------

/// The cursor to acknowledge after SHOWING `attention.items`: the last displayed
/// item's, never `attention.cursor` (items beyond those shown stay unseen).
pub fn attention_ack_cursor(s: &Summary) -> Option<String> {
    s.attention.items.last().map(|i| i.cursor.clone())
}

pub fn wait_kind_label(kind: &str) -> &'static str {
    match kind {
        "ask_user" => "Question for you",
        "tool_approval" => "Approval needed",
        "event" => "Waiting for an event",
        _ => "Waiting",
    }
}

/// The tool calls of a `tool_approval` wait as "name(args)" lines.
pub fn wait_tool_calls(w: &Wait) -> Vec<String> {
    if w.kind != "tool_approval" {
        return Vec::new();
    }
    w.details
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .map(|c| {
                    let name = c.get("name").and_then(Value::as_str).unwrap_or("?");
                    let args = c.get("arguments").cloned().unwrap_or(Value::Null);
                    format!("{name}({args})")
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Decision D1: the resume payload follows the wait's `kind`, never its text.
/// `answer` is the typed answer: "approve"/"deny" for `tool_approval`, the
/// reply for `ask_user`, JSON text for `event`.
pub fn wait_answer_payload(kind: &str, answer: &str) -> Result<Value, String> {
    match kind {
        "tool_approval" => match answer {
            "approve" => Ok(json!({"approved": true})),
            "deny" => Ok(json!({"approved": false})),
            other => Err(format!(
                "a tool approval is answered approve or deny, not {other:?}"
            )),
        },
        "ask_user" => Ok(json!({"response": answer})),
        "event" => serde_json::from_str::<Value>(answer)
            .map(|payload| json!({"payload": payload}))
            .map_err(|e| format!("the event payload is not valid JSON: {e}")),
        other => Err(format!(
            "this wait has no known kind ({other:?}); open its run to answer it"
        )),
    }
}

/// The resume command body for `POST /api/gateway/commands`.
pub fn resume_command(command_id: &str, w: &Wait, payload: Value) -> Value {
    json!({
        "command_id": command_id,
        "run_id": w.run_id,
        "type": "resume",
        "payload": {"wait_key": w.wait_key, "payload": payload},
        "client_id": "abstractcode_tui",
    })
}

// ---------------------------------------------------------------------------
// Create / revise
// ---------------------------------------------------------------------------

/// Clients write `schedule@2` (round 16, "R16.1 API — FINAL"); `schedule@1`
/// rows keep reading as before.
pub const SCHEDULE_VERSION: u64 = 2;

/// The one sentence shown IN PLACE of a round-16 feature when the connected
/// gateway does not serve it (its route answered 404/405: AbstractGateway
/// 0.13.x). Shown once where the feature would be — never the route's raw
/// error, never repeated as a toast.
pub const NEEDS_NEWER_GATEWAY: &str = "Not available on this gateway (needs AbstractGateway 0.14).";

/// The mark on the calendar kinds (Daily / Weekly / Monthly) on such a gateway.
pub const NEEDS_014_MARK: &str = "needs 0.14";

/// Once at… on such a gateway: `schedule@1` reads the time as UTC.
pub const ONCE_UTC_LEGACY: &str =
    "This gateway reads this time as UTC (AbstractGateway 0.14 uses your account's time zone).";

/// Whether the gateway serves the round-16 schedule API (`POST
/// /automations/schedule-preview` and `schedule@2` triggers; they shipped
/// together in AbstractGateway 0.14). Probed once per session: the first
/// preview call answers it, and nothing is asked again once `Missing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScheduleApi {
    #[default]
    Unknown,
    Served,
    Missing,
}

/// A refusal that means "this gateway has no such route" (404 Not Found or
/// 405 Method Not Allowed — what 0.13.x answers for a round-16 route), as
/// opposed to the route refusing the request.
pub fn is_missing_route(e: &ApiError) -> bool {
    matches!(e.status, Some(404 | 405))
}

/// The `schedule@1` form of a `schedule@2` trigger, for a gateway without
/// the round-16 schedule API (`ScheduleApi::Missing`): Repeat keeps its
/// interval and limits (UTC, as `schedule@1` always was); Once at… becomes
/// its `start_at`, read as UTC; a calendar rule (Daily / Weekly / Monthly)
/// has no `schedule@1` form → [`NEEDS_NEWER_GATEWAY`]. Any other trigger
/// (email, an existing `schedule@1`) is returned unchanged.
pub fn legacy_trigger(trigger: &Value) -> Result<Value, String> {
    let is_v2 = trigger.get("source_id").and_then(Value::as_str) == Some("schedule")
        && trigger.get("source_version").and_then(Value::as_u64) == Some(SCHEDULE_VERSION);
    if !is_v2 {
        return Ok(trigger.clone());
    }
    let config = trigger
        .get("config")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut out = Map::new();
    match config.get("kind").and_then(Value::as_str) {
        Some("every") => {
            for key in ["every", "start_at", "count", "until"] {
                if let Some(v) = config.get(key).filter(|v| !v.is_null()) {
                    out.insert(key.into(), v.clone());
                }
            }
        }
        Some("once") => {
            let at = config
                .get("at")
                .and_then(Value::as_str)
                .and_then(utc_from_input)
                .ok_or_else(|| schedule_text("error_once").to_string())?;
            out.insert("start_at".into(), json!(at));
        }
        _ => return Err(NEEDS_NEWER_GATEWAY.into()),
    }
    Ok(json!({"source_id": "schedule", "source_version": 1, "config": out}))
}

/// A create body for a gateway without the round-16 schedule API: its
/// trigger in the `schedule@1` form ([`legacy_trigger`]), or the sentence.
pub fn legacy_create_body(body: &Value) -> Result<Value, String> {
    let mut out = body.clone();
    if let Some(t) = body.get("trigger") {
        out["trigger"] = legacy_trigger(t)?;
    }
    Ok(out)
}

/// Whether a trigger needs the round-16 schedule API (`schedule@2`).
pub fn needs_schedule_api(trigger: &Value) -> bool {
    trigger.get("source_id").and_then(Value::as_str) == Some("schedule")
        && trigger.get("source_version").and_then(Value::as_u64) == Some(SCHEDULE_VERSION)
}

/// The weekdays of a weekly rule, Monday first (the wire's order).
pub const CALENDAR_DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// One string of the vendored `schedule` wording block (the kit's
/// `SCHEDULE_TEXT`): "Daily", "Once at…", "in {time_zone} (your account's
/// time zone)", …
pub fn schedule_text(key: &str) -> &'static str {
    spec_str(&["schedule", key])
}

/// A weekday chip's label ("Mon") from the vendored wording.
pub fn day_label(day: &str) -> &'static str {
    spec_str(&["schedule", "days", day])
}

/// "in Europe/Paris (your account's time zone)".
pub fn time_zone_line(time_zone: &str) -> String {
    schedule_text("time_zone_line").replace("{time_zone}", time_zone)
}

/// "in Europe/Paris (this automation's time zone)" (an existing automation).
pub fn time_zone_line_automation(time_zone: &str) -> String {
    schedule_text("time_zone_line_automation").replace("{time_zone}", time_zone)
}

/// The calendar rule of a stored `schedule@2` config (Daily / Weekly /
/// Monthly), as the form's `When`; `None` for any other config.
pub fn calendar_when_from(config: &Map<String, Value>) -> Option<When> {
    let at = config
        .get("at")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match config.get("kind").and_then(Value::as_str)? {
        "daily" => Some(When::Daily { at }),
        "weekly" => Some(When::Weekly {
            days: config
                .get("days")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            at,
        }),
        "monthly" => Some(When::Monthly {
            day: match config.get("day") {
                Some(Value::String(d)) => d.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => String::new(),
            },
            at,
        }),
        _ => None,
    }
}

/// What a person picked for a calendar rule, kept across kind switches
/// (the kit's `CalendarRuleState`): the time, the weekly days and the
/// monthly day survive Weekly → Monthly → Weekly. An emptied day set stays
/// empty (it is refused when saved, never silently refilled).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CalendarRuleState {
    pub at: String,
    pub days: Option<Vec<String>>,
    pub day: Option<String>,
}

impl CalendarRuleState {
    /// Take what `when` says (its time, and its days or its day).
    pub fn absorb(&mut self, when: &When) {
        match when {
            When::Daily { at } => self.at = at.clone(),
            When::Weekly { days, at } => {
                self.at = at.clone();
                self.days = Some(days.clone());
            }
            When::Monthly { day, at } => {
                self.at = at.clone();
                self.day = Some(day.clone());
            }
            _ => {}
        }
    }
    /// The rule of `kind` ("daily" | "weekly" | "monthly") from this state;
    /// never-picked fields take the defaults (08:00, Monday, day 1).
    pub fn rule(&self, kind: &str) -> When {
        let at = if self.at.is_empty() {
            "08:00".to_string()
        } else {
            self.at.clone()
        };
        match kind {
            "weekly" => When::Weekly {
                days: self.days.clone().unwrap_or_else(|| vec!["mon".into()]),
                at,
            },
            "monthly" => When::Monthly {
                day: self.day.clone().unwrap_or_else(|| "1".into()),
                at,
            },
            _ => When::Daily { at },
        }
    }
}

/// The rule of `kind` keeping what `previous` says (the kit's
/// `calendarWhenOf`); use a `CalendarRuleState` to keep it across switches.
pub fn calendar_when_of(kind: &str, previous: &When) -> When {
    let mut state = CalendarRuleState::default();
    state.absorb(previous);
    state.rule(kind)
}

/// The trigger an Edit of a calendar rule writes: the rule's config plus
/// the binding's own `time_zone`, `count` and `until` (kept; the form never
/// edits them; `start_at` is not carried — the gateway re-anchors) — the
/// kit's `reviseChanges`. Also the trigger the Edit panel previews.
pub fn revise_calendar_trigger(s: &Summary, when: &When) -> Result<Value, String> {
    let mut config = schedule_config(when)?;
    if let Some(zone) = s.trigger.config.get("time_zone").and_then(Value::as_str) {
        config.insert("time_zone".into(), json!(zone));
    }
    for key in ["count", "until"] {
        if let Some(v) = s.trigger.config.get(key).filter(|v| !v.is_null()) {
            config.insert(key.into(), v.clone());
        }
    }
    Ok(
        json!({"source_id": s.trigger.source_id, "source_version": s.trigger.source_version, "config": config}),
    )
}

/// When the automation runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    /// Repeat: every `amount` `unit` (m|h|d), a fixed UTC interval, from now
    /// or from `start_at`.
    Every { amount: String, unit: char },
    /// Daily at `HH:MM` in the account's time zone.
    Daily { at: String },
    /// Weekly on `days` (wire names, any order) at `HH:MM`.
    Weekly { days: Vec<String>, at: String },
    /// Monthly on `day` ("1".."31" or "last") at `HH:MM`.
    Monthly { day: String, at: String },
    /// Once at a wall time `YYYY-MM-DD HH:MM` in the account's time zone.
    Once { at: String },
    /// "When an email arrives" (`email.received@1`, from `CreateForm::email`);
    /// offered only while the account's email is usable.
    Email,
}

impl When {
    /// Every kind's line is the gateway's `first_run_sentence`
    /// (schedule-preview), never one composed here.
    pub fn is_served(&self) -> bool {
        true
    }
    /// Once / Daily / Weekly / Monthly run on the account's time zone (the
    /// line names it); Repeat is a fixed UTC interval (no zone line).
    pub fn uses_time_zone(&self) -> bool {
        !matches!(self, When::Every { .. })
    }
    /// Daily / Weekly / Monthly (a wall-clock rule).
    pub fn is_calendar(&self) -> bool {
        matches!(
            self,
            When::Daily { .. } | When::Weekly { .. } | When::Monthly { .. }
        )
    }
}

/// `HH:MM`, 00:00–23:59 (a fixed format, read structurally).
pub fn is_wall_time(at: &str) -> bool {
    let b = at.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && [0, 1, 3, 4].iter().all(|&i| b[i].is_ascii_digit())
        && (b[0] - b'0') * 10 + (b[1] - b'0') < 24
        && b[3] < b'6'
}

/// `YYYY-MM-DD HH:MM` (or with `T`) → the wire's `YYYY-MM-DDTHH:MM`, or None.
pub fn wall_datetime(value: &str) -> Option<String> {
    let v = value.trim();
    let b = v.as_bytes();
    if b.len() != 16 || !matches!(b[10], b'T' | b' ') || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    if ![0, 1, 2, 3, 5, 6, 8, 9]
        .iter()
        .all(|&i| b[i].is_ascii_digit())
    {
        return None;
    }
    let (m, d) = (&v[5..7], &v[8..10]);
    if !("01"..="12").contains(&m) || !("01"..="31").contains(&d) || !is_wall_time(&v[11..16]) {
        return None;
    }
    Some(format!("{}T{}", &v[..10], &v[11..16]))
}

/// The kit's default growing-context budget (`DEFAULT_GROWING_MAX_TOKENS`).
pub const DEFAULT_GROWING_MAX_TOKENS: u64 = 50_000;
/// The kit's `GROWING_CONTEXT_HELP`, verbatim.
pub const GROWING_CONTEXT_HELP: &str = "Limits history carried into the next run, keeping recent whole turns. The newest turn is kept even if oversized. New messages and tool results can grow context beyond this budget.";
/// The kit's label of the growing budget field.
pub const GROWING_MAX_TOKENS_LABEL: &str = "Max growing context (tokens)";
const GROWING_MAX_TOKENS_ERROR: &str =
    "Max growing context must be a positive whole number of tokens.";

/// The create form (raw strings, as typed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateForm {
    pub prompt: String,
    pub when: When,
    /// The "When an email arrives" fields (used when `when` is `Email`).
    pub email: crate::automation_email::EmailTriggerForm,
    /// `independent` | `growing`.
    pub context: String,
    /// "Max growing context (tokens)" (growing only; default 50000).
    pub growing_max_tokens: String,
    /// `auto` | `ask`.
    pub tool_approval: String,
    /// The Tools section's selection: `None` = "Use workflow default tools";
    /// `Some(list)` = exactly these (`[]` disables tools).
    pub tools: Option<Vec<String>>,
    /// The Mailbox section: "Email result" and its recipients.
    pub notify_email: bool,
    pub recipients: crate::automation_email::RecipientsForm,
    pub title: String,
    /// "Title and limits" (the kit dialog's fields): first run at (UTC;
    /// empty = now; Repeat only), stop after this many runs and stop at (UTC)
    /// (Repeat and the calendar rules). Typed as `YYYY-MM-DD HH:MM` / a
    /// whole number.
    pub start_at: String,
    pub count: String,
    pub until: String,
    /// The dialog's Workspaces section (R13.2 / R14.4): the run-level
    /// payload stored as `target.input_data.workspace`; `None` = "Use my
    /// default" (nothing sent).
    pub workspace: Option<crate::workspaces::RunValue>,
}

impl Default for CreateForm {
    fn default() -> Self {
        CreateForm {
            prompt: String::new(),
            when: When::Every {
                amount: "24".into(),
                unit: 'h',
            },
            email: crate::automation_email::EmailTriggerForm::default(),
            context: "independent".into(),
            growing_max_tokens: DEFAULT_GROWING_MAX_TOKENS.to_string(),
            tool_approval: "auto".into(),
            tools: None,
            notify_email: false,
            recipients: crate::automation_email::RecipientsForm::default(),
            title: String::new(),
            start_at: String::new(),
            count: String::new(),
            until: String::new(),
            workspace: None,
        }
    }
}

/// The create target for the TUI's workflow selection: the gateway default as
/// `{flow_id: "@default", interface}`, else `{bundle_ref, flow_id}`.
pub fn target_for(workflow: &crate::store::Workflow) -> Option<Value> {
    if workflow.gateway_default {
        return Some(json!({"flow_id": "@default", "interface": CODE_AGENT_INTERFACE}));
    }
    let bundle = workflow.bundle_id.trim();
    let flow = workflow.flow_id.trim();
    if bundle.is_empty() || flow.is_empty() {
        return None;
    }
    let version = workflow.version.trim();
    let bundle_ref = if version.is_empty() {
        bundle.to_string()
    } else {
        format!("{bundle}@{version}")
    };
    Some(json!({"bundle_ref": bundle_ref, "flow_id": flow}))
}

/// `YYYY-MM-DD HH:MM[:SS]` (or with `T`) read as UTC → RFC3339.
pub fn utc_from_input(value: &str) -> Option<String> {
    let v = value.trim();
    let b = v.as_bytes();
    if !(b.len() == 16 || b.len() == 19) || !matches!(b[10], b'T' | b' ') {
        return None;
    }
    let ts = format!(
        "{}T{}{}Z",
        &v[..10],
        &v[11..16],
        if b.len() == 19 { &v[16..] } else { ":00" }
    );
    unix_secs(&ts).map(|_| ts)
}

/// The prompt's first line, at most 120 characters.
pub fn default_title(prompt: &str) -> String {
    let first = prompt
        .trim()
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if first.chars().count() > 120 {
        let mut t: String = first.chars().take(119).collect();
        t.push('…');
        t
    } else {
        first
    }
}

/// The `schedule@2` trigger of a form, or the reasons it is incomplete
/// (the kit's `scheduleTriggerFrom`).
pub fn schedule_trigger(form: &CreateForm) -> Result<Value, Vec<String>> {
    let config = schedule_config_form(form)?;
    Ok(json!({"source_id": "schedule", "source_version": SCHEDULE_VERSION, "config": config}))
}

/// The kit's own line under When, for the email trigger only ("Runs when an
/// email arrives · … ."); every schedule kind (Repeat with its bounds,
/// Daily, Weekly, Monthly, Once) returns "": its line is the gateway's
/// `first_run_sentence` (schedule-preview), never one composed here (the
/// kit's `schedulePreview`).
pub fn schedule_preview(form: &CreateForm) -> String {
    if matches!(form.when, When::Email) {
        let (config, errors) = crate::automation_email::email_trigger_config_from(&form.email);
        if errors.is_empty() {
            return format!(
                "Runs {}.",
                crate::automation_email::email_trigger_label(&config)
            );
        }
    }
    String::new()
}

/// The kit's line when the preview is empty.
pub fn incomplete_line(form: &CreateForm) -> &'static str {
    if matches!(form.when, When::Email) {
        "Incomplete email trigger."
    } else {
        schedule_text("incomplete")
    }
}

/// The schedule config with the dialog's limits (the kit's
/// `scheduleConfigFrom`): start_at for Repeat; count / until for Repeat and
/// the calendar rules; Once carries none. No `time_zone`: the gateway fills
/// the owner's account zone. The kit's sentences, one per problem.
pub fn schedule_config_form(form: &CreateForm) -> Result<Map<String, Value>, Vec<String>> {
    let mut errors: Vec<String> = Vec::new();
    let mut config = match schedule_config(&form.when) {
        Ok(c) => c,
        Err(e) => {
            if !matches!(
                form.when,
                When::Every { .. }
                    | When::Daily { .. }
                    | When::Weekly { .. }
                    | When::Monthly { .. }
            ) {
                return Err(vec![e]);
            }
            errors.push(e);
            Map::new()
        }
    };
    if matches!(form.when, When::Once { .. }) {
        return Ok(config);
    }
    if !form.start_at.trim().is_empty() && matches!(form.when, When::Every { .. }) {
        match utc_from_input(&form.start_at) {
            Some(ts) => {
                config.insert("start_at".into(), json!(ts));
            }
            None => errors.push("First run must be a date and time (UTC).".into()),
        }
    }
    if !form.count.trim().is_empty() {
        match form.count.trim().parse::<u64>() {
            Ok(n) if n >= 1 => {
                config.insert("count".into(), json!(n));
            }
            _ => errors.push("Maximum runs must be a whole number of at least 1.".into()),
        }
    }
    if !form.until.trim().is_empty() {
        match utc_from_input(&form.until) {
            Some(ts) => {
                config.insert("until".into(), json!(ts));
            }
            None => errors.push("Stop at must be a date and time (UTC).".into()),
        }
    }
    if errors.is_empty() {
        Ok(config)
    } else {
        Err(errors)
    }
}

fn schedule_config(when: &When) -> Result<Map<String, Value>, String> {
    let mut config = Map::new();
    let wall = |at: &str| -> Result<String, String> {
        let at = at.trim();
        if is_wall_time(at) {
            Ok(at.to_string())
        } else {
            Err(schedule_text("error_at").into())
        }
    };
    match when {
        When::Once { at } => match wall_datetime(at) {
            Some(at) => {
                config.insert("kind".into(), json!("once"));
                config.insert("at".into(), json!(at));
            }
            None => return Err(schedule_text("error_once").into()),
        },
        When::Every { amount, unit } => {
            let n = amount.trim();
            if !matches!(unit, 'm' | 'h' | 'd') || !is_duration(&format!("{n}{unit}")) {
                return Err("The interval must be a whole number of at least 1.".into());
            }
            config.insert("kind".into(), json!("every"));
            config.insert("every".into(), json!(format!("{n}{unit}")));
        }
        When::Daily { at } => {
            let at = wall(at)?;
            config.insert("kind".into(), json!("daily"));
            config.insert("at".into(), json!(at));
        }
        When::Weekly { days, at } => {
            // Monday-first, de-duplicated (the gateway normalizes the same way).
            let picked: Vec<&str> = CALENDAR_DAYS
                .iter()
                .copied()
                .filter(|d| days.iter().any(|x| x == d))
                .collect();
            if picked.is_empty() {
                return Err(schedule_text("error_days").into());
            }
            let at = wall(at)?;
            config.insert("kind".into(), json!("weekly"));
            config.insert("days".into(), json!(picked));
            config.insert("at".into(), json!(at));
        }
        When::Monthly { day, at } => {
            let day = day.trim();
            let value = if day == "last" {
                json!("last")
            } else {
                match day.parse::<u64>() {
                    Ok(n) if (1..=31).contains(&n) && !day.starts_with('0') => json!(n),
                    _ => return Err(schedule_text("error_day").into()),
                }
            };
            let at = wall(at)?;
            config.insert("kind".into(), json!("monthly"));
            config.insert("day".into(), value);
            config.insert("at".into(), json!(at));
        }
        When::Email => return Err("Not a schedule.".into()),
    }
    Ok(config)
}

/// `{mode}` + `growing.max_tokens` only when it differs from the default
/// (the kit's `automationContext`).
pub fn automation_context(mode: &str, max_tokens: u64) -> Value {
    if max_tokens != DEFAULT_GROWING_MAX_TOKENS {
        json!({"mode": mode, "growing": {"max_tokens": max_tokens}})
    } else {
        json!({"mode": mode})
    }
}

/// A growing automation's budget as its definition states it (the kit's
/// `definition.context.growing.max_tokens`, else the default).
pub fn growing_max_tokens_of(_summary: &Summary, def: &Definition) -> u64 {
    def.growing
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_GROWING_MAX_TOKENS)
}

/// The revision a new "Max growing context (tokens)" value makes (the
/// kit's `reviseChanges`: `changes.context = automationContext(mode,
/// maxTokens)` when it changed): `Ok(None)` = unchanged, `Err` = the kit's
/// sentence.
pub fn growing_budget_changes(
    summary: &Summary,
    def: &Definition,
    typed: &str,
) -> Result<Option<Value>, String> {
    let n = match typed.trim().parse::<u64>() {
        Ok(n) if n > 0 && n <= 9_007_199_254_740_991 => n,
        _ => return Err(GROWING_MAX_TOKENS_ERROR.into()),
    };
    if n == growing_max_tokens_of(summary, def) {
        return Ok(None);
    }
    Ok(Some(json!({"context": automation_context("growing", n)})))
}

/// `POST /api/gateway/automations/schedule-preview` (nothing stored): the
/// normalized trigger, the zone, the gateway's words and the first run.
#[derive(Debug, Clone, PartialEq)]
pub struct SchedulePreview {
    pub time_zone: String,
    pub schedule_rule_text: String,
    pub first_run_sentence: String,
    pub next_run_at: Option<String>,
    pub next_run_local: Option<String>,
}

pub fn parse_schedule_preview(v: &Value) -> Parse<SchedulePreview> {
    let what = "schedule preview";
    Ok(SchedulePreview {
        time_zone: req_str(v, "time_zone", what)?,
        schedule_rule_text: req_str(v, "schedule_rule_text", what)?,
        first_run_sentence: req_str(v, "first_run_sentence", what)?,
        next_run_at: opt_str(v, "next_run_at").filter(|s| !s.is_empty()),
        next_run_local: opt_str(v, "next_run_local").filter(|s| !s.is_empty()),
    })
}

/// Where the When step's served line stands, keyed by the trigger it describes.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewState {
    Loading,
    Ready(SchedulePreview),
    Failed(String),
    /// The gateway has no schedule-preview route (AbstractGateway 0.13.x):
    /// the line is [`NEEDS_NEWER_GATEWAY`], never the route's error, and
    /// it does not stop Continue.
    Unavailable,
    /// No gateway answer (transport failure): [`UNREACHED_LINE`], never the
    /// raw transport text, and it does not stop Continue (only a refusal
    /// from the route itself does).
    Unreached,
}

/// The line when the preview got no gateway answer.
pub const UNREACHED_LINE: &str = "The gateway could not be reached.";

/// The served lines of a preview: the time-zone line (when ready and
/// `with_zone` — not for Repeat, a fixed UTC interval) and the sentence —
/// `first_run_sentence`, "Checking the schedule…", or the gateway's refusal.
pub fn preview_lines(state: &PreviewState, with_zone: bool) -> Vec<String> {
    match state {
        PreviewState::Loading => vec![schedule_text("describing").to_string()],
        PreviewState::Ready(p) if with_zone => {
            vec![time_zone_line(&p.time_zone), p.first_run_sentence.clone()]
        }
        PreviewState::Ready(p) => vec![p.first_run_sentence.clone()],
        PreviewState::Failed(e) => vec![e.clone()],
        PreviewState::Unavailable => vec![NEEDS_NEWER_GATEWAY.to_string()],
        PreviewState::Unreached => vec![UNREACHED_LINE.to_string()],
    }
}

/// The exact `POST /api/gateway/automations` body as the kit's
/// `buildCreateRequest` (+ the dialog's tool selection) shapes it, or the
/// reasons it cannot be built — in the kit's order and words. `email_usable`
/// = `GET /me/email` says the account can be used now: without it nothing
/// email-shaped is sent (an email choice falls back to Repeat, like the
/// kit's `shownKind`). The target's `input_data` gets the task (`prompt`);
/// the host replaces it with the workflow's real inputs
/// ([`schedule_body`]).
pub fn build_create_request(
    form: &CreateForm,
    target: Option<Value>,
    email_usable: bool,
    request_id: &str,
) -> Result<Value, Vec<String>> {
    let mut errors = Vec::new();
    if target.is_none() {
        errors.push("Choose what to run.".to_string());
    }
    let prompt = form.prompt.trim().to_string();
    if prompt.is_empty() {
        errors.push("Write the task to run.".into());
    }
    let title = if form.title.trim().is_empty() {
        default_title(&prompt)
    } else {
        form.title.trim().to_string()
    };
    if title.chars().count() > 120 {
        errors.push("Title is at most 120 characters.".into());
    }
    let email = matches!(form.when, When::Email) && email_usable;
    // An email choice made while the account was usable falls back to Repeat
    // (with the default interval) when it stops being usable.
    let fallback;
    let shown: &CreateForm = if matches!(form.when, When::Email) && !email_usable {
        fallback = CreateForm {
            when: When::Every {
                amount: "24".into(),
                unit: 'h',
            },
            ..form.clone()
        };
        &fallback
    } else {
        form
    };
    let mut config = Map::new();
    if email {
        let (c, e) = crate::automation_email::email_trigger_config_from(&form.email);
        config = c;
        errors.extend(e);
    } else {
        match schedule_config_form(shown) {
            Ok(c) => config = c,
            Err(e) => errors.extend(e),
        }
    }
    let recipients = if email_usable && form.notify_email && form.recipients.list {
        let (list, e) = crate::automation_email::allowed_recipients_from(&form.recipients);
        errors.extend(e);
        Some(list)
    } else {
        None
    };
    let growing = form.context == "growing";
    let max_tokens = if growing {
        match form.growing_max_tokens.trim().parse::<u64>() {
            Ok(n) if n > 0 && n <= 9_007_199_254_740_991 => n,
            _ => {
                errors.push(GROWING_MAX_TOKENS_ERROR.into());
                0
            }
        }
    } else {
        DEFAULT_GROWING_MAX_TOKENS
    };
    if form.context != "independent" && !growing {
        errors.push("Context must be Independent or Growing.".into());
    }
    if form.tool_approval != "auto" && form.tool_approval != "ask" {
        errors.push("Tools must run without asking or ask each time.".into());
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut target = target.expect("checked above");
    let input = target
        .as_object_mut()
        .expect("targets are objects")
        .entry("input_data")
        .or_insert_with(|| json!({}));
    input
        .as_object_mut()
        .expect("input_data is an object")
        .insert("prompt".into(), json!(prompt));
    let input = crate::schedule_input::with_automation_tools(input, form.tools.as_deref());
    target["input_data"] = input;
    let trigger = if email {
        json!({"source_id": crate::automation_email::SOURCE_ID,
               "source_version": crate::automation_email::SOURCE_VERSION, "config": config})
    } else {
        json!({"source_id": "schedule", "source_version": SCHEDULE_VERSION, "config": config})
    };
    let mut body = json!({
        "request_id": request_id,
        "title": title,
        "target": target,
        "trigger": trigger,
        "context": automation_context(&form.context, max_tokens),
        "policy": {"tool_approval": form.tool_approval},
    });
    if email_usable && form.notify_email {
        let list = recipients.unwrap_or_else(|| vec!["self".to_string()]);
        body["notify"] = crate::automation_email::notify_for(true, &list);
    }
    Ok(body)
}

/// The whole create body as the Code web sends it: the kit's body
/// ([`build_create_request`]) whose `target.input_data` is replaced by the
/// workflow's real inputs (`built`, from
/// [`crate::schedule_input::automation_input`]) with the dialog's tool
/// selection and the Workspaces section's value applied
/// ([`crate::schedule_input::finish_input`]).
pub fn schedule_body(
    form: &CreateForm,
    target: Option<Value>,
    email_usable: bool,
    built: &Value,
    request_id: &str,
) -> Result<Value, Vec<String>> {
    let mut body = build_create_request(form, target, email_usable, request_id)?;
    body["target"]["input_data"] =
        crate::schedule_input::finish_input(built, form.tools.as_deref(), form.workspace.as_ref());
    Ok(body)
}

/// The Edit form (the definition panel): title, interval, context, the
/// limits (repeating schedules) and the Mailbox options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviseForm {
    pub title: String,
    /// `None` for a trigger without an interval.
    pub every: Option<String>,
    /// The calendar rule of a `schedule@2` Daily / Weekly / Monthly
    /// automation (`None` for any other trigger).
    pub calendar: Option<When>,
    pub context: String,
    /// "Stop after this many runs" ("" = no limit); `None` when the trigger
    /// is not a repeating schedule.
    pub count: Option<String>,
    /// "Stop at (UTC)" (`YYYY-MM-DD HH:MM`, "" = no end); `None` when the
    /// trigger is not a repeating schedule.
    pub until: Option<String>,
    /// "Email result"; `None` without a definition.
    pub notify_email: Option<bool>,
    pub recipients: Option<crate::automation_email::RecipientsForm>,
}

/// `2026-10-31T18:00:00+00:00` → `2026-10-31 18:00` (the typed form).
fn typed_utc(ts: &str) -> String {
    let t = ts.trim();
    if t.len() >= 16 && t.as_bytes()[10] == b'T' {
        format!("{} {}", &t[..10], &t[11..16])
    } else {
        t.to_string()
    }
}

/// The form of a summary (+ its definition's `notify` when known).
pub fn revise_form_from(s: &Summary) -> ReviseForm {
    revise_form_with(s, None)
}

pub fn revise_form_with(s: &Summary, def: Option<&Definition>) -> ReviseForm {
    let email =
        crate::automation_email::is_email_trigger(&s.trigger.source_id, s.trigger.source_version);
    let every = if s.trigger.source_id == "schedule" || email {
        s.trigger
            .config
            .get("every")
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        None
    };
    let calendar = if is_schedule_v2(&s.trigger) {
        calendar_when_from(&s.trigger.config)
    } else {
        None
    };
    let repeating = s.trigger.source_id == "schedule" && every.is_some();
    let config_text = |key: &str| -> String {
        match s.trigger.config.get(key) {
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(v)) => typed_utc(v),
            _ => String::new(),
        }
    };
    ReviseForm {
        title: s.title.clone(),
        every,
        calendar,
        context: s.context_mode.clone(),
        count: repeating.then(|| config_text("count")),
        until: repeating.then(|| config_text("until")),
        notify_email: def.map(|d| crate::automation_email::notify_emails(&d.notify)),
        recipients: def.map(|d| {
            crate::automation_email::recipients_form_from(
                &crate::automation_email::notify_recipients(&d.notify),
            )
        }),
    }
}

/// Only the fields that changed (`Ok(None)` when nothing did) — the kit's
/// `reviseChanges` shapes: a new interval keeps the rest of the trigger
/// config (an email trigger drops its old `start_at` so it never re-reads
/// mail); the limits ride `trigger.config` (`count` / `until`; empty
/// removes the limit); "Email result" and its recipients send `notify`.
pub fn revise_changes(s: &Summary, form: &ReviseForm) -> Result<Option<Value>, Vec<String>> {
    revise_changes_with(s, None, form)
}

pub fn revise_changes_with(
    s: &Summary,
    def: Option<&Definition>,
    form: &ReviseForm,
) -> Result<Option<Value>, Vec<String>> {
    let mut errors = Vec::new();
    let mut changes = Map::new();
    let title = form.title.trim();
    if title.is_empty() {
        errors.push("Title is required.".to_string());
    } else if title.chars().count() > 120 {
        errors.push("Title is at most 120 characters.".into());
    } else if title != s.title {
        changes.insert("title".into(), json!(title));
    }
    let before = revise_form_with(s, def);
    let email =
        crate::automation_email::is_email_trigger(&s.trigger.source_id, s.trigger.source_version);
    let mut config = s.trigger.config.clone();
    let mut trigger_changed = false;
    if let Some(every) = form.every.as_deref().map(str::trim) {
        if Some(every) != before.every.as_deref() {
            if !is_duration(every) {
                errors.push(
                    "Interval must be a whole number of minutes, hours or days (e.g. 30m, 8h, 7d)."
                        .into(),
                );
            } else if email
                && duration_seconds(every).unwrap_or(0) < crate::automation_email::MIN_EVERY_SECONDS
            {
                errors.push("The check interval is at least 60 seconds.".into());
            } else {
                config.insert("every".into(), json!(every));
                if email {
                    config.remove("start_at");
                }
                trigger_changed = true;
            }
        }
    }
    if let (Some(count), Some(prev)) = (
        form.count.as_deref().map(str::trim),
        before.count.as_deref(),
    ) {
        if count != prev {
            if count.is_empty() {
                config.remove("count");
                trigger_changed = true;
            } else {
                match count.parse::<u64>() {
                    Ok(n) if n >= 1 => {
                        config.insert("count".into(), json!(n));
                        trigger_changed = true;
                    }
                    _ => errors.push("Maximum runs must be a whole number of at least 1.".into()),
                }
            }
        }
    }
    if let (Some(until), Some(prev)) = (
        form.until.as_deref().map(str::trim),
        before.until.as_deref(),
    ) {
        if until != prev {
            if until.is_empty() {
                config.remove("until");
                trigger_changed = true;
            } else {
                match utc_from_input(until) {
                    Some(ts) => {
                        config.insert("until".into(), json!(ts));
                        trigger_changed = true;
                    }
                    None => errors.push("Stop at must be a date and time (UTC).".into()),
                }
            }
        }
    }
    if trigger_changed {
        changes.insert(
            "trigger".into(),
            json!({"source_id": s.trigger.source_id, "source_version": s.trigger.source_version, "config": config}),
        );
    }
    // A changed calendar rule: the rule + the binding's own time zone.
    if let (Some(when), Some(prev)) = (&form.calendar, &before.calendar) {
        if when != prev {
            match revise_calendar_trigger(s, when) {
                Ok(trigger) => {
                    changes.insert("trigger".into(), trigger);
                }
                Err(e) => errors.push(e),
            }
        }
    }
    if form.context != s.context_mode {
        if form.context != "independent" && form.context != "growing" {
            errors.push("Context must be Independent or Growing.".into());
        } else {
            changes.insert(
                "context".into(),
                automation_context(
                    &form.context,
                    def.and_then(|d| d.growing.get("max_tokens"))
                        .and_then(Value::as_u64)
                        .unwrap_or(DEFAULT_GROWING_MAX_TOKENS),
                ),
            );
        }
    }
    if def.is_some() {
        if let Some(on) = form.notify_email {
            use crate::automation_email::{allowed_recipients_from, notify_for, RecipientsForm};
            let prev_form = before.recipients.clone().unwrap_or_default();
            let next_form = if on {
                form.recipients.clone().unwrap_or_default()
            } else {
                prev_form.clone()
            };
            let (next, e) = allowed_recipients_from(&next_form);
            let (prev, _) = allowed_recipients_from(&RecipientsForm { ..prev_form });
            if !e.is_empty() {
                errors.extend(e);
            } else if Some(on) != before.notify_email || next != prev {
                changes.insert("notify".into(), notify_for(on, &next));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(if changes.is_empty() {
        None
    } else {
        Some(Value::Object(changes))
    })
}

// ---------------------------------------------------------------------------
// Requests (method, path under the gateway origin, body) — one builder per
// route, so the HTTP lane and the contract tests share them.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub method: &'static str,
    pub path: String,
    pub body: Option<Value>,
}

const BASE: &str = "/api/gateway/automations";

fn enc(id: &str) -> String {
    crate::gateway::url_encode(id)
}

pub fn list_request(cursor: Option<&str>) -> Request {
    let mut path = format!("{BASE}?limit={PAGE_LIMIT}");
    if let Some(c) = cursor {
        path.push_str(&format!("&cursor={}", enc(c)));
    }
    Request {
        method: "GET",
        path,
        body: None,
    }
}

/// `GET /api/gateway/automations?status=archived&limit=…[&cursor=…]` — the
/// archived ones (the web's `Archived · N` list reads the same).
pub fn archived_list_request(cursor: Option<&str>) -> Request {
    let mut path = format!("{BASE}?status=archived&limit={PAGE_LIMIT}");
    if let Some(c) = cursor {
        path.push_str(&format!("&cursor={}", enc(c)));
    }
    Request {
        method: "GET",
        path,
        body: None,
    }
}

pub fn detail_request(id: &str) -> Request {
    Request {
        method: "GET",
        path: format!("{BASE}/{}", enc(id)),
        body: None,
    }
}

pub fn occurrences_request(id: &str, cursor: Option<&str>) -> Request {
    let mut path = format!("{BASE}/{}/occurrences?limit={PAGE_LIMIT}", enc(id));
    if let Some(c) = cursor {
        path.push_str(&format!("&cursor={}", enc(c)));
    }
    Request {
        method: "GET",
        path,
        body: None,
    }
}

/// `POST /api/gateway/automations/schedule-preview {trigger}`.
pub fn preview_request(trigger: Value) -> Request {
    Request {
        method: "POST",
        path: "/api/gateway/automations/schedule-preview".into(),
        body: Some(json!({ "trigger": trigger })),
    }
}

pub fn create_request(body: Value) -> Request {
    Request {
        method: "POST",
        path: BASE.to_string(),
        body: Some(body),
    }
}

pub fn command_request(id: &str, command_id: &str, command_type: &str) -> Request {
    Request {
        method: "POST",
        path: format!("{BASE}/{}/commands", enc(id)),
        body: Some(json!({"command_id": command_id, "type": command_type})),
    }
}

pub fn revise_request(
    id: &str,
    command_id: &str,
    expected_revision: Option<u64>,
    changes: Value,
) -> Request {
    let mut body = json!({"command_id": command_id, "changes": changes});
    if let Some(rev) = expected_revision {
        body["expected_revision"] = json!(rev);
    }
    Request {
        method: "PATCH",
        path: format!("{BASE}/{}", enc(id)),
        body: Some(body),
    }
}

pub fn discuss_request(id: &str, request_id: &str, occurrence_index: u64, prompt: &str) -> Request {
    Request {
        method: "POST",
        path: format!("{BASE}/{}/discuss", enc(id)),
        body: Some(
            json!({"request_id": request_id, "occurrence_index": occurrence_index, "prompt": prompt}),
        ),
    }
}

pub fn seen_request(id: &str, attention_cursor: &str) -> Request {
    Request {
        method: "POST",
        path: format!("{BASE}/{}/seen", enc(id)),
        body: Some(json!({"attention_cursor": attention_cursor})),
    }
}

pub fn wait_answer_request(command_id: &str, w: &Wait, payload: Value) -> Request {
    Request {
        method: "POST",
        path: "/api/gateway/commands".into(),
        body: Some(resume_command(command_id, w, payload)),
    }
}

/// One id per user action, reused only when the SAME action is retried after
/// a transport failure (no gateway answer), so the gateway answers the retry
/// idempotently; after an answer (success or refusal) the next click is new.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionIds {
    pending: Option<(String, String)>,
}

impl ActionIds {
    pub fn id_for(&mut self, action_key: &str, mint: impl FnOnce() -> String) -> String {
        match &self.pending {
            Some((key, id)) if key == action_key => id.clone(),
            _ => {
                let id = mint();
                self.pending = Some((action_key.to_string(), id.clone()));
                id
            }
        }
    }

    /// Record the outcome: a transport failure keeps the id for a retry.
    pub fn settle(&mut self, transport_failure: bool) {
        if !transport_failure {
            self.pending = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Contract G: the gateway's `{"detail": {"reason_code", "message", "field"?}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: Option<u16>,
    pub code: String,
    pub message: String,
    pub field: Option<String>,
}

impl ApiError {
    /// A transport failure (no gateway answer): the same action may retry with the same id.
    pub fn is_transport(&self) -> bool {
        self.status.is_none()
    }
}

/// Parse an HTTP error body into the typed error. A body without the envelope
/// is `invalid_response` — never a silent success.
pub fn parse_api_error(status: u16, body: &str) -> ApiError {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("detail").cloned());
    match detail {
        Some(d) if d.get("reason_code").and_then(Value::as_str).is_some() => ApiError {
            status: Some(status),
            code: d["reason_code"].as_str().unwrap_or_default().to_string(),
            message: d
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            field: d.get("field").and_then(Value::as_str).map(str::to_string),
        },
        Some(Value::String(text)) => ApiError {
            status: Some(status),
            code: "invalid_response".into(),
            message: text,
            field: None,
        },
        _ => ApiError {
            status: Some(status),
            code: "invalid_response".into(),
            message: body.trim().to_string(),
            field: None,
        },
    }
}

/// One visible sentence per error code, plus the server's own message.
pub fn api_error_text(e: &ApiError) -> String {
    // A route this gateway lacks (a round-16 feature on 0.13.x): the one
    // sentence, alone — decided by STATUS: 405 (the path exists for another
    // method only), or 404 with the router's own "Not Found" (no such path;
    // a route's own 404 — automation_not_found, a missing run — keeps its words).
    let no_route = e.status == Some(405)
        || (e.status == Some(404) && e.code == "invalid_response" && e.message == "Not Found");
    if e.message == NEEDS_NEWER_GATEWAY || no_route {
        return NEEDS_NEWER_GATEWAY.to_string();
    }
    let head = match e.code.as_str() {
        "unauthorized" => "Sign in to the gateway to manage automations.",
        "forbidden" => "You are not allowed to do this with automations.",
        "automation_not_found" => "This automation does not exist (or is not yours).",
        "occurrence_not_found" => "That occurrence does not exist.",
        "revision_conflict" => {
            "The automation changed since this view loaded. Reload it, then try again."
        }
        "automation_busy" => "An occurrence is already running or queued. Wait for it to finish.",
        "invalid_state" => "The automation's current state does not allow this.",
        "identity_conflict" => "This request id was already used for a different request.",
        "invalid_request" => "The gateway rejected the request as malformed.",
        "invalid_definition" => "The automation definition is not valid.",
        "unsupported_feature" => "The gateway does not support this yet.",
        "unknown_trigger_source" => "The gateway does not know this trigger source.",
        "invalid_response" => "The gateway gave an unexpected answer.",
        "unreachable" => "The gateway could not be reached.",
        _ => "",
    };
    let head = if head.is_empty() {
        format!("The gateway refused the request ({}).", e.code)
    } else {
        head.to_string()
    };
    let field = e
        .field
        .as_deref()
        .map(|f| format!(" (field {f})"))
        .unwrap_or_default();
    let msg = e.message.trim();
    if msg.is_empty() || head.contains(msg) {
        format!("{head}{field}")
    } else {
        format!("{head} {msg}{field}")
    }
}

/// Whether the gateway advertises the Automations API
/// (`capabilities.contracts.common.automations.available`).
pub fn capability_from_discovery(discovery: &Value) -> Result<(), String> {
    let auto = discovery
        .pointer("/capabilities/contracts/common/automations")
        .or_else(|| discovery.pointer("/contracts/common/automations"));
    match auto.and_then(|a| a.get("available")).and_then(Value::as_bool) {
        Some(true) => Ok(()),
        Some(false) => Err("This gateway has the Automations API turned off.".into()),
        None => Err(
            "This gateway does not advertise the Automations API (capabilities.contracts.common.automations); \
             update AbstractGateway (0.6.0 or later) to use automations."
                .into(),
        ),
    }
}

// ---------------------------------------------------------------------------
// View state (lives in `Store::automations`; mutated on the UI thread only)
// ---------------------------------------------------------------------------

/// The open automation: its summary, definition and loaded occurrences.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Detail {
    pub id: String,
    pub summary: Option<Summary>,
    pub definition: Option<Definition>,
    /// Oldest first.
    pub occurrences: Vec<Occurrence>,
    /// Cursor of the next OLDER page (`None` = everything loaded).
    pub next_cursor: Option<String>,
    pub error: String,
}

/// Everything the `/automations` screens render.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct View {
    /// `None` until asked; `Err(reason)` when the gateway lacks the API.
    pub availability: Option<Result<(), String>>,
    /// `None` until the first answer; `Err` = the last read failed.
    pub list: Option<Result<Vec<Summary>, String>>,
    pub loading: bool,
    /// `archived_automations` of the newest list answer (the gateway's
    /// count, never a client tally).
    pub archived_count: u64,
    /// The archived automations (`GET /automations?status=archived`);
    /// `None` until read.
    pub archived: Option<Result<Vec<Summary>, String>>,
    /// The `Archived · N` line is open (its rows show, each with Unarchive).
    pub archived_open: bool,
    pub detail: Option<Detail>,
    /// An action is in flight (controls are disabled with "Working…").
    pub busy: bool,
    pub notice: String,
    pub error: String,
    pub ids: ActionIds,
    /// (automation id, last acknowledged attention cursor).
    pub acked: Vec<(String, String)>,
    /// The acknowledgement in flight (never sent twice); after a failure it
    /// is retried once the next gateway read lands (`ack_failed`).
    pub ack_inflight: Option<(String, String)>,
    pub ack_failed: bool,
    /// A discussion the gateway just started: the UI switches to it once.
    pub discussion: Option<(u64, DiscussResponse)>,
    /// An automation the gateway just created: the UI opens it once.
    pub created: Option<String>,
    /// `GET /api/gateway/me/email`, read when `/schedule` or an Edit opens
    /// (`None` = unknown: not read yet or the call failed — not usable).
    pub email: Option<crate::automation_email::EmailStatus>,
    /// `/schedule`'s "What" picker: `GET /bundles?executable_for=…`.
    pub executable: Option<Result<crate::workflow_picker::Executable, String>>,
    /// Input schemas read for `/schedule` (key `bundle@version:flow`):
    /// the normalised schema, or the sentence why it could not be read.
    pub schemas: Vec<(String, Result<Value, String>)>,
    /// The `/schedule` When step's served line (schedule-preview), keyed by
    /// the trigger JSON it describes (a stale answer never replaces a newer ask).
    pub preview: Option<(String, PreviewState)>,
    /// Per automation id: the calendar rule picked in the Edit panel, kept
    /// across kind switches (each switch is saved as its own revision).
    pub calendar_rules: Vec<(String, CalendarRuleState)>,
    /// Whether this gateway serves the round-16 schedule API (probed once
    /// per session by the first preview; see [`ScheduleApi`]).
    pub schedule_api: ScheduleApi,
    /// The action in flight on one automation (space Active, g Run now,
    /// x Stop, a Archive, u Unarchive): its row shows the pending mark until
    /// the gateway's state shows it done, or it refuses.
    pub pending: Option<Pending>,
}

/// An action sent for one automation and not yet seen done. The gateway
/// ACCEPTS a command when it is queued and its controller applies it
/// moments later, so "accepted" is not "done": the row keeps its pending
/// mark until a re-read shows the new state (or the last follow-up read).
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub id: String,
    pub control: Control,
    /// The summary's state when the action was sent.
    pub status: String,
    pub occurrences: u64,
    pub current_run: Option<String>,
    /// The gateway's notice once it accepted the command (`None` = no answer yet).
    pub accepted: Option<String>,
}

impl Pending {
    pub fn of(s: &Summary, control: Control) -> Pending {
        Pending {
            id: s.id.clone(),
            control,
            status: s.status.clone(),
            occurrences: s.occurrence_count,
            current_run: s.current.as_ref().map(|c| c.run_id.clone()),
            accepted: None,
        }
    }

    /// Whether `s` — this automation as the gateway now lists it (`None` =
    /// absent from the active list) — shows the action done.
    pub fn done_by(&self, s: Option<&Summary>) -> bool {
        match self.control {
            Control::Pause => s.is_some_and(|s| s.status != "active"),
            Control::Resume => s.is_some_and(|s| s.status != "paused"),
            Control::Archive => s.is_none_or(|s| s.status == "archived"),
            Control::Unarchive => s.is_some_and(|s| s.status != "archived"),
            Control::RunNow => s.is_some_and(|s| {
                s.occurrence_count > self.occurrences
                    || s.current.as_ref().map(|c| &c.run_id) != self.current_run.as_ref()
            }),
            Control::StopCurrent => {
                s.is_none_or(|s| s.current.as_ref().map(|c| &c.run_id) != self.current_run.as_ref())
            }
            Control::Revise | Control::Discuss => true,
        }
    }
}

impl View {
    /// The control in flight on automation `id`, if any.
    pub fn pending_on(&self, id: &str) -> Option<Control> {
        self.pending
            .as_ref()
            .filter(|p| p.id == id)
            .map(|p| p.control)
    }

    /// An action was sent for `s`: busy (input for the controls ignored)
    /// with the pending mark on its row until the answer.
    pub fn start_pending(&mut self, s: &Summary, control: Control) {
        self.busy = true;
        self.error.clear();
        self.notice = control.busy_notice().to_string();
        self.pending = Some(Pending::of(s, control));
    }

    /// The gateway accepted the command: still pending until a re-read
    /// shows it done (or the follow-up reads end, [`View::finish_pending`]).
    pub fn accept_pending(&mut self, notice: String) {
        match self.pending.as_mut() {
            Some(p) => p.accepted = Some(notice),
            None => {
                self.busy = false;
                self.notice = notice;
            }
        }
        self.resolve_pending();
    }

    /// The gateway refused (or could not be reached): no longer pending,
    /// and its "Pausing…" goes (the refusal is the line now).
    pub fn fail_pending(&mut self) {
        if let Some(p) = self.pending.take() {
            if self.notice == p.control.busy_notice() {
                self.notice.clear();
            }
        }
    }

    /// The follow-up reads ended: an accepted action is reported as the
    /// gateway worded it, even if its state has not shown the change yet.
    pub fn finish_pending(&mut self) {
        if let Some(notice) = self.pending.as_ref().and_then(|p| p.accepted.clone()) {
            self.pending = None;
            self.busy = false;
            self.notice = notice;
        }
    }

    /// Done once the newest list (active, then archived) shows it.
    fn resolve_pending(&mut self) {
        let Some(p) = self.pending.as_ref() else {
            return;
        };
        let Some(notice) = p.accepted.clone() else {
            return;
        };
        let find = |list: &Option<Result<Vec<Summary>, String>>| match list {
            Some(Ok(items)) => items.iter().find(|s| s.id == p.id).cloned(),
            _ => None,
        };
        let fresh = find(&self.list).or_else(|| find(&self.archived));
        if p.done_by(fresh.as_ref()) {
            self.pending = None;
            self.busy = false;
            self.notice = notice;
        }
    }
}

impl View {
    /// The Edit panel's remembered rule for `id`, brought up to date with
    /// the stored `current` rule.
    pub fn calendar_state(&self, id: &str, current: &When) -> CalendarRuleState {
        let mut st = self
            .calendar_rules
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, st)| st.clone())
            .unwrap_or_default();
        st.absorb(current);
        st
    }
    /// Remember what was just picked for `id`.
    pub fn remember_calendar(&mut self, id: &str, when: &When) {
        match self.calendar_rules.iter_mut().find(|(k, _)| k == id) {
            Some((_, st)) => st.absorb(when),
            None => {
                let mut st = CalendarRuleState::default();
                st.absorb(when);
                self.calendar_rules.push((id.to_string(), st));
            }
        }
    }
    /// The served line for `trigger`, if it is the one last asked.
    pub fn preview_for(&self, trigger: &Value) -> Option<&PreviewState> {
        let key = trigger.to_string();
        self.preview
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, st)| st)
    }
    /// An answer arrived: kept only when it is for the latest ask.
    pub fn apply_preview(&mut self, trigger: &Value, state: PreviewState) {
        let key = trigger.to_string();
        if self.preview.as_ref().is_some_and(|(k, _)| *k == key) {
            self.preview = Some((key, state));
        }
    }
}

/// The `schemas` key of a workflow.
pub fn schema_key(bundle: &str, version: &str, flow: &str) -> String {
    format!("{bundle}@{version}:{flow}")
}

impl View {
    /// The schema read for `key`, if any (`Err` = why it could not be read).
    pub fn schema(&self, key: &str) -> Option<&Result<Value, String>> {
        self.schemas
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Keep a schema answer (the newest few).
    pub fn put_schema(&mut self, key: String, answer: Result<Value, String>) {
        self.schemas.retain(|(k, _)| *k != key);
        self.schemas.push((key, answer));
        if self.schemas.len() > 8 {
            let _ = self.schemas.remove(0);
        }
    }

    /// The list answered: keep the open automation's summary in step.
    pub fn apply_list(&mut self, items: Vec<Summary>) {
        self.retry_failed_ack();
        if let Some(d) = self.detail.as_mut() {
            if let Some(fresh) = items.iter().find(|s| s.id == d.id) {
                d.summary = Some(fresh.clone());
            }
        }
        self.list = Some(Ok(items));
        self.loading = false;
        self.resolve_pending();
    }

    /// The open automation's detail and FIRST occurrence page answered.
    /// Rows already loaded (older pages) are kept; the older-page cursor is
    /// kept once pages beyond the first are loaded.
    pub fn apply_detail(
        &mut self,
        id: &str,
        definition: Definition,
        summary: Summary,
        page: Page<Occurrence>,
    ) {
        self.retry_failed_ack();
        let Some(d) = self.detail.as_mut().filter(|d| d.id == id) else {
            return;
        };
        let deeper = d.occurrences.len() > page.items.len();
        d.occurrences = merge_occurrences(&d.occurrences, &page.items);
        if !deeper {
            d.next_cursor = page.next_cursor;
        }
        d.definition = Some(definition);
        d.summary = Some(summary);
        d.error.clear();
    }

    /// An older page answered.
    pub fn apply_more(&mut self, id: &str, page: Page<Occurrence>) {
        if let Some(d) = self.detail.as_mut().filter(|d| d.id == id) {
            d.occurrences = merge_occurrences(&d.occurrences, &page.items);
            d.next_cursor = page.next_cursor;
        }
    }

    pub fn open(&mut self, id: &str) {
        if self.detail.as_ref().map(|d| d.id.as_str()) != Some(id) {
            let summary = match &self.list {
                Some(Ok(items)) => items.iter().find(|s| s.id == id).cloned(),
                _ => None,
            };
            self.detail = Some(Detail {
                id: id.to_string(),
                summary,
                ..Detail::default()
            });
            // A different automation: the previous one's messages go. Coming
            // back from one of its own forms keeps them (a refused revision
            // must stay readable).
            self.notice.clear();
            self.error.clear();
        }
    }

    /// The cursor to acknowledge for the open automation, if it has shown
    /// attention items not acknowledged yet (never the summary's latest).
    pub fn cursor_to_ack(&self) -> Option<(String, String)> {
        let d = self.detail.as_ref()?;
        let cursor = attention_ack_cursor(d.summary.as_ref()?)?;
        let key = (d.id.clone(), cursor);
        let done = self.acked.contains(&key) || self.ack_inflight.as_ref() == Some(&key);
        (!done).then_some(key)
    }

    /// The acknowledgement was sent.
    pub fn ack_sent(&mut self, id: &str, cursor: &str) {
        self.ack_inflight = Some((id.to_string(), cursor.to_string()));
        self.ack_failed = false;
    }

    /// The gateway accepted the acknowledgement.
    pub fn mark_acked(&mut self, id: &str, cursor: &str) {
        self.acked.retain(|(i, _)| i != id);
        self.acked.push((id.to_string(), cursor.to_string()));
        self.ack_inflight = None;
    }

    /// The acknowledgement failed: retried after the next gateway read,
    /// never in a loop.
    pub fn ack_failed(&mut self) {
        self.ack_failed = true;
    }

    fn retry_failed_ack(&mut self) {
        if self.ack_failed {
            self.ack_inflight = None;
            self.ack_failed = false;
        }
    }
}

/// What a started discussion is, in words, from the gateway's answer.
pub fn discussion_notice(index: u64, r: &DiscussResponse) -> String {
    format!(
        "discussion forked from occurrence #{index} (session {}): it works in its own workspace {}; the automation's \
         files are mounted read-only at {} for the file tools (shell commands are not sandboxed), and nothing is \
         written back into the automation's session",
        r.session_id, r.workspace_root, r.mounted_workspace
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_growing_budget_is_one_context_revision() {
        let s = summary("active", false, &[]);
        let mut def = Definition {
            revision: 3,
            workflow_id: String::new(),
            tool_approval: "auto".into(),
            growing: Map::new(),
            max_attempts: None,
            workspace_root: String::new(),
            target: Value::Null,
            notify: Value::Null,
        };
        assert_eq!(growing_max_tokens_of(&s, &def), DEFAULT_GROWING_MAX_TOKENS);
        assert_eq!(
            growing_budget_changes(&s, &def, " 80000 "),
            Ok(Some(
                json!({"context": {"mode": "growing", "growing": {"max_tokens": 80000}}})
            ))
        );
        assert_eq!(
            growing_budget_changes(&s, &def, "50000"),
            Ok(None),
            "unchanged"
        );
        for bad in ["", "0", "-3", "lots", "1.5"] {
            assert_eq!(
                growing_budget_changes(&s, &def, bad),
                Err(GROWING_MAX_TOKENS_ERROR.to_string()),
                "{bad:?}"
            );
        }
        def.growing.insert("max_tokens".into(), json!(80000));
        assert_eq!(growing_max_tokens_of(&s, &def), 80000);
        assert_eq!(
            growing_budget_changes(&s, &def, "50000"),
            Ok(Some(json!({"context": {"mode": "growing"}}))),
            "back to the default: the mode alone (the kit's automationContext)"
        );
    }

    fn summary(status: &str, current: bool, caps: &[&str]) -> Summary {
        Summary {
            id: "a1".into(),
            title: "t".into(),
            status: status.into(),
            trigger: Trigger {
                source_id: "schedule".into(),
                source_version: 1,
                config: json!({"every": "8h"}).as_object().unwrap().clone(),
            },
            context_mode: "independent".into(),
            workspace_root: None,
            next_run_at: None,
            next_run_local: None,
            time_zone: "Europe/Paris".into(),
            schedule_text: "Every 8 hours (UTC)".into(),
            schedule_rule_text: "Every 8 hours (UTC)".into(),
            current: current.then(|| CurrentOccurrence {
                index: 3,
                run_id: "r3".into(),
                attempt: 1,
                status: "running".into(),
            }),
            occurrence_count: 3,
            last: None,
            attention: Attention {
                pending_waits: 0,
                unseen_count: 0,
                items: vec![],
                waits: vec![],
            },
            legacy: false,
            revision: Some(1),
            capabilities: caps.iter().map(|c| c.to_string()).collect(),
        }
    }

    const ALL: &[&str] = &[
        "revise",
        "pause",
        "resume",
        "run_now",
        "stop_current",
        "archive",
        "discuss",
    ];

    #[test]
    fn run_now_hint_is_the_kits_shared_text() {
        // Operator 2026-09-28: one shared "Run now" tooltip in every client. The
        // terminal's key line and /automations help say the kit's one line.
        assert_eq!(
            run_now_one_line(),
            "Run it once now, without waiting for the schedule; the next scheduled run keeps its time."
        );
        assert_eq!(
            run_now_key_line(),
            format!("g run now: {}", run_now_one_line())
        );
        let full = control_hint(Control::RunNow);
        assert!(
            full.starts_with("Run it once now, without waiting for the schedule.\n"),
            "{full}"
        );
        assert!(full.contains("the next scheduled run keeps its time, or starts right after this run if its time comes first"), "{full}");
        assert!(
            full.contains(
                "Does not count toward a run limit. Works while paused; it stays paused."
            ),
            "{full}"
        );
        let spec: Value = serde_json::from_str(AUTOMATION_CONTROLS_JSON).unwrap();
        for c in [
            Control::Pause,
            Control::Resume,
            Control::RunNow,
            Control::StopCurrent,
            Control::Revise,
            Control::Archive,
            Control::Discuss,
        ] {
            assert_eq!(
                control_hint(c),
                spec["hints"][c.capability()].as_str().unwrap()
            );
        }
        let help = crate::commands::HELP_LINES
            .iter()
            .find(|(k, _)| k.starts_with("/automations"))
            .map(|(_, v)| *v)
            .unwrap();
        assert!(
            help.contains(&format!("g run now: {}", run_now_one_line())),
            "{help}"
        );
    }

    #[test]
    fn run_now_follows_current_occurrence_only() {
        let idle = summary("active", false, ALL);
        assert_eq!(control_state(&idle, Control::RunNow, false), Ok(()));
        assert!(control_state(&idle, Control::StopCurrent, false).is_err());
        let busy = summary("active", true, ALL);
        assert_eq!(
            control_state(&busy, Control::RunNow, false),
            Err("An occurrence is in progress.".into())
        );
        assert_eq!(control_state(&busy, Control::StopCurrent, false), Ok(()));
        // Paused: run now works and keeps it paused; pause is off.
        let paused = summary("paused", false, ALL);
        assert_eq!(control_state(&paused, Control::RunNow, false), Ok(()));
        assert_eq!(
            control_state(&paused, Control::Pause, false),
            Err("Already paused.".into())
        );
        assert_eq!(control_state(&paused, Control::Resume, false), Ok(()));
    }

    #[test]
    fn active_switch_is_the_schedule_state_and_says_why_it_cannot_change() {
        let active = summary("active", false, ALL);
        let paused = summary("paused", false, ALL);
        assert_eq!(active_switch(&active, false), Ok(true));
        assert_eq!(active_switch(&paused, false), Ok(false));
        assert_eq!(active_command(&active), Control::Pause);
        assert_eq!(active_command(&paused), Control::Resume);
        assert_eq!(active_switch(&active, true), Err("Working…".into()));
        let archived = summary("archived", false, ALL);
        assert_eq!(
            active_switch(&archived, false),
            Err("Archived: history is kept, nothing runs.".into())
        );
        let ended = summary("completed", false, ALL);
        assert_eq!(
            active_switch(&ended, false),
            Err("The automation has ended.".into())
        );
        let mut legacy = summary("active", false, ALL);
        legacy.legacy = true;
        assert!(active_switch(&legacy, false)
            .unwrap_err()
            .starts_with("Legacy schedule"));
        // The transition's own capability decides (resume for a paused one).
        let no_resume = summary("paused", false, &["pause", "run_now"]);
        assert_eq!(
            active_switch(&no_resume, false),
            Err("Not permitted for this automation.".into())
        );
        // The accepted notice names the NEW state (Code web HEADER_NOTICES).
        let notice = |t: &str| Control::from_command_type(t).and_then(Control::accepted_notice);
        assert_eq!(notice("automation.pause"), Some("Automation paused."));
        assert_eq!(notice("automation.resume"), Some("Automation active."));
        assert_eq!(notice("automation.run_now"), Some("Run requested."));
        assert_eq!(notice("automation.stop_current"), Some("Stop requested."));
        assert_eq!(notice("automation.archive"), Some("Automation archived."));
        assert_eq!(
            notice("automation.unarchive"),
            Some("Unarchived: it is paused until you make it active.")
        );
        assert_eq!(active_label(), "Active");
    }

    #[test]
    fn archived_keeps_only_discuss() {
        let s = summary("archived", false, ALL);
        for c in [
            Control::Pause,
            Control::Resume,
            Control::RunNow,
            Control::Revise,
            Control::Archive,
        ] {
            assert_eq!(
                control_state(&s, c, false),
                Err("Archived: history is kept, nothing runs.".into())
            );
        }
        assert_eq!(control_state(&s, Control::Discuss, false), Ok(()));
        // Capabilities decide first.
        let s = summary("active", false, &["discuss"]);
        assert_eq!(
            control_state(&s, Control::Pause, false),
            Err("Not permitted for this automation.".into())
        );
        assert_eq!(
            control_state(&s, Control::Pause, true),
            Err("Working…".into())
        );
    }

    #[test]
    fn labels_and_time() {
        assert_eq!(status_label("active"), "Active ▶");
        assert_eq!(status_label("paused"), "Paused ⏸");
        assert_eq!(
            format_utc("2026-09-27T08:00:00.412307+00:00"),
            "2026-09-27 08:00 UTC"
        );
        assert_eq!(
            format_utc("2026-09-27T08:00:05Z"),
            "2026-09-27 08:00:05 UTC"
        );
        let now = unix_secs("2026-09-27T06:35:00Z").unwrap();
        assert_eq!(relative_in("2026-09-27T07:00:00Z", now), "in 25 min");
        assert_eq!(relative_in("2026-09-27T09:40:00Z", now), "in 3 h 5 min");
        assert_eq!(relative_in("2026-09-27T06:00:00Z", now), "due now");
        assert!(
            is_duration("7d") && !is_duration("07d") && !is_duration("1w") && !is_duration("m")
        );
    }

    #[test]
    fn next_comes_from_the_served_values_only() {
        let mut s = summary("active", true, ALL);
        assert_eq!(next_label(&s, 0), "none scheduled");
        assert_eq!(run_now_next_line(&s), None);
        s.next_run_at = Some("2026-09-27T07:00:00Z".into());
        s.next_run_local = Some("2026-09-27T09:00:00+02:00".into());
        let now = unix_secs("2026-09-27T06:35:00Z").unwrap();
        assert_eq!(
            next_label(&s, now),
            "2026-09-27 09:00 Europe/Paris (in 25 min)"
        );
        assert_eq!(
            run_now_next_line(&s).as_deref(),
            Some("Next scheduled run: 2026-09-27 09:00 Europe/Paris.")
        );
        assert_eq!(next_run_text(&s, now).as_deref(), Some("next in 25 min"));
        // The served local string is CUT, never re-derived: a gateway that
        // serves another wall time is shown as served.
        s.next_run_local = Some("2026-09-27T23:59:00-07:00".into());
        s.time_zone = "America/Los_Angeles".into();
        assert_eq!(
            next_label(&s, now),
            "2026-09-27 23:59 America/Los_Angeles (in 25 min)"
        );
        // Without the served `next_run_at` there is no next run, whatever the rest says.
        s.next_run_at = None;
        assert_eq!(next_run_text(&s, now), None);
        let p = summary("paused", false, ALL);
        assert_eq!(next_label(&p, now), "none while paused");
        assert_eq!(served_local("garbage", "UTC"), "garbage");
    }

    #[test]
    fn calendar_rows_read_the_served_rule() {
        let mut s = summary("active", false, ALL);
        s.trigger = Trigger {
            source_id: "schedule".into(),
            source_version: 2,
            config: json!({"kind": "daily", "at": "08:00", "time_zone": "Europe/Paris"})
                .as_object()
                .unwrap()
                .clone(),
        };
        s.schedule_rule_text = "Every day at 08:00 (Europe/Paris)".into();
        assert_eq!(summary_cadence(&s), "Every day at 08:00 (Europe/Paris)");
        assert_eq!(
            summary_trigger_text(&s),
            "Every day at 08:00 (Europe/Paris)"
        );
        // A rule the gateway did not serve reads as "—", never a sentence made up here.
        s.schedule_rule_text.clear();
        assert_eq!(summary_cadence(&s), "—");
        // Repeat too reads the served words (never a local "every 24 h").
        s.trigger.config = json!({"kind": "every", "every": "24h"})
            .as_object()
            .unwrap()
            .clone();
        s.schedule_rule_text = "Every 24 hours (UTC) · 3 runs max".into();
        assert_eq!(summary_cadence(&s), "Every 24 hours (UTC) · 3 runs max");
        assert_eq!(
            summary_trigger_text(&s),
            "Every 24 hours (UTC) · 3 runs max"
        );
        // schedule@1 rows as well; a missing value reads as "—".
        s.trigger.source_version = 1;
        assert_eq!(summary_cadence(&s), "Every 24 hours (UTC) · 3 runs max");
        s.schedule_rule_text.clear();
        assert_eq!(summary_cadence(&s), "—");
        assert_eq!(summary_trigger_text(&s), "—");
    }

    /// A row from a gateway before round 16 (the operator's 0.13.x shape,
    /// copied from a live `GET /automations`): no `time_zone`, `next_run_at`,
    /// `next_run_local`, `schedule_text` or `schedule_rule_text` — only
    /// `next_fire_at`. The list must read; the rule shows "—" and the next
    /// run falls back to `next_fire_at`, in UTC.
    #[test]
    fn a_pre_round_16_row_reads_with_next_fire_at_and_dashes() {
        let v: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/legacy_summary/list-gateway-0.13.json"
        ))
        .unwrap();
        let page = parse_list_page(&v).expect("a pre-round-16 row never fails the list");
        let s = &page.items[0];
        assert_eq!(
            s.next_run_at.as_deref(),
            Some("2026-10-09T19:53:29.622724+00:00")
        );
        assert_eq!(
            s.next_run_local.as_deref(),
            Some("2026-10-09T19:53:29.622724+00:00")
        );
        assert_eq!(s.time_zone, "UTC");
        assert_eq!(s.schedule_rule_text, "");
        assert_eq!(summary_cadence(s), "—");
        assert_eq!(summary_trigger_text(s), "—");
        let now = unix_secs("2026-10-09T16:53:29+00:00").unwrap();
        assert_eq!(next_label(s, now), "2026-10-09 19:53 UTC (in 3 h)");
        assert_eq!(next_run_text(s, now).as_deref(), Some("next in 3 h"));
        assert!(run_now_next_line(s)
            .unwrap()
            .contains("2026-10-09 19:53 UTC"));
        assert!(
            timing_line(s, now).starts_with("— · last "),
            "{}",
            timing_line(s, now)
        );
        // A served field of the wrong type is as absent as a missing one.
        let mut row = v["items"][0].clone();
        row["time_zone"] = json!(5);
        row["schedule_rule_text"] = json!(["x"]);
        row["next_run_local"] = Value::Null;
        let s = parse_summary(&row).expect("invalid served fields never fail the row");
        assert_eq!(s.time_zone, "UTC");
        assert_eq!(served_rule(&s), "—");
        // No next run served at all: none scheduled, never a computed one.
        let mut row = v["items"][0].clone();
        row.as_object_mut().unwrap().remove("next_fire_at");
        let s = parse_summary(&row).unwrap();
        assert_eq!(
            (s.next_run_at.clone(), s.time_zone.clone()),
            (None, String::new())
        );
        assert_eq!(next_label(&s, now), "none scheduled");
        // A round-16 row keeps its served values (the fallback never overrides them).
        let mut row = v["items"][0].clone();
        row["time_zone"] = json!("Europe/Paris");
        row["next_run_at"] = json!("2026-10-09T19:53:29.622724+00:00");
        row["next_run_local"] = json!("2026-10-09T21:53:29.622724+02:00");
        row["schedule_rule_text"] = json!("Every 24 hours (UTC)");
        let s = parse_summary(&row).unwrap();
        assert_eq!(
            next_label(&s, now),
            "2026-10-09 21:53 Europe/Paris (in 3 h)"
        );
        assert_eq!(summary_cadence(&s), "Every 24 hours (UTC)");
    }

    fn form_with(when: When) -> CreateForm {
        CreateForm {
            prompt: "brief me".into(),
            when,
            ..CreateForm::default()
        }
    }

    #[test]
    fn every_when_kind_writes_schedule_v2() {
        let target = || Some(json!({"flow_id": "f"}));
        let cfg = |when: When| {
            build_create_request(&form_with(when), target(), false, "r").unwrap()["trigger"].clone()
        };
        assert_eq!(
            cfg(When::Daily { at: "08:00".into() }),
            json!({"source_id": "schedule", "source_version": 2, "config": {"kind": "daily", "at": "08:00"}})
        );
        assert_eq!(
            cfg(When::Weekly {
                days: vec!["fri".into(), "mon".into(), "fri".into()],
                at: "07:30".into()
            })["config"],
            json!({"kind": "weekly", "days": ["mon", "fri"], "at": "07:30"})
        );
        assert_eq!(
            cfg(When::Monthly {
                day: "31".into(),
                at: "08:00".into()
            })["config"],
            json!({"kind": "monthly", "day": 31, "at": "08:00"})
        );
        assert_eq!(
            cfg(When::Monthly {
                day: "last".into(),
                at: "23:59".into()
            })["config"],
            json!({"kind": "monthly", "day": "last", "at": "23:59"})
        );
        assert_eq!(
            cfg(When::Once {
                at: "2026-10-09 10:00".into()
            })["config"],
            json!({"kind": "once", "at": "2026-10-09T10:00"})
        );
        assert_eq!(
            cfg(When::Every {
                amount: "24".into(),
                unit: 'h'
            })["config"],
            json!({"kind": "every", "every": "24h"})
        );
        // No time zone on the wire: the gateway fills the owner's.
        assert!(cfg(When::Daily { at: "08:00".into() })["config"]
            .get("time_zone")
            .is_none());
        // Calendar rules carry max runs / stop at; never a first-run time.
        let f = CreateForm {
            count: "3".into(),
            until: "2026-12-31 18:00".into(),
            start_at: "2026-10-08 09:00".into(),
            ..form_with(When::Daily { at: "08:00".into() })
        };
        assert_eq!(
            build_create_request(&f, target(), false, "r").unwrap()["trigger"]["config"],
            json!({"kind": "daily", "at": "08:00", "count": 3, "until": "2026-12-31T18:00:00Z"})
        );
        // Every kind's line is the gateway's; Repeat names no account zone.
        let repeat = form_with(When::Every {
            amount: "1".into(),
            unit: 'h',
        });
        assert!(f.when.is_served() && repeat.when.is_served());
        assert!(f.when.uses_time_zone() && !repeat.when.uses_time_zone());
    }

    #[test]
    fn revising_a_calendar_rule_keeps_the_bindings_time_zone() {
        let mut s = summary("active", false, ALL);
        s.trigger = Trigger {
            source_id: "schedule".into(),
            source_version: 2,
            config: json!({"kind": "monthly", "day": 31, "at": "08:00", "time_zone": "America/Los_Angeles",
                           "start_at": "2026-09-01T00:00:00Z", "anchor": "2026-09-01T00:00:00Z"})
                .as_object()
                .unwrap()
                .clone(),
        };
        let before = revise_form_from(&s);
        assert_eq!(
            before.calendar,
            Some(When::Monthly {
                day: "31".into(),
                at: "08:00".into()
            })
        );
        assert_eq!(revise_changes(&s, &before), Ok(None));
        let mut f = before.clone();
        f.calendar = Some(When::Monthly {
            day: "last".into(),
            at: "08:00".into(),
        });
        let c = revise_changes(&s, &f).unwrap().unwrap();
        assert_eq!(
            c,
            json!({"trigger": {"source_id": "schedule", "source_version": 2,
                "config": {"kind": "monthly", "day": "last", "at": "08:00", "time_zone": "America/Los_Angeles"}}})
        );
        f.calendar = Some(calendar_when_of(
            "weekly",
            &When::Monthly {
                day: "31".into(),
                at: "06:15".into(),
            },
        ));
        assert_eq!(
            f.calendar,
            Some(When::Weekly {
                days: vec!["mon".into()],
                at: "06:15".into()
            })
        );
        f.calendar = Some(When::Weekly {
            days: vec![],
            at: "06:15".into(),
        });
        assert_eq!(
            revise_changes(&s, &f),
            Err(vec![schedule_text("error_days").to_string()])
        );
        // A schedule@1 / Repeat row has no calendar rule to edit.
        assert_eq!(
            revise_form_from(&summary("active", false, ALL)).calendar,
            None
        );
        assert_eq!(
            time_zone_line_automation("UTC"),
            "in UTC (this automation's time zone)"
        );
    }

    #[test]
    fn revising_a_calendar_rule_keeps_the_bindings_limits() {
        let mut s = summary("active", false, ALL);
        s.trigger = Trigger {
            source_id: "schedule".into(),
            source_version: 2,
            config: json!({"kind": "daily", "at": "08:00", "time_zone": "Europe/Paris",
                           "count": 10, "until": "2026-12-31T18:00:00+00:00",
                           "start_at": "2026-09-01T00:00:00Z", "anchor": "2026-09-01T00:00:00Z"})
            .as_object()
            .unwrap()
            .clone(),
        };
        let mut f = revise_form_from(&s);
        f.calendar = Some(When::Daily { at: "07:30".into() });
        assert_eq!(
            revise_changes(&s, &f).unwrap().unwrap()["trigger"]["config"],
            json!({"kind": "daily", "at": "07:30", "time_zone": "Europe/Paris",
                   "count": 10, "until": "2026-12-31T18:00:00+00:00"})
        );
    }

    #[test]
    fn picked_days_survive_kind_switches() {
        let mut v = View::default();
        let weekly = When::Weekly {
            days: vec!["mon".into(), "fri".into()],
            at: "07:30".into(),
        };
        v.remember_calendar("a", &weekly);
        let monthly = v.calendar_state("a", &weekly).rule("monthly");
        assert_eq!(
            monthly,
            When::Monthly {
                day: "1".into(),
                at: "07:30".into()
            }
        );
        v.remember_calendar("a", &monthly);
        assert_eq!(v.calendar_state("a", &monthly).rule("weekly"), weekly);
        // An emptied day set stays empty (refused when saved, never refilled).
        let empty = When::Weekly {
            days: vec![],
            at: "07:30".into(),
        };
        v.remember_calendar("a", &empty);
        assert_eq!(
            v.calendar_state("a", &When::Daily { at: "07:30".into() })
                .rule("weekly"),
            empty
        );
        // Another automation starts from its own stored rule.
        assert_eq!(
            v.calendar_state("b", &When::Daily { at: "09:00".into() })
                .rule("weekly"),
            When::Weekly {
                days: vec!["mon".into()],
                at: "09:00".into()
            }
        );
    }

    #[test]
    fn when_errors_are_the_kits_sentences() {
        let target = || Some(json!({"flow_id": "f"}));
        let err =
            |when: When| build_create_request(&form_with(when), target(), false, "r").unwrap_err();
        assert_eq!(
            err(When::Daily { at: "24:00".into() }),
            vec![schedule_text("error_at").to_string()]
        );
        assert_eq!(
            err(When::Daily { at: "8:00".into() }),
            vec!["Pick the time of day (HH:MM).".to_string()]
        );
        assert_eq!(
            err(When::Weekly {
                days: vec![],
                at: "08:00".into()
            }),
            vec![schedule_text("error_days").to_string()]
        );
        assert_eq!(
            err(When::Monthly {
                day: "32".into(),
                at: "08:00".into()
            }),
            vec![schedule_text("error_day").to_string()]
        );
        assert_eq!(
            err(When::Monthly {
                day: "0".into(),
                at: "08:00".into()
            }),
            vec![schedule_text("error_day").to_string()]
        );
        assert_eq!(
            err(When::Once {
                at: "tomorrow".into()
            }),
            vec![schedule_text("error_once").to_string()]
        );
        assert_eq!(
            err(When::Once {
                at: "2026-13-01 10:00".into()
            }),
            vec![schedule_text("error_once").to_string()]
        );
    }

    #[test]
    fn preview_answer_and_request() {
        let r = preview_request(
            json!({"source_id": "schedule", "source_version": 2, "config": {"kind": "daily", "at": "08:00"}}),
        );
        assert_eq!(r.method, "POST");
        assert_eq!(r.path, "/api/gateway/automations/schedule-preview");
        assert_eq!(
            r.body.as_ref().unwrap()["trigger"]["config"]["kind"],
            "daily"
        );
        let answer = json!({
            "trigger": {"source_id": "schedule", "source_version": 2, "config": {"kind": "daily", "at": "08:00", "time_zone": "Europe/Paris"}},
            "time_zone": "Europe/Paris",
            "schedule_rule_text": "Every day at 08:00 (Europe/Paris)",
            "schedule_text": "Every day at 08:00 (Europe/Paris) · next Fri 9 Oct 08:00",
            "next_run_at": "2026-10-09T06:00:00+00:00",
            "next_run_local": "2026-10-09T08:00:00+02:00",
            "first_run_sentence": "Runs every day at 08:00 (Europe/Paris), first run Fri 9 Oct 08:00."
        });
        let p = parse_schedule_preview(&answer).unwrap();
        assert_eq!(
            preview_lines(&PreviewState::Ready(p.clone()), false),
            vec!["Runs every day at 08:00 (Europe/Paris), first run Fri 9 Oct 08:00.".to_string()],
            "Repeat names no account time zone"
        );
        assert_eq!(
            preview_lines(&PreviewState::Ready(p), true),
            vec![
                "in Europe/Paris (your account's time zone)".to_string(),
                "Runs every day at 08:00 (Europe/Paris), first run Fri 9 Oct 08:00.".to_string()
            ]
        );
        assert_eq!(
            preview_lines(&PreviewState::Loading, true),
            vec!["Checking the schedule…".to_string()]
        );
        assert!(parse_schedule_preview(&json!({"time_zone": "UTC"})).is_err());
        // A stale answer never replaces the newer ask.
        let a = json!({"k": 1});
        let b = json!({"k": 2});
        let mut v = View {
            preview: Some((b.to_string(), PreviewState::Loading)),
            ..View::default()
        };
        v.apply_preview(&a, PreviewState::Failed("old".into()));
        assert_eq!(v.preview_for(&b), Some(&PreviewState::Loading));
        assert_eq!(v.preview_for(&a), None);
        v.apply_preview(&b, PreviewState::Failed("no".into()));
        assert_eq!(v.preview_for(&b), Some(&PreviewState::Failed("no".into())));
    }

    #[test]
    fn create_body_is_the_shared_contract() {
        let form = CreateForm {
            prompt: "  check memory\nsecond line ".into(),
            when: When::Every {
                amount: "5".into(),
                unit: 'm',
            },
            context: "growing".into(),
            tool_approval: "ask".into(),
            title: String::new(),
            ..CreateForm::default()
        };
        let target = Some(json!({"flow_id": "@default", "interface": CODE_AGENT_INTERFACE}));
        let body = build_create_request(&form, target, false, "rid-1").unwrap();
        assert_eq!(
            body,
            json!({
                "request_id": "rid-1",
                "title": "check memory",
                "target": {"flow_id": "@default", "interface": "abstractcode.agent.v1",
                           "input_data": {"prompt": "check memory\nsecond line"}},
                "trigger": {"source_id": "schedule", "source_version": 2, "config": {"kind": "every", "every": "5m"}},
                "context": {"mode": "growing"},
                "policy": {"tool_approval": "ask"},
            })
        );
        let once = CreateForm {
            when: When::Once {
                at: "2026-10-01 08:30".into(),
            },
            ..form.clone()
        };
        let body = build_create_request(
            &once,
            Some(json!({"bundle_ref": "b@1", "flow_id": "f"})),
            false,
            "r",
        )
        .unwrap();
        assert_eq!(
            body["trigger"]["config"],
            json!({"kind": "once", "at": "2026-10-01T08:30"})
        );
        let bad = CreateForm {
            prompt: " ".into(),
            when: When::Every {
                amount: "0".into(),
                unit: 'h',
            },
            ..CreateForm::default()
        };
        let errs = build_create_request(&bad, None, false, "r").unwrap_err();
        assert_eq!(
            errs,
            vec![
                "Choose what to run.".to_string(),
                "Write the task to run.".into(),
                "The interval must be a whole number of at least 1.".into(),
            ]
        );
    }

    #[test]
    fn title_and_limits_and_workspaces_ride_the_create_body() {
        let form = CreateForm {
            prompt: "check memory".into(),
            title: "Memory".into(),
            start_at: "2026-10-08 09:00".into(),
            count: "3".into(),
            until: "2026-10-31 18:00".into(),
            workspace: Some(crate::workspaces::RunValue {
                posture: crate::workspaces::Posture::AllowedOnly,
                default_mode: crate::workspaces::Mode::Rw,
                folders: vec![crate::workspaces::Rule {
                    path: "/Users/ada/home/work".into(),
                    mode: crate::workspaces::Mode::Ro,
                }],
            }),
            ..CreateForm::default()
        };
        let built = json!({"prompt": "check memory", "workspace_root": "/tmp/x"});
        let body = schedule_body(
            &form,
            Some(json!({"bundle_ref": "b@1", "flow_id": "f"})),
            false,
            &built,
            "r",
        )
        .unwrap();
        assert_eq!(body["title"], json!("Memory"));
        assert_eq!(
            body["trigger"]["config"],
            json!({"kind": "every", "every": "24h", "start_at": "2026-10-08T09:00:00Z", "count": 3, "until": "2026-10-31T18:00:00Z"})
        );
        assert_eq!(
            body["target"]["input_data"]["workspace"],
            json!({"posture": "allowed_only", "default_mode": "rw",
                   "folders": [{"path": "/Users/ada/home/work", "mode": "ro"}]})
        );
        // Use my default sends nothing workspace-shaped.
        let plain = schedule_body(
            &CreateForm {
                workspace: None,
                ..form.clone()
            },
            Some(json!({"flow_id": "f"})),
            false,
            &built,
            "r",
        )
        .unwrap();
        assert!(plain["target"]["input_data"].get("workspace").is_none());
        // The kit's sentences for a bad limit; a once schedule ignores the limits.
        let bad = CreateForm {
            count: "0".into(),
            start_at: "tomorrow".into(),
            ..form.clone()
        };
        let errs =
            build_create_request(&bad, Some(json!({"flow_id": "f"})), false, "r").unwrap_err();
        assert_eq!(
            errs,
            vec![
                "First run must be a date and time (UTC).".to_string(),
                "Maximum runs must be a whole number of at least 1.".into()
            ]
        );
        let once = CreateForm {
            when: When::Once {
                at: "2026-10-09 10:00".into(),
            },
            ..bad
        };
        let body = build_create_request(&once, Some(json!({"flow_id": "f"})), false, "r").unwrap();
        assert_eq!(
            body["trigger"]["config"],
            json!({"kind": "once", "at": "2026-10-09T10:00"})
        );
    }

    #[test]
    fn nothing_email_shaped_rides_when_email_result_is_off() {
        let form = CreateForm {
            prompt: "x".into(),
            ..CreateForm::default()
        };
        let body = build_create_request(&form, Some(json!({"flow_id": "f"})), true, "r").unwrap();
        assert!(body.get("notify").is_none(), "{body}");
        let on = CreateForm {
            notify_email: true,
            ..form.clone()
        };
        let body = build_create_request(&on, Some(json!({"flow_id": "f"})), true, "r").unwrap();
        assert_eq!(body["notify"], json!({"channels": ["console", "email"]}));
        // Not usable: the switch's value is never sent.
        let body = build_create_request(&on, Some(json!({"flow_id": "f"})), false, "r").unwrap();
        assert!(body.get("notify").is_none(), "{body}");
    }

    #[test]
    fn an_email_trigger_interval_is_at_least_60_seconds() {
        let mut s = summary("active", false, ALL);
        s.trigger = Trigger {
            source_id: "email.received".into(),
            source_version: 1,
            config: serde_json::from_value(
                json!({"every": "1h", "start_at": "2026-10-08T16:40:00Z"}),
            )
            .unwrap(),
        };
        let mut f = revise_form_from(&s);
        f.every = Some("59s".into());
        assert_eq!(
            revise_changes(&s, &f),
            Err(vec![
                "The check interval is at least 60 seconds.".to_string()
            ])
        );
        f.every = Some("60s".into());
        let c = revise_changes(&s, &f).unwrap().unwrap();
        assert_eq!(
            c["trigger"]["config"],
            json!({"every": "60s"}),
            "start_at dropped"
        );
    }

    #[test]
    fn revise_sends_only_changes() {
        let s = summary("active", false, ALL);
        assert_eq!(revise_changes(&s, &revise_form_from(&s)), Ok(None));
        let mut f = revise_form_from(&s);
        f.every = Some("6h".into());
        f.context = "growing".into();
        let c = revise_changes(&s, &f).unwrap().unwrap();
        assert_eq!(c["trigger"]["config"]["every"], "6h");
        assert_eq!(c["context"], json!({"mode": "growing"}));
        assert!(c.get("title").is_none());
        f.every = Some("6 hours".into());
        assert!(revise_changes(&s, &f).is_err());
    }

    #[test]
    fn wait_answers_follow_the_kind() {
        assert_eq!(
            wait_answer_payload("tool_approval", "approve"),
            Ok(json!({"approved": true}))
        );
        assert_eq!(
            wait_answer_payload("tool_approval", "deny"),
            Ok(json!({"approved": false}))
        );
        assert!(wait_answer_payload("tool_approval", "yes please").is_err());
        assert_eq!(
            wait_answer_payload("ask_user", "Tuesday"),
            Ok(json!({"response": "Tuesday"}))
        );
        assert_eq!(
            wait_answer_payload("event", "{\"a\":1}"),
            Ok(json!({"payload": {"a": 1}}))
        );
        assert!(wait_answer_payload("event", "{nope").is_err());
        assert!(wait_answer_payload("mystery", "x").is_err());
    }

    #[test]
    fn errors_read_the_envelope() {
        let e = parse_api_error(
            409,
            r#"{"detail":{"reason_code":"automation_busy","message":"busy now"}}"#,
        );
        assert_eq!(e.code, "automation_busy");
        assert_eq!(
            api_error_text(&e),
            "An occurrence is already running or queued. Wait for it to finish. busy now"
        );
        let raw = parse_api_error(502, "<html>bad gateway</html>");
        assert_eq!(raw.code, "invalid_response");
    }

    // -- compatibility with AbstractGateway 0.13.x (no round-16 schedule API) --

    #[test]
    fn only_404_and_405_mean_the_route_is_missing() {
        let e = |status: Option<u16>, code: &str| ApiError {
            status,
            code: code.into(),
            message: "Method Not Allowed".into(),
            field: None,
        };
        assert!(is_missing_route(&e(Some(405), "invalid_request")));
        assert!(is_missing_route(&e(Some(404), "invalid_response")));
        for (status, code) in [
            (Some(422), "invalid_definition"),
            (Some(400), "invalid_request"),
            (None, "unreachable"),
        ] {
            assert!(!is_missing_route(&e(status, code)), "{status:?}");
        }
        // The operator's sentence ("…malformed. Method Not Allowed") is never shown:
        // a route the gateway lacks reads as the one sentence.
        assert_eq!(
            api_error_text(&e(Some(405), "invalid_request")),
            NEEDS_NEWER_GATEWAY
        );
        assert_eq!(
            api_error_text(&parse_api_error(
                405,
                r#"{"detail": {"reason_code": "invalid_request", "message": "Method Not Allowed"}}"#
            )),
            NEEDS_NEWER_GATEWAY
        );
        assert!(!api_error_text(&e(Some(400), "invalid_request")).contains("0.14"));
        // A 404 with the router's "Not Found" (no such path) is the sentence; a
        // route's own 404 keeps its words.
        assert_eq!(
            api_error_text(&parse_api_error(404, r#"{"detail":"Not Found"}"#)),
            NEEDS_NEWER_GATEWAY
        );
        let own = parse_api_error(
            404,
            r#"{"detail": {"reason_code": "automation_not_found", "message": "no such automation"}}"#,
        );
        assert_eq!(
            api_error_text(&own),
            "This automation does not exist (or is not yours). no such automation"
        );
        assert!(
            !api_error_text(&parse_api_error(404, r#"{"detail":"Run not found"}"#))
                .contains("0.14")
        );
    }

    #[test]
    fn a_v2_trigger_has_a_v1_form_except_the_calendar_rules() {
        let v2 =
            |config: Value| json!({"source_id": "schedule", "source_version": 2, "config": config});
        assert_eq!(
            legacy_trigger(&v2(
                json!({"kind": "every", "every": "8h", "start_at": "2026-10-11T07:00:00Z", "count": 5, "until": null})
            )),
            Ok(json!({"source_id": "schedule", "source_version": 1,
                      "config": {"every": "8h", "start_at": "2026-10-11T07:00:00Z", "count": 5}}))
        );
        assert_eq!(
            legacy_trigger(&v2(json!({"kind": "once", "at": "2026-10-11T07:30"}))),
            Ok(
                json!({"source_id": "schedule", "source_version": 1, "config": {"start_at": "2026-10-11T07:30:00Z"}})
            )
        );
        for kind in ["daily", "weekly", "monthly"] {
            assert_eq!(
                legacy_trigger(&v2(
                    json!({"kind": kind, "at": "08:00", "days": ["mon"], "day": "1"})
                )),
                Err(NEEDS_NEWER_GATEWAY.to_string()),
                "{kind}"
            );
        }
        // Anything else goes as it is.
        let v1 = json!({"source_id": "schedule", "source_version": 1, "config": {"every": "1h"}});
        assert_eq!(legacy_trigger(&v1), Ok(v1.clone()));
        let email = json!({"source_id": "email.received", "source_version": 1, "config": {}});
        assert_eq!(legacy_trigger(&email), Ok(email.clone()));
        assert!(needs_schedule_api(&v2(
            json!({"kind": "every", "every": "1h"})
        )));
        assert!(!needs_schedule_api(&v1) && !needs_schedule_api(&email));
        let body = json!({"title": "t", "trigger": v2(json!({"kind": "every", "every": "24h"}))});
        assert_eq!(
            legacy_create_body(&body).unwrap()["trigger"]["config"],
            json!({"every": "24h"})
        );
        assert_eq!(legacy_create_body(&body).unwrap()["title"], "t");
    }

    #[test]
    fn an_unavailable_preview_is_one_sentence() {
        for zone in [false, true] {
            assert_eq!(
                preview_lines(&PreviewState::Unavailable, zone),
                vec![NEEDS_NEWER_GATEWAY.to_string()]
            );
            assert_eq!(
                preview_lines(&PreviewState::Unreached, zone),
                vec![UNREACHED_LINE.to_string()]
            );
        }
    }

    // -- the pending state of an action (operator 2026-10-09: "space needs a spinner") --

    #[test]
    fn an_action_stays_pending_until_the_gateways_state_shows_it() {
        let active = summary("active", false, ALL);
        let mut v = View {
            list: Some(Ok(vec![active.clone()])),
            ..View::default()
        };
        v.start_pending(&active, Control::Pause);
        assert!(v.busy);
        assert_eq!(v.pending_on("a1"), Some(Control::Pause));
        assert_eq!(v.pending_on("other"), None);
        assert_eq!(v.notice, "Pausing…");
        // Accepted (queued): still pending, the controls still ignore input.
        v.accept_pending("Automation paused.".into());
        assert_eq!(v.pending_on("a1"), Some(Control::Pause));
        assert!(v.busy);
        assert_eq!(
            control_state(&active, Control::Resume, v.busy),
            Err("Working…".into())
        );
        // A re-read that still says active: still pending.
        v.apply_list(vec![active.clone()]);
        assert_eq!(v.pending_on("a1"), Some(Control::Pause));
        // The re-read shows it paused: done, the result said.
        v.apply_list(vec![summary("paused", false, ALL)]);
        assert_eq!(v.pending, None);
        assert!(!v.busy);
        assert_eq!(v.notice, "Automation paused.");
    }

    #[test]
    fn a_refused_action_is_not_pending_and_a_slow_state_ends_with_the_follow_ups() {
        let active = summary("active", false, ALL);
        let mut v = View::default();
        v.start_pending(&active, Control::RunNow);
        v.fail_pending();
        assert_eq!(v.pending, None);
        assert_eq!(v.notice, "", "no stale “Starting a run…” after a refusal");
        // Accepted, but no re-read showed the run (it came and went): the
        // follow-up reads end with the gateway's words.
        v.start_pending(&active, Control::RunNow);
        v.finish_pending();
        assert!(v.pending.is_some(), "no answer yet: still pending");
        v.accept_pending("Run requested.".into());
        v.finish_pending();
        assert_eq!(
            (v.pending.clone(), v.busy, v.notice.as_str()),
            (None, false, "Run requested.")
        );
    }

    #[test]
    fn each_control_knows_when_it_is_done() {
        let s = |status: &str, current: bool, count: u64| {
            let mut s = summary(status, current, ALL);
            s.occurrence_count = count;
            s
        };
        let p = |from: &Summary, c: Control| Pending::of(from, c);
        let idle = s("active", false, 3);
        assert!(p(&idle, Control::RunNow).done_by(Some(&s("active", true, 3))));
        assert!(p(&idle, Control::RunNow).done_by(Some(&s("active", false, 4))));
        assert!(!p(&idle, Control::RunNow).done_by(Some(&idle)));
        let busy = s("active", true, 3);
        assert!(p(&busy, Control::StopCurrent).done_by(Some(&s("active", false, 3))));
        assert!(!p(&busy, Control::StopCurrent).done_by(Some(&busy)));
        assert!(
            p(&idle, Control::Archive).done_by(None),
            "gone from the active list"
        );
        assert!(p(&idle, Control::Archive).done_by(Some(&s("archived", false, 3))));
        let archived = s("archived", false, 3);
        assert!(p(&archived, Control::Unarchive).done_by(Some(&s("paused", false, 3))));
        assert!(!p(&archived, Control::Unarchive).done_by(None));
        let paused = s("paused", false, 3);
        assert!(p(&paused, Control::Resume).done_by(Some(&idle)));
        assert!(!p(&paused, Control::Resume).done_by(Some(&paused)));
    }
}
