//! Entity conversation state: `Focus`, `EntityConvo`, and the turn/recovery
//! state machine as a PURE fold (offline-testable — every transition is a
//! function over the convo + a parsed response; threads and signals live
//! elsewhere).
//!
//! Stale-result discipline (the `is_following` twin): every closure a turn
//! thread posts re-checks `guard(convos, name, run_id, epoch)` before
//! touching state — a late result from an abandoned/ended conversation
//! applies NOTHING. `turn_epoch` bumps on every send/adopt/close.

use std::time::Instant;

use crate::entities::{
    CloseResponse, ToolDetail, TurnResponse, VisitOpen, VisitStatus, VisitTranscript,
};
// `bounded`/`one_line` are transcript.rs's pub(crate) text bounds — this
// module carried byte-identical private copies until cycle-3 (presence
// review P3-11: consolidation needed the transcript side opened up).
use crate::transcript::{bounded, one_line, Item, ToolStatus};

/// Which conversation the transcript pane mirrors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    Agent,
    /// Entity slug (lowercase).
    Entity(String),
}

/// Which BRAIN answers this conversation (the entity app's selector,
/// ported): `Visit` = the durable visit lane (driver turn loop, parked
/// run between turns); `Flow` = summon-per-prompt of the `entity-chat`
/// VisualFlow (`entity-life` bundle) through the production door — each
/// message is ONE summon; continuity rides the entity's own memory graph
/// under one session id; the VIEW is session-local (nothing to adopt or
/// close server-side). Fixed at OPEN — switching brains mid-conversation
/// was the reference implementation's P0 chimera hazard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Brain {
    #[default]
    Visit,
    Flow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvoStatus {
    /// Open/adopt in flight — no run_id yet (or rehydrating).
    Opening,
    /// Visit open, no turn yet sent from this client.
    Ready,
    /// A turn is executing server-side (non-interruptible).
    TurnRunning,
    /// Between turns: the run parks on the visitor wait.
    Parked,
    /// Terminal: closed (operator/idle/failed) — transcript stays readable;
    /// the next `@name` opens or adopts fresh.
    Closed,
    /// The open itself was refused (paused/opening-grace/hosted-chat/…).
    Refused,
}

/// One entity conversation. `items` reuses the transcript `Item` enum so
/// the same feed/pane renders both conversations.
#[derive(Debug, Clone)]
pub struct EntityConvo {
    /// Entity slug (lowercase) — the conversation identity.
    pub name: String,
    pub run_id: String,
    pub session_id: String,
    pub visit_id: String,
    pub items: Vec<Item>,
    pub status: ConvoStatus,
    /// Draft held while a turn runs — auto-sent when the turn parks (the
    /// ruled v1 between-turns steering).
    pub held_draft: String,
    /// Bumped on every send/adopt/close; posted closures re-check it.
    pub turn_epoch: u64,
    /// Entity state word for the chip (from the roster/cognition poll).
    pub entity_state: String,
    /// Server-side turn counter (from open/adopt/turn responses).
    pub turn_n: u64,
    /// Live-visit token spend from `/cognition` (None until polled —
    /// never a fabricated count).
    pub spend_tokens: Option<u64>,
    /// When the in-flight turn started (client clock, strip/chip elapsed).
    pub turn_started: Option<Instant>,
    /// The cached roster said "asleep" at open time: close restores sleep.
    pub woke_for_visit: bool,
    /// The turn-recovery loop owns this run (post-timeout /visit polling
    /// at 5s): the 7s conversation poller SKIPS it — two threads polling
    /// one run was benign but wasteful (cycle-2 leftover). Set when the
    /// timeout notice folds; cleared on EVERY recovery exit path (parked,
    /// closed, transcript error, thread death). Only meaningful while
    /// `status == TurnRunning` — the poller's skip checks both, so a
    /// stuck latch can never starve idle-close detection on a parked
    /// conversation.
    pub recovery_owned: bool,
    /// Which brain answers (see [`Brain`]). Fixed at open.
    pub brain: Brain,
}

impl EntityConvo {
    /// A fresh conversation entering the Opening state, with the honest
    /// wake note when the cached roster shows the entity asleep.
    pub fn opening(name: &str, cached_state: &str) -> EntityConvo {
        let name = name.to_lowercase();
        let asleep = cached_state == "asleep";
        let mut items = vec![Item::Info {
            text: format!("opening a visit with {name}…"),
        }];
        if asleep {
            items.push(Item::Info {
                text: format!(
                    "{name} was asleep — this visit wakes {name}; close restores the sleep"
                ),
            });
        }
        EntityConvo {
            name,
            run_id: String::new(),
            session_id: String::new(),
            visit_id: String::new(),
            items,
            status: ConvoStatus::Opening,
            held_draft: String::new(),
            turn_epoch: 0,
            entity_state: cached_state.to_string(),
            turn_n: 0,
            spend_tokens: None,
            turn_started: None,
            woke_for_visit: asleep,
            recovery_owned: false,
            brain: Brain::Visit,
        }
    }

    /// A fresh FLOW-BRAIN conversation (`/brain <name>`): no server open —
    /// the first message performs the first summon. Ready immediately;
    /// the session id is client-minted so every summon of this
    /// conversation groups under one id (continuity in the entity's own
    /// graph). The opening lines teach the lane's honest semantics.
    pub fn flow_opening(name: &str, cached_state: &str, session_id: &str) -> EntityConvo {
        let name = name.to_lowercase();
        let items = vec![Item::Info {
            text: format!(
                "flow-brain conversation with {name} — each message is one door summon \
                 (entity-chat flow); memory persists in {name}'s graph; this view is \
                 session-local"
            ),
        }];
        EntityConvo {
            name,
            run_id: String::new(),
            session_id: session_id.to_string(),
            visit_id: String::new(),
            items,
            status: ConvoStatus::Ready,
            held_draft: String::new(),
            turn_epoch: 0,
            entity_state: cached_state.to_string(),
            turn_n: 0,
            spend_tokens: None,
            turn_started: None,
            woke_for_visit: false,
            recovery_owned: false,
            brain: Brain::Flow,
        }
    }

    /// Chip text for the header row ("castor ✎3m" / "castor parked").
    pub fn chip(&self) -> String {
        let state = match self.status {
            ConvoStatus::Opening => "opening…".to_string(),
            ConvoStatus::Ready => "ready".to_string(),
            ConvoStatus::TurnRunning => {
                let secs = self
                    .turn_started
                    .map(|s| s.elapsed().as_secs())
                    .unwrap_or(0);
                format!("✎{}", fmt_elapsed(secs))
            }
            ConvoStatus::Parked => "parked".to_string(),
            ConvoStatus::Closed => "closed".to_string(),
            ConvoStatus::Refused => "refused".to_string(),
        };
        format!("◆{} {state}", self.name)
    }
}

