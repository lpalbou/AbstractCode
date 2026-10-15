//! Quit-with-live-run gate (design: untracked/reviews/quit-modal-design.md).
//!
//! The thin-client truth this lane teaches: the agent runs on the
//! GATEWAY — quitting this client never stops it. All three quit
//! gestures (Ctrl+Q, /quit, double-Ctrl+C) funnel through ONE gate:
//! idle quits instantly (byte-identical to before); a live agent run
//! opens the modal — leave running (default), pause-then-quit,
//! cancel-then-quit, Esc stays.
//!
//! The load-bearing refinement (the designer's silent-no-op trap):
//! a quit-time verb must physically LEAVE the dying process. Choosing
//! one spawns a DEDICATED `quit-verb-send` thread (never the worker's
//! sequential command loop, which can hold a verb behind minutes of
//! in-flight HTTP) and enters Delivering: the app quits only on the
//! gateway's ACCEPTANCE (the durable command store's 2xx — never "the
//! run finished pausing"; commands apply at tick boundaries), bounded
//! by an 8s timeout into an honest Failed state (quit-anyway / stay).
//!
//! Repeat quit gestures ALWAYS resolve to the safe verb (leave &
//! quit): hammering Ctrl+C×3 / Ctrl+Q×2 exits at worst one press
//! slower than before, and cancel is never reachable by repetition.
//! Entity visits never gate (ruled: visits PARK on quit — quit ≠
//! goodbye); an active visit gets a mention line when the modal shows
//! for agent reasons. Honest limit: terminal-close/SIGHUP/SIGKILL show
//! no modal — the run continues durably and boot reattach recovers.

use std::cell::Cell;
use std::time::Duration;

use abstracttui::prelude::*;
use abstracttui::reactive::after;
use abstracttui::text;

use crate::store::{Phase, QuitState, QuitVerb, Store};
use crate::ui::UiCtx;

/// Delivery ack window: connect timeout (5s) + margin for the send
/// thread's round-trip (and its one same-id retry). On timeout the
/// Failed state says the command may still land if the user stays —
/// the dedicated thread keeps running and a late ack still quits.
/// `#[WARNING:TIMEOUT]` quit-acknowledgement grace (ADR-0027 §4).
/// NOT a run deadline: the run keeps going on the gateway and a late ack is
/// still honored (`quit.rs` late-ack path). It only bounds how long the UI
/// waits before letting the operator go.
pub(crate) const QUIT_ACK_TIMEOUT: Duration = Duration::from_secs(8);

thread_local! {
    /// Delivery generation: guards the timeout job (a stale timer from
    /// an earlier delivery must never fail a newer one). UI-thread only.
    static QUIT_GEN: Cell<u64> = const { Cell::new(0) };
}

fn next_gen() -> u64 {
    QUIT_GEN.with(|g| {
        g.set(g.get() + 1);
        g.get()
    })
}

/// The ONE quit authority — every gesture lands here.
pub(crate) fn request_quit(cx: Scope, store: Store, ctx: &UiCtx) {
    match store.quit_state.get_untracked() {
        // Repeat gesture / quit-anyway: the safe verb, always exits.
        QuitState::Choosing
        | QuitState::Delivering { .. }
        | QuitState::Acked { .. }
        | QuitState::Failed { .. } => {
            resolve_leave_and_quit(store, ctx);
        }
        QuitState::None => {
            if quit_gates(store.phase.get_untracked()) {
                store.quit_state.set(QuitState::Choosing);
                open_quit(cx, store, ctx);
            } else {
                ctx.quitter.quit();
            }
        }
    }
}

/// Leave running & quit: nothing is sent; the run continues durably and
/// the next launch reattaches. The modal needs no explicit close — the
/// engine restores the terminal on loop exit.
fn resolve_leave_and_quit(store: Store, ctx: &UiCtx) {
    // Freeze the state so the post-teardown echo names the outcome
    // (the lib.rs mirror reads it via the root effect).
    if matches!(store.quit_state.get_untracked(), QuitState::None) {
        store.quit_state.set(QuitState::Choosing);
    }
    ctx.quitter.quit();
}

