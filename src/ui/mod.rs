//! Root view composition + orchestration (timers, toasts, modals).

pub mod chrome;
pub mod modals;
pub mod transcript_view;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use abstracttui::app::{current_viewport, Overlays};
use abstracttui::prelude::*;
use abstracttui::reactive::after;

use crate::commands::{self, Command};
use crate::config::Prefs;
use crate::run_input::StartOpts;
use crate::runner::Cmd;
use crate::store::{Phase, Store};
use crate::transcript::{Item, PendingWait, WaitKind};

/// Rows consumed by fixed chrome at its MINIMUM: header 1 + activity 1 +
/// composer 1 (borderless TextArea at one row) + status 1. The transcript
/// pane gets the rest. Deliberately the minimum, not the maximum: the
/// composer can grow to 4 rows, making the real pane SMALLER than this
/// estimate — and both consumers err benignly in that direction. The
/// PgDn bottom check (`page`) computes a max offset ≤ the true one, so
/// it can only re-arm follow EARLY (benign jump to tail), never fail to
/// re-stick at the true bottom. The shrink clamp snaps a stranded
/// offset to that same ≤-true maximum — always a valid in-content
/// position (at worst a few rows above the true bottom, one wheel tick
/// away), never past the content, so the pane can never clamp blank.
pub const CHROME_ROWS: i32 = 4;

#[derive(Clone)]
pub struct UiCtx {
    pub tx: Sender<Cmd>,
    pub overlays: Overlays,
    pub quitter: Quitter,
    pub prefs: Rc<RefCell<Prefs>>,
    pub workspace_root: Option<String>,
    pub workspace_mode: Option<String>,
    pub max_iterations: u32,
    /// Prior turns replayed in full detail on session attach.
    pub replay_turns: usize,
    /// Short host label for the status bar (e.g. "127.0.0.1:8080").
    pub gateway_label: String,
    /// One modal at a time; opening a new one closes the old.
    pub modal: Rc<RefCell<Option<Modal>>>,
    /// Bumped on every modal open/close so effects that must re-evaluate
    /// when the modal SLOT changes (not just when the pending wait changes)
    /// have a signal to track — e.g. reopening an approval prompt after an
    /// unrelated modal (/tools) closed over it (live finding: a pending
    /// approval with no modal and no way back).
    pub modal_epoch: Signal<u64>,
    /// A pending wait the user explicitly dismissed (step_id): the modal
    /// stays closed until they reopen it (Enter on an empty composer).
    pub dismissed_wait: Rc<RefCell<Option<String>>>,
    /// The step_id of the wait whose modal is CURRENTLY open, if any —
    /// distinguishes "the approval prompt is up" from "some picker is up"
    /// (a picker opened over the prompt replaces it; the prompt must come
    /// back when the picker closes).
    pub wait_modal_for: Rc<RefCell<Option<String>>>,
}

impl UiCtx {
    /// Retire a modal taken out of the slot: the LAYER goes away NOW
    /// (pixels + input routing — layer removal touches no reactive state,
    /// so it is safe inside the modal's own widget callbacks), while the
    /// SCOPE disposal stays deferred one tick.
    ///
    /// Why the deferral survives 0.2.0 (review correction): engine
    /// backlog 0250 DID land in 0.2.0 — `List`/`Table` `select` now
    /// complete all widget bookkeeping BEFORE the user callback, so the
    /// originally-documented hazard (List's post-`on_select`
    /// `offset.update` on a disposed scope) is gone. But the
    /// disposal-safety law is NOT engine-wide: `Button`'s mouse path
    /// still writes its own `pressed` signal AFTER `on_click` returns
    /// (button.rs `MouseKind::Up`: `fire(); pressed.set(false)`), so a
    /// synchronous modal close from a mouse-clicked approve/deny button
    /// would still die with "handle used after its node was disposed".
    /// Delete only when EVERY widget callback that can close a modal is
    /// disposal-safe.
    ///
    /// The split matters (live 2026-07-21, /model stage 2): a deferred
    /// LAYER removal left the replaced modal alive-for-input a full tick
    /// after its successor opened. Two modal layers at the same z
    /// dispatch to the OLDEST but paint the NEWEST — every key aimed at
    /// the visible new modal landed on the invisible dead one.
    fn retire(&self, m: Modal) {
        m.layer().remove();
        after(Duration::ZERO, move || m.close());
    }