/// Human elapsed: `42s` / `3m05s` / `9h20m` — never a raw `33628s`
/// (POLISH-1). Seconds drop at hour scale (minute precision is what an
/// hours-long run is read at); the shared formatter for chips, the
/// activity strip, and goal status.
pub fn fmt_elapsed(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

/// Header-chip paint plan: which chips render, in which order, and how
/// many collapse into the "+N" tail. `widths[i]` is chip i's display
/// width (WITHOUT the 2-cell separator; this fold adds it, mirroring the
/// header's print). Chips paint whole or not at all.
///
/// Identity order (the convos vec) paints first-come-first-fit — EXCEPT
/// when the FOCUSED chip would land in the hidden tail: then it paints
/// FIRST (the one chip the user is looking at must always be visible).
/// PAINT order is presentation only: Alt+E CYCLE order stays the convos
/// vec order unconditionally (`cycle_focus` never consults this plan), so
/// reordering the pixels never reorders the keyboard.
pub fn chip_paint_plan(
    widths: &[usize],
    focused: Option<usize>,
    avail: usize,
) -> (Vec<usize>, usize) {
    // Greedy whole-chip fit over a candidate order. While chips REMAIN
    // after the one being placed, 4 cells stay reserved for the overflow
    // marker ("  +N") so the tail collapses honestly instead of clipping.
    let fit = |order: &[usize]| -> usize {
        let mut used = 0usize;
        let mut placed = 0usize;
        for (pos, &ix) in order.iter().enumerate() {
            let need = 2 + widths[ix];
            let marker_w = if order.len() - pos > 1 { 4 } else { 0 };
            if avail.saturating_sub(used) < need + marker_w {
                break;
            }
            used += need;
            placed += 1;
        }
        placed
    };
    let identity: Vec<usize> = (0..widths.len()).collect();
    let k = fit(&identity);
    match focused {
        // No focus, or the focused chip already paints: identity order.
        None => (identity[..k].to_vec(), widths.len() - k),
        Some(f) if f < k || f >= widths.len() => (identity[..k].to_vec(), widths.len() - k),
        Some(f) => {
            // Focused-first: the focused chip takes position 0; the rest
            // keep identity order behind it.
            let mut order = vec![f];
            order.extend((0..widths.len()).filter(|&i| i != f));
            let k2 = fit(&order);
            (order[..k2].to_vec(), widths.len() - k2)
        }
    }
}

/// Locate the convo a posted closure may touch: it must still exist, still
/// hold the same run, and still be on the same epoch. `run_id` empty on
/// the CALLER side means "the open outcome" (the convo has no run yet).
pub fn guard(convos: &[EntityConvo], name: &str, run_id: &str, epoch: u64) -> Option<usize> {
    convos
        .iter()
        .position(|c| c.name == name && c.run_id == run_id && c.turn_epoch == epoch)
}

/// Find a conversation by entity name (any state).
pub fn find(convos: &[EntityConvo], name: &str) -> Option<usize> {
    let name = name.to_lowercase();
    convos.iter().position(|c| c.name == name)
}

/// The FLOW-lane staleness guard: name + SESSION + epoch. Flow
/// conversations have no stable run identity (each summon mints a fresh
/// run; the convo's `run_id` holds the LATEST one), so identity is the
/// conversation's session id (client-minted, unique per thread) plus the
/// epoch bumped on every send/close. The session id is load-bearing
/// (adversary P0-1): epoch inheritance on replace already prevents
/// number collisions, and the sid match makes cross-THREAD application
/// structurally impossible even if a future edit drops the inheritance.
pub fn guard_flow(
    convos: &[EntityConvo],
    name: &str,
    session_id: &str,
    epoch: u64,
) -> Option<usize> {
    convos.iter().position(|c| {
        c.name == name
            && c.brain == Brain::Flow
            && c.session_id == session_id
            && c.turn_epoch == epoch
    })
}

// ---------------------------------------------------------------------------
// Transitions (pure folds)
// ---------------------------------------------------------------------------

/// Open succeeded: Opening → Ready.
pub fn fold_open_success(convo: &mut EntityConvo, open: &VisitOpen) {
    convo.run_id = open.run_id.clone();
    convo.session_id = open.session_id.clone();
    convo.visit_id = open.visit_id.clone();
    convo.status = ConvoStatus::Ready;
    convo.turn_epoch += 1;
    if !open.participants.is_empty() {
        convo.items.push(Item::Info {
            text: format!("visit open — present: {}", open.participants.join(", ")),
        });
    }
    for w in &open.prelude_warnings {
        convo.items.push(Item::Info {
            text: format!("warning: {w}"),
        });
    }
}

/// Open refused with a non-adoptable 409 (or any open error): render the
/// gateway's `detail` VERBATIM — never guess which refusal case it was.
/// A draft held during the open surfaces as undelivered (never silently
/// lost with the refusal).
pub fn fold_open_refused(convo: &mut EntityConvo, detail: &str) {
    convo.status = ConvoStatus::Refused;
    convo.turn_epoch += 1;
    convo.items.push(Item::Error {
        text: detail.to_string(),
    });
    surface_dropped_draft(convo, "the open was refused");
}

/// Adopt a LIVE visit discovered through 409 → `GET /visit` →
/// `GET /transcript`: rebuild items from the transcript. Sliding-window
/// honesty: `_visit.history` keeps only the last ~10 turns — when `turn_n`
/// exceeds the rendered turns, say so instead of pretending completeness.
///
/// Prior items stay ABOVE the adopted transcript (they are chronologically
/// older — a reopened conversation keeps its old visit's transcript first;
/// cycle-2 review: the old order rendered the previous visit BELOW the new
/// one). Transient lines are dropped: the "opening…" notice (both the
/// fresh and the reopen spelling) and the wake note — a 409 proves the
/// entity was ALREADY in a live visit, so "this visit wakes" was a stale
/// cached-roster guess, and `woke_for_visit` clears with it (our close of
/// an adopted visit cannot honestly claim "prior sleep restored").
pub fn fold_adopt(convo: &mut EntityConvo, status: &VisitStatus, transcript: &VisitTranscript) {
    convo.run_id = status.run_id.clone();
    convo.session_id = status.session_id.clone();
    convo.visit_id = status.visit_id.clone();
    convo.turn_n = status.turn_n;
    convo.turn_epoch += 1;
    convo.status = ConvoStatus::Parked;
    convo.woke_for_visit = false;
    convo.items.retain(|i| {
        !matches!(i, Item::Info { text }
            if text.starts_with("opening a ") || text.contains("this visit wakes"))
    });
    let mut items = std::mem::take(&mut convo.items);
    items.push(Item::Info {
        text: format!(
            "adopted the live visit with {} (turn {})",
            convo.name, status.turn_n
        ),
    });
    let user_turns = transcript.turns.iter().filter(|t| t.role == "user").count() as u64;
    if status.turn_n > user_turns {
        items.push(Item::Info {
            text: "earlier turns live in the entity's memory, not this window".into(),
        });
    }
    render_transcript_turns(&mut items, &transcript.turns);
    for w in &transcript.warnings {
        items.push(Item::Info {
            text: format!("warning: {w}"),
        });
    }
    convo.items = items;
}

fn render_transcript_turns(items: &mut Vec<Item>, turns: &[crate::entities::TranscriptTurn]) {
    for t in turns {
        match t.role.as_str() {
            "user" => {
                // `_visit.history` stores the RENDERED user message
                // (presence + dated MEMORIES block + raw words — live gate
                // 2026-07-22). Rendering it whole presented ~20 lines of
                // prompt chrome as the visitor's own words; split it so
                // the user card carries only what the visitor typed and
                // the memories ride details-gated (probe parity with the
                // live turn's `memories` field).
                let (memories, raw) = crate::entities::split_rendered_user(&t.content);
                if let Some(block) = memories {
                    let n = block.lines().filter(|l| l.starts_with("- ")).count();
                    items.push(Item::Probe {
                        title: format!("memories in context ({n})"),
                        body: block,
                    });
                }
                items.push(Item::User { text: raw });
            }
            "assistant" => {
                push_tool_cards(items, &t.tool_details);
                if !t.content.trim().is_empty() {
                    items.push(Item::Assistant {
                        text: t.content.clone(),
                        final_answer: true,
                    });
                }
            }
            _ => {}
        }
    }
}

fn push_tool_cards(items: &mut Vec<Item>, details: &[ToolDetail]) {
    for (i, d) in details.iter().enumerate() {
        let failed = d.success == Some(false);
        items.push(Item::Tool {
            key: format!("entity:{}:{}", items.len(), i),
            name: d.name.clone(),
            args_preview: one_line(&d.arg, 200),
            status: if failed {
                ToolStatus::Failed
            } else {
                ToolStatus::Ok
            },
            result_preview: bounded(&d.result, 700),
            error: if failed {
                one_line(&d.result, 200)
            } else {
                String::new()
            },
        });
    }
}

/// The user sent a turn: Ready/Parked → TurnRunning (+ the user card).
/// Returns the epoch the turn thread must carry.
pub fn fold_send_turn(convo: &mut EntityConvo, text: &str) -> u64 {
    convo.items.push(Item::User {
        text: text.to_string(),
    });
    convo.status = ConvoStatus::TurnRunning;
    convo.turn_started = Some(Instant::now());
    convo.turn_epoch += 1;
    convo.turn_epoch
}

/// A turn response landed. Renders the probe honestly:
/// - tool cards from `tool_details` (ledger truth, never prose),
/// - one ALWAYS-VISIBLE chip line ("· 2 memories · 1 diary entry"),
/// - full memory digests behind the details toggle (`Item::Probe`),
/// - reply → final Assistant; body error → Error card.
///
/// Returns the held draft when one should auto-send next.
pub fn fold_turn_reply(convo: &mut EntityConvo, resp: &TurnResponse) -> Option<String> {
    convo.turn_started = None;
    // Belt: a turn reply ends the turn lifecycle whatever claimed it (a
    // reply cannot race our own recovery loop — same thread — but the
    // latch must never outlive the turn).
    convo.recovery_owned = false;
    convo.turn_n = resp.turn_n.max(convo.turn_n);
    for n in &resp.notices {
        convo.items.push(Item::Info {
            text: format!("notice: {n}"),
        });
    }
    push_tool_cards(&mut convo.items, &resp.tool_details);
    // Probe chip: counts always visible; texts behind the details toggle.
    let mut chips = Vec::new();
    if !resp.memories.is_empty() {
        chips.push(format!("{} memories", resp.memories.len()));
    }
    if resp.diary_entries > 0 {
        chips.push(format!(
            "{} diary entr{}",
            resp.diary_entries,
            if resp.diary_entries == 1 { "y" } else { "ies" }
        ));
    }
    if !resp.tools_ran.is_empty() {
        chips.push(format!("tools: {}", resp.tools_ran.join(", ")));
    }
    if !chips.is_empty() {
        convo.items.push(Item::Info {
            text: format!("· {}", chips.join(" · ")),
        });
    }
    if !resp.memories.is_empty() {
        let body = resp
            .memories
            .iter()
            .map(|m| {
                let origin = if m.origin.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", m.origin)
                };
                format!("[{}] {}{}\n  {}", m.kind, m.title, origin, m.digest)
            })
            .collect::<Vec<_>>()
            .join("\n");
        convo.items.push(Item::Probe {
            title: format!("memories in context ({})", resp.memories.len()),
            body,
        });
    }
    match resp.status.as_str() {
        "failed" => {
            convo.items.push(Item::Error {
                // No-details fallback names the fact only — the status
                // strip already teaches the reopen gesture ("visit
                // closed — @name reopens").
                text: if resp.error.is_empty() {
                    "the turn failed server-side (no error details returned)".to_string()
                } else {
                    resp.error.clone()
                },
            });
            // The gateway finalizes failed runs; the next @name opens fresh.
            convo.status = ConvoStatus::Closed;
            surface_dropped_draft(convo, "the visit failed");
            None
        }
        // "completed" = a close/idle-timeout raced this turn to terminal;
        // "cancelled" = the gateway's crash-orphan recovery drove the run
        // to terminal — both are closed visits, never a parked one.
        "completed" | "cancelled" => {
            if !resp.reply.trim().is_empty() {
                convo.items.push(Item::Assistant {
                    text: resp.reply.clone(),
                    final_answer: true,
                });
            }
            convo.items.push(Item::Info {
                text: format!("the visit with {} closed during this turn", convo.name),
            });
            convo.status = ConvoStatus::Closed;
            surface_dropped_draft(convo, "the visit closed");
            None
        }
        _ => {
            if !resp.reply.trim().is_empty() {
                convo.items.push(Item::Assistant {
                    text: resp.reply.clone(),
                    final_answer: true,
                });
            }
            convo.status = ConvoStatus::Parked;
            take_held_draft(convo)
        }
    }
}