/// The sequencer: acks resolve Delivering; a run concluding under the
/// open modal makes the question moot (the user already said quit).
pub(crate) fn wire_quit(cx: Scope, store: Store, ctx: &UiCtx) {
    let quitter = ctx.quitter.clone();
    cx.effect(move || {
        let state = store.quit_state.get();
        let phase = store.phase.get();
        let ack = store.verb_ack.get();
        match state {
            QuitState::None | QuitState::Acked { .. } => {}
            QuitState::Failed { verb, run_id, .. } => {
                // A late ack landing AFTER the timeout: honor it — the
                // verb was delivered, the quit intent stands (audit P2:
                // the modal said "not confirmed" while the toast said
                // "paused durably").
                if let Some(a) = ack {
                    if a.verb == verb && a.run_id == run_id && a.ok {
                        store.verb_ack.set(None);
                        store.quit_state.set(QuitState::Acked { verb, run_id });
                        quitter.quit();
                    }
                }
            }
            QuitState::Choosing | QuitState::Delivering { .. } if phase == Phase::Idle => {
                // Moot: the run concluded while the user was deciding.
                // The drain guard (queue_lane) held new work back, so
                // nothing starts under a quitting user.
                quitter.quit();
            }
            QuitState::Choosing => {}
            QuitState::Delivering { verb, run_id, .. } => {
                if let Some(a) = ack {
                    if a.verb == verb && a.run_id == run_id {
                        store.verb_ack.set(None);
                        if a.ok {
                            // Terminal state BEFORE the quit: the
                            // post-teardown echo reads it as "confirmed"
                            // (a resolved delivery must never echo as
                            // not-confirmed).
                            store.quit_state.set(QuitState::Acked { verb, run_id });
                            quitter.quit();
                        } else {
                            store.quit_state.set(QuitState::Failed {
                                verb,
                                run_id,
                                definitive: a.definitive,
                                error: a.error,
                            });
                        }
                    }
                    // Non-matching acks: stale earlier verb — ignored
                    // (same-verb-same-run success is outcome-equivalent;
                    // the failure direction re-prompts, conservative).
                }
            }
        }
    });
}

/// Choice handler: send the verb, enter Delivering, arm the timeout.
fn deliver(store: Store, ctx: &UiCtx, verb: QuitVerb) {
    let run_id = store.run_id.get_untracked();
    if run_id.is_empty() {
        // Starting, not yet bound — the buttons render disabled; belt.
        store.notify("the run has not bound yet — leave, or wait a moment");
        return;
    }
    store.verb_ack.set(None); // clear BEFORE send (UI thread owns both)
    let gen = next_gen();
    store.quit_state.set(QuitState::Delivering {
        verb,
        run_id: run_id.clone(),
        gen,
    });
    // DEDICATED one-shot send (quit-delivery plan v2, operator-
    // validated): the worker's single sequential command loop can hold
    // a quit-time verb behind minutes of in-flight HTTP (image fetches,
    // ~30s/file uploads, history probes) against a HEALTHY gateway —
    // transit collapses to one HTTP round-trip and delivery survives a
    // busy or dead worker. ONE send path: the quit lane never also
    // enqueues on the worker (two sends would mint two command_ids —
    // the store's dedup would NOT collapse them). The command_id is
    // minted HERE and reused by the send authority's one transient
    // retry (exactly-once; runtime receipt c5541).
    let client = ctx.client.clone();
    let wake = abstracttui::reactive::wake_handle();
    let command_id = crate::gateway::mint_command_id();
    let thread_run_id = run_id.clone();
    let spawned = std::thread::Builder::new()
        .name("quit-verb-send".into())
        .spawn(move || {
            crate::runner::send_verb_blocking(
                &client,
                &wake,
                store,
                verb,
                thread_run_id,
                &command_id,
            );
        });
    if spawned.is_err() {
        // OS thread exhaustion — definitive: nothing was sent.
        store.quit_state.set(QuitState::Failed {
            verb,
            run_id,
            definitive: true,
            error: "could not start the send thread — the command was not sent; the run \
                    continues on the gateway"
                .into(),
        });
        return;
    }
    after(QUIT_ACK_TIMEOUT, move || {
        if let QuitState::Delivering {
            gen: g,
            verb,
            run_id,
        } = store.quit_state.get_untracked()
        {
            if g == gen {
                store.quit_state.set(QuitState::Failed {
                    verb,
                    run_id,
                    definitive: false,
                    error: "no confirmation in 8s — the gateway is slow to accept the command"
                        .into(),
                });
            }
        }
    });
}