    pub fn close_modal(&self) {
        // Take FIRST in its own statement: an if-let on `borrow_mut().take()`
        // keeps the RefMut alive through the body, and the epoch bump below
        // synchronously re-runs effects that read `self.modal` (BorrowError).
        let taken = self.modal.borrow_mut().take();
        if let Some(m) = taken {
            self.retire(m);
            // Input routing returns to the main tree, where the composer
            // holds focus (autofocus keeps it current across rebuilds).
            self.wait_modal_for.borrow_mut().take();
            self.modal_epoch.update(|e| *e = e.wrapping_add(1));
        }
    }

    /// True while a modal is open (any kind).
    pub fn modal_open(&self) -> bool {
        self.modal.borrow().is_some()
    }

    /// Open a modal, replacing any current one ATOMICALLY: the slot never
    /// reads empty to reactive observers mid-replacement, and the epoch
    /// bumps exactly once, AFTER the slot holds the new modal.
    ///
    /// Deliberately NOT `close_modal()` + open: the close-half's epoch
    /// bump runs `maybe_flush` synchronously when called outside a
    /// dispatch batch (e.g. from a timer job like the deferred /model
    /// stage-2 open). `wire_wait_modals` then observed "pending wait +
    /// no modal" in the gap, opened its prompt re-entrantly, and the
    /// outer open overwrote the slot — dropping the prompt's `Modal`
    /// handle WITHOUT closing it (drop does not close). The leaked layer
    /// swallowed every key while the new modal painted over it: a
    /// visible, dead picker (live 2026-07-21, /model stage 2).
    pub fn open_modal(&self, cx: Scope, size: Size, build: impl FnOnce(Scope) -> View) {
        // Take in its own statement (RefMut must not live across retire).
        let replaced = self.modal.borrow_mut().take();
        if let Some(old) = replaced {
            self.retire(old);
            // The marker belongs to the retired prompt, if any; the
            // incoming modal's opener re-sets it after this returns.
            // No focus_composer: focus belongs to the incoming modal.
            self.wait_modal_for.borrow_mut().take();
        }
        let modal = Modal::open(&self.overlays, cx, current_viewport(), size, build);
        *self.modal.borrow_mut() = Some(modal);
        self.modal_epoch.update(|e| *e = e.wrapping_add(1));
    }

    /// Send a command to the runner. Returns false when the command
    /// loop is DEAD (runner-thread panic): the command went nowhere,
    /// and callers that flipped optimistic state (phase, cleared waits)
    /// must revert it. Most call sites can ignore the return — a lost
    /// pause/cancel on a dead loop changes nothing the panic notice
    /// didn't already say.
    pub fn send(&self, cmd: Cmd) -> bool {
        self.tx.send(cmd).is_ok()
    }
}

