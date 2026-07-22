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
const RESULT_PREVIEW_MAX: usize = 700;
const TEXT_BLOCK_MAX: usize = 8_000;
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
    },
    Tool {
        key: String,
        name: String,
        args_preview: String,
        status: ToolStatus,
        result_preview: String,
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum WaitKind {
    Approval { tool_calls: Vec<Value> },
    Ask { prompt: String },
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
    pub llm_calls: u64,
    pub tool_calls: u64,
    /// Output tokens per completed llm_call, newest last (sparkline food).
    pub output_series: Vec<f32>,
    /// Input tokens of the NEWEST llm_call — the context size the model
    /// received on the latest cycle (the honest "context used" number).
    pub last_input_tokens: u64,
    /// Cumulative prompt tokens served from the provider cache this run
    /// (0 when the provider never reports cache hits).
    pub cached_tokens: u64,
    /// The model that actually served the newest llm_call — the resolved
    /// truth even under "gateway defaults" (from the result's own field).
    pub effective_model: String,
}

/// Lifetime-of-fold totals (across runs; per-run stats reset per run).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub runs: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FoldEffect {
    /// A subworkflow run was discovered; the runner should stream it too.
    FollowRun(String),
    /// An image artifact appeared; fetch bytes and hand them to the UI.
    FetchImage { run_id: String, artifact_id: String },
}