// ---------------------------------------------------------------------------
// Flow-brain transitions (summon-per-prompt lane)
// ---------------------------------------------------------------------------

/// A flow-brain summon completed. Renders the STRUCTURED degraded
/// contract (never bracket-parsed prose — the reference implementation's
/// rule): `answer` → assistant card; `degraded > 0` → warn line ("he said
/// nothing" and "the turn died" stay distinguishable); `moment_error` →
/// its own warn line. Ready for the next message (flow conversations
/// never park — each summon is complete in itself); returns the held
/// draft when one should auto-send next.
pub fn fold_flow_reply(
    convo: &mut EntityConvo,
    run_id: &str,
    answer: &str,
    degraded: i64,
    moment_error: &str,
) -> Option<String> {
    convo.turn_started = None;
    convo.run_id = run_id.to_string();
    convo.turn_n += 1;
    if !answer.trim().is_empty() {
        convo.items.push(Item::Assistant {
            text: answer.to_string(),
            final_answer: true,
        });
    }
    if degraded > 0 {
        convo.items.push(Item::Info {
            text: format!(
                "⚠ degraded turn ({degraded} degraded moment{}) — the reply may be partial",
                if degraded == 1 { "" } else { "s" }
            ),
        });
    }
    if !moment_error.trim().is_empty() {
        convo.items.push(Item::Info {
            text: format!("⚠ moment error: {moment_error}"),
        });
    }
    if answer.trim().is_empty() && degraded == 0 && moment_error.trim().is_empty() {
        // An empty answer with a CLEAN contract is still a fact worth a
        // line — never render silence as a hang.
        convo.items.push(Item::Info {
            text: "the summon completed without words".to_string(),
        });
    }
    convo.status = ConvoStatus::Ready;
    take_held_draft(convo)
}

