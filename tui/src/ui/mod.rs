//! Root view composition + orchestration (timers, toasts, modals).

pub mod animation;
pub mod approval_view;
pub mod attachments;
pub mod chrome;
pub mod entity_actions;
pub mod entity_modals;
pub mod goal;
pub mod item_menu;
pub mod linkify;
pub mod loading;
pub mod logo;
pub mod modals;
pub mod preview;
pub mod queue_lane;
pub mod queue_modal;
pub mod quit;
pub mod splash;
pub mod stance;
pub mod thinking;
pub mod transcript_view;

use queue_lane::{
    buffer_steer, steer_or_buffer, swap_queue_for_session, wire_pending_steer, wire_queue_drain,
    wire_queue_persistence,
};
pub use queue_lane::{enqueue_prompt, queue_preview, restore_session_queue};

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
use crate::store::{Conn, Phase, Store};
use crate::transcript::{Item, PendingWait, WaitKind};

/// Rows consumed by fixed chrome at its MINIMUM: header 1 + breathing
/// row 1 (the blank line between the transcript's last text and the
/// control panel — operator ask 2026-07-23) + activity 1 + composer 1
/// (borderless TextArea at one row) + status 1. The transcript pane
/// gets the rest. Deliberately the minimum, not the maximum: the
/// composer can grow to 4 rows, making the real pane SMALLER than this
/// estimate — and both consumers err benignly in that direction. The
/// PgDn bottom check (`page`) computes a max offset ≤ the true one, so
/// it can only re-arm follow EARLY (benign jump to tail), never fail to
/// re-stick at the true bottom. The shrink clamp snaps a stranded
/// offset to that same ≤-true maximum — always a valid in-content
/// position (at worst a few rows above the true bottom, one wheel tick
/// away), never past the content, so the pane can never clamp blank.
pub const CHROME_ROWS: i32 = 5;