pub fn root(cx: Scope, store: Store, ctx: UiCtx) -> View {
    let theme = use_theme(cx);
    let spin = cx.signal(0u64);
    // Engine follow-tail (0.2.0): the Scroll pins to the bottom while
    // `follow` is true and its own gestures (wheel) manage disengage /
    // re-arm; the key shortcuts below write the same signal.
    let scroll_offset = cx.signal(0i32);
    let follow = cx.signal(true);
    let feed = abstracttui::widgets::FeedState::new(cx);

    spawn_run_ticker(cx, store, spin);
    spawn_probe_ticker(cx, store, ctx.tx.clone());
    wire_toasts(cx, store, ctx.overlays.clone());
    wire_startup_notices(cx, store);
    wire_wait_modals(cx, store, ctx.clone());
    transcript_view::wire_feed(cx, store, &feed);

    // Durable composer state (draft, caret, input history): lives in root
    // scope so theme rebuilds of the TextArea keep everything.
    let composer = abstracttui::widgets::TextAreaState::new(cx);

    let on_submit = {
        let ctx = ctx.clone();
        move |text: &str| {
            follow.set(true); // sending jumps back to the tail
            submit(cx, store, &ctx, text)
        }
    };

    let root_ctx = ctx.clone();
    let quit = ctx.quitter.clone();
    let esc_ctx = ctx.clone();
    let esc_composer = composer.clone();

    // Shrink clamp: a feed REBUILD can shrink the content far below a
    // scrolled-up offset (details toggled off folds every finished tool
    // + thinking card; a session switch replaces the fold wholesale).
    // Scroll never clamps a bound external offset signal, and with
    // follow disengaged nothing else re-positions — the pane rendered
    // NOTHING until a wheel/Esc rescue (review finding, test-pinned:
    // `details_shrink_while_scrolled_up_never_blanks_the_pane`). Track
    // the extent; when the offset ends up beyond it, snap to the new
    // bottom. Growth never triggers this (max_off only grows), so live
    // streaming never fights a reading user.
    {
        let feed = feed.clone();
        cx.effect(move || {
            let total = feed.total_rows().get();
            if follow.get_untracked() {
                return; // the engine's follow pin owns the offset
            }
            let vp = current_viewport();
            let pane_h = (vp.h - CHROME_ROWS).max(3);
            let max_off = (total - pane_h).max(0);
            if scroll_offset.get_untracked() > max_off {
                scroll_offset.set(max_off);
            }
        });
    }

    // Page-jump helper: land at the tail -> re-arm follow (the engine
    // re-arms on its OWN gestures only; external offset writes decide
    // for themselves — feed.total_rows() replaces the old measure math).
    let page = {
        let feed = feed.clone();
        move |delta: i32| {
            if delta < 0 {
                follow.set(false);
                scroll_offset.update(|o| *o = (*o + delta).max(0));
                return;
            }
            let vp = current_viewport();
            let pane_h = (vp.h - CHROME_ROWS).max(3);
            let max_off = (feed.total_rows().get_untracked() - pane_h).max(0);
            let next = (scroll_offset.get_untracked() + delta).min(max_off);
            scroll_offset.set(next);
            if next >= max_off {
                follow.set(true);
            }
        }
    };

    Element::new()
        .style(LayoutStyle::column())
        .shortcut(KeyChord::new(Mods::CTRL, Key::Char('t')), move |_| {
            cycle_theme(&root_ctx);
        })
        .shortcut(KeyChord::new(Mods::CTRL, Key::Char('d')), {
            let ctx = ctx.clone();
            move |_| toggle_details(store, &ctx)
        })
        .shortcut(KeyChord::new(Mods::CTRL, Key::Char('q')), move |_| {
            quit.quit()
        })
        .shortcut(KeyChord::plain(Key::Escape), {
            move |_| handle_escape(cx, store, &esc_ctx, &esc_composer, follow)
        })
        .shortcut(KeyChord::plain(Key::PageUp), {
            let page = page.clone();
            move |_| page(-10)
        })
        .shortcut(KeyChord::plain(Key::PageDown), move |_| page(10))
        .child(dyn_view_scoped(LayoutStyle::column().grow(1.0), {
            let ctx = ctx.clone();
            let feed = feed.clone();
            let overlays = ctx.overlays.clone();
            move |scx| {
                let t = theme.get().tokens;
                Element::new()
                    // Fill the viewport: a content-hugging column floats the
                    // status bar mid-screen on first launch (live finding).
                    .style(
                        LayoutStyle::column()
                            .grow(1.0)
                            .width(Dimension::Percent(1.0))
                            .height(Dimension::Percent(1.0)),
                    )
                    .child(chrome::header(&t, store))
                    .child(transcript_view::pane(
                        scx,
                        &t,
                        store,
                        &feed,
                        scroll_offset,
                        follow,
                    ))
                    .child(chrome::activity_strip(&t, store, spin))
                    // In-flow composer: grows 1..4 rows with the draft (the
                    // absolute-position + spacer trick existed only for the
                    // pre-0.2.0 focus_first policy).
                    .child(chrome::composer(
                        scx,
                        &t,
                        store,
                        &composer,
                        &overlays,
                        on_submit.clone(),
                    ))
                    .child(chrome::status_bar(&t, store, &ctx))
                    .build()
            }
        }))
        .build()
}

