//! abstractcode-tui — AbstractCode on AbstractTUI, speaking to AbstractGateway.
//!
//! The agent runs durably on the gateway; this crate is a reactive terminal
//! client: it starts runs, streams ledgers live, renders cycles/tools/
//! answers, resolves approval + ask-user waits, steers mid-run, and keeps a
//! durable session (server-side history replay).
//!
//! Layer map:
//! - [`config`]: connection + preference resolution (login store shared with
//!   the Python `abstractcode` CLI).
//! - [`gateway`]: blocking HTTP client + SSE ledger streaming.
//! - [`protocol`]: pure extraction over ledger records (waits, tools, usage,
//!   flow output).
//! - [`transcript`]: the fold from records to transcript items, stats, and
//!   pending waits.
//! - [`store`]: the reactive signal store (UI thread owns all writes).
//! - [`runner`]: the worker thread owning every HTTP call and stream.
//! - [`ui`]: AbstractTUI views (chrome, transcript, modals).
//! - [`cli`], [`exec`]: argument parsing, doctor/login, headless one-shots.
//! - [`export`]: `/export` renderers (archival markdown + SFT JSONL).

pub mod cli;
pub mod commands;
pub mod config;
pub mod convo;
pub mod discovery;
pub mod entities;
pub mod exec;
pub mod export;
pub mod gateway;
pub mod mention;
pub mod paths;
pub mod preview;
pub mod project_context;
pub mod protocol;
pub mod run_input;
pub mod runner;
pub mod store;
pub mod tool_policy;
pub mod transcript;
pub mod ui;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

use abstracttui::prelude::*;

use crate::transcript::Item;

/// Full CLI entry: parse args, route subcommands, run the TUI. Returns the
/// process exit code.
pub fn run_cli(argv: &[String]) -> i32 {
    let args = match cli::parse(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("abstractcode-tui: {e}");
            eprintln!("{}", cli::usage());
            return 2;
        }
    };
    if args.show_help {
        println!("{}", cli::usage());
        return 0;
    }
    if args.show_version {
        println!("abstractcode-tui {}", cli::VERSION);
        return 0;
    }
    if args.show_caps {
        println!(
            "{}",
            abstracttui::term::Capabilities::detect_env().summary()
        );
        return 0;
    }
    match args.subcommand.as_deref() {
        Some("login") => cli::login(&args),
        Some("doctor") => cli::doctor(&args),
        Some("exec") => exec::run(&args),
        _ => run_tui(&args),
    }
}

/// Boot-time render policy, extracted so a test can pin it (the
/// adversary's P2-4: the opt-in lived only inside `run_tui`, which no
/// headless test executes — deleting it was caught by nothing).
///
/// Auto-heal on focus regain (0.2.6, our 0299 ask 2): an externally
/// cleared screen (Cmd+K, `printf '\033c'`) fixes itself at the next
/// focus round-trip — this replaces the old ~5s chrome heartbeat
/// entirely. Ctrl+L / /redraw remain the explicit verbs; terminals
/// without DEC 1004 focus reporting (tmux without `focus-events on`)
/// only get those, which docs/troubleshooting.md says honestly.
fn apply_boot_render_policy() {
    abstracttui::app::set_redraw_on_focus_gained(true);
}

/// Launch-animation policy, extracted so a test can pin it (the
/// `apply_boot_render_policy` pattern): `--animation` SETS AND PERSISTS
/// the choice — the operator turns the animation off once, not once per
/// launch — and absent the flag, the saved preference stands, with
/// never-chosen reading as ON. The engine's boot gate (tty, `NO_COLOR`,
/// `TERM=dumb`, `ABSTRACTTUI_NO_SPLASH`, dumb caps) applies on top
/// inside `ui::splash::play_boot`, so it can only ever say no harder.
fn resolve_animation(prefs: &mut config::Prefs, flag: Option<bool>) -> bool {
    if let Some(on) = flag {
        prefs.animation = Some(on);
    }
    prefs.animation.unwrap_or(true)
}

