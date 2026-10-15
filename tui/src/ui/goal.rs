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
use crate::ui::{agent_start_opts, queue_preview, send_start, UiCtx};

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
        store.notify("this session already has a goal — /goal shows it, /goal stop cancels it");
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
    // prompt mirrors the goal text (ledger/user-card readability).
    // Continuity is the server seed's job, as for every run.
    let mut opts = agent_start_opts(store, ctx);
    let max_cycles = ctx.prefs.borrow().goal_cycles();
    opts.goal = Some((text.to_string(), max_cycles));
    // The goal bundle needs an EXPLICIT tool list. `goal-agent@0.0.1`'s
    // `worker` node has a CONNECTED `tools` pin and no `pinDefaults.tools`,
    // so the runtime's "was tools specified?" test
    // (`abstractruntime/.../visual/executor.py:3537` — `"tools" in
    // input_data`) sees the key present with a null value, normalizes it to
    // `[]`, and skips the node-default fallback. For every other workflow
    // `tools: None` correctly means "use the flow's own defaults"; here it
    // means the worker cycles with ZERO tools and can never do or verify
    // real work. Send the enabled inventory so "untouched /tools" means the
    // same thing to this bundle as to the others.
    if opts.tools.is_none() {
        let grantable = store.grantable_tool_names();
        if !grantable.is_empty() {
            opts.tools = Some(grantable);
        }
    }
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

/// The goal's one-line name: its text, or — for a goal run whose record
/// on the gateway carries none — that it is a goal run.
pub(crate) fn goal_label(text: &str) -> String {
    if text.trim().is_empty() {
        "goal run".to_string()
    } else {
        format!("goal: {}", queue_preview(text))
    }
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
                    goal_label(&g.text),
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
                    "{} — its run is not live here; /goal stop cancels it",
                    goal_label(&g.text)
                ));
            }
        }
    }
}

/// `/goal stop` — durable cancel of the goal run on the gateway. The goal
/// IS that run: once it is cancelled, no client derives a goal from it.
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
}

/// The goal lifecycle effect: binds a pending goal to the run that
/// reaches Running (starts are phase-serialized, so that IS the goal
/// run), owns `fold.finish_on_root_only` (re-derived from run identity —
/// `begin_run` deliberately never touches it: the runner's begin_run post
/// and this effect race, and identity is the only stable truth), clears
/// a pending goal whose start failed, and drops the goal when the goal
/// run's end is OBSERVED (fold followed it to finished). Nothing is kept
/// locally: a session's goal is its live goal run on the gateway, derived
/// by the reattach probe (operator ruling 2026-10-10).
pub(crate) fn wire_goal(cx: Scope, store: Store) {
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
                        store.notify(format!("{} — finished", goal_label(&g.text)));
                    }
                }
            }
        }
    });
}
