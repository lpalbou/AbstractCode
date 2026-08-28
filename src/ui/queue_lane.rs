//! The queue + steer lane: client-held future work and mid-run guidance.
//!
//! Extracted from `ui/mod.rs` (consolidation round-2 P2-2). Three
//! families, one concern — text the user wrote that is NOT yet a run:
//! - the steer trio (`steer_or_buffer`/`buffer_steer`/`send_steer_to`):
//!   guidance for the CYCLING run, buffered until a cycling target
//!   exists (never dropped, never sent to a dead run);
//! - the queue quartet (`enqueue_prompt`/`queue_preview`/
//!   `swap_queue_for_session`/`restore_session_queue`): FIFO prompts
//!   persisted per session (write-through; restores land PAUSED so a
//!   restart never auto-spends);
//! - the wiring trio (`wire_queue_persistence`/`wire_queue_drain`/
//!   `wire_pending_steer`) root installs once.
//!
//! `queue_preview` is re-exported from `ui` (three external consumers);
//! everything else routes through `ui::submit`/`reset_session_state`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use abstracttui::reactive::{after, Scope};

use crate::runner::Cmd;
use crate::store::{Phase, Store};
use crate::transcript::Item;
use crate::ui::{load_session_tool_prefs, persist_prefs, start_run, UiCtx};

/// Deliver a steer to a KNOWN cycling run (the only target a guidance
/// inbox is actually drained from — wrapper bundles run the agent loop in
/// a subrun, so a root-targeted steer is silently never folded).
pub(crate) fn send_steer_to(store: Store, ctx: &UiCtx, target: &str, text: &str) {
    store.fold.update(|f| {
        f.push_item(Item::Steer {
            text: text.to_string(),
        })
    });
    ctx.send(Cmd::Steer {
        run_id: target.to_string(),
        text: text.to_string(),
    });
}

/// Buffer text for delivery once the fold's cycling target lands. Rapid
/// submits append newline-joined, KEEPING the first arming identity (a
/// Starting-armed buffer joined by a Running-armed line still delivers on
/// the new tree's first cycle — both predicates pass together there).
pub(crate) fn buffer_steer(store: Store, text: &str, while_starting: bool) {
    let root = store.fold.with_untracked(|f| f.root_run_id().to_string());
    store.pending_steer.update(|slot| {
        *slot = Some(match slot.take() {
            Some(mut prev) => {
                prev.text.push('\n');
                prev.text.push_str(text);
                prev
            }
            None => crate::store::PendingSteer {
                armed_at_root: root,
                armed_while_starting: while_starting,
                text: text.to_string(),
            },
        });
    });
}

/// Steer the active run — or buffer when it has NO cycling target yet.
/// The manual steer path shared the silent window between Running and the
/// first cycle record: a root-targeted steer there was never folded
/// (plan item 1, cycle-2 generalization of the Starting-only buffer).
pub(crate) fn steer_or_buffer(store: Store, ctx: &UiCtx, text: &str) {
    if store.phase.get_untracked() == Phase::Idle {
        store.notify("no active run to steer");
        return;
    }
    match store.fold.with_untracked(|f| f.cycling_target()) {
        Some(target) => send_steer_to(store, ctx, &target, text),
        None => {
            buffer_steer(store, text, false);
            store.notify("run hasn't reached its first cycle — guidance buffered");
        }
    }
}

/// Preview line for queued-prompt echoes/rows (one line, bounded).
pub fn queue_preview(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        out.push(if ch == '\n' || ch == '\r' || ch == '\t' {
            ' '
        } else {
            ch
        });
        if out.chars().count() >= 70 {
            out.push('…');
            break;
        }
    }
    out
}

