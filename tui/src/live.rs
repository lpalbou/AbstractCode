//! Live (streamed) assistant replies — contract S, client half.
//!
//! The gateway multiplexes two VOLATILE event kinds onto the run's
//! existing ledger SSE stream (`GET /runs/{id}/ledger/stream`), with NO
//! `id:` line — they are not ledger records and never move the resume
//! cursor:
//!
//! ```text
//! event: llm.delta      data: {"run_id","node_id","call_id","seq","text",
//!                              "channel":"content"|"reasoning","snapshot":bool,
//!                              "truncated"?:bool}
//! event: llm.delta_end  data: {"run_id","node_id","call_id","seq",
//!                              "reason":"completed"|"failed"|"cancelled"
//!                                       |"unavailable","detail"?}
//! ```
//!
//! Every frame also carries `root_run_id` (and `parent_run_id`): the
//! gateway's hub is keyed by the ROOT run, and a subscriber to the root
//! receives its child runs' deltas too (CONTRACTS.md S-2 §1) — so this
//! client takes live frames from the ROOT stream only, and captions a
//! child run's bubble "sub-agent · <node>".
//!
//! `call_id` is the ledger `StepRecord.step_id` of the `llm_call`. On
//! (re)subscribe the gateway sends one `snapshot: true` delta per open
//! call (the text so far), then live deltas; the client drops every
//! bubble on each (re)connect before those snapshots (§3). The durable
//! record is written BEFORE `delta_end` (§2): a call whose record the
//! transcript holds never gets a bubble again. Retries get a fresh
//! `call_id`; a call that ran without streaming still ends with a
//! `delta_end` — an end for a call never seen is normal, not an error,
//! except `reason: "unavailable"`, which is shown as a one-line note
//! (§7). Unknown fields (`kind`, …) are ignored; `<think>` is split
//! server-side and never re-parsed here (§10).
//!
//! The live text is a PREVIEW: the durable `llm_call` completed record
//! (and the final answer) replace it. [`LiveReplies`] is the pure state
//! the TUI renders; [`ExecPrinter`] is the headless `exec` twin.

use std::collections::HashSet;
use std::io::Write;

use serde_json::Value;

pub const DELTA_EVENT: &str = "llm.delta";

/// Suffix of the call id a REINVOKED model call streams under.
pub const REINVOKE_SUFFIX: &str = ":reinvoke";