fn submit(cx: Scope, store: Store, ctx: &UiCtx, text: &str) {
    let text = text.trim().to_string();
    if text.is_empty() {
        // Enter on an empty composer reopens a dismissed wait prompt.
        let pending = store.fold.with_untracked(|f| f.pending_wait.clone());
        if let Some(wait) = pending {
            ctx.dismissed_wait.borrow_mut().take();
            open_wait_modal(cx, store, ctx, wait);
        }
        return;
    }
    match commands::parse(&text) {
        None => {
            let phase = store.phase.get_untracked();
            match phase {
                Phase::Running => steer(store, ctx, &text),
                Phase::Starting => {
                    // The new run id has not landed yet — a steer now would
                    // target the PREVIOUS run. Refuse honestly.
                    store.notify("run is still starting — resend the guidance in a moment");
                }
                Phase::Idle => start_run(store, ctx, &text),
            }
        }
        Some(cmd) => dispatch_command(cx, store, ctx, cmd),
    }
}

fn start_run(store: Store, ctx: &UiCtx, prompt: &str) {
    let workflow = store.workflow.get_untracked();
    if workflow.flow_id.is_empty() {
        let has_any = store.workflows.with_untracked(|w| !w.is_empty());
        if has_any {
            store.notify("pick an agent workflow first (/workflow)");
        } else {
            store.notify(
                "no agent workflows on this gateway (abstractcode.agent.v1) — install basic-agent, then /workflow",
            );
        }
        return;
    }
    let session_id = store.session_id.get_untracked();
    // Conversation context BEFORE this turn's user card lands (whole
    // completed turns only; caps mirror the server seed defaults). Client
    // messages win over the server seed — needed live because wrapper
    // bundles can leave prior roots non-completed (helper pollers),
    // starving the server-side session replay.
    let messages = store.fold.with_untracked(|f| f.chat_messages(40, 24_000));
    store.fold.update(|f| {
        f.push_item(Item::User {
            text: prompt.to_string(),
        })
    });
    store.phase.set(Phase::Starting);
    // The first prompt names the session in the /sessions picker.
    persist_prefs(ctx, |p| p.touch_session(&session_id, Some(prompt)));
    // Tool selection: untouched = the workflow's own defaults (send
    // nothing); customized = the checked set is the run's exact allowlist.
    // Only disabled names that EXIST in the inventory count — a stale name
    // from another gateway must not silently flip the run into explicit-
    // allowlist mode (adversary finding 6).
    let disabled = store.disabled_tools.get_untracked();
    let inventory = store.tools.get_untracked();
    let effective_disabled = disabled
        .iter()
        .filter(|d| inventory.iter().any(|t| t.name == **d))
        .count();
    let tools = if effective_disabled == 0 {
        None
    } else if inventory.is_empty() {
        store.notify("tool selection not applied: inventory not loaded yet (/tools)");
        None
    } else {
        Some(
            inventory
                .iter()
                .map(|t| t.name.clone())
                .filter(|n| !disabled.contains(n))
                .collect::<Vec<_>>(),
        )
    };
    let opts = StartOpts {
        provider: store.provider.get_untracked(),
        model: store.model.get_untracked(),
        workspace_root: ctx.workspace_root.clone(),
        workspace_mode: ctx.workspace_mode.clone(),
        max_iterations: ctx.max_iterations,
        system: String::new(),
        messages,
        tools,
        skills: store.selected_skills.get_untracked(),
    };
    let delivered = ctx.send(Cmd::Start {
        prompt: prompt.to_string(),
        flow_id: workflow.flow_id,
        bundle_id: workflow.bundle_id,
        session_id,
        opts: Box::new(opts),
    });
    if !delivered {
        // The runner's command loop is dead (worker panic): the start
        // went nowhere, so Starting would spin forever — the exact lie
        // the panic handler's Idle reset exists to prevent. The user
        // card stays (they said it); the phase tells the truth.
        store.phase.set(Phase::Idle);
        store.fold.update(|f| {
            f.push_item(Item::Error {
                text: "run not started: the gateway worker is dead — restart the app".into(),
            })
        });
    }
}