/// Verb word for titles/copy.
fn verb_word(v: QuitVerb) -> &'static str {
    match v {
        QuitVerb::Pause => "pause",
        QuitVerb::Cancel => "cancel",
    }
}

fn verb_gerund(v: QuitVerb) -> &'static str {
    match v {
        QuitVerb::Pause => "pausing",
        QuitVerb::Cancel => "cancelling",
    }
}

/// The quit modal: one modal, three dyn states (Choosing / Delivering /
/// Failed). Esc always means stay.
pub(crate) fn open_quit(cx: Scope, store: Store, ctx: &UiCtx) {
    let ctx2 = ctx.clone();
    let vp = abstracttui::app::current_viewport();
    let size = Size::new(72.min(vp.w - 4).max(40), 15.min(vp.h - 4).max(10));
    ctx.open_modal(cx, size, move |_mcx| {
        let esc_ctx = ctx2.clone();
        let leave_ctx = ctx2.clone();
        let pause_ctx = ctx2.clone();
        let cancel_ctx = ctx2.clone();
        let quitq = ctx2.quitter.clone();
        Element::new()
            .style(LayoutStyle::column().gap(1).padding(Edges::all(1)))
            .focusable()
            .autofocus()
            .shortcut(KeyChord::plain(Key::Escape), move |_| {
                // Stay: nothing sent (Choosing) or already sent
                // (Delivering — the outcome lands as the normal toast).
                store.quit_state.set(QuitState::None);
                esc_ctx.close_modal();
            })
            .shortcut(KeyChord::plain(Key::Enter), move |_| {
                // Leave & quit (Choosing) / quit anyway (Failed).
                // Delivering: no-op (the sequencer owns the exit).
                match store.quit_state.get_untracked() {
                    QuitState::Delivering { .. } => {}
                    _ => resolve_leave_and_quit(store, &leave_ctx),
                }
            })
            .shortcut(KeyChord::plain(Key::Char('l')), {
                let ctx = ctx2.clone();
                move |_| {
                    if store.quit_state.get_untracked() == QuitState::Choosing {
                        resolve_leave_and_quit(store, &ctx);
                    }
                }
            })
            .shortcut(KeyChord::plain(Key::Char('p')), move |_| {
                let paused = store.paused.get_untracked();
                if matches!(store.quit_state.get_untracked(), QuitState::Choosing) && !paused {
                    deliver(store, &pause_ctx, QuitVerb::Pause);
                }
            })
            .shortcut(KeyChord::plain(Key::Char('c')), move |_| {
                if matches!(store.quit_state.get_untracked(), QuitState::Choosing) {
                    deliver(store, &cancel_ctx, QuitVerb::Cancel);
                }
            })
            // Ctrl+Q inside the modal = repeat gesture = leave/quit-anyway
            // (root shortcuts are shadowed by the modal tree; Ctrl+C
            // reaches request_quit through the global action already).
            .shortcut(KeyChord::new(Mods::CTRL, Key::Char('q')), move |_| {
                let _ = &quitq;
                match store.quit_state.get_untracked() {
                    QuitState::None => {}
                    _ => quitq.quit(),
                }
            })
            .child(dyn_view(
                LayoutStyle::default().grow(1.0).basis(Dimension::Cells(0)),
                move || {
                    let state = store.quit_state.get();
                    let t2 = abstracttui::app::current_theme().tokens;
                    let mut lines: Vec<(String, Rgba)> = Vec::new();
                    let mut push = |s: String, ink: Rgba| lines.push((s, ink));
                    match state {
                        QuitState::None | QuitState::Choosing => {
                            push("a run is still executing".into(), t2.accent);
                            push(String::new(), t2.text);
                            push(
                                "The agent runs on the gateway — quitting this client never stops it"
                                    .into(),
                                t2.text,
                            );
                            push(String::new(), t2.text);
                            // Facts line (dyn: rebuilds as records land).
                            let run_id = store.run_id.get();
                            let short = run_id.get(..8).unwrap_or(run_id.as_str());
                            let paused = store.paused.get();
                            let status = if paused {
                                "paused"
                            } else if store.phase.get() == Phase::Starting {
                                "starting"
                            } else {
                                "running"
                            };
                            let elapsed =
                                crate::convo::fmt_elapsed(store.elapsed_secs.get_untracked());
                            if run_id.is_empty() {
                                push("run starting — not yet bound".into(), t2.text_muted);
                                push(
                                    "pause/cancel become available once the run binds — or leave: it continues either way".into(),
                                    t2.text_faint,
                                );
                            } else {
                                push(
                                    format!("run {short}… · {status} · {elapsed}"),
                                    t2.text_muted,
                                );
                            }
                            if store.goal.get().is_some() {
                                push("goal run — it loops until verified done".into(), t2.text_muted);
                            }
                            if paused {
                                push("it stays paused — /resume after relaunch continues it".into(), t2.text_muted);
                            }
                            let waiting =
                                store.fold.with(|f| f.pending_wait.is_some());
                            if waiting {
                                push(
                                    "waiting on your approval — zero spend while parked".into(),
                                    t2.text_muted,
                                );
                            }
                            let queued = store.queue.with(|q| q.len());
                            if queued > 0 {
                                push(
                                    format!("{queued} queued prompt(s) stay saved — they restore paused on relaunch"),
                                    t2.text_faint,
                                );
                            }
                            let visiting = crate::ui::entity_actions::any_convo_active(store);
                            if visiting {
                                push(
                                    "an entity visit is open — visits park; reopening resumes them".into(),
                                    t2.text_faint,
                                );
                            }
                            push(String::new(), t2.text);
                            let pause_part = if paused || run_id.is_empty() {
                                ""
                            } else {
                                " · p pause, then quit"
                            };
                            let cancel_part = if run_id.is_empty() {
                                ""
                            } else {
                                " · c cancel it, then quit"
                            };
                            push(
                                format!("Enter leave it running & quit{pause_part}{cancel_part} · Esc stay"),
                                t2.text,
                            );
                            push(
                                "relaunching this session reattaches to the run".into(),
                                t2.text_faint,
                            );
                        }
                        QuitState::Delivering { verb, .. } => {
                            push(
                                format!("{} the run on the gateway…", verb_gerund(verb)),
                                t2.accent,
                            );
                            push(String::new(), t2.text);
                            push(
                                "waiting for the gateway to accept the command (up to 8s)".into(),
                                t2.text,
                            );
                            push(String::new(), t2.text);
                            push(
                                "Esc stay (the outcome lands as a notice) · Ctrl+Q quit anyway (abandons the command)"
                                    .into(),
                                t2.text_faint,
                            );
                        }
                        QuitState::Acked { verb, .. } => {
                            push(
                                format!("{} accepted — quitting", verb_word(verb)),
                                t2.ok,
                            );
                        }
                        QuitState::Failed {
                            verb,
                            definitive,
                            error,
                            ..
                        } => {
                            push(format!("{} not confirmed", verb_word(verb)), t2.warn);
                            push(String::new(), t2.text);
                            push(error, t2.text);
                            if definitive {
                                // The gateway answered with an error /
                                // the worker is dead: the command will
                                // NOT land — never claim it might.
                                push(
                                    "the command was not accepted — the run keeps executing on the gateway;".into(),
                                    t2.text_muted,
                                );
                                push(
                                    "stay and retry with /pause or /cancel, or quit and relaunch.".into(),
                                    t2.text_muted,
                                );
                            } else {
                                push(
                                    "the request may still be in flight in this app — staying keeps it alive".into(),
                                    t2.text_muted,
                                );
                                push(
                                    "(a late confirmation still quits); quitting now abandons the attempt.".into(),
                                    t2.text_muted,
                                );
                            }
                            push(String::new(), t2.text);
                            push("Enter quit anyway · Esc stay".into(), t2.text);
                        }
                    }
                    // A column of intrinsic line(1) rows: a draw-only
                    // element under nested grow/basis-0 measures 0 and
                    // never paints (found live — the modal panel showed
                    // blank); per-row elements have intrinsic height.
                    let mut col = Element::new().style(LayoutStyle::column());
                    for (line, ink) in lines {
                        col = col.child(
                            Element::new()
                                .style(LayoutStyle::line(1).shrink(0.0))
                                .draw(move |canvas, rect| {
                                    let fitted =
                                        text::truncate_ellipsis(&line, (rect.w - 2).max(6));
                                    canvas.print(
                                        Point::new(rect.x + 1, rect.y),
                                        &fitted,
                                        ink,
                                        Rgba::TRANSPARENT,
                                    );
                                })
                                .build(),
                        );
                    }
                    col.build()
                },
            ))
            .build()
    });
}