/// Session-boundary queue swap (cycle-2: STASH, never drop): the leaving
/// session's queue is already durable in its prefs slot (write-through on
/// every mutation — `wire_queue_persistence`); this echoes the stash
/// visibly, drops the moment-bound steer buffer with an echo, and loads
/// the TARGET session's stash PAUSED (a restore never auto-starts).
pub(crate) fn swap_queue_for_session(store: Store, ctx: &UiCtx, old_sid: &str, new_sid: &str) {
    let stashed = store.queue.with_untracked(|q| q.len());
    if stashed > 0 {
        let short = &old_sid[..old_sid.len().min(18)];
        store.fold.update(|f| {
            f.push_item(Item::Info {
                text: format!(
                    "{stashed} queued prompt(s) stashed with session {short} — they restore (paused) when you return"
                ),
            })
        });
    }
    // The steer buffer is moment-bound guidance for a run that no longer
    // matters — dropped WITH an echo (plan: never a silent drop).
    if let Some(ps) = store.reset_steer_lane() {
        store.fold.update(|f| {
            f.push_item(Item::Info {
                text: format!(
                    "buffered guidance dropped (session boundary): {}",
                    queue_preview(&ps.text)
                ),
            })
        });
    }
    restore_session_queue(store, ctx, new_sid);
    // The tools-modal config follows its session too (operator ask): load
    // the target's saved slot, or seed a fresh one (camera default-off
    // arms for when the inventory is already loaded — the effect re-runs
    // on the flag flip because it reads `store.tools`, which is non-empty
    // by now on a live switch).
    load_session_tool_prefs(store, ctx, new_sid);
    // The goal follows its session: load the target's slot (label only —
    // the fold flag re-derives from run identity in `wire_goal`).
    let goal = ctx
        .prefs
        .borrow()
        .session_goal(new_sid)
        .map(|(text, run_id)| crate::store::GoalState { text, run_id });
    store.goal.set(goal);
}

/// Load a session's stashed queue: items restore PAUSED with a visible
/// notice; an empty stash resets the lane. NEVER auto-starts (the one
/// rule tying quit/reopen and session switches together: a queue only
/// auto-drains within the session + process continuity it was armed in).
pub fn restore_session_queue(store: Store, ctx: &UiCtx, sid: &str) {
    let stash = ctx.prefs.borrow().session_queue(sid);
    let items: Vec<crate::store::QueuedPrompt> = stash
        .into_iter()
        .map(|text| crate::store::QueuedPrompt {
            id: store.mint_queue_id(),
            text,
        })
        .collect();
    let n = items.len();
    // Paused BEFORE the queue lands: the drain effect observes the queue
    // write synchronously, and an unpaused restore would start run one.
    store.queue_paused.set(n > 0);
    store.queue.set(items);
    if n > 0 {
        store.notify(format!(
            "{n} queued prompt(s) restored (paused — /queue then r resumes)"
        ));
    }
}

/// `/queue <text>` — enqueue a prompt (FIFO). Draining is the
/// `wire_queue_drain` effect's job: idle + unpaused queues start
/// immediately; during a run the item waits for a successful finish.
pub fn enqueue_prompt(store: Store, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        store.notify("usage: /queue <prompt> — bare /queue opens the manager");
        return;
    }
    let id = store.mint_queue_id();
    store.queue.update(|q| {
        q.push(crate::store::QueuedPrompt {
            id,
            text: text.to_string(),
        })
    });
    let n = store.queue.with_untracked(|q| q.len());
    if store.queue_paused.get_untracked() {
        store.notify(format!(
            "queued #{n} (queue paused — /queue then r resumes): {}",
            queue_preview(text)
        ));
    } else {
        store.notify(format!("queued #{n}: {}", queue_preview(text)));
    }
}

/// Write-through queue persistence (cycle-2: the queue PERSISTS per
/// session — "piling up requests that each gets executed sequentially"
/// is a promise a silent quit-drop broke). Tracks the QUEUE only; the
/// session id is read untracked, so a session switch (which sets the id
/// FIRST, then swaps the queue) never files the old queue under the new
/// id. Every restore path loads PAUSED, so persistence costs zero
/// unattended token spend on stale context.
pub(crate) fn wire_queue_persistence(cx: Scope, store: Store, ctx: UiCtx) {
    cx.effect(move || {
        let texts: Vec<String> = store
            .queue
            .with(|q| q.iter().map(|p| p.text.clone()).collect());
        let sid = store.session_id.get_untracked();
        if sid.is_empty() {
            return; // pre-boot: nothing to key the slot on
        }
        persist_prefs(&ctx, |p| p.set_session_queue(&sid, &texts));
    });
}