fn steer(store: Store, ctx: &UiCtx, text: &str) {
    let target = store.fold.with_untracked(|f| f.steer_target());
    if target.is_empty() {
        store.notify("no active run to steer");
        return;
    }
    store.fold.update(|f| {
        f.push_item(Item::Steer {
            text: text.to_string(),
        })
    });
    ctx.send(Cmd::Steer {
        run_id: target,
        text: text.to_string(),
    });
}

fn dispatch_command(cx: Scope, store: Store, ctx: &UiCtx, cmd: Command) {
    match cmd {
        Command::Help => modals::open_help(cx, ctx),
        Command::Quit => ctx.quitter.quit(),
        Command::NewSession => new_session(store, ctx),
        Command::Theme(Some(id)) => {
            if set_theme_by_id(&id) {
                save_theme_pref(ctx, &id);
                store.notify(format!("theme: {id}"));
                // The composer dyn rebuild re-fires its autofocus (0.2.0).
            } else {
                store.notify(format!("unknown theme: {id} (try /theme)"));
            }
        }
        Command::Theme(None) => modals::open_theme_picker(cx, store, ctx),
        Command::Workflow => modals::open_workflow_picker(cx, store, ctx),
        Command::Model => modals::open_model_picker(cx, store, ctx),
        Command::Tools => {
            ctx.send(Cmd::LoadTools);
            modals::open_tools(cx, store, ctx);
        }
        Command::Skills => {
            ctx.send(Cmd::LoadSkills);
            modals::open_skills(cx, store, ctx);
        }
        Command::Mcp => {
            ctx.send(Cmd::LoadMcp);
            modals::open_mcp(cx, store, ctx);
        }
        Command::Cache => {
            ctx.send(Cmd::LoadCacheInfo {
                provider: store.provider.get_untracked(),
                model: store.model.get_untracked(),
            });
            modals::open_cache(cx, store, ctx);
        }
        Command::Sessions => modals::open_sessions(cx, store, ctx),
        Command::Session(None) => {
            let sid = store.session_id.get_untracked();
            store.notify(format!("session: {sid} (/sessions lists recent ones)"));
        }
        Command::Session(Some(id)) => switch_session(store, ctx, &id),
        Command::Cancel => cancel_run(store, ctx),
        Command::Pause => {
            let run_id = store.run_id.get_untracked();
            if run_id.is_empty() || store.phase.get_untracked() == Phase::Idle {
                store.notify("no active run to pause");
            } else if store.paused.get_untracked() {
                store.notify("already paused — /resume continues");
            } else {
                ctx.send(Cmd::Pause { run_id });
            }
        }
        Command::Resume => {
            let run_id = store.run_id.get_untracked();
            if run_id.is_empty() || !store.paused.get_untracked() {
                store.notify("no paused run — /pause pauses the active one");
            } else {
                ctx.send(Cmd::ResumePaused { run_id });
            }
        }
        Command::Details => toggle_details(store, ctx),
        Command::AutoApprove => {
            let now = !store.auto_approve.get_untracked();
            store.auto_approve.set(now);
            store.notify(if now {
                "auto-approve ON — tool batches resume without prompting (this session only)"
            } else {
                "auto-approve OFF — tool batches prompt again"
            });
        }
        Command::Steer(text) => {
            if text.is_empty() {
                store.notify("usage: /steer <guidance>");
            } else {
                steer(store, ctx, &text);
            }
        }
        Command::Unknown(head) => {
            store.notify(format!("unknown command {head} — /help lists commands"));
        }
    }
}