/// A flow-brain summon FAILED (start refused, run failed, poll bound hit,
/// transport died). The conversation stays usable: unlike a visit, there
/// is no server-side thread to lose — the next message simply summons
/// again. The error renders honestly; a held draft surfaces back into
/// view rather than riding into a lane that just failed.
pub fn fold_flow_failure(convo: &mut EntityConvo, error: &str) {
    convo.turn_started = None;
    convo.items.push(Item::Error {
        text: error.to_string(),
    });
    convo.status = ConvoStatus::Ready;
    surface_dropped_draft(convo, "the summon failed");
}

/// `/end` on a flow-brain conversation: LOCAL close only — there is no
/// server visit to close (each summon already completed; the entity's
/// memory keeps everything). The view marks itself closed honestly.
pub fn fold_flow_end(convo: &mut EntityConvo) {
    convo.turn_started = None;
    convo.turn_epoch += 1; // invalidate any in-flight summon's posts
    convo.status = ConvoStatus::Closed;
    // The persistence claim is CONDITIONAL (adversary P1-3): with zero
    // completed turns nothing was summoned, so nothing formed — an
    // unconditional "memory persists" would be false exactly there.
    convo.items.push(Item::Info {
        text: if convo.turn_n > 0 {
            format!(
                "flow-brain conversation ended (view only — {}'s memory of it persists in the graph)",
                convo.name
            )
        } else {
            "flow-brain conversation ended (nothing was sent — no memory formed)".to_string()
        },
    });
    surface_dropped_draft(convo, "the conversation ended");
}

/// A transport-level turn failure (HTTP error, NOT a read timeout — the
/// timeout path enters recovery instead). The run may or may not have
/// consumed the message; the honest state is Parked with the error shown.
/// A held draft is SURFACED, never auto-sent: its predecessor may not
/// have arrived, so firing a follow-up into the same broken transport
/// would send a steer whose context never existed.
pub fn fold_turn_transport_error(convo: &mut EntityConvo, error: &str) {
    convo.turn_started = None;
    // Recovery exit path: the turn thread's PANIC fold routes here — a
    // recovery loop that died must release the latch or the poller never
    // watches this run again.
    convo.recovery_owned = false;
    convo.items.push(Item::Error {
        text: format!("turn not delivered: {error}"),
    });
    convo.status = ConvoStatus::Parked;
    surface_dropped_draft(convo, "turn delivery failed");
}

/// Read timeout: the turn is still executing server-side. Announce it and
/// let the recovery poller take over ON THE SAME thread (status unchanged:
/// TurnRunning is the truth). The recovery latch arms here: from this
/// moment the recovery loop polls /visit, so the conversation poller
/// stands down for this run.
pub fn fold_timeout_notice(convo: &mut EntityConvo) {
    convo.recovery_owned = true;
    convo.items.push(Item::Info {
        text: format!(
            "the turn is still running server-side — {} turns are non-interruptible; \
             recovering the reply by polling",
            convo.name
        ),
    });
}

/// Recovery: the run parked again — render the turns BEYOND our `turn_n`
/// from the transcript (diff by turn_n, per the plan). Returns the held
/// draft when one should auto-send next (the park IS a between-turns
/// boundary; the promise on the hold banner must hold here too).
pub fn fold_recovery_parked(
    convo: &mut EntityConvo,
    transcript: &VisitTranscript,
) -> Option<String> {
    convo.turn_started = None;
    convo.recovery_owned = false; // recovery ends here: the poller resumes
    convo.status = ConvoStatus::Parked;
    if transcript.turn_n <= convo.turn_n {
        // The server shows NO new turn: the message may never have been
        // processed (the read timeout can race a dead connection).
        // Re-rendering the previous reply here would misattribute it as
        // the answer (cycle-2 review) — say what is known instead.
        convo.items.push(Item::Info {
            text: "the visit is parked but the server shows no new turn — \
                   the message may not have been delivered; send again if it went unanswered"
                .into(),
        });
        return take_held_draft(convo);
    }
    let rendered_users = transcript.turns.iter().filter(|t| t.role == "user").count() as u64;
    // The transcript window holds the last ~N turns; our turn_n counts
    // SERVER turns. New content = assistant turns after our last-known
    // turn, bounded by the window.
    let new_turns = (transcript.turn_n - convo.turn_n).min(rendered_users) as usize;
    // Take the LAST `new_turns` assistant messages (tail-anchored, the
    // transcript endpoint's own attribution rule).
    let assistants: Vec<&crate::entities::TranscriptTurn> = transcript
        .turns
        .iter()
        .filter(|t| t.role == "assistant")
        .collect();
    let take = assistants.len().min(new_turns);
    for t in &assistants[assistants.len() - take..] {
        push_tool_cards(&mut convo.items, &t.tool_details);
        if !t.content.trim().is_empty() {
            convo.items.push(Item::Assistant {
                text: t.content.clone(),
                final_answer: true,
            });
        }
    }
    if take == 0 {
        convo.items.push(Item::Info {
            text: "the turn completed but its reply is outside the transcript window".into(),
        });
    }
    convo.turn_n = transcript.turn_n;
    take_held_draft(convo)
}

/// Recovery observed the visit CLOSED (idle-close or failure raced us):
/// render the final words and mark Closed.
pub fn fold_recovery_closed(convo: &mut EntityConvo, transcript: &VisitTranscript) {
    convo.turn_started = None;
    convo.recovery_owned = false; // recovery ends here (closed is terminal)
    if let Some(last) = transcript
        .turns
        .iter()
        .rev()
        .find(|t| t.role == "assistant" && !t.content.trim().is_empty())
    {
        push_tool_cards(&mut convo.items, &last.tool_details);
        convo.items.push(Item::Assistant {
            text: last.content.clone(),
            final_answer: true,
        });
    }
    convo.items.push(Item::Info {
        text: format!(
            "the visit with {} ended server-side ({})",
            convo.name,
            if transcript.status.is_empty() {
                "closed"
            } else {
                &transcript.status
            }
        ),
    });
    convo.status = ConvoStatus::Closed;
    surface_dropped_draft(convo, "the visit ended");
}

/// `/end` completed: render the close output (reflection summary when the
/// body carries one; notices always) and close.
pub fn fold_close(convo: &mut EntityConvo, resp: &CloseResponse) {
    convo.turn_started = None;
    convo.turn_epoch += 1;
    if !resp.summary.is_empty() {
        convo.items.push(Item::Assistant {
            text: resp.summary.clone(),
            final_answer: true,
        });
    }
    for w in &resp.warnings {
        convo.items.push(Item::Info {
            text: format!("warning: {w}"),
        });
    }
    let restored = if convo.woke_for_visit {
        " — prior sleep restored"
    } else {
        ""
    };
    let turns = resp
        .turns
        .map(|n| format!(", {n} turn(s)"))
        .unwrap_or_default();
    convo.items.push(Item::Info {
        text: format!(
            "visit closed ({}{turns}) — reflection ran{restored}",
            resp.status
        ),
    });
    convo.status = ConvoStatus::Closed;
    // Belt: holds only exist during Opening/TurnRunning and /end is
    // refused in both, so this is normally empty — but never silent.
    surface_dropped_draft(convo, "the visit closed");
}