/// The queue drain: ONE effect owns every queue transition so pausing
/// and draining can never race each other (plan item 1).
///
/// Semantics table (docs/design/plan-interaction-model.md, cycle-2):
/// - run completes successfully → auto-drain the next item as a NEW run
///   (StartOpts build inside `start_run` AT DRAIN TIME, so the context
///   carries the just-finished answer);
/// - run fails / is cancelled → `queue_paused = true`, items kept;
/// - a queued START that fails (HTTP or synchronous client refusal) →
///   item RESTORED AT HEAD + paused — nothing was spent, `r` retries the
///   same item (loop-free: paused blocks the drain until explicit resume);
/// - client-side UNREADINESS (no workflow / dead worker) is checked
///   BEFORE dequeuing — refusal pauses with the item KEPT (an unchecked
///   drain either stalls silently armed or loses the popped item);
/// - drain HOLDS while `fold.pending_wait` is some — waits can arm after
///   `finished` (helper subrun asks have no finished gate), and a
///   drain-started run would `begin_run`-wipe the prompt and orphan the
///   wait. Fold-tracking makes the wait's RESOLUTION re-fire the drain;
/// - manual run while paused → proceeds, does NOT auto-resume;
/// - the dequeue itself runs as a DEFERRED job (`after(ZERO)`): the
///   phase flips Idle INSIDE runner-posted closures that keep touching
///   signals afterwards — a synchronous start would interleave the new
///   run's cards with the old run's teardown writes (e.g. the
///   start-failure error card landing AFTER the next queued user card).
///
/// `last_outcome` is a take-semantics mailbox: consumed here, reset to
/// None — edge-triggering by construction (a modal `r` resume must not
/// re-pause against a stale Failed; a replayed Success must not
/// double-drain).
pub(crate) fn wire_queue_drain(cx: Scope, store: Store, ctx: UiCtx) {
    use crate::store::RunOutcome;
    // The item a drain-started run is executing, with the root id at
    // dequeue time: a Failed outcome whose fold root NEVER CHANGED means
    // the START failed (the runner's Err post skips begin_run) — restore
    // the item at head. A root that DID change means the run began and
    // failed mid-flight: the item was spent (transcript keeps the
    // evidence), items-kept + pause is the whole answer.
    let inflight: Rc<RefCell<Option<(crate::store::QueuedPrompt, String)>>> =
        Rc::new(RefCell::new(None));
    // One deferred dequeue at a time (the effect can re-fire between the
    // schedule and the job).
    let scheduled = Rc::new(Cell::new(false));
    cx.effect(move || {
        let phase = store.phase.get();
        let queue_len = store.queue.with(|q| q.len());
        let paused = store.queue_paused.get();
        let outcome = store.last_outcome.get();
        // TRACKED fold reads: the wait guard must re-fire when the wait
        // resolves (a fold change with no phase change), and the restore
        // decision reads the root id.
        let (wait_pending, fold_root) = store
            .fold
            .with(|f| (f.pending_wait.is_some(), f.root_run_id().to_string()));
        if phase != Phase::Idle {
            return; // a run is active; outcomes are consumed on Idle
        }
        // Quit gate (quit-modal design §D5): with a quit in flight, a
        // Success conclusion must NOT start the next queued prompt —
        // the sequencer auto-quits on the conclusion, and a drain here
        // would abandon a run the user never saw. TRACKED: *stay*
        // (state back to None) re-arms the drain.
        if store
            .quit_state
            .with(|q| !matches!(q, crate::store::QuitState::None))
        {
            return;
        }
        if outcome != RunOutcome::None {
            store.last_outcome.set(RunOutcome::None);
            if outcome.holds_the_queue() {
                // Start-failure restore (cycle-2): nothing was spent.
                let started_item = inflight.borrow_mut().take();
                if let Some((item, armed_root)) = started_item {
                    // Only a start FAILURE gives the prompt back — a turn that
                    // ran and stopped short was spent, however incomplete.
                    if outcome == RunOutcome::Failed && fold_root == armed_root {
                        store.queue.update(|q| q.insert(0, item));
                    }
                }
                let held = store.queue.with_untracked(|q| q.len());
                if held > 0 && !store.queue_paused.get_untracked() {
                    store.queue_paused.set(true);
                    store.notify(format!(
                        "queue paused — run {} ({held} prompt(s) held · /queue then r resumes)",
                        match outcome {
                            RunOutcome::Cancelled => "cancelled",
                            // Named for what it was: the turn ended with work
                            // outstanding, so the next prompt would stack on
                            // top of it.
                            RunOutcome::StoppedShort => "stopped before finishing",
                            _ => "failed",
                        }
                    ));
                }
                return;
            }
            // Success: the drained item (if any) completed — spent.
            inflight.borrow_mut().take();
        }
        if paused || queue_len == 0 || wait_pending {
            return;
        }
        if scheduled.get() {
            return;
        }
        scheduled.set(true);
        let ctx = ctx.clone();
        let scheduled = scheduled.clone();
        let inflight = inflight.clone();
        after(Duration::ZERO, move || {
            scheduled.set(false);
            // Re-check every condition UNTRACKED: the world can move
            // between the effect and this job (a manual send, a resume,
            // a late wait).
            if store.phase.get_untracked() != Phase::Idle
                || store.queue_paused.get_untracked()
                || store.fold.with_untracked(|f| f.pending_wait.is_some())
            {
                return;
            }
            // Workflow readiness BEFORE dequeuing (cycle-2 guard): an
            // unready start_run returns without a phase change — an
            // unchecked drain would stall silently armed or lose the
            // popped item. Refusal pauses with the item KEPT.
            if store.workflow.with_untracked(|w| w.flow_id.is_empty()) {
                store.queue_paused.set(true);
                store.notify(
                    "queue paused — no agent workflow selected (/workflow, then /queue → r)",
                );
                return;
            }
            let Some(item) = store.queue.with_untracked(|q| q.first().cloned()) else {
                return;
            };
            let left = store.queue.with_untracked(|q| q.len()) - 1;
            store.queue.update(|q| q.retain(|p| p.id != item.id));
            if left > 0 {
                store.notify(format!("queue: starting next prompt ({left} left)"));
            } else {
                store.notify("queue: starting next prompt");
            }
            let armed_root = store.fold.with_untracked(|f| f.root_run_id().to_string());
            *inflight.borrow_mut() = Some((item.clone(), armed_root));
            start_run(store, &ctx, &item.text);
            // A SYNCHRONOUS refusal (dead worker channel) leaves the
            // phase Idle: restore the item at head + pause — nothing was
            // spent, `r` retries it (cycle-2: was popped-and-lost).
            if store.phase.get_untracked() == Phase::Idle {
                inflight.borrow_mut().take();
                store.queue.update(|q| q.insert(0, item));
                if !store.queue_paused.get_untracked() {
                    store.queue_paused.set(true);
                    store.notify(
                        "queue paused — the start was refused (item kept; fix it, then /queue → r)",
                    );
                }
            }
        });
    });
}