/// Pure gate predicate (test surface): does this quit gesture open the
/// modal? Only a live AGENT run gates — entity visits park by ruling,
/// queued prompts persist per session.
pub(crate) fn quit_gates(phase: Phase) -> bool {
    phase != Phase::Idle
}

/// The post-teardown line telling the user how to come BACK to the
/// conversation they just left (operator ask 2026-10-10: "when we quit, we
/// must show a helper line back in the shell on how to relaunch/resume
/// that session"). `--session <id>` reopens that exact conversation;
/// `gateway` is the `--gateway-url` the user launched with, repeated so
/// the command reaches the same gateway (never the token — a credential
/// does not belong in shell scrollback; the saved login supplies it).
pub fn resume_line(session_id: &str, gateway: Option<&str>) -> Option<String> {
    let sid = session_id.trim();
    if sid.is_empty() {
        return None;
    }
    let gw = gateway
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(|g| format!(" --gateway-url {g}"))
        .unwrap_or_default();
    Some(format!(
        "resume this conversation: abstractcode{gw} --session {sid}"
    ))
}

/// Keep `slot` equal to the conversation that is open NOW — it changes
/// in-session (`/sessions`, `n`) — so the post-teardown [`resume_line`]
/// names the one the user is leaving. Signals die with the reactive root,
/// hence a plain mirror read after `run()` returns.
pub fn mirror_session_id(
    cx: abstracttui::reactive::Scope,
    store: crate::store::Store,
    slot: std::rc::Rc<std::cell::RefCell<String>>,
) {
    cx.effect(move || {
        *slot.borrow_mut() = store.session_id.get();
    });
}