/// `@name` on a Closed/Refused conversation: keep the old transcript,
/// open a FRESH visit (new run, new epoch — "the next @name opens fresh").
pub fn fold_reopen(convo: &mut EntityConvo, cached_state: &str) {
    convo.run_id.clear();
    convo.session_id.clear();
    convo.visit_id.clear();
    convo.turn_n = 0;
    convo.turn_started = None;
    convo.recovery_owned = false; // fresh visit: no recovery claim carries over
                                  // `@name` explicitly requests the VISIT lane — a closed flow-brain
                                  // thread reopening here must flip its brain or the record becomes a
                                  // chimera (flow field, visit transport — the reference P0 class).
    convo.brain = Brain::Visit;
    convo.status = ConvoStatus::Opening;
    convo.turn_epoch += 1;
    convo.woke_for_visit = cached_state == "asleep";
    convo.items.push(Item::Info {
        text: format!("opening a new visit with {}…", convo.name),
    });
    if convo.woke_for_visit {
        convo.items.push(Item::Info {
            text: format!(
                "{} was asleep — this visit wakes {}; close restores the sleep",
                convo.name, convo.name
            ),
        });
    }
}

/// A background poll observed the visit gone while we hold it open.
/// `observed_status` is the OLD run's terminal status word, read from its
/// transcript after the /visit body stopped naming it (the transcript
/// endpoint works on terminal runs; empty = the read failed or the run
/// is unreadable). Worded PER STATE — "idle timeout or another client"
/// was claimed for what can also be a server-side failure-terminal
/// (cycle-2 leftover):
/// - `completed` → a graceful close (the reaper's idle close or another
///   client's /end; reflection ran either way),
/// - `failed` → the run failed server-side (an Error card, not an Info),
/// - `cancelled` → the gateway's crash-orphan recovery drove it terminal,
/// - empty/unknown → the visit is gone with no terminal status readable —
///   say only what is known, never guess the cause,
/// - `waiting`/`running` → a transient misread (the status poll and the
///   transcript read disagree): apply NOTHING — the run is demonstrably
///   alive and the next poll settles it.
pub fn fold_poll_closed(convo: &mut EntityConvo, observed_status: &str) {
    if !matches!(convo.status, ConvoStatus::Parked | ConvoStatus::Ready) {
        return;
    }
    match observed_status {
        "waiting" | "running" => return, // alive after all: not a close
        "failed" => {
            convo.items.push(Item::Error {
                text: format!("the visit with {} failed server-side", convo.name),
            });
        }
        "completed" => {
            convo.items.push(Item::Info {
                text: format!(
                    "the visit with {} was closed server-side (idle timeout or another client)",
                    convo.name
                ),
            });
        }
        "cancelled" => {
            convo.items.push(Item::Info {
                text: format!(
                    "the visit with {} was cancelled server-side (crash-orphan recovery)",
                    convo.name
                ),
            });
        }
        _ => {
            convo.items.push(Item::Info {
                text: format!(
                    "the visit with {} is gone server-side (no terminal status readable)",
                    convo.name
                ),
            });
        }
    }
    convo.status = ConvoStatus::Closed;
    surface_dropped_draft(convo, "the visit closed server-side");
}

/// A background cognition poll: update the chip's state word + live spend.
pub fn fold_poll_cognition(convo: &mut EntityConvo, state: &str, live_visit_tokens: Option<u64>) {
    if !state.is_empty() {
        convo.entity_state = state.to_string();
    }
    if live_visit_tokens.is_some() {
        convo.spend_tokens = live_visit_tokens;
    }
}

/// Hold a draft typed during a running turn (v1 between-turns steering:
/// later text REPLACES the held draft — the composer banner says so).
pub fn hold_draft(convo: &mut EntityConvo, text: &str) {
    convo.held_draft = text.to_string();
}

/// Take the held draft for auto-send (empties the slot).
pub fn take_held_draft(convo: &mut EntityConvo) -> Option<String> {
    if convo.held_draft.trim().is_empty() {
        convo.held_draft.clear();
        return None;
    }
    Some(std::mem::take(&mut convo.held_draft))
}