/// Deliver buffered guidance (`pending_steer`) on the NEW TREE's FIRST
/// REASON-CYCLE record — never on run_id/phase alone (plan item 1,
/// cycle-2): the runner's start-success closure writes run_id → phase →
/// `begin_run` in that order and signal writes flush effects
/// synchronously, so a phase-keyed delivery reads the PREVIOUS run's
/// fold; and a root-targeted steer is silently never folded on wrapper
/// bundles (the agent loop drains guidance in a SUBRUN).
///
/// Identity predicate: armed-while-Starting delivers only once
/// `root_run_id() != armed_at_root` (the new run began — `begin_run`
/// cleared the cycling target in between, so a stale old-run cycle can
/// never satisfy this); armed-while-Running delivers only while
/// `root_run_id() == armed_at_root` (the run it was meant for).
///
/// Disposal is explicit and visible: start failed (Idle, root unchanged
/// from a Starting-armed buffer) → Error card carrying the text; run
/// over before any cycle → Info card ("resend if still relevant").
/// `/new` + session switches clear with an echo (`swap_queue_for_session`);
/// quit drops it silently (moment-bound guidance, unlike the queue).
pub(crate) fn wire_pending_steer(cx: Scope, store: Store, ctx: UiCtx) {
    cx.effect(move || {
        let phase = store.phase.get();
        let Some(ps) = store.pending_steer.get() else {
            return;
        };
        let (root, cycling, finished) = store
            .fold
            .with(|f| (f.root_run_id().to_string(), f.cycling_target(), f.finished));
        let identity_ok = if ps.armed_while_starting {
            root != ps.armed_at_root
        } else {
            root == ps.armed_at_root
        };
        if let Some(target) = cycling {
            // `!finished` matters: the finished_now closure writes the
            // outcome mailbox BEFORE the phase flip, and its flush runs
            // this effect while phase still reads Running — delivering
            // into a finished run would vanish silently.
            if identity_ok && phase == Phase::Running && !finished {
                store.pending_steer.set(None);
                send_steer_to(store, &ctx, &target, &ps.text);
                return;
            }
        }
        if phase == Phase::Idle {
            store.pending_steer.set(None);
            if ps.armed_while_starting && root == ps.armed_at_root {
                // No new run ever began: the start failed.
                store.fold.update(|f| {
                    f.push_item(Item::Error {
                        text: format!(
                            "guidance not delivered — the run did not start. Your text: {}",
                            ps.text
                        ),
                    })
                });
            } else {
                store.fold.update(|f| {
                    f.push_item(Item::Info {
                        text: format!(
                            "steer arrived after the run finished — resend if still relevant: {}",
                            ps.text
                        ),
                    })
                });
            }
        }
    });
}
