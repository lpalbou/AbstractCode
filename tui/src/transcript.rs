//! The transcript fold: ledger records -> UI items, stats, pending waits.
//!
//! One pure-ish state machine, owned by the UI thread. The runner posts raw
//! ledger records here; the fold appends/updates transcript items, folds
//! usage, tracks the blocking wait, and surfaces side requests (follow a
//! discovered subrun, fetch an image artifact) for the runner to act on.
//!
//! Dedup discipline (ported from the reference clients): wait keys and tool
//! call ids are seen-once sets, so ledger replays (reconnects re-read from a
//! cursor; reattach replays from 0) never duplicate cards or re-prompt
//! answered waits.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::protocol::{self, UsageDelta};

const ARGS_PREVIEW_MAX: usize = 200;
/// Strip-rendered cycle-intent one-liner (visibility review P2-1).
const CYCLE_PREVIEW_MAX: usize = 48;
pub const MAX_ITEMS: usize = 500;
/// Truncation hysteresis: items float in [MAX_ITEMS, MAX_ITEMS +
/// TRUNCATE_CHUNK] and each drain cuts back to MAX_ITEMS (see
/// `push_item` for why per-push draining was quadratic).
pub const TRUNCATE_CHUNK: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    AwaitingApproval,
    Running,
    Ok,
    Failed,
    Denied,
    /// The run reached a terminal state while this call was still
    /// in flight (adversarial review round 2, F3): "running" is a
    /// present-tense claim — frozen into scrollback it made "hung or
    /// historical?" unanswerable a week later. Not a failure: the
    /// result was simply never observed.
    Interrupted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CallCost {
    /// Provider-reported whole-call duration in milliseconds. `None`
    /// means the record did not carry a positive `gen_time`.
    pub gen_time_ms: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Prompt tokens the provider REPORTED as served from its cache
    /// for THIS call (operator ask 2026-08-19: "% input context that
    /// was cached and not recomputed" on the cycle rule). 0 = not
    /// reported — absence of evidence, not a cold cache — and the rule
    /// renders nothing rather than a derived guess (the prev-input
    /// derivation lives in /cache, labeled as the estimate it is).
    pub cached_tokens: u64,
}