/// The ledger step id a live call id belongs to: the id itself, or the
/// original step for a reinvoked re-run (`<step_id>:reinvoke`) — whose
/// durable record carries the step's own id.
pub fn step_id_of(call_id: &str) -> &str {
    call_id.strip_suffix(REINVOKE_SUFFIX).unwrap_or(call_id)
}
pub const DELTA_END_EVENT: &str = "llm.delta_end";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Content,
    Reasoning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDelta {
    pub run_id: String,
    /// The tree's root run (empty when an older hub omits it).
    pub root_run_id: String,
    pub node_id: String,
    pub call_id: String,
    pub seq: u64,
    pub text: String,
    pub channel: Channel,
    pub snapshot: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndReason {
    Completed,
    Failed,
    Cancelled,
    /// `cancelled` with `detail: "reinvoked"`: a stray kill forced the
    /// model call to run again; the re-run streams under
    /// `<step_id>:reinvoke` (see [`step_id_of`]). Shown as "reply
    /// restarted", never as a cancellation.
    Restarted,
    /// The run asked for streaming but this call could not stream; the
    /// detail names why (`structured_output`, `remote_core`,
    /// `provider_cannot_stream`, `sink_error`, `usage_unavailable`).
    Unavailable(String),
    /// A reason word outside the contract — carried verbatim and treated
    /// as "did not complete" (the bubble goes, the word is shown).
    Other(String),
}

impl EndReason {
    pub fn word(&self) -> &str {
        match self {
            EndReason::Completed => "completed",
            EndReason::Failed => "failed",
            EndReason::Cancelled => "cancelled",
            EndReason::Restarted => "restarted",
            EndReason::Unavailable(_) => "unavailable",
            EndReason::Other(w) => w,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveEnd {
    pub run_id: String,
    pub root_run_id: String,
    pub node_id: String,
    pub call_id: String,
    pub seq: u64,
    pub reason: EndReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveEvent {
    Delta(LiveDelta),
    End(LiveEnd),
}

impl LiveEvent {
    pub fn run_id(&self) -> &str {
        match self {
            LiveEvent::Delta(d) => &d.run_id,
            LiveEvent::End(e) => &e.run_id,
        }
    }
    pub fn root_run_id(&self) -> &str {
        match self {
            LiveEvent::Delta(d) => &d.root_run_id,
            LiveEvent::End(e) => &e.root_run_id,
        }
    }
    pub fn call_id(&self) -> &str {
        match self {
            LiveEvent::Delta(d) => &d.call_id,
            LiveEvent::End(e) => &e.call_id,
        }
    }
}

/// Parse one SSE event. `Ok(None)` = not a live event (the caller's
/// other arms handle it); `Err` = a live event whose data breaks the
/// contract (surfaced by the caller, never silently dropped).
pub fn parse_event(event: &str, data: &str) -> Result<Option<LiveEvent>, String> {
    if event != DELTA_EVENT && event != DELTA_END_EVENT {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(data).map_err(|e| format!("{event}: not JSON ({e})"))?;
    let text_field = |key: &str| -> String {
        v.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let call_id = text_field("call_id");
    if call_id.is_empty() {
        return Err(format!("{event}: missing call_id"));
    }
    let seq = v
        .get("seq")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{event}: missing seq"))?;
    let run_id = text_field("run_id");
    let root_run_id = text_field("root_run_id");
    let node_id = text_field("node_id");
    if event == DELTA_END_EVENT {
        let reason = match text_field("reason").as_str() {
            "completed" => EndReason::Completed,
            "failed" => EndReason::Failed,
            "cancelled" if text_field("detail") == "reinvoked" => EndReason::Restarted,
            "cancelled" => EndReason::Cancelled,
            "unavailable" => EndReason::Unavailable({
                let detail = text_field("detail");
                if detail.is_empty() {
                    "no detail given".to_string()
                } else {
                    detail
                }
            }),
            "" => return Err(format!("{event}: missing reason")),
            other => EndReason::Other(other.to_string()),
        };
        return Ok(Some(LiveEvent::End(LiveEnd {
            run_id,
            root_run_id,
            node_id,
            call_id,
            seq,
            reason,
        })));
    }
    // The text is NOT trimmed: whitespace deltas are content.
    let text = v
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{event}: missing text"))?
        .to_string();
    let channel = match v.get("channel").and_then(Value::as_str) {
        Some("content") => Channel::Content,
        Some("reasoning") => Channel::Reasoning,
        Some(other) => return Err(format!("{event}: unknown channel {other:?}")),
        None => return Err(format!("{event}: missing channel")),
    };
    Ok(Some(LiveEvent::Delta(LiveDelta {
        run_id,
        root_run_id,
        node_id,
        call_id,
        seq,
        text,
        channel,
        snapshot: v.get("snapshot").and_then(Value::as_bool).unwrap_or(false),
        truncated: v.get("truncated").and_then(Value::as_bool).unwrap_or(false),
    })))
}

/// One in-flight streamed call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveReply {
    pub call_id: String,
    pub run_id: String,
    /// True when `run_id` is a child of the root (captioned "sub-agent").
    pub child: bool,
    pub node_id: String,
    pub content: String,
    pub reasoning: String,
    /// The gateway dropped older text of this call (its per-call buffer
    /// cap): what is shown is the TAIL, and the view says so.
    pub truncated: bool,
    /// `delta_end completed` arrived; the durable record is still due.
    pub finished: bool,
    /// Bumped whenever `content` was REPLACED rather than appended (a
    /// snapshot): the view rebuilds its stream item instead of appending.
    pub content_gen: u64,
    content_seq: Option<u64>,
    reasoning_seq: Option<u64>,
}

/// Every live reply of the current run tree, in first-seen order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveReplies {
    root: String,
    entries: Vec<LiveReply>,
    /// Calls whose bubble was already retired (durable record, failure,
    /// final answer): a late or replayed frame for them is ignored.
    retired: HashSet<String>,
    /// `unavailable` details already noted this turn (one line per cause,
    /// not one per model call).
    noted_unavailable: HashSet<String>,
    gen_counter: u64,
}

impl LiveReplies {
    pub fn entries(&self) -> &[LiveReply] {
        &self.entries
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Bind to a run tree; a different root wipes everything (a new turn
    /// never inherits the previous turn's bubbles).
    pub fn bind_root(&mut self, root: &str) {
        if self.root != root {
            self.root = root.to_string();
            self.entries.clear();
            self.retired.clear();
            self.noted_unavailable.clear();
        }
    }

    /// The stream (re)connected: every bubble goes; the snapshots that
    /// follow rebuild the calls still open (CONTRACTS.md S-2 §3). Retired
    /// calls stay retired. Returns whether anything was on screen.
    pub fn reset_on_connect(&mut self) -> bool {
        let had = !self.entries.is_empty();
        self.entries.clear();
        had
    }

    /// True when `call_id` already has its durable record (or its bubble
    /// was taken down) — no frame may bring it back.
    pub fn is_retired(&self, call_id: &str) -> bool {
        self.retired.contains(call_id)
    }

    /// Drop every bubble (the final answer landed, the run ended).
    /// Returns whether anything was on screen.
    pub fn clear(&mut self) -> bool {
        let had = !self.entries.is_empty();
        for e in self.entries.drain(..) {
            self.retired.insert(e.call_id);
        }
        had
    }

    /// Apply one live event. Returns a line to show in the transcript when
    /// a bubble was taken down for a reason other than completion.
    pub fn apply(&mut self, ev: LiveEvent) -> Option<String> {
        if let LiveEvent::End(LiveEnd {
            reason: EndReason::Unavailable(detail),
            node_id,
            ..
        }) = &ev
        {
            // Said once per cause per turn, bubble or not: the operator
            // asked for streaming and this call did not stream.
            let first = self.noted_unavailable.insert(detail.clone());
            let gone = self
                .entries
                .iter()
                .position(|e| e.call_id == ev.call_id())
                .map(|ix| self.entries.remove(ix));
            self.retired.insert(ev.call_id().to_string());
            if !first && gone.is_none() {
                return None;
            }
            let at = if node_id.is_empty() {
                String::new()
            } else {
                format!(" ({node_id})")
            };
            return Some(format!(
                "live reply unavailable{at}: {detail} — the answer appears when the call completes"
            ));
        }
        if self.retired.contains(ev.call_id()) {
            return None;
        }
        match ev {
            LiveEvent::Delta(d) => {
                self.apply_delta(d);
                None
            }
            LiveEvent::End(end) => {
                let ix = self.entries.iter().position(|e| e.call_id == end.call_id)?;
                if end.reason == EndReason::Completed {
                    self.entries[ix].finished = true;
                    return None;
                }
                let gone = self.entries.remove(ix);
                self.retired.insert(gone.call_id.clone());
                let chars = gone.content.chars().count();
                if end.reason == EndReason::Restarted {
                    return Some(format!(
                        "reply restarted — the model call runs again; its partial text ({chars} chars) was discarded"
                    ));
                }
                Some(format!(
                    "live reply {} — the partial text ({chars} chars) was discarded; the run's record shows what happened",
                    end.reason.word()
                ))
            }
        }
    }

    fn apply_delta(&mut self, d: LiveDelta) {
        let ix = match self.entries.iter().position(|e| e.call_id == d.call_id) {
            Some(ix) => ix,
            None => {
                // A new call on the same run+node supersedes a FINISHED
                // bubble there whose durable record has not folded yet
                // (the ledger tail can lag the live lane). Only finished
                // ones: an open call is never guessed away.
                let retired = &mut self.retired;
                self.entries.retain(|e| {
                    let stale = e.finished && e.run_id == d.run_id && e.node_id == d.node_id;
                    if stale {
                        retired.insert(e.call_id.clone());
                    }
                    !stale
                });
                let child =
                    !d.root_run_id.is_empty() && !d.run_id.is_empty() && d.run_id != d.root_run_id;
                self.entries.push(LiveReply {
                    call_id: d.call_id.clone(),
                    run_id: d.run_id.clone(),
                    child,
                    node_id: d.node_id.clone(),
                    content: String::new(),
                    reasoning: String::new(),
                    truncated: false,
                    finished: false,
                    content_gen: 0,
                    content_seq: None,
                    reasoning_seq: None,
                });
                self.entries.len() - 1
            }
        };
        let e = &mut self.entries[ix];
        if d.truncated {
            e.truncated = true;
        }
        let (text, last_seq) = match d.channel {
            Channel::Content => (&mut e.content, &mut e.content_seq),
            Channel::Reasoning => (&mut e.reasoning, &mut e.reasoning_seq),
        };
        if d.snapshot {
            // The text SO FAR replaces whatever this client held (a
            // reconnect may have missed deltas, or replay them).
            *text = d.text;
            *last_seq = Some(d.seq);
            if d.channel == Channel::Content {
                self.gen_counter += 1;
                e.content_gen = self.gen_counter;
            }
            return;
        }
        if last_seq.is_some_and(|s| d.seq <= s) {
            return; // already folded (a replayed frame)
        }
        *last_seq = Some(d.seq);
        text.push_str(&d.text);
    }

    /// The durable ledger caught up: an `llm_call` record with status
    /// completed/failed retires its call (`call_id == step_id`) — the
    /// bubble goes, and no later frame for it may bring it back
    /// (CONTRACTS.md S-2 §2). Matched by id only: the ledger tail can lag
    /// the live lane, so a (run, node) match could take down the NEXT
    /// call's bubble. Returns whether a bubble was on screen.
    pub fn retire_durable(&mut self, closed: &[DurableClose]) -> bool {
        let mut changed = false;
        for c in closed {
            self.retired.insert(c.step_id.clone());
            let before = self.entries.len();
            // A reinvoked re-run's bubble belongs to the same step.
            self.retired
                .insert(format!("{}{REINVOKE_SUFFIX}", c.step_id));
            self.entries.retain(|e| step_id_of(&e.call_id) != c.step_id);
            changed |= self.entries.len() != before;
        }
        changed
    }
}

/// An `llm_call` that the durable ledger reports closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableClose {
    pub run_id: String,
    pub step_id: String,
    pub node_id: String,
}

/// The `llm_call` completed/failed records in one batch.
pub fn durable_closes(source_run_id: &str, records: &[Value]) -> Vec<DurableClose> {
    records
        .iter()
        .filter(|rec| crate::protocol::effect_type(rec) == "llm_call")
        .filter(|rec| {
            matches!(
                crate::protocol::record_status(rec).as_str(),
                "completed" | "failed"
            )
        })
        .filter_map(|rec| {
            let step_id = rec.get("step_id").and_then(Value::as_str)?.trim();
            if step_id.is_empty() {
                return None;
            }
            let run = crate::protocol::record_run_id(rec);
            Some(DurableClose {
                run_id: if run.is_empty() {
                    source_run_id.to_string()
                } else {
                    run
                },
                step_id: step_id.to_string(),
                node_id: crate::protocol::record_node_id(rec),
            })
        })
        .collect()
}

/// Headless `exec --stream on`: prints content deltas to stdout as they
/// arrive, one `✎` line-run per call, and remembers what it printed so
/// the final answer is not printed a second time.
///
/// Reasoning deltas are NOT printed (the reply stream must never mix the
/// model's reasoning into its answer); the cycle lines still summarize it.
#[derive(Debug, Default)]
pub struct ExecPrinter {
    /// (call_id, text printed for it) in first-seen order.
    printed: Vec<(String, String)>,
    /// The call whose text is mid-line on stdout.
    open: Option<String>,
    truncated_noted: HashSet<String>,
    unavailable_noted: HashSet<String>,
}

impl ExecPrinter {
    pub fn on_event(&mut self, ev: &LiveEvent, out: &mut impl Write) {
        match ev {
            LiveEvent::Delta(d) if d.channel == Channel::Content => {
                let ix = match self.printed.iter().position(|(c, _)| *c == d.call_id) {
                    Some(ix) => ix,
                    None => {
                        self.printed.push((d.call_id.clone(), String::new()));
                        self.printed.len() - 1
                    }
                };
                if self.open.as_deref() != Some(d.call_id.as_str()) {
                    self.close_line(out);
                    let _ = write!(out, "✎ ");
                    self.open = Some(d.call_id.clone());
                }
                let so_far = &mut self.printed[ix].1;
                if d.snapshot {
                    // Printed text cannot be taken back: print only what
                    // extends it, or say the live text restarted.
                    if let Some(rest) = d.text.strip_prefix(so_far.as_str()) {
                        let _ = write!(out, "{rest}");
                    } else {
                        let _ =
                            write!(out, "\n[live text restarted after a reconnect]\n{}", d.text);
                    }
                    *so_far = d.text.clone();
                } else {
                    let _ = write!(out, "{}", d.text);
                    so_far.push_str(&d.text);
                }
                if d.truncated && self.truncated_noted.insert(d.call_id.clone()) {
                    let _ = write!(
                        out,
                        " [#TRUNCATION: the gateway dropped older live text of this call; the final answer prints in full]"
                    );
                }
                let _ = out.flush();
            }
            LiveEvent::Delta(_) => {}
            LiveEvent::End(LiveEnd {
                reason: EndReason::Unavailable(detail),
                call_id,
                ..
            }) => {
                if self.open.as_deref() == Some(call_id.as_str()) {
                    self.printed.retain(|(c, _)| c != call_id);
                    self.close_line(out);
                }
                if self.unavailable_noted.insert(detail.clone()) {
                    let _ = writeln!(
                        out,
                        "· live reply unavailable: {detail} — the answer prints when the call completes"
                    );
                    let _ = out.flush();
                }
            }
            LiveEvent::End(end) => {
                if self.open.as_deref() == Some(end.call_id.as_str()) {
                    if end.reason == EndReason::Restarted {
                        let _ = write!(out, " [reply restarted]");
                        self.printed.retain(|(c, _)| *c != end.call_id);
                    } else if end.reason != EndReason::Completed
                        && self.printed.iter().any(|(c, _)| *c == end.call_id)
                    {
                        let _ = write!(out, " [live reply {}]", end.reason.word());
                        self.printed.retain(|(c, _)| *c != end.call_id);
                    }
                    self.close_line(out);
                }
            }
        }
    }

    /// End an open live line before anything else prints.
    pub fn close_line(&mut self, out: &mut impl Write) {
        if self.open.take().is_some() {
            let _ = writeln!(out);
            let _ = out.flush();
        }
    }

    /// True when `answer` was already printed live, whole (so the final
    /// answer block must not repeat it).
    pub fn streamed_whole(&self, answer: &str) -> bool {
        let want = answer.trim();
        !want.is_empty() && self.printed.iter().any(|(_, t)| t.trim() == want)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn delta(call: &str, seq: u64, text: &str) -> LiveEvent {
        LiveEvent::Delta(LiveDelta {
            run_id: "r".into(),
            root_run_id: "root".into(),
            node_id: "reason".into(),
            call_id: call.into(),
            seq,
            text: text.into(),
            channel: Channel::Content,
            snapshot: false,
            truncated: false,
        })
    }

    fn end(call: &str, reason: EndReason) -> LiveEvent {
        LiveEvent::End(LiveEnd {
            run_id: "r".into(),
            root_run_id: "root".into(),
            node_id: "reason".into(),
            call_id: call.into(),
            seq: 99,
            reason,
        })
    }

    #[test]
    fn parses_both_events_and_ignores_unknown_fields() {
        let d = parse_event(
            "llm.delta",
            r#"{"kind":"llm.delta","run_id":"r","root_run_id":"root","parent_run_id":"root","node_id":"reason","call_id":"s1","seq":3,"text":" hi","channel":"content","snapshot":false,"extra":1}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(d, delta("s1", 3, " hi"));
        let e = parse_event(
            "llm.delta_end",
            r#"{"kind":"llm.delta_end","run_id":"r","node_id":"reason","call_id":"s1","seq":4,"reason":"failed"}"#,
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            e,
            LiveEvent::End(LiveEnd {
                reason: EndReason::Failed,
                seq: 4,
                ..
            })
        ));
        let t = parse_event(
            "llm.delta",
            r#"{"run_id":"r","call_id":"s1","seq":5,"text":"x","channel":"reasoning","snapshot":true,"truncated":true}"#,
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            t,
            LiveEvent::Delta(LiveDelta {
                channel: Channel::Reasoning,
                snapshot: true,
                truncated: true,
                ..
            })
        ));
        // Not a live event: the caller's other arms own it.
        assert_eq!(parse_event("step", "{}"), Ok(None));
    }

    #[test]
    fn contract_breaks_are_errors_never_silent() {
        for (ev, data) in [
            ("llm.delta", "not json"),
            ("llm.delta", r#"{"seq":1,"text":"x","channel":"content"}"#),
            (
                "llm.delta",
                r#"{"call_id":"c","text":"x","channel":"content"}"#,
            ),
            (
                "llm.delta",
                r#"{"call_id":"c","seq":1,"channel":"content"}"#,
            ),
            (
                "llm.delta",
                r#"{"call_id":"c","seq":1,"text":"x","channel":"tool"}"#,
            ),
            ("llm.delta_end", r#"{"call_id":"c","seq":1}"#),
        ] {
            assert!(parse_event(ev, data).is_err(), "{ev} {data}");
        }
    }

    #[test]
    fn deltas_append_in_seq_order_and_replays_are_ignored() {
        let mut l = LiveReplies::default();
        l.bind_root("root");
        l.apply(delta("c1", 1, "Hel"));
        l.apply(delta("c1", 2, "lo"));
        l.apply(delta("c1", 2, "lo")); // replayed frame
        assert_eq!(l.entries()[0].content, "Hello");
    }

    #[test]
    fn a_snapshot_replaces_the_text_and_bumps_the_generation() {
        let mut l = LiveReplies::default();
        l.apply(delta("c1", 1, "Hel"));
        let gen0 = l.entries()[0].content_gen;
        let LiveEvent::Delta(mut snap) = delta("c1", 7, "Hello wor") else {
            unreachable!()
        };
        snap.snapshot = true;
        l.apply(LiveEvent::Delta(snap));
        assert_eq!(l.entries()[0].content, "Hello wor");
        assert_ne!(l.entries()[0].content_gen, gen0);
        l.apply(delta("c1", 7, "IGNORED"));
        l.apply(delta("c1", 8, "ld"));
        assert_eq!(l.entries()[0].content, "Hello world");
    }

    #[test]
    fn reasoning_never_mixes_into_the_reply_text() {
        let mut l = LiveReplies::default();
        let LiveEvent::Delta(mut r) = delta("c1", 1, "let me think") else {
            unreachable!()
        };
        r.channel = Channel::Reasoning;
        l.apply(LiveEvent::Delta(r));
        l.apply(delta("c1", 2, "Answer"));
        assert_eq!(l.entries()[0].content, "Answer");
        assert_eq!(l.entries()[0].reasoning, "let me think");
    }

    #[test]
    fn end_reasons_finish_or_retire_with_a_visible_note() {
        let mut l = LiveReplies::default();
        l.apply(delta("c1", 1, "partial"));
        assert_eq!(l.apply(end("c1", EndReason::Completed)), None);
        assert!(
            l.entries()[0].finished,
            "completed keeps the text until the record lands"
        );

        l.apply(delta("c2", 1, "doomed"));
        let note = l
            .apply(end("c2", EndReason::Failed))
            .expect("a failed stream says so");
        assert!(
            note.contains("failed") && note.contains("6 chars"),
            "{note}"
        );
        assert!(l.entries().iter().all(|e| e.call_id != "c2"));
        // A late frame for a retired call never resurrects it.
        l.apply(delta("c2", 2, "zombie"));
        assert!(l.entries().iter().all(|e| e.call_id != "c2"));

        l.apply(delta("c3", 1, "x"));
        let note = l.apply(end("c3", EndReason::Cancelled)).unwrap();
        assert!(note.contains("cancelled"), "{note}");
    }

    #[test]
    fn a_reinvoked_call_restarts_the_reply_under_its_new_id() {
        let mut l = LiveReplies::default();
        l.apply(delta("s1", 1, "first try"));
        let ev = parse_event(
            "llm.delta_end",
            r#"{"call_id":"s1","seq":2,"reason":"cancelled","detail":"reinvoked"}"#,
        )
        .unwrap()
        .unwrap();
        let note = l.apply(ev).expect("said");
        assert!(note.starts_with("reply restarted"), "{note}");
        assert!(!note.contains("cancelled"), "{note}");
        assert!(l.is_empty(), "the first item is dropped");
        l.apply(delta("s1:reinvoke", 1, "second try"));
        assert_eq!(l.entries().len(), 1, "the re-run's item appears");
        assert_eq!(l.entries()[0].content, "second try");
        // The step's durable record retires the re-run's bubble too.
        assert!(l.retire_durable(&[DurableClose {
            run_id: "r".into(),
            step_id: "s1".into(),
            node_id: "reason".into(),
        }]));
        assert!(l.is_empty());
        l.apply(delta("s1:reinvoke", 2, "late"));
        assert!(l.is_empty());
        // Other cancelled ends are unchanged.
        l.apply(delta("s2", 1, "x"));
        let other = parse_event(
            "llm.delta_end",
            r#"{"call_id":"s2","seq":2,"reason":"cancelled","detail":"user"}"#,
        )
        .unwrap()
        .unwrap();
        assert!(l.apply(other).unwrap().contains("live reply cancelled"));
        assert_eq!(step_id_of("s1:reinvoke"), "s1");
        assert_eq!(step_id_of("s1"), "s1");
    }

    #[test]
    fn exec_prints_reply_restarted_then_the_rerun() {
        let mut p = ExecPrinter::default();
        let mut out: Vec<u8> = Vec::new();
        p.on_event(&delta("s1", 1, "first"), &mut out);
        p.on_event(&end("s1", EndReason::Restarted), &mut out);
        p.on_event(&delta("s1:reinvoke", 1, "The answer"), &mut out);
        p.on_event(&end("s1:reinvoke", EndReason::Completed), &mut out);
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, "✎ first [reply restarted]\n✎ The answer\n");
        assert!(p.streamed_whole("The answer"));
        assert!(!p.streamed_whole("first"));
    }

    #[test]
    fn an_end_for_a_call_never_seen_is_normal() {
        let mut l = LiveReplies::default();
        assert_eq!(l.apply(end("never", EndReason::Failed)), None);
        assert_eq!(l.apply(end("never2", EndReason::Completed)), None);
        assert!(l.is_empty());
    }

    #[test]
    fn the_durable_record_retires_its_call_and_no_frame_brings_it_back() {
        let mut l = LiveReplies::default();
        l.apply(delta("step-1", 1, "a"));
        l.apply(delta("step-2", 1, "b")); // the NEXT call, same node, record lagging
        let recs = vec![json!({
            "run_id": "r", "step_id": "step-1", "node_id": "reason",
            "status": "completed", "effect": {"type": "llm_call"}
        })];
        let closes = durable_closes("r", &recs);
        assert_eq!(closes.len(), 1);
        assert!(l.retire_durable(&closes));
        assert_eq!(l.entries().len(), 1, "only the recorded call goes");
        assert_eq!(l.entries()[0].call_id, "step-2");
        // S-2 §2: late deltas AND a reconnect snapshot for a call whose
        // record the transcript holds are ignored.
        l.apply(delta("step-1", 5, "late"));
        let LiveEvent::Delta(mut snap) = delta("step-1", 6, "a snapshot") else {
            unreachable!()
        };
        snap.snapshot = true;
        l.apply(LiveEvent::Delta(snap));
        assert!(l.entries().iter().all(|e| e.call_id != "step-1"));
        assert!(l.is_retired("step-1"));
        // A record folded BEFORE any frame (record-first ordering) also
        // keeps the call from ever getting a bubble.
        l.retire_durable(&[DurableClose {
            run_id: "r".into(),
            step_id: "step-3".into(),
            node_id: "reason".into(),
        }]);
        l.apply(delta("step-3", 1, "never shown"));
        assert!(l.entries().iter().all(|e| e.call_id != "step-3"));
        // Non-llm and started records close nothing.
        let noise = vec![
            json!({"step_id": "x", "status": "started", "effect": {"type": "llm_call"}}),
            json!({"step_id": "y", "status": "completed", "effect": {"type": "tool_calls"}}),
        ];
        assert!(durable_closes("r", &noise).is_empty());
    }

    #[test]
    fn a_reconnect_drops_every_bubble_before_the_snapshots() {
        let mut l = LiveReplies::default();
        l.apply(delta("c1", 1, "stale half"));
        l.apply(delta("c2", 1, "gone call"));
        assert!(l.reset_on_connect());
        assert!(l.is_empty(), "S-2 §3: nothing survives a reconnect");
        let LiveEvent::Delta(mut snap) = delta("c1", 9, "stale half, now whole") else {
            unreachable!()
        };
        snap.snapshot = true;
        l.apply(LiveEvent::Delta(snap));
        assert_eq!(l.entries().len(), 1, "only the still-open call returns");
        assert_eq!(l.entries()[0].content, "stale half, now whole");
        assert!(!l.reset_on_connect() || l.is_empty());
    }

    #[test]
    fn unavailable_is_a_one_line_note_per_cause() {
        let unavailable = |call: &str, detail: &str| {
            LiveEvent::End(LiveEnd {
                run_id: "r".into(),
                root_run_id: "root".into(),
                node_id: "reason".into(),
                call_id: call.into(),
                seq: 1,
                reason: EndReason::Unavailable(detail.into()),
            })
        };
        let mut l = LiveReplies::default();
        let note = l
            .apply(unavailable("c1", "structured_output"))
            .expect("an unseen call's unavailable end is still said");
        assert!(
            note.contains("structured_output") && note.contains("reason"),
            "{note}"
        );
        assert_eq!(
            l.apply(unavailable("c2", "structured_output")),
            None,
            "once per cause"
        );
        assert!(l.apply(unavailable("c3", "usage_unavailable")).is_some());
        let parsed = parse_event(
            "llm.delta_end",
            r#"{"call_id":"c","seq":1,"reason":"unavailable","detail":"remote_core"}"#,
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            parsed,
            LiveEvent::End(LiveEnd { reason: EndReason::Unavailable(ref d), .. }) if d == "remote_core"
        ));
    }

    #[test]
    fn a_child_runs_bubble_is_marked_and_a_new_call_supersedes_a_finished_one() {
        let mut l = LiveReplies::default();
        l.apply(delta("c1", 1, "child text"));
        assert!(l.entries()[0].child, "run r under root 'root' is a child");
        l.apply(end("c1", EndReason::Completed));
        l.apply(delta("c2", 1, "next cycle"));
        assert_eq!(l.entries().len(), 1);
        assert_eq!(l.entries()[0].call_id, "c2");
        assert!(l.is_retired("c1"));
    }

    #[test]
    fn a_new_root_wipes_the_previous_turn() {
        let mut l = LiveReplies::default();
        l.bind_root("a");
        l.apply(delta("c1", 1, "x"));
        l.bind_root("a");
        assert_eq!(l.entries().len(), 1);
        l.bind_root("b");
        assert!(l.is_empty());
        l.apply(delta("c1", 2, "y"));
        assert_eq!(l.entries().len(), 1, "retired set is per root");
    }

    #[test]
    fn truncation_is_sticky() {
        let mut l = LiveReplies::default();
        let LiveEvent::Delta(mut d) = delta("c1", 1, "tail") else {
            unreachable!()
        };
        d.truncated = true;
        d.snapshot = true;
        l.apply(LiveEvent::Delta(d));
        l.apply(delta("c1", 2, "more"));
        assert!(l.entries()[0].truncated);
    }

    #[test]
    fn exec_prints_deltas_live_and_knows_the_answer_was_streamed() {
        let mut p = ExecPrinter::default();
        let mut out: Vec<u8> = Vec::new();
        p.on_event(&delta("c1", 1, "The answer"), &mut out);
        p.on_event(&delta("c1", 2, " is 42."), &mut out);
        let LiveEvent::Delta(mut r) = delta("c1", 3, "SECRET REASONING") else {
            unreachable!()
        };
        r.channel = Channel::Reasoning;
        p.on_event(&LiveEvent::Delta(r), &mut out);
        p.on_event(&end("c1", EndReason::Completed), &mut out);
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, "✎ The answer is 42.\n");
        assert!(p.streamed_whole("The answer is 42.\n"));
        assert!(!p.streamed_whole("A different answer"));
    }

    #[test]
    fn exec_snapshot_extends_or_restarts_and_a_failed_stream_is_named() {
        let mut p = ExecPrinter::default();
        let mut out: Vec<u8> = Vec::new();
        p.on_event(&delta("c1", 1, "Hello"), &mut out);
        let LiveEvent::Delta(mut s) = delta("c1", 4, "Hello world") else {
            unreachable!()
        };
        s.snapshot = true;
        p.on_event(&LiveEvent::Delta(s), &mut out);
        let LiveEvent::Delta(mut s2) = delta("c1", 5, "Goodbye") else {
            unreachable!()
        };
        s2.snapshot = true;
        p.on_event(&LiveEvent::Delta(s2), &mut out);
        p.on_event(&end("c1", EndReason::Failed), &mut out);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "✎ Hello world\n[live text restarted after a reconnect]\nGoodbye [live reply failed]\n"
        );
        assert!(
            !p.streamed_whole("Goodbye"),
            "a failed stream never stands in for the answer"
        );
    }
}