/// Drop the held draft VISIBLY: typed text is never silently lost (the
/// agent lane's pending-steer honesty rule) — the undelivered words render
/// as an Error card naming why they never went.
fn surface_dropped_draft(convo: &mut EntityConvo, why: &str) {
    if convo.held_draft.trim().is_empty() {
        convo.held_draft.clear();
        return;
    }
    let text = std::mem::take(&mut convo.held_draft);
    convo.items.push(Item::Error {
        text: format!("held message not delivered ({why}). Your text: {text}"),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        transcript_from_response, turn_from_response, visit_open_from_response,
        visit_status_from_response,
    };
    use serde_json::json;

    fn fixture_turn() -> TurnResponse {
        turn_from_response(
            &serde_json::from_str(include_str!("../tests/fixtures/entities/turn_reply.json"))
                .unwrap(),
        )
    }

    #[test]
    fn open_success_moves_to_ready_with_warnings() {
        let mut c = EntityConvo::opening("Castor", "asleep");
        assert_eq!(c.name, "castor", "slug lowercases");
        assert!(c.woke_for_visit);
        assert!(c
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text.contains("was asleep"))));
        let open = visit_open_from_response(
            &serde_json::from_str(include_str!("../tests/fixtures/entities/visit_open.json"))
                .unwrap(),
        );
        let epoch_before = c.turn_epoch;
        fold_open_success(&mut c, &open);
        assert_eq!(c.status, ConvoStatus::Ready);
        assert_eq!(c.run_id, "b0a1c2d3-e4f5-4678-9abc-def012345678");
        assert!(c.turn_epoch > epoch_before, "epoch bumps on adopt/open");
        assert!(c
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text.contains("warning:") )));
    }

    #[test]
    fn refused_open_renders_detail_verbatim() {
        let mut c = EntityConvo::opening("castor", "awake");
        let detail: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/entities/refusal_409.json"))
                .unwrap();
        fold_open_refused(&mut c, detail.get("detail").unwrap().as_str().unwrap());
        assert_eq!(c.status, ConvoStatus::Refused);
        assert!(c
            .items
            .iter()
            .any(|i| matches!(i, Item::Error { text } if text.contains("one life, one summon"))));
    }

    #[test]
    fn adopt_renders_window_honesty_when_turn_n_exceeds_rendered() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        let status = visit_status_from_response(
            &serde_json::from_str(include_str!(
                "../tests/fixtures/entities/visit_status_open.json"
            ))
            .unwrap(),
        );
        let transcript = transcript_from_response(
            &serde_json::from_str(include_str!("../tests/fixtures/entities/transcript.json"))
                .unwrap(),
        );
        fold_adopt(&mut c, &status, &transcript);
        assert_eq!(c.status, ConvoStatus::Parked);
        assert_eq!(c.turn_n, 12);
        assert!(
            c.items.iter().any(|i| matches!(i, Item::Info { text }
                if text.contains("earlier turns live in the entity's memory"))),
            "sliding-window honesty line present"
        );
        // Transcript turns rendered: 2 user + 2 assistant + 1 tool card.
        assert_eq!(
            c.items
                .iter()
                .filter(|i| matches!(i, Item::User { .. }))
                .count(),
            2
        );
        assert_eq!(
            c.items
                .iter()
                .filter(|i| matches!(i, Item::Tool { .. }))
                .count(),
            1
        );
        // The rendered user turn splits: raw words on the user card, the
        // dated MEMORIES chrome details-gated as a probe (live shape).
        assert!(
            c.items
                .iter()
                .any(|i| matches!(i, Item::User { text } if text == "how are the doors today?")),
            "user card carries only the visitor's words"
        );
        assert!(
            !c.items
                .iter()
                .any(|i| matches!(i, Item::User { text } if text.contains("MEMORIES ("))),
            "prompt chrome never renders as the visitor's words"
        );
        assert!(
            c.items
                .iter()
                .any(|i| matches!(i, Item::Probe { title, body }
                if title.starts_with("memories in context") && body.contains("as_of_seq=223"))),
            "the memories block rides details-gated"
        );
        assert!(
            !c.items
                .iter()
                .any(|i| matches!(i, Item::Info { text } if text.starts_with("opening a visit"))),
            "the transient opening line is dropped on adopt"
        );
    }

    #[test]
    fn adopt_without_window_overflow_says_nothing_extra() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        let status = visit_status_from_response(&json!({
            "open": true, "run_id": "r1", "session_id": "s", "visit_id": "v",
            "turn_n": 2, "status": "waiting"}));
        let transcript = transcript_from_response(&json!({
            "run_id": "r1", "turn_n": 2, "status": "waiting",
            "turns": [
                {"role": "user", "content": "a"}, {"role": "assistant", "content": "b"},
                {"role": "user", "content": "c"}, {"role": "assistant", "content": "d"}]}));
        fold_adopt(&mut c, &status, &transcript);
        assert!(!c
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text.contains("earlier turns"))));
    }

    #[test]
    fn turn_reply_renders_probe_chip_tools_and_reply() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        let epoch = fold_send_turn(&mut c, "hello");
        assert_eq!(c.status, ConvoStatus::TurnRunning);
        assert!(c.turn_started.is_some());
        assert_eq!(epoch, c.turn_epoch);
        let held = fold_turn_reply(&mut c, &fixture_turn());
        assert_eq!(held, None);
        assert_eq!(c.status, ConvoStatus::Parked);
        assert!(c.turn_started.is_none());
        assert_eq!(c.turn_n, 1);
        // The probe chip line (always visible).
        assert!(c.items.iter().any(|i| matches!(i, Item::Info { text }
            if text.contains("2 memories") && text.contains("1 diary entry"))));
        // Full digests behind the details toggle.
        assert!(c
            .items
            .iter()
            .any(|i| matches!(i, Item::Probe { title, body }
            if title.contains("memories in context") && body.contains("door check"))));
        // Ledger-truth tool card.
        assert!(c
            .items
            .iter()
            .any(|i| matches!(i, Item::Tool { name, status, .. }
            if name == "search_memory" && *status == ToolStatus::Ok)));
        // The reply is the final answer.
        assert!(matches!(c.items.last().unwrap(),
            Item::Assistant { text, final_answer: true } if text.contains("connectivity")));
    }

    #[test]
    fn failed_turn_body_closes_with_error_card() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        fold_send_turn(&mut c, "hello");
        hold_draft(&mut c, "queued while running");
        let resp = turn_from_response(&json!({
            "run_id": "", "reply": "", "turn_n": 1, "status": "failed",
            "error": "internal error: the turn loop failed"}));
        let held = fold_turn_reply(&mut c, &resp);
        assert_eq!(held, None, "a failed turn never auto-sends the held draft");
        assert_eq!(c.status, ConvoStatus::Closed);
        assert!(c.held_draft.is_empty());
        assert!(c
            .items
            .iter()
            .any(|i| matches!(i, Item::Error { text } if text.contains("failed"))));
    }

    #[test]
    fn held_draft_auto_sends_on_park_and_replaces() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        fold_send_turn(&mut c, "first");
        hold_draft(&mut c, "second");
        hold_draft(&mut c, "third (replaces second)");
        let held = fold_turn_reply(&mut c, &fixture_turn());
        assert_eq!(held.as_deref(), Some("third (replaces second)"));
        assert!(c.held_draft.is_empty());
        // Whitespace-only holds never send.
        hold_draft(&mut c, "   ");
        assert_eq!(take_held_draft(&mut c), None);
    }

    #[test]
    fn epoch_guard_rejects_stale_results() {
        let mut convos = vec![EntityConvo::opening("doorcheck", "awake")];
        fold_open_success(&mut convos[0], &VisitOpen::default());
        convos[0].run_id = "r1".into();
        let epoch = fold_send_turn(&mut convos[0], "hello");
        assert!(guard(&convos, "doorcheck", "r1", epoch).is_some());
        // A close bumps the epoch: the in-flight turn result is now stale.
        fold_close(&mut convos[0], &CloseResponse::default());
        assert!(
            guard(&convos, "doorcheck", "r1", epoch).is_none(),
            "stale epoch applies nothing"
        );
        assert!(guard(&convos, "doorcheck", "r2", epoch + 1).is_none());
        assert!(guard(&convos, "ghost", "r1", epoch).is_none());
    }

    #[test]
    fn recovery_parked_diffs_by_turn_n() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        c.turn_n = 11; // we knew 11 turns; the transcript says 12 now
        fold_send_turn(&mut c, "twelfth question");
        fold_timeout_notice(&mut c);
        assert_eq!(c.status, ConvoStatus::TurnRunning, "timeout keeps running");
        let transcript = transcript_from_response(
            &serde_json::from_str(include_str!("../tests/fixtures/entities/transcript.json"))
                .unwrap(),
        );
        fold_recovery_parked(&mut c, &transcript);
        assert_eq!(c.status, ConvoStatus::Parked);
        assert_eq!(c.turn_n, 12);
        // Exactly ONE new assistant turn rendered (12 - 11).
        let replies = c
            .items
            .iter()
            .filter(|i| matches!(i, Item::Assistant { .. }))
            .count();
        assert_eq!(replies, 1);
        assert!(matches!(c.items.last().unwrap(),
            Item::Assistant { text, .. } if text.contains("the gate answers")));
    }

    #[test]
    fn recovery_closed_renders_last_words() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        fold_send_turn(&mut c, "hello");
        let transcript = transcript_from_response(&json!({
            "run_id": "r1", "turn_n": 1, "status": "completed",
            "turns": [{"role": "user", "content": "hello"},
                       {"role": "assistant", "content": "goodbye words"}]}));
        fold_recovery_closed(&mut c, &transcript);
        assert_eq!(c.status, ConvoStatus::Closed);
        assert!(c
            .items
            .iter()
            .any(|i| matches!(i, Item::Assistant { text, .. } if text == "goodbye words")));
        assert!(c
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text.contains("ended server-side"))));
    }

    #[test]
    fn poll_close_marks_parked_convos_only() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        fold_send_turn(&mut c, "x");
        fold_poll_closed(&mut c, "completed"); // TurnRunning: the turn thread owns this
        assert_eq!(c.status, ConvoStatus::TurnRunning);
        c.status = ConvoStatus::Parked;
        fold_poll_closed(&mut c, "completed");
        assert_eq!(c.status, ConvoStatus::Closed);
        assert!(c
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text.contains("idle timeout"))));
    }

    #[test]
    fn poll_close_words_each_observed_state() {
        let parked = || {
            let mut c = EntityConvo::opening("doorcheck", "awake");
            fold_open_success(&mut c, &VisitOpen::default());
            c.status = ConvoStatus::Parked;
            c
        };
        // completed → the graceful-close wording (idle reaper or /end).
        let mut c = parked();
        fold_poll_closed(&mut c, "completed");
        assert_eq!(c.status, ConvoStatus::Closed);
        assert!(c.items.iter().any(|i| matches!(i, Item::Info { text }
            if text.contains("closed server-side (idle timeout or another client)"))));
        // failed → an ERROR card naming the failure, never "idle timeout".
        let mut c = parked();
        fold_poll_closed(&mut c, "failed");
        assert_eq!(c.status, ConvoStatus::Closed);
        assert!(c.items.iter().any(|i| matches!(i, Item::Error { text }
            if text.contains("failed server-side"))));
        assert!(
            !c.items
                .iter()
                .any(|i| matches!(i, Item::Info { text } if text.contains("idle timeout"))),
            "a failure is never worded as an idle close"
        );
        // cancelled → the crash-orphan recovery wording.
        let mut c = parked();
        fold_poll_closed(&mut c, "cancelled");
        assert_eq!(c.status, ConvoStatus::Closed);
        assert!(c.items.iter().any(|i| matches!(i, Item::Info { text }
            if text.contains("cancelled server-side"))));
        // unknown (transcript unreadable) → gone, cause NOT claimed.
        let mut c = parked();
        fold_poll_closed(&mut c, "");
        assert_eq!(c.status, ConvoStatus::Closed);
        assert!(c.items.iter().any(|i| matches!(i, Item::Info { text }
            if text.contains("gone server-side (no terminal status readable)"))));
        // waiting/running observed = a transient misread: apply NOTHING.
        for alive in ["waiting", "running"] {
            let mut c = parked();
            let before = c.items.len();
            fold_poll_closed(&mut c, alive);
            assert_eq!(c.status, ConvoStatus::Parked, "{alive}: not a close");
            assert_eq!(c.items.len(), before, "{alive}: no line rendered");
        }
    }

    #[test]
    fn recovery_latch_arms_on_timeout_and_clears_on_every_exit() {
        // Arm: the timeout notice is the recovery handoff.
        let start = || {
            let mut c = EntityConvo::opening("doorcheck", "awake");
            fold_open_success(&mut c, &VisitOpen::default());
            fold_send_turn(&mut c, "x");
            fold_timeout_notice(&mut c);
            assert!(c.recovery_owned, "latch arms with the timeout notice");
            assert_eq!(c.status, ConvoStatus::TurnRunning);
            c
        };
        // Exit 1: recovery observed the park.
        let mut c = start();
        c.turn_n = 0;
        let transcript = transcript_from_response(&json!({
            "run_id": "r", "turn_n": 1, "status": "waiting",
            "turns": [{"role": "user", "content": "x"},
                       {"role": "assistant", "content": "y"}]}));
        fold_recovery_parked(&mut c, &transcript);
        assert!(!c.recovery_owned, "park releases the latch");
        // Exit 2: recovery observed the close.
        let mut c = start();
        fold_recovery_closed(&mut c, &VisitTranscript::default());
        assert!(!c.recovery_owned, "close releases the latch");
        // Exit 3: the thread died (panic fold routes here).
        let mut c = start();
        fold_turn_transport_error(&mut c, "thread died");
        assert!(!c.recovery_owned, "transport error releases the latch");
        // Exit 4 (belt): a reply landing anyway ends the claim.
        let mut c = start();
        fold_turn_reply(&mut c, &fixture_turn());
        assert!(!c.recovery_owned, "a turn reply releases the latch");
        // Reopen never carries a stale claim into a fresh visit.
        let mut c = start();
        c.status = ConvoStatus::Closed;
        fold_reopen(&mut c, "awake");
        assert!(!c.recovery_owned);
    }

    #[test]
    fn close_restores_sleep_note_only_when_woken() {
        let mut woke = EntityConvo::opening("doorcheck", "asleep");
        fold_open_success(&mut woke, &VisitOpen::default());
        fold_close(
            &mut woke,
            &CloseResponse {
                status: "completed".into(),
                summary: "a calm check".into(),
                ..Default::default()
            },
        );
        assert!(woke
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text.contains("prior sleep restored"))));
        assert!(woke
            .items
            .iter()
            .any(|i| matches!(i, Item::Assistant { text, .. } if text.contains("a calm check"))));
        let mut awake = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut awake, &VisitOpen::default());
        fold_close(&mut awake, &CloseResponse::default());
        assert!(!awake
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text.contains("sleep restored"))));
    }

    #[test]
    fn cognition_poll_updates_chip_state_and_spend() {
        let mut c = EntityConvo::opening("doorcheck", "asleep");
        fold_poll_cognition(&mut c, "awake", Some(1234));
        assert_eq!(c.entity_state, "awake");
        assert_eq!(c.spend_tokens, Some(1234));
        // A poll without live spend keeps the last-known value.
        fold_poll_cognition(&mut c, "", None);
        assert_eq!(c.entity_state, "awake");
        assert_eq!(c.spend_tokens, Some(1234));
    }

    #[test]
    fn adopt_keeps_prior_history_above_and_clears_stale_wake_claims() {
        // Reopen-then-adopt: the OLD visit's transcript must stay ABOVE the
        // adopted one (chronology), the transient "opening a new visit…"
        // line must drop, and the stale wake note must not survive — a 409
        // proves the entity was already in a live visit, so close cannot
        // honestly claim "prior sleep restored" (woke_for_visit clears).
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        c.run_id = "old-run".into();
        c.items.push(Item::Assistant {
            text: "old visit reply".into(),
            final_answer: true,
        });
        fold_close(&mut c, &CloseResponse::default());
        // The cached roster now (stale) says asleep; reopen notes the wake.
        fold_reopen(&mut c, "asleep");
        assert!(c.woke_for_visit);
        assert!(c
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text.contains("this visit wakes"))));
        let status = visit_status_from_response(&json!({
            "open": true, "run_id": "new-run", "session_id": "s2", "visit_id": "v2",
            "turn_n": 1, "status": "waiting"}));
        let transcript = transcript_from_response(&json!({
            "run_id": "new-run", "turn_n": 1, "status": "waiting",
            "turns": [{"role": "user", "content": "someone else's turn"},
                       {"role": "assistant", "content": "new visit reply"}]}));
        fold_adopt(&mut c, &status, &transcript);
        assert!(!c.woke_for_visit, "adopt clears the stale wake claim");
        assert!(
            !c.items.iter().any(|i| matches!(i, Item::Info { text }
                if text.contains("this visit wakes") || text.starts_with("opening a "))),
            "transient opening + wake lines dropped on adopt"
        );
        let old_ix = c
            .items
            .iter()
            .position(|i| matches!(i, Item::Assistant { text, .. } if text == "old visit reply"))
            .expect("old transcript kept");
        let adopt_ix = c
            .items
            .iter()
            .position(|i| matches!(i, Item::Info { text } if text.starts_with("adopted the live")))
            .expect("adopt notice present");
        let new_ix = c
            .items
            .iter()
            .position(|i| matches!(i, Item::Assistant { text, .. } if text == "new visit reply"))
            .expect("adopted transcript rendered");
        assert!(
            old_ix < adopt_ix && adopt_ix < new_ix,
            "chronology: old visit above the adopted one (old={old_ix} adopt={adopt_ix} new={new_ix})"
        );
    }

    #[test]
    fn recovery_with_no_new_turn_never_replays_the_previous_reply() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        c.turn_n = 2; // server already held 2 turns before our send
        fold_send_turn(&mut c, "a message the server may never have seen");
        // Recovery observes the run parked with turn_n UNCHANGED: the POST
        // never landed. The old reply must NOT re-render as the answer.
        let transcript = transcript_from_response(&json!({
            "run_id": "r1", "turn_n": 2, "status": "waiting",
            "turns": [{"role": "user", "content": "earlier"},
                       {"role": "assistant", "content": "an earlier reply"}]}));
        let held = fold_recovery_parked(&mut c, &transcript);
        assert_eq!(held, None);
        assert_eq!(c.status, ConvoStatus::Parked);
        assert_eq!(
            c.items
                .iter()
                .filter(|i| matches!(i, Item::Assistant { .. }))
                .count(),
            0,
            "no stale reply misattributed as the answer"
        );
        assert!(c.items.iter().any(|i| matches!(i, Item::Info { text }
            if text.contains("no new turn"))));
    }

    #[test]
    fn recovery_park_returns_the_held_draft_for_auto_send() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        c.turn_n = 1;
        fold_send_turn(&mut c, "second question");
        hold_draft(&mut c, "third question, held during the slow turn");
        let transcript = transcript_from_response(&json!({
            "run_id": "r1", "turn_n": 2, "status": "waiting",
            "turns": [{"role": "user", "content": "second question"},
                       {"role": "assistant", "content": "the slow answer"}]}));
        let held = fold_recovery_parked(&mut c, &transcript);
        assert_eq!(
            held.as_deref(),
            Some("third question, held during the slow turn"),
            "a recovered park honors the hold banner's auto-send promise"
        );
        assert!(c.held_draft.is_empty());
        assert!(matches!(c.items.last().unwrap(),
            Item::Assistant { text, .. } if text == "the slow answer"));
    }

    #[test]
    fn dropped_held_drafts_surface_visibly_never_silently() {
        let dropped_text = |c: &EntityConvo| {
            c.items.iter().any(|i| {
                matches!(i, Item::Error { text }
                if text.contains("held message not delivered") && text.contains("my held words"))
            })
        };
        // Failed turn body.
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        fold_send_turn(&mut c, "x");
        hold_draft(&mut c, "my held words");
        let failed = turn_from_response(&json!({
            "run_id": "", "reply": "", "turn_n": 1, "status": "failed", "error": "boom"}));
        assert_eq!(fold_turn_reply(&mut c, &failed), None);
        assert!(dropped_text(&c), "failed turn surfaces the held draft");
        assert!(c.held_draft.is_empty());
        // Poll-observed close.
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        c.status = ConvoStatus::Parked;
        hold_draft(&mut c, "my held words");
        fold_poll_closed(&mut c, "completed");
        assert!(dropped_text(&c), "poll close surfaces the held draft");
        // Transport error (never auto-sent: the predecessor may be lost).
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        fold_send_turn(&mut c, "x");
        hold_draft(&mut c, "my held words");
        fold_turn_transport_error(&mut c, "connection refused");
        assert!(dropped_text(&c), "transport error surfaces the held draft");
        assert_eq!(c.status, ConvoStatus::Parked);
        // Refused open.
        let mut c = EntityConvo::opening("doorcheck", "awake");
        hold_draft(&mut c, "my held words");
        fold_open_refused(&mut c, "entity is paused");
        assert!(dropped_text(&c), "refused open surfaces the held draft");
        // Recovery-observed close.
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        fold_send_turn(&mut c, "x");
        hold_draft(&mut c, "my held words");
        fold_recovery_closed(&mut c, &VisitTranscript::default());
        assert!(dropped_text(&c), "recovery close surfaces the held draft");
    }

    #[test]
    fn cancelled_turn_body_closes_like_completed() {
        let mut c = EntityConvo::opening("doorcheck", "awake");
        fold_open_success(&mut c, &VisitOpen::default());
        fold_send_turn(&mut c, "x");
        let resp = turn_from_response(&json!({
            "run_id": "r", "reply": "", "turn_n": 1, "status": "cancelled"}));
        assert_eq!(fold_turn_reply(&mut c, &resp), None);
        assert_eq!(
            c.status,
            ConvoStatus::Closed,
            "a cancelled run is terminal, never a parked visit"
        );
    }

    #[test]
    fn chip_paint_plan_keeps_the_focused_chip_visible() {
        // 5 chips of 16 cells (18 with the separator), 4 reserved for the
        // "+N" marker while chips remain. At 40 cells identity order fits
        // exactly two.
        let w = [16usize; 5];
        assert_eq!(chip_paint_plan(&w, None, 40), (vec![0, 1], 3));
        // Focused chip already visible: identity order stands.
        assert_eq!(chip_paint_plan(&w, Some(1), 40), (vec![0, 1], 3));
        // Focused chip would hide behind "+N": it paints FIRST; the rest
        // keep identity order behind it; "+N" counts every unpainted chip.
        assert_eq!(chip_paint_plan(&w, Some(4), 40), (vec![4, 0], 3));
        // Nothing fits: honest empty paint + the full count hidden.
        assert_eq!(chip_paint_plan(&w, Some(4), 10), (Vec::new(), 5));
        // A single chip fits without the marker reservation.
        assert_eq!(chip_paint_plan(&[16], None, 18), (vec![0], 0));
        // Out-of-range focus (stale index) degrades to identity order.
        assert_eq!(chip_paint_plan(&w, Some(9), 40), (vec![0, 1], 3));
    }

    #[test]
    fn chips_read_status_words() {
        let mut c = EntityConvo::opening("castor", "awake");
        assert_eq!(c.chip(), "◆castor opening…");
        fold_open_success(&mut c, &VisitOpen::default());
        assert_eq!(c.chip(), "◆castor ready");
        fold_send_turn(&mut c, "x");
        assert!(c.chip().starts_with("◆castor ✎"));
        c.status = ConvoStatus::Parked;
        assert_eq!(c.chip(), "◆castor parked");
        assert_eq!(fmt_elapsed(42), "42s");
        assert_eq!(fmt_elapsed(185), "3m05s");
    }

    #[test]
    fn elapsed_formats_hours_never_raw_seconds() {
        // POLISH-1: `33628s` on the strip read as broken — hours render
        // as `9h20m` (minute precision; seconds noise dropped).
        assert_eq!(fmt_elapsed(33628), "9h20m");
        assert_eq!(fmt_elapsed(3600), "1h00m");
        assert_eq!(fmt_elapsed(3599), "59m59s");
        assert_eq!(fmt_elapsed(7265), "2h01m");
        assert_eq!(fmt_elapsed(0), "0s");
    }
}
