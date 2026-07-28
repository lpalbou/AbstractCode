//! The `/goal` lane (plan item 3 — client half; ships DARK until the
//! flow seat publishes a goal bundle: discovery is by catalog interface).
//!
//! Self-contained function family extracted from `ui/mod.rs`
//! (consolidation round-2 P2-1): the `/goal` router, run starter, status
//! report, durable stop, and the `wire_goal` effect that owns the fold's
//! `finish_on_root_only` defense. Root wires `wire_goal`;
//! `dispatch_command` routes `Command::Goal` here — everything else is
//! internal to the lane.

use abstracttui::reactive::Scope;

use crate::runner::Cmd;
use crate::store::{Phase, Store};
use crate::transcript::Item;
use crate::ui::{agent_start_opts, persist_prefs, queue_preview, send_start, UiCtx};

/// `/goal` router: bare = status, the exact word `stop` = durable cancel,
/// anything else = the goal text.
pub(crate) fn dispatch_goal(store: Store, ctx: &UiCtx, arg: Option<String>) {
    match arg {
        None => goal_status(store),
        Some(word) if word.trim().eq_ignore_ascii_case("stop") => stop_goal(store, ctx),
        Some(text) => start_goal_run(store, ctx, text.trim()),
    }
}

fn start_goal_run(store: Store, ctx: &UiCtx, text: &str) {
    if text.is_empty() {
        store.notify("usage: /goal <goal text> · /goal stop cancels");
        return;
    }
    if store.phase.get_untracked() != Phase::Idle {
        store.notify("a run is active — /goal after it finishes (Esc Esc cancels it)");
        return;
    }
    if store.goal.get_untracked().is_some() {
        store.notify("a goal is already recorded — /goal shows it, /goal stop clears it");
        return;
    }
    // Discovery by interface (abstractcode.goal.v1): the honest dark
    // notice while the flow seat hasn't published the bundle; the feature
    // lights up on catalog load the moment one appears.
    let workflow = store.goal_workflows.with_untracked(|w| w.first().cloned());
    let Some(workflow) = workflow else {
        store.notify(
            "no goal workflows on this gateway (abstractcode.goal.v1) — the goal bundle is not published yet",
        );
        return;
    };
    // Goal contract input: {goal, max_cycles, use_session_history} — the
    // prompt mirrors the goal text (ledger/user-card readability). No
    // client transcript messages: continuity is the server seed's job
    // for a bundle whose input contract we don't own.
    let mut opts = agent_start_opts(store, ctx, Vec::new());
    let max_cycles = ctx.prefs.borrow().goal_cycles();
    opts.goal = Some((text.to_string(), max_cycles));
    store.fold.update(|f| {
        f.push_item(Item::Info {
            text: format!(
                "goal run ({}) — loops until verified done or {max_cycles} cycles; /goal stop cancels",
                workflow.label()
            ),
        })
    });
    // Chips never ride goal runs (v1) — say so while they wait.
    crate::ui::attachments::note_kept_for_goal(store);
    send_start(store, ctx, workflow, text, opts, Vec::new());
    if store.phase.get_untracked() == Phase::Idle {
        return; // synchronous refusal (dead worker): nothing to track
    }
    // Arm AFTER the phase flipped to Starting: `wire_goal` reads a
    // pending goal (empty run_id) at phase Idle as "the start failed".
    store.goal.set(Some(crate::store::GoalState {
        text: text.to_string(),
        run_id: String::new(),
    }));
}