#[derive(Clone)]
pub struct UiCtx {
    pub tx: Sender<Cmd>,
    /// Clone of the gateway client for LANES THAT MUST NOT QUEUE behind
    /// the worker's sequential loop (today: the quit modal's dedicated
    /// verb send — an in-flight artifact fetch could hold a quit-time
    /// pause for minutes). UI-thread code must never CALL it directly
    /// (no HTTP on the render thread); it exists to be cloned into
    /// short-lived named threads.
    pub client: crate::gateway::GatewayClient,
    pub overlays: Overlays,
    pub quitter: Quitter,
    pub prefs: Rc<RefCell<Prefs>>,
    pub workspace_root: Option<String>,
    // No workspace_mode here: `store.workspace_mode` (the signal, seeded
    // at boot, edited by /workspace) is the ONE authority — a UiCtx copy
    // was dead state that could only drift (cycle-3 audit).
    pub max_iterations: u32,
    /// `--max-iterations` was explicitly given at launch.
    pub max_iterations_explicit: bool,
    /// `--no-project-context`: suppress AGENTS.md injection for this session.
    pub no_project_context: bool,
    /// `--no-prompt-cache`: state the OFF posture for this session.
    ///
    /// The flag was wired into `exec` only. The interactive path built its
    /// `StartOpts` with `prompt_cache: None` — "server truth" — so the TUI
    /// ACCEPTED `--no-prompt-cache` (rc=0, where an unknown flag gives rc=2)
    /// and then silently cached anyway. A flag that parses, exits clean, and
    /// does nothing is worse than one that is rejected: the operator has no
    /// way to discover it did not take.
    pub no_prompt_cache: bool,
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
    /// Retire a modal taken out of the slot. Since abstracttui 0.2.3
    /// (our 0297 filing) the disposal-safety law is ENGINE-WIDE — every
    /// widget callback completes its own bookkeeping BEFORE the user
    /// callback runs (Button's post-`on_click` `pressed` write and
    /// TextArea's caret republish were the last offenders, both fixed
    /// and per-site disposal-pinned) — so a synchronous `close()`
    /// (layer removal + scope disposal in one call) is safe from inside
    /// the modal's own widget callbacks. The one-tick disposal deferral
    /// this method used to carry is deleted; layer + scope go together,
    /// so the equal-z invisible-key-eater window (live 2026-07-21,
    /// /model stage 2) cannot open either.
    fn retire(&self, m: Modal) {
        m.close();
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
        // Select mode stays ON across modals since abstracttui 0.2.8
        // (our first-app/0285, fixed at the engine): the selection layer
        // claims a mouse gesture only once it DRAGS — a plain click
        // passes through to the modal's buttons, a drag that started on
        // a button releases the press without firing, and drag-copy
        // inside modals works. The open_modal/close_modal set_enabled
        // toggles this method carried while 0285 was open are DELETED;
        // the boot enable in lib.rs remains the single writer.
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

pub fn root(cx: Scope, store: Store, ctx: UiCtx, actions: &abstracttui::app::Actions) -> View {
    let theme = use_theme(cx);
    let spin = cx.signal(0u64);
    // Engine follow-tail (0.2.0): the Scroll pins to the bottom while
    // `follow` is true and its own gestures (wheel) manage disengage /
    // re-arm; the key shortcuts below write the same signal.
    let scroll_offset = cx.signal(0i32);
    let follow = cx.signal(true);
    let feed = abstracttui::widgets::FeedState::new(cx);
    // The splash predicate (IDLE-2), hoisted HERE so the pane's render
    // branch and the animation ticker read ONE truth (a predicate
    // duplicated across an effect and a render is the mirror-drift
    // class this codebase has already paid for twice). "Splash" = the
    // agent lane with no conversation yet (boot Info notices only).
    let splash_visible = cx.memo(move || {
        // A session restore in flight shows the loading screen
        // (`ui::loading`), which shares the splash's frame clock — the
        // ticker predicate below reads THIS memo, so both full-pane
        // ambient surfaces arm the one interval (zero-wakeup rule kept
        // in one predicate). The pane checks `restoring` before its
        // splash branch, so the OR here never paints a splash early.
        if store.restoring.get() {
            return true;
        }
        if matches!(store.focus.get(), crate::convo::Focus::Agent) {
            store
                .fold
                .with(|f| f.items.iter().all(|i| matches!(i, Item::Info { .. })))
        } else {
            false
        }
    });
    // The splash animation frame: advanced by a ticker that exists ONLY
    // while the splash is visible — everywhere else the app keeps the
    // engine's zero-wakeup idle guarantee (the one deliberate exception
    // is the splash itself: a continuous logo shimmer is the point).
    let splash = cx.signal(0u64);
    wire_splash_ticker(cx, store, splash_visible, splash);
    // `/animation`: the feed accumulates whether or not the pane is
    // showing (opening it mid-run must show the run's whole history, not
    // start from blank); the ticker exists only while it IS showing.
    let anim_feed = animation::wire_feed(cx, store);
    let anim_frame = cx.signal(0u64);
    animation::wire_ticker(cx, store, anim_frame);
    // `/stance` (2026-08-21): the Cognitive Monitor's conduct read, in
    // the terminal. Session-scoped and off by default; the signal lives
    // HERE rather than in `store` so the feature stays one directory
    // plus three lines (see `ui::stance`'s removal note).
    let stance_mode = cx.signal(stance::OFF);
    let stance_frame = cx.signal(0u64);
    wire_stance_ticker(cx, store, stance_mode, stance_frame);
    // The read floats bottom-right as an OVERLAY rather than taking a
    // row of the column: it paints over the transcript and routes no
    // input, so scrolling and selecting the conversation underneath are
    // untouched (`ui::stance::wire_overlay`).
    stance::wire_overlay(cx, store, ctx.overlays.clone(), stance_mode, stance_frame);

    wire_camera_default_off(cx, store, ctx.clone());
    spawn_run_ticker(cx, store, spin);
    spawn_probe_ticker(cx, store, ctx.tx.clone());
    wire_conn_self_heal(
        cx,
        store,
        ctx.tx.clone(),
        ctx.prefs.clone(),
        ctx.replay_turns,
    );
    wire_gpu_cadence(cx, store);
    wire_llm_meter(cx, store);
    wire_toasts(cx, store, ctx.overlays.clone());
    wire_startup_notices(cx, store);
    wire_wait_modals(cx, store, ctx.clone());
    wire_queue_drain(cx, store, ctx.clone());
    wire_queue_persistence(cx, store, ctx.clone());
    wire_pending_steer(cx, store, ctx.clone());
    goal::wire_goal(cx, store, ctx.clone());
    quit::wire_quit(cx, store, &ctx);
    transcript_view::wire_feed(
        cx,
        store,
        &feed,
        ctx.workspace_root.as_deref().map(std::rc::Rc::from),
    );
    entity_actions::wire_poller_view(cx, store);
    entity_actions::wire_focus_follow(cx, store, follow);
    wire_history_autoload(cx, store, &ctx, follow, scroll_offset);

    // Durable composer state (draft, caret, input history): lives in root
    // scope so theme rebuilds of the TextArea keep everything.
    let composer = abstracttui::widgets::TextAreaState::new(cx);
    // Where the composer currently lives in the tree — the root's
    // type-to-focus handler needs it (see `chrome::ComposerAnchor`).
    let composer_anchor = chrome::ComposerAnchor::default();
    wire_ctrl_c(cx, actions, store, &ctx, &composer);

    // One-shot composer seed (queue modal `e` pops an item into the
    // composer; the modal cannot reach the root-scoped TextAreaState, so
    // it writes this signal instead).
    {
        let composer = composer.clone();
        cx.effect(move || {
            if let Some(text) = store.composer_seed.get() {
                store.composer_seed.set(None);
                composer.set_text(&text);
            }
        });
    }

    // An undelivered steer comes BACK to the composer — but only into an
    // empty one. A draft the operator typed after the failure outranks a
    // restore; the error card carries the words in both cases.
    {
        let composer = composer.clone();
        cx.effect(move || {
            if let Some(text) = store.steer_restore.get() {
                store.steer_restore.set(None);
                if composer.text().trim().is_empty() {
                    composer.set_text(&text);
                }
            }
        });
    }

    let on_submit = {
        let ctx = ctx.clone();
        let composer = composer.clone();
        move |text: &str| {
            follow.set(true); // sending jumps back to the tail
            submit(cx, store, &ctx, &composer, text, stance_mode)
        }
    };

    let root_ctx = ctx.clone();
    let esc_ctx = ctx.clone();
    let esc_composer = composer.clone();
    // Header fact (HDR-1): the workspace directory's basename — computed
    // once (the root never changes in-session; /workspace edits scope,
    // not the root).
    let cwd_base: String = ctx
        .workspace_root
        .as_deref()
        .map(|p| {
            std::path::Path::new(p)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string())
        })
        .unwrap_or_default();

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

    // Page-jump helper (external offset writes decide follow for
    // themselves — feed.total_rows() replaces the old measure math).
    // Follow derives from GEOMETRY, mirroring the engine's own gesture
    // rule (Scroll::derive_follow, 0130 semantics: landing on the
    // bottom edge (re-)arms follow, landing above releases it). The
    // old up-branch released UNCONDITIONALLY — on a transcript that
    // FITS the pane (max_off == 0) a PageUp that visibly moves nothing
    // disengaged follow at offset 0, and with older turns on the
    // gateway the scroll-top auto-loader then streamed the WHOLE
    // session off one keypress; the wheel path never did (the engine
    // keeps follow armed on fitting content), so the two up-gestures
    // also disagreed. Geometry keeps the auto-load trigger what the
    // ruling names: reaching the top of a transcript you actually
    // scrolled.
    let page = {
        let feed = feed.clone();
        move |delta: i32| {
            let vp = current_viewport();
            let pane_h = (vp.h - CHROME_ROWS).max(3);
            let max_off = (feed.total_rows().get_untracked() - pane_h).max(0);
            let next = (scroll_offset.get_untracked() + delta).clamp(0, max_off);
            if next < max_off {
                // Release BEFORE the offset write (the old up-branch's
                // ordering): the engine's follow pin must never fight
                // the write within the same flush.
                follow.set(false);
                scroll_offset.set(next);
            } else {
                scroll_offset.set(next);
                follow.set(true);
            }
        }
    };

    Element::new()
        .style(LayoutStyle::column())
        // INPUT RETURNS TO THE COMPOSER (operator ask 2026-08-16). The
        // transcript `Scroll` is focusable, so ONE Tab — or a click
        // anywhere in the scrollback, which focuses the nearest
        // focusable ancestor — parks the keyboard there, and the Scroll
        // answers only arrows/PageUp/Home/End. Everything the user
        // MEANT for the prompt was DROPPED after that, with no visible
        // sign of where it went: characters, `/commands`, pasted text,
        // dropped files. Typing and pasting are the universal "I want
        // to write" gestures, so they hand focus back and keep what
        // arrived.
        //
        // Capture phase at the root: handlers run root->target BEFORE
        // the shortcut table, so this sees the event first. It claims
        // PRINTABLE characters and PASTES only — Ctrl/Alt chords fall
        // through to the shortcut table (Ctrl+T/D/E/Q/L/O), and the
        // Scroll keeps every navigation key it owns. Guards: nothing
        // happens while the composer already holds focus (its own edit
        // model and paste hook own that input, `/` dropdown included),
        // and modals never reach here at all — a modal overlay swallows
        // keys inside its own layer tree.
        .on(abstracttui::ui::Phase::Capture, {
            let composer = composer.clone();
            let anchor = composer_anchor.clone();
            move |ctx: &mut abstracttui::ui::EventCtx, ev: &abstracttui::ui::UiEvent| {
                if composer.focused().get_untracked() {
                    return; // the composer already owns this input
                }
                // No anchor = the composer has never mounted. Do
                // NOTHING (side effects included) rather than act for a
                // widget that cannot take the focus: input parked in an
                // unfocused draft is worse than the dropped input it
                // replaces.
                let Some(id) = anchor.get() else { return };
                let insert = match ev {
                    // `Key::Char` + `!ctrl && !alt` mirrors the engine's
                    // own insert rule (`widgets::textarea_model`), so
                    // SHIFT arrives already folded into the character on
                    // both wire spellings.
                    abstracttui::ui::UiEvent::Key(k) => {
                        let Key::Char(ch) = k.key else { return };
                        if k.mods.contains(Mods::CTRL) || k.mods.contains(Mods::ALT) {
                            return;
                        }
                        ch.to_string()
                    }
                    // The composer's paste contract, run from the one
                    // place that can still see this event: a verified
                    // file drop becomes attachment chips (Consume —
                    // nothing inserted, focus still comes back so the
                    // user can type the prompt that goes WITH the file),
                    // everything else inserts with newlines normalized.
                    // Both halves are the engine's — `handle_paste` is
                    // the same hook body `TextArea::on_paste` runs, and
                    // the normalization mirrors its block-paste rule.
                    // An unknown future action INSERTS (engine ADR-0003
                    // §3: never silently drop the user's text).
                    abstracttui::ui::UiEvent::Paste(raw) => {
                        match attachments::handle_paste(store, raw) {
                            abstracttui::widgets::PasteAction::Consume => String::new(),
                            _ => raw.replace("\r\n", "\n").replace('\r', "\n"),
                        }
                    }
                    _ => return,
                };
                if !insert.is_empty() {
                    let caret = composer.caret_byte();
                    composer.replace_range(caret..caret, &insert);
                }
                ctx.request_focus(id);
                ctx.stop_propagation();
            }
        })
        .shortcut(KeyChord::new(Mods::CTRL, Key::Char('t')), move |_| {
            cycle_theme(&root_ctx);
        })
        .shortcut(KeyChord::new(Mods::CTRL, Key::Char('d')), {
            let ctx = ctx.clone();
            move |_| toggle_details(store, &ctx)
        })
        // Alt+E cycles conversation focus. This was Ctrl+E until
        // abstracttui 0.3.2, which gave the text widgets Codex's editor
        // keymap — Ctrl+E is move-to-line-end there, and a FOCUSED
        // editor consumes its chords before any shortcut can see them
        // (the engine's documented resolution order). The composer holds
        // focus almost always, so Ctrl+E had become a dead key rather
        // than an ambiguous one. Alt keeps the E-for-entity mnemonic
        // (the engine's editor claims Alt+b/f/d and Alt+arrows, never
        // Alt+e); on macOS it needs "Option as Meta/Esc+", the same
        // setting Alt+Enter already asks for, and `/focus <name>` is
        // the spelling that works on every terminal regardless.
        .shortcut(KeyChord::new(Mods::ALT, Key::Char('e')), move |_| {
            entity_actions::cycle_focus(store)
        })
        .shortcut(KeyChord::new(Mods::CTRL, Key::Char('q')), {
            let ctx = ctx.clone();
            move |_| quit::request_quit(cx, store, &ctx)
        })
        // Ctrl+L force-redraw (HDR-2a): the recovery affordance for
        // externally-lost cells (Cmd+K / terminal clear) — the Python
        // app had one (fullscreen_ui.py:3494); the port dropped it and
        // a wiped screen stayed blank FOREVER (the maintainer's exact
        // screenshot). Since abstracttui 0.2.6 (our 0299 filing) the
        // engine owns the verb: `request_full_redraw()` poisons the
        // diff baseline + invalidates the presenter, so the next frame
        // re-emits EVERY cell with absolute anchoring and re-places
        // protocol images — the app-side translucent-veil workaround
        // (and its measured limits) is deleted. Works while the
        // composer holds focus: the TextArea edit model ignores
        // ctrl-modified chars, so the chord falls through to root
        // shortcuts (the Ctrl+T mechanism).
        .shortcut(KeyChord::new(Mods::CTRL, Key::Char('l')), move |_| {
            abstracttui::app::request_full_redraw()
        })
        // Ctrl+O: undo the newest consumed file-drop (chips out, the
        // raw pasted path text back into the composer). Taught only by
        // the drop notice; quiet no-op otherwise.
        .shortcut(KeyChord::new(Mods::CTRL, Key::Char('o')), {
            let composer = composer.clone();
            move |_| attachments::undo_drop(store, &composer)
        })
        // A press ON the stance panel folds/unfolds it. The panel is a
        // DRAW overlay — it routes no input, which is what keeps the
        // transcript under it scrollable and selectable — so the click
        // is caught HERE, at capture phase, and consumed only when it
        // lands inside the panel's own box. Everything else passes
        // through untouched.
        .on(abstracttui::ui::Phase::Capture, move |ectx, ev| {
            let abstracttui::ui::UiEvent::Mouse(m) = ev else {
                return;
            };
            if !matches!(
                m.kind,
                abstracttui::ui::MouseKind::Down(abstracttui::ui::MouseButton::Left)
            ) {
                return;
            }
            let view = abstracttui::app::current_viewport();
            if stance::hit(stance_mode.get_untracked(), view, m.pos) {
                ectx.stop_propagation();
                stance::command(stance_mode, None);
            }
        })
        // Ctrl+G folds/unfolds the stance panel (same cycle as
        // `/stance`): off → the folded strip → the card → off. A chord
        // because "fold it away" is a thing you do mid-conversation, and
        // typing a command to hide a read-out is not folding.
        .shortcut(KeyChord::new(Mods::CTRL, Key::Char('g')), move |_| {
            let line = stance::command(stance_mode, None);
            store.notify(&line);
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
                // Focus + phase reads: a change rebuilds this chrome
                // (placeholder is a build-time TextArea param). The durable
                // TextAreaState lives in root scope, so the draft survives —
                // same rule as theme rebuilds. Phase-swapped under AGENT
                // focus only (plan item 1 discoverability): under entity
                // focus the composer belongs to the entity lane's banner,
                // and an agent-lane "enter steers" hint there would lie.
                // Newline-chord honesty (0.2.2, our 0295): teach the BEST
                // chord this terminal actually speaks. Reading the caps
                // SIGNAL here rebuilds the chrome when the probe upgrades
                // mid-session (kitty enter-flags follow the probe), so the
                // hint flips to Shift+Enter the moment it becomes true.
                let kitty_kbd = abstracttui::app::use_caps(scx).get().kitty_keyboard;
                let placeholder = match store.focus.get() {
                    crate::convo::Focus::Agent => agent_placeholder(store.phase.get(), kitty_kbd),
                    crate::convo::Focus::Entity(name) => entity_actions::entity_placeholder(&name),
                };
                Element::new()
                    // Fill the viewport: a content-hugging column floats the
                    // status bar mid-screen on first launch (live finding).
                    .style(
                        LayoutStyle::column()
                            .grow(1.0)
                            .width(Dimension::Percent(1.0))
                            .height(Dimension::Percent(1.0)),
                    )
                    .child(chrome::header(&t, store, spin, cwd_base.clone()))
                    .child(transcript_view::pane(
                        scx,
                        &t,
                        store,
                        &ctx,
                        &feed,
                        scroll_offset,
                        follow,
                        splash_visible,
                        splash,
                        anim_feed.clone(),
                        anim_frame,
                    ))
                    // One breathing row between the transcript's last line
                    // and the control panel (operator ask, 2026-07-23). A
                    // fixed spacer, NOT padding on the pane: the Scroll owns
                    // its viewport math, and a bottom padding would shift
                    // where the engine believes the visible window ends.
                    // Counted in CHROME_ROWS (5) so the pane-height
                    // estimates stay honest.
                    .child(
                        Element::new()
                            .style(LayoutStyle::line(1).shrink(0.0))
                            .build(),
                    )
                    // Pending attachment chips (only while staged — no
                    // reserved blank; CHROME_ROWS estimates deliberately
                    // exclude this sometimes-present row).
                    .child(attachments::chips_row(cx, store, &ctx))
                    .child(chrome::activity_strip(&t, store, spin, follow))
                    // In-flow composer: grows 1..4 rows with the draft (the
                    // absolute-position + spacer trick existed only for the
                    // pre-0.2.0 focus_first policy).
                    .child(chrome::composer(
                        scx,
                        &t,
                        store,
                        &composer,
                        &composer_anchor,
                        &overlays,
                        placeholder,
                        on_submit.clone(),
                    ))
                    .child(chrome::status_bar(&t, store, &ctx))
                    .build()
            }
        }))
        .build()
}

/// Agent-focus composer placeholder, phase-swapped (plan item 1): the
/// composer itself teaches what Enter does RIGHT NOW. The newline chord
/// is capability-derived (0.2.2, our 0295): Shift+Enter where the kitty
/// keyboard protocol is live, else Ctrl+J — the works-everywhere chord
/// (it IS the LF byte on the legacy wire). Pure over its inputs so the
/// swap rule is unit-testable.
fn agent_placeholder(phase: Phase, kitty_keyboard: bool) -> String {
    let newline = if kitty_keyboard {
        "Shift+Enter newline"
    } else {
        "Ctrl+J newline"
    };
    match phase {
        Phase::Idle => format!("describe a task — Enter sends · {newline} · /help"),
        Phase::Starting => {
            "run starting — Enter buffers guidance for it · /queue <text> lines up the next task"
                .to_string()
        }
        Phase::Running => {
            format!("Enter steers the run · {newline} · /queue <text> lines up the next task")
        }
    }
}