fn new_session(store: Store, ctx: &UiCtx) {
    // A live run keeps executing server-side; cancel it rather than
    // silently orphaning it behind a cleared view.
    if store.phase.get_untracked() != Phase::Idle {
        let run_id = store.run_id.get_untracked();
        if !run_id.is_empty() {
            ctx.send(Cmd::Cancel { run_id });
            store.notify("active run cancelled");
        }
    }
    let sid = crate::config::mint_session_id();
    store.session_id.set(sid.clone());
    persist_prefs(ctx, |p| {
        p.session_id = Some(sid.clone());
        p.touch_session(&sid, None);
    });
    store.fold.update(|f| {
        *f = crate::transcript::Fold::new();
        f.push_item(Item::Info {
            text: format!("new session {sid}"),
        });
    });
    store.totals.set(Default::default());
    store.run_id.set(String::new());
    store.phase.set(Phase::Idle);
    // A blanket approval never crosses a session boundary.
    store.auto_approve.set(false);
    store.paused.set(false);
}

/// Switch to another durable session: transcript restarts locally (history
/// lives on the gateway), a live run of that session is reattached.
pub fn switch_session(store: Store, ctx: &UiCtx, id: &str) {
    let id = id.trim().to_string();
    if id.is_empty() || id == store.session_id.get_untracked() {
        return;
    }
    if store.phase.get_untracked() != Phase::Idle {
        let run_id = store.run_id.get_untracked();
        if !run_id.is_empty() {
            ctx.send(Cmd::Cancel { run_id });
            store.notify("active run cancelled");
        }
    }
    store.session_id.set(id.clone());
    persist_prefs(ctx, |p| {
        p.session_id = Some(id.clone());
        p.touch_session(&id, None);
    });
    store.fold.update(|f| {
        *f = crate::transcript::Fold::new();
        f.push_item(Item::Info {
            text: format!("session switched to {id} — durable memory continues on the gateway"),
        });
    });
    store.totals.set(Default::default());
    store.run_id.set(String::new());
    store.phase.set(Phase::Idle);
    // A blanket approval never crosses a session boundary.
    store.auto_approve.set(false);
    store.paused.set(false);
    ctx.send(Cmd::ProbeAttach {
        session_id: id,
        replay_turns: ctx.replay_turns,
    });
}

/// Show/hide the reasoning detail (thinking blocks + tool result previews).
/// The answers-only view is the "clean" mode; details are one keystroke away.
fn toggle_details(store: Store, ctx: &UiCtx) {
    let now = !store.show_details.get_untracked();
    store.show_details.set(now);
    persist_prefs(ctx, |p| p.show_details = Some(now));
    store.notify(if now {
        "details: shown (reasoning + tool cards + results)"
    } else {
        "details: hidden — clean answers view (active/failed tools stay) — Ctrl+D restores"
    });
}

fn cancel_run(store: Store, ctx: &UiCtx) {
    let run_id = store.run_id.get_untracked();
    if run_id.is_empty() || store.phase.get_untracked() == Phase::Idle {
        store.notify("no active run");
        return;
    }
    ctx.send(Cmd::Cancel { run_id });
}

fn handle_escape(
    _cx: Scope,
    store: Store,
    ctx: &UiCtx,
    composer: &abstracttui::widgets::TextAreaState,
    follow: Signal<bool>,
) {
    if !composer.text().is_empty() {
        composer.clear();
        return;
    }
    follow.set(true); // jump back to the tail
    if store.phase.get_untracked() != Phase::Idle {
        let now = Instant::now();
        let double = store
            .last_esc
            .get_untracked()
            .map(|t| now.duration_since(t) < Duration::from_millis(900))
            .unwrap_or(false);
        if double {
            store.last_esc.set(None);
            cancel_run(store, ctx);
        } else {
            store.last_esc.set(Some(now));
            store.notify("Esc again to cancel the run");
        }
    }
}