/// Queued prompts per session, `(session id, texts)`: the current
/// session's queue first, then the in-memory stash of the sessions left
/// this run. The queue lives in memory only, so [`unsent_prompt_lines`]
/// lists what was not sent after teardown (same mirror rule as above).
pub type UnsentPrompts = Vec<(String, Vec<String>)>;

pub fn mirror_unsent_prompts(
    cx: abstracttui::reactive::Scope,
    store: crate::store::Store,
    slot: std::rc::Rc<std::cell::RefCell<UnsentPrompts>>,
) {
    cx.effect(move || {
        let current: Vec<String> = store
            .queue
            .with(|q| q.iter().map(|p| p.text.clone()).collect());
        let mut groups: UnsentPrompts = Vec::new();
        if !current.is_empty() {
            groups.push((store.session_id.get(), current));
        }
        store.queue_stash.with(|s| {
            groups.extend(s.iter().filter(|(_, p)| !p.is_empty()).cloned());
        });
        *slot.borrow_mut() = groups;
    });
}

/// The quit listing of prompts that were not sent, whole and by session
/// (each can be resumed with `--session`). Empty when nothing was queued.
pub fn unsent_prompt_lines(groups: &[(String, Vec<String>)]) -> Vec<String> {
    let total: usize = groups.iter().map(|(_, p)| p.len()).sum();
    if total == 0 {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "abstractcode: {total} queued prompt(s) were not sent (the queue lives only while the app runs):"
    )];
    for (session, prompts) in groups {
        lines.push(format!("  session {session}:"));
        lines.extend(prompts.iter().map(|t| format!("    · {t}")));
    }
    lines
}

/// The post-teardown echo line for the outcome the user chose (read by
/// lib.rs AFTER the reactive root died — mirrored continuously there).
pub fn quit_echo_line(state: &QuitState, phase: Phase, run_id: &str) -> Option<String> {
    let short: String = run_id.chars().take(8).collect();
    match state {
        QuitState::None => {
            if phase == Phase::Idle || run_id.is_empty() {
                None
            } else {
                // Quit without the modal resolving a verb (repeat
                // gesture / conclusion race): leave semantics.
                Some(format!(
                    "a run is still executing on the gateway (run {short}…) — relaunching this session reattaches to it"
                ))
            }
        }
        QuitState::Choosing => Some(format!(
            "a run is still executing on the gateway (run {short}…) — relaunching this session reattaches to it"
        )),
        QuitState::Acked { verb, run_id } => Some(acked_echo_line(*verb, run_id)),
        QuitState::Failed {
            verb,
            definitive: true,
            ..
        } => Some(format!(
            "{} was NOT accepted by the gateway — the run keeps executing; relaunch to check (/status)",
            verb_word(*verb)
        )),
        QuitState::Delivering { verb, .. } | QuitState::Failed { verb, .. } => Some(format!(
            "{} was NOT confirmed — it may or may not have reached the gateway; relaunch to check (/status)",
            verb_word(*verb)
        )),
    }
}