fn one_line(text: &str, max: usize) -> String {
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

fn bounded(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}… [#TRUNCATION: bounded for display; the run ledger keeps the full text]")
}

pub fn value_preview(v: Option<&Value>, max: usize) -> String {
    match v {
        None => String::new(),
        Some(Value::String(s)) => one_line(s, max),
        Some(Value::Null) => String::new(),
        Some(other) => one_line(&other.to_string(), max),
    }
}

fn value_block(v: Option<&Value>, max: usize) -> String {
    match v {
        None => String::new(),
        Some(Value::String(s)) => bounded(s, max),
        Some(Value::Null) => String::new(),
        Some(other) => bounded(
            &serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
            max,
        ),
    }
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
    /// The FIRST-LEVEL agent run (parent == root) that emits reasoning
    /// cycles: answers come from root or this run — never from deeper
    /// delegate children (their flow ends are intermediate results).
    agent_run_id: String,
    seen_images: HashSet<String>,
    /// The root run: final answers come from here only.
    root_run_id: String,
    /// The run currently emitting reasoning cycles — the steer target.
    steer_run_id: String,
    /// When the newest still-inflight llm_call STARTED (client clock).
    /// Drives the "model call running for Nm — provider may be slow" strip
    /// hint; cleared on completion/terminal.
    pub llm_inflight_since: Option<std::time::Instant>,
    /// True once a final answer or terminal error landed.
    pub finished: bool,
    /// True when the ROOT (or answer-source run) recorded a failure.
    pub failed: bool,
    truncated_notice_added: bool,
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
        self.followed.insert(root_run_id.to_string());
        self.finished = false;
        self.failed = false;
        self.activity.clear();
        self.cycle = 0;
        self.cycles.clear();
        self.stats = Stats::default();
        self.session.runs += 1;
        self.pending_wait = None;
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

    /// Where steering should go: the run currently cycling, else the root.
    pub fn steer_target(&self) -> String {
        if self.steer_run_id.is_empty() {
            self.root_run_id.clone()
        } else {
            self.steer_run_id.clone()
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
                        text: "[#TRUNCATION] older transcript items dropped from view; the gateway ledger keeps everything".into(),
                    },
                );
            }
            // Drop overflow from just after the standing notice at index 0.
            let overflow = self.items.len() - MAX_ITEMS;
            self.items.drain(1..1 + overflow);
        }
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

    /// A resume FAILED after optimistic clearing: the run is still waiting
    /// server-side, so restore the prompt and let the user retry. Guarded:
    /// a stale restore from a PREVIOUS run must never clobber the current
    /// run's pending wait (the runner also guards by is_following).
    pub fn reopen_wait(&mut self, wait: PendingWait) {
        if !self.is_following(&wait.run_id) {
            return;
        }
        self.answered_waits
            .remove(&(wait.wait_key.clone(), wait.step_id.clone()));
        if !self.finished && self.pending_wait.is_none() {
            self.pending_wait = Some(wait);
        }
    }

    /// Mark the tool cards belonging to a wait as approved/denied.
    pub fn mark_wait_tools(&mut self, wait_key: &str, approved: bool) {
        let _ = wait_key;
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
        // the strip hint; completion (any llm_call) clears it.
        if etype == "llm_call" {
            match status.as_str() {
                "started" => self.llm_inflight_since = Some(std::time::Instant::now()),
                "completed" | "failed" => self.llm_inflight_since = None,
                _ => {}
            }
        }

        // --- reasoning cycles -------------------------------------------------
        if etype == "llm_call" && node_id == "reason" && status == "started" {
            let n = self.cycles.entry(rec_run.clone()).or_insert(0);
            *n += 1;
            self.cycle = self.cycle.max(*n);
            self.activity = format!("thinking (cycle {})", self.cycle);
            self.steer_run_id = rec_run.clone();
            // The ANSWER-source agent run: the first-level cycling run
            // (parent == root). Delegate children cycle too, but their flow
            // ends are intermediate results for their parent, never the
            // turn's answer. Unknown parent (partial replays without the
            // discovery record) is treated as first-level: production always
            // learns parents through the subworkflow waits that discover
            // followed runs, so unknown-parent = root-attached in practice.
            if self.agent_run_id.is_empty()
                && self
                    .parents
                    .get(&rec_run)
                    .map(|p| *p == self.root_run_id)
                    .unwrap_or(true)
            {
                self.agent_run_id = rec_run.clone();
            }
        }
        if let Some(cycle) = protocol::cycle_result_from_record(rec) {
            if node_id == "reason" {
                let n = *self.cycles.get(&rec_run).unwrap_or(&1);
                self.push_item(Item::Thinking {
                    iteration: n.max(1),
                    content: bounded(&cycle.content, TEXT_BLOCK_MAX),
                    reasoning: bounded(&cycle.reasoning, TEXT_BLOCK_MAX),
                });
            }
        }
        // Cumulative token totals fold from EVERY followed run (the tree's
        // real spend), but the "latest" fields — served model + live context
        // size — are honest only from the ANSWER-SOURCE lane: a delegate
        // child's tiny call must not relabel the header or the ctx chip
        // (adversary finding: delegate pollution).
        let answer_lane = rec_run == self.root_run_id || rec_run == self.agent_run_id;
        if let Some(usage) = protocol::usage_from_record(rec) {
            self.fold_usage(usage, answer_lane);
        }
        if answer_lane {
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
                    if text.is_empty() || lower == "ready" || lower == "completed" {
                        // Reference semantics: an empty/ready status CLEARS
                        // the activity line instead of leaving stale text.
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
                            "error" => self.push_item(Item::Error {
                                text: bounded(&text, TEXT_BLOCK_MAX),
                            }),
                            "warning" | "warn" => self.push_item(Item::Info {
                                text: bounded(&format!("warning: {text}"), TEXT_BLOCK_MAX),
                            }),
                            _ => self.push_item(Item::Assistant {
                                text: bounded(&text, TEXT_BLOCK_MAX),
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
                    "error" => self.push_item(Item::Error {
                        text: bounded(&msg, TEXT_BLOCK_MAX),
                    }),
                    "warning" | "warn" => self.push_item(Item::Info {
                        text: bounded(&format!("warning: {msg}"), TEXT_BLOCK_MAX),
                    }),
                    _ => self.push_item(Item::Assistant {
                        text: bounded(&msg, TEXT_BLOCK_MAX),
                        final_answer: false,
                    }),
                }
            }
        }

        // --- waits ------------------------------------------------------------
        if status == "waiting" {
            if let Some(wait) = protocol::extract_wait(rec) {
                if let Some(sub) = protocol::subworkflow_run_id(wait) {
                    if self.followed.insert(sub.clone()) {
                        self.parents.insert(sub.clone(), rec_run.clone());
                        effects.push(FoldEffect::FollowRun(sub));
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
        // agent subrun's flow end (the cycling run whose parent is the
        // root). Wrapper bundles keep helper subflows (status watchers)
        // running after the agent answered, so waiting for root completion
        // alone can block the turn forever (live-verified on
        // basic-agent@0.0.2). Deeper cycling runs (delegate_agent children)
        // produce INTERMEDIATE results for their parent — never the answer.
        let answer_source = rec_run == self.root_run_id
            || (!self.agent_run_id.is_empty() && rec_run == self.agent_run_id);
        if answer_source && !self.finished {
            if let Some(out) = protocol::extract_flow_output(rec) {
                // A final can be text, artifacts, or both (reference parity:
                // the assistant finishes on meta-only outputs too).
                let has_media_meta = out
                    .meta
                    .as_ref()
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
                if status == "completed" && (!out.response.is_empty() || has_media_meta) {
                    // Only trust flow-end shaped nodes: the reference clients
                    // accept any record carrying result.output; agent flows
                    // only produce it at the end node of the run.
                    let text = if out.response.is_empty() {
                        "(the run produced media output)".to_string()
                    } else {
                        bounded(&out.response, TEXT_BLOCK_MAX * 4)
                    };
                    self.push_item(Item::Assistant {
                        text,
                        final_answer: true,
                    });
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
                    self.finished = true;
                    self.activity.clear();
                    self.pending_wait = None;
                }
            }
        }

        if status == "failed" {
            let err = protocol::error_from_record(rec);
            self.push_item(Item::Error {
                text: bounded(&err, TEXT_BLOCK_MAX),
            });
            if rec_run == self.root_run_id {
                self.finished = true;
                self.activity.clear();
                self.pending_wait = None;
            }
        }

        effects
    }

    /// The runner observed the root run reach a terminal status.
    pub fn run_terminal(&mut self, status: &str) {
        self.activity.clear();
        self.pending_wait = None;
        self.llm_inflight_since = None;
        if !self.finished {
            match status {
                "completed" => {}
                "cancelled" => self.push_item(Item::Info {
                    text: "run cancelled".into(),
                }),
                other => self.push_item(Item::Error {
                    text: format!("run ended: {other}"),
                }),
            }
            self.finished = true;
        }
    }

    fn fold_usage(&mut self, usage: UsageDelta, answer_lane: bool) {
        self.stats.llm_calls += 1;
        self.stats.input_tokens += usage.input_tokens;
        self.stats.output_tokens += usage.output_tokens;
        self.stats.cached_tokens += usage.cached_tokens;
        if answer_lane && usage.input_tokens > 0 {
            // The live "context used" number: the agent lane's newest call.
            self.stats.last_input_tokens = usage.input_tokens;
        }
        self.session.input_tokens += usage.input_tokens;
        self.session.output_tokens += usage.output_tokens;
        self.stats.output_series.push(usage.output_tokens as f32);
        if self.stats.output_series.len() > 64 {
            let n = self.stats.output_series.len() - 64;
            self.stats.output_series.drain(0..n);
        }
    }

    fn tool_key(run_id: &str, node_id: &str, index: usize, call_id: &str) -> String {
        if !call_id.is_empty() {
            format!("call:{call_id}")
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
        if !call_id.is_empty() && !self.seen_call_ids.insert(call_id.clone()) {
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
                    ..
                } = item
                {
                    if n == name && *status == ToolStatus::AwaitingApproval {
                        *status = ToolStatus::Running;
                        *ap = value_preview(args, ARGS_PREVIEW_MAX);
                        return;
                    }
                }
            }
        }
        self.push_item(Item::Tool {
            key,
            name: name.to_string(),
            args_preview: value_preview(args, ARGS_PREVIEW_MAX),
            status: ToolStatus::Running,
            result_preview: String::new(),
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
        let result_preview = value_block(view.output.as_ref(), RESULT_PREVIEW_MAX);
        let status = if !view.error.is_empty() || view.success == Some(false) {
            ToolStatus::Failed
        } else {
            ToolStatus::Ok
        };
        // Newest matching card by exact key first…
        for item in self.items.iter_mut().rev() {
            if let Item::Tool {
                key: k,
                status: st,
                result_preview: rp,
                error,
                ..
            } = item
            {
                if *k == key {
                    *st = status;
                    *rp = result_preview;
                    *error = one_line(&view.error, ARGS_PREVIEW_MAX);
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
                    result_preview: rp,
                    error,
                    ..
                } = item
                {
                    if *name == view.name
                        && matches!(*st, ToolStatus::Running | ToolStatus::AwaitingApproval)
                    {
                        *st = status;
                        *rp = result_preview;
                        *error = one_line(&view.error, ARGS_PREVIEW_MAX);
                        return;
                    }
                }
            }
        }
        // No started card (replay from mid-stream): append a finished one.
        if !view.call_id.is_empty() {
            self.seen_call_ids.insert(view.call_id.clone());
        }
        let _ = run_id;
        let _ = node_id;
        let _ = index;
        self.push_item(Item::Tool {
            key,
            name: view.name,
            args_preview: value_preview(view.arguments.as_ref(), ARGS_PREVIEW_MAX),
            status,
            result_preview,
            error: one_line(&view.error, ARGS_PREVIEW_MAX),
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
                    let args_preview = value_preview(view.arguments.as_ref(), ARGS_PREVIEW_MAX);
                    if !view.call_id.is_empty() && !self.seen_call_ids.insert(view.call_id.clone())
                    {
                        let call_key = format!("call:{}", view.call_id);
                        let mut flipped = false;
                        for item in self.items.iter_mut().rev() {
                            if let Item::Tool {
                                key: k,
                                status,
                                args_preview: ap,
                                ..
                            } = item
                            {
                                if *k == call_key {
                                    if *status == ToolStatus::Running {
                                        *status = ToolStatus::AwaitingApproval;
                                    }
                                    // The wait carries the FINAL (rewritten)
                                    // arguments — the truth of what will run.
                                    *ap = args_preview.clone();
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
                        status: ToolStatus::AwaitingApproval,
                        result_preview: String::new(),
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
                kind: WaitKind::Ask { prompt },
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
            Item::Tool {
                status,
                result_preview,
                ..
            } => {
                assert_eq!(*status, ToolStatus::Ok);
                assert!(result_preview.contains("data"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(fold.stats.tool_calls, 1);
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
        fold.mark_wait_tools("tool_approval:1", true);
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
        match fold.items.last().unwrap() {
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
        match fold.items.last().unwrap() {
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
                fold.mark_wait_tools(&wk, true);
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
            fold.items.last().unwrap(),
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
        match fold.items.last().unwrap() {
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
            Item::Tool {
                status,
                result_preview,
                ..
            } => {
                assert_eq!(*status, ToolStatus::Ok);
                assert!(result_preview.contains("results"));
            }
            other => panic!("unexpected {other:?}"),
        }
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

    #[test]
    fn chat_messages_carry_completed_turns_under_caps() {
        let mut fold = Fold::new();
        fold.begin_run("r1");
        fold.push_item(Item::User { text: "q1".into() });
        fold.push_item(Item::Thinking {
            iteration: 1,
            content: "…".into(),
            reasoning: String::new(),
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
        assert!(
            matches!(fold.items.last().unwrap(), Item::Error { text } if text.contains("exploded"))
        );
        assert!(fold.finished);
    }
}