impl CallCost {
    fn from_record(rec: &Value) -> Self {
        let usage = protocol::usage_from_record(rec).unwrap_or_default();
        Self {
            gen_time_ms: protocol::gen_time_ms_from_record(rec),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_tokens: usage.cached_tokens,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    User {
        text: String,
    },
    Steer {
        text: String,
    },
    /// One reasoning cycle's model output (content + optional reasoning).
    Thinking {
        iteration: u32,
        content: String,
        reasoning: String,
        /// Per-call cost facts, rendered on the transcript's cycle rule.
        ///
        /// `gen_time` is the whole call — prompt processing AND generation — because
        /// that is all a non-streaming response can report; the split needs a
        /// first-token timestamp the provider never sends back. What the two token
        /// counts DO separate is which side the time went to: a call with a large
        /// input and few output tokens spent it preprocessing, and one with a low
        /// output rate against a small input is not working, it is stuck. The
        /// pathological call in run 6b77351b was 120 minutes for 111 output tokens.
        call: CallCost,
    },
    Tool {
        key: String,
        name: String,
        /// One-line humane hint for the FOLDED row (bounded on purpose —
        /// it has one row to answer "which call was this").
        args_preview: String,
        /// The arguments in full, uncut: what `/details` renders. Nothing
        /// here is elided — an inner value, a path, a whole heredoc all
        /// survive (operator directive 2026-08-20: details means details).
        args_full: String,
        status: ToolStatus,
        /// The tool's output in FULL. The fold is no longer a preview
        /// store: bounding happens in the view, per verbosity mode, so
        /// `/details` can show everything the ledger reported.
        result: String,
        /// The error in FULL (multi-line preserved). The folded row
        /// one-lines it at render; `/details` shows all of it.
        error: String,
    },
    Assistant {
        text: String,
        final_answer: bool,
    },
    Image {
        run_id: String,
        artifact_id: String,
        label: String,
    },
    Info {
        text: String,
    },
    Error {
        text: String,
    },
    /// Details-gated probe body (entity turn transparency: memory digests
    /// behind the always-visible count chip) — treated exactly like
    /// `Thinking` in `is_visible`.
    Probe {
        title: String,
        body: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum WaitKind {
    Approval {
        tool_calls: Vec<Value>,
    },
    Ask {
        prompt: String,
        /// The flow's OWN declaration of what this ask is
        /// (`details.kind`/`details.mode`), empty when it declared none.
        /// Carried so unattended drivers answer from the server's word
        /// instead of each inventing its own reading of the prose.
        gate: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingWait {
    pub run_id: String,
    pub wait_key: String,
    /// The waiting record's step id — the OCCURRENCE identity. The runtime
    /// reuses stable wait keys across repeated asks (`user:{run}:{node}`),
    /// so dedup must key on the occurrence, not the key.
    pub step_id: String,
    pub kind: WaitKind,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Cumulative total tokens. Some usage dicts arrive with ONLY
    /// `total_tokens` (live coder run 0312b41d…: `{"input_tokens": 0,
    /// "output_tokens": 0, "total_tokens": 3180}`) — without this field
    /// the strip read "0↑ 0↓ tk" against "23 tools" for five hours.
    /// ATTRIBUTION CORRECTED (cycle-2 forensics, 2026-07-23): the zeros
    /// are minted by abstractcore's `_build_usage_dict`, which reads only
    /// Chat-Completions spellings (`prompt_tokens`/`completion_tokens`)
    /// and zero-fills when a relay answers in Responses-API spelling
    /// (`input_tokens`/`output_tokens` INSIDE the raw usage — the live
    /// incident's relay reported input 136,423 correctly). Not a model
    /// pathology; core fix filed. The client-side derivations (this
    /// field + the ctx `~` estimate) stay as defense either way.
    pub total_tokens: u64,
    pub llm_calls: u64,
    pub tool_calls: u64,
    /// Tool results that FAILED this run — the "edit_file failed 5× in a
    /// row" pattern was invisible from fixed chrome (visibility review
    /// P2-2): each ✗ card just scrolled past in the wall.
    pub tool_failures: u64,
    /// Output tokens per completed llm_call, newest last (sparkline food).
    pub output_series: Vec<f32>,
    /// Input tokens of the NEWEST llm_call — the context size the model
    /// received on the latest cycle (the honest "context used" number).
    pub last_input_tokens: u64,
    /// True when `last_input_tokens` was DERIVED (total − output) from a
    /// zero-poisoned split rather than reported — chrome renders "ctx ~N"
    /// so an estimate is never presented as a measurement.
    pub last_input_is_estimate: bool,
    /// Input tokens of the call BEFORE the newest one. The difference
    /// against `last_input_tokens` is the only cache-relevant number a
    /// client can compute without provider cooperation: how much of this
    /// prompt is NEW (must be evaluated) versus carried forward from the
    /// previous cycle (a cacheable prefix). Local llama.cpp-class servers
    /// almost never report `cached_input_tokens`, so this derivation is
    /// what makes the cache panel say anything at all — and it explains the
    /// thing operators actually feel, which is later cycles costing more
    /// because a big tool result was folded into the context.
    pub prev_input_tokens: u64,
    /// Cumulative prompt tokens served from the provider cache this run
    /// (0 when the provider never reports cache hits).
    pub cached_tokens: u64,
    /// The model that actually served the newest llm_call — the resolved
    /// truth even under "gateway defaults" (from the result's own field).
    pub effective_model: String,
    /// Cache hits the provider reported for the NEWEST telemetry-lane call
    /// (0 = not reported for that call). `cached_tokens` above is the
    /// run's cumulative total; this is the one call an operator is looking
    /// at when they open /cache.
    pub last_cached_tokens: u64,
    /// Output tokens of the newest telemetry-lane call.
    pub last_output_tokens: u64,
    /// Provider-reported whole-call duration of the newest telemetry-lane
    /// call, in whole milliseconds (0 = unreported).
    pub last_gen_time_ms: u64,
    /// Sum of provider-reported call durations this run (ms; unreported
    /// calls contribute 0, so this is a FLOOR, never an inflated total).
    pub gen_time_ms: u64,
    /// Calls whose usage carried a non-zero cache-hit count. Paired with
    /// `llm_calls` it separates "the cache missed" from "the provider
    /// never reports hits" — the distinction the panel used to blur.
    pub cache_reported_calls: u64,
    /// Sum of input tokens over calls that reported one — the honest
    /// denominator for a hit rate (calls with a zero-poisoned split
    /// contribute nothing to either side of the ratio).
    pub cacheable_input_tokens: u64,
    /// Derived NEW tokens across telemetry-lane calls: per call, the
    /// growth over the previous call's context (the whole prompt on the
    /// first call, and after a reset). Client-side derivation — labeled as
    /// an estimate everywhere it is shown.
    pub new_tokens: u64,
    /// Derived CARRIED tokens: per call, the previous call's context that
    /// this call re-sent — the reusable prefix a prompt cache exists to
    /// serve.
    pub carried_tokens: u64,
    /// Telemetry-lane calls whose context SHRANK against the previous one
    /// (compaction, reset, or a lane change): each one breaks the prefix a
    /// cache had built.
    pub context_resets: u64,
    /// Largest context observed on a telemetry-lane call this run.
    pub peak_input_tokens: u64,
}

/// Lifetime-of-fold totals (across runs; per-run stats reset per run).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Cumulative totals — the only honest number when the provider
    /// reports no input/output split (see `Stats::total_tokens`).
    pub total_tokens: u64,
    pub runs: u64,
    /// Model calls across every run in this session.
    pub llm_calls: u64,
    /// Prompt tokens the provider reported as served from its cache,
    /// across the whole session (0 = never reported — see
    /// `cache_reported_calls` before reading that as a cold cache).
    pub cached_tokens: u64,
    /// Session-lifetime mirrors of the per-run cache metrics
    /// (`Stats::cache_reported_calls` and friends). Per-run stats reset at
    /// `begin_run`; these are what "how is the cache doing overall?"
    /// actually means.
    pub cache_reported_calls: u64,
    pub cacheable_input_tokens: u64,
    pub new_tokens: u64,
    pub carried_tokens: u64,
    pub context_resets: u64,
    pub peak_input_tokens: u64,
    /// Sum of provider-reported call durations (ms); a floor, since
    /// unreported calls contribute 0.
    pub gen_time_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FoldEffect {
    /// A subworkflow run was discovered; the runner should stream it too.
    FollowRun(String),
    /// An image artifact appeared; fetch bytes and hand them to the UI.
    FetchImage { run_id: String, artifact_id: String },
    /// The final answer was OFFLOADED by the runtime's ledger offloader
    /// (outputs >256 KB persist as `{"$artifact": id}` and the read
    /// surface serves the ref unresolved): fetch the artifact content and
    /// resolve the placeholder card via `resolve_offloaded_answer`. The
    /// turn already CONCLUDED when this effect fired — a failed fetch
    /// labels the card honestly, never re-captures the composer.
    FetchAnswer { run_id: String, artifact_id: String },
}

/// The placeholder text for a not-yet-fetched offloaded answer. Exact-match
/// currency between `apply` (push) and `resolve_offloaded_answer` (swap).
pub fn offload_placeholder(artifact_id: &str) -> String {
    format!("(retrieving the full answer — the gateway stored it as artifact {artifact_id})")
}

/// Stable prefix of the fetch-failure label — the swap matcher's second
/// currency (`swap_answer_card`).
pub(crate) const OFFLOAD_FAILURE_PREFIX: &str = "(the final answer could not be retrieved";

/// The honest label when the offloaded answer's fetch failed. DELIBERATELY
/// carries no URL, no artifact id, and no retry framing: this text is
/// stored as ASSISTANT words and replays into later turns'
/// `context.messages` — the 2026-07-23 incident (Lane B, ledger-verified)
/// showed a URL-bearing "gateway unreachable — …" label plus an operator
/// "try again" is a complete instruction kit: the model called fetch_url
/// on the gateway's own artifact endpoint and hit the auth middleware.
/// `reason` is `GwError::compact_reason()`-class text (evidence-worded,
/// URL-free), never a raw error Display. It also names NO storage
/// internals (operator ruling 2026-07-26: users do not read ledgers) —
/// recovery is structural, not instructed: `unresolved_offload` stays
/// armed for the late-truth reconcile, and reopening the session
/// re-fires the fetch.
pub fn offload_failure_label(reason: &str) -> String {
    format!("{OFFLOAD_FAILURE_PREFIX}: {reason} — the run completed)")
}

/// The runtime's deterministic workflow-id prefix for a VisualFlow Agent
/// node's compiled ReAct subworkflow:
/// `visual_react_agent_{flow_id}_{node_id}` — minted by ONE function
/// (abstractruntime `visualflow_compiler/visual/agent_ids.py`,
/// `visual_react_workflow_id`), whose module docstring pins the ids as
/// "stable across hosts so a VisualFlow JSON document can be executed
/// outside the web editor (CLI, AbstractCode, third-party apps)". A
/// spawn record declaring this shape IS an agent loop — ledger
/// structure, no behavior needed. Live example:
/// `visual_react_agent_basic-agent_0_0_3_81795ea9_node-2`.
const VISUAL_REACT_AGENT_PREFIX: &str = "visual_react_agent_";

/// pub(crate): `convo.rs` shares these text bounds (it carried
/// byte-identical private copies until cycle-3 — the double-wave drift
/// class). Crate-internal only; the lib API never exposes them.
pub(crate) fn one_line(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max + 1));
    for ch in text.chars() {
        out.push(if ch == '\n' || ch == '\r' || ch == '\t' {
            ' '
        } else {
            ch
        });
        if out.chars().count() >= max {
            out.push('…'); // [#TRUNCATION] preview bound; the ledger keeps the full text
            break;
        }
    }
    out
}

/// The arguments in FULL for `/details`: the same humane ordering as
/// [`tool_args_preview`] — identifying value first, then `k=v` — with
/// every cut removed. No outer bound, no per-value 60-char clip, no
/// path compaction, and newlines PRESERVED (a heredoc or a multi-line
/// patch reads as itself). Details means details (operator, 2026-08-20).
pub(crate) fn tool_args_full(v: Option<&Value>) -> String {
    let obj = match v {
        None | Some(Value::Null) => return String::new(),
        Some(Value::String(s)) => return s.clone(),
        Some(Value::Object(m)) => m,
        Some(other) => return other.to_string(),
    };
    let scalar = |val: &Value| -> Option<String> {
        match val {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    };
    let mut parts: Vec<String> = Vec::new();
    let mut lead_key: Option<&str> = None;
    for k in ARGS_PRIMARY {
        if let Some(val) = obj.get(k).and_then(&scalar) {
            if !val.is_empty() {
                parts.push(format!("{k}: {val}"));
                lead_key = Some(k);
                break;
            }
        }
    }
    let mut keys: Vec<&String> = obj
        .keys()
        .filter(|k| Some(k.as_str()) != lead_key)
        .collect();
    keys.sort_by_key(|k| (args_rank(k), k.as_str()));
    for k in keys {
        let val = &obj[k.as_str()];
        let rendered = match scalar(val) {
            Some(s) => s,
            None => serde_json::to_string_pretty(val).unwrap_or_else(|_| val.to_string()),
        };
        parts.push(format!("{k}: {rendered}"));
    }
    parts.join("\n")
}

/// Keys whose value identifies the call — shared by the bounded hint and
/// the full render so both lead with the same fact.
const ARGS_PRIMARY: [&str; 9] = [
    "command",
    "file_path",
    "path",
    "url",
    "query",
    "pattern",
    "prompt",
    "name",
    "id",
];

/// Semantic key order (adversarial review round 2, F1): known pairs get
/// their reading order, the rest stay alphabetical behind them.
fn args_rank(k: &str) -> usize {
    const ORDERED: [&str; 6] = ["old", "new", "start_line", "end_line", "offset", "limit"];
    ORDERED
        .iter()
        .position(|o| *o == k)
        .unwrap_or(ORDERED.len())
}

/// Humane tool-args preview (adversarial design review, 2026-08-19):
/// the FOLDED tool row and the strip must answer "WHICH call was this"
/// at a glance — raw JSON spent a third of the row on syntax, and
/// alphabetical key order pushed the identifying value (`file_path`,
/// `command`) off the edge. Objects render as VALUES: the primary
/// identifier first (bare), the rest as `k=v`, long absolute paths
/// tail-compacted at a '/' boundary so the basename always survives.
///
/// This is a BOUND: `max` chars through `one_line`, plus a 60-char clip
/// per value and path compaction. It is the one-row summary, never the
/// truth — [`tool_args_full`] is what `/details` renders, and the
/// ledger keeps the verbatim arguments behind both.
pub(crate) fn tool_args_preview(v: Option<&Value>, max: usize) -> String {
    let obj = match v {
        None | Some(Value::Null) => return String::new(),
        Some(Value::String(s)) => return one_line(s, max),
        Some(Value::Object(m)) => m,
        Some(other) => return one_line(&other.to_string(), max),
    };
    let scalar = |val: &Value| -> Option<String> {
        match val {
            Value::String(s) => Some(compact_path(s)),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    };
    let mut parts: Vec<String> = Vec::new();
    let mut lead_key: Option<&str> = None;
    for k in ARGS_PRIMARY {
        if let Some(val) = obj.get(k).and_then(&scalar) {
            if !val.is_empty() {
                parts.push(val);
                lead_key = Some(k);
                break;
            }
        }
    }
    let mut keys: Vec<&String> = obj
        .keys()
        .filter(|k| Some(k.as_str()) != lead_key)
        .collect();
    keys.sort_by_key(|k| (args_rank(k), k.as_str()));
    for k in keys {
        let val = &obj[k.as_str()];
        match scalar(val) {
            Some(s) => parts.push(format!("{k}={}", one_line(&s, 60))),
            // Nested values stay honest as compact JSON.
            None => parts.push(format!("{k}={}", one_line(&val.to_string(), 60))),
        }
    }
    one_line(&parts.join("  "), max)
}

/// Tail-compact a long absolute path at a '/' boundary so the basename
/// and as many parent segments as fit survive (the `js/ga…` death of a
/// head-first cut). Same intent as `ui::chrome::tail_ellipsis`,
/// boundary-aware; lives here because the fold owns preview text.
fn compact_path(s: &str) -> String {
    const KEEP: usize = 44;
    if !s.starts_with('/') || s.chars().count() <= KEEP {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let tail: String = chars[chars.len() - KEEP..].iter().collect();
    match tail.find('/') {
        Some(ix) => format!("…{}", &tail[ix..]),
        None => format!("…{tail}"),
    }
}

/// The client-clock anchor for an inflight (llm/tool) started record:
/// BACK-DATED from the record's own `started_at` when it carries one,
/// because reattach/restore replays a mid-execution `started` minutes
/// after the fact — anchoring at replay time under-reported the elapsed
/// ("tool call 3s" during minute 6 of a real scan; adversary P2-3, the
/// same class the run-elapsed clock already fixes from `created_at`).
/// Missing field, unparseable timestamp, or clock skew (record newer
/// than now / farther back than the process epoch) all fall back to
/// `Instant::now()` — conservative: the clock ticks from 0, never lies
/// forward.
/// A short label for a starting tool_calls batch: the first call's name
/// plus a bounded argument preview ("execute_command: npm test …"), or
/// "N tools" for a multi-call batch. Names WHAT is running so the strip
/// can escalate honestly on a long/stuck tool (observability wave).
fn inflight_tool_label(rec: &Value) -> Option<String> {
    let calls = rec
        .get("effect")
        .and_then(|e| e.get("payload"))
        .and_then(|p| p.get("tool_calls"))
        .and_then(Value::as_array)?;
    if calls.len() > 1 {
        return Some(format!("{} tools", calls.len()));
    }
    let c = calls.first()?;
    let name = c.get("name").and_then(Value::as_str).unwrap_or("tool");
    let arg = tool_args_preview(c.get("arguments"), 48);
    if arg.is_empty() {
        Some(name.to_string())
    } else {
        Some(format!("{name}: {arg}"))
    }
}

fn inflight_anchor(rec: &Value) -> std::time::Instant {
    let now = std::time::Instant::now();
    if let Some(ms) = protocol::started_at_epoch_ms(rec) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if now_ms > ms {
            if let Some(anchor) = now.checked_sub(std::time::Duration::from_millis(now_ms - ms)) {
                return anchor;
            }
        }
    }
    now
}

/// The CONTENT fields beside `rendered` — `stdout`, `stderr`,
/// `exit_code`, a `results` array, whatever a tool actually reported
/// alongside its summary line.
///
/// Two rulings meet here and both hold. 2026-08-19: the transport
/// envelope is noise, so the human `rendered` payload LEADS and the
/// wrapper keys (`platform`, `duration_s`, the echoed `command`, a null
/// `error`) never reach the card. 2026-08-20: details truncates
/// nothing — so a field carrying real output can no longer be dropped
/// on the floor the way every sibling of `rendered` used to be
/// (adversarial review F5). The rule is therefore a DENYLIST, not an
/// allowlist: anything unrecognised is content and survives; only the
/// known transport keys, the nulls and the empties are filtered. Empty
/// for a plain `{rendered}` envelope, which is the common case — a
/// normal tool result gains no noise.
fn envelope_rest(v: Option<&Value>) -> String {
    let Some(Value::Object(map)) = v else {
        return String::new();
    };
    /// Transport, not output. `command` is the tool's echo of its own
    /// arguments, which the card already prints in full above.
    const TRANSPORT: [&str; 6] = [
        "rendered",
        "error",
        "success",
        "platform",
        "duration_s",
        "command",
    ];
    let mut parts: Vec<String> = Vec::new();
    for (k, val) in map {
        if TRANSPORT.contains(&k.as_str()) || val.is_null() {
            continue;
        }
        let text = match val {
            Value::String(s) => s.clone(),
            other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
        };
        if text.trim().is_empty() {
            continue;
        }
        parts.push(format!("{k}: {text}"));
    }
    parts.join("\n")
}

/// The tool output as text, in FULL. Bounding is the VIEW's job now
/// (`/details` shows everything; the folded row shows none of this), so
/// nothing is cut on the way into the fold — the operator's rule, 2026-08-20.
fn value_block(v: Option<&Value>) -> String {
    match v {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => String::new(),
        Some(other) => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    }
}

/// The turn's verdict AS THE SERVER WORDED IT (`output.stop_reason`).
///
/// Carried, never composed. This host renders `label` in fixed chrome and
/// `headline`/`remedy` in the card; it does not decide what a stop means,
/// because every other host on this gateway would then have to decide the
/// same thing again and could decide it differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopVerdict {
    /// `final_answer` | `iteration_budget` | `stuck_repeat` | `stuck_oscillation`.
    pub code: String,
    /// The loop finished the task.
    pub finished: bool,
    /// The iteration budget was actually SPENT (false for a stuck-loop stop).
    pub budget_exhausted: bool,
    /// Short line for the status chrome.
    pub label: String,
    /// One sentence for the operator.
    pub headline: String,
    /// What to do about it (may be empty).
    pub remedy: String,
}

#[derive(Default)]
pub struct Fold {
    pub items: Vec<Item>,
    pub stats: Stats,
    pub session: SessionStats,
    pub pending_wait: Option<PendingWait>,
    /// Current status line ("Reading files…"); cleared on waits/terminal.
    pub activity: String,
    /// Cycle counters per run id (agent loops live in subruns).
    cycles: HashMap<String, u32>,
    /// Highest cycle across followed runs — what the activity strip shows.
    pub cycle: u32,
    /// (wait_key, step_id) pairs already surfaced — occurrence identity.
    seen_waits: HashSet<(String, String)>,
    answered_waits: HashSet<(String, String)>,
    seen_call_ids: HashSet<String>,
    followed: HashSet<String>,
    /// sub run -> the run whose waiting record discovered it.
    parents: HashMap<String, String>,
    /// The FIRST-LEVEL agent run (parent == root): answers come from root
    /// or this run — never from deeper delegate children (their flow ends
    /// are intermediate results). Bound STRUCTURALLY at spawn time from
    /// the parent's own ledger declaration (`details.sub_workflow_id` on
    /// the subworkflow wait — see `bind from spawn` in `apply`), with the
    /// first-reason-cycle heuristic kept only as the labeled `#FALLBACK`
    /// for ledgers that predate the declaration.
    agent_run_id: String,
    /// Catalog-declared agent workflow ids — the run-facing form the
    /// gateway mints per catalog entrypoint (`{bundle}@{version}:{flow}`,
    /// abstractgateway routes/gateway.py) and serves verbatim as each
    /// entrypoint's `workflow_id` in `GET /bundles`; the SAME ids appear
    /// as `sub_workflow_id` in parent spawn records. Populated by the
    /// runner at catalog load (`set_agent_workflows`); catalog state, so
    /// it SURVIVES `begin_run`. Empty (unwired) still binds structurally
    /// through the runtime's Agent-node id contract (see
    /// [`VISUAL_REACT_AGENT_PREFIX`]).
    agent_workflow_ids: HashSet<String>,
    /// Children the ledger declared TOOL-MODE at spawn
    /// (`wrap_as_tool_result` — a tool observation for their parent by
    /// contract): never answer-source candidates, even when they cycle.
    /// Load-bearing against delegate children, which run their parent's
    /// OWN workflow id (abstractagent react_runtime.py delegate_agent).
    tool_children: HashSet<String>,
    seen_images: HashSet<String>,
    /// The root run: final answers come from here only.
    root_run_id: String,
    /// The run currently emitting reasoning cycles — the steer target.
    steer_run_id: String,
    /// Still-inflight llm_calls per run id (started, not yet completed/
    /// failed). A single slot was dishonest with parallel lanes: any
    /// OTHER run's completion cleared the hint while a slow call kept
    /// running (coder trees cycle builder + verifier concurrently).
    llm_inflight: HashMap<String, std::time::Instant>,
    /// When the OLDEST still-inflight llm_call started (client clock) —
    /// derived from `llm_inflight`, kept as a plain field because it is
    /// the worker-1 seam the chrome/strip reads. Drives the "model call
    /// Ns" live segment + the ≥60s slow-provider hint; cleared on
    /// completion/terminal/begin_run/rehydrate (F9: it used to survive
    /// run boundaries and label an idle session with a stale call).
    pub llm_inflight_since: Option<std::time::Instant>,
    /// Still-inflight tool_calls batches per run id — the tool twin of
    /// `llm_inflight` (live P0, 2026-07-23: a search_files over an
    /// unignored build tree executed for 8m39s gateway-side while the
    /// strip said only "running search_files" with no clock — the
    /// operator read it as a hang). `waiting` clears (an approval-gated
    /// batch is not EXECUTING while it waits for the user); the
    /// approved-resume window re-arms via `tool_resumed` — called by
    /// the client at its own resume, because the runtime completes the
    /// ORIGINAL step with no second started record.
    tool_inflight: HashMap<String, std::time::Instant>,
    /// When the OLDEST still-inflight tool batch started (client
    /// clock) — drives the "tool call Ns" strip segment + the ≥60s
    /// executing-gateway-side hint. Cleared everywhere
    /// `llm_inflight_since` is (one helper clears both).
    pub tool_inflight_since: Option<std::time::Instant>,
    /// A short label for the CURRENTLY running tool batch (first call's
    /// name + a bounded arg preview, or "N tools" for a batch), so the
    /// strip can NAME what a long-running tool actually is instead of a
    /// static "large scans" guess (observability wave 2026-07-27).
    pub inflight_tool_label: Option<String>,
    /// True once a final answer or terminal error landed.
    pub finished: bool,
    /// True when the ROOT (or answer-source run) recorded a failure.
    pub failed: bool,
    /// The turn STOPPED before finishing, with the iteration count when the
    /// verdict carried one. Named for the FACT, not for one of its causes:
    /// while this was called `budget_exhausted` the exit-code channel
    /// collapsed every unfinished stop into "raise the budget", including
    /// stuck-loop stops the server had explicitly marked
    /// `budget_exhausted: false`. The cause lives in `stop`.
    ///
    /// Separate from `failed` on purpose: nothing failed — the agent was
    /// interrupted mid-task, which is a THIRD outcome beside "finished" and
    /// "errored" and must never be rendered or exit-coded as either. Before
    /// this existed the verdict reached only an `Item::Error` in scrollback
    /// while the fixed chrome line still read `✓ done` and headless `exec`
    /// still exited 0 — so a truncated run was indistinguishable from a
    /// completed one to both the operator and every script, and
    /// `zelda_headless_bench.py` scored it as a PASS.
    pub stopped_short: Option<u64>,
    /// The server's own verdict for this turn, latched for the conclusion
    /// line and the exit code. `None` = the engine predates the contract, and
    /// this host must NOT invent an explanation to fill the gap.
    pub stop: Option<StopVerdict>,
    /// Turn wall clock (set at `begin_run`) — feeds the done summary's
    /// elapsed. Meaningless during ledger REPLAY (`replay` suppresses
    /// the elapsed segment, never the summary itself).
    pub run_started_at: Option<std::time::Instant>,
    /// Set by the rehydrate folds around historical bundles: the done
    /// summary keeps its ledger-true facts (cycles/tools/tokens) but
    /// omits elapsed — fold-time instants are not turn durations.
    pub replay: bool,
    /// The newest reasoning cycle's one-line gist — the strip renders it
    /// beside "thinking (cycle N)" so a long call names its intent
    /// (P2-1). Lifetime rides `activity`: the strip shows it only while
    /// the activity IS a thinking label, so tool/conclusion transitions
    /// hide it without bespoke clears.
    pub cycle_preview: String,
    /// WHICH cycle wrote `cycle_preview` — the RUN it came from and
    /// that run's own cycle number.
    ///
    /// The gist lands with a cycle's RESULT record; the next cycle's
    /// `started` record arrives moments later and moves `activity` to
    /// "thinking (cycle N+1)" — so a gist rendered without this
    /// provenance reads as the CURRENT cycle's intent while being the
    /// PREVIOUS cycle's words (operator report 2026-08-21: cycle 2
    /// quoting cycle 1's opening line).
    ///
    /// The RUN half matters as much as the number: `cycles` counts per
    /// run while `cycle` is a MAX across runs, so in any tree where two
    /// runs cycle — delegate children, and goal loops, which start a
    /// fresh cycling subrun per iteration — comparing a per-run number
    /// against the max attributes one run's words to another's cycle
    /// (adversary finding P1, 2026-08-21). `cycle_gist` compares like
    /// with like or says nothing.
    cycle_preview_of: u32,
    cycle_preview_run: String,
    /// The newest turn's one-line outcome ("completed · 9m14s · …") —
    /// the idle strip renders `last run: …` from it so "did it finish?"
    /// is answered from fixed chrome too, not only at the transcript
    /// tail (visibility review P1-1). Cleared at `begin_run`? NO —
    /// deliberately kept until the NEXT conclusion overwrites it: the
    /// Starting/Running strip branches don't render it, and clearing at
    /// begin_run would erase the answer exactly when a queue drain
    /// starts the next run.
    pub done_note: String,
    /// `/goal` defense (plan item 3, the P0 fix): while set, `finished`
    /// fires ONLY on the root's own flow end / terminal — an agent-subrun
    /// answer-shaped flow end renders as a NON-final card instead of
    /// releasing the composer (goal bundles start one cycling subrun PER
    /// ITERATION; without this the loop reads finished at iteration 1).
    /// Owned by the UI's `wire_goal` effect (`begin_run` never touches
    /// it: the runner's begin_run post races the effect, and the flag's
    /// truth depends only on run identity, which the effect re-derives).
    pub finish_on_root_only: bool,
    truncated_notice_added: bool,
    /// The artifact id whose FINAL-answer card is still a placeholder or
    /// a fetch-failure label — the late-truth reconcile's gate. Set when
    /// the concluding flow end offloaded its output; cleared by a
    /// successful fetch swap, by the reconcile itself, and at
    /// `begin_run`. While set, a LATER answer-source flow end carrying
    /// the INLINE response swaps the card (the wrapper ROOT completes
    /// minutes after the agent subrun with `output.response` inline —
    /// Lane B verified the incident's root held the words the artifact
    /// fetch lost to one transport blip).
    unresolved_offload: Option<String>,
}

/// The attributed form of the newest reasoning gist — see
/// [`Fold::cycle_gist`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CycleGist {
    /// The live cycle's OWN words.
    Own(String),
    /// The newest words from the same cycling run, from an earlier cycle.
    Last(String),
}

impl Fold {
    pub fn new() -> Fold {
        Fold::default()
    }

    pub fn begin_run(&mut self, root_run_id: &str) {
        self.root_run_id = root_run_id.to_string();
        self.steer_run_id.clear();
        self.agent_run_id.clear();
        self.followed.clear();
        self.parents.clear();
        // Run state only: `agent_workflow_ids` is CATALOG state and
        // survives run boundaries (the catalog does not change because a
        // run ended).
        self.tool_children.clear();
        self.followed.insert(root_run_id.to_string());
        self.finished = false;
        self.failed = false;
        // Per-TURN verdict, cleared like `failed`. Leaving it latched made a
        // truncated turn poison every later turn in the session: turn 2 of a
        // clean run still rendered "stopped: iteration budget (N)" in the
        // fixed chrome. A honesty fix that lies in the other direction is
        // still a lying client.
        self.stopped_short = None;
        self.stop = None;
        self.activity.clear();
        self.cycle = 0;
        self.cycles.clear();
        self.stats = Stats::default();
        // Turn clock for the done summary (client wall clock; the strip's
        // elapsed lives in the store — this one survives phase flips).
        self.run_started_at = Some(std::time::Instant::now());
        self.cycle_preview.clear();
        self.cycle_preview_of = 0;
        self.cycle_preview_run.clear();
        self.session.runs += 1;
        self.pending_wait = None;
        // A prior turn's unresolved answer card is history — the new
        // turn must not reconcile into it.
        self.unresolved_offload = None;
        // F9: a prior run/turn that died mid-LLM-call must not arm the
        // "model call Nm — provider may be slow" hint on the new run.
        self.clear_llm_inflight();
        // Per-run dedup state: providers reuse call ids across turns and the
        // runtime reuses wait keys across runs; cross-run replays are already
        // fenced by is_following.
        self.seen_waits.clear();
        self.answered_waits.clear();
        self.seen_call_ids.clear();
    }

    pub fn root_run_id(&self) -> &str {
        &self.root_run_id
    }

    /// Declare the catalog's agent-interface workflow ids: the entrypoint
    /// `workflow_id` fields (run-facing `{bundle}@{version}:{flow}` form)
    /// of every `GET /bundles` entrypoint carrying
    /// `abstractcode.agent.v1`. Replaces the previous set (the catalog is
    /// the truth; reloads re-declare). Wired by the runner at catalog
    /// load; an unwired (empty) set degrades gracefully — the runtime's
    /// Agent-node id contract still binds structurally, and the cycle
    /// `#FALLBACK` covers the rest.
    pub fn set_agent_workflows<I>(&mut self, ids: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.agent_workflow_ids = ids.into_iter().filter(|w| !w.is_empty()).collect();
    }

    /// The declared agent-workflow ids (catalog state) — read back by
    /// session resets so a fold wipe can carry the declarations over
    /// (adversary P2-7: dropping them degraded answer-source binding to
    /// the id-prefix fallback until the next catalog load).
    pub fn agent_workflows(&self) -> impl Iterator<Item = &String> {
        self.agent_workflow_ids.iter()
    }

    /// True when a spawn-declared child workflow id is an AGENT workflow —
    /// derived from structure, never from behavior: the runtime's
    /// deterministic Agent-node id contract ([`VISUAL_REACT_AGENT_PREFIX`])
    /// or membership in the catalog's agent-interface set
    /// (`set_agent_workflows`). Unrecognized ids answer false — such a
    /// child MAY still be an agent (hand-registered react workflows, e.g.
    /// the literal `react_agent` id), which the cycle `#FALLBACK` covers.
    fn is_agent_workflow(&self, workflow_id: &str) -> bool {
        !workflow_id.is_empty()
            && (workflow_id.starts_with(VISUAL_REACT_AGENT_PREFIX)
                || self.agent_workflow_ids.contains(workflow_id))
    }

    /// Bind (or, in goal mode, re-bind) the ANSWER-SOURCE agent run.
    /// First-wins on normal runs — a late second candidate must never
    /// steal the answer lane mid-turn; goal trees (`finish_on_root_only`)
    /// follow the NEWEST candidate because they start one agent subrun
    /// per iteration. Idempotent; callers are the structural spawn
    /// binding and the cycle `#FALLBACK`.
    fn bind_agent_run(&mut self, run_id: &str) {
        if self.agent_run_id.is_empty() || (self.finish_on_root_only && run_id != self.agent_run_id)
        {
            self.agent_run_id = run_id.to_string();
        }
    }

    /// True when records from `run_id` belong to the active run tree —
    /// stale stream threads from a previous run post records that must drop.
    pub fn is_following(&self, run_id: &str) -> bool {
        self.followed.contains(run_id)
    }

    /// The conversation as chat messages for the NEXT run's context —
    /// user prompts + final answers only (thinking/tools/steers are
    /// intra-turn detail). Server-side session replay seeds from COMPLETED
    /// root runs only, and wrapper bundles can leave roots waiting on
    /// helper pollers long after the answer landed (live-verified:
    /// basic-agent@0.0.2 roots still waiting hours later) — so the client
    /// carries its own transcript context; client messages win by the
    /// durable-sessions contract, and the server seed still covers
    /// restarts (empty fold sends nothing).
    ///
    /// Budget discipline mirrors the server seed: whole turns drop from
    /// the oldest side under both caps; the newest turn always survives.
    pub fn chat_messages(&self, max_messages: usize, max_chars: usize) -> Vec<(String, String)> {
        let mut turns: Vec<(String, Option<String>)> = Vec::new();
        for item in &self.items {
            match item {
                Item::User { text } => turns.push((text.clone(), None)),
                Item::Assistant {
                    text,
                    final_answer: true,
                } => {
                    if let Some(last) = turns.last_mut() {
                        if last.1.is_none() {
                            last.1 = Some(text.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        // Complete turns only: a dangling user message is provider-hostile.
        let mut complete: Vec<(String, String)> = turns
            .into_iter()
            .filter_map(|(u, a)| a.map(|a| (u, a)))
            .collect();
        // Drop oldest whole turns under the caps (2 messages per turn).
        loop {
            let msgs = complete.len() * 2;
            let chars: usize = complete.iter().map(|(u, a)| u.len() + a.len()).sum();
            if complete.len() > 1 && (msgs > max_messages || chars > max_chars) {
                complete.remove(0);
            } else {
                break;
            }
        }
        let mut out = Vec::with_capacity(complete.len() * 2);
        for (u, a) in complete {
            out.push(("user".to_string(), u));
            out.push(("assistant".to_string(), a));
        }
        out
    }

    /// What the activity strip may honestly say about the model's words
    /// beside "thinking (cycle N)".
    ///
    /// The ledger carries a cycle's text only in its RESULT record, so
    /// while a cycle is in flight the newest gist the client holds is an
    /// EARLIER one. It is still worth showing — it is what the lane just
    /// decided — but it has to say whose words it is:
    ///
    /// - `Own` — the live cycle's own words (its result landed and no
    ///   later cycle has started): the em-dash reading.
    /// - `Last` — the newest words from the SAME cycling run, from an
    ///   earlier cycle. Deliberately NUMBERLESS: `cycles` counts per run
    ///   while `cycle` is a max across runs, so any number printed here
    ///   would be in a different namespace from the "(cycle N)" beside
    ///   it. "last" is true in both.
    /// - `None` — nothing attributable: no gist, or a gist from a
    ///   DIFFERENT run than the one now cycling (a delegate child's
    ///   words are not this lane's intent).
    pub fn cycle_gist(&self) -> Option<CycleGist> {
        if self.cycle_preview.is_empty() || self.cycle_preview_run.is_empty() {
            return None;
        }
        if self.cycle_preview_run != self.steer_run_id {
            return None;
        }
        let live = *self.cycles.get(&self.steer_run_id).unwrap_or(&0);
        if self.cycle_preview_of == live {
            Some(CycleGist::Own(self.cycle_preview.clone()))
        } else if self.cycle_preview_of < live {
            Some(CycleGist::Last(self.cycle_preview.clone()))
        } else {
            // A gist ahead of the live count: unreachable through the
            // fold's own writes, and not something to guess about.
            None
        }
    }

    /// The run currently emitting reasoning cycles — `None` until the
    /// first reason-cycle record lands (and again after `begin_run`).
    /// Deliberately NO root fallback: a root-targeted steer is a lie for
    /// guidance delivery (wrapper bundles drain guidance in the agent
    /// SUBRUN; the root never folds it), so the pending-steer buffer keys
    /// on this accessor's Some/None truth. (The old `steer_target()`
    /// root-fallback twin had no production caller left and was deleted —
    /// cycle-3 whole-system audit.)
    pub fn cycling_target(&self) -> Option<String> {
        if self.steer_run_id.is_empty() {
            None
        } else {
            Some(self.steer_run_id.clone())
        }
    }

    pub fn push_item(&mut self, item: Item) {
        self.items.push(item);
        // Truncate in CHUNKS, not per push: at the cap, a per-push drain
        // shifted every index on every batch, and the feed sync (keyed by
        // index) re-rendered ~all items per batch — O(N²) typesetting on
        // the UI thread (adversary finding 8, 2026-07-22). Chunked drains
        // keep indices stable between drains (pure-append fast path); a
        // drain shrinks the list, which the feed sync answers with one
        // rebuild — unless the same OBSERVED batch also appends enough to
        // refill the length, in which case the fast path re-renders every
        // shifted index in place (fingerprint mismatch), which is
        // order-correct at the same cost (see wire_feed; pinned by
        // headless_ui::truncation_drains_keep_the_feed_in_sync_with_fold_order).
        // Amortized either way: one full re-render per TRUNCATE_CHUNK pushes.
        if self.items.len() > MAX_ITEMS + TRUNCATE_CHUNK {
            if !self.truncated_notice_added {
                self.truncated_notice_added = true;
                self.items.insert(
                    0,
                    Item::Info {
                        // A client action, not a storage claim (operator
                        // ruling 2026-07-26): scroll-to-top / `/history`
                        // stream earlier turns back from the gateway.
                        text: "[#TRUNCATION] older transcript items dropped from view — scroll to the top (or /history) to stream earlier turns back".into(),
                    },
                );
            }
            // Drop overflow from just after the standing notice at index 0.
            let overflow = self.items.len() - MAX_ITEMS;
            self.items.drain(1..1 + overflow);
        }
    }

    /// True once older items were dropped from view (the standing
    /// `[#TRUNCATION]` notice is in). `/export` reads this so a truncated
    /// transcript's export declares its incompleteness in the header.
    pub fn truncated(&self) -> bool {
        self.truncated_notice_added
    }

    /// The user answered (or the runner optimistically resumed) a wait.
    pub fn wait_answered(&mut self, wait_key: &str, step_id: &str) {
        self.answered_waits
            .insert((wait_key.to_string(), step_id.to_string()));
        if let Some(w) = &self.pending_wait {
            if w.wait_key == wait_key {
                self.pending_wait = None;
            }
        }
    }

    /// The client resumed an APPROVED tool batch: execution re-enters
    /// gateway-side, but the runtime completes the ORIGINAL step — there
    /// is no second `started` record to re-arm the tool clock from. The
    /// CLIENT is the actor that knows the execution window reopened (it
    /// sent the resume), so it re-arms here (adversary P1-1: the
    /// permissions belt resumes approval waits without a modal,
    /// making gated batches indistinguishable from the no-wait path —
    /// without this, an approved 10-minute build recreates the exact
    /// clockless-hang misread this feature exists to kill). Denials
    /// never re-arm; a REFUSED resume rolls back via `reopen_wait`.
    pub fn tool_resumed(&mut self, run_id: &str) {
        // Symmetry with reopen_wait's guard (cycle-3 hardening): a stale
        // modal's decide for a run this fold no longer follows must not
        // arm a clock the current turn would render.
        if !self.is_following(run_id) || self.finished {
            return;
        }
        self.tool_inflight
            .insert(run_id.to_string(), std::time::Instant::now());
        self.tool_inflight_since = self.tool_inflight.values().min().copied();
    }

    /// A resume FAILED after optimistic clearing: the run is still waiting
    /// server-side, so restore the prompt and let the user retry. Guarded:
    /// a stale restore from a PREVIOUS run must never clobber the current
    /// run's pending wait (the runner also guards by is_following).
    pub fn reopen_wait(&mut self, wait: PendingWait) {
        if !self.is_following(&wait.run_id) {
            return;
        }
        // Roll back the optimistic `tool_resumed` re-arm: the gateway
        // refused the resume, so the batch is still PARKED, not executing.
        self.tool_inflight.remove(&wait.run_id);
        self.tool_inflight_since = self.tool_inflight.values().min().copied();
        self.answered_waits
            .remove(&(wait.wait_key.clone(), wait.step_id.clone()));
        if !self.finished && self.pending_wait.is_none() {
            self.pending_wait = Some(wait);
        }
    }

    /// Mark every awaiting-approval tool card as approved/denied. No wait
    /// key parameter on purpose: tool cards carry no wait identity, and
    /// only ONE wait is ever pending (`pending_wait` is a single slot) —
    /// every awaiting card on screen belongs to the wait being answered.
    /// A silently-ignored key parameter here would read as key-scoped
    /// marking that the implementation cannot deliver.
    pub fn mark_wait_tools(&mut self, approved: bool) {
        for item in self.items.iter_mut().rev() {
            if let Item::Tool { status, error, .. } = item {
                if *status == ToolStatus::AwaitingApproval {
                    if approved {
                        *status = ToolStatus::Running;
                    } else {
                        *status = ToolStatus::Denied;
                        if error.is_empty() {
                            *error = "denied by user".into();
                        }
                    }
                }
            }
        }
    }

    /// Fold one ledger record from `source_run_id`. Returns side requests.
    pub fn apply(&mut self, source_run_id: &str, rec: &Value) -> Vec<FoldEffect> {
        let mut effects = Vec::new();
        let status = protocol::record_status(rec);
        let etype = protocol::effect_type(rec);
        let node_id = protocol::record_node_id(rec);
        let rec_run = {
            let rid = protocol::record_run_id(rec);
            if rid.is_empty() {
                source_run_id.to_string()
            } else {
                rid
            }
        };

        // Slow-call visibility: any followed run's inflight llm_call arms
        // the strip hint. Tracked PER RUN: completion clears only that
        // run's entry — a parallel lane's fast call must not hide a slow
        // call still running elsewhere (the derived `llm_inflight_since`
        // is the oldest survivor).
        if etype == "llm_call" {
            match status.as_str() {
                // The `!finished` gate (cycle-3 P3, stop-race residual): a
                // started that folds in the same batch that concluded the
                // turn, whose lane then dies to StopFollows before its
                // completed folds, must not tick into the next turn's
                // Starting window. Post-conclusion records carry no
                // user-visible clock anyway (phase Idle).
                "started" if !self.finished => {
                    self.llm_inflight
                        .insert(rec_run.clone(), inflight_anchor(rec));
                }
                "completed" | "failed" => {
                    self.llm_inflight.remove(&rec_run);
                }
                _ => {}
            }
            self.llm_inflight_since = self.llm_inflight.values().min().copied();
        }

        // The tool twin (live P0, 2026-07-23): a tool batch that executes
        // for minutes gateway-side must carry a visible clock — "running
        // search_files" alone read as a client hang while the ledger later
        // proved 8m39s of real execution. Same per-run shape as llm_call;
        // `waiting` clears (approval-gated batches are not executing —
        // the approved-resume re-arm is `tool_resumed`, called by the
        // client at the moment it resumes, because the runtime completes
        // the ORIGINAL step with no second started record).
        if etype == "tool_calls" {
            match status.as_str() {
                // Same `!finished` gate as the llm twin (stop-race).
                "started" if !self.finished => {
                    self.tool_inflight
                        .insert(rec_run.clone(), inflight_anchor(rec));
                    // Capture WHAT is running so a long tool names itself on
                    // the strip (observability wave 2026-07-27: the 8h-hang
                    // showed "large scans can take minutes" for hours while
                    // the real command — a wedged server+browser probe — was
                    // never named; a labeled command is the first cue a human
                    // needs to see "this is stuck", the model-blocked twin of
                    // the operator's own diagnosis).
                    self.inflight_tool_label = inflight_tool_label(rec);
                }
                "waiting" | "completed" | "failed" => {
                    self.tool_inflight.remove(&rec_run);
                }
                _ => {}
            }
            self.tool_inflight_since = self.tool_inflight.values().min().copied();
            if self.tool_inflight.is_empty() {
                self.inflight_tool_label = None;
            }
        }

        // --- reasoning cycles -------------------------------------------------
        if etype == "llm_call" && node_id == "reason" && status == "started" {
            let n = self.cycles.entry(rec_run.clone()).or_insert(0);
            *n += 1;
            self.cycle = self.cycle.max(*n);
            self.activity = format!("thinking (cycle {})", self.cycle);
            self.steer_run_id = rec_run.clone();
            // #FALLBACK answer-source binding — the cycle heuristic,
            // demoted (conformance lane, 2026-07-23): the PRIMARY binding
            // is STRUCTURAL and happens at the spawn record (see the
            // waits section — the parent's ledger declares the child's
            // workflow id), so an agent child binds from birth and one
            // that dies before its first cycle still concludes the turn.
            // This path remains only for ledgers whose spawn records
            // predate the declaration fields: the first-level run that
            // EMITS reasoning cycles is taken as the agent. Ledger-
            // declared TOOL-MODE children never bind — a structural fact
            // beats cycling behavior (delegate children cycle while
            // running their parent's own workflow id).
            //
            // Unknown parent (partial replays without the discovery
            // record) is treated as first-level: production always learns
            // parents through the subworkflow waits that discover
            // followed runs, so unknown-parent = root-attached in
            // practice.
            //
            // Goal trees (finish_on_root_only) start one first-level
            // cycling subrun PER ITERATION: there the lane FOLLOWS the
            // live iteration, or iteration 2+ results and ctx/model
            // telemetry would fold from a dead run (cycle-3 whole-system
            // audit). Normal runs keep first-wins (a late second
            // first-level cycler must never steal the answer lane
            // mid-turn). Both rules live in `bind_agent_run`.
            let first_level = self
                .parents
                .get(&rec_run)
                .map(|p| *p == self.root_run_id)
                .unwrap_or(true);
            if first_level && !self.tool_children.contains(&rec_run) {
                self.bind_agent_run(&rec_run);
            }
        }
        if let Some(cycle) = protocol::cycle_result_from_record(rec) {
            if node_id == "reason" {
                let n = *self.cycles.get(&rec_run).unwrap_or(&1);
                // The strip names the cycle's INTENT (visibility review
                // P2-1): "thinking (cycle 30)" alone said nothing about
                // what a 4-minute call was attempting — the model's own
                // words are the honest label. Newest cycle wins; the
                // one-liner clears with `activity` at conclusion paths.
                let gist = one_line(
                    if cycle.content.trim().is_empty() {
                        &cycle.reasoning
                    } else {
                        &cycle.content
                    },
                    CYCLE_PREVIEW_MAX,
                );
                // Only the CYCLING lane's words are kept. A result
                // from another run (a delegate child, or a `completed`
                // whose `started` never folded in a partial replay)
                // would otherwise clobber the slot and blank the strip
                // — or worse, be rendered as this lane's intent
                // (adversary finding P1, 2026-08-21).
                if !gist.is_empty() && rec_run == self.steer_run_id {
                    self.cycle_preview = gist;
                    self.cycle_preview_of = n.max(1);
                    self.cycle_preview_run = rec_run.clone();
                }
                self.push_item(Item::Thinking {
                    iteration: n.max(1),
                    content: cycle.content.clone(),
                    reasoning: cycle.reasoning.clone(),
                    call: CallCost::from_record(rec),
                });
            }
        }
        // Cumulative token totals fold from EVERY followed run (the tree's
        // real spend), but the "latest" fields — served model + live context
        // size — are honest only from the ANSWER-SOURCE lane: a delegate
        // child's tiny call must not relabel the header or the ctx chip
        // (adversary finding: delegate pollution).
        //
        // Deep-cycling trees (live coder run 5f810f81…: every cycling run
        // sits at depth 2–3, so no FIRST-LEVEL agent ever binds): with the
        // lane restricted to root/agent, ctx + model stayed dead for the
        // whole 5-hour run. When NO first-level agent exists, the run
        // currently emitting reasoning cycles is the honest "latest"
        // source; once a first-level agent binds, deeper children still
        // never relabel (the pollution rule stands).
        let telemetry_lane = rec_run == self.root_run_id
            || (!self.agent_run_id.is_empty() && rec_run == self.agent_run_id)
            || (self.agent_run_id.is_empty()
                && !self.steer_run_id.is_empty()
                && rec_run == self.steer_run_id);
        if let Some(usage) = protocol::usage_from_record(rec) {
            // The call's own duration rides along: /cache reports model
            // time and throughput, and the only place the wall clock for
            // one call exists is the record that carried its usage.
            self.fold_usage(
                usage,
                telemetry_lane,
                protocol::gen_time_ms_from_record(rec),
            );
        }
        if telemetry_lane {
            if let Some(model) = protocol::model_from_record(rec) {
                self.stats.effective_model = model;
            }
        }

        // --- tool calls -------------------------------------------------------
        if etype == "tool_calls" && status == "started" {
            let calls = rec
                .get("effect")
                .and_then(|e| e.get("payload"))
                .and_then(|p| p.get("tool_calls"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for (i, tc) in calls.iter().enumerate() {
                if let Some(view) = protocol::tool_call_view(tc) {
                    self.upsert_tool_started(
                        &rec_run,
                        &node_id,
                        i,
                        &view.name,
                        view.call_id,
                        view.arguments.as_ref(),
                    );
                }
            }
            if !calls.is_empty() {
                let names: Vec<String> = calls
                    .iter()
                    .filter_map(|tc| protocol::tool_call_view(tc).map(|v| v.name))
                    .collect();
                self.activity = format!("running {}", names.join(", "));
            }
        }
        if etype == "tool_calls" && status == "completed" {
            let views = protocol::tool_results_from_record(rec);
            let views_len = views.len() as u64;
            for (i, view) in views.into_iter().enumerate() {
                self.finish_tool(&rec_run, &node_id, i, view);
            }
            // Count from the payload list when present; terminal records may
            // carry a `$slim` marker instead (runtime ledger dedup), where
            // the result views are the only honest count.
            let payload_len = rec
                .get("effect")
                .and_then(|e| e.get("payload"))
                .and_then(|p| p.get("tool_calls"))
                .and_then(Value::as_array)
                .map(|a| a.len() as u64)
                .unwrap_or(0);
            self.stats.tool_calls += payload_len.max(views_len);
        }

        // --- emitted events ---------------------------------------------------
        if let Some(ev) = protocol::extract_emit_event(rec) {
            match ev.name.as_str() {
                "abstract.status" => {
                    let text = protocol::status_text_from_payload(ev.payload.as_ref());
                    let lower = text.to_lowercase();
                    if text.is_empty()
                        || matches!(
                            lower.as_str(),
                            "ready" | "completed" | "done" | "finished" | "cancelled" | "failed"
                        )
                    {
                        // Reference semantics: an empty/ready status CLEARS
                        // the activity line instead of leaving stale text.
                        // Terminal-sounding texts clear too: wrapper-bundle
                        // helpers emit `{"value": "Done"}` per round
                        // (live coder run — "Done · cycle 12 · 17880s"
                        // read as concluded while the tree kept working).
                        self.activity.clear();
                    } else {
                        self.activity = text;
                    }
                }
                "abstract.message" => {
                    let (text, level) = match &ev.payload {
                        Some(Value::String(s)) => (s.trim().to_string(), String::new()),
                        Some(v) if v.is_object() => {
                            let t = ["text", "message", "value"]
                                .iter()
                                .map(|k| v.get(k).and_then(Value::as_str).unwrap_or("").trim())
                                .find(|x| !x.is_empty())
                                .unwrap_or("")
                                .to_string();
                            let level = v
                                .get("level")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .trim()
                                .to_lowercase();
                            (t, level)
                        }
                        _ => (String::new(), String::new()),
                    };
                    if !text.is_empty() {
                        match level.as_str() {
                            "error" => self.push_item(Item::Error { text }),
                            "warning" | "warn" => self.push_item(Item::Info {
                                text: format!("warning: {text}"),
                            }),
                            _ => self.push_item(Item::Assistant {
                                text,
                                final_answer: false,
                            }),
                        }
                    }
                }
                // The documented UI-event tool contract (abstractcode
                // docs/ui_events.md): workflow-lane flows emit tool activity
                // as events rather than tool_calls effects.
                "abstract.tool_execution" => {
                    let items = match &ev.payload {
                        Some(Value::Array(a)) => a.clone(),
                        Some(v) if v.is_object() => vec![v.clone()],
                        _ => Vec::new(),
                    };
                    for (i, it) in items.iter().enumerate() {
                        let name = ["tool", "name"]
                            .iter()
                            .map(|k| it.get(k).and_then(Value::as_str).unwrap_or("").trim())
                            .find(|x| !x.is_empty())
                            .unwrap_or("")
                            .to_string();
                        if name.is_empty() {
                            continue;
                        }
                        let call_id = ["call_id", "id"]
                            .iter()
                            .map(|k| it.get(k).and_then(Value::as_str).unwrap_or("").trim())
                            .find(|x| !x.is_empty())
                            .unwrap_or("")
                            .to_string();
                        let args = it
                            .get("arguments")
                            .or_else(|| it.get("args"))
                            .or_else(|| it.get("params"))
                            .or_else(|| it.get("parameters"));
                        self.upsert_tool_started(&rec_run, "evt", i, &name, call_id, args);
                    }
                }
                "abstract.tool_result" => {
                    let items = match &ev.payload {
                        Some(Value::Array(a)) => a.clone(),
                        Some(v) if v.is_object() => vec![v.clone()],
                        _ => Vec::new(),
                    };
                    for (i, it) in items.iter().enumerate() {
                        let name = ["tool", "name"]
                            .iter()
                            .map(|k| it.get(k).and_then(Value::as_str).unwrap_or("").trim())
                            .find(|x| !x.is_empty())
                            .unwrap_or("")
                            .to_string();
                        if name.is_empty() {
                            continue;
                        }
                        let call_id = ["call_id", "id"]
                            .iter()
                            .map(|k| it.get(k).and_then(Value::as_str).unwrap_or("").trim())
                            .find(|x| !x.is_empty())
                            .unwrap_or("")
                            .to_string();
                        let view = protocol::ToolResultView {
                            name,
                            call_id,
                            arguments: it.get("arguments").or_else(|| it.get("args")).cloned(),
                            success: it.get("success").and_then(Value::as_bool),
                            error: it
                                .get("error")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .trim()
                                .to_string(),
                            output: it
                                .get("output")
                                .or_else(|| it.get("result"))
                                .or_else(|| it.get("response"))
                                .cloned(),
                        };
                        self.finish_tool(&rec_run, "evt", i, view);
                        self.stats.tool_calls += 1;
                    }
                }
                "abstract.media.image.generated" => {
                    if let Some(payload) = &ev.payload {
                        if let Some(artifact) = payload.get("image_artifact") {
                            let aid = artifact
                                .get("$artifact")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            if !aid.is_empty() && self.seen_images.insert(aid.clone()) {
                                let prompt = payload
                                    .get("prompt")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .trim()
                                    .to_string();
                                self.push_item(Item::Image {
                                    run_id: rec_run.clone(),
                                    artifact_id: aid.clone(),
                                    label: if prompt.is_empty() {
                                        "generated image".into()
                                    } else {
                                        one_line(&prompt, 80)
                                    },
                                });
                                effects.push(FoldEffect::FetchImage {
                                    run_id: rec_run.clone(),
                                    artifact_id: aid,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // --- answer_user effect (system messages) -----------------------------
        if etype == "answer_user" && status == "completed" {
            let result = rec.get("result");
            let payload = rec.get("effect").and_then(|e| e.get("payload"));
            let msg = [result, payload]
                .iter()
                .flatten()
                .flat_map(|v| ["message", "text", "content"].iter().map(move |k| (v, k)))
                .map(|(v, k)| v.get(*k).and_then(Value::as_str).unwrap_or("").trim())
                .find(|x| !x.is_empty())
                .unwrap_or("")
                .to_string();
            let level = [result, payload]
                .iter()
                .flatten()
                .map(|v| v.get("level").and_then(Value::as_str).unwrap_or("").trim())
                .find(|x| !x.is_empty())
                .unwrap_or("")
                .to_lowercase();
            if !msg.is_empty() {
                match level.as_str() {
                    "error" => self.push_item(Item::Error { text: msg.clone() }),
                    "warning" | "warn" => self.push_item(Item::Info {
                        text: format!("warning: {msg}"),
                    }),
                    _ => {
                        // Workflow PROGRESS lines (flow's committed stable
                        // prefix "build cycle N of M", multiagent-coding
                        // 0.0.7) ALSO drive the strip's activity — the
                        // glance surface — not just a transcript card that
                        // scrolls away behind tool output. This is the
                        // operator's "never a surprise" for the fix budget:
                        // the current cycle is visible at a glance, then the
                        // builder's own reasoning cycle overwrites it, and
                        // conclusion clears it. Keyed on the prefix so real
                        // intermediate answers stay transcript-only.
                        if msg.starts_with("build cycle ") {
                            self.activity = one_line(&msg, CYCLE_PREVIEW_MAX);
                        }
                        self.push_item(Item::Assistant {
                            text: msg.clone(),
                            final_answer: false,
                        });
                    }
                }
            }
        }

        // --- waits ------------------------------------------------------------
        if status == "waiting" {
            if let Some(wait) = protocol::extract_wait(rec) {
                if let Some(spawn) = protocol::subworkflow_spawn(rec) {
                    let sub = spawn.sub_run_id.clone();
                    if self.followed.insert(sub.clone()) {
                        self.parents.insert(sub.clone(), rec_run.clone());
                        effects.push(FoldEffect::FollowRun(sub.clone()));
                    }
                    // STRUCTURAL answer-source binding (the maintainer's
                    // rule: the ledger already knows — never guess): the
                    // parent's own wait record DECLARES the child's
                    // workflow (`details.sub_workflow_id`; the effect
                    // payload carries the same required id). When the
                    // spawning parent is the ROOT and the declared
                    // workflow is an agent workflow (`is_agent_workflow`:
                    // the runtime's Agent-node id contract, or the
                    // catalog's agent-interface set), the child IS the
                    // answer source from birth — an agent that dies
                    // BEFORE its first reason cycle is already bound and
                    // `subrun_terminal` concludes the turn honestly.
                    // Ledger-declared TOOL-MODE children
                    // (wrap_as_tool_result — delegate_agent and friends)
                    // are remembered and can never bind, even via the
                    // cycle #FALLBACK: they run their parent's own
                    // workflow id, and their flow ends are tool
                    // observations by contract.
                    if spawn.wrap_as_tool_result {
                        self.tool_children.insert(sub);
                    } else if rec_run == self.root_run_id
                        && self.is_agent_workflow(&spawn.workflow_id)
                    {
                        self.bind_agent_run(&sub);
                    }
                } else {
                    let step_id = rec
                        .get("step_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    self.consider_wait(&rec_run, &step_id, wait);
                }
            }
        } else if let Some(w) = &self.pending_wait {
            // Any later record from the waiting run means it progressed past
            // the wait (answered elsewhere, auto-approved, or resumed after a
            // replayed ledger): clear the stale prompt. Records arrive in
            // ledger order, so ordering is the correctness argument here
            // (mirrors the reference client's resolve_blocking_wait).
            if w.run_id == rec_run {
                self.pending_wait = None;
            }
        }

        // --- final output / errors --------------------------------------------
        // The answer arrives on the ROOT's flow end — or on the FIRST-LEVEL
        // agent subrun's flow end (the spawn-declared agent child, or the
        // cycle-#FALLBACK-bound run on pre-declaration ledgers). Wrapper
        // bundles keep helper subflows (status watchers) running after the
        // agent answered, so waiting for root completion alone can block
        // the turn forever (live-verified on basic-agent@0.0.2). Deeper
        // cycling runs (delegate_agent children) produce INTERMEDIATE
        // results for their parent — never the answer.
        //
        // `is_run_output_record` gates eligibility: only the run's OWN
        // terminal record carries its output — a SYNC subworkflow's
        // completion record carries the CHILD's output on the parent's
        // ledger and must never read as the parent's answer.
        let answer_source = rec_run == self.root_run_id
            || (!self.agent_run_id.is_empty() && rec_run == self.agent_run_id);
        if answer_source
            && !self.finished
            && status == "completed"
            && protocol::is_run_output_record(rec)
        {
            let out = protocol::extract_flow_output(rec);
            // A final can be text, artifacts, or both (reference parity:
            // the assistant finishes on meta-only outputs too).
            let has_media_meta = out
                .as_ref()
                .and_then(|o| o.meta.as_ref())
                .map(|m| {
                    [
                        "image_artifact",
                        "video_artifact",
                        "audio_artifact",
                        "music_artifact",
                    ]
                    .iter()
                    .any(|k| m.get(*k).is_some())
                })
                .unwrap_or(false);
            let usable = out
                .as_ref()
                .map(|o| !o.response.is_empty() || has_media_meta || o.offload_artifact.is_some())
                .unwrap_or(false);
            if usable {
                let out = out.expect("usable implies Some");
                // Only trust flow-end shaped nodes: the reference clients
                // accept any record carrying result.output; agent flows
                // only produce it at the end node of the run.
                //
                // OFFLOADED ANSWER (the "never finishes" P0, live run
                // c61e4ac9…): outputs over the runtime's inline cap
                // persist as `{"$artifact": id}` — the turn CONCLUDES NOW
                // on a placeholder card and the artifact content is
                // fetched to swap the real words in (a failed fetch
                // labels the card; the composer is never held hostage
                // by the fetch).
                let text = if !out.response.is_empty() {
                    out.response.clone()
                } else if let Some(aid) = &out.offload_artifact {
                    offload_placeholder(aid)
                } else {
                    "(the run produced media output)".to_string()
                };
                // Goal defense (finish_on_root_only): a SUBRUN's
                // answer-shaped end is an ITERATION RESULT — render it
                // non-final and keep the turn open; only the root's own
                // end concludes. Images still fetch (interim artifacts
                // are real output either way).
                let concludes = !self.finish_on_root_only || rec_run == self.root_run_id;
                self.push_item(Item::Assistant {
                    text,
                    final_answer: concludes,
                });
                if let Some(aid) = &out.offload_artifact {
                    if concludes {
                        // Arm the late-truth reconcile: until the fetch
                        // (or a later inline flow end) resolves this
                        // card, it holds placeholder/failure text.
                        self.unresolved_offload = Some(aid.clone());
                    }
                    effects.push(FoldEffect::FetchAnswer {
                        run_id: rec_run.clone(),
                        artifact_id: aid.clone(),
                    });
                }
                if let Some(meta) = &out.meta {
                    if let Some(artifact) = meta.get("image_artifact") {
                        let aid = artifact
                            .get("$artifact")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if !aid.is_empty() && self.seen_images.insert(aid.clone()) {
                            self.push_item(Item::Image {
                                run_id: rec_run.clone(),
                                artifact_id: aid.clone(),
                                label: "generated image".into(),
                            });
                            effects.push(FoldEffect::FetchImage {
                                run_id: rec_run.clone(),
                                artifact_id: aid,
                            });
                        }
                    }
                }
                if concludes {
                    self.finished = true;
                    self.activity.clear();
                    self.pending_wait = None;
                    // Conclusion-by-answer is the COMMON end in wrapper
                    // trees (roots park on pollers; run_terminal never
                    // fires) — a dangling inflight clock here rendered
                    // "tool call 14m" into the NEXT turn's Starting
                    // window (adversary P2-2).
                    self.clear_llm_inflight();
                    // The loop's own verdict decides the WORD. A run that
                    // exhausted its iteration budget mid-task must not read
                    // as "done" — that is the client claiming completion the
                    // loop never claimed.
                    self.push_verdict_note(rec);
                    self.push_done_summary("completed");
                } else if rec_run == self.steer_run_id {
                    // An ITERATION's own end (the goal lane): its
                    // guidance inbox dies with the run — clear the
                    // cycling target so a steer typed between
                    // iterations BUFFERS until the next iteration's
                    // first cycle instead of riding silently into a
                    // dead run (cycle-3 audit, cell (e): a terminal
                    // run never ticks again, so an injected steer
                    // there is folded by no one).
                    self.steer_run_id.clear();
                }
            } else if protocol::is_flow_end_record(rec) {
                // The answer-lane run's own COMPLETION record
                // (`result.completed == true` — written only by the
                // runtime's terminal appenders) with NO readable output:
                // conclude HONESTLY instead of ignoring the end of the
                // run. Without this, a flow whose final output carries
                // no conventional text key left `finished` unset forever
                // while the wrapper root kept waiting on its status
                // poller — the composer-captured-for-hours shape.
                let concludes = !self.finish_on_root_only || rec_run == self.root_run_id;
                if concludes {
                    self.push_item(Item::Info {
                        text: "the run completed without a readable final answer — /status shows the run; /history replays this session".into(),
                    });
                    self.finished = true;
                    self.activity.clear();
                    self.pending_wait = None;
                    self.clear_llm_inflight();
                    // The loop's own verdict decides the WORD. A run that
                    // exhausted its iteration budget mid-task must not read
                    // as "done" — that is the client claiming completion the
                    // loop never claimed.
                    self.push_verdict_note(rec);
                    self.push_done_summary("completed");
                } else if rec_run == self.steer_run_id {
                    self.steer_run_id.clear();
                }
            }
        }

        // LATE-TRUTH RECONCILE (Lane B fix, 2026-07-23 incident): the turn
        // concluded on an OFFLOADED answer whose card still holds
        // placeholder or fetch-failure text, and a LATER answer-source
        // flow end carries the response INLINE — wrapper ROOTS complete
        // minutes after the agent subrun, and in the live incident the
        // root's `output.response` held exactly the words one transport
        // blip lost. Swap the card in place; never push a second final.
        // Works on live streams AND ledger replay (rehydrate folds
        // through this same path in chronological order).
        if answer_source
            && self.finished
            && status == "completed"
            && protocol::is_run_output_record(rec)
        {
            if let Some(aid) = self.unresolved_offload.clone() {
                let inline = protocol::extract_flow_output(rec)
                    .map(|o| o.response)
                    .unwrap_or_default();
                if !inline.is_empty() && self.swap_answer_card(&aid, inline.clone()) {
                    self.unresolved_offload = None;
                }
            }
        }

        if status == "failed" {
            let err = protocol::error_from_record(rec);
            self.push_item(Item::Error { text: err });
            if rec_run == self.root_run_id {
                self.finished = true;
                // `failed` was declared but never set (exec's exit code and
                // the queue's pause-on-failure both read it).
                self.failed = true;
                self.activity.clear();
                self.pending_wait = None;
                self.clear_llm_inflight();
                self.push_done_summary("failed");
            }
        }

        effects
    }

    /// The terminal summary marker — ONE loud, durable "did it finish?"
    /// artifact pushed at every conclusion point (visibility review
    /// P1-1: exec prints `done: status · N llm calls · …`; the TUI
    /// printed nothing — the wall just stopped, and the pty harness
    /// could find no structural done-needle on the whole screen).
    /// Exactly-once by the same `finished` guards that already fence
    /// the conclusion sites. Facts are fold-truth (ledger-derived);
    /// elapsed is the client turn clock, omitted under replay.
    /// Surface the loop's own machine-readable verdict BEFORE the done line,
    /// when it says something the "✓ done" line would hide.
    ///
    /// Two cases, both from `abstractagent/adapters/react_runtime.py`:
    /// `outcome: "iteration_budget"` (the `max_iterations_node` fired — the
    /// loop STOPPED, it did not FINISH) and `review_skipped: true` (the
    /// verifier pass was asked for and could not run, so nothing checked the
    /// answer). Ignoring both is how a truncated run reads as a completed
    /// one.
    fn push_verdict_note(&mut self, rec: &Value) {
        let verdict = match protocol::run_verdict(rec) {
            Some(v) if !v.is_unremarkable() => v,
            _ => return,
        };
        if verdict.stopped_short() {
            // Latch it so the CONCLUSION word and the exit code can tell the
            // truth too — an error card at the transcript tail is not where
            // "did it finish?" gets answered.
            self.stopped_short = Some(verdict.iterations.unwrap_or(0));
            if verdict.has_stop_reason() {
                self.stop = Some(StopVerdict {
                    code: verdict.stop_code.clone(),
                    finished: verdict.stop_finished,
                    budget_exhausted: verdict.stop_budget_exhausted,
                    label: verdict.stop_label.clone(),
                    headline: verdict.stop_headline.clone(),
                    remedy: verdict.stop_remedy.clone(),
                });
                let mut text = verdict.stop_headline.clone();
                if !verdict.stop_remedy.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&verdict.stop_remedy);
                }
                if !text.is_empty() {
                    self.push_item(Item::Error { text });
                }
            } else {
                // LEGACY ENGINE (pre-`stop_reason`). Report the enum and
                // nothing more: this host cannot tell an exhausted budget
                // from a stuck-loop stop — both arrive as
                // `outcome: "iteration_budget"` — and guessing is how it came
                // to advise raising a budget that was never spent.
                self.stop = None;
                let iters = verdict
                    .iterations
                    .map(|n| format!(" after {n} iterations"))
                    .unwrap_or_default();
                self.push_item(Item::Error {
                    text: format!(
                        "the agent STOPPED{iters}, it did not finish — this engine reports no \
                         stop reason, so there is nothing more this client can tell you about why."
                    ),
                });
            }
        }
        for notice in &verdict.notices {
            // The SERVER decides how loud a caveat is. Flattening every
            // notice to Info made a `warn` and an `error` look identical
            // here and different in the next host to render the same run.
            let item = match notice.severity.as_str() {
                "error" | "critical" | "fatal" => Item::Error {
                    text: notice.text.clone(),
                },
                _ => Item::Info {
                    text: notice.text.clone(),
                },
            };
            self.push_item(item);
        }
        if verdict.review_skipped && verdict.notices.is_empty() {
            // Legacy only: engines that send `notices` say this themselves,
            // in their own words, to every host at once.
            self.push_item(Item::Info {
                text: "#FALLBACK: the verifier pass was requested but did not run — \
                       this answer was not checked against the tool outputs."
                    .into(),
            });
        }
    }

    fn push_done_summary(&mut self, outcome: &str) {
        // A budget-stopped run reports "completed" at the RUN level — correct
        // there, wrong at the turn level, because the loop stopped mid-task.
        // The verdict overrides the word so the one line the operator reads
        // from fixed chrome (`last run: …`, sourced from `done_note`) cannot
        // say "done" about work that was cut off.
        let (glyph, word) = match (outcome, self.stopped_short) {
            ("completed", Some(iters)) => (
                "⚠",
                match self.stop.as_ref() {
                    // The server worded it; this host repeats it.
                    Some(v) if !v.label.is_empty() => v.label.clone(),
                    // Legacy engine: the enum, with no cause attached to it.
                    _ if iters > 0 => format!("stopped after {iters} iterations"),
                    _ => "stopped".to_string(),
                },
            ),
            ("completed", None) => ("✓", "done".to_string()),
            ("cancelled", _) => ("⊘", "cancelled".to_string()),
            ("unknown", _) => ("✗", "ended (status unknown)".to_string()),
            (other, _) => ("✗", other.to_string()),
        };
        let mut parts: Vec<String> = Vec::new();
        if !self.replay {
            if let Some(t0) = self.run_started_at {
                parts.push(crate::convo::fmt_elapsed(t0.elapsed().as_secs()));
            }
        }
        if self.stats.llm_calls > 0 {
            parts.push(format!("{} llm calls", self.stats.llm_calls));
        }
        if self.stats.tool_calls > 0 {
            if self.stats.tool_failures > 0 {
                parts.push(format!(
                    "{} tools ({} ✗)",
                    self.stats.tool_calls, self.stats.tool_failures
                ));
            } else {
                parts.push(format!("{} tools", self.stats.tool_calls));
            }
        }
        if self.stats.input_tokens > 0 || self.stats.output_tokens > 0 {
            parts.push(format!(
                "{}↑ {}↓ tk",
                crate::ui::chrome::fmt_tokens(self.stats.input_tokens),
                crate::ui::chrome::fmt_tokens(self.stats.output_tokens)
            ));
        } else if self.stats.total_tokens > 0 {
            parts.push(format!(
                "{} tk",
                crate::ui::chrome::fmt_tokens(self.stats.total_tokens)
            ));
        }
        let tail = if parts.is_empty() {
            String::new()
        } else {
            format!(" · {}", parts.join(" · "))
        };
        let note = format!("{word}{tail}");
        self.done_note = note.clone();
        self.push_item(Item::Info {
            text: format!("{glyph} {note}"),
        });
    }

    /// The runner observed the root run reach a terminal status.
    pub fn run_terminal(&mut self, status: &str) {
        self.activity.clear();
        self.pending_wait = None;
        self.clear_llm_inflight();
        // Restamp unfinished tool rows (adversarial review round 2,
        // F3): once the tree is terminal, nothing will ever complete
        // them — a row left saying "running" in scrollback is a
        // present-tense lie. "interrupted" is the honest past tense:
        // the run ended before the result was observed.
        for item in self.items.iter_mut() {
            if let Item::Tool { status, .. } = item {
                if matches!(*status, ToolStatus::Running | ToolStatus::AwaitingApproval) {
                    *status = ToolStatus::Interrupted;
                }
            }
        }
        if !self.finished {
            match status {
                "completed" => {}
                "cancelled" => self.push_item(Item::Info {
                    text: "run cancelled".into(),
                }),
                // F4: the stream ended but the final status could not be
                // read (gateway restarting / token expired mid-run). Say
                // exactly that — the old path fabricated "completed" and
                // drained the queue against a dead gateway.
                "unknown" => {
                    self.push_item(Item::Error {
                        text: "run ended but the final status could not be read from the gateway — check the connection (/doctor)".into(),
                    });
                    self.failed = true;
                }
                other => {
                    self.push_item(Item::Error {
                        text: format!("run ended: {other}"),
                    });
                    self.failed = true;
                }
            }
            self.finished = true;
            self.push_done_summary(status);
        }
    }

    /// The runner observed a FOLLOWED SUBRUN reach a terminal status.
    ///
    /// The P0 this exists for (live tree 76fc3fcb…/9c5cad22…, 2026-07-22):
    /// the ANSWER-SOURCE agent subrun terminally FAILED ("Model
    /// unloaded." at cycle 1) — the wrapper root absorbed the failure and
    /// parked forever on its status poller, so neither the root-failed
    /// path nor the root-terminal path could ever conclude the turn: the
    /// composer stayed captured for 15+ hours. The subrun's own terminal
    /// status is the missing conclusion signal.
    ///
    /// Keyed on RUN status, never on failed effect records: effect
    /// failures retry (`attempt` N) and can be absorbed
    /// (`_absorb_failure`) without killing the run — only the run's
    /// terminal state is conclusive. Helper subruns (pollers, status
    /// emitters) and goal iterations (`finish_on_root_only`) never
    /// conclude from here: helpers are absorbed by design, and a goal
    /// root decides its own loop's fate.
    pub fn subrun_terminal(&mut self, run_id: &str, status: &str) {
        // An UNREADABLE status never concludes a turn from a subrun — a
        // transient status-read failure must not kill a healthy run.
        // Both callers guard this today (`finish()` early-returns, exec
        // matches real terminal statuses); the fold enforces it too so
        // the invariant is structural, not caller etiquette (cycle-2
        // review F5). FIRST, before any mutation: an unknown status
        // proves nothing, so it must not drop inflight entries either
        // (cycle-3 P3: the removal leaked around this guard). The ROOT's
        // "unknown" stays an honest failure in `run_terminal` — there
        // the whole turn is unobservable.
        if status == "unknown" {
            return;
        }
        // A terminal subrun can never be mid-call again: drop ITS inflight
        // clock entries BEFORE the remaining early returns — a cancelled
        // lane (or a goal iteration under finish_on_root_only) whose
        // ledger ended on a dangling `started` would otherwise tick on
        // the strip forever (adversary P2-2: the early returns skipped
        // per-run cleanup).
        self.llm_inflight.remove(run_id);
        self.llm_inflight_since = self.llm_inflight.values().min().copied();
        self.tool_inflight.remove(run_id);
        self.tool_inflight_since = self.tool_inflight.values().min().copied();
        if self.finished || self.finish_on_root_only {
            return;
        }
        // Only the bound answer-source agent run concludes the turn. The
        // binding is STRUCTURAL — the parent's spawn record declares the
        // child's workflow at birth (see `apply`'s waits section) — so an
        // agent child that dies BEFORE its first reason cycle is already
        // bound and concludes here (the cycle heuristic survives only as
        // the labeled #FALLBACK for pre-declaration ledgers). An unbound
        // run id here is a helper (or a tree whose ledger never declared
        // an agent child), which keeps waiting on the root paths.
        if self.agent_run_id.is_empty() || run_id != self.agent_run_id {
            return;
        }
        match status {
            // A completed answer-source is handled by the flow-output /
            // flow-end paths in `apply` — reaching here means the run
            // ended without ANY readable conclusion record. Say so and
            // free the composer (the alternative is the forever-spinner).
            "completed" => {
                self.push_item(Item::Info {
                    text: "the agent run completed without a readable final answer — /status shows the run; /history replays this session".into(),
                });
            }
            "cancelled" => {
                self.push_item(Item::Info {
                    text: "agent run cancelled".into(),
                });
            }
            other => {
                self.push_item(Item::Error {
                    text: format!(
                        "the agent run ended: {other} — the turn cannot produce an answer (details above; the wrapper run may keep polling on the gateway)"
                    ),
                });
                self.failed = true;
            }
        }
        self.finished = true;
        self.activity.clear();
        self.pending_wait = None;
        self.clear_llm_inflight();
        self.push_done_summary(status);
    }

    /// The bound answer-source agent run, when one exists and is not the
    /// root — the run whose terminal status concludes the turn
    /// (`subrun_terminal`). Exec's polling loop watches it alongside the
    /// root.
    pub fn answer_run_id(&self) -> Option<&str> {
        if self.agent_run_id.is_empty() || self.agent_run_id == self.root_run_id {
            None
        } else {
            Some(&self.agent_run_id)
        }
    }

    /// Drop every armed slow-call hint (run boundaries: begin_run,
    /// terminal, rehydration — F9: a prior turn that died mid-LLM-call
    /// must never label an idle session with "model call Nm").
    ///
    /// (Cycle-3 note: the fold's record-truth OBS-1a-live accessor pair
    /// — `live_llm_call()` epoch-ms start + `last_call_rate()` from
    /// provider `gen_time` — was removed as a dead second rate
    /// authority: chrome deliberately renders the client-clock twins
    /// (`llm_inflight_since` + `store.last_call_rate`), which are
    /// monotonic and thus skew-proof. The record-truth parsers survive
    /// in `protocol::started_at_epoch_ms`/`gen_time_ms_from_record` if
    /// a gen_time-truth rate is ever wired.)
    pub fn clear_llm_inflight(&mut self) {
        self.llm_inflight.clear();
        self.llm_inflight_since = None;
        // The tool clock clears at exactly the same boundaries (begin_run /
        // terminal / rehydrate / runner reset) — one helper, both twins,
        // so no boundary can clear one and leak the other.
        self.tool_inflight.clear();
        self.tool_inflight_since = None;
        self.inflight_tool_label = None;
    }

    /// Swap the offloaded-answer card's text. Two-pass matcher: the EXACT
    /// placeholder for `artifact_id` first (content-addressed — immune to
    /// neighboring cards), then the newest fetch-failure label (stable
    /// prefix — the artifact id is deliberately absent from failure text,
    /// so the prefix is the only handle). Safe across run boundaries; a
    /// no-match (cleared session, truncated transcript, already-resolved
    /// card) is a clean no-op. Returns whether a card was swapped.
    fn swap_answer_card(&mut self, artifact_id: &str, new_text: String) -> bool {
        let placeholder = offload_placeholder(artifact_id);
        for item in self.items.iter_mut().rev() {
            if let Item::Assistant { text, .. } = item {
                if *text == placeholder {
                    *text = new_text;
                    return true;
                }
            }
        }
        for item in self.items.iter_mut().rev() {
            if let Item::Assistant { text, .. } = item {
                if text.starts_with(OFFLOAD_FAILURE_PREFIX) {
                    *text = new_text;
                    return true;
                }
            }
        }
        false
    }

    /// Swap an offloaded-answer placeholder for the fetched words (or an
    /// honest failure label). On failure the label is NEUTRAL and
    /// URL-free (`offload_failure_label` — the 2026-07-23 fetch_url
    /// incident), and `unresolved_offload` stays armed so a later
    /// answer-source flow end carrying the inline response can still
    /// land the real words (the late-truth reconcile in `apply`).
    pub fn resolve_offloaded_answer(&mut self, artifact_id: &str, outcome: Result<String, String>) {
        match outcome {
            Ok(t) => {
                if self.swap_answer_card(artifact_id, t.clone())
                    && self.unresolved_offload.as_deref() == Some(artifact_id)
                {
                    self.unresolved_offload = None;
                }
            }
            Err(reason) => {
                self.swap_answer_card(artifact_id, offload_failure_label(&reason));
            }
        }
    }

    fn fold_usage(&mut self, usage: UsageDelta, telemetry_lane: bool, gen_time_ms: Option<f64>) {
        self.stats.llm_calls += 1;
        self.stats.input_tokens += usage.input_tokens;
        self.stats.output_tokens += usage.output_tokens;
        self.stats.total_tokens += usage.total_tokens;
        self.stats.cached_tokens += usage.cached_tokens;
        self.session.llm_calls += 1;
        self.session.cached_tokens += usage.cached_tokens;
        // "Did the cache miss?" vs "does this provider report hits at
        // all?" — one call that reported a hit proves the provider talks,
        // so a later zero is a real miss. Without this counter both look
        // identical (a 0 that means nothing was said).
        if usage.cached_tokens > 0 {
            self.stats.cache_reported_calls += 1;
            self.session.cache_reported_calls += 1;
        }
        // Denominator for every hit/reuse rate: only calls that actually
        // reported an input count. A zero-poisoned split contributes to
        // NEITHER side, so a ratio can never be inflated by a call the
        // provider described in totals only.
        if usage.input_tokens > 0 {
            self.stats.cacheable_input_tokens += usage.input_tokens;
            self.session.cacheable_input_tokens += usage.input_tokens;
        }
        let gen_ms = gen_time_ms
            .filter(|ms| *ms > 0.0)
            .map(|ms| ms.round() as u64)
            .unwrap_or(0);
        self.stats.gen_time_ms += gen_ms;
        self.session.gen_time_ms += gen_ms;
        if telemetry_lane && usage.input_tokens > 0 {
            // The live "context used" number: the agent lane's newest call.
            // Keep the previous one first — their difference is the new-vs-carried
            // split the cache panel reports (see `Stats::prev_input_tokens`).
            self.stats.prev_input_tokens = self.stats.last_input_tokens;
            self.stats.last_input_tokens = usage.input_tokens;
            self.stats.last_input_is_estimate = false;
        } else if telemetry_lane && usage.total_tokens > 0 {
            // Zero-poisoned split (abstractcore's usage normalization
            // zero-fills when a relay answers in Responses-API spelling;
            // cycle-2 forensics 2026-07-23 — the relay itself reported
            // the split fine): refusing the update kept the PREVIOUS
            // call's number on screen — the live incident froze
            // "ctx 4.0k" while the wire carried ~137k (the frozen meter
            // corroborated a wrong hypothesis for a full investigation
            // cycle). Derive the honest estimate instead: total − output
            // (≈ input; over-states by at most the output share when
            // output is also unreported), and mark it so chrome renders
            // "~" — an estimate labeled, never a stale number presented
            // as fresh.
            self.stats.prev_input_tokens = self.stats.last_input_tokens;
            self.stats.last_input_tokens = usage.total_tokens.saturating_sub(usage.output_tokens);
            self.stats.last_input_is_estimate = true;
        }
        if telemetry_lane {
            // Latest-call facts. Set on the lane that owns "latest"
            // everywhere else in this fold (root/agent), so /cache's
            // newest-call block can never describe a delegate's tiny call.
            self.stats.last_cached_tokens = usage.cached_tokens;
            self.stats.last_output_tokens = usage.output_tokens;
            self.stats.last_gen_time_ms = gen_ms;
            let last = self.stats.last_input_tokens;
            let prev = self.stats.prev_input_tokens;
            // NEW vs CARRIED, accumulated. The per-call split (see
            // `Stats::prev_input_tokens`) is only a snapshot; summing it
            // is what answers "how much of everything this session sent
            // was a reusable prefix?" — the question a prompt cache is
            // bought to answer, and one no provider reports for you.
            if last > 0 {
                if prev == 0 {
                    self.stats.new_tokens += last;
                    self.session.new_tokens += last;
                } else if last >= prev {
                    self.stats.new_tokens += last - prev;
                    self.stats.carried_tokens += prev;
                    self.session.new_tokens += last - prev;
                    self.session.carried_tokens += prev;
                } else {
                    // The context SHRANK: the prefix a cache had built is
                    // gone, so none of this prompt is credited as carried.
                    self.stats.context_resets += 1;
                    self.session.context_resets += 1;
                    self.stats.new_tokens += last;
                    self.session.new_tokens += last;
                }
                self.stats.peak_input_tokens = self.stats.peak_input_tokens.max(last);
                self.session.peak_input_tokens = self.session.peak_input_tokens.max(last);
            }
        }
        self.session.input_tokens += usage.input_tokens;
        self.session.output_tokens += usage.output_tokens;
        self.session.total_tokens += usage.total_tokens;
        // Sparkline food: output per call. SPLITLESS usage (input and
        // output both 0 with a non-zero total — the coder-run provider
        // shape) substitutes the call's total so the sparkline still
        // shows per-call activity instead of a flat zero line. A split
        // provider's legitimately-empty response (input > 0) is never
        // substituted — a total spike there would mislabel prompt size
        // as output.
        let series_val = if usage.input_tokens == 0 && usage.output_tokens == 0 {
            usage.total_tokens as f32
        } else {
            usage.output_tokens as f32
        };
        self.stats.output_series.push(series_val);
        if self.stats.output_series.len() > 64 {
            let n = self.stats.output_series.len() - 64;
            self.stats.output_series.drain(0..n);
        }
    }

    /// Card identity for a tool call.
    ///
    /// The id branch is scoped by `run_id` for the same reason the positional branch
    /// always was: a card key is only safe as a global namespace if the id inside it
    /// is globally unique, and a correlation id is unique per RUN, not per session.
    /// Unscoped, two calls sharing an id anywhere in the tree (a retry, a sub-agent)
    /// collapse onto one card — the second call renders no card and its result
    /// overwrites the first's, so a card shows another call's output under its own
    /// name and arguments.
    fn tool_key(run_id: &str, node_id: &str, index: usize, call_id: &str) -> String {
        if !call_id.is_empty() {
            format!("call:{run_id}:{call_id}")
        } else {
            format!("pos:{run_id}:{node_id}:{index}")
        }
    }

    fn upsert_tool_started(
        &mut self,
        run_id: &str,
        node_id: &str,
        index: usize,
        name: &str,
        call_id: String,
        args: Option<&Value>,
    ) {
        let key = Self::tool_key(run_id, node_id, index, &call_id);
        // Track the run-scoped KEY, not the bare id: the set decides "does a card
        // already exist for this call?", and card identity is the key.
        if !call_id.is_empty() && !self.seen_call_ids.insert(key.clone()) {
            // Already have a card (from the approval wait); flip it to running.
            for item in self.items.iter_mut().rev() {
                if let Item::Tool { key: k, status, .. } = item {
                    if *k == key && *status == ToolStatus::AwaitingApproval {
                        *status = ToolStatus::Running;
                    }
                    if *k == key {
                        return;
                    }
                }
            }
            return;
        }
        if call_id.is_empty() {
            // Id-less providers: an approval wait may already have minted a
            // card under a different positional key — flip the OLDEST
            // same-name awaiting card instead of duplicating.
            for item in self.items.iter_mut() {
                if let Item::Tool {
                    name: n,
                    status,
                    args_preview: ap,
                    args_full: af,
                    ..
                } = item
                {
                    if n == name && *status == ToolStatus::AwaitingApproval {
                        *status = ToolStatus::Running;
                        // BOTH copies move together (adversarial review
                        // 2026-08-20, F2): updating only the hint left
                        // `/details` showing the PRE-rewrite arguments —
                        // the full view was less truthful than the summary.
                        *ap = tool_args_preview(args, ARGS_PREVIEW_MAX);
                        *af = tool_args_full(args);
                        return;
                    }
                }
            }
        }
        self.push_item(Item::Tool {
            key,
            name: name.to_string(),
            args_preview: tool_args_preview(args, ARGS_PREVIEW_MAX),
            args_full: tool_args_full(args),
            status: ToolStatus::Running,
            result: String::new(),
            error: String::new(),
        });
    }

    fn finish_tool(
        &mut self,
        run_id: &str,
        node_id: &str,
        index: usize,
        view: protocol::ToolResultView,
    ) {
        let key = Self::tool_key(run_id, node_id, index, &view.call_id);
        // Offloaded tool outputs (server audit follow-up 4): outputs
        // >256 KB are append-time offloaded to the artifact store and
        // the ledger carries a `$artifact` ref — rendering the raw ref
        // JSON reads as garbage (and a ref-bearing preview later riding
        // context is the 2026-07-23 instruction-kit class). Name it.
        let result = match view.output.as_ref().and_then(|o| {
            o.get("$artifact")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        }) {
            Some(aid) => format!(
                "(large output stored as artifact {}…)",
                &aid[..aid.len().min(8)]
            ),
            // Relevance (adversarial design review, 2026-08-19): a
            // result object whose runtime envelope carries a human
            // `rendered` string IS the payload — it LEADS, ahead of the
            // transport JSON around it ("error": null, "platform":
            // "Darwin"). But leading is not the same as dropping: the
            // remaining fields used to vanish here with no notice
            // (adversarial review 2026-08-20, F5), so a tool reporting
            // `rendered` plus `stdout`/`exit_code` lost everything but
            // the summary line. They now follow, labeled, and the view
            // shows them in details like any other body.
            None => match view
                .output
                .as_ref()
                .and_then(|o| o.get("rendered"))
                .and_then(Value::as_str)
                .filter(|r| !r.trim().is_empty())
            {
                Some(rendered) => {
                    let mut body = rendered.to_string();
                    let rest = envelope_rest(view.output.as_ref());
                    if !rest.is_empty() {
                        body.push_str("\n— envelope —\n");
                        body.push_str(&rest);
                    }
                    body
                }
                None => value_block(view.output.as_ref()),
            },
        };
        let status = if !view.error.is_empty() || view.success == Some(false) {
            ToolStatus::Failed
        } else {
            ToolStatus::Ok
        };
        if status == ToolStatus::Failed {
            // Failure streaks become visible from fixed chrome
            // ("38 tools · 5 ✗") instead of only as ✗ cards scrolling
            // past in the wall (visibility review P2-2).
            self.stats.tool_failures += 1;
        }
        // Newest matching card by exact key first…
        for item in self.items.iter_mut().rev() {
            if let Item::Tool {
                key: k,
                status: st,
                result: rp,
                error,
                ..
            } = item
            {
                if *k == key {
                    *st = status;
                    *rp = result;
                    *error = view.error.clone();
                    return;
                }
            }
        }
        // …else the OLDEST unfinished same-name card (id-less providers run
        // calls in issue order, so oldest-first keeps results aligned).
        if view.call_id.is_empty() {
            for item in self.items.iter_mut() {
                if let Item::Tool {
                    name,
                    status: st,
                    result: rp,
                    error,
                    ..
                } = item
                {
                    if *name == view.name
                        && matches!(*st, ToolStatus::Running | ToolStatus::AwaitingApproval)
                    {
                        *st = status;
                        *rp = result;
                        *error = view.error.clone();
                        return;
                    }
                }
            }
        }
        // No started card (replay from mid-stream): append a finished one.
        if !view.call_id.is_empty() {
            self.seen_call_ids.insert(key.clone());
        }
        self.push_item(Item::Tool {
            key,
            name: view.name,
            args_preview: tool_args_preview(view.arguments.as_ref(), ARGS_PREVIEW_MAX),
            args_full: tool_args_full(view.arguments.as_ref()),
            status,
            result,
            error: view.error.clone(),
        });
    }

    fn consider_wait(&mut self, run_id: &str, step_id: &str, wait: &Value) {
        let wk = protocol::wait_key(wait);
        // Occurrence identity: the runtime reuses stable wait keys (e.g.
        // `user:{run}:{node}` for repeated asks), so a repeated key with a
        // NEW step id is a NEW question and must re-prompt. Replays carry
        // the same step id and stay deduplicated.
        let occurrence = (wk.clone(), step_id.to_string());
        if wk.is_empty()
            || self.seen_waits.contains(&occurrence)
            || self.answered_waits.contains(&occurrence)
        {
            return;
        }
        let tool_calls = protocol::tool_calls_from_wait(wait);
        if protocol::is_tool_approval_wait(wait) || !tool_calls.is_empty() {
            self.seen_waits.insert(occurrence.clone());
            // Surface the pause on the tool cards. The runtime may record the
            // tool_calls effect as STARTED before pausing on approval, so a
            // card can already exist for a call id — flip it instead of
            // duplicating (live-verified ordering on basic-agent).
            for (i, tc) in tool_calls.iter().enumerate() {
                if let Some(view) = protocol::tool_call_view(tc) {
                    let key = Self::tool_key(run_id, "approval", i, &view.call_id);
                    let args_preview = tool_args_preview(view.arguments.as_ref(), ARGS_PREVIEW_MAX);
                    // `tool_key` is the single authority for card identity. This site
                    // used to hand-roll `call:{id}` to find the card `upsert_tool_started`
                    // had made — which only worked while both spellings were unscoped.
                    if !view.call_id.is_empty() && !self.seen_call_ids.insert(key.clone()) {
                        let call_key = key.clone();
                        let mut flipped = false;
                        for item in self.items.iter_mut().rev() {
                            if let Item::Tool {
                                key: k,
                                status,
                                args_preview: ap,
                                args_full: af,
                                ..
                            } = item
                            {
                                if *k == call_key {
                                    if *status == ToolStatus::Running {
                                        *status = ToolStatus::AwaitingApproval;
                                    }
                                    // The wait carries the FINAL (rewritten)
                                    // arguments — the truth of what will run.
                                    // Both copies move (F2): this is the
                                    // APPROVAL path, so a stale full view
                                    // would show the operator a command
                                    // that is not the one about to run.
                                    *ap = args_preview.clone();
                                    *af = tool_args_full(view.arguments.as_ref());
                                    flipped = true;
                                    break;
                                }
                            }
                        }
                        if flipped {
                            continue;
                        }
                    }
                    self.push_item(Item::Tool {
                        key,
                        name: view.name,
                        args_preview,
                        args_full: tool_args_full(view.arguments.as_ref()),
                        status: ToolStatus::AwaitingApproval,
                        result: String::new(),
                        error: String::new(),
                    });
                }
            }
            self.activity = "waiting for tool approval".into();
            self.pending_wait = Some(PendingWait {
                run_id: run_id.to_string(),
                wait_key: wk,
                step_id: step_id.to_string(),
                kind: WaitKind::Approval { tool_calls },
            });
            return;
        }
        if protocol::is_ask_user_wait(wait) {
            self.seen_waits.insert(occurrence);
            let prompt = {
                let p = protocol::wait_prompt(wait);
                if p.is_empty() {
                    "Input required:".to_string()
                } else {
                    p
                }
            };
            self.activity = "waiting for your answer".into();
            self.pending_wait = Some(PendingWait {
                run_id: run_id.to_string(),
                wait_key: wk,
                step_id: step_id.to_string(),
                kind: WaitKind::Ask {
                    prompt,
                    gate: protocol::ask_wait_kind(wait),
                },
            });
        }
    }
}

/// Correction note: the approval-wait tool key uses node "approval"; the
/// started record uses the real node id, so key-based flip relies on call_id
/// (which both sides carry in practice). The name+status fallback in
/// `finish_tool` covers id-less providers.
#[cfg(test)]
mod tests {
    use super::*;

    /// Two distinct calls must never collapse onto one card.
    ///
    /// Regression: `tool_key` keyed the id branch as a GLOBAL `call:{id}` namespace
    /// and `tool_call_view` preferred the MODEL-supplied `call_id` (observed as bare
    /// repeating integers) over the run-unique `runtime_call_id`. Two `read_file`
    /// calls sharing an id then collapsed: the second minted no card, and its result
    /// overwrote the first's — one card labelled with call A's arguments while
    /// displaying call B's output, with both files genuinely read.
    #[test]
    fn colliding_call_ids_do_not_merge_tool_cards() {
        let record = json!({
            "run_id": "root", "node_id": "n1", "status": "completed",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"name": "read_file", "arguments": {"file_path": "Core/Self_Model.md"},
                 "call_id": "1", "runtime_call_id": "rtcall_abc_1"},
                {"name": "read_file", "arguments": {"file_path": "Semantic/Critical_Insights.md"},
                 "call_id": "1", "runtime_call_id": "rtcall_abc_2"}
            ]}},
            "result": {"results": [
                {"name": "read_file", "call_id": "1", "runtime_call_id": "rtcall_abc_1",
                 "success": true, "output": "CONTENT-OF-SELF-MODEL"},
                {"name": "read_file", "call_id": "1", "runtime_call_id": "rtcall_abc_2",
                 "success": true, "output": "CONTENT-OF-CRITICAL-INSIGHTS"}
            ]}
        });

        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply("root", &record);

        let cards: Vec<(String, String)> = fold
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Tool {
                    args_preview,
                    result,
                    ..
                } => Some((args_preview.clone(), result.clone())),
                _ => None,
            })
            .collect();

        assert_eq!(cards.len(), 2, "each call gets its own card: {cards:?}");
        for (args, result) in &cards {
            // The whole point: the file named in the arguments is the file whose
            // content the card shows.
            if args.contains("Self_Model") {
                assert!(
                    result.contains("SELF-MODEL"),
                    "card shows another call's output: {cards:?}"
                );
            } else {
                assert!(
                    result.contains("CRITICAL-INSIGHTS"),
                    "card shows another call's output: {cards:?}"
                );
            }
        }
    }

    /// The same correlation id in two different runs is two different calls.
    #[test]
    fn same_call_id_across_runs_stays_separate() {
        let mk = |run: &str, path: &str, out: &str| {
            json!({
                "run_id": run, "node_id": "n1", "status": "completed",
                "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                    {"name": "read_file", "arguments": {"file_path": path}, "call_id": "dup"}
                ]}},
                "result": {"results": [
                    {"name": "read_file", "call_id": "dup", "success": true, "output": out}
                ]}
            })
        };
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply("root", &mk("root", "a.md", "OUTPUT-A"));
        fold.apply("root", &mk("sub", "b.md", "OUTPUT-B"));

        let n = fold
            .items
            .iter()
            .filter(|i| matches!(i, Item::Tool { .. }))
            .count();
        assert_eq!(n, 2, "a shared id in a different run is a different call");
    }

    #[test]
    fn done_summary_marks_every_conclusion() {
        // Answer conclusion: "✓ done" with the run's facts, done_note
        // mirrors it for the idle strip (visibility review P1-1).
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "n", "status": "completed",
                    "effect": {"type": "llm_call", "payload": {}},
                    "result": {"content": "hi", "usage": {"input_tokens": 100, "output_tokens": 20}}}),
        );
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "completed", "node_id": "end",
                    "result": {"output": {"response": "answer"}}}),
        );
        assert!(fold.finished);
        let summary = fold
            .items
            .iter()
            .rev()
            .find_map(|i| match i {
                Item::Info { text } if text.starts_with("✓ done") => Some(text.clone()),
                _ => None,
            })
            .expect("the ✓ done marker lands in the transcript");
        assert!(summary.contains("llm calls"), "facts ride it: {summary}");
        assert!(!fold.done_note.is_empty(), "the idle strip copy is set");
        // Failed root: "✗ failed" AFTER the error card.
        let mut f2 = Fold::new();
        f2.begin_run("root");
        f2.apply(
            "root",
            &json!({"run_id": "root", "status": "failed", "node_id": "n",
                    "error": "exploded"}),
        );
        assert!(f2
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text.starts_with("✗ failed"))));
        // Replay omits elapsed (fold-time instants are not durations)
        // but keeps the ledger-true facts.
        let mut f3 = Fold::new();
        f3.begin_run("root");
        f3.replay = true;
        f3.apply(
            "root",
            &json!({"run_id": "root", "status": "completed", "node_id": "end",
                    "result": {"output": {"response": "a"}}}),
        );
        let s3 = f3
            .items
            .iter()
            .rev()
            .find_map(|i| match i {
                Item::Info { text } if text.starts_with("✓ done") => Some(text.clone()),
                _ => None,
            })
            .expect("replayed turns get the marker too");
        assert!(
            !s3.contains("0s"),
            "replay never renders fold-time as duration: {s3}"
        );
        // Exactly-once: the late root-terminal report changes nothing.
        let n = fold.items.len();
        fold.run_terminal("completed");
        assert_eq!(fold.items.len(), n, "conclusion guards keep it once");
    }

    /// The newest item that is NOT the terminal done-summary marker —
    /// conclusion tests assert the answer/error CONTENT; the summary
    /// itself is pinned by `done_summary_marks_every_conclusion`.
    fn last_content(fold: &Fold) -> &Item {
        fold.items
            .iter()
            .rev()
            .find(|i| {
                !matches!(i, Item::Info { text }
                    if text.starts_with("✓ ") || text.starts_with("✗ ") || text.starts_with("⊘ "))
            })
            .expect("at least one non-summary item")
    }
    use serde_json::json;

    fn rec_started_tools(run: &str, calls: Value) -> Value {
        json!({"run_id": run, "node_id": "act", "status": "started",
               "effect": {"type": "tool_calls", "payload": {"tool_calls": calls}}})
    }

    fn rec_completed_tools(run: &str, calls: Value, results: Value) -> Value {
        json!({"run_id": run, "node_id": "act", "status": "completed",
               "effect": {"type": "tool_calls", "payload": {"tool_calls": calls}},
               "result": {"results": results}})
    }

    #[test]
    fn tool_lifecycle_updates_in_place() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &rec_started_tools(
                "root",
                json!([{"name": "read_file", "call_id": "c1", "arguments": {"path": "x"}}]),
            ),
        );
        assert_eq!(fold.items.len(), 1);
        fold.apply(
            "root",
            &rec_completed_tools(
                "root",
                json!([{"name": "read_file", "call_id": "c1"}]),
                json!([{"call_id": "c1", "success": true, "output": "data"}]),
            ),
        );
        assert_eq!(fold.items.len(), 1);
        match &fold.items[0] {
            Item::Tool { status, result, .. } => {
                assert_eq!(*status, ToolStatus::Ok);
                assert!(result.contains("data"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(fold.stats.tool_calls, 1);
    }

    /// The approval card must show the arguments that will ACTUALLY run.
    ///
    /// The runtime may REWRITE a call before asking for approval, and the
    /// wait record carries the final form. The folded hint was updated on
    /// that flip while the full copy kept the original (adversarial
    /// review 2026-08-20, F2) — so `/details`, the view an operator opens
    /// to decide, showed a command that was not the one about to run.
    /// Both copies move together.
    #[test]
    fn a_rewritten_call_updates_both_the_hint_and_the_full_arguments() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "tools", "status": "started",
                "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                    {"id": "c1", "name": "execute_command",
                     "arguments": {"command": "rm -rf /tmp/ORIGINAL"}}]}}}),
        );
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "act", "status": "waiting",
                "result": {"wait": {"reason": "job", "wait_key": "tool_approval:1",
                    "details": {"mode": "approval_required",
                        "tool_calls": [{"name": "execute_command", "call_id": "c1",
                            "arguments": {"command": "rm -rf /tmp/REWRITTEN"}}]}}}}),
        );
        let (hint, full) = fold
            .items
            .iter()
            .find_map(|i| match i {
                Item::Tool {
                    args_preview,
                    args_full,
                    ..
                } => Some((args_preview.clone(), args_full.clone())),
                _ => None,
            })
            .expect("the tool card");
        assert!(
            hint.contains("REWRITTEN") && !hint.contains("ORIGINAL"),
            "the folded hint carries the rewritten call: {hint}"
        );
        assert!(
            full.contains("REWRITTEN") && !full.contains("ORIGINAL"),
            "the DETAILS copy carries the rewritten call too: {full}"
        );

        // Same law on the id-less flip: an approval card matched by NAME
        // when a started record carries no call id.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "act", "status": "waiting",
                "result": {"wait": {"reason": "job", "wait_key": "tool_approval:2",
                    "details": {"mode": "approval_required",
                        "tool_calls": [{"name": "execute_command",
                            "arguments": {"command": "rm -rf /tmp/ORIGINAL"}}]}}}}),
        );
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "tools", "status": "started",
                "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                    {"name": "execute_command",
                     "arguments": {"command": "rm -rf /tmp/REWRITTEN"}}]}}}),
        );
        let (hint, full) = fold
            .items
            .iter()
            .find_map(|i| match i {
                Item::Tool {
                    args_preview,
                    args_full,
                    status: ToolStatus::Running,
                    ..
                } => Some((args_preview.clone(), args_full.clone())),
                _ => None,
            })
            .expect("the flipped tool card");
        assert!(
            hint.contains("REWRITTEN") && !hint.contains("ORIGINAL"),
            "id-less flip updates the hint: {hint}"
        );
        assert!(
            full.contains("REWRITTEN") && !full.contains("ORIGINAL"),
            "id-less flip updates the DETAILS copy: {full}"
        );
    }

    #[test]
    fn approval_wait_then_started_flips_to_running_without_duplicate() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        let wait_rec = json!({"run_id": "sub", "node_id": "act", "status": "waiting",
            "result": {"wait": {"reason": "job", "wait_key": "tool_approval:1",
                "details": {"mode": "approval_required",
                            "tool_calls": [{"name": "write_file", "call_id": "c9", "arguments": {"path": "a"}}]}}}});
        fold.apply("sub", &wait_rec);
        assert!(matches!(
            fold.pending_wait.as_ref().unwrap().kind,
            WaitKind::Approval { .. }
        ));
        assert_eq!(fold.items.len(), 1);

        fold.wait_answered("tool_approval:1", "");
        fold.mark_wait_tools(true);
        assert!(fold.pending_wait.is_none());

        fold.apply(
            "sub",
            &rec_started_tools(
                "sub",
                json!([{"name": "write_file", "call_id": "c9", "arguments": {"path": "a"}}]),
            ),
        );
        assert_eq!(
            fold.items.len(),
            1,
            "no duplicate card on started after approval"
        );
        match &fold.items[0] {
            Item::Tool { status, .. } => assert_eq!(*status, ToolStatus::Running),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn subworkflow_wait_yields_follow_effect_once() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        let rec = json!({"run_id": "root", "status": "waiting",
            "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:sub1",
                                 "details": {"sub_run_id": "sub1"}}}});
        let fx = fold.apply("root", &rec);
        assert_eq!(fx, vec![FoldEffect::FollowRun("sub1".into())]);
        let fx2 = fold.apply("root", &rec);
        assert!(fx2.is_empty(), "follow effects deduplicate");
    }

    #[test]
    fn final_answer_only_from_root() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        let sub_final = json!({"run_id": "sub1", "status": "completed", "node_id": "end",
                               "result": {"output": {"answer": "intermediate"}}});
        fold.apply("sub1", &sub_final);
        assert!(fold.items.is_empty());
        let root_final = json!({"run_id": "root", "status": "completed", "node_id": "end",
                                "result": {"output": {"answer": "the final word"}}});
        fold.apply("root", &root_final);
        assert!(fold.finished);
        match last_content(&fold) {
            Item::Assistant { text, final_answer } => {
                assert!(final_answer);
                assert_eq!(text, "the final word");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn agent_subrun_answer_finishes_the_turn() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        // The subrun proves itself as the agent loop by cycling.
        fold.apply(
            "sub1",
            &json!({"run_id": "sub1", "node_id": "reason", "status": "started",
                                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        let sub_final = json!({"run_id": "sub1", "status": "completed", "node_id": "done",
                               "result": {"output": {"answer": "DONE", "report": "task: x"}}});
        fold.apply("sub1", &sub_final);
        assert!(fold.finished, "agent-loop subrun answer finishes the turn");
        match last_content(&fold) {
            Item::Assistant { text, final_answer } => {
                assert!(final_answer);
                assert_eq!(text, "DONE");
            }
            other => panic!("unexpected {other:?}"),
        }
        // A HELPER subrun's output must never finish the turn.
        let mut fold2 = Fold::new();
        fold2.begin_run("root");
        fold2.apply(
            "helper",
            &json!({"run_id": "helper", "status": "completed", "node_id": "done",
                                       "result": {"output": {"answer": "poller done"}}}),
        );
        assert!(!fold2.finished);
    }

    #[test]
    fn real_agent_ledger_fixture_folds_cleanly() {
        // Captured live from basic-agent@0.0.2 (gateway 2026-07-21): the
        // agent subrun with a write_file approval round-trip.
        let raw = include_str!("../tests/fixtures/agent_subrun_ledger.json");
        let records: Vec<Value> = serde_json::from_str(raw).expect("fixture parses");
        assert!(records.len() >= 8);
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.steer_run_id = "5d786409-b5f3-4657-8f4d-346874de5ba1".into();
        let mut pending_seen = false;
        for rec in &records {
            let run = protocol::record_run_id(rec);
            fold.apply(&run, rec);
            if let Some(w) = &fold.pending_wait {
                pending_seen = true;
                assert!(matches!(w.kind, WaitKind::Approval { .. }));
                // Simulate the user approving through the modal.
                let wk = w.wait_key.clone();
                let sid = w.step_id.clone();
                fold.wait_answered(&wk, &sid);
                fold.mark_wait_tools(true);
            }
        }
        assert!(pending_seen, "the fixture carries a tool-approval wait");
        assert!(fold.finished, "the fixture ends with the agent's answer");
        assert_eq!(fold.stats.tool_calls, 1);
        assert!(fold.stats.llm_calls >= 2);
        let tools: Vec<&Item> = fold
            .items
            .iter()
            .filter(|i| matches!(i, Item::Tool { .. }))
            .collect();
        assert_eq!(tools.len(), 1, "one tool card, updated in place: {tools:?}");
        match tools[0] {
            Item::Tool { status, name, .. } => {
                assert_eq!(name, "write_file");
                assert_eq!(*status, ToolStatus::Ok);
            }
            _ => unreachable!(),
        }
        assert!(matches!(
            last_content(&fold),
            Item::Assistant {
                final_answer: true,
                ..
            }
        ));
    }

    #[test]
    fn ask_wait_dedups_on_replay() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        let rec = json!({"run_id": "root", "status": "waiting",
            "result": {"wait": {"reason": "user", "wait_key": "ask1", "prompt": "Which one?"}}});
        fold.apply("root", &rec);
        assert!(fold.pending_wait.is_some());
        let sid = fold.pending_wait.as_ref().unwrap().step_id.clone();
        fold.wait_answered("ask1", &sid);
        fold.apply("root", &rec);
        assert!(
            fold.pending_wait.is_none(),
            "answered waits never re-prompt (same occurrence)"
        );
    }

    #[test]
    fn usage_and_cycles_fold() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "sub", "node_id": "reason", "status": "started",
                                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        assert_eq!(fold.cycle, 1);
        fold.apply("root", &json!({"run_id": "sub", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "thinking out loud", "usage": {"input_tokens": 100, "output_tokens": 7}}}));
        assert_eq!(fold.stats.llm_calls, 1);
        assert_eq!(fold.stats.input_tokens, 100);
        assert_eq!(fold.stats.output_series, vec![7.0]);
        assert!(matches!(fold.items.last().unwrap(), Item::Thinking { .. }));
    }

    #[test]
    fn splitless_usage_folds_totals() {
        // The EXACT usage shape from live coder run 0312b41d… (gpt-5.6-sol
        // via a proxy endpoint): only total_tokens is non-zero. Bug (e):
        // the strip read "0↑ 0↓ tk" for five hours.
        let mut fold = Fold::new();
        fold.begin_run("root");
        let rec = |total: u64| {
            json!({"run_id": "root", "node_id": "reason", "status": "completed",
                   "effect": {"type": "llm_call", "payload": {}},
                   "result": {"content": "…", "model": "gpt-5.6-sol",
                               "usage": {"input_tokens": 0, "output_tokens": 0,
                                          "total_tokens": total,
                                          "prompt_tokens": 0, "completion_tokens": 0}}})
        };
        fold.apply("root", &rec(3_180));
        fold.apply("root", &rec(2_821));
        assert_eq!(fold.stats.input_tokens, 0);
        assert_eq!(fold.stats.output_tokens, 0);
        assert_eq!(fold.stats.total_tokens, 6_001, "totals fold");
        assert_eq!(fold.session.total_tokens, 6_001, "session totals fold");
        assert_eq!(
            fold.stats.output_series,
            vec![3_180.0, 2_821.0],
            "sparkline substitutes per-call totals when the split is absent"
        );
        // A split provider's empty response is NEVER substituted (a total
        // spike there would mislabel prompt size as output).
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": "…", "usage": {"input_tokens": 500, "output_tokens": 0}}}),
        );
        assert_eq!(fold.stats.output_series.last(), Some(&0.0));
    }

    #[test]
    fn terminal_sounding_status_events_clear_activity() {
        // Wrapper-bundle helpers emit `abstract.status {"value": "Done"}`
        // per round (live basic-agent helper2; the coder screenshot's
        // sticky "Done · cycle 12 · 17880s"). Terminal-sounding texts
        // must CLEAR the strip, not stick while the tree keeps working.
        let mut fold = Fold::new();
        fold.begin_run("root");
        let status = |text: &str| {
            json!({"run_id": "helper", "status": "started",
                   "effect": {"type": "emit_event",
                               "payload": {"name": "abstract.status",
                                            "payload": {"value": text}}}})
        };
        for terminal in ["Done", "cancelled", "FAILED", "finished"] {
            fold.activity = "working".into();
            fold.apply("helper", &status(terminal));
            assert!(
                fold.activity.is_empty(),
                "{terminal:?} must clear the activity line"
            );
        }
        // Real progress texts still land.
        fold.apply("helper", &status("Verifying gates"));
        assert_eq!(fold.activity, "Verifying gates");
    }

    #[test]
    fn transcript_bounds_hold() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        // Chunked truncation: items float in [MAX_ITEMS, MAX_ITEMS +
        // TRUNCATE_CHUNK] (hysteresis — a per-push drain was quadratic
        // through the index-keyed feed sync). Push past one full drain
        // and assert both the bound and the standing notice.
        for i in 0..(MAX_ITEMS + TRUNCATE_CHUNK + 50) {
            fold.push_item(Item::Info {
                text: format!("line {i}"),
            });
        }
        assert!(fold.items.len() <= MAX_ITEMS + TRUNCATE_CHUNK + 1);
        assert!(matches!(&fold.items[0], Item::Info { text } if text.contains("#TRUNCATION")));
        // The newest item always survives a drain.
        let last_text = match fold.items.last().unwrap() {
            Item::Info { text } => text.clone(),
            other => panic!("unexpected tail item: {other:?}"),
        };
        assert_eq!(
            last_text,
            format!("line {}", MAX_ITEMS + TRUNCATE_CHUNK + 49)
        );
    }

    #[test]
    fn repeated_ask_on_stable_wait_key_reprompts() {
        // The runtime reuses `user:{run}:{node}` for every ask_user from the
        // same node — a NEW step_id is a NEW question (review P0).
        let mut fold = Fold::new();
        fold.begin_run("root");
        let ask = |step: &str| {
            json!({"run_id": "root", "status": "waiting", "step_id": step,
                   "result": {"wait": {"reason": "user", "wait_key": "user:root:act", "prompt": "Q?"}}})
        };
        fold.apply("root", &ask("step-1"));
        let w1 = fold.pending_wait.clone().expect("first ask prompts");
        fold.wait_answered(&w1.wait_key, &w1.step_id);
        // Run progresses, then asks AGAIN with the same key, new step.
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "completed", "node_id": "act",
                                    "effect": {"type": "resume"}}),
        );
        fold.apply("root", &ask("step-2"));
        assert!(
            fold.pending_wait.is_some(),
            "second ask on the same wait key must re-prompt"
        );
        // A REPLAY of step-2 (reconnect) must not duplicate.
        fold.wait_answered("user:root:act", "step-2");
        fold.apply("root", &ask("step-2"));
        assert!(
            fold.pending_wait.is_none(),
            "replayed occurrence stays answered"
        );
    }

    #[test]
    fn delegate_child_answer_never_finishes_the_turn() {
        // delegate_agent children cycle too; their flow ends are
        // INTERMEDIATE results for the parent (review P1).
        let mut fold = Fold::new();
        fold.begin_run("root");
        // Root discovers the first-level agent subrun.
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "waiting",
            "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:agent1",
                                 "details": {"sub_run_id": "agent1"}}}}),
        );
        // The agent cycles (first-level: parent == root).
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "reason", "status": "started",
                                      "effect": {"type": "llm_call", "payload": {}}}),
        );
        // The agent delegates: a child subrun that ALSO cycles.
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "status": "waiting",
            "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:child1",
                                 "details": {"sub_run_id": "child1"}}}}),
        );
        fold.apply(
            "child1",
            &json!({"run_id": "child1", "node_id": "reason", "status": "started",
                                      "effect": {"type": "llm_call", "payload": {}}}),
        );
        // The CHILD finishes with an answer-shaped output.
        fold.apply("child1", &json!({"run_id": "child1", "node_id": "done", "status": "completed",
                                      "result": {"output": {"answer": "intermediate delegate result"}}}));
        assert!(
            !fold.finished,
            "delegate child output must not finish the turn"
        );
        // The AGENT's own answer does.
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "done", "status": "completed",
                                      "result": {"output": {"answer": "the real answer"}}}),
        );
        assert!(fold.finished);
        match last_content(&fold) {
            Item::Assistant {
                text,
                final_answer: true,
            } => assert_eq!(text, "the real answer"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn per_run_dedup_state_resets_between_runs() {
        // Providers reuse call ids across turns (review P2): run 2's tool
        // must get its own card.
        let mut fold = Fold::new();
        fold.begin_run("r1");
        fold.apply(
            "r1",
            &rec_started_tools("r1", json!([{"name": "read_file", "call_id": "call_0"}])),
        );
        fold.apply("r1", &json!({"run_id": "r1", "node_id": "act", "status": "completed",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [{"name": "read_file", "call_id": "call_0"}]}},
            "result": {"results": [{"call_id": "call_0", "success": true, "output": "one"}]}}));
        fold.begin_run("r2");
        fold.apply(
            "r2",
            &rec_started_tools("r2", json!([{"name": "read_file", "call_id": "call_0"}])),
        );
        let tool_cards = fold
            .items
            .iter()
            .filter(|i| matches!(i, Item::Tool { .. }))
            .count();
        assert_eq!(
            tool_cards, 2,
            "run 2 gets its own card despite the reused call id"
        );
    }

    #[test]
    fn tool_events_contract_renders_cards() {
        // abstract.tool_execution / abstract.tool_result (docs/ui_events.md).
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "completed",
            "effect": {"type": "emit_event", "payload": {"name": "abstract.tool_execution",
                "payload": [{"tool": "web_search", "call_id": "e1", "params": {"q": "rust"}}]}}}),
        );
        assert!(matches!(
            fold.items.last().unwrap(),
            Item::Tool {
                status: ToolStatus::Running,
                ..
            }
        ));
        fold.apply("root", &json!({"run_id": "root", "status": "completed",
            "effect": {"type": "emit_event", "payload": {"name": "abstract.tool_result",
                "payload": [{"tool": "web_search", "call_id": "e1", "success": true, "output": "results…"}]}}}));
        match fold.items.last().unwrap() {
            Item::Tool { status, result, .. } => {
                assert_eq!(*status, ToolStatus::Ok);
                assert!(result.contains("results"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    /// The fold is not a preview store (operator directive 2026-08-20).
    /// Tool output, tool errors, the model's words and its reasoning all
    /// land WHOLE, so `/details` has something to show — the old
    /// ingestion bounds (700 chars of result, 200 of error, 8k of a
    /// thinking block) destroyed the text before any view could decide.
    #[test]
    fn ingestion_keeps_every_body_whole() {
        let long_out = format!("HEAD {} TAIL", "z".repeat(50_000));
        let long_err = format!("EHEAD {} ETAIL", "e".repeat(5_000));
        let long_arg = format!("cargo test {} --nocapture-TAIL", "x".repeat(5_000));
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "completed",
            "effect": {"type": "emit_event", "payload": {"name": "abstract.tool_result",
                "payload": [{"tool": "execute_command", "call_id": "e1", "success": true,
                             "arguments": {"command": long_arg},
                             "output": long_out}]}}}),
        );
        match fold.items.last().unwrap() {
            Item::Tool {
                args_preview,
                args_full,
                result,
                ..
            } => {
                assert!(
                    result.contains("HEAD") && result.contains("TAIL"),
                    "the tool result lands whole (len {})",
                    result.len()
                );
                assert!(
                    !result.contains("#TRUNCATION"),
                    "nothing is shortened on the way into the fold"
                );
                assert!(
                    args_full.contains("--nocapture-TAIL"),
                    "the arguments land whole for the details card"
                );
                // The folded row still gets its bounded one-line hint —
                // that view has one row, and it says so.
                assert!(
                    args_preview.chars().count() <= ARGS_PREVIEW_MAX + 1,
                    "the FOLDED hint stays bounded: {args_preview}"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        // The `rendered` envelope path: the payload leads WHOLE, and its
        // content siblings survive (adversarial review F5 — `stdout` and
        // friends used to be dropped with no notice), while the transport
        // keys stay out (the 2026-08-19 relevance ruling).
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "completed",
            "effect": {"type": "emit_event", "payload": {"name": "abstract.tool_result",
                "payload": [{"tool": "execute_command", "call_id": "e3", "success": true,
                             "output": {"rendered": format!("RHEAD {} RTAIL", "r".repeat(50_000)),
                                        "stdout": "STDOUT-KEPT",
                                        "exit_code": 0,
                                        "platform": "Darwin",
                                        "duration_s": 0.09,
                                        "error": null}}]}}}),
        );
        match fold.items.last().unwrap() {
            Item::Tool { result, .. } => {
                assert!(
                    result.contains("RHEAD") && result.contains("RTAIL"),
                    "the rendered payload lands whole (len {})",
                    result.len()
                );
                assert!(
                    result.contains("STDOUT-KEPT") && result.contains("exit_code"),
                    "content siblings of `rendered` survive: {result}"
                );
                assert!(
                    !result.contains("platform") && !result.contains("duration_s"),
                    "transport keys stay out of the card: {result}"
                );
            }
            other => panic!("unexpected {other:?}"),
        }

        // Errors: whole, newlines and all (the folded row one-lines at render).
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "completed",
            "effect": {"type": "emit_event", "payload": {"name": "abstract.tool_result",
                "payload": [{"tool": "broken", "call_id": "e2", "success": false,
                             "error": long_err}]}}}),
        );
        match fold.items.last().unwrap() {
            Item::Tool { error, .. } => assert!(
                error.contains("EHEAD") && error.contains("ETAIL"),
                "the tool error lands whole (len {})",
                error.len()
            ),
            other => panic!("unexpected {other:?}"),
        }
    }

    /// The other five bodies claim-1 frees: thinking, its reasoning
    /// channel, an assistant message, an error notice and a warning
    /// notice. The tool-body test above cannot see these — the
    /// adversarial review (F9) proved a re-introduced 80-char cap on
    /// `Item::Thinking` left the whole suite green.
    #[test]
    fn ingestion_keeps_thinking_answers_and_notices_whole() {
        let big = |head: &str, tail: &str| format!("{head} {} {tail}", "w ".repeat(30_000));
        let mut fold = Fold::new();
        fold.begin_run("root");
        // A cycle: content + the reasoning channel, both oversized.
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "reason", "status": "started",
                "effect": {"type": "llm_call", "payload": {}}}),
        );
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "reason", "status": "completed",
            "effect": {"type": "llm_call", "payload": {}},
            "result": {"content": big("CHEAD", "CTAIL"), "reasoning": big("RHEAD", "RTAIL")}}),
        );
        let thinking = fold
            .items
            .iter()
            .find_map(|i| match i {
                Item::Thinking {
                    content, reasoning, ..
                } => Some((content.clone(), reasoning.clone())),
                _ => None,
            })
            .expect("a thinking card");
        assert!(
            thinking.0.contains("CHEAD") && thinking.0.contains("CTAIL"),
            "the model's words land whole (len {})",
            thinking.0.len()
        );
        assert!(
            thinking.1.contains("RHEAD") && thinking.1.contains("RTAIL"),
            "the reasoning channel lands whole (len {})",
            thinking.1.len()
        );

        // Message events: assistant text, an error notice, a warning.
        let msg = |level: &str, text: String| {
            json!({"run_id": "root", "status": "completed",
                "effect": {"type": "emit_event", "payload": {"name": "abstract.message",
                    "payload": {"text": text, "level": level}}}})
        };
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply("root", &msg("info", big("AHEAD", "ATAIL")));
        fold.apply("root", &msg("error", big("EHEAD", "ETAIL")));
        fold.apply("root", &msg("warning", big("WHEAD", "WTAIL")));
        let mut seen = (false, false, false);
        for item in &fold.items {
            match item {
                Item::Assistant { text, .. } if text.contains("AHEAD") => {
                    seen.0 = text.contains("ATAIL");
                }
                Item::Error { text } if text.contains("EHEAD") => seen.1 = text.contains("ETAIL"),
                Item::Info { text } if text.contains("WHEAD") => seen.2 = text.contains("WTAIL"),
                _ => {}
            }
        }
        assert!(seen.0, "an assistant message lands whole");
        assert!(seen.1, "an error notice lands whole");
        assert!(seen.2, "a warning notice lands whole");
    }

    #[test]
    fn status_event_clears_activity_when_empty() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply("root", &json!({"run_id": "root", "status": "completed",
            "effect": {"type": "emit_event", "payload": {"name": "abstract.status", "payload": {"text": "Working"}}}}));
        assert_eq!(fold.activity, "Working");
        fold.apply("root", &json!({"run_id": "root", "status": "completed",
            "effect": {"type": "emit_event", "payload": {"name": "abstract.status", "payload": {"text": ""}}}}));
        assert!(fold.activity.is_empty(), "empty status clears the line");
    }

    /// Coder progress (flow multiagent-coding 0.0.7): a "build cycle N of
    /// M" answer_user message drives BOTH the strip (glance surface —
    /// operator's "never a surprise" for the fix budget) AND a persistent
    /// transcript card (cycle-boundary record). A non-progress answer_user
    /// message is transcript-only — it must NOT hijack the strip.
    #[test]
    fn inflight_tool_label_names_the_running_command() {
        // Observability wave 2026-07-27: a started tool batch records
        // WHAT is running so the strip can name it; a completed batch
        // clears the label. The 8h hang showed only "large scans" — a
        // labeled command is the human's first "is this stuck?" cue.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "act", "status": "started",
                "effect": {"type": "tool_calls", "payload": {"tool_calls":
                    [{"name": "execute_command", "call_id": "c1",
                      "arguments": {"command": "python3 -m http.server 8765"}}]}}}),
        );
        assert!(fold.tool_inflight_since.is_some());
        let label = fold.inflight_tool_label.clone().unwrap_or_default();
        assert!(
            label.starts_with("execute_command"),
            "names the tool: {label}"
        );
        assert!(
            label.contains("http.server"),
            "carries the command: {label}"
        );
        // A multi-call batch summarizes.
        let mut f2 = Fold::new();
        f2.begin_run("r");
        f2.apply(
            "r",
            &json!({"run_id": "r", "node_id": "act", "status": "started",
                "effect": {"type": "tool_calls", "payload": {"tool_calls":
                    [{"name": "read_file", "call_id": "a"},
                     {"name": "list_files", "call_id": "b"}]}}}),
        );
        assert_eq!(f2.inflight_tool_label.as_deref(), Some("2 tools"));
        // Completion clears the label.
        f2.apply(
            "r",
            &json!({"run_id": "r", "node_id": "act", "status": "completed",
                "effect": {"type": "tool_calls", "payload": {"tool_calls":
                    [{"name": "read_file", "call_id": "a"},
                     {"name": "list_files", "call_id": "b"}]}},
                "result": {"results": [{"call_id": "a", "success": true},
                                       {"call_id": "b", "success": true}]}}),
        );
        assert!(f2.inflight_tool_label.is_none(), "cleared on completion");
    }

    #[test]
    fn build_cycle_progress_drives_the_strip_and_leaves_the_card() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "completed",
                "effect": {"type": "answer_user",
                    "payload": {"message": "build cycle 2 of 6", "level": "message"}}}),
        );
        assert_eq!(fold.activity, "build cycle 2 of 6", "strip shows the cycle");
        assert!(
            matches!(fold.items.last(), Some(Item::Assistant { text, final_answer: false }) if text == "build cycle 2 of 6"),
            "and a persistent transcript card marks the boundary"
        );
        // A real intermediate answer stays transcript-only.
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "completed",
                "effect": {"type": "answer_user",
                    "payload": {"message": "here is a partial result", "level": "message"}}}),
        );
        assert_eq!(
            fold.activity, "build cycle 2 of 6",
            "a non-progress message does not hijack the strip"
        );
    }

    #[test]
    fn chat_messages_carry_completed_turns_under_caps() {
        let mut fold = Fold::new();
        fold.begin_run("r1");
        fold.push_item(Item::User { text: "q1".into() });
        fold.push_item(Item::Thinking {
            iteration: 1,
            content: "…".into(),
            reasoning: String::new(),
            call: CallCost::default(),
        });
        fold.push_item(Item::Assistant {
            text: "a1".into(),
            final_answer: true,
        });
        fold.push_item(Item::User {
            text: "q2 (no answer yet)".into(),
        });
        let msgs = fold.chat_messages(40, 24_000);
        assert_eq!(
            msgs,
            vec![
                ("user".to_string(), "q1".to_string()),
                ("assistant".to_string(), "a1".to_string())
            ],
            "only complete turns travel; thinking never does"
        );
        // Cap discipline: oldest whole turns drop, newest survives.
        let mut fold2 = Fold::new();
        for i in 0..30 {
            fold2.push_item(Item::User {
                text: format!("q{i}"),
            });
            fold2.push_item(Item::Assistant {
                text: format!("a{i}"),
                final_answer: true,
            });
        }
        let msgs2 = fold2.chat_messages(10, 24_000);
        assert_eq!(msgs2.len(), 10);
        assert_eq!(
            msgs2.last().unwrap().1,
            "a29",
            "newest turn always survives"
        );
    }

    #[test]
    fn failed_record_reports_error() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "failed", "error": "LLM exploded"}),
        );
        assert!(matches!(last_content(&fold), Item::Error { text } if text.contains("exploded")));
        assert!(fold.finished);
    }

    #[test]
    fn failed_flag_tracks_root_failures_only_and_resets_per_run() {
        // Plan item 4 (finding 8): exec's exit code + the queue's
        // pause-on-failure read `fold.failed` — it must be TRUE exactly
        // when the root failed, never for a subrun's recoverable error.
        let mut fold = Fold::new();
        fold.begin_run("root");
        // A SUBRUN failure records an error card but is not terminal.
        fold.apply(
            "sub1",
            &json!({"run_id": "sub1", "status": "failed", "error": "tool exploded"}),
        );
        assert!(!fold.failed, "subrun failure never sets the run failed");
        assert!(!fold.finished);
        // The ROOT failure is terminal + failed.
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "failed", "error": "flow died"}),
        );
        assert!(fold.failed, "root failure sets failed");
        assert!(fold.finished);
        // A new run resets it.
        fold.begin_run("root2");
        assert!(!fold.failed);
        // run_terminal truth: failed status sets it; cancelled does not
        // (exec exits 130 for cancels through its own status branch).
        fold.run_terminal("failed");
        assert!(fold.failed && fold.finished);
        let mut fold2 = Fold::new();
        fold2.begin_run("r");
        fold2.run_terminal("cancelled");
        assert!(fold2.finished && !fold2.failed, "cancel is not a failure");
    }

    #[test]
    fn finish_on_root_only_defers_the_finish_to_the_root() {
        // The /goal P0 defense: goal bundles loop one cycling subrun PER
        // ITERATION — without the flag the TUI declared the goal finished
        // at iteration 1 and released the composer over a live run.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.finish_on_root_only = true;
        // Root discovers the iteration's agent subrun; it cycles.
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "waiting",
            "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:iter1",
                                 "details": {"sub_run_id": "iter1"}}}}),
        );
        fold.apply(
            "iter1",
            &json!({"run_id": "iter1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        // The iteration ends with an ANSWER-SHAPED flow end.
        fold.apply(
            "iter1",
            &json!({"run_id": "iter1", "node_id": "done", "status": "completed",
                    "result": {"output": {"answer": "iteration 1 result"}}}),
        );
        assert!(
            !fold.finished,
            "a subrun answer must NOT finish a root-only run"
        );
        match last_content(&fold) {
            Item::Assistant { text, final_answer } => {
                assert!(!final_answer, "iteration results render NON-final");
                assert_eq!(text, "iteration 1 result");
            }
            other => panic!("unexpected {other:?}"),
        }
        // Cycle-3 audit, cell (e): the iteration's end CLEARS the cycling
        // target — its guidance inbox died with the run, so a steer typed
        // between iterations must BUFFER, never ride into a dead run.
        assert_eq!(
            fold.cycling_target(),
            None,
            "the cycling target dies with the iteration"
        );
        // Iteration 2 cycles: under finish_on_root_only the answer lane
        // FOLLOWS the live iteration (first-wins would render nothing and
        // fold ctx/model telemetry from a dead run for iterations 2+).
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "waiting",
            "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:iter2",
                                 "details": {"sub_run_id": "iter2"}}}}),
        );
        fold.apply(
            "iter2",
            &json!({"run_id": "iter2", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        assert_eq!(fold.cycling_target().as_deref(), Some("iter2"));
        fold.apply(
            "iter2",
            &json!({"run_id": "iter2", "node_id": "done", "status": "completed",
                    "result": {"output": {"answer": "iteration 2 result"}}}),
        );
        assert!(!fold.finished, "iteration 2's end keeps the goal open");
        match last_content(&fold) {
            Item::Assistant { text, final_answer } => {
                assert!(!final_answer, "iteration 2 renders NON-final too");
                assert_eq!(text, "iteration 2 result");
            }
            other => panic!("unexpected {other:?}"),
        }
        // Only the ROOT's own end concludes.
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "end", "status": "completed",
                    "result": {"output": {"answer": "goal met: evidence attached"}}}),
        );
        assert!(fold.finished, "the root end finishes the goal run");
        assert!(matches!(
            last_content(&fold),
            Item::Assistant {
                final_answer: true,
                ..
            }
        ));
        // Without the flag, the same subrun answer DOES finish (the
        // wrapper-bundle behavior every existing test pins).
        let mut normal = Fold::new();
        normal.begin_run("root");
        normal.apply(
            "iter1",
            &json!({"run_id": "iter1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        normal.apply(
            "iter1",
            &json!({"run_id": "iter1", "node_id": "done", "status": "completed",
                    "result": {"output": {"answer": "the answer"}}}),
        );
        assert!(normal.finished);
    }

    #[test]
    fn offloaded_answer_concludes_and_fetch_resolves_the_words() {
        // THE "never finishes" P0 (live run c61e4ac9…, 2026-07-22): a heavy
        // agent turn's final output persists as {"$artifact": id} (runtime
        // ledger offloader, outputs >256 KB served unresolved) — the fold
        // saw no text, `finished` never flipped, and the composer stayed
        // captured for 5 hours while the answer sat in the artifact store.
        let mut fold = Fold::new();
        fold.begin_run("root");
        // The agent subrun proves itself by cycling (parent unknown =>
        // first-level, the production shape after a partial replay).
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        // The done record: the LIVE offloaded shape, verbatim.
        let done = json!({"run_id": "agent1", "node_id": "done", "status": "completed",
                          "effect": null,
                          "result": {"completed": true,
                                      "output": {"$artifact": "e3b19ad9e42a2b725048bab40138f975"}}});
        let fx = fold.apply("agent1", &done);
        assert!(
            fold.finished,
            "the offloaded flow end must conclude the turn"
        );
        assert_eq!(
            fx,
            vec![FoldEffect::FetchAnswer {
                run_id: "agent1".into(),
                artifact_id: "e3b19ad9e42a2b725048bab40138f975".into()
            }],
            "the fold asks the runner to fetch the real words"
        );
        let placeholder = offload_placeholder("e3b19ad9e42a2b725048bab40138f975");
        match last_content(&fold) {
            Item::Assistant { text, final_answer } => {
                assert!(*final_answer);
                assert_eq!(*text, placeholder, "placeholder names the artifact");
            }
            other => panic!("unexpected {other:?}"),
        }
        // The fetch resolves: the placeholder swaps for the real answer.
        fold.resolve_offloaded_answer(
            "e3b19ad9e42a2b725048bab40138f975",
            Ok("You were right. I tested the game myself.".into()),
        );
        match last_content(&fold) {
            Item::Assistant { text, final_answer } => {
                assert!(*final_answer);
                assert!(text.starts_with("You were right."));
            }
            other => panic!("unexpected {other:?}"),
        }
        // A second resolve (stale double-fetch) is a clean no-op.
        fold.resolve_offloaded_answer("e3b19ad9e42a2b725048bab40138f975", Ok("other".into()));
        assert!(
            matches!(last_content(&fold), Item::Assistant { text, .. } if text.starts_with("You were right.")),
        );
    }

    #[test]
    fn offloaded_answer_fetch_failure_labels_honestly_and_never_recaptures() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {"$artifact": "abc123"}}}),
        );
        assert!(fold.finished, "conclusion never waits on the fetch");
        fold.resolve_offloaded_answer("abc123", Err("HTTP 500".into()));
        match last_content(&fold) {
            Item::Assistant { text, final_answer } => {
                assert!(*final_answer, "the failure label stays the final answer");
                // The 2026-07-23 fetch_url incident contract: the label
                // carries the compact reason but NO artifact id, NO URL,
                // and NO retry framing — this text replays into later
                // turns' context.messages as assistant words.
                assert!(
                    text.starts_with(OFFLOAD_FAILURE_PREFIX) && text.contains("HTTP 500"),
                    "neutral evidence-worded label: {text}"
                );
                assert!(
                    !text.contains("abc123") && !text.contains("http"),
                    "no artifact id, no URL in travelling text: {text}"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(fold.finished, "a failed fetch must not reopen the turn");
    }

    #[test]
    fn late_root_flow_end_reconciles_a_lost_offloaded_answer() {
        // The 2026-07-23 incident's missed truth: the agent subrun's
        // 445KB output offloaded (placeholder), the artifact fetch died
        // on a transport reset (failure label), and the wrapper ROOT
        // completed minutes later with the answer INLINE in
        // output.response — which the fold IGNORED (finished gate). The
        // late-truth reconcile swaps the failure card for the real
        // words; works identically on live streams and ledger replay.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {"$artifact": "lostart"}}}),
        );
        fold.resolve_offloaded_answer("lostart", Err("gateway unreachable".into()));
        assert!(
            matches!(last_content(&fold), Item::Assistant { text, .. } if text.starts_with(OFFLOAD_FAILURE_PREFIX))
        );
        // The root's own flow end, minutes later, answer inline.
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "end", "status": "completed",
                    "result": {"completed": true, "output": {"response": "The full report: everything shipped."}}}),
        );
        match last_content(&fold) {
            Item::Assistant { text, final_answer } => {
                assert!(*final_answer);
                assert_eq!(
                    text, "The full report: everything shipped.",
                    "the root's inline response replaces the failure label"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        // No SECOND final card was pushed — the swap happened in place.
        let finals = fold
            .items
            .iter()
            .filter(|i| {
                matches!(
                    i,
                    Item::Assistant {
                        final_answer: true,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(finals, 1, "swap in place, never a duplicate final");
        // A later successful fetch retry is a clean no-op (words already
        // landed; neither placeholder nor failure prefix matches).
        fold.resolve_offloaded_answer("lostart", Ok("stale retry words".into()));
        assert!(
            matches!(last_content(&fold), Item::Assistant { text, .. } if text == "The full report: everything shipped.")
        );
    }

    #[test]
    fn offloaded_iteration_result_respects_finish_on_root_only() {
        // Goal defense interplay: an ITERATION whose output was offloaded
        // still renders non-final and keeps the goal open.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.finish_on_root_only = true;
        fold.apply(
            "iter1",
            &json!({"run_id": "iter1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        let fx = fold.apply(
            "iter1",
            &json!({"run_id": "iter1", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {"$artifact": "iterart"}}}),
        );
        assert!(
            !fold.finished,
            "a subrun's offloaded end keeps the goal open"
        );
        assert!(
            fx.contains(&FoldEffect::FetchAnswer {
                run_id: "iter1".into(),
                artifact_id: "iterart".into()
            }),
            "iteration words still fetch: {fx:?}"
        );
        assert!(matches!(
            fold.items.last().unwrap(),
            Item::Assistant {
                final_answer: false,
                ..
            }
        ));
    }

    /// LEGACY ENGINE (no `stop_reason` on the wire): a run that STOPPED must
    /// not read as one that FINISHED — and this host must not invent a reason
    /// it cannot know. An exhausted budget and a stuck-loop stop both arrive
    /// as `outcome: "iteration_budget"`, so any remedy composed here is a
    /// guess, and guessing is what sent operators to `--max-iterations` for a
    /// run with 38 of its 50 iterations unspent.
    #[test]
    fn a_legacy_stop_is_reported_without_inventing_a_cause() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {
                        "answer": "Here is the game.",
                        "outcome": "iteration_budget",
                        "iterations": 50}}}),
        );
        assert!(fold.finished);
        assert_eq!(fold.stopped_short, Some(50));
        assert!(fold.stop.is_none(), "no server verdict was sent");
        assert!(!fold.failed, "a stop is a third outcome, not a failure");
        assert!(
            fold.done_note.contains("stopped after 50 iterations"),
            "got {:?}",
            fold.done_note
        );
        assert!(!fold.done_note.starts_with("done"));
        let card = fold
            .items
            .iter()
            .find_map(|i| match i {
                Item::Error { text } if text.contains("STOPPED") => Some(text.clone()),
                _ => None,
            })
            .expect("the stop is still announced");
        assert!(
            !card.contains("max-iterations") && !card.to_lowercase().contains("raise"),
            "a host must not invent a remedy it cannot derive: {card:?}"
        );
        assert_eq!(
            crate::exec::exit_code_for_status_with_verdict("completed", true),
            crate::exec::EXIT_STOPPED_SHORT
        );
        assert_ne!(crate::exec::EXIT_STOPPED_SHORT, 0);
        assert_ne!(crate::exec::EXIT_STOPPED_SHORT, 1);
    }

    /// THE ARCHITECTURE RULE. `abstractcode` is a thin host: it shows what
    /// the gateway sends. The loop's terminal node authors the verdict, so this
    /// TUI, AbstractObserver, the web client and any chat bridge all show the
    /// SAME sentence. This pins that the card is the server's two fields
    /// concatenated and nothing else — no shape word, no count, no advice
    /// composed here.
    #[test]
    fn the_stop_card_is_exactly_what_the_server_worded() {
        let headline =
            "The agent stopped early after 12 iterations: it repeated the same tool batch 5 times.";
        let remedy = "The iteration budget was not the limit, so raising it will not help.";
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "max_iterations", "status": "completed",
                    "result": {"completed": true, "output": {
                        "answer": "Progress report…",
                        "outcome": "iteration_budget",
                        "iterations": 12,
                        "conclusion_forced": {"kind": "repeat", "span": 5, "nudged": true},
                        "stop_reason": {
                            "code": "stuck_repeat", "finished": false, "budget_exhausted": false,
                            "iterations": 12,
                            "label": "stopped: repeated tool calls after 12 iterations",
                            "headline": headline, "remedy": remedy}}}}),
        );
        let stop = fold.stop.as_ref().expect("the server verdict latches");
        assert_eq!(stop.code, "stuck_repeat");
        assert!(!stop.finished);
        assert!(
            fold.done_note
                .contains("stopped: repeated tool calls after 12 iterations"),
            "the chrome line is the server's label: {:?}",
            fold.done_note
        );
        let card = fold
            .items
            .iter()
            .find_map(|i| match i {
                Item::Error { text } if text.starts_with("The agent stopped early") => {
                    Some(text.clone())
                }
                _ => None,
            })
            .expect("the stop card fires");
        assert_eq!(card, format!("{headline} {remedy}"));
    }

    /// SEAM TEST against a REAL server payload. The fixture is a verbatim
    /// capture from a live `gpt-5.4-mini` run on `endpoint:airelay` whose loop
    /// hit the stuck guillotine — not a hand-written approximation of the
    /// contract, which is how a client and a server drift apart while both
    /// their own test suites stay green. Regenerate it by re-running the
    /// capture, never by editing it to match this client.
    #[test]
    fn a_live_server_verdict_folds_into_the_card_and_the_chrome_line() {
        let raw = include_str!("../tests/fixtures/stop_reason_stuck_live.json");
        let rec: Value = serde_json::from_str(raw).expect("fixture parses");
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply("root", &rec);

        let stop = fold
            .stop
            .as_ref()
            .expect("the live payload carries a server verdict this client can read");
        assert_eq!(stop.code, "stuck_repeat");
        assert!(!stop.finished);
        assert!(!stop.label.is_empty() && !stop.headline.is_empty() && !stop.remedy.is_empty());
        // Chrome and card are the server's own strings.
        assert!(
            fold.done_note.contains(&stop.label),
            "got {:?}",
            fold.done_note
        );
        let card = fold
            .items
            .iter()
            .find_map(|i| match i {
                Item::Error { text } => Some(text.clone()),
                _ => None,
            })
            .expect("a stop card fires");
        assert_eq!(card, format!("{} {}", stop.headline, stop.remedy));
        assert_eq!(
            crate::exec::stopped_head(fold.stopped_short, fold.stop.as_ref()),
            stop.label
        );
    }

    /// A server-worded plain budget stop carries the SERVER's remedy — the
    /// client neither adds to it nor swaps in the stuck wording.
    #[test]
    fn a_server_worded_budget_stop_carries_the_server_remedy() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "max_iterations", "status": "completed",
                    "result": {"completed": true, "output": {
                        "answer": "…", "outcome": "iteration_budget", "iterations": 50,
                        "stop_reason": {
                            "code": "iteration_budget", "finished": false, "budget_exhausted": true,
                            "iterations": 50,
                            "label": "stopped: iteration budget after 50 iterations",
                            "headline": "The agent ran out of iterations and STOPPED — it did not finish.",
                            "remedy": "Raise the iteration budget, or send the remaining work as a follow-up turn."}}}}),
        );
        assert_eq!(
            fold.stop.as_ref().map(|v| v.code.as_str()),
            Some("iteration_budget")
        );
        assert!(fold
            .done_note
            .contains("stopped: iteration budget after 50 iterations"));
        assert!(fold.items.iter().any(|i| matches!(i, Item::Error { text }
            if text.contains("Raise the iteration budget"))));
        assert!(
            !fold.items.iter().any(|i| matches!(i, Item::Error { text }
                if text.contains("repeated the same tool batch"))),
            "the stuck wording must never appear for a budget stop"
        );
    }

    /// The forced latch must not survive into a LATER verdict in the same
    /// turn. `push_verdict_note` runs at every conclusion record — subruns
    /// included, since `finish_on_root_only` defaults to false — so a
    /// loop-forced subrun followed by a genuinely budget-exhausted root would
    /// otherwise keep the subrun's word for the whole turn.
    #[test]
    fn a_forced_subrun_does_not_relabel_a_later_plain_budget_verdict() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        let worded = json!({"run_id": "sub", "node_id": "max_iterations", "status": "completed",
                    "result": {"completed": true, "output": {
                        "answer": "sub", "outcome": "iteration_budget", "iterations": 5,
                        "stop_reason": {"code": "stuck_repeat", "finished": false,
                                        "label": "stopped: repeated tool calls after 5 iterations",
                                        "headline": "It repeated a batch.",
                                        "remedy": "Try another route."}}}});
        let unworded = json!({"run_id": "root", "node_id": "max_iterations", "status": "completed",
                    "result": {"completed": true, "output": {
                        "answer": "root", "outcome": "iteration_budget", "iterations": 50}}});
        // Unit-level on purpose: which records REACH `push_verdict_note`
        // depends on the answer-source/root routing above, and the latch must
        // be safe regardless of that routing.
        fold.push_verdict_note(&worded);
        assert!(fold.stop.is_some(), "the server verdict latches");
        // A later verdict with NO server wording must not inherit the previous
        // one — the chrome line would then describe the wrong turn.
        fold.push_verdict_note(&unworded);
        assert!(
            fold.stop.is_none(),
            "an unworded verdict must clear the latch, not inherit the previous wording"
        );
        fold.push_done_summary("completed");
        assert!(
            fold.done_note.contains("stopped after 50 iterations"),
            "got {:?}",
            fold.done_note
        );
    }

    /// The server says how loud a caveat is, and how a stop ended. Both were
    /// being thrown away: `severity` was flattened to Info for every notice,
    /// and `stop_reason.budget_exhausted` — the field that separates a spent
    /// budget from a stuck-loop stop — was never read at all, so the
    /// machine-readable exit channel could still call one the other.
    #[test]
    fn the_servers_severity_and_budget_facts_survive_the_fold() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "max_iterations", "status": "completed",
                    "result": {"completed": true, "output": {
                        "answer": "…", "outcome": "iteration_budget", "iterations": 12,
                        "notices": [
                            {"code": "review_skipped", "severity": "warn", "text": "warn line"},
                            {"code": "evidence_missing", "severity": "error", "text": "error line"}],
                        "stop_reason": {
                            "code": "stuck_repeat", "finished": false, "budget_exhausted": false,
                            "iterations": 12, "label": "stopped: repeated tool calls",
                            "headline": "It repeated a batch.", "remedy": "Try another route."}}}}),
        );
        let stop = fold.stop.as_ref().expect("verdict latches");
        assert!(
            !stop.budget_exhausted,
            "the server said the budget was NOT spent; this host must carry that"
        );
        // Severity picks the ink: the warn is an info line, the error is not.
        assert!(fold
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text == "warn line")));
        assert!(fold
            .items
            .iter()
            .any(|i| matches!(i, Item::Error { text } if text == "error line")));
        // And the live capture agrees on the same field.
        let raw = include_str!("../tests/fixtures/stop_reason_stuck_live.json");
        let rec: Value = serde_json::from_str(raw).expect("fixture parses");
        let v = crate::protocol::run_verdict(&rec).expect("verdict");
        assert!(
            !v.stop_budget_exhausted,
            "a live stuck stop spent no budget"
        );
        assert!(!v.stop_finished);
    }

    /// Server-authored `notices` render verbatim, and the legacy client-side
    /// sentence yields to them rather than doubling up.
    #[test]
    fn notices_are_rendered_verbatim_and_never_doubled_by_the_legacy_line() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {
                        "answer": "…", "outcome": "final_answer", "iterations": 4,
                        "review_skipped": true,
                        "notices": [{"code": "review_skipped", "severity": "warn",
                                     "text": "The verifier pass was requested but did not run."}],
                        "stop_reason": {"code": "final_answer", "finished": true,
                                        "label": "done", "headline": "", "remedy": ""}}}}),
        );
        let infos: Vec<String> = fold
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Info { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(infos
            .iter()
            .any(|t| t == "The verifier pass was requested but did not run."));
        assert!(!infos.iter().any(|t| t.contains("#FALLBACK")), "{infos:?}");
        assert!(
            fold.done_note.starts_with("done"),
            "got {:?}",
            fold.done_note
        );
    }

    /// The mirror: an ordinary completion is untouched by the new branch —
    /// still `✓ done`, still exit 0. Guards against over-firing the verdict.
    #[test]
    fn ordinary_completion_still_reads_done_and_exits_zero() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {"answer": "Done."}}}),
        );
        assert!(fold.finished);
        assert_eq!(fold.stopped_short, None);
        assert!(
            fold.done_note.starts_with("done"),
            "got {:?}",
            fold.done_note
        );
        assert_eq!(
            crate::exec::exit_code_for_status_with_verdict("completed", false),
            0
        );
        // A real failure keeps priority over the budget verdict.
        assert_eq!(
            crate::exec::exit_code_for_status_with_verdict("failed", true),
            1
        );
        assert_eq!(
            crate::exec::exit_code_for_status_with_verdict("cancelled", true),
            130
        );
    }

    #[test]
    fn flow_end_without_readable_output_concludes_honestly() {
        // The key-independent conclusion: the answer-lane run's own
        // completion record (`result.completed == true`) with an output
        // carrying NO conventional text key must flip `finished` with an
        // honest label — never leave the composer captured forever.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {"weird_key": 42}}}),
        );
        assert!(fold.finished, "the flow end concludes even without text");
        assert!(matches!(
            last_content(&fold),
            Item::Info { text } if text.contains("without a readable final answer")
        ));
        // A HELPER's odd completion record must NOT conclude (answer lane only).
        let mut fold2 = Fold::new();
        fold2.begin_run("root");
        fold2.apply(
            "helper",
            &json!({"run_id": "helper", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {"weird_key": 1}}}),
        );
        assert!(!fold2.finished, "helper flow ends never conclude the turn");
        // Ordinary completed records without the completion marker (resume,
        // wait_until, emits) never trigger the fallback.
        let mut fold3 = Fold::new();
        fold3.begin_run("root");
        fold3.apply(
            "root",
            &json!({"run_id": "root", "node_id": "n", "status": "completed",
                    "effect": {"type": "resume"}, "result": {"resumed": true}}),
        );
        assert!(!fold3.finished);
    }

    #[test]
    fn llm_inflight_is_per_run_and_clears_on_boundaries() {
        // Step-3 honesty: a parallel lane's fast completion must not hide
        // a slow call still running elsewhere; and F9 — the hint must die
        // at run boundaries (begin_run / run_terminal / explicit clear).
        let mut fold = Fold::new();
        fold.begin_run("root");
        let started = |run: &str| {
            json!({"run_id": run, "node_id": "reason", "status": "started",
                   "effect": {"type": "llm_call", "payload": {}}})
        };
        let completed = |run: &str| {
            json!({"run_id": run, "node_id": "reason", "status": "completed",
                   "effect": {"type": "llm_call", "payload": {}},
                   "result": {"content": "x"}})
        };
        fold.apply("a", &started("a"));
        let a_since = fold.llm_inflight_since.expect("armed on start");
        fold.apply("b", &started("b"));
        // The OLDEST inflight call is the honest elapsed anchor.
        assert_eq!(fold.llm_inflight_since, Some(a_since));
        fold.apply("b", &completed("b"));
        assert_eq!(
            fold.llm_inflight_since,
            Some(a_since),
            "b's completion must not clear a's still-running call"
        );
        fold.apply("a", &completed("a"));
        assert!(fold.llm_inflight_since.is_none(), "all lanes done");
        // F9: begin_run clears a dangling hint.
        fold.apply("a", &started("a"));
        fold.begin_run("root2");
        assert!(fold.llm_inflight_since.is_none(), "begin_run clears");
        // run_terminal clears too.
        fold.apply("c", &started("c"));
        fold.run_terminal("completed");
        assert!(fold.llm_inflight_since.is_none(), "terminal clears");
        // The explicit clear (rehydrate's boundary) works standalone.
        let mut fold2 = Fold::new();
        fold2.apply("x", &started("x"));
        fold2.clear_llm_inflight();
        assert!(fold2.llm_inflight_since.is_none());
    }

    #[test]
    fn tool_inflight_arms_on_start_clears_on_wait_and_boundaries() {
        // The tool twin of the llm-inflight clock (live P0, 2026-07-23:
        // an 8m39s gateway-side search_files rendered as a bare
        // "running search_files" — no clock, read as a client hang).
        let mut fold = Fold::new();
        fold.begin_run("root");
        let started = |run: &str| {
            json!({"run_id": run, "node_id": "act", "status": "started",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"name": "search_files", "arguments": {"pattern": "x"}}
            ]}}})
        };
        let completed = |run: &str| {
            json!({"run_id": run, "node_id": "act", "status": "completed",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"name": "search_files", "arguments": {"pattern": "x"}}
            ]}},
            "result": {"results": [
                {"name": "search_files", "success": true, "output": "ok"}
            ]}})
        };
        fold.apply("a", &started("a"));
        let a_since = fold.tool_inflight_since.expect("armed on start");
        fold.apply("b", &started("b"));
        // Oldest inflight batch anchors the elapsed (parallel lanes).
        assert_eq!(fold.tool_inflight_since, Some(a_since));
        fold.apply("b", &completed("b"));
        assert_eq!(
            fold.tool_inflight_since,
            Some(a_since),
            "b's completion must not clear a's still-running batch"
        );
        fold.apply("a", &completed("a"));
        assert!(fold.tool_inflight_since.is_none(), "all batches done");
        // An approval-gated batch is NOT executing: waiting clears.
        let waiting = json!({"run_id": "w", "node_id": "act", "status": "waiting",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"name": "write_file", "arguments": {"path": "a"}}
            ]}},
            "result": {"wait": {"reason": "user", "wait_key": "tool_approval:k",
                "details": {"mode": "approval_required"}}}});
        fold.apply("w", &started("w"));
        assert!(fold.tool_inflight_since.is_some());
        fold.apply("w", &waiting);
        assert!(
            fold.tool_inflight_since.is_none(),
            "waiting-for-approval is not execution"
        );
        // Boundaries clear through the shared helper (begin_run/terminal).
        fold.apply("c", &started("c"));
        fold.begin_run("root2");
        assert!(fold.tool_inflight_since.is_none(), "begin_run clears");
        fold.apply("d", &started("d"));
        fold.run_terminal("completed");
        assert!(fold.tool_inflight_since.is_none(), "terminal clears");
    }

    #[test]
    fn tool_clock_survives_the_approval_round_trip_honestly() {
        // Adversary P1-1: the approved-resume execution window has no
        // second `started` record — the CLIENT re-arms at its own resume
        // (tool_resumed), and a REFUSED resume rolls back (reopen_wait).
        let mut fold = Fold::new();
        fold.begin_run("root");
        let started = json!({"run_id": "root", "node_id": "act", "status": "started",
        "effect": {"type": "tool_calls", "payload": {"tool_calls": [
            {"name": "execute_command", "arguments": {"command": "cargo build"}}
        ]}}});
        let waiting = json!({"run_id": "root", "node_id": "act", "status": "waiting",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"name": "execute_command", "arguments": {"command": "cargo build"}}
            ]}},
            "result": {"wait": {"reason": "user", "wait_key": "tool_approval:k",
                "details": {"mode": "approval_required"}}}});
        fold.apply("root", &started);
        fold.apply("root", &waiting);
        assert!(fold.tool_inflight_since.is_none(), "parked, not executing");
        // The client approves and resumes: the clock re-arms.
        let wait = fold.pending_wait.clone().expect("wait pending");
        fold.wait_answered(&wait.wait_key, &wait.step_id);
        fold.tool_resumed("root");
        assert!(
            fold.tool_inflight_since.is_some(),
            "approved resume re-arms the clock"
        );
        // The gateway refused the resume: rollback — parked again.
        fold.reopen_wait(wait);
        assert!(
            fold.tool_inflight_since.is_none(),
            "refused resume rolls the re-arm back"
        );
    }

    #[test]
    fn tool_clock_clears_on_failed_status_and_slim_completed_records() {
        // Adversary P2-4b: `failed` batches and $slim-payload completions
        // (runtime ledger dedup replaces effect.payload on terminal
        // records — exactly the big-args batches this clock targets)
        // must both clear.
        let mut fold = Fold::new();
        fold.begin_run("root");
        let started = json!({"run_id": "root", "node_id": "act", "status": "started",
        "effect": {"type": "tool_calls", "payload": {"tool_calls": [
            {"name": "search_files", "arguments": {"pattern": "x"}}
        ]}}});
        // The failed batch runs on a CHILD lane: a root-run failure record
        // also CONCLUDES the turn (finished=true), which the `!finished`
        // arming gate would then block for the second half of this test.
        let child_started = json!({"run_id": "a", "node_id": "act", "status": "started",
        "effect": {"type": "tool_calls", "payload": {"tool_calls": [
            {"name": "search_files", "arguments": {"pattern": "x"}}
        ]}}});
        fold.apply("a", &child_started);
        assert!(fold.tool_inflight_since.is_some());
        let failed = json!({"run_id": "a", "node_id": "act", "status": "failed",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": []}},
            "error": "boom"});
        fold.apply("a", &failed);
        assert!(fold.tool_inflight_since.is_none(), "failed clears");

        fold.apply("root", &started);
        assert!(fold.tool_inflight_since.is_some());
        let slim_completed = json!({"run_id": "root", "node_id": "act",
        "status": "completed",
        "effect": {"type": "tool_calls", "payload": {"$slim": {"of": "step"}}},
        "result": {"results": [
            {"name": "search_files", "success": true, "output": "ok"}
        ]}});
        fold.apply("root", &slim_completed);
        assert!(
            fold.tool_inflight_since.is_none(),
            "$slim completed record still clears (etype survives payload dedup)"
        );
    }

    #[test]
    fn inflight_clocks_clear_at_conclusion_and_terminal_subruns() {
        // Adversary P2-2: conclusion-by-answer is the COMMON end in
        // wrapper trees (run_terminal never fires there) — a dangling
        // clock rendered into the next turn's Starting window; and
        // subrun_terminal's early returns skipped per-run cleanup.
        let mut fold = Fold::new();
        fold.begin_run("root");
        let tool_started = |run: &str| {
            json!({"run_id": run, "node_id": "act", "status": "started",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"name": "search_files", "arguments": {"pattern": "x"}}
            ]}}})
        };
        fold.apply("root", &tool_started("root"));
        assert!(fold.tool_inflight_since.is_some());
        // The answer concludes the turn (flow output on the root).
        let answer = json!({"run_id": "root", "node_id": "flow_end",
            "status": "completed",
            "effect": {"type": "flow_output", "payload": {}},
            "result": {"output": {"response": "done"}, "completed": true}});
        fold.apply("root", &answer);
        assert!(fold.finished, "answer concluded");
        assert!(
            fold.tool_inflight_since.is_none(),
            "conclusion clears the inflight clocks"
        );
        // Post-conclusion arming is GATED (the stop-race fix): a started
        // that folds after the turn concluded must not tick into the
        // next turn's Starting window.
        fold.apply("late", &tool_started("late"));
        assert!(
            fold.tool_inflight_since.is_none(),
            "post-conclusion started never arms"
        );

        // subrun_terminal drops the terminated run's entries even when it
        // early-returns — goal mode (finish_on_root_only) is the live
        // early-return where arming still happens (finished stays false).
        let mut goal = Fold::new();
        goal.begin_run("root");
        goal.finish_on_root_only = true;
        goal.apply("iter1", &tool_started("iter1"));
        assert!(goal.tool_inflight_since.is_some());
        // An UNREADABLE status proves nothing — it must not drop entries
        // (the structural guard runs before any mutation).
        goal.subrun_terminal("iter1", "unknown");
        assert!(
            goal.tool_inflight_since.is_some(),
            "unknown status mutates nothing"
        );
        goal.subrun_terminal("iter1", "cancelled");
        assert!(
            goal.tool_inflight_since.is_none(),
            "terminal subrun's entries drop before the goal-mode early return"
        );
    }

    #[test]
    fn inflight_anchor_back_dates_from_the_record_timestamp() {
        // Adversary P2-3: reattach replays a mid-execution `started`
        // minutes late — anchoring at replay time under-reported the
        // elapsed. The anchor back-dates from the record's started_at.
        let mut fold = Fold::new();
        fold.begin_run("root");
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(300);
        let secs = past
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // RFC3339 UTC from epoch seconds (no external deps in tests).
        let days = secs / 86_400;
        let (h, m, s) = ((secs % 86_400) / 3600, (secs % 3600) / 60, secs % 60);
        // Convert days-since-epoch to a civil date (Howard Hinnant's
        // algorithm, as used by the protocol parser's inverse).
        let (y, mo, d) = {
            let z = days as i64 + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z.rem_euclid(146_097);
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let y = yoe + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let mo = if mp < 10 { mp + 3 } else { mp - 9 };
            (if mo <= 2 { y + 1 } else { y }, mo, d)
        };
        let stamp = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z");
        let started = json!({"run_id": "root", "node_id": "act", "status": "started",
        "started_at": stamp,
        "effect": {"type": "tool_calls", "payload": {"tool_calls": [
            {"name": "search_files", "arguments": {"pattern": "x"}}
        ]}}});
        fold.apply("root", &started);
        let since = fold.tool_inflight_since.expect("armed");
        let elapsed = since.elapsed().as_secs();
        assert!(
            (295..=305).contains(&elapsed),
            "back-dated anchor reports ~300s immediately, got {elapsed}s"
        );
    }

    #[test]
    fn run_terminal_unknown_is_an_honest_failure() {
        // F4: a stream that ends with an unreadable final status must say
        // so (error card + failed) — never a silent hang, never a
        // fabricated success.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.run_terminal("unknown");
        assert!(fold.finished);
        assert!(fold.failed, "unknown terminal is a Failed outcome");
        assert!(matches!(
            last_content(&fold),
            Item::Error { text } if text.contains("final status could not be read")
        ));
    }

    #[test]
    fn cycling_target_is_none_until_a_cycle_and_clears_on_begin_run() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        assert_eq!(fold.cycling_target(), None, "no cycle yet");
        fold.apply(
            "sub1",
            &json!({"run_id": "sub1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        assert_eq!(fold.cycling_target().as_deref(), Some("sub1"));
        // begin_run clears it: a stale old-run cycle can never satisfy a
        // new run's delivery predicate.
        fold.begin_run("root2");
        assert_eq!(fold.cycling_target(), None);
    }

    // -- subrun_terminal (the failed-agent P0, lane A 2026-07-23) ----------

    /// Discover `sub` from `root` (the subworkflow wait the runner
    /// follows) and make it the cycling answer-source agent run.
    fn wire_agent_subrun(fold: &mut Fold, root: &str, sub: &str) {
        fold.apply(
            root,
            &json!({"run_id": root, "status": "waiting",
                "result": {"wait": {"reason": "subworkflow",
                    "wait_key": format!("subworkflow:{sub}"),
                    "details": {"sub_run_id": sub}}}}),
        );
        fold.apply(
            sub,
            &json!({"run_id": sub, "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
    }

    #[test]
    fn failed_answer_source_subrun_concludes_the_turn() {
        // Live tree 76fc3fcb…/9c5cad22… (2026-07-22): the agent subrun
        // died terminally ("Model unloaded.") at cycle 1; the wrapper
        // root ABSORBED the failure and parked forever on its status
        // poller. The failed RECORD alone must not conclude (records
        // retry/absorb) — the run's TERMINAL STATUS does.
        let mut fold = Fold::new();
        fold.begin_run("root");
        wire_agent_subrun(&mut fold, "root", "agent1");
        fold.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "reason", "status": "failed",
                    "effect": {"type": "llm_call", "payload": {}},
                    "error": "LMStudio API error (400): {\"error\": \"Model unloaded.\"}"}),
        );
        assert!(
            !fold.finished,
            "a failed effect record is not a conclusion (retries/absorption exist)"
        );
        fold.subrun_terminal("agent1", "failed");
        assert!(fold.finished, "the agent run's terminal failure concludes");
        assert!(fold.failed, "and it is a Failed outcome");
        assert!(matches!(
            last_content(&fold),
            Item::Error { text } if text.contains("the agent run ended: failed")
        ));
        assert!(fold.pending_wait.is_none());
        assert!(fold.activity.is_empty());
    }

    #[test]
    fn subrun_terminal_ignores_helpers_unknowns_and_goal_iterations() {
        // Helper subruns (pollers/status emitters) terminate all the time
        // while the turn is live — only the ANSWER-SOURCE run concludes.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:helper1",
                    "details": {"sub_run_id": "helper1"}}}}),
        );
        fold.subrun_terminal("helper1", "completed");
        assert!(!fold.finished, "a helper's terminal never concludes");
        // An unfollowed/unbound run id (no agent bound yet) never concludes.
        fold.subrun_terminal("stranger", "failed");
        assert!(!fold.finished);
        // An UNREADABLE status never concludes — even for the BOUND agent
        // (cycle-2 review F5: the guard was caller-etiquette only; a
        // transient status-read failure must not kill a healthy run).
        let mut bound = Fold::new();
        bound.begin_run("root");
        wire_agent_subrun(&mut bound, "root", "agent1");
        bound.subrun_terminal("agent1", "unknown");
        assert!(
            !bound.finished && !bound.failed,
            "unknown status is a no-op even on the bound answer source"
        );
        // A real terminal afterwards still concludes normally.
        bound.subrun_terminal("agent1", "failed");
        assert!(bound.finished && bound.failed);
        // Goal mode: iteration subruns terminate PER ITERATION — the root
        // owns the loop's fate (finish_on_root_only defense).
        let mut goal = Fold::new();
        goal.begin_run("root");
        goal.finish_on_root_only = true;
        wire_agent_subrun(&mut goal, "root", "iter1");
        goal.subrun_terminal("iter1", "failed");
        assert!(!goal.finished, "goal iterations never conclude from here");
    }

    #[test]
    fn answerless_completed_agent_subrun_concludes_honestly() {
        // The agent run ENDED (terminal completed) but no conclusion
        // record ever folded (offloaded/unconventional output): free the
        // composer with the honest info card instead of spinning forever.
        let mut fold = Fold::new();
        fold.begin_run("root");
        wire_agent_subrun(&mut fold, "root", "agent1");
        fold.subrun_terminal("agent1", "completed");
        assert!(fold.finished);
        assert!(!fold.failed, "an answerless completion is not a failure");
        assert!(matches!(
            last_content(&fold),
            Item::Info { text } if text.contains("without a readable final answer")
        ));
        // Idempotent + never double-concluding: a later terminal no-ops.
        let items_before = fold.items.len();
        fold.subrun_terminal("agent1", "failed");
        assert_eq!(fold.items.len(), items_before);
        assert!(!fold.failed);
    }

    /// Operator ruling (2026-07-26): user-facing text names CLIENT
    /// actions or plain facts — never storage internals ("a user will
    /// most likely NEVER read the ledger"). Drives every notice/error-
    /// minting path this fold owns, then sweeps the words. Code
    /// comments and ops surfaces may still say "ledger"; Item text may
    /// not.
    #[test]
    fn user_facing_fold_texts_never_point_at_the_ledger() {
        fn sweep(context: &str, texts: &[String]) {
            for t in texts {
                assert!(
                    !t.to_lowercase().contains("ledger"),
                    "{context}: user-facing text points at the ledger: {t:?}"
                );
            }
        }
        fn all_texts(fold: &Fold) -> Vec<String> {
            let mut out = Vec::new();
            for item in &fold.items {
                match item {
                    Item::User { text }
                    | Item::Steer { text }
                    | Item::Assistant { text, .. }
                    | Item::Info { text }
                    | Item::Error { text } => out.push(text.clone()),
                    Item::Thinking {
                        content, reasoning, ..
                    } => {
                        out.push(content.clone());
                        out.push(reasoning.clone());
                    }
                    Item::Tool {
                        args_preview,
                        result,
                        error,
                        ..
                    } => {
                        out.push(args_preview.clone());
                        out.push(result.clone());
                        out.push(error.clone());
                    }
                    Item::Image { label, .. } => out.push(label.clone()),
                    Item::Probe { title, body } => {
                        out.push(title.clone());
                        out.push(body.clone());
                    }
                }
            }
            out
        }

        // Direct text builders. (`bounded` is gone: the fold no longer
        // shortens anything on the way in — 2026-08-20.)
        sweep(
            "builders",
            &[
                offload_failure_label("HTTP 500"),
                offload_placeholder("abc123"),
            ],
        );

        // The view-cap drop notice (and everything else already folded).
        let mut fold = Fold::new();
        fold.begin_run("root");
        for i in 0..(MAX_ITEMS + TRUNCATE_CHUNK + 1) {
            fold.push_item(Item::Info {
                text: format!("filler {i}"),
            });
        }
        assert!(fold.truncated(), "the drop notice is in");
        // An offloaded TOOL result (the artifact preview label).
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "act", "status": "completed",
                "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                    {"name": "read_file", "arguments": {"path": "big"}}]}},
                "result": {"results": [{"name": "read_file", "success": true,
                    "output": {"$artifact": "e3b19ad9e42a2b725048bab40138f975"}}]}}),
        );
        // A failed offloaded-answer fetch (the honest failure card).
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {"$artifact": "lostart"}}}),
        );
        fold.resolve_offloaded_answer("lostart", Err("gateway unreachable".into()));
        sweep("main fold", &all_texts(&fold));

        // The unreadable-terminal error and the answerless conclusions.
        let mut unknown = Fold::new();
        unknown.begin_run("root");
        unknown.run_terminal("unknown");
        sweep("run_terminal unknown", &all_texts(&unknown));

        let mut answerless = Fold::new();
        answerless.begin_run("root");
        wire_agent_subrun(&mut answerless, "root", "agent1");
        answerless.subrun_terminal("agent1", "completed");
        sweep("answerless completion", &all_texts(&answerless));

        let mut weird = Fold::new();
        weird.begin_run("root");
        weird.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        weird.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {"weird_key": 42}}}),
        );
        sweep("flow end without readable output", &all_texts(&weird));
    }

    #[test]
    fn answer_run_id_names_the_bound_agent_only() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        assert_eq!(fold.answer_run_id(), None, "nothing bound yet");
        wire_agent_subrun(&mut fold, "root", "agent1");
        assert_eq!(fold.answer_run_id(), Some("agent1"));
        // A root-level cycler (flat agent flow) is covered by the root
        // terminal paths — answer_run_id stays None.
        let mut flat = Fold::new();
        flat.begin_run("root");
        flat.apply(
            "root",
            &json!({"run_id": "root", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        assert_eq!(flat.answer_run_id(), None);
    }

    #[test]
    fn splitless_usage_repairs_ctx_totals_and_session_from_raw_response() {
        // Token counters for the coder shape (task 3): the strip read
        // "0↑ 0↓" and ctx never moved because the NORMALIZED usage is
        // splitless — the raw provider block on the same record carries
        // the truth. Totals, session, last_input_tokens (ctx) all repair.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "reason", "status": "completed",
                "effect": {"type": "llm_call", "payload": {}},
                "result": {
                    "content": "…", "model": "gpt-5.6-sol", "gen_time": 7584.5,
                    "usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 8282,
                               "prompt_tokens": 0, "completion_tokens": 0},
                    "raw_response": {"usage": {
                        "input_tokens": 7975,
                        "input_tokens_details": {"cache_write_tokens": 0, "cached_tokens": 2560},
                        "output_tokens": 307,
                        "output_tokens_details": {"reasoning_tokens": 20},
                        "total_tokens": 8282}}}}),
        );
        assert_eq!(fold.stats.input_tokens, 7975);
        assert_eq!(fold.stats.output_tokens, 307);
        assert_eq!(fold.stats.total_tokens, 8282, "normalized total kept");
        assert_eq!(fold.stats.cached_tokens, 2560, "nested details cache hits");
        assert_eq!(fold.stats.last_input_tokens, 7975, "ctx meter feeds");
        assert_eq!(fold.session.input_tokens, 7975);
        assert_eq!(fold.session.output_tokens, 307);
        assert_eq!(fold.stats.effective_model, "gpt-5.6-sol");
    }

    /// The cache panel's new-vs-carried split. Providers of the llama.cpp class
    /// never report `cached_input_tokens`, so the only cache-relevant thing a
    /// client can state is how much of THIS prompt is new against the previous
    /// call — a difference between two numbers it already holds.
    #[test]
    fn prev_input_tokens_tracks_the_call_before_the_newest() {
        let call = |input: u64, output: u64| {
            json!({"run_id": "root", "node_id": "reason", "status": "completed",
                   "effect": {"type": "llm_call", "payload": {}},
                   "result": {"content": "…", "usage": {
                       "input_tokens": input, "output_tokens": output,
                       "total_tokens": input + output}}})
        };
        let mut fold = Fold::new();
        fold.begin_run("root");

        fold.apply("root", &call(4_396, 111));
        assert_eq!(fold.stats.last_input_tokens, 4_396);
        assert_eq!(fold.stats.prev_input_tokens, 0, "no previous call yet");

        // A 20k-token tool result folds into the transcript: the next prompt is
        // mostly carried, and the NEW part is what actually has to be evaluated.
        fold.apply("root", &call(24_831, 17_405));
        assert_eq!(fold.stats.last_input_tokens, 24_831);
        assert_eq!(fold.stats.prev_input_tokens, 4_396);
        assert_eq!(
            fold.stats.last_input_tokens - fold.stats.prev_input_tokens,
            20_435,
            "new tokens on this call"
        );

        // Compaction shrinks the context: the panel must not underflow.
        fold.apply("root", &call(9_000, 50));
        assert_eq!(fold.stats.prev_input_tokens, 24_831);
        assert!(fold.stats.last_input_tokens < fold.stats.prev_input_tokens);

        // Re-send amplification, the other number the panel reports.
        assert_eq!(fold.stats.input_tokens, 38_227);
        assert_eq!(fold.stats.output_tokens, 17_566);
        assert_eq!(fold.stats.llm_calls, 3);
    }

    /// Session-scope cache accounting: per-run `Stats` reset at
    /// `begin_run`, so the only thing that can answer "how is the cache
    /// doing overall?" is `SessionStats`. It must survive the run
    /// boundary, and the derived new/carried split must be accumulated
    /// per call rather than re-derived from the surviving endpoints
    /// (which would silently drop every intermediate cycle).
    #[test]
    fn session_cache_metrics_accumulate_across_runs() {
        let call = |input: u64, output: u64, cached: u64, gen_ms: f64| {
            json!({"run_id": "root", "node_id": "reason", "status": "completed",
                   "effect": {"type": "llm_call", "payload": {}},
                   "result": {"content": "…", "gen_time": gen_ms, "usage": {
                       "input_tokens": input, "output_tokens": output,
                       "total_tokens": input + output,
                       "prompt_tokens_details": {"cached_tokens": cached}}}})
        };
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply("root", &call(1_000, 100, 0, 2_000.0));
        fold.apply("root", &call(3_000, 100, 900, 4_000.0));
        // Run 1: 1000 new (first call) + 2000 new / 1000 carried.
        assert_eq!(fold.stats.new_tokens, 3_000);
        assert_eq!(fold.stats.carried_tokens, 1_000);
        assert_eq!(fold.stats.cached_tokens, 900);
        assert_eq!(fold.stats.cache_reported_calls, 1, "one call reported hits");
        assert_eq!(fold.stats.cacheable_input_tokens, 4_000);
        assert_eq!(fold.stats.peak_input_tokens, 3_000);
        assert_eq!(fold.stats.gen_time_ms, 6_000);
        assert_eq!(
            fold.stats.last_cached_tokens, 900,
            "newest call, not the total"
        );
        assert_eq!(fold.stats.last_output_tokens, 100);
        assert_eq!(fold.stats.last_gen_time_ms, 4_000);

        // A second run wipes the per-run stats and keeps the session's.
        fold.begin_run("root2");
        assert_eq!(fold.stats.new_tokens, 0, "per-run stats reset");
        assert_eq!(fold.stats.cached_tokens, 0);
        fold.apply(
            "root2",
            &json!({"run_id": "root2", "node_id": "reason", "status": "completed",
                    "effect": {"type": "llm_call", "payload": {}},
                    "result": {"content": "…", "usage": {
                        "input_tokens": 5_000, "output_tokens": 200,
                        "total_tokens": 5_200}}}),
        );
        assert_eq!(fold.session.runs, 2);
        assert_eq!(fold.session.llm_calls, 3);
        assert_eq!(
            fold.session.cached_tokens, 900,
            "session cache total survives"
        );
        assert_eq!(fold.session.cache_reported_calls, 1);
        assert_eq!(fold.session.cacheable_input_tokens, 9_000);
        assert_eq!(
            fold.session.new_tokens, 8_000,
            "run 2's first call is all new"
        );
        assert_eq!(fold.session.carried_tokens, 1_000);
        assert_eq!(fold.session.peak_input_tokens, 5_000);
        assert_eq!(
            fold.session.gen_time_ms, 6_000,
            "unreported gen_time adds 0"
        );
        assert_eq!(fold.session.context_resets, 0);
    }

    /// A shrinking context breaks the cacheable prefix: nothing is
    /// credited as carried, and the reset is COUNTED — a silent shrink
    /// would leave the reuse rate looking healthy while every call after
    /// it re-evaluated from scratch.
    #[test]
    fn a_context_shrink_counts_as_a_reset_and_credits_no_carry() {
        let call = |input: u64| {
            json!({"run_id": "root", "node_id": "reason", "status": "completed",
                   "effect": {"type": "llm_call", "payload": {}},
                   "result": {"content": "…", "usage": {
                       "input_tokens": input, "output_tokens": 10,
                       "total_tokens": input + 10}}})
        };
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply("root", &call(10_000));
        fold.apply("root", &call(30_000));
        fold.apply("root", &call(4_000)); // compaction
        assert_eq!(fold.stats.context_resets, 1);
        assert_eq!(fold.session.context_resets, 1);
        assert_eq!(
            fold.stats.carried_tokens, 10_000,
            "only the growing call carried"
        );
        assert_eq!(fold.stats.new_tokens, 10_000 + 20_000 + 4_000);
        assert_eq!(fold.stats.peak_input_tokens, 30_000);
    }

    /// A DELEGATE child's call must not relabel the latest-call block —
    /// the same lane rule the ctx chip and served-model line already
    /// follow. Cumulative totals still fold from every followed run.
    #[test]
    fn delegate_calls_never_relabel_the_latest_call_block() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "reason", "status": "completed",
                    "effect": {"type": "llm_call", "payload": {}},
                    "result": {"content": "…", "usage": {
                        "input_tokens": 20_000, "output_tokens": 500,
                        "total_tokens": 20_500,
                        "prompt_tokens_details": {"cached_tokens": 8_000}}}}),
        );
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:kid",
                    "details": {"sub_run_id": "kid"}}}}),
        );
        fold.apply(
            "kid",
            &json!({"run_id": "kid", "node_id": "reason", "status": "completed",
                    "effect": {"type": "llm_call", "payload": {}},
                    "result": {"content": "…", "usage": {
                        "input_tokens": 300, "output_tokens": 20, "total_tokens": 320}}}),
        );
        assert_eq!(
            fold.stats.last_input_tokens, 20_000,
            "delegate never relabels"
        );
        assert_eq!(fold.stats.last_cached_tokens, 8_000);
        assert_eq!(fold.stats.last_output_tokens, 500);
        // …but the tree's real spend still counts.
        assert_eq!(fold.stats.input_tokens, 20_300);
        assert_eq!(fold.session.llm_calls, 2);
        assert_eq!(fold.stats.cacheable_input_tokens, 20_300);
    }

    #[test]
    fn deep_cycler_feeds_ctx_and_model_when_no_first_level_agent_exists() {
        // The coder tree cycles at depth 2–3 (parents ≠ root) so no
        // first-level agent ever binds — the LIVE CYCLER is the honest
        // telemetry source there. Once a first-level agent exists, deeper
        // children still never relabel (delegate-pollution rule).
        let mut fold = Fold::new();
        fold.begin_run("root");
        // root → level1 (never cycles) → builder (cycles).
        fold.apply(
            "root",
            &json!({"run_id": "root", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:level1",
                    "details": {"sub_run_id": "level1"}}}}),
        );
        fold.apply(
            "level1",
            &json!({"run_id": "level1", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:builder",
                    "details": {"sub_run_id": "builder"}}}}),
        );
        fold.apply(
            "builder",
            &json!({"run_id": "builder", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        assert_eq!(fold.answer_run_id(), None, "depth-2 cycler never binds");
        fold.apply(
            "builder",
            &json!({"run_id": "builder", "node_id": "reason", "status": "completed",
                "effect": {"type": "llm_call", "payload": {}},
                "result": {"content": "…", "model": "gpt-5.6-sol",
                            "usage": {"input_tokens": 22480, "output_tokens": 493}}}),
        );
        assert_eq!(
            fold.stats.last_input_tokens, 22480,
            "the live cycler feeds ctx when no first-level agent exists"
        );
        assert_eq!(fold.stats.effective_model, "gpt-5.6-sol");

        // With a FIRST-LEVEL agent bound, a delegate child's call must
        // not relabel (the pollution rule survives the widening).
        let mut basic = Fold::new();
        basic.begin_run("root");
        wire_agent_subrun(&mut basic, "root", "agent1");
        basic.apply(
            "agent1",
            &json!({"run_id": "agent1", "node_id": "reason", "status": "completed",
                "effect": {"type": "llm_call", "payload": {}},
                "result": {"content": "…", "model": "big-model",
                            "usage": {"input_tokens": 9000, "output_tokens": 50}}}),
        );
        basic.apply(
            "agent1",
            &json!({"run_id": "agent1", "status": "waiting",
                "result": {"wait": {"reason": "subworkflow", "wait_key": "subworkflow:child1",
                    "details": {"sub_run_id": "child1"}}}}),
        );
        basic.apply(
            "child1",
            &json!({"run_id": "child1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        basic.apply(
            "child1",
            &json!({"run_id": "child1", "node_id": "reason", "status": "completed",
                "effect": {"type": "llm_call", "payload": {}},
                "result": {"content": "…", "model": "tiny-delegate",
                            "usage": {"input_tokens": 42, "output_tokens": 5}}}),
        );
        assert_eq!(
            basic.stats.last_input_tokens, 9000,
            "delegate never relabels ctx"
        );
        assert_eq!(basic.stats.effective_model, "big-model");
    }

    // -- structural answer-source binding (conformance lane, 2026-07-23) ---

    /// The REAL spawn-record shape (live gateway run 76fc3fcb…, node-2):
    /// the parent's wait record declares the child's workflow id.
    fn spawn_rec(parent: &str, sub: &str, workflow_id: &str, wrap: bool) -> serde_json::Value {
        let mut details = json!({
            "sub_run_id": sub,
            "sub_workflow_id": workflow_id,
            "async": true
        });
        if wrap {
            details["wrap_as_tool_result"] = json!(true);
        }
        json!({"run_id": parent, "node_id": "node-2", "status": "waiting",
            "effect": {"type": "start_subworkflow",
                        "payload": {"workflow_id": workflow_id, "async": true, "wait": true}},
            "result": {"wait": {"reason": "subworkflow",
                "wait_key": format!("subworkflow:{sub}"),
                "details": details}}})
    }

    #[test]
    fn spawn_declared_agent_binds_before_any_cycle_and_concludes_when_it_dies_first() {
        // THE SPECIMEN FIX: the ledger declares the agent at spawn — an
        // agent child that dies BEFORE its first reason-cycle record is
        // already bound, and its terminal status concludes the turn
        // (previously: never bound, composer captured forever —
        // the "dies before first cycle" residual).
        let mut fold = Fold::new();
        fold.begin_run("root");
        // A helper spawn first (the basic-agent node-4 status flow):
        // declared with a NON-agent workflow id — never binds.
        fold.apply(
            "root",
            &spawn_rec("root", "helper1", "basic-agent@0.0.3:15f19f7f", false),
        );
        assert_eq!(fold.answer_run_id(), None, "helpers never bind");
        // The agent spawn (the runtime's Agent-node id contract).
        let fx = fold.apply(
            "root",
            &spawn_rec(
                "root",
                "agent1",
                "visual_react_agent_basic-agent_0_0_3_81795ea9_node-2",
                false,
            ),
        );
        assert!(fx.contains(&FoldEffect::FollowRun("agent1".into())));
        assert_eq!(
            fold.answer_run_id(),
            Some("agent1"),
            "the agent binds AT SPAWN, before any cycle record"
        );
        // The child dies with ZERO records of its own (crashed before
        // its first cycle). The terminal report concludes honestly.
        fold.subrun_terminal("agent1", "failed");
        assert!(fold.finished, "the turn concludes");
        assert!(fold.failed, "as a failure");
        assert!(matches!(
            last_content(&fold),
            Item::Error { text } if text.contains("the agent run ended: failed")
        ));
        // A helper terminal after conclusion changes nothing.
        let n = fold.items.len();
        fold.subrun_terminal("helper1", "completed");
        assert_eq!(fold.items.len(), n);
    }

    #[test]
    fn catalog_declared_agent_workflow_binds_and_the_set_survives_begin_run() {
        // The second structural source: the catalog's agent-interface set
        // (entrypoint workflow_ids carrying abstractcode.agent.v1) — the
        // wrapper-spawns-a-catalog-agent shape. Ids are OPAQUE here (the
        // rule is membership, never name matching).
        let mut fold = Fold::new();
        fold.set_agent_workflows(vec!["wrapper-kit@1.2.3:inner-agent".to_string()]);
        fold.begin_run("root");
        fold.apply(
            "root",
            &spawn_rec("root", "sub9", "wrapper-kit@1.2.3:inner-agent", false),
        );
        assert_eq!(
            fold.answer_run_id(),
            Some("sub9"),
            "catalog-set membership binds at spawn"
        );
        // The set is CATALOG state: a new run in the same session still
        // has it (begin_run clears run state only).
        fold.begin_run("root2");
        assert_eq!(fold.answer_run_id(), None, "run state cleared");
        fold.apply(
            "root2",
            &spawn_rec("root2", "sub10", "wrapper-kit@1.2.3:inner-agent", false),
        );
        assert_eq!(fold.answer_run_id(), Some("sub10"));
        // An id OUTSIDE both sources does not bind at spawn (the cycle
        // #FALLBACK may still bind it later — hand-registered agents).
        let mut other = Fold::new();
        other.begin_run("root");
        other.apply(
            "root",
            &spawn_rec("root", "subX", "hand-rolled@0.1.0:mystery", false),
        );
        assert_eq!(other.answer_run_id(), None);
    }

    #[test]
    fn tool_mode_children_never_bind_even_when_they_cycle() {
        // The delegate hazard, fixed BY STRUCTURE: a delegate child runs
        // its parent's OWN workflow id (react_runtime.py delegate_agent
        // payload) and CYCLES — behavior-based binding would adopt it and
        // its intermediate flow end would falsely conclude the turn. The
        // ledger's wrap_as_tool_result declaration excludes it from both
        // the spawn binding AND the cycle #FALLBACK.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &spawn_rec(
                "root",
                "delegate1",
                "visual_react_agent_flat_root_node-1",
                true, // wrap_as_tool_result: a tool observation by contract
            ),
        );
        assert_eq!(fold.answer_run_id(), None, "tool-mode spawn never binds");
        // The delegate cycles — the #FALLBACK must NOT adopt it either.
        fold.apply(
            "delegate1",
            &json!({"run_id": "delegate1", "node_id": "reason", "status": "started",
                    "effect": {"type": "llm_call", "payload": {}}}),
        );
        assert_eq!(
            fold.answer_run_id(),
            None,
            "a ledger-declared tool child never binds, cycling or not"
        );
        // Its answer-shaped end is a tool observation, not the answer.
        fold.apply(
            "delegate1",
            &json!({"run_id": "delegate1", "node_id": "done", "status": "completed",
                    "result": {"completed": true, "output": {"answer": "delegate words"}}}),
        );
        assert!(
            !fold.finished,
            "a tool child's end never concludes the turn"
        );
        // The ROOT's own end still does.
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "end", "status": "completed",
                    "result": {"completed": true, "output": {"answer": "the real answer"}}}),
        );
        assert!(fold.finished);
    }

    #[test]
    fn deep_agent_spawns_never_bind_as_answer_source() {
        // Coder-tree protection: an agent-shaped spawn whose PARENT is
        // not the root (level-2/3 verifier/builder agents) is a delegate
        // of a deeper run — the first-level rule is structural (the
        // spawning record's own run id).
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &spawn_rec("root", "level1", "coding-agent@0.2.4:coding-agent", false),
        );
        fold.apply(
            "level1",
            &spawn_rec(
                "level1",
                "verifier",
                "visual_react_agent_coding-verify_node-3",
                false,
            ),
        );
        assert_eq!(
            fold.answer_run_id(),
            None,
            "a deep agent spawn never binds — answers come from the root"
        );
    }

    #[test]
    fn goal_spawn_binding_follows_the_live_iteration() {
        // finish_on_root_only + structural binding: each iteration's
        // spawn re-binds the answer lane at BIRTH (before its first
        // cycle) — telemetry follows the live iteration immediately.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.finish_on_root_only = true;
        fold.apply(
            "root",
            &spawn_rec("root", "iter1", "visual_react_agent_goal_node-2", false),
        );
        assert_eq!(fold.answer_run_id(), Some("iter1"));
        fold.apply(
            "root",
            &spawn_rec("root", "iter2", "visual_react_agent_goal_node-2", false),
        );
        assert_eq!(
            fold.answer_run_id(),
            Some("iter2"),
            "goal mode follows the newest iteration from its spawn record"
        );
        // Normal mode keeps first-wins (a late second first-level agent
        // spawn must never steal the answer lane mid-turn).
        let mut normal = Fold::new();
        normal.begin_run("root");
        normal.apply(
            "root",
            &spawn_rec("root", "a1", "visual_react_agent_x_node-1", false),
        );
        normal.apply(
            "root",
            &spawn_rec("root", "a2", "visual_react_agent_x_node-9", false),
        );
        assert_eq!(normal.answer_run_id(), Some("a1"), "first-wins holds");
    }

    #[test]
    fn sync_subworkflow_completion_output_is_not_the_answer() {
        // A SYNC start_subworkflow completion carries the CHILD's output
        // on the PARENT's ledger ({"sub_run_id", "output"} — runtime.py
        // _handle_start_subworkflow). Reading it as the parent's final
        // answer concluded turns early while the root kept executing.
        let mut fold = Fold::new();
        fold.begin_run("root");
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "node-3", "status": "completed",
                "effect": {"type": "start_subworkflow",
                            "payload": {"workflow_id": "some@1.0.0:child", "async": false}},
                "result": {"sub_run_id": "child77",
                            "output": {"answer": "the child's words"}}}),
        );
        assert!(
            !fold.finished,
            "a sync spawn's completion is the child's output, never the run's end"
        );
        assert!(
            !fold
                .items
                .iter()
                .any(|i| matches!(i, Item::Assistant { .. })),
            "no assistant card from an effect result: {:?}",
            fold.items
        );
        // The root's OWN terminal record (the runtime marker) concludes.
        fold.apply(
            "root",
            &json!({"run_id": "root", "node_id": "end", "status": "completed",
                    "result": {"completed": true, "output": {"answer": "root's answer"}}}),
        );
        assert!(fold.finished);
        assert!(matches!(
            last_content(&fold),
            Item::Assistant { text, final_answer: true } if text == "root's answer"
        ));
    }

    /// The humane args preview (adversarial design review, 2026-08-19):
    /// the identifying value leads bare, the rest follow as k=v, long
    /// absolute paths keep their basename. The ledger keeps the
    /// verbatim JSON; this is the row's answer to "which call was
    /// this".
    #[test]
    fn tool_args_preview_leads_with_the_identifying_value() {
        let args = serde_json::json!({
            "end_line": "1187",
            "file_path": "/Users/albou/tmp/abstractframework/todel2/js/game.js",
            "start_line": "720",
        });
        let p = tool_args_preview(Some(&args), 200);
        assert!(
            p.starts_with("…") && p.contains("/js/game.js"),
            "file_path leads, tail-compacted at a '/' boundary: {p}"
        );
        assert!(
            p.contains("end_line=1187") && p.contains("start_line=720"),
            "the remaining args follow as k=v: {p}"
        );
        // Semantic order (round-2 F1): ranges read forward, diffs
        // original-first — alphabetical printed them backwards.
        assert!(
            p.find("start_line").unwrap() < p.find("end_line").unwrap(),
            "start before end: {p}"
        );
        let edit = serde_json::json!({
            "file_path": "js/game.js",
            "new": "b",
            "old": "a",
        });
        let p = tool_args_preview(Some(&edit), 200);
        assert!(
            p.find("old=a").unwrap() < p.find("new=b").unwrap(),
            "a diff reads original-then-replacement: {p}"
        );
        assert!(
            !p.contains('{') && !p.contains('"'),
            "no JSON syntax in the preview: {p}"
        );

        // `command` outranks everything and renders bare.
        let args = serde_json::json!({"command": "node smoke.js --level 3"});
        assert_eq!(
            tool_args_preview(Some(&args), 200),
            "node smoke.js --level 3"
        );

        // Non-object shapes render as plain one-line previews.
        assert_eq!(tool_args_preview(None, 200), "");
        assert_eq!(tool_args_preview(Some(&serde_json::Value::Null), 200), "");
        let s = serde_json::Value::String("raw string args".into());
        assert_eq!(tool_args_preview(Some(&s), 200), "raw string args");

        // Nested values stay honest as compact JSON, never dropped.
        let args = serde_json::json!({"filters": {"lang": "rs"}, "query": "fold"});
        let p = tool_args_preview(Some(&args), 200);
        assert!(
            p.starts_with("fold") && p.contains("filters="),
            "query leads; the nested arg is named: {p}"
        );
    }

    /// A result object with a human `rendered` string shows THAT, not
    /// the transport envelope around it (adversarial design review,
    /// 2026-08-19) — and a failure in the envelope still fails the
    /// card through `success`/`error`, so no failure hides behind the
    /// payload.
    #[test]
    fn finish_tool_prefers_the_rendered_payload_over_the_envelope() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        let started = serde_json::json!({
            "run_id": "root", "node_id": "tools", "status": "started",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"id": "c1", "name": "execute_command",
                 "arguments": {"command": "wc -l js/game.js"}}
            ]}},
        });
        fold.apply("root", &started);
        let done = serde_json::json!({
            "run_id": "root", "node_id": "tools", "status": "completed",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"id": "c1", "name": "execute_command",
                 "arguments": {"command": "wc -l js/game.js"}}
            ]}},
            "result": {"results": [
                {"call_id": "c1", "name": "execute_command", "success": true,
                 "output": {
                     "command": "wc -l js/game.js",
                     "duration_s": 0.09,
                     "error": null,
                     "platform": "Darwin",
                     "rendered": "301 js/game.js"
                 }}
            ]},
        });
        fold.apply("root", &done);
        let (args, result) = fold
            .items
            .iter()
            .find_map(|i| match i {
                Item::Tool {
                    args_preview,
                    result,
                    status: ToolStatus::Ok,
                    ..
                } => Some((args_preview.clone(), result.clone())),
                _ => None,
            })
            .expect("finished tool card");
        assert_eq!(args, "wc -l js/game.js", "humane args preview: {args}");
        assert_eq!(
            result, "301 js/game.js",
            "the rendered payload is the preview, not the envelope: {result}"
        );
        assert!(
            !result.contains("platform"),
            "no transport keys in the preview: {result}"
        );
    }

    /// A terminal run restamps unfinished tool rows (adversarial
    /// review round 2, F3): "running" is a present-tense claim — once
    /// the tree is terminal nothing will ever complete the row, and
    /// scrollback must say so ("interrupted"), not lie forever.
    #[test]
    fn run_terminal_restamps_unfinished_tool_rows_as_interrupted() {
        let mut fold = Fold::new();
        fold.begin_run("root");
        let started = serde_json::json!({
            "run_id": "root", "node_id": "tools", "status": "started",
            "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                {"id": "c1", "name": "execute_command",
                 "arguments": {"command": "node soak.js"}}
            ]}},
        });
        fold.apply("root", &started);
        assert!(fold.items.iter().any(|i| matches!(
            i,
            Item::Tool {
                status: ToolStatus::Running,
                ..
            }
        )));
        fold.run_terminal("cancelled");
        assert!(
            fold.items.iter().any(|i| matches!(
                i,
                Item::Tool {
                    status: ToolStatus::Interrupted,
                    ..
                }
            )),
            "the running row restamps to interrupted at run end"
        );
        assert!(
            !fold.items.iter().any(|i| matches!(
                i,
                Item::Tool {
                    status: ToolStatus::Running,
                    ..
                }
            )),
            "no present-tense rows survive a terminal run"
        );
    }
}
