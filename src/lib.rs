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

pub mod cli;
pub mod commands;
pub mod config;
pub mod exec;
pub mod gateway;
pub mod protocol;
pub mod run_input;
pub mod runner;
pub mod store;
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

    let conn = config::resolve_connection(args.gateway.as_deref(), args.token.as_deref());
    let gateway_label = conn
        .base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_string();

    // Session: flag > saved > minted. A fresh mint is saved immediately so
    // the next launch continues the same conversation.
    let session_id = args
        .session
        .clone()
        .or_else(|| prefs.session_id.clone())
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
    let max_iterations = args.max_iterations;
    let args_replay_turns = args.replay_turns;

    let client = gateway::GatewayClient::new(&conn.base_url, conn.token.as_deref());
    let attach_session = session_id.clone();

    let mut app = App::new(Size::new(120, 36));
    // Engine screen-text selection (0.2.0, the feature filed as 0270):
    // left-drag paints a selection over the rendered text, release (or
    // Enter/c/Ctrl+C) copies via OSC 52, Esc/click clears. Always-on:
    // left-drag has no other meaning in this app, and wheel scrolling is
    // untouched by it. Native terminal selection stays one Shift/Option
    // drag away (the engine's troubleshooting doc has the matrix).
    abstracttui::app::selection::selection().set_enabled(true);
    let overlays = app.overlays();
    let quitter = app.quitter();
    let (tx, rx) = mpsc::channel::<runner::Cmd>();
    let shutdown_tx = tx.clone();
    let mut rx_slot = Some(rx);

    let mount_result = app.mount(move |cx| {
        let store = store::Store::create(cx);
        cx.provide_context(store);
        store.session_id.set(session_id.clone());
        store.provider.set(provider.clone());
        store.model.set(model.clone());
        if let Some(details) = show_details_pref {
            store.show_details.set(details);
        }
        // Persisted capability selections (the /tools + /skills pickers).
        store.disabled_tools.set(prefs.disabled_tools.clone());
        store.selected_skills.set(prefs.skills.clone());

        let wake = abstracttui::reactive::wake_handle();
        let rx = rx_slot.take().expect("mount runs once");
        runner::spawn(client.clone(), wake, store, tx.clone(), rx);

        // Boot sequence: probe, load the catalog (+ saved workflow), and
        // reattach to a live run of this session if one exists.
        let _ = tx.send(runner::Cmd::Probe);
        let _ = tx.send(runner::Cmd::LoadCatalog {
            preferred_bundle: pref_bundle.clone(),
            preferred_flow: pref_flow.clone(),
        });
        let _ = tx.send(runner::Cmd::LoadTools);
        let _ = tx.send(runner::Cmd::LoadSkills);
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
            overlays: overlays.clone(),
            quitter: quitter.clone(),
            prefs: Rc::new(RefCell::new(prefs)),
            workspace_root,
            workspace_mode,
            max_iterations,
            replay_turns: args_replay_turns,
            gateway_label,
            modal: Rc::new(RefCell::new(None)),
            modal_epoch: cx.signal(0u64),
            dismissed_wait: Rc::new(RefCell::new(None)),
            wait_modal_for: Rc::new(RefCell::new(None)),
        };
        ui::root(cx, store, ctx)
    });
    if let Err(e) = mount_result {
        eprintln!("abstractcode-tui: mount failed: {e:?}");
        return 1;
    }
    // Initial focus comes from the composer's `.autofocus()` (0.2.0 fires
    // it correctly inside dyn regenerations too — no focus bookkeeping).
    let outcome = app.run();
    // Stop the worker + any live streams before leaving (the process would
    // reap them anyway; being explicit keeps shutdown race-free).
    let _ = shutdown_tx.send(runner::Cmd::Shutdown);
    match outcome {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("abstractcode-tui: {e:?}");
            1
        }
    }
}