fn submit(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    composer: &abstracttui::widgets::TextAreaState,
    text: &str,
    // `/stance`'s mode lives in root scope, not in `store` (the feature
    // owns no shared state — see `ui::stance`'s removal note), so it
    // rides the two hops from the composer to the dispatcher.
    stance_mode: Signal<u8>,
) {
    let text = text.trim().to_string();
    // `?` opens the keys + commands reference (REST-1): the status-bar
    // legend moved behind it — the footer's `? keys + commands` names
    // this exact gesture. A bare `?` is never a useful agent prompt.
    if text == "?" {
        modals::open_help(cx, ctx);
        return;
    }
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
            // Commands parsed first (global); then @name routing (leading-@
            // only — a mid-prompt @ is plain text); then the focused
            // conversation decides.
            match entity_actions::route_mention(store, ctx, &text) {
                entity_actions::Routed::Consumed => return,
                entity_actions::Routed::UnknownName => {
                    // Draft preserved: put the text back so a typo'd name
                    // is one edit away (the composer cleared on submit).
                    composer.set_text(&text);
                    return;
                }
                entity_actions::Routed::No => {}
            }
            if let crate::convo::Focus::Entity(name) = store.focus.get_untracked() {
                entity_actions::send_or_hold(store, ctx, &name, &text);
                return;
            }
            let phase = store.phase.get_untracked();
            match phase {
                Phase::Running => {
                    steer_or_buffer(store, ctx, &text);
                    // Chips never ride steers — say so while they wait.
                    attachments::note_kept_for_steer(store);
                }
                Phase::Starting => {
                    // The new run id has not landed yet — a steer now would
                    // target the PREVIOUS run. Buffer instead of dropping
                    // (the old refusal toast LOST the text): delivered when
                    // the NEW tree's first reason-cycle lands, error-carded
                    // if the start fails (`wire_pending_steer`).
                    buffer_steer(store, &text, true);
                    store.notify("run is starting — guidance buffered, delivered when it's up");
                }
                // The ONE call site that carries pending attachments
                // (explicit plain-prompt send): goal starts and queue
                // drains go through `start_run` and stay chip-free.
                Phase::Idle => start_run_attaching(store, ctx, &text),
            }
        }
        Some(cmd) => dispatch_command(cx, store, ctx, cmd, stance_mode),
    }
}

/// Explicit composer send: carries the pending attachment chips.
pub(crate) fn start_run_attaching(store: Store, ctx: &UiCtx, prompt: &str) {
    let pending = store.pending_attachments.get_untracked();
    start_run_inner(store, ctx, prompt, pending);
}

/// Chip-free start (queue drains, programmatic restarts).
pub(crate) fn start_run(store: Store, ctx: &UiCtx, prompt: &str) {
    start_run_inner(store, ctx, prompt, Vec::new());
}

fn start_run_inner(
    store: Store,
    ctx: &UiCtx,
    prompt: &str,
    attachments: Vec<crate::store::PendingAttachment>,
) {
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
    // Conversation context BEFORE this turn's user card lands (whole
    // completed turns only; caps mirror the server seed defaults). Client
    // messages win over the server seed — needed live because wrapper
    // bundles can leave prior roots non-completed (helper pollers),
    // starving the server-side session replay.
    let messages = store.fold.with_untracked(|f| f.chat_messages(40, 24_000));
    let opts = agent_start_opts(store, ctx, messages);
    send_start(store, ctx, workflow, prompt, opts, attachments);
}

/// The run infrastructure every start shares (provider/model, workspace
/// scope, tool selection + policy, skills) — used by plain prompts and
/// `/goal` runs (which add goal params on top).
pub(crate) fn agent_start_opts(
    store: Store,
    ctx: &UiCtx,
    messages: Vec<(String, String)>,
) -> StartOpts {
    // Tool selection: untouched = the workflow's own defaults (send
    // nothing); customized = the checked set is the run's exact allowlist.
    // Only disabled names that EXIST in the inventory count — a stale name
    // from another gateway must not silently flip the run into explicit-
    // allowlist mode (adversary finding 6).
    let disabled = store.disabled_tools.get_untracked();
    let inventory = store.tools.get_untracked();
    // Effective user-disabled = names matching ENABLED inventory rows
    // only — the ONE shared predicate with the /tools title (cycle-2
    // adversary P1-2: counting a served-disabled match here flipped
    // the run into explicit-allowlist mode while the title said
    // "untouched", and the invisible stale-pref state — a name the
    // user disabled while enabled, later gate-disabled server-side —
    // could silently WIDEN the agent's tool set past the workflow's
    // baked pin). A served-disabled row cannot run either way; only a
    // user choice about a grantable row means "customized".
    let effective_disabled = crate::store::Store::effective_user_disabled(&inventory, &disabled);
    let tools = if effective_disabled == 0 {
        None
    } else {
        Some(
            inventory
                .iter()
                // Served-disabled rows (full-catalog surfacing: the
                // gateway serves gate-disabled tools `enabled:false` so
                // their existence is visible) are NEVER granted: an
                // explicit allowlist naming a disabled tool would claim
                // a grant the gateway cannot honor.
                .filter(|t| !t.served_disabled)
                .map(|t| t.name.clone())
                .filter(|n| !disabled.contains(n))
                .collect::<Vec<_>>(),
        )
    };
    // Workspace scope: the LIVE signals own mode + allowed paths (seeded
    // from flags/prefs at boot, edited by /workspace); the root stays the
    // boot resolution (--workspace / cwd).
    let ws_mode = store.workspace_mode.get_untracked();
    // Server-side tool policy (facts #1): expand the accepted tier + pins
    // over the CURRENT inventory into name lists the runtime honors with
    // no wait round-trip. The client-side belt (wire_wait_modals) stays as
    // the fallback for waits that still arrive (names outside this
    // snapshot).
    let tool_policy = crate::tool_policy::expand_run_policy(
        &store.tool_classes(),
        &store.accepted_tier.get_untracked(),
        &store.tool_overrides.get_untracked(),
    );
    StartOpts {
        provider: store.provider.get_untracked(),
        model: store.model.get_untracked(),
        gating_mode: store.gating_mode.get_untracked(),
        reasoning: store.reasoning.get_untracked(),
        // The operator-declared window rides as `_limits.max_tokens`
        // (CTX-0); 0 = undeclared = the key stays absent.
        context_window: store.context_window.get_untracked(),
        // Filled by the WORKER after upload (Cmd::Start carries the
        // pending list) — the shared opts stay attachment-free so goal
        // and queue-drain starts can never pick chips up implicitly.
        attachments: Vec::new(),
        // `--no-prompt-cache` states the OFF posture; absent = server truth.
        // Same rule as `exec.rs`, deliberately: the two entry points must not
        // disagree about whether a launch flag is honoured.
        prompt_cache: if ctx.no_prompt_cache {
            Some(false)
        } else {
            None
        },
        workspace_root: ctx.workspace_root.clone(),
        workspace_mode: if ws_mode.trim().is_empty() {
            None
        } else {
            Some(ws_mode)
        },
        workspace_allowed: store.workspace_allowed.get_untracked(),
        // The SIGNAL, not the UiCtx copy: `--max-iterations` seeds it at boot
        // and `/iterations` edits it, so there is one authority (same rule as
        // `workspace_mode` above). A budget is "explicit" whenever a number is
        // in force from either source — absent one we send nothing and take
        // the server's own, which is what every other client gets.
        max_iterations: store.max_iterations.get_untracked(),
        max_iterations_explicit: store.max_iterations.get_untracked() > 0,
        system: String::new(),
        // Verifier-before-conclude: the session posture (`/review`), always
        // STATED so a run's transcript records what was asked for rather
        // than inheriting whichever server default was in force.
        review_mode: Some(store.review_mode.get_untracked()),
        review_capable: store
            .workflow
            .with_untracked(|w| crate::discovery::workflow_is_review_capable(&w.bundle_id)),
        review_max_rounds: store.review_rounds.get_untracked(),
        // Project instructions (AGENTS.md) for the session's workspace —
        // resolved through the SAME helper headless `exec` uses, so both
        // surfaces inject identical context for identical workspaces. The
        // notices ride the toast lane; a missing file stays silent.
        system_prompt_extra: crate::project_context::resolve_project_context(
            ctx.workspace_root.as_deref(),
            ctx.no_project_context,
            |line| store.notify(line),
            |sources, chars| store.notify(format!("project context: {sources} ({chars} chars)")),
        ),
        messages,
        tools,
        skills: store.selected_skills.get_untracked(),
        goal: None,
        tool_policy,
    }
}