/// Acked-verb echo (the sequencer quit on a confirmed pause/cancel).
pub fn acked_echo_line(verb: QuitVerb, run_id: &str) -> String {
    let short: String = run_id.chars().take(8).collect();
    match verb {
        QuitVerb::Pause => format!(
            "pause accepted for run {short}… — it holds durably at the next step boundary; /resume after relaunch continues it"
        ),
        QuitVerb::Cancel => format!(
            "cancel accepted for run {short}… — the gateway applies it at the next step boundary"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_mirror_follows_a_switch() {
        let slot = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let mirror = slot.clone();
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            store.session_id.set("acode-first".into());
            mirror_session_id(cx, store, mirror);
            assert_eq!(*slot.borrow(), "acode-first");
            store.session_id.set("acode-switched".into());
            assert_eq!(*slot.borrow(), "acode-switched", "a switch is followed");
        });
        root.dispose();
        assert_eq!(*slot.borrow(), "acode-switched", "readable after teardown");
    }

    #[test]
    fn unsent_prompts_are_listed_by_session_after_teardown() {
        let slot = std::rc::Rc::new(std::cell::RefCell::new(UnsentPrompts::new()));
        let mirror = slot.clone();
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            store.session_id.set("acode-now".into());
            mirror_unsent_prompts(cx, store, mirror);
            assert!(slot.borrow().is_empty());
            store.queue.update(|q| {
                q.push(crate::store::QueuedPrompt {
                    id: 1,
                    text: "run the suite".into(),
                })
            });
            store
                .queue_stash
                .update(|s| s.push(("acode-left".into(), vec!["write the docs".into()])));
        });
        root.dispose();
        let groups = slot.borrow().clone();
        assert_eq!(
            unsent_prompt_lines(&groups),
            vec![
                "abstractcode: 2 queued prompt(s) were not sent (the queue lives only while the app runs):",
                "  session acode-now:",
                "    · run the suite",
                "  session acode-left:",
                "    · write the docs",
            ]
        );
        assert!(unsent_prompt_lines(&[]).is_empty());
    }

    #[test]
    fn the_resume_line_names_the_session_and_the_explicit_gateway() {
        assert_eq!(
            resume_line("acode-dd819e7205e8", None).as_deref(),
            Some("resume this conversation: abstractcode --session acode-dd819e7205e8")
        );
        assert_eq!(
            resume_line("acode-1", Some("http://10.0.0.2:8080")).as_deref(),
            Some("resume this conversation: abstractcode --gateway-url http://10.0.0.2:8080 --session acode-1")
        );
        assert_eq!(resume_line("  ", None), None);
    }

    #[test]
    fn gate_predicate_and_echo_lines() {
        assert!(!quit_gates(Phase::Idle));
        assert!(quit_gates(Phase::Starting));
        assert!(quit_gates(Phase::Running));
        // Idle + no run: silent quit, no echo.
        assert_eq!(quit_echo_line(&QuitState::None, Phase::Idle, ""), None);
        // Leave with a live run names the reattach story.
        let leave = quit_echo_line(&QuitState::Choosing, Phase::Running, "abcd1234-rest").unwrap();
        assert!(leave.contains("abcd1234") && leave.contains("reattaches"));
        // Quit-anyway from Delivering/Failed is honest about non-delivery.
        let anyway = quit_echo_line(
            &QuitState::Failed {
                verb: QuitVerb::Cancel,
                run_id: "abcd1234".into(),
                definitive: false,
                error: "x".into(),
            },
            Phase::Running,
            "abcd1234",
        )
        .unwrap();
        assert!(anyway.contains("NOT confirmed"));
        assert!(acked_echo_line(QuitVerb::Pause, "abcd1234").contains("holds durably"));
    }
}