fn cycle_theme(ctx: &UiCtx) {
    let themes = abstracttui::theme::themes();
    let current = abstracttui::app::current_theme().id;
    let idx = themes.iter().position(|t| t.id == current).unwrap_or(0);
    let next = &themes[(idx + 1) % themes.len()];
    set_theme_by_id(next.id);
    save_theme_pref(ctx, next.id);
    // The composer dyn rebuild re-fires its autofocus (0.2.0).
}

pub fn save_theme_pref(ctx: &UiCtx, id: &str) {
    persist_prefs(ctx, |p| p.theme = Some(id.to_string()));
}

pub fn persist_prefs(ctx: &UiCtx, mutate: impl FnOnce(&mut Prefs)) {
    let mut prefs = ctx.prefs.borrow_mut();
    mutate(&mut prefs);
    // Best-effort persistence; a read-only home dir must not break the UI.
    let _ = prefs.save();
}

// ---------------------------------------------------------------------------
// Timers + effects
// ---------------------------------------------------------------------------

/// Spinner + elapsed ticker (engine `reactive::interval`, cancellable):
/// alive only while a run is active, so an idle app stays zero-wakeup
/// (the engine's idle guarantee). The phase effect starts/cancels the
/// interval; a suspended terminal coalesces missed ticks (no catch-up).
fn spawn_run_ticker(cx: Scope, store: Store, spin: Signal<u64>) {
    let handle: Rc<RefCell<Option<abstracttui::reactive::IntervalHandle>>> =
        Rc::new(RefCell::new(None));
    cx.effect(move || {
        let active = store.phase.get() != Phase::Idle;
        let mut slot = handle.borrow_mut();
        if active && slot.is_none() {
            *slot = Some(abstracttui::reactive::interval(
                cx,
                Duration::from_millis(120),
                move || {
                    spin.update(|s| *s = s.wrapping_add(1));
                    if let Some(started) = store.run_started.get_untracked() {
                        let secs = started.elapsed().as_secs();
                        if store.elapsed_secs.get_untracked() != secs {
                            store.elapsed_secs.set(secs);
                        }
                    }
                },
            ));
        } else if !active {
            if let Some(h) = slot.take() {
                h.cancel();
            }
        }
    });
}

/// Idle connection probe: one ping every 30s keeps the orb honest without
/// meaningful idle cost; while a run streams, the stream itself is the probe.
fn spawn_probe_ticker(cx: Scope, store: Store, tx: Sender<Cmd>) {
    abstracttui::reactive::interval(cx, Duration::from_secs(30), move || {
        if store.phase.get_untracked() == Phase::Idle {
            let _ = tx.send(Cmd::Probe);
        }
    });
}

/// Surface the engine's startup-notices lane (capability fallbacks; in
/// debug builds also the zero-collapse layout diagnostic) as toasts —
/// unrendered notices only flush to stderr after teardown, which is the
/// one place a developer is no longer looking.
fn wire_startup_notices(cx: Scope, store: Store) {
    let notices = abstracttui::app::use_startup_notices(cx);
    let seen = Rc::new(Cell::new(0usize));
    cx.effect(move || {
        let list = notices.get();
        let start = seen.replace(list.len());
        for notice in list.iter().skip(start) {
            store.notify(format!("engine: {notice}"));
        }
    });
}

/// Drain `store.notices` into toast overlays (seen-counter pattern).
fn wire_toasts(cx: Scope, store: Store, overlays: Overlays) {
    let seen = Rc::new(Cell::new(0usize));
    cx.effect(move || {
        let list = store.notices.get();
        let start = seen.replace(list.len());
        for (slot, notice) in list.iter().skip(start).enumerate() {
            let overlays = overlays.clone();
            let notice = notice.clone();
            after(Duration::from_millis(60 + 400 * slot as u64), move || {
                Toast::show(
                    &overlays,
                    cx,
                    current_viewport(),
                    notice,
                    Duration::from_secs(3),
                );
            });
        }
    });
}