/// Push the user card, flip to Starting, and hand the start to the
/// runner — with the dead-worker honesty fallback.
pub(crate) fn send_start(
    store: Store,
    ctx: &UiCtx,
    workflow: crate::store::Workflow,
    prompt: &str,
    opts: StartOpts,
    attachments: Vec<crate::store::PendingAttachment>,
) {
    let session_id = store.session_id.get_untracked();
    store.fold.update(|f| {
        f.push_item(Item::User {
            text: prompt.to_string(),
        })
    });
    store.phase.set(Phase::Starting);
    // The strip shows elapsed while Starting: a stale value from the
    // PREVIOUS run must never flash before the runner's reset lands.
    store.elapsed_secs.set(0);
    // Anchor the clock NOW (visibility review P0-1): two live paths
    // (boot attach to a parked wrapper root; mid-run session switch)
    // leave a stale hours-old `run_started` behind, and the ticker
    // resurrects it within 120ms of Starting — "starting run · 9h20m"
    // for a task submitted one second ago. Anchoring here makes the
    // Starting window tick honestly from 0; `apply_start_binding`
    // re-anchors moments later when the run actually starts.
    store.run_started.set(Some(std::time::Instant::now()));
    // The first prompt names the session in the /sessions picker.
    persist_prefs(ctx, |p| p.touch_session(&session_id, Some(prompt)));
    let delivered = ctx.send(Cmd::Start {
        prompt: prompt.to_string(),
        flow_id: workflow.flow_id,
        bundle_id: workflow.bundle_id,
        session_id,
        opts: Box::new(opts),
        attachments,
        // UI-thread snapshot: the worker must never read this signal
        // itself (thread stamp — verify-pass NEW-1).
        attachment_cap: store.max_attachment_bytes.get_untracked(),
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

fn dispatch_command(cx: Scope, store: Store, ctx: &UiCtx, cmd: Command, stance_mode: Signal<u8>) {
    match cmd {
        Command::Help => modals::open_help(cx, ctx),
        Command::Quit => quit::request_quit(cx, store, ctx),
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
        Command::Workflow => {
            // Catalog freshness at the operator's gesture (the /tools /skills
            // /mcp Load*-before-open pattern): the boot's LoadCatalog was the
            // ONLY successful load a session ever ran, so a long-lived TUI
            // pinned the launch-time snapshot and entrypoints registered
            // afterwards never appeared (operator incident 2026-07-25).
            // Preference mirrors the Down→Ok self-heal: prefs is the boot's
            // own source, and `load_catalog` never clobbers a non-empty
            // selection. The picker rows are LIVE (reactive-picker
            // follow-up, flow's c5483 thread): a refresh landing while
            // the picker is open renders in place — no reopen needed.
            let (preferred_bundle, preferred_flow) = {
                let p = ctx.prefs.borrow();
                (p.bundle_id.clone(), p.flow_id.clone())
            };
            ctx.send(Cmd::LoadCatalog {
                preferred_bundle,
                preferred_flow,
            });
            modals::open_workflow_picker(cx, store, ctx);
        }
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
        Command::Sessions(None) => modals::open_sessions(cx, store, ctx),
        Command::Sessions(Some(id)) => switch_session(store, ctx, &id),
        Command::Cancel => {
            entity_actions::agent_command_notice(store, "/cancel");
            cancel_run(store, ctx)
        }
        Command::Conclude(note) => {
            entity_actions::agent_command_notice(store, "/conclude");
            let run_id = store.run_id.get_untracked();
            if run_id.is_empty() || store.phase.get_untracked() == Phase::Idle {
                store.notify("no active run to conclude");
            } else if store.paused.get_untracked() {
                store.notify("the run is paused — /resume first, then /conclude");
            } else {
                ctx.send(Cmd::Conclude { run_id, note });
            }
        }
        Command::Pause => {
            entity_actions::agent_command_notice(store, "/pause");
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
            entity_actions::agent_command_notice(store, "/resume");
            let run_id = store.run_id.get_untracked();
            if run_id.is_empty() || !store.paused.get_untracked() {
                store.notify("no paused run — /pause pauses the active one");
            } else {
                ctx.send(Cmd::ResumePaused { run_id });
            }
        }
        // OBS-6 toggle — one arm, one authority: `toggle_gpu_meter`
        // (carries the dead-worker revert; two concurrent lanes landed
        // an arm each, folded here).
        Command::Gpu => toggle_gpu_meter(store, ctx),
        Command::Resources => {
            // GATED on the gateway's declared host_state contract: with
            // the contract confirmed, fetch at the gesture (`/host/state`
            // is slow by contract — open + `r` only, never polled); with
            // contracts still unanswered, re-probe capabilities so the
            // open modal can heal live; with the contract known-absent,
            // fetch nothing — the modal says so honestly.
            store.host_estimate.set(None);
            match store.host_contracts.get_untracked() {
                Some(c) if c.host_state => {
                    // Held facts (fresh or stale) stay visible while the
                    // open-time fetch runs; only a factless state shows
                    // the Pending screen.
                    if !matches!(
                        store.host_state.get_untracked(),
                        crate::store::HostState::Ready(_) | crate::store::HostState::Stale(_)
                    ) {
                        store.host_state.set(crate::store::HostState::Pending);
                    }
                    ctx.send(Cmd::LoadHostState);
                }
                None => {
                    ctx.send(Cmd::LoadCapabilities);
                }
                Some(_) => {}
            }
            modals::open_resources(cx, store, ctx);
        }
        Command::Details(arg) => match arg.as_deref().map(str::trim) {
            None | Some("") => toggle_details(store, ctx),
            Some("full") | Some("expand") | Some("on") => {
                store.show_details.set(true);
                persist_prefs(ctx, |p| p.show_details = Some(true));
                store.notify(
                    "details: full — tool args + results, thinking expanded · /details fold collapses",
                );
            }
            Some("fold") | Some("gist") | Some("off") => {
                store.show_details.set(false);
                persist_prefs(ctx, |p| p.show_details = Some(false));
                store.notify(
                    "details: folded — one-line tool calls with status tags, thinking gists · /details full expands",
                );
            }
            Some(other) => {
                store.notify(format!("/details takes full|fold (got {other:?})"));
            }
        },
        Command::Gating(arg) => {
            let v = arg
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .to_ascii_lowercase();
            match v.as_str() {
                "" => {
                    let state = if store.gating_mode.get_untracked() == "auto" {
                        "auto — unattended (the coder's approval pauses are SKIPPED)"
                    } else {
                        "wait — gated (the coder pauses for your approval; the default)"
                    };
                    store.notify(format!("gating: {state} · /gating auto | wait to change"));
                }
                "auto" => {
                    store.gating_mode.set("auto".into());
                    store.notify(
                        "gating: auto — unattended. The coder will NOT pause for approval. \
                         Tools still gate per your permission mode (/permissions).",
                    );
                }
                "wait" | "gated" | "on" => {
                    store.gating_mode.set(String::new());
                    store.notify("gating: wait — gated (the coder pauses for your approval)");
                }
                other => {
                    store.notify(format!("/gating takes auto | wait (got {other:?})"));
                }
            }
        }
        Command::Review(arg) => {
            let raw = arg.as_deref().map(str::trim).unwrap_or("");
            let v = raw.to_ascii_lowercase();
            let (head, tail) = match v.split_once(char::is_whitespace) {
                Some((h, t)) => (h, t.trim()),
                None => (v.as_str(), ""),
            };
            match head {
                "" => {
                    let on = store.review_mode.get_untracked();
                    let rounds = store.review_rounds.get_untracked();
                    let state = if on {
                        format!(
                            "ON (max_rounds={rounds}) — a strict verifier re-reads the \
                             transcript before any tool-call-free answer is accepted, and \
                             can force more tool calls"
                        )
                    } else {
                        "OFF — the agent concludes on its first answer without a check".into()
                    };
                    store.notify(format!("review: {state} · /review on | off | rounds N"));
                }
                "on" | "true" => {
                    store.review_mode.set(true);
                    store.notify(format!(
                        "review: ON (max_rounds={}) — the agent must survive a verifier pass \
                         before it may conclude",
                        store.review_rounds.get_untracked()
                    ));
                }
                "off" | "false" => {
                    store.review_mode.set(false);
                    store.notify(
                        "review: OFF — the agent concludes on its first tool-call-free answer. \
                         Faster, and the class of run that claims done too early.",
                    );
                }
                "rounds" | "max_rounds" => match tail.parse::<u32>() {
                    Ok(n) => {
                        store.review_rounds.set(n);
                        // Rounds without review on is a claim about a loop
                        // that never runs — say so rather than silently
                        // storing a dead number.
                        let note = if store.review_mode.get_untracked() {
                            String::new()
                        } else {
                            " (review is OFF — /review on to use it)".into()
                        };
                        store.notify(format!("review rounds: {n}{note}"));
                    }
                    Err(_) => store.notify(format!("/review rounds takes a number (got {tail:?})")),
                },
                other => store.notify(format!("/review takes on | off | rounds N (got {other:?})")),
            }
        }
        Command::Reasoning(arg) => match arg.as_deref().map(str::trim) {
            // Bare /reasoning: the dial for the current route (stage 3
            // opened directly; the probe fires for the current model).
            None | Some("") => modals::open_reasoning_stage(cx, store, ctx),
            Some(level) => {
                let v = level.to_ascii_lowercase();
                if v == "default" || v == "clear" {
                    modals::apply_reasoning_public(store, ctx, "");
                } else if crate::config::valid_reasoning_level(&v) {
                    modals::apply_reasoning_public(store, ctx, &v);
                } else {
                    store.notify(format!(
                        "/reasoning takes none|minimal|low|medium|high|xhigh|auto|default (got {level:?})"
                    ));
                }
            }
        },
        Command::Export(arg) => export_transcript(store, arg.as_deref().unwrap_or("")),
        Command::Attach(arg) => attachments::dispatch_attach(cx, store, ctx, arg),
        Command::History(arg) => {
            let older = store.older_turns.get_untracked();
            match (older, store.history_cursor.get_untracked().is_some()) {
                (0, _) | (_, false) => {
                    store.notify("no earlier history to stream — the whole session is shown");
                }
                (older, true) => {
                    // Bloc size: explicit n, `all` = everything older,
                    // default = the boot bloc size.
                    let count = match arg.as_deref().map(str::trim) {
                        Some("all") => older,
                        Some(n) => match n.parse::<usize>() {
                            Ok(v) if v > 0 => v,
                            _ => {
                                store
                                    .notify(format!("/history takes a count or `all` (got {n:?})"));
                                return;
                            }
                        },
                        None => ctx.replay_turns.max(1),
                    };
                    dispatch_history_bloc(store, ctx, count);
                }
            }
        }
        Command::Status => {
            // Probe server truth at the gesture: the modal renders the
            // client view immediately and the live get_run result when
            // it lands (run-less sessions skip the probe).
            store.run_status_probe.set(None);
            let run_id = store.run_id.get_untracked();
            if !run_id.is_empty() {
                ctx.send(Cmd::ProbeRunStatus { run_id });
            }
            modals::open_status(cx, store, ctx);
        }
        Command::Permissions(arg) => set_permissions(store, ctx, arg),
        Command::Workspace => modals::open_workspace(cx, store, ctx),
        Command::Steer(text) => {
            if text.is_empty() {
                store.notify("usage: /steer <guidance>");
            } else {
                steer_or_buffer(store, ctx, &text);
            }
        }
        Command::Queue(arg) => {
            // The queue is AGENT-LANE ONLY (plan: composition with the
            // entities plan): entity turns already hold your draft and
            // send it as the next turn — a second queue there would lie.
            // The DRAIN keeps running regardless of focus; only the
            // command surface + hints are agent-scoped.
            if matches!(store.focus.get_untracked(), crate::convo::Focus::Entity(_)) {
                store.notify(
                    "queue is agent-lane — /focus agent (entity turns already hold your draft and send it as the next turn)",
                );
                return;
            }
            match arg {
                None => queue_modal::open_queue(cx, store, ctx),
                Some(text) => enqueue_prompt(store, &text),
            }
        }
        Command::Goal(arg) => goal::dispatch_goal(store, ctx, arg),
        Command::Context(arg) => set_context_window(store, ctx, arg),
        Command::Iterations(arg) => set_max_iterations(store, ctx, arg),
        Command::Stance(arg) => {
            let line = stance::command(stance_mode, arg.as_deref());
            store.notify(&line);
        }
        Command::Redraw => abstracttui::app::request_full_redraw(),
        Command::Entities(name) => {
            // Async refresh behind the instantly-opened cached view.
            store.entities_loading.set(true);
            ctx.send(Cmd::LoadEntities);
            entity_modals::open_entities(cx, store, ctx, name);
        }
        Command::Brain(arg) => match arg {
            Some(name) => entity_actions::open_flow_convo(store, &name),
            None => {
                // Bare /brain reports the FOCUSED conversation's brain.
                let report = match store.focus.get_untracked() {
                    crate::convo::Focus::Entity(name) => store.convos.with_untracked(|cs| {
                        crate::convo::find(cs, &name).map(|ix| {
                            let brain = match cs[ix].brain {
                                crate::convo::Brain::Flow => {
                                    "flow (summon-per-prompt of the entity-chat flow)"
                                }
                                crate::convo::Brain::Visit => "visit (the durable driver lane)",
                            };
                            format!("{name}'s brain here: {brain}")
                        })
                    }),
                    crate::convo::Focus::Agent => None,
                };
                store.notify(report.unwrap_or_else(|| {
                    "usage: /brain <name> — flow-brain conversation with an entity".to_string()
                }));
            }
        },
        Command::Task { name, title } => entity_actions::leave_task(store, ctx, &name, &title),
        Command::End { name, reason } => {
            entity_actions::end_visit(store, ctx, name.as_deref(), &reason)
        }
        Command::FocusSwitch(word) => entity_actions::focus_by_word(store, &word),
        Command::Unknown(head) => {
            store.notify(format!("unknown command {head} — /help lists commands"));
        }
    }
}

/// `/export` — write the agent-lane transcript to a file (spec in
/// `crate::export`). This arm only ORCHESTRATES: gather items/meta from
/// the store, call the pure renderers, write, notify. It deliberately
/// works under any focus (it exports the agent lane either way — v1) and
/// reads the command's own `--details` flag, never the view toggle, so
/// repeated exports are stable regardless of Ctrl+D state.
fn export_transcript(store: Store, rest: &str) {
    use crate::export::{self, ExportFormat};
    let args = match export::parse_args(rest) {
        Ok(a) => a,
        Err(e) => {
            store.notify(e);
            return;
        }
    };
    let (items, truncated) = store
        .fold
        .with_untracked(|f| (f.items.clone(), f.truncated()));
    if !export::has_conversation(&items) {
        // Name the LANE (round-2 P2-4): under entity focus the user may
        // be looking at a rich visit while the AGENT lane is empty — an
        // unqualified "no conversation" contradicts their screen.
        store.notify(
            "nothing to export yet — the agent transcript has no conversation \
             (v1 exports the agent lane; entity visits live server-side)",
        );
        return;
    }
    let session_id = store.session_id.get_untracked();
    let now = crate::config::now_iso_utc();
    let path =
        match export::resolve_output_path(args.path.as_deref(), &session_id, &now, args.format) {
            Ok(p) => p,
            Err(e) => {
                store.notify(e);
                return;
            }
        };
    let (contents, summary) = match args.format {
        ExportFormat::Markdown => {
            let meta = export::ExportMeta {
                session_id,
                // Unresolved-workflow guard (round-2 P1-2): `label()` on
                // the Default workflow returns ":" — the third consumer
                // needing the same guard chrome/transcript_view carry;
                // "" keeps the header's own omit-when-unknown contract.
                workflow: store.workflow.with_untracked(|w| {
                    if w.flow_id.is_empty() {
                        String::new()
                    } else {
                        w.label()
                    }
                }),
                exported_at: now,
                truncated,
            };
            let shown = items
                .iter()
                .filter(|i| export::included(i, args.details))
                .count();
            (
                export::to_markdown(&items, &meta, args.details),
                format!("{shown} item(s)"),
            )
        }
        ExportFormat::Jsonl => {
            let (lines, skipped) = export::sft_lines(&items, args.details);
            if lines.is_empty() {
                store.notify(format!(
                    "no completed turns to export as JSONL ({skipped} unanswered prompt(s) skipped) — SFT lines need a user prompt with a final answer"
                ));
                return;
            }
            let summary = if skipped > 0 {
                format!(
                    "{} training line(s), {skipped} incomplete turn(s) skipped",
                    lines.len()
                )
            } else {
                format!("{} training line(s)", lines.len())
            };
            (lines.join("\n") + "\n", summary)
        }
    };
    if let Err(e) = export::write_new_file(&path, &contents) {
        store.notify(e);
        return;
    }
    // Absolute path in the notice (canonicalize succeeds — we just wrote
    // it; the unwrap_or keeps a fs race from panicking the UI).
    let abs = std::fs::canonicalize(&path).unwrap_or(path);
    // Format-conditional honesty (round-2 P1-3): the markdown header
    // carries the truncation note IN the file; JSONL is schema-pure (no
    // header line by design), so this notice is the ONLY warning surface
    // and must name the jsonl-specific consequence.
    let trunc_note = if !truncated {
        ""
    } else if args.format == ExportFormat::Jsonl {
        " — note: older items were truncated from view; the earliest turns are missing from every line's prefix"
    } else {
        " — note: older items were truncated from view (header says so)"
    };
    store.notify(format!(
        "exported agent transcript: {summary} → {} ({}){trunc_note}",
        abs.display(),
        args.format.label()
    ));
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
    let old_sid = store.session_id.get_untracked();
    let sid = crate::config::mint_session_id();
    store.session_id.set(sid.clone());
    persist_prefs(ctx, |p| {
        p.session_id = Some(sid.clone());
        p.touch_session(&sid, None);
    });
    reset_session_state(store, ctx, &old_sid, &sid, format!("new session {sid}"));
}

/// The session-boundary reset shared by `/new` and `/sessions` — ONE
/// authority so the two paths can never drift (cycle-2 integration
/// review: they had duplicated this block line-for-line; a reset added
/// to one and forgotten in the other is exactly the six-agent-wave
/// defect class). Resets the SESSION-SCOPED lanes only:
/// - transcript fold (+ the caller's boundary note),
/// - session totals + the last-call rate (the session's call history),
/// - run identity + phase + pause state,
/// - queue stash-and-restore, steer echo-drop, and the goal slot
///   (`swap_queue_for_session` — AFTER the fold reset, so echoes render
///   new-side),
/// - focus home to the agent lane (entity convos are server-side visits
///   and SURVIVE; session boundaries reset the AGENT conversation only).
///
/// Deliberately NOT touched (by intent, not omission): persisted prefs
/// (`context_window`, tool tier/pins, workspace scope), gateway-level
/// state (catalog, tools, skills, `/gpu` meter — a host fact), and the
/// bounded artifact-image cache (keyed by artifact id; a wiped fold
/// references none of it).
fn reset_session_state(store: Store, ctx: &UiCtx, old_sid: &str, new_sid: &str, note: String) {
    store.fold.update(|f| {
        // Catalog state SURVIVES the wipe (adversary P2-7): the agent-
        // workflow declarations came from the catalog load, not from the
        // session — dropping them here degraded answer-source binding to
        // the id-prefix fallback until the next catalog load (a switch
        // straight onto a live catalog-id run would mis-bind).
        let agent_ids: Vec<String> = f.agent_workflows().cloned().collect();
        *f = crate::transcript::Fold::new();
        f.set_agent_workflows(agent_ids);
        f.push_item(Item::Info { text: note });
    });
    store.totals.set(Default::default());
    store.last_call_rate.set(None);
    store.run_id.set(String::new());
    store.phase.set(Phase::Idle);
    store.paused.set(false);
    // History-bloc state belongs to the OLD session.
    store.history_cursor.set(None);
    store.older_turns.set(0);
    store.history_loading.set(false);
    // A stale (done, total) from the old session's probe must not flash
    // on the next loading screen; the new probe posts its own counters.
    store.restore_progress.set(None);
    // …and a failure notice belongs to the session it was about.
    store.restore_failed.set(None);
    // Stale-clock hygiene (visibility review P0-1 path 2): a mid-run
    // session switch cancels the run, but its `finish()` clear is
    // fold-guarded and skipped after this reset — the anchor must die
    // here or the NEXT submit resurrects an hours-old clock.
    store.run_started.set(None);
    // Pending chips die at session boundaries (cached refs are
    // session-bound; carrying files into an unrelated conversation is
    // the surprising behavior) — with a notice, never silently.
    attachments::discard_on_session_boundary(store);
    swap_queue_for_session(store, ctx, old_sid, new_sid);
    store.focus.set(crate::convo::Focus::Agent);
}

/// The ONE history-bloc dispatcher (slash command + scroll-top
/// auto-load): flips `history_loading`, rewrites the stub line into a
/// live progress indicator, and sends the worker command. The runner's
/// completion paths own the flip back + stub honesty (success replaces
/// the stub via prepend; failure restores it; "nothing older" removes
/// it).
fn dispatch_history_bloc(store: Store, ctx: &UiCtx, count: usize) {
    let older = store.older_turns.get_untracked();
    let Some(before) = store.history_cursor.get_untracked() else {
        return;
    };
    if older == 0 || store.history_loading.get_untracked() {
        // Only /history can reach the in-flight half of this guard (the
        // auto-load effect pre-checks the flag): a silent return read as
        // a dead keystroke — say what is happening instead.
        if older > 0 {
            store.notify("a history bloc is already streaming from the gateway");
        }
        return;
    }
    let streaming = count.min(older);
    store.history_loading.set(true);
    // The stub IS the progress surface — the user triggering this is
    // looking straight at it (top of the transcript).
    store.fold.update(|f| {
        if let Some(Item::Info { text }) = f.items.iter_mut().find(
            |i| matches!(i, Item::Info { text } if text.starts_with(crate::runner::OLDER_TURNS_STUB_PREFIX)),
        ) {
            *text = format!(
                "{}streaming {streaming} of {older} earlier turn(s)…)",
                crate::runner::OLDER_TURNS_STUB_PREFIX
            );
        }
    });
    store.notify(format!(
        "streaming {streaming} earlier turn(s) from the gateway…"
    ));
    if !ctx.send(Cmd::LoadHistory {
        session_id: store.session_id.get_untracked(),
        before,
        count,
    }) {
        // Dead worker: undo the armed state honestly (the runner will
        // never flip it back) — the flag AND the stub's "streaming…"
        // claim, which would otherwise freeze as a forever-lie.
        store.history_loading.set(false);
        store
            .fold
            .update(|f| crate::runner::restore_history_stub(f, older));
    }
}

/// Scroll-top auto-load (operator UX ruling, 2026-07-25: "if I scroll
/// up more than 5 turns, it should automatically load the past history
/// — possibly a waiting screen or progress to show something is
/// happening"). Reaching the TOP of a scrolled-up transcript with
/// older turns on the gateway dispatches the previous bloc; the stub
/// line becomes the progress indicator. Completion re-runs the effect
/// (older_turns/history_loading writes), so HOLDING at the top
/// cascades bloc-by-bloc until the session is fully loaded — each bloc
/// is a fresh network round-trip, which paces the cascade naturally.
/// Esc (jump to tail) exits at any time.
fn wire_history_autoload(
    cx: Scope,
    store: Store,
    ctx: &UiCtx,
    follow: Signal<bool>,
    scroll_offset: Signal<i32>,
) {
    /// "At the top" margin in rows — a near-top arrival counts (wheel
    /// momentum rarely lands exactly on 0).
    const TOP_MARGIN: i32 = 2;
    let ctx = ctx.clone();
    cx.effect(move || {
        let at_top = !follow.get() && scroll_offset.get() <= TOP_MARGIN;
        // Read the gates reactively so completions re-arm the cascade.
        if !at_top
            || store.history_loading.get()
            || store.restoring.get()
            || store.older_turns.get() == 0
        {
            return;
        }
        // Agent-lane only: entity conversations have no bloc history.
        if !matches!(store.focus.get(), crate::convo::Focus::Agent) {
            return;
        }
        dispatch_history_bloc(store, &ctx, ctx.replay_turns.max(1));
    });
}

/// `/gpu` — toggle the gateway-host GPU meter (OBS-6). OFF is the
/// default (zero polling); ON sets `Pending` and starts the poller on
/// its own thread (first sample fires immediately). The signal flips to
/// `Off` HERE, on the UI thread, before the disable command — the
/// poller's generation guard makes any in-flight sample's post a no-op,
/// so `Off` can never be overwritten by a stale reading.
fn toggle_gpu_meter(store: Store, ctx: &UiCtx) {
    if matches!(store.gpu.get_untracked(), crate::store::GpuMeter::Off) {
        store.gpu.set(crate::store::GpuMeter::Pending);
        if ctx.send(Cmd::GpuEnable) {
            store.notify(
                "GPU meter ON — gateway-host GPU polled ~3s active / ~30s idle · /gpu turns it off",
            );
        } else {
            // Dead worker: the poller never started — say so, honestly.
            store.gpu.set(crate::store::GpuMeter::Off);
            store.notify("GPU meter not started: the gateway worker is dead — restart the app");
        }
    } else {
        store.gpu.set(crate::store::GpuMeter::Off);
        ctx.send(Cmd::GpuDisable);
        store.notify("GPU meter OFF");
    }
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
    let old_sid = store.session_id.get_untracked();
    store.session_id.set(id.clone());
    persist_prefs(ctx, |p| {
        p.session_id = Some(id.clone());
        p.touch_session(&id, None);
    });
    // Ordering matters: session_id is already the target, so the queue
    // persistence write-through inside the shared reset files the loaded
    // stash under the right id.
    reset_session_state(
        store,
        ctx,
        &old_sid,
        &id,
        format!("session switched to {id} — durable memory continues on the gateway"),
    );
    if ctx.send(Cmd::ProbeAttach {
        session_id: id,
        replay_turns: ctx.replay_turns,
    }) {
        // Arm the loading screen NOW, on the UI thread: the worker may
        // be mid-fetch elsewhere, and the waiting surface must appear
        // the frame the picker closes, not when the worker reaches the
        // probe. The runner re-arms (idempotent) and every one of its
        // exit paths clears; a dead worker never arms a forever-lie.
        store.restoring.set(true);
        store.restore_progress.set(None);
    }
}

/// `/permissions [read|write|all]` — THE tool-permission surface (the
/// c5028 consolidation; replaces `/tools tier` and the deleted `/auto`
/// blanket). Batches whose every call classifies at-or-below the level
/// auto-approve (maintainer bug (a), 2026-07-22: "if the highest tier is
/// accepted, nothing is ever asked"). Bare reports; unknown spellings
/// refuse loudly.
fn set_permissions(store: Store, ctx: &UiCtx, arg: Option<String>) {
    use crate::tool_policy::Tier;
    match arg {
        None => {
            let current = Tier::parse_or_default(&store.accepted_tier.get_untracked());
            store.notify(format!(
                "permissions: {} — {} · /permissions <read|write|all> changes it",
                current.label(),
                current.description()
            ));
        }
        Some(raw) => match Tier::parse(&raw) {
            None => store.notify(format!(
                "unknown permissions {raw:?} — expected read, write, or all"
            )),
            Some(tier) => apply_permissions(store, ctx, tier),
        },
    }
}

pub(crate) fn apply_permissions(store: Store, ctx: &UiCtx, tier: crate::tool_policy::Tier) {
    store.accepted_tier.set(tier.label().to_string());
    // Per-session (operator ask): the level is part of "those preferences"
    // and sticks with the session it was set in.
    persist_tool_prefs(store, ctx, |p| p.accepted_tier = tier.label().to_string());
    store.notify(format!(
        "permissions: {} — {}",
        tier.label(),
        tier.description()
    ));
}

/// Cycle read → write → all → read (the /tools modal's `t` key).
pub fn cycle_permissions(store: Store, ctx: &UiCtx) {
    use crate::tool_policy::Tier;
    let next = match Tier::parse_or_default(&store.accepted_tier.get_untracked()) {
        Tier::Read => Tier::Write,
        Tier::Write => Tier::All,
        Tier::All => Tier::Read, // wrap is fail-safe: back to strictest
    };
    apply_permissions(store, ctx, next);
}

/// `/context [tokens|off]` — the OPERATOR-DECLARED context window
/// (CTX-0). Bare = report window + latest usage; a token count declares
/// (persisted); `off`/`0`/`clear` clears. The declaration drives the
/// footer's `ctx used/window (%)` meter and rides runs as
/// `_limits.max_tokens`. Source honesty: the label everywhere says
/// "declared" — this is the operator's statement, never a client-shipped
/// capability table (the Python predecessor's own first resolution rung
/// is exactly this).
/// `/iterations [N|off]` — the iteration budget this client REQUESTS.
///
/// The bare form answers the question the failure card raises and cannot
/// answer itself: *whose* number was that? This client asks for nothing by
/// default, so the budget a run actually gets is the server's, and on the
/// published `basic-agent` bundles the server's is a pin inside the bundle —
/// not a framework default anyone can read from here. So the report says
/// only what this client KNOWS (what it asks for, or that it asks for
/// nothing) and never invents the server's number. Fabricating a "current
/// budget" from a client-side table is the 2026-07-17 class exactly.
fn set_max_iterations(store: Store, ctx: &UiCtx, arg: Option<String>) {
    // A ceiling on what we will ASK for. The server clamps or refuses by its
    // own rules; this only stops a fat-fingered `/iterations 100000` from
    // riding out as a serious request.
    const MAX_REQUEST: u32 = 10_000;

    match arg {
        None => {
            let asked = store.max_iterations.get_untracked();
            if asked == 0 {
                store.notify(
                    "iteration budget: asking for nothing — the server's own applies \
                     (the same one every client gets) · /iterations <n> requests one",
                );
            } else {
                store.notify(format!(
                    "iteration budget: asking for {asked} on new runs \
                     (the server may clamp or refuse it) · /iterations off takes the server's"
                ));
            }
        }
        Some(w)
            if w.eq_ignore_ascii_case("off")
                || w.eq_ignore_ascii_case("clear")
                || w.trim() == "0" =>
        {
            store.max_iterations.set(0);
            persist_prefs(ctx, |p| p.max_iterations = 0);
            store.notify("iteration budget cleared — new runs take the server's own");
        }
        Some(raw) => match raw.trim().parse::<u32>() {
            Ok(n) if (1..=MAX_REQUEST).contains(&n) => {
                store.max_iterations.set(n);
                persist_prefs(ctx, |p| p.max_iterations = n);
                store.notify(format!(
                    "iteration budget: will ask for {n} — applies to the NEXT run, \
                     not the one in flight"
                ));
            }
            _ => store.notify(format!(
                "not an iteration count: {raw} — try a whole number 1-{MAX_REQUEST} \
                 (/iterations off takes the server's)"
            )),
        },
    }
}

fn set_context_window(store: Store, ctx: &UiCtx, arg: Option<String>) {
    use crate::ui::chrome::fmt_tokens;
    match arg {
        None => {
            let window = store.context_window.get_untracked();
            let used = store.fold.with_untracked(|f| f.stats.last_input_tokens);
            if window == 0 {
                let used_part = if used > 0 {
                    format!(" · latest call used {} tk", fmt_tokens(used))
                } else {
                    String::new()
                };
                store.notify(format!(
                    "context window not declared — /context <tokens> (e.g. /context 262k) sets it{used_part}"
                ));
            } else {
                let used_part = if used > 0 {
                    format!(
                        " · latest call used {} tk ({}%)",
                        fmt_tokens(used),
                        used.saturating_mul(100) / window.max(1)
                    )
                } else {
                    " · no call measured yet".to_string()
                };
                store.notify(format!(
                    "context window {} tk (declared){used_part} · /context off clears",
                    fmt_tokens(window)
                ));
            }
        }
        Some(w)
            if w.eq_ignore_ascii_case("off")
                || w.eq_ignore_ascii_case("clear")
                || w.trim() == "0" =>
        {
            store.context_window.set(0);
            persist_prefs(ctx, |p| p.context_window = 0);
            store.notify("context window cleared — ctx shows absolute tokens again");
        }
        Some(raw) => match crate::config::parse_token_count(&raw) {
            None => store.notify(format!(
                "not a token count: {raw} — try 262144, 262k, or 1m (/context off clears)"
            )),
            Some(n) => {
                store.context_window.set(n);
                persist_prefs(ctx, |p| p.context_window = n);
                store.notify(format!(
                    "context window declared: {} tk — the footer shows ctx used/window (%), warns ≥75%",
                    fmt_tokens(n)
                ));
            }
        },
    }
}

/// Global fallback actions on the engine keymap (HDR-2a, modal half).
///
/// Modal trees swallow EVERY key they route — consumed or not — before
/// root-tree shortcuts (engine overlay dispatch), and registered actions
/// run LAST, only for keys nothing in the UI consumed. So the root
/// shortcut answers the normal case, this action answers the open-modal
/// case, and the pair can never double-fire (a consumed chord never
/// reaches the keymap). Ctrl+L must survive an open modal because a
/// wiped screen with an approval prompt up is exactly when recovery
/// matters most (the prompt is also invisible then).
///
/// Shared between `run_tui` (production boot) and the headless harness,
/// so tests exercise the same registration the app ships.
pub fn register_global_actions(actions: &abstracttui::app::Actions) {
    // `register` refuses name/chord collisions (returns false) — there
    // are none today; a future collision surfaces in the Ctrl+L test.
    let _ = actions.register(
        "redraw",
        Some(KeyChord::new(Mods::CTRL, Key::Char('l'))),
        abstracttui::app::request_full_redraw,
    );
}

/// Ctrl+C, owned app-wide (operator ruling 2026-07-23): the FIRST press
/// clears the draft (if any) and ARMS quit; a SECOND press within the
/// window quits — two consecutive Ctrl+C are always required to leave.
///
/// Registered as a GLOBAL ACTION, not a root-tree shortcut: unconsumed
/// keys — including keys a MODAL tree ignored — reach the actions
/// registry BEFORE the engine's default Ctrl+C-quits rule, so this
/// registration permanently shadows the instant-quit default everywhere
/// (a root-tree shortcut would cover only the main tree, and a modal-open
/// Ctrl+C would still have insta-quit through the engine default).
///
/// The window is 2s — wider than the Esc-Esc cancel arm's 900ms because
/// the second press follows READING the arm notice, not a reflex
/// double-tap. Mid-selection, the selection layer still owns Ctrl+C
/// (release-copy clears the region, so the NEXT press reaches us) —
/// select mode is an explicit gesture and copy is its contract.
fn wire_ctrl_c(
    cx: Scope,
    actions: &abstracttui::app::Actions,
    store: Store,
    ctx: &UiCtx,
    composer: &abstracttui::widgets::TextAreaState,
) {
    let armed: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let quit_ctx = ctx.clone();
    let composer = composer.clone();
    let _ = actions.register(
        "clear-or-quit",
        Some(KeyChord::new(Mods::CTRL, Key::Char('c'))),
        move || {
            let now = Instant::now();
            let within = armed
                .get()
                .map(|t| now.duration_since(t) <= Duration::from_millis(2000))
                .unwrap_or(false);
            if within {
                // The quit gate: idle quits instantly; a live run opens
                // the modal (repeat presses resolve to leave & quit —
                // hammering always exits, never cancels). Global action:
                // fires even with the modal open (modal trees ignore
                // Ctrl+C; unconsumed keys reach the actions registry).
                quit::request_quit(cx, store, &quit_ctx);
                return;
            }
            let had_draft = !composer.text().is_empty();
            if had_draft {
                composer.clear();
            }
            armed.set(Some(now));
            store.notify(if had_draft {
                "prompt cleared — Ctrl+C again to quit"
            } else {
                "press Ctrl+C again to quit"
            });
        },
    );
}

/// OBS-1a-live wiring: measure the newest COMPLETED llm_call's
/// throughput client-side. On the inflight Some→None transition with a
/// grown call count, rate = the cumulative-OUTPUT delta across the
/// transition over the client-observed started→completed window.
/// `stats.output_tokens` accumulates only genuine output (splitless
/// receipts add 0 there — the sparkline's total-tokens substitution
/// never reaches this numerator), so a splitless call yields honest
/// ABSENCE instead of dividing prompt+output by wall time (cycle-2
/// review P1-A: the old `output_series.last()` numerator overstated
/// splitless-provider throughput ~130×). Network + ledger latency ride
/// the denominator, so a measured rate slightly UNDERSTATES provider
/// tok/s — the conservative direction; every render labels it
/// "(last call)". Worker-1 seam: reads `llm_inflight_since` + stats
/// only (additive read, never a fold write).
fn wire_llm_meter(cx: Scope, store: Store) {
    let prev_start: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let prev_calls: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let prev_out: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    cx.effect(move || {
        let (inflight, calls, out_now) = store.fold.with(|f| {
            (
                f.llm_inflight_since,
                f.stats.llm_calls,
                f.stats.output_tokens,
            )
        });
        if inflight.is_none() {
            if let Some(start) = prev_start.get() {
                if calls > prev_calls.get() {
                    let secs = start.elapsed().as_secs_f64();
                    // saturating: a session switch zeroes the fold's stats
                    // while prev_out still holds the old cumulative.
                    let delta = out_now.saturating_sub(prev_out.get());
                    if secs > 0.05 && delta > 0 {
                        store.last_call_rate.set(Some(delta as f64 / secs));
                    }
                }
            }
        }
        prev_start.set(inflight);
        prev_calls.set(calls);
        // Unconditional: mid-flight usage from OTHER lanes is absorbed as
        // it folds, so only usage landing in the same reactive batch as
        // the transition (the completing call's own receipt) counts.
        prev_out.set(out_now);
    });
}

/// Toggle transcript VERBOSITY (operator directive 2026-08-19): full =
/// tool args + result bodies + expanded thinking; folded = one-line
/// tool calls with status tags + thinking gists. Thinking and every
/// called tool stay visible in BOTH states.
/// pub(crate): modal layers swallow keys before root shortcuts (engine
/// overlay dispatch), so modals promising Ctrl+D must bind it themselves.
pub(crate) fn toggle_details(store: Store, ctx: &UiCtx) {
    let now = !store.show_details.get_untracked();
    store.show_details.set(now);
    persist_prefs(ctx, |p| p.show_details = Some(now));
    store.notify(if now {
        "details: full — tool args + results, thinking expanded (Ctrl+D folds)"
    } else {
        "details: folded — one-line tool calls with status tags, thinking gists (Ctrl+D expands)"
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
    // The animation pane exits FIRST and CONSUMES the press. No command
    // sets `store.animation` today (see
    // `docs/backlog/proposed/ambient-run-animations.md`), so this rung is
    // inert — it stays because it is the rung that makes the feature SAFE
    // to switch back on: Esc here is already quadruple-loaded (clear
    // draft, jump to tail, arm cancel, fire cancel), and a user tapping
    // it twice to get the words back must never reach "cancel the run".
    // Clearing `last_esc` on the way out is the second half of that.
    if store.animation.get_untracked() > 0 {
        store.animation.set(0);
        store.last_esc.set(None);
        return;
    }
    if !composer.text().is_empty() {
        composer.clear();
        return;
    }
    // Jump-to-tail CONSUMES the press (visibility review P2-8): a user
    // double-tapping Esc from scrollback to "get back down" must never
    // arm-and-fire run-cancel — the visibility-restoring gesture cannot
    // be allowed to destroy a 9-minute run. The next Esc (already at
    // the tail) arms cancel as before.
    if !follow.get_untracked() {
        follow.set(true);
        return;
    }
    follow.set(true); // jump back to the tail
                      // Entity focus consumes the cancel arm: an entity turn is
                      // non-interruptible (honest notice, never a fake cancel), and arming
                      // an AGENT cancel under entity focus would target a run the user is
                      // not even looking at.
    if entity_actions::escape_in_entity_focus(store) {
        return;
    }
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

/// Persist the tools-modal config PER SESSION (operator ask 2026-07-23:
/// "those preferences should be sticky per session"). The slot is built
/// from the live store signals (the caller has already applied the edit
/// to the signal), the `mutate` overriding the specific field for
/// explicitness. Mirrors to the global baseline too, so a brand-new
/// session seeds from the latest setup and legacy/headless readers of the
/// top-level fields keep working.
pub fn persist_tool_prefs(
    store: Store,
    ctx: &UiCtx,
    mutate: impl FnOnce(&mut crate::config::SessionToolPrefs),
) {
    let sid = store.session_id.get_untracked();
    let mut slot = crate::config::SessionToolPrefs {
        disabled_tools: store.disabled_tools.get_untracked(),
        tool_overrides: store.tool_overrides.get_untracked(),
        accepted_tier: store.accepted_tier.get_untracked(),
    };
    mutate(&mut slot);
    let mut prefs = ctx.prefs.borrow_mut();
    prefs.set_session_tool_prefs(&sid, &slot);
    prefs.disabled_tools = slot.disabled_tools.clone();
    prefs.tool_overrides = slot.tool_overrides.clone();
    prefs.tool_accepted_tier = slot.accepted_tier.clone();
    let _ = prefs.save();
}

/// Load a session's remembered tools-modal config into the store signals
/// (session switch / boot). A session with a saved slot loads it exactly;
/// a fresh session seeds from the global baseline and arms the camera
/// default-off seed (item 3) for when the inventory arrives. Borrow of
/// `ctx.prefs` is released BEFORE the signal writes — a store `.set` can
/// flush effects that re-borrow prefs (BorrowError otherwise).
/// Seed the tool-pref SIGNALS for `sid` from prefs — the ONE authority
/// for the slot semantics (saved slot wins; fresh seeds from the global
/// baseline; a blank tier never overwrites; `camera_seed_pending` arms on
/// fresh). Returns `fresh`. Takes `&Prefs` (not `&UiCtx`) so BOOT can
/// call it before a ctx exists — the boot path used to carry an inlined
/// mirror of this logic, and the two copies had already drifted on the
/// blank-tier rule (the consolidation survey's P2-2).
pub fn seed_tool_pref_signals(store: Store, prefs: &crate::config::Prefs, sid: &str) -> bool {
    let (slot, fresh) = match prefs.session_tool_prefs(sid) {
        Some(tp) => (tp, false),
        None => (
            crate::config::SessionToolPrefs {
                disabled_tools: prefs.disabled_tools.clone(),
                tool_overrides: prefs.tool_overrides.clone(),
                accepted_tier: prefs.tool_accepted_tier.clone(),
            },
            true,
        ),
    };
    store.disabled_tools.set(slot.disabled_tools);
    store.tool_overrides.set(slot.tool_overrides);
    if !slot.accepted_tier.trim().is_empty() {
        store.accepted_tier.set(slot.accepted_tier);
    }
    store.camera_seed_pending.set(fresh);
    fresh
}

pub fn load_session_tool_prefs(store: Store, ctx: &UiCtx, sid: &str) {
    let fresh = {
        let prefs = ctx.prefs.borrow();
        seed_tool_pref_signals(store, &prefs, sid)
    };
    // On a live switch the inventory is already loaded, so the tools-load
    // effect won't re-fire — seed now. On boot (tools not yet loaded) this
    // is a no-op and the effect does it when the inventory arrives.
    if fresh {
        seed_camera_off_if_pending(store, ctx);
    }
}

/// Seed camera tools OFF for a session that has no saved slot (one-shot,
/// idempotent via the pending flag). Shared by the tools-load effect and
/// the session-switch load so either trigger order works.
fn seed_camera_off_if_pending(store: Store, ctx: &UiCtx) {
    if !store.camera_seed_pending.get_untracked() {
        return;
    }
    let camera: Vec<String> = store.tools.with_untracked(|tools| {
        tools
            .iter()
            .filter(|t| t.toolset == "camera" && !t.served_disabled)
            .map(|t| t.name.clone())
            .collect()
    });
    if store.tools.with_untracked(|t| t.is_empty()) {
        return; // inventory not loaded yet — the effect will seed later
    }
    // Consume the one-shot flag once the inventory is known.
    store.camera_seed_pending.set(false);
    if camera.is_empty() {
        return;
    }
    let mut disabled = store.disabled_tools.get_untracked();
    let mut added = false;
    for name in camera {
        if !disabled.contains(&name) {
            disabled.push(name);
            added = true;
        }
    }
    if added {
        store.disabled_tools.set(disabled.clone());
        persist_tool_prefs(store, ctx, |p| p.disabled_tools = disabled.clone());
    }
}

/// Camera tools are OFF by default (operator ask 2026-07-23: privacy —
/// photo/video capture should never be silently available). The seed is
/// per-session and one-shot: it fires once the inventory loads for a
/// session that has no saved tool-prefs slot, adds the camera toolset's
/// grantable names to `disabled_tools`, and clears the pending flag — so
/// a user who then ENABLES camera keeps it (the slot is authoritative;
/// the seed never re-applies). NOTE: this is the CLIENT half; the server
/// half (camera served default-off in the gateway/workflow defaults) is
/// the gateway/camera seat's — filed separately.
fn wire_camera_default_off(cx: Scope, store: Store, ctx: UiCtx) {
    cx.effect(move || {
        store.tools.with(|_| {}); // tracked dep: re-run when the inventory (re)loads
        seed_camera_off_if_pending(store, &ctx);
    });
}

// ---------------------------------------------------------------------------
// Timers + effects
// ---------------------------------------------------------------------------

/// Spinner + elapsed ticker (engine `reactive::interval`, cancellable):
/// alive only while a run is active, so an idle app stays zero-wakeup
/// (the engine's idle guarantee). The phase effect starts/cancels the
/// interval; a suspended terminal coalesces missed ticks (no catch-up).
///
/// The ~5s chrome self-heal heartbeat (HDR-2b) that used to ride this
/// ticker is DELETED with abstracttui 0.2.6 (our 0299 filing): the
/// boot-time `set_redraw_on_focus_gained(true)` heals an externally
/// cleared screen at the next focus round-trip, and Ctrl+L/`/redraw`
/// (`request_full_redraw`, real poison-prev + presenter-invalidate
/// semantics — images re-place, the transcript pane heals too) covers
/// the rest. The per-5s veiled re-emission and all its measured limits
/// (chrome-band-only scope, protocol-image decay) die with it.
fn spawn_run_ticker(cx: Scope, store: Store, spin: Signal<u64>) {
    let handle: Rc<RefCell<Option<abstracttui::reactive::IntervalHandle>>> =
        Rc::new(RefCell::new(None));
    cx.effect(move || {
        // Entity turns need the spinner + elapsed display too.
        let active = store.phase.get() != Phase::Idle || entity_actions::any_convo_active(store);
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

/// The `/stance` breath ticker: armed ONLY while the figure is showing
/// AND a run is live — the line view is static text, and a finished run
/// does not breathe (the honesty gate the whole widget rests on). The
/// engine's zero-wakeup idle guarantee is untouched everywhere else.
fn wire_stance_ticker(cx: Scope, store: Store, mode: Signal<u8>, frame: Signal<u64>) {
    let handle: Rc<RefCell<Option<abstracttui::reactive::IntervalHandle>>> =
        Rc::new(RefCell::new(None));
    cx.effect(move || {
        let on = mode.get() == stance::FIGURE && store.phase.get() != Phase::Idle;
        let mut slot = handle.borrow_mut();
        if !on {
            if let Some(h) = slot.take() {
                h.cancel();
            }
            return;
        }
        if slot.is_some() {
            return;
        }
        frame.set(0);
        *slot = Some(abstracttui::reactive::interval(
            cx,
            Duration::from_millis(220),
            move || frame.update(|f| *f = f.wrapping_add(1)),
        ));
    });
}

/// The splash animation ticker (IDLE-2): ~150ms frames for the logo
/// shimmer, armed ONLY while the splash predicate holds. The moment a
/// conversation starts (or focus leaves the agent lane) the interval
/// cancels and the app returns to zero idle wakeups — the splash is
/// the ONE deliberate exception to that guarantee, and it dies with
/// the first user card. 150ms is the spinner's cadence class: slow
/// enough to cost nothing measurable, fast enough that the sweep reads
/// as motion instead of stutter. On a DEAD gateway (Conn::Down — the
/// screen that may sit unattended for hours) the cadence halves to
/// 300ms: the sweep still reads, the wakeups halve (refinement pass).
/// Each arm resets the frame to 0 so every splash entrance replays the
/// same fade-up + sweep (deterministic re-entry, not a mid-phase jump).
fn wire_splash_ticker(
    cx: Scope,
    store: Store,
    splash_visible: abstracttui::reactive::Memo<bool>,
    splash: Signal<u64>,
) {
    let handle: Rc<RefCell<Option<abstracttui::reactive::IntervalHandle>>> =
        Rc::new(RefCell::new(None));
    // Track the period the live interval was armed with, so a conn flip
    // re-arms at the new cadence instead of being ignored.
    let armed_period: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    cx.effect(move || {
        let on = splash_visible.get();
        // Tracked: a Down→Ok flip mid-splash re-runs this effect and
        // re-arms at the right cadence.
        let period_ms: u64 = if matches!(store.conn.get(), Conn::Down(..)) {
            300
        } else {
            150
        };
        let mut slot = handle.borrow_mut();
        if on {
            if slot.is_some() && armed_period.get() == period_ms {
                return; // already ticking at the right cadence
            }
            if let Some(h) = slot.take() {
                h.cancel();
            } else {
                // A fresh splash ENTRANCE (not a cadence re-arm):
                // replay the fade-up from frame 0.
                splash.set(0);
            }
            armed_period.set(period_ms);
            *slot = Some(abstracttui::reactive::interval(
                cx,
                Duration::from_millis(period_ms),
                move || splash.update(|f| *f = f.wrapping_add(1)),
            ));
        } else if let Some(h) = slot.take() {
            h.cancel();
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

/// F1 catalog self-heal: the boot sends `LoadCatalog`/`LoadTools` ONCE —
/// a gateway that was down at launch left the app refusing every task
/// FOREVER ("no agent workflows") while the orb promised reconnection.
/// Re-issue the pair on every `Conn::Down → Conn::Ok` EDGE. Edge-
/// triggered by construction: `conn` writes go through `set_if_changed`
/// everywhere, so Ok→Ok probes never re-run this effect, and the local
/// was-down latch means only a genuine flip fires the reload. The
/// preferred workflow re-resolves from prefs (the boot's own source), so
/// the heal lands on the saved selection; `load_catalog` itself never
/// clobbers a selection the user made while offline.
///
/// Takes `tx`/`prefs` rather than `UiCtx` so the headless test needs no
/// overlay/quitter scaffolding.
fn wire_conn_self_heal(
    cx: Scope,
    store: Store,
    tx: Sender<Cmd>,
    prefs: Rc<RefCell<Prefs>>,
    replay_turns: usize,
) {
    let was_down = Rc::new(Cell::new(false));
    cx.effect(move || {
        let conn = store.conn.get();
        let now_down = matches!(conn, Conn::Down(..));
        let before = was_down.replace(now_down);
        if before && conn == Conn::Ok {
            // The restore is retried too, not just the catalog. A
            // failed rehydration is exactly the thing a reconnection
            // fixes, and telling the operator to `/sessions` and
            // re-select by hand (which the old error card did) asked
            // them to do what this edge can do for them.
            if store.restore_failed.get_untracked().is_some() {
                let _ = tx.send(Cmd::ProbeAttach {
                    session_id: store.session_id.get_untracked(),
                    replay_turns,
                });
            }
            let (preferred_bundle, preferred_flow) = {
                let p = prefs.borrow();
                (p.bundle_id.clone(), p.flow_id.clone())
            };
            let _ = tx.send(Cmd::LoadCatalog {
                preferred_bundle,
                preferred_flow,
            });
            let _ = tx.send(Cmd::LoadTools);
            store.notify("gateway is back — refreshing the catalog");
        }
    });
}

/// OBS-6 cadence hint: mirror "is anything running" (agent phase or an
/// entity turn) into the GPU poller's atomic — the poller thread cannot
/// read signals, and its cadence (~3s active / ~30s idle) keys on this.
fn wire_gpu_cadence(cx: Scope, store: Store) {
    cx.effect(move || {
        let active = store.phase.get() != Phase::Idle || entity_actions::any_convo_active(store);
        crate::gateway::gpu::set_fast(active);
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
///
/// Approval waits resume WITHOUT a prompt under ONE admission (the c5028
/// consolidation — the /auto blanket is DELETED; its three latent holes
/// died with it: ask-pin bypass, served-disabled-clamp bypass,
/// empty-batch auto-approve): the PERSISTED permissions level — every
/// call in the batch classifies at-or-below it (`/permissions`,
/// per-session in prefs.json). Pins beat the level in both directions;
/// unrecognized names classify All and clear only at `all` (the standing
/// 2026-07-22 ruling — a deliberate choice, labeled, never a blind
/// blanket).
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
                if let WaitKind::Approval { tool_calls } = &wait.kind {
                    // TRACKED read: raising the level while a prompt is up
                    // re-decides it immediately (the maintainer's "if the
                    // highest tier is accepted, nothing is ever asked").
                    let accepted = store.accepted_tier.get();
                    let overrides = store.tool_overrides.get_untracked();
                    // Prefer the gateway's served facts when the
                    // discovery inventory carried them; else the name
                    // table. Unrecognized names classify All (fail
                    // closed) and clear only at `all` — the deliberate
                    // 2026-07-22 ruling, which is why the deleted /auto
                    // blanket's unrecognized clamp has no lane left to
                    // gate (its labeling half lives on in the approval
                    // modal body).
                    let classes = store.tool_classes();
                    let tier_ok = crate::tool_policy::batch_auto_approves_with(
                        tool_calls, &accepted, &overrides, &classes,
                    );
                    if tier_ok && auto_answered.borrow().as_deref() != Some(wait.step_id.as_str()) {
                        *auto_answered.borrow_mut() = Some(wait.step_id.clone());
                        auto_approve_wait(
                            store,
                            &ctx,
                            &wait,
                            "within the accepted permissions — /permissions changes it",
                        );
                        // If the prompt for THIS occurrence is already on
                        // screen (tier raised while it was up), close it
                        // here: the fold write above is this effect's OWN
                        // dependency write, so it does not re-schedule the
                        // effect — the pending-None close branch would
                        // otherwise wait for an unrelated signal change
                        // (live finding 2026-07-22: Resume fired, modal
                        // stayed).
                        if ctx.wait_modal_for.borrow().as_deref() == Some(wait.step_id.as_str()) {
                            ctx.close_modal();
                        }
                        return;
                    }
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

/// Resume an approval wait without prompting (the permissions policy).
/// Same optimistic bookkeeping as the modal's approve path; a refused
/// resume restores the wait and the effect falls back to the modal.
/// `why` names the admitting rule in the toast — an invisible approval
/// must still say which policy spoke for it — and the resume payload
/// carries `approved_by: "policy"` + the rule (R3, c5028): a policy
/// auto-click must be ledger-distinguishable from a human decision.
fn auto_approve_wait(store: Store, ctx: &UiCtx, wait: &PendingWait, why: &str) {
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
        f.mark_wait_tools(true);
        // The promptless lane (the permissions belt) is exactly where a
        // slow approved batch would otherwise run clockless —
        // indistinguishable from the no-wait path (adversary P1-1).
        f.tool_resumed(&wait.run_id);
    });
    ctx.send(Cmd::Resume {
        run_id: wait.run_id.clone(),
        wait_key: wait.wait_key.clone(),
        payload: serde_json::json!({
            "approved": true,
            "approved_by": "policy",
            "rule": why,
        }),
        approved: Some(true),
        restore: Box::new(wait.clone()),
    });
    let summary = if names.is_empty() {
        "tool batch".to_string()
    } else {
        names.join(", ")
    };
    store.notify(format!("auto-approved: {summary} ({why})"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// The composer hint teaches the BEST newline chord per terminal
    /// (0295): Shift+Enter only where the kitty keyboard protocol is
    /// actually live; Ctrl+J (the universal LF-byte chord) everywhere
    /// else. Starting-phase text carries no chord (guidance buffering
    /// is the teaching there).
    #[test]
    fn agent_placeholder_derives_the_newline_chord_from_caps() {
        assert!(agent_placeholder(Phase::Idle, false).contains("Ctrl+J newline"));
        assert!(agent_placeholder(Phase::Idle, true).contains("Shift+Enter newline"));
        assert!(!agent_placeholder(Phase::Idle, true).contains("Ctrl+J"));
        assert!(agent_placeholder(Phase::Running, false).contains("Ctrl+J newline"));
        assert!(agent_placeholder(Phase::Running, true).contains("Shift+Enter newline"));
        assert!(!agent_placeholder(Phase::Starting, true).contains("newline"));
    }

    /// F1 headless proof: LoadCatalog + LoadTools re-issue EXACTLY ONCE
    /// per Down→Ok flip — never on a healthy boot (Unknown→Ok), never on
    /// repeated Ok, and again on the NEXT flip.
    #[test]
    fn conn_self_heal_reissues_catalog_once_per_down_ok_flip() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = Store::create(cx);
            let (tx, rx) = mpsc::channel::<Cmd>();
            let prefs = Prefs {
                bundle_id: Some("basic-agent".into()),
                flow_id: Some("81795ea9".into()),
                ..Default::default()
            };
            wire_conn_self_heal(cx, store, tx, Rc::new(RefCell::new(prefs)), 20);

            // Healthy boot (Unknown → Ok): the boot sequence already
            // loads; the heal must stay quiet.
            store.conn.set(Conn::Ok);
            assert!(rx.try_recv().is_err(), "no re-issue on a healthy boot");

            // Gateway down at launch (or lost): entering Down is quiet.
            store
                .conn
                .set(Conn::Down("connection refused".into(), true));
            assert!(rx.try_recv().is_err(), "going down issues nothing");
            // A different Down message is still down (no flip) — including
            // an evidence downgrade (gone → soft-threshold timeout).
            store.conn.set(Conn::Down("timed out".into(), false));
            assert!(rx.try_recv().is_err());

            // The flip: Down → Ok re-issues the pair, exactly once,
            // carrying the saved workflow preference.
            store.conn.set(Conn::Ok);
            match rx.try_recv() {
                Ok(Cmd::LoadCatalog {
                    preferred_bundle,
                    preferred_flow,
                }) => {
                    assert_eq!(preferred_bundle.as_deref(), Some("basic-agent"));
                    assert_eq!(preferred_flow.as_deref(), Some("81795ea9"));
                }
                other => panic!("expected LoadCatalog, got {other:?}"),
            }
            assert!(
                matches!(rx.try_recv(), Ok(Cmd::LoadTools)),
                "LoadTools rides the same flip"
            );
            assert!(rx.try_recv().is_err(), "exactly once per flip");

            // A second flip fires again (each outage heals independently).
            store.conn.set(Conn::Down("gone again".into(), true));
            store.conn.set(Conn::Ok);
            assert!(matches!(rx.try_recv(), Ok(Cmd::LoadCatalog { .. })));
            assert!(matches!(rx.try_recv(), Ok(Cmd::LoadTools)));
            assert!(rx.try_recv().is_err());
        });
        root.dispose();
    }
}