fn goal_status(store: Store) {
    match store.goal.get_untracked() {
        None => store.notify(
            "no active goal — /goal <text> starts one (needs a goal workflow: abstractcode.goal.v1)",
        ),
        Some(g) => {
            let live = !g.run_id.is_empty()
                && store.run_id.get_untracked() == g.run_id
                && store.phase.get_untracked() != Phase::Idle;
            if live {
                // Token part: the strip's shared rule — `fmt_tokens` for the
                // numbers (this was a third hand-rolled copy: raw `12000↑`
                // where every sibling says `12k↑`) and render-when-known
                // (all-zero totals omit the part; "0 tk" before the first
                // receipt claimed a measurement that never happened).
                use crate::ui::chrome::fmt_tokens;
                let (cycle, stats_line) = store.fold.with_untracked(|f| {
                    let tokens = if f.stats.input_tokens > 0 || f.stats.output_tokens > 0 {
                        format!(
                            "{}↑ {}↓ tk",
                            fmt_tokens(f.stats.input_tokens),
                            fmt_tokens(f.stats.output_tokens)
                        )
                    } else if f.stats.total_tokens > 0 {
                        // Splitless-usage providers report only totals.
                        format!("{} tk", fmt_tokens(f.stats.total_tokens))
                    } else {
                        String::new()
                    };
                    (f.cycle, tokens)
                });
                let mut parts = vec![
                    format!("goal: {}", queue_preview(&g.text)),
                    format!("cycle {cycle}"),
                    crate::convo::fmt_elapsed(store.elapsed_secs.get_untracked()),
                ];
                if !stats_line.is_empty() {
                    parts.push(stats_line);
                }
                parts.push("/goal stop cancels".into());
                store.notify(parts.join(" · "));
            } else if g.run_id.is_empty() {
                store.notify(format!("goal starting: {}", queue_preview(&g.text)));
            } else {
                store.notify(format!(
                    "goal recorded ({}) but its run is not live here — /goal stop clears it",
                    queue_preview(&g.text)
                ));
            }
        }
    }
}

/// `/goal stop` — durable cancel of the goal run + slot clear. Also the
/// escape hatch for a stale recorded goal whose run died while the app
/// was away (the clear-on-end effect only fires for OBSERVED ends).
fn stop_goal(store: Store, ctx: &UiCtx) {
    let Some(g) = store.goal.get_untracked() else {
        store.notify("no active goal to stop");
        return;
    };
    if !g.run_id.is_empty() {
        ctx.send(Cmd::Cancel {
            run_id: g.run_id.clone(),
        });
        store.notify("goal stopped — cancel sent (durable on the gateway)");
    } else {
        store.notify("goal cleared");
    }
    store.goal.set(None);
    let sid = store.session_id.get_untracked();
    persist_prefs(ctx, |p| p.set_session_goal(&sid, None));
}

/// The goal lifecycle effect: binds a pending goal to the run that
/// reaches Running (starts are phase-serialized, so that IS the goal
/// run), owns `fold.finish_on_root_only` (re-derived from run identity —
/// `begin_run` deliberately never touches it: the runner's begin_run post
/// and this effect race, and identity is the only stable truth), clears
/// a pending goal whose start failed, and retires the slot when the goal
/// run's end is OBSERVED (fold followed it to finished — a restored slot
/// at boot survives until the reattach probe decides).
pub(crate) fn wire_goal(cx: Scope, store: Store, ctx: UiCtx) {
    cx.effect(move || {
        let rid = store.run_id.get();
        let phase = store.phase.get();
        let goal = store.goal.get();
        let set_flag = |want: bool| {
            // Change-guarded: a plain fold.update notifies every fold
            // subscriber (feed sync included) — never write a no-op.
            let current = store.fold.with_untracked(|f| f.finish_on_root_only);
            if current != want {
                store.fold.update(|f| f.finish_on_root_only = want);
            }
        };
        match goal {
            None => set_flag(false),
            Some(g) if g.run_id.is_empty() => {
                if phase == Phase::Running && !rid.is_empty() {
                    let sid = store.session_id.get_untracked();
                    persist_prefs(&ctx, |p| {
                        p.set_session_goal(&sid, Some((g.text.clone(), rid.clone())))
                    });
                    store.goal.set(Some(crate::store::GoalState {
                        text: g.text.clone(),
                        run_id: rid.clone(),
                    }));
                    set_flag(true);
                } else if phase == Phase::Idle {
                    // The start failed before any run began (the runner's
                    // Err post) — the error card is already on screen.
                    store.goal.set(None);
                    set_flag(false);
                    store.notify("goal did not start — see the error above");
                }
            }
            Some(g) => {
                let is_goal_run = g.run_id == rid;
                set_flag(is_goal_run);
                if is_goal_run && phase == Phase::Idle {
                    let observed_end = store
                        .fold
                        .with_untracked(|f| f.root_run_id() == g.run_id && f.finished);
                    if observed_end {
                        store.goal.set(None);
                        let sid = store.session_id.get_untracked();
                        persist_prefs(&ctx, |p| p.set_session_goal(&sid, None));
                        store.notify(format!("goal run finished: {}", queue_preview(&g.text)));
                    }
                }
            }
        }
    });
}