/// Open/close the approval + ask modals as the pending wait OR the modal
/// slot changes. The invariant this maintains (live finding: a pending
/// approval with no modal and no way back): whenever a wait is pending, not
/// explicitly deferred, and no other modal is in the way, its prompt is on
/// screen — including AFTER a picker/help modal that was opened over it
/// closes (the modal_epoch signal re-runs this effect on every open/close).
/// With auto-approve armed ("approve all"), approval waits resume
/// immediately instead of prompting.
fn wire_wait_modals(cx: Scope, store: Store, ctx: UiCtx) {
    // Occurrences already auto-approved once: if one comes BACK (the resume
    // failed and restored the wait), fall through to the modal instead of
    // retrying forever against a refusing gateway.
    let auto_answered: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    cx.effect(move || {
        let pending: Option<PendingWait> = store.fold.with(|f| f.pending_wait.clone());
        let _slot = ctx.modal_epoch.get(); // re-evaluate on modal open/close
        match pending {
            None => {
                // Close only if the open modal IS the wait prompt — a
                // picker the user opened must not be yanked away.
                if ctx.wait_modal_for.borrow().is_some() {
                    ctx.close_modal();
                }
            }
            Some(wait) => {
                if matches!(wait.kind, WaitKind::Approval { .. })
                    && store.auto_approve.get_untracked()
                    && auto_answered.borrow().as_deref() != Some(wait.step_id.as_str())
                {
                    *auto_answered.borrow_mut() = Some(wait.step_id.clone());
                    auto_approve_wait(store, &ctx, &wait);
                    return;
                }
                // Explicitly deferred (Esc): stay closed until Enter reopens.
                if ctx.dismissed_wait.borrow().as_deref() == Some(wait.step_id.as_str()) {
                    return;
                }
                // The prompt for THIS occurrence is already up.
                if ctx.wait_modal_for.borrow().as_deref() == Some(wait.step_id.as_str()) {
                    return;
                }
                // Another modal (picker/help) is up: leave it; its close
                // bumps the epoch and this effect brings the prompt back.
                if ctx.modal_open() {
                    return;
                }
                open_wait_modal(cx, store, &ctx, wait);
            }
        }
    });
}

/// Open the prompt for a pending wait and record WHICH occurrence the open
/// modal belongs to. The bookkeeping write happens AFTER the open — an
/// `open_modal` starts by closing the previous modal, which clears the
/// marker (write-before-open would be undone immediately).
pub fn open_wait_modal(cx: Scope, store: Store, ctx: &UiCtx, wait: PendingWait) {
    match &wait.kind {
        WaitKind::Approval { .. } => modals::open_approval(cx, store, ctx, wait.clone()),
        WaitKind::Ask { .. } => modals::open_ask(cx, store, ctx, wait.clone()),
    }
    *ctx.wait_modal_for.borrow_mut() = Some(wait.step_id);
}

/// Resume an approval wait without prompting (the "approve all" posture).
/// Same optimistic bookkeeping as the modal's approve path; a refused
/// resume restores the wait and the effect falls back to the modal.
fn auto_approve_wait(store: Store, ctx: &UiCtx, wait: &PendingWait) {
    let names: Vec<String> = match &wait.kind {
        WaitKind::Approval { tool_calls } => tool_calls
            .iter()
            .filter_map(|tc| tc.get("name").and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .collect(),
        _ => return,
    };
    store.fold.update(|f| {
        f.wait_answered(&wait.wait_key, &wait.step_id);
        f.mark_wait_tools(&wait.wait_key, true);
    });
    ctx.send(Cmd::Resume {
        run_id: wait.run_id.clone(),
        wait_key: wait.wait_key.clone(),
        payload: serde_json::json!({"approved": true}),
        approved: Some(true),
        restore: Box::new(wait.clone()),
    });
    let summary = if names.is_empty() {
        "tool batch".to_string()
    } else {
        names.join(", ")
    };
    store.notify(format!("auto-approved: {summary} (/auto turns this off)"));
}