fn run_tui(args: &cli::Args) -> i32 {
    if !abstracttui::term::have_tty() {
        eprintln!("abstractcode-tui: needs an interactive terminal (use `exec` for headless runs)");
        return 2;
    }

    // Theme: flag > env (engine convention) > saved pref.
    let mut prefs = config::Prefs::load();
    let start_theme = args
        .theme
        .clone()
        .or_else(|| std::env::var("ABSTRACTTUI_THEME").ok())
        .or_else(|| prefs.theme.clone());
    if let Some(id) = start_theme {
        if !abstracttui::app::set_theme_by_id(&id) {
            eprintln!("abstractcode-tui: unknown theme {id:?} — using the default");
        }
    }

    // Launch animation. Resolved (and persisted) BEFORE the session
    // write below, which is the save that carries it to disk.
    let animation = resolve_animation(&mut prefs, args.animation);

    let conn = config::resolve_connection(args.gateway.as_deref(), args.token.as_deref());
    let gateway_label = conn
        .base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_string();

    // Session: explicit `--session` > explicit `--resume` (last saved)
    // > a FRESH mint. Launch starts a NEW conversation by default
    // (operator ruling 2026-07-26: "launching code-tui should start on
    // a new session and not automatically reattach to the last one");
    // continuity is always an explicit act — the flags here or the
    // in-app `/sessions` picker. The minted id is still saved so
    // `--resume` and the picker know what "last" was.
    let session_id = args
        .session
        .clone()
        .or_else(|| {
            if args.resume {
                prefs.session_id.clone()
            } else {
                None
            }
        })
        .unwrap_or_else(config::mint_session_id);
    prefs.session_id = Some(session_id.clone());
    prefs.touch_session(&session_id, None);
    let _ = prefs.save();

    // Workflow preference: flag > saved (resolution happens against the
    // live catalog inside the runner).
    let (pref_bundle, pref_flow) = match args.workflow.as_deref() {
        Some(raw) => {
            let (b, f) = cli::split_workflow_ref(raw);
            (Some(b), f)
        }
        None => (prefs.bundle_id.clone(), prefs.flow_id.clone()),
    };

    let workspace_root = if args.no_workspace {
        None
    } else {
        args.workspace.clone().or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.display().to_string())
        })
    };
    let workspace_mode = args
        .workspace_mode
        .clone()
        .or_else(|| prefs.workspace_mode.clone());

    let show_details_pref = prefs.show_details;
    let provider = args
        .provider
        .clone()
        .or_else(|| prefs.provider.clone())
        .unwrap_or_default();
    let model = args
        .model
        .clone()
        .or_else(|| prefs.model.clone())
        .unwrap_or_default();
    // Reasoning effort: flag > pair-coupled pref (a persisted effort
    // applies only under the provider/model it was saved with — a
    // route change resets it, the first-citizen coupling rule).
    let reasoning = args
        .reasoning
        .clone()
        .unwrap_or_else(|| config::coupled_reasoning(&prefs, &provider, &model));
    // Operator-declared context window (CTX-0): flag (session-scoped) >
    // persisted /context declaration. 0 = undeclared (honest absolute
    // ctx display; no `_limits` on the wire).
    let context_window = if args.max_tokens > 0 {
        args.max_tokens
    } else {
        prefs.context_window
    };
    let max_iterations = args.max_iterations;
    let args_replay_turns = args.replay_turns;

    let client = gateway::GatewayClient::new(&conn.base_url, conn.token.as_deref());
    let attach_session = session_id.clone();

    let mut app = App::new(Size::new(120, 36));
    // Engine screen-text selection (0.2.0, the feature filed as 0270):
    // left-drag paints a selection over the rendered text, release (or
    // Enter/c/Ctrl+C) copies via OSC 52, Esc/click clears. Always on —
    // since abstracttui 0.2.8 (our first-app/0285) the selection layer
    // claims a gesture only once it DRAGS, so plain clicks pass through
    // to buttons everywhere, modals included; this boot enable is the
    // ONLY writer (the open_modal/close_modal suspend toggles the app
    // carried while 0285 was open are deleted). Native terminal
    // selection stays one Shift/Option drag away (the engine's
    // troubleshooting doc has the matrix).
    abstracttui::app::selection::selection().set_enabled(true);
    apply_boot_render_policy();
    let overlays = app.overlays();
    let quitter = app.quitter();
    // Ctrl+L must survive an OPEN MODAL (HDR-2a): see
    // `ui::register_global_actions` for the routing rationale.
    ui::register_global_actions(&app.actions());
    // The actions handle rides into root() for the Ctrl+C clear-or-quit
    // registration (needs the composer state, which exists only there).
    let actions = app.actions();
    let (tx, rx) = mpsc::channel::<runner::Cmd>();
    let shutdown_tx = tx.clone();
    let mut rx_slot = Some(rx);

    // Queue quit-echo mirror (courtesy): the queue PERSISTS per session
    // (prefs write-through) and restores PAUSED on the next launch — the
    // quit line says so where the user can still read it (post-teardown
    // stderr; an in-altscreen eprintln is invisible). Mirrored
    // continuously because signals die with the app's reactive root and
    // cannot be read after run() returns.
    let queue_echo: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let queue_echo_ui = queue_echo.clone();
    // Quit-outcome echo (quit-modal design §3.6): the run/quit state at
    // teardown, mirrored continuously (signals die with the reactive
    // root). `None` = quit was silent (idle) — no line.
    let quit_echo: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let quit_echo_ui = quit_echo.clone();

    let mount_result = app.mount(move |cx| {
        let store = store::Store::create(cx);
        cx.provide_context(store);
        cx.effect(move || {
            let texts: Vec<String> = store
                .queue
                .with(|q| q.iter().map(|p| ui::queue_preview(&p.text)).collect());
            *queue_echo_ui.borrow_mut() = texts;
        });
        cx.effect(move || {
            let state = store.quit_state.get();
            let phase = store.phase.get();
            let run_id = store.run_id.get();
            // The ack-quit path posts its own line: a Delivering state
            // that the sequencer resolved with quitter.quit() means the
            // verb was CONFIRMED — mirror the acked wording. The
            // sequencer clears verb_ack on match, so "Delivering at
            // teardown with no ack" reads as not-confirmed (quit-anyway).
            *quit_echo_ui.borrow_mut() = ui::quit::quit_echo_line(&state, phase, &run_id);
        });
        store.session_id.set(session_id.clone());
        store.provider.set(provider.clone());
        store.model.set(model.clone());
        store.reasoning.set(reasoning.clone());
        // Verifier-before-conclude: `--review`/`--no-review` seeds the
        // session; `/review` retunes it. The default is ON (see
        // `cli::DEFAULT_REVIEW_MODE`).
        store
            .review_mode
            .set(args.review.unwrap_or(cli::DEFAULT_REVIEW_MODE));
        store.review_rounds.set(args.review_rounds);
        store.context_window.set(context_window);
        if let Some(details) = show_details_pref {
            store.show_details.set(details);
        }
        // Persisted skill selection (global; the /skills picker).
        store.selected_skills.set(prefs.skills.clone());
        // Entity roster: last-good cache loads instantly ('@' completion +
        // /entities work offline); one async refresh follows below.
        let (cached_roster, roster_as_of) = entities::load_cached_roster();
        store.entities.set(cached_roster);
        store.entities_as_of.set(roster_as_of);
        // Tools-modal config is STICKY PER SESSION (operator ask): the ONE
        // slot authority (`ui::seed_tool_pref_signals`) seeds the signals;
        // camera-default-off arms via the pending flag and fires when the
        // inventory loads (the tools-load effect — no ctx exists yet here).
        let _fresh = ui::seed_tool_pref_signals(store, &prefs, &session_id);
        // Live workspace scope (seeded from flags/prefs; /workspace edits).
        // The signal is the ONE authority — UiCtx carries no copy.
        store.workspace_mode.set(workspace_mode.unwrap_or_default());
        store.workspace_allowed.set(prefs.workspace_allowed.clone());

        let wake = abstracttui::reactive::wake_handle();
        let rx = rx_slot.take().expect("mount runs once");
        let ui_client = client.clone();
        runner::spawn(
            client.clone(),
            wake,
            store,
            tx.clone(),
            rx,
            args.workflow.clone(),
        );

        // Boot sequence: probe, load the catalog (+ saved workflow), and
        // reattach to a live run of this session if one exists.
        let _ = tx.send(runner::Cmd::Probe);
        let _ = tx.send(runner::Cmd::LoadCatalog {
            preferred_bundle: pref_bundle.clone(),
            preferred_flow: pref_flow.clone(),
        });
        let _ = tx.send(runner::Cmd::LoadTools);
        let _ = tx.send(runner::Cmd::LoadSkills);
        // MCP registry at boot (HDR-1/REST-1): the header + footer carry
        // `mcp N` — without this load the count existed only after /mcp
        // was opened once. One cheap GET.
        let _ = tx.send(runner::Cmd::LoadMcp);
        let _ = tx.send(runner::Cmd::LoadEntities);
        let _ = tx.send(runner::Cmd::ProbeAttach {
            session_id: attach_session.clone(),
            replay_turns: args_replay_turns,
        });

        store.fold.update(|f| {
            f.push_item(Item::Info {
                text: format!("session {session_id} · durable memory lives on the gateway"),
            })
        });

        let ctx = ui::UiCtx {
            tx,
            client: ui_client.clone(),
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            prefs: Rc::new(RefCell::new(prefs)),
            workspace_root,
            max_iterations,
            max_iterations_explicit: args.max_iterations_explicit,
            no_project_context: args.no_project_context,
            no_prompt_cache: args.no_prompt_cache,
            replay_turns: args_replay_turns,
            gateway_label,
            modal: Rc::new(RefCell::new(None)),
            modal_epoch: cx.signal(0u64),
            dismissed_wait: Rc::new(RefCell::new(None)),
            wait_modal_for: Rc::new(RefCell::new(None)),
        };
        // Cycle-2 queue persistence: restore this session's stash PAUSED
        // (a restore never auto-starts — the one rule tying quit/reopen
        // and session switches together), plus the recorded goal (its
        // strip label; `wire_goal` restores `finish_on_root_only` when
        // the reattach probe lands on the goal run).
        ui::restore_session_queue(store, &ctx, &session_id);
        let goal = ctx
            .prefs
            .borrow()
            .session_goal(&session_id)
            .map(|(text, run_id)| store::GoalState { text, run_id });
        store.goal.set(goal);
        ui::root(cx, store, ctx, &actions)
    });
    if let Err(e) = mount_result {
        eprintln!("abstractcode-tui: mount failed: {e:?}");
        return 1;
    }
    // The boot animation, between mount and run: mounting has already
    // handed the probe/catalog/tools/entities fetches to the worker
    // thread, so the ~1.9s identity plays while they land instead of
    // being added to the time before first paint. Any key skips it; a
    // non-tty, NO_COLOR, TERM=dumb, ABSTRACTTUI_NO_SPLASH or
    // `--animation off` skips it silently (a launch animation that
    // prints an explanation is worse than no launch animation).
    let _ = ui::splash::play_boot(animation);
    // Initial focus comes from the composer's `.autofocus()` (0.2.0 fires
    // it correctly inside dyn regenerations too — no focus bookkeeping).
    let outcome = app.run();
    // Stop the worker + any live streams before leaving (the process would
    // reap them anyway; being explicit keeps shutdown race-free).
    let _ = shutdown_tx.send(runner::Cmd::Shutdown);
    // Queue persistence honesty (cycle-2: REVERSED from drop-on-quit —
    // the queue is saved per session by write-through): a courtesy line
    // where the user can still read it (post-teardown stderr).
    {
        let leftover = queue_echo.borrow();
        if !leftover.is_empty() {
            eprintln!(
                "abstractcode-tui: {} queued prompt(s) saved with this session — they restore PAUSED on the next launch (/queue then r resumes):",
                leftover.len()
            );
            for text in leftover.iter() {
                eprintln!("  · {text}");
            }
        }
    }
    // Quit-outcome honesty (quit-modal design §3.6; audit P1: the
    // mirror existed but was never READ — a successful pause-then-quit
    // exited with no confirmation at all). One line, post-teardown,
    // where the user can still read it.
    if let Some(line) = quit_echo.borrow().as_ref() {
        eprintln!("abstractcode-tui: {line}");
    }
    match outcome {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("abstractcode-tui: {e:?}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_animation;
    use crate::config::Prefs;

    /// The flag decides AND sticks; without it the saved preference
    /// decides; never-chosen is ON. (The seam this pins is the one that
    /// would otherwise fail silently: a launch flag that changes this
    /// run but forgets to persist looks identical at launch and wrong on
    /// the next one.)
    #[test]
    fn animation_flag_sets_persists_and_defaults_on() {
        let mut fresh = Prefs::default();
        assert!(
            resolve_animation(&mut fresh, None),
            "never chosen reads as on"
        );
        assert_eq!(fresh.animation, None, "no flag writes no preference");

        let mut off = Prefs::default();
        assert!(!resolve_animation(&mut off, Some(false)));
        assert_eq!(off.animation, Some(false), "--animation off is PERSISTED");
        // Next launch, no flag: the saved choice still governs.
        assert!(!resolve_animation(&mut off, None));
        // And it is reversible from the same surface.
        assert!(resolve_animation(&mut off, Some(true)));
        assert_eq!(off.animation, Some(true));
    }

    /// The focus-gained auto-heal opt-in is load-bearing boot policy
    /// (it REPLACED the deleted ~5s chrome heartbeat): pin that the
    /// boot policy actually sets the engine flag, so removing the call
    /// can never ship silently. Thread-local state — asserted in the
    /// same thread that applies it.
    #[test]
    fn boot_render_policy_opts_into_focus_gained_redraw() {
        assert!(
            !abstracttui::app::redraw_on_focus_gained(),
            "engine default is OFF (the opt-in below is what ships it)"
        );
        super::apply_boot_render_policy();
        assert!(
            abstracttui::app::redraw_on_focus_gained(),
            "boot policy enables the focus-gained auto-heal"
        );
        // Restore the thread-local (the engine suites' discipline):
        // harmless today — each test runs on a fresh thread — but a
        // future same-thread sibling asserting the default must not
        // inherit an ordering dependency.
        abstracttui::app::set_redraw_on_focus_gained(false);
    }
}
