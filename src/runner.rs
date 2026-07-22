//! The worker: owns the gateway client, run streams, and every HTTP call.
//!
//! Threading contract (the engine rule): the UI thread owns all signals;
//! this module's threads never touch them directly — they post closures
//! through `WakeHandle`, and those closures run on the UI thread. Commands
//! flow the other way through an mpsc channel. Stale-stream protection:
//! posted record closures re-check `fold.is_following(run_id)` before
//! applying, so streams from an abandoned run can never contaminate a new
//! one.
//!
//! Live-data lane decision (0.2.0 evaluation, deliberate): the engine's
//! `channel_source`/`bounded_source` bind homogeneous DATA streams to a
//! `Signal<Vec<T>>` with overflow policies. Ledger records are not that:
//! they are ordered STATE DELTAS folded into `Fold` (a dropped record is
//! a lost tool result or a lost wait — `DropOldest`/`Coalesce` would be
//! silent corruption, and an unbounded `channel_source` accumulates into
//! a Vec nobody reads). `wake.post` of fold closures IS the sanctioned
//! transport for this shape, and 0.2.0's waker dedup already coalesces
//! bursts into one wake. What we did adopt from the wave: panic
//! surfacing on every worker thread (`catch_unwind` + a posted error —
//! `reactive::spawn_worker` itself binds the CURRENT thread's runtime,
//! so it cannot be called from the runner thread; this is its manual
//! twin over the runner's already-UI-bound handle).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use abstracttui::reactive::WakeHandle;
use serde_json::Value;

use crate::gateway::GatewayClient;
use crate::run_input::{build_input_data, StartOpts};
use crate::store::{
    CacheInfo, Conn, ImageEntry, McpServer, Phase, ProviderInfo, SessionTotals, SkillInfo, Store,
    ToolInfo, Workflow,
};
use crate::transcript::{FoldEffect, Item, PendingWait};

const ARTIFACT_MAX_BYTES: usize = 8 * 1024 * 1024;
/// Default prior-turn replay depth at boot (`--replay-turns` overrides).
/// A cap exists because each turn costs one history-bundle fetch carrying
/// the run tree's COMPLETE ledgers — deep sessions get the newest N in full
/// detail; older turns stay one `--replay-turns` bump away.
pub const REHYDRATE_DEFAULT_TURNS: usize = 20;

pub enum Cmd {
    Probe,
    LoadCatalog {
        preferred_bundle: Option<String>,
        preferred_flow: Option<String>,
    },
    LoadTools,
    /// Load the gateway skill shelf (for `/skills`).
    LoadSkills,
    /// Load the gateway MCP server registry (for `/mcp`).
    LoadMcp,
    /// Probe prompt-cache capability for a provider/model route. Empty
    /// strings mean "the effective default route" (resolved from the
    /// gateway's capability defaults first).
    LoadCacheInfo {
        provider: String,
        model: String,
    },
    Start {
        prompt: String,
        flow_id: String,
        bundle_id: String,
        session_id: String,
        /// Boxed: StartOpts carries vectors (messages/tools/skills) and
        /// would dominate the enum's size (clippy large-variant).
        opts: Box<StartOpts>,
    },
    /// Probe the session for a live run and attach to it if one exists —
    /// after REHYDRATING the session's prior turns from their run ledgers
    /// (quit/crash must come back to the same transcript).
    ProbeAttach {
        session_id: String,
        /// How many prior turns to replay in full detail (0 = none).
        replay_turns: usize,
    },
    /// Pause the active run tree durably (gateway `pause` command).
    Pause {
        run_id: String,
    },
    /// Resume a PAUSED run tree (distinct from answering a wait).
    ResumePaused {
        run_id: String,
    },
    Follow {
        root_run_id: String,
        run_id: String,
    },
    Resume {
        run_id: String,
        wait_key: String,
        payload: Value,
        approved: Option<bool>,
        restore: Box<PendingWait>,
    },
    Steer {
        run_id: String,
        text: String,
    },
    Cancel {
        run_id: String,
    },
    FetchImage {
        run_id: String,
        artifact_id: String,
    },
    /// The answer landed: stop following helper subruns (wrapper bundles can
    /// keep status-watcher subflows polling long after the agent finished).
    StopFollows,
    Shutdown,
}

struct Runner {
    client: GatewayClient,
    wake: WakeHandle,
    store: Store,
    tx: Sender<Cmd>,
    /// Stop flags for every live stream thread (flipped on new run/quit);
    /// the bool marks the root stream, which outlives `StopFollows`.
    stream_stops: Vec<(bool, Arc<AtomicBool>)>,
}

pub fn spawn(
    client: GatewayClient,
    wake: WakeHandle,
    store: Store,
    tx: Sender<Cmd>,
    rx: Receiver<Cmd>,
) -> std::thread::JoinHandle<()> {
    let panic_wake = wake.clone();
    std::thread::Builder::new()
        .name("gateway-runner".into())
        .spawn(move || {
            // Panic surfacing (the 0.2.0 worker discipline): a dead
            // command loop must say so on screen, never freeze silently.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let mut runner = Runner {
                    client,
                    wake,
                    store,
                    tx,
                    stream_stops: Vec::new(),
                };
                while let Ok(cmd) = rx.recv() {
                    if matches!(cmd, Cmd::Shutdown) {
                        runner.stop_streams();
                        break;
                    }
                    runner.handle(cmd);
                }
            }));
            if let Err(payload) = result {
                let msg = panic_text(payload.as_ref());
                panic_wake.post(move || {
                    store.fold.update(|f| {
                        f.push_item(Item::Error {
                            text: format!(
                                "gateway worker died: {msg} — restart the app to reconnect"
                            ),
                        })
                    });
                    store.notify("gateway worker died — restart the app");
                    // Degrade honestly: no command loop means no pause/
                    // cancel/steer can be delivered — a spinner claiming
                    // otherwise would lie (adversary finding 10). The run
                    // itself continues durably server-side.
                    store.phase.set(Phase::Idle);
                });
            }
        })
        .expect("spawn gateway-runner thread")
}

/// Best-effort extraction of a panic payload's message.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

impl Runner {
    fn stop_streams(&mut self) {
        for (_, stop) in self.stream_stops.drain(..) {
            stop.store(true, Ordering::Relaxed);
        }
    }

    fn stop_follow_streams(&mut self) {
        self.stream_stops.retain(|(is_root, stop)| {
            if !is_root {
                stop.store(true, Ordering::Relaxed);
            }
            *is_root
        });
    }

    fn post(&self, f: impl FnOnce() + Send + 'static) {
        self.wake.post(f);
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Shutdown => unreachable!("handled by the loop"),
            Cmd::Probe => self.probe(),
            Cmd::LoadCatalog {
                preferred_bundle,
                preferred_flow,
            } => self.load_catalog(preferred_bundle, preferred_flow),
            Cmd::LoadTools => self.load_tools(),
            Cmd::LoadSkills => self.load_skills(),
            Cmd::LoadMcp => self.load_mcp(),
            Cmd::LoadCacheInfo { provider, model } => self.load_cache_info(provider, model),
            Cmd::Start {
                prompt,
                flow_id,
                bundle_id,
                session_id,
                opts,
            } => self.start_run(prompt, flow_id, bundle_id, session_id, *opts),
            Cmd::ProbeAttach {
                session_id,
                replay_turns,
            } => self.probe_attach(&session_id, replay_turns),
            Cmd::Pause { run_id } => self.pause(run_id),
            Cmd::ResumePaused { run_id } => self.resume_paused(run_id),
            Cmd::Follow {
                root_run_id,
                run_id,
            } => self.spawn_stream(root_run_id, run_id, false),
            Cmd::Resume {
                run_id,
                wait_key,
                payload,
                approved,
                restore,
            } => self.resume(run_id, wait_key, payload, approved, *restore),
            Cmd::Steer { run_id, text } => self.steer(run_id, text),
            Cmd::Cancel { run_id } => self.cancel(run_id),
            Cmd::FetchImage {
                run_id,
                artifact_id,
            } => self.fetch_image(run_id, artifact_id),
            Cmd::StopFollows => self.stop_follow_streams(),
        }
    }

    fn probe(&self) {
        let store = self.store;
        match self.client.ping() {
            Ok(_) => self.post(move || {
                store.conn.set_if_changed(Conn::Ok);
            }),
            Err(e) => {
                let msg = e.to_string();
                self.post(move || {
                    store.conn.set_if_changed(Conn::Down(msg));
                });
            }
        }
    }

    fn load_catalog(&self, preferred_bundle: Option<String>, preferred_flow: Option<String>) {
        let store = self.store;
        match self.client.list_bundles() {
            Ok(v) => {
                let workflows = agent_workflows_from_bundles(&v);
                let chosen = choose_workflow(
                    &workflows,
                    preferred_bundle.as_deref(),
                    preferred_flow.as_deref(),
                );
                self.post(move || {
                    if let Some(w) = chosen {
                        // Never clobber a user selection made while loading.
                        if store.workflow.with(|cur| cur.flow_id.is_empty()) {
                            store.workflow.set(w);
                        }
                    }
                    store.workflows.set(workflows);
                    store.conn.set_if_changed(Conn::Ok);
                });
            }
            Err(e) => {
                let msg = e.to_string();
                self.post(move || {
                    store.conn.set_if_changed(Conn::Down(msg.clone()));
                    store.notify(format!("catalog load failed: {msg}"));
                });
            }
        }
        // Providers/models power the /model picker; failure is non-fatal.
        if let Ok(v) = self.client.discovery_providers(true) {
            let providers = providers_from_discovery(&v);
            // Provider-endpoint profiles can come back with models:[] from
            // the bulk route (fixed gateway-side 2026-07-22; the per-provider
            // route always served them). Backfill through the gateway's own
            // per-provider route — REUSED, never re-derived — bounded so a
            // wall of dead endpoints cannot stall the boot sequence.
            let empty: Vec<String> = providers
                .iter()
                .filter(|p| p.models.is_empty())
                .map(|p| p.name.clone())
                .take(6)
                .collect();
            self.post(move || store.providers.set(providers));
            for name in empty {
                if let Ok(mv) = self.client.provider_models(&name) {
                    let models = models_from_provider_models(&mv);
                    if models.is_empty() {
                        continue;
                    }
                    let n = name.clone();
                    self.post(move || {
                        store.providers.update(|list| {
                            if let Some(p) = list.iter_mut().find(|p| p.name == n) {
                                p.models = models;
                            }
                        });
                    });
                }
            }
        }
        // What "gateway defaults" resolves to (the console's text route) —
        // display honesty for the header + /cache. Failure is non-fatal.
        if let Ok(v) = self.client.capability_defaults() {
            if let Some((provider, model)) = default_text_route(&v) {
                let (p, m) = (provider.clone(), model.clone());
                self.post(move || store.default_route.set((p, m)));
                // Probe cache capability for the default route so `/cache`
                // answers immediately (a route override re-probes).
                let _ = self.tx.send(Cmd::LoadCacheInfo { provider, model });
            }
        }
        // Workspace honesty: when the gateway clamps client workspace scopes
        // (the default posture), files land in a gateway-managed workspace,
        // not the launch directory — say so up front instead of letting the
        // first tool result surprise the user.
        if let Ok(v) = self.client.workspace_policy() {
            let overrides = v
                .get("policy")
                .and_then(|p| p.get("client_workspace_scope_overrides"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !overrides {
                self.post(move || {
                    store.fold.update(|f| {
                        f.push_item(Item::Info {
                            text: "workspace: gateway-managed — files land in the gateway's workspace (details: /help)".into(),
                        })
                    });
                });
            }
        }
    }

    fn load_tools(&self) {
        let store = self.store;
        match self.client.discovery_tools() {
            Ok(v) => {
                let tools = tools_from_discovery(&v);
                self.post(move || {
                    store.tools.set(tools);
                    store.tools_error.set(String::new());
                });
            }
            Err(e) => {
                let msg = e.to_string();
                self.post(move || {
                    store.tools_error.set(msg.clone());
                    store.notify(format!("tool discovery failed: {msg}"));
                });
            }
        }
    }

    fn load_skills(&self) {
        let store = self.store;
        match self.client.skills() {
            Ok(v) => {
                let skills = skills_from_response(&v);
                self.post(move || {
                    store.skills_catalog.set(skills);
                    store.skills_error.set(String::new());
                });
            }
            Err(e) => {
                let msg = e.to_string();
                self.post(move || store.skills_error.set(msg));
            }
        }
    }

    fn load_mcp(&self) {
        let store = self.store;
        match self.client.mcp_servers() {
            Ok(v) => {
                let (servers, note) = mcp_from_response(&v);
                self.post(move || {
                    store.mcp_servers.set(servers);
                    store.mcp_note.set(note);
                });
            }
            Err(e) => {
                let msg = e.to_string();
                self.post(move || store.mcp_note.set(format!("discovery failed: {msg}")));
            }
        }
    }

    fn load_cache_info(&self, provider: String, model: String) {
        let store = self.store;
        // The default route substitutes only when there is NO override at
        // all. A provider-only override ("ollama · provider default model")
        // must probe THAT provider — substituting the gateway default here
        // paired one route's verdict with another route's label (adversary
        // finding 5).
        let (provider, model) = if provider.is_empty() && model.is_empty() {
            match self
                .client
                .capability_defaults()
                .ok()
                .and_then(|v| default_text_route(&v))
            {
                Some(route) => route,
                None => return, // no override and no default route: nothing to probe
            }
        } else {
            (provider, model)
        };
        match self.client.prompt_cache_capabilities(&provider, &model) {
            Ok(v) => {
                let caps = v.get("capabilities").unwrap_or(&Value::Null);
                let info = CacheInfo {
                    provider,
                    model,
                    supported: caps
                        .get("supported")
                        .and_then(Value::as_bool)
                        .or_else(|| v.get("supported").and_then(Value::as_bool))
                        .unwrap_or(false),
                    mode: caps
                        .get("mode")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                };
                self.post(move || store.cache.set(Some(info)));
            }
            Err(_) => {
                // Honest unknown: leave/clear the probe rather than invent.
                self.post(move || store.cache.set(None));
            }
        }
    }

    fn start_run(
        &mut self,
        prompt: String,
        flow_id: String,
        bundle_id: String,
        session_id: String,
        opts: StartOpts,
    ) {
        self.stop_streams();
        let store = self.store;
        let input = build_input_data(&prompt, &opts);
        let bundle = if bundle_id.trim().is_empty() {
            None
        } else {
            Some(bundle_id.as_str())
        };
        match self
            .client
            .start_run(&flow_id, bundle, Some(&session_id), input)
        {
            Ok(run_id) => {
                let rid = run_id.clone();
                self.post(move || {
                    store.run_id.set(rid.clone());
                    store.phase.set(Phase::Running);
                    store.run_started.set(Some(std::time::Instant::now()));
                    store.elapsed_secs.set(0);
                    store.paused.set_if_changed(false);
                    store.fold.update(|f| f.begin_run(&rid));
                    store.conn.set_if_changed(Conn::Ok);
                });
                self.spawn_stream(run_id.clone(), run_id, true);
            }
            Err(e) => {
                let msg = e.to_string();
                self.post(move || {
                    store.phase.set(Phase::Idle);
                    store.fold.update(|f| {
                        f.push_item(Item::Error {
                            text: format!("run start failed: {msg}"),
                        })
                    });
                    if msg.contains("unreachable") {
                        store.conn.set_if_changed(Conn::Down(msg));
                    }
                });
            }
        }
    }

    fn probe_attach(&mut self, session_id: &str, replay_turns: usize) {
        let store = self.store;
        // List enough roots to find the live run even when replay is off.
        let list_limit = replay_turns.clamp(1, 100).max(5) as u32;
        let v = match self.client.list_runs(session_id, list_limit) {
            Ok(v) => v,
            Err(_) => return,
        };
        let mut items = v
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        // Chronological: the gateway lists newest-first.
        items.sort_by(|a, b| {
            let ka = a.get("created_at").and_then(Value::as_str).unwrap_or("");
            let kb = b.get("created_at").and_then(Value::as_str).unwrap_or("");
            ka.cmp(kb)
        });

        // The LIVE run (if any) is excluded from rehydration — attaching
        // replays its whole tree anyway. Paused runs count as live
        // (resumable).
        let live_run_id = items
            .iter()
            .rev()
            .find(|r| {
                matches!(
                    r.get("status").and_then(Value::as_str).unwrap_or(""),
                    "running" | "waiting"
                )
            })
            .and_then(|r| r.get("run_id").and_then(Value::as_str))
            .map(str::to_string);

        // Rehydrate PRIOR turns in FULL DETAIL: fold each run tree's
        // bundled ledgers through the SAME fold the live stream uses, so a
        // restart renders exactly what the live session rendered (cycles,
        // tool cards, answers — the details toggle governs display). Built
        // worker-side into a fresh Fold, swapped in with one post.
        let mut fold = crate::transcript::Fold::new();
        let mut effects: Vec<FoldEffect> = Vec::new();
        let mut replayed = 0usize;
        if replay_turns > 0 {
            for run in &items {
                let rid = run.get("run_id").and_then(Value::as_str).unwrap_or("");
                if rid.is_empty() || Some(rid.to_string()) == live_run_id {
                    continue;
                }
                if let Ok(bundle) = self.client.history_bundle(rid, false, 0) {
                    let failed = matches!(
                        run.get("status").and_then(Value::as_str).unwrap_or(""),
                        "failed" | "cancelled"
                    );
                    if rehydrate_run_into(&mut fold, rid, &bundle, failed, &mut effects) {
                        replayed += 1;
                    }
                }
            }
        }
        if replayed > 0 {
            fold.push_item(Item::Info {
                text: format!(
                    "replayed {replayed} prior turn(s) in full detail from the gateway ledgers"
                ),
            });
            let session = fold.session;
            self.post(move || {
                store.fold.update(|f| {
                    // Prepend the restored history to whatever the boot
                    // already rendered (the session info line).
                    let mut restored = fold;
                    restored.items.append(&mut f.items);
                    *f = restored;
                });
                store.totals.set(SessionTotals {
                    input_tokens: session.input_tokens,
                    output_tokens: session.output_tokens,
                    runs: session.runs,
                });
            });
            // Images from prior turns re-render through the normal path.
            for fx in effects {
                if let FoldEffect::FetchImage {
                    run_id,
                    artifact_id,
                } = fx
                {
                    let _ = self.tx.send(Cmd::FetchImage {
                        run_id,
                        artifact_id,
                    });
                }
            }
        }

        if let Some(run_id) = live_run_id {
            let paused = items.iter().any(|r| {
                r.get("run_id").and_then(Value::as_str) == Some(run_id.as_str())
                    && r.get("paused").and_then(Value::as_bool).unwrap_or(false)
            });
            let rid = run_id.clone();
            self.post(move || {
                store.paused.set_if_changed(paused);
                store.notify(format!(
                    "reattaching to live run {}",
                    &rid[..rid.len().min(8)]
                ));
            });
            self.attach(run_id);
        }
    }

    fn pause(&self, run_id: String) {
        let store = self.store;
        match self.client.pause(&run_id) {
            Ok(_) => self.post(move || {
                store.paused.set(true);
                store.notify("run paused durably on the gateway — /resume continues it");
            }),
            Err(e) => {
                let msg = e.to_string();
                self.post(move || store.notify(format!("pause failed: {msg}")));
            }
        }
    }

    fn resume_paused(&self, run_id: String) {
        let store = self.store;
        match self.client.resume_paused(&run_id) {
            Ok(_) => self.post(move || {
                store.paused.set(false);
                store.notify("run resumed");
            }),
            Err(e) => {
                let msg = e.to_string();
                self.post(move || store.notify(format!("resume failed: {msg}")));
            }
        }
    }

    fn attach(&mut self, run_id: String) {
        self.stop_streams();
        let store = self.store;
        // The ledger replay carries cycles/tools/answers but NOT the user's
        // original prompt (a client-side card on the start path) — without
        // this fetch a restored turn showed an answer with no question.
        let prompt = self
            .client
            .input_data(&run_id)
            .ok()
            .and_then(|v| {
                ["input_data", ""]
                    .iter()
                    .find_map(|k| {
                        let node = if k.is_empty() { Some(&v) } else { v.get(*k) };
                        node.and_then(|n| n.get("prompt")).and_then(Value::as_str)
                    })
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let rid = run_id.clone();
        self.post(move || {
            store.run_id.set(rid.clone());
            store.phase.set(Phase::Running);
            store.run_started.set(Some(std::time::Instant::now()));
            store.fold.update(|f| {
                f.begin_run(&rid);
                if !prompt.trim().is_empty() {
                    f.push_item(Item::User {
                        text: prompt.trim().to_string(),
                    });
                }
            });
        });
        self.spawn_stream(run_id.clone(), run_id, true);
    }

    fn resume(
        &self,
        run_id: String,
        wait_key: String,
        payload: Value,
        approved: Option<bool>,
        restore: PendingWait,
    ) {
        let store = self.store;
        match self.client.resume(&run_id, &wait_key, payload) {
            Ok(_) => {
                let wk = wait_key.clone();
                let rid = run_id.clone();
                let step_id = restore.step_id.clone();
                self.post(move || {
                    store.fold.update(|f| {
                        // Guard: a slow resume outcome from a PREVIOUS run
                        // must not touch the current run's wait state.
                        if !f.is_following(&rid) {
                            return;
                        }
                        f.wait_answered(&wk, &step_id);
                        if let Some(approved) = approved {
                            f.mark_wait_tools(&wk, approved);
                        }
                    });
                });
            }
            Err(e) => {
                let msg = e.to_string();
                self.post(move || {
                    store.notify(format!("resume failed: {msg}"));
                    store.fold.update(|f| {
                        // Restore the prompt: the run is still waiting
                        // server-side (reopen_wait itself refuses runs the
                        // fold no longer follows).
                        f.reopen_wait(restore.clone());
                    });
                });
            }
        }
    }

    fn steer(&self, run_id: String, text: String) {
        let store = self.store;
        match self.client.steer(&run_id, &text) {
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                self.post(move || {
                    store.notify(format!("steer failed: {msg}"));
                    store.fold.update(|f| {
                        f.push_item(Item::Error {
                            text: format!("steer not delivered: {msg}"),
                        })
                    });
                });
            }
        }
    }

    fn cancel(&self, run_id: String) {
        let store = self.store;
        match self.client.cancel(&run_id) {
            Ok(_) => self.post(move || store.notify("cancel requested")),
            Err(e) => {
                let msg = e.to_string();
                self.post(move || store.notify(format!("cancel failed: {msg}")));
            }
        }
    }

    fn fetch_image(&self, run_id: String, artifact_id: String) {
        let store = self.store;
        let aid = artifact_id.clone();
        // UPSERT by artifact id, never push (`Store::upsert_image` — one
        // authority, unit-tested there): session revisits re-request the
        // same artifacts (the fold's seen_images resets), and append-only
        // entries both leaked bitmaps and let a transient error entry
        // permanently shadow a later successful fetch (`image_for`
        // returns the first match) — adversary finding 7, 2026-07-22.
        let upsert = move |entry: ImageEntry| store.upsert_image(entry);
        match self
            .client
            .artifact_bytes(&run_id, &artifact_id, ARTIFACT_MAX_BYTES)
        {
            Ok((bytes, _content_type)) => match abstracttui::gfx::decode_image(&bytes) {
                Ok(bitmap) => {
                    let bitmap = Arc::new(bitmap);
                    self.post(move || {
                        upsert(ImageEntry {
                            artifact_id: aid,
                            bitmap: Some(bitmap),
                            error: String::new(),
                        })
                    });
                }
                Err(e) => {
                    let msg = format!("image decode failed: {e}");
                    self.post(move || {
                        upsert(ImageEntry {
                            artifact_id: aid,
                            bitmap: None,
                            error: msg,
                        })
                    });
                }
            },
            Err(e) => {
                let msg = format!("image fetch failed: {e}");
                self.post(move || {
                    upsert(ImageEntry {
                        artifact_id: aid,
                        bitmap: None,
                        error: msg,
                    })
                });
            }
        }
    }

    /// Spawn a stream thread for one run of the active tree.
    fn spawn_stream(&mut self, root_run_id: String, run_id: String, is_root: bool) {
        let stop = Arc::new(AtomicBool::new(false));
        self.stream_stops.push((is_root, stop.clone()));
        let client = self.client.clone();
        let wake = self.wake.clone();
        let store = self.store;
        let tx = self.tx.clone();
        let name = format!("ledger-{}", &run_id[..run_id.len().min(8)]);
        let panic_wake = wake.clone();
        let panic_run = run_id.clone();
        let _ = std::thread::Builder::new().name(name).spawn(move || {
            // Panic surfacing: a dead ledger stream is a stuck-looking
            // run — say so instead of freezing silently.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                stream_run(client, wake, store, tx, root_run_id, run_id, is_root, stop);
            }));
            if let Err(payload) = result {
                let msg = panic_text(payload.as_ref());
                let short = panic_run[..panic_run.len().min(8)].to_string();
                panic_wake.post(move || {
                    store.notify(format!("ledger stream {short} died: {msg}"));
                });
            }
        });
    }
}

/// One run's streaming loop: SSE first, polling fallback, terminal detection.
#[allow(clippy::too_many_arguments)]
fn stream_run(
    client: GatewayClient,
    wake: WakeHandle,
    store: Store,
    tx: Sender<Cmd>,
    root_run_id: String,
    run_id: String,
    is_root: bool,
    stop: Arc<AtomicBool>,
) {
    let mut cursor: u64 = 0;
    let mut consecutive_errors: u32 = 0;
    let post_records = |cursor_records: Vec<Value>| {
        if cursor_records.is_empty() {
            return;
        }
        let rid = run_id.clone();
        let root = root_run_id.clone();
        let tx = tx.clone();
        wake.post(move || {
            let mut finished_now = false;
            let mut session = crate::transcript::SessionStats::default();
            // Stale-stream guard flag: the fold update below early-returns
            // for records from an abandoned run — and then NOTHING else may
            // touch signals either. (Review finding: totals.set ran
            // unconditionally, so one late batch from a dead stream zeroed
            // the session totals display with the default SessionStats.)
            let mut current = false;
            store.fold.update(|f| {
                if f.root_run_id() != root || !f.is_following(&rid) {
                    return; // stale stream from a previous run
                }
                current = true;
                let was_finished = f.finished;
                for rec in &cursor_records {
                    for fx in f.apply(&rid, rec) {
                        match fx {
                            FoldEffect::FollowRun(sub) => {
                                let _ = tx.send(Cmd::Follow {
                                    root_run_id: root.clone(),
                                    run_id: sub,
                                });
                            }
                            FoldEffect::FetchImage {
                                run_id,
                                artifact_id,
                            } => {
                                let _ = tx.send(Cmd::FetchImage {
                                    run_id,
                                    artifact_id,
                                });
                            }
                        }
                    }
                }
                finished_now = f.finished && !was_finished;
                session = f.session;
            });
            if !current {
                return; // stale stream: no signal may change
            }
            store.totals.set(SessionTotals {
                input_tokens: session.input_tokens,
                output_tokens: session.output_tokens,
                runs: session.runs,
            });
            if finished_now {
                // The turn's answer landed. Wrapper bundles may keep helper
                // subflows polling after this; release the composer now and
                // stop the helper streams (the root stream stays to observe
                // the eventual terminal state).
                store.phase.set(Phase::Idle);
                store.run_started.set(None);
                let _ = tx.send(Cmd::StopFollows);
            }
        });
    };

    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let outcome = client.stream_ledger(
            &run_id,
            cursor,
            &stop,
            |c| cursor = c.max(cursor),
            // One post per network read: live records reach the UI at
            // arrival cadence (batching across reads held the approval
            // modal hostage — live-verified failure).
            &post_records,
        );

        match outcome {
            Ok(true) => {
                // Gateway said done: the run is terminal. Report the root.
                finish(&client, &wake, &store, &root_run_id, &run_id, is_root);
                return;
            }
            Ok(false) => {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                consecutive_errors = 0;
                // Idle or clean close without done: check status, reconnect.
                match client.get_run(&run_id) {
                    Ok(v) => {
                        let status = v.get("status").and_then(Value::as_str).unwrap_or("");
                        if matches!(status, "completed" | "failed" | "cancelled") {
                            drain_rest(&client, &run_id, &mut cursor, &post_records);
                            finish(&client, &wake, &store, &root_run_id, &run_id, is_root);
                            return;
                        }
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(500)),
                }
            }
            Err(e) => {
                // Fatal HTTP statuses never heal by retrying: report and stop.
                if matches!(e.status, Some(401) | Some(403) | Some(404)) {
                    let msg = e.to_string();
                    wake.post(move || {
                        store.notify(format!("stream failed: {msg}"));
                        store.fold.update(|f| {
                            f.push_item(crate::transcript::Item::Error {
                                text: format!("ledger stream refused ({msg}) — check credentials (doctor/login)"),
                            });
                        });
                    });
                    if is_root {
                        finish(&client, &wake, &store, &root_run_id, &run_id, is_root);
                    }
                    return;
                }
                consecutive_errors += 1;
                if consecutive_errors == 1 {
                    let msg = e.to_string();
                    wake.post(move || {
                        store.conn.set_if_changed(Conn::Down(msg));
                    });
                }
                // Poll fallback: the run is durable server-side; keep folding
                // from the REST ledger until the stream comes back.
                for _ in 0..8 {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Ok((items, next)) = client.get_ledger(&run_id, cursor, 500) {
                        wake.post(move || {
                            store.conn.set_if_changed(Conn::Ok);
                        });
                        cursor = next;
                        post_records(items);
                        if let Ok(v) = client.get_run(&run_id) {
                            let status = v.get("status").and_then(Value::as_str).unwrap_or("");
                            if matches!(status, "completed" | "failed" | "cancelled") {
                                finish(&client, &wake, &store, &root_run_id, &run_id, is_root);
                                return;
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(
                        (500 * consecutive_errors as u64).min(5_000),
                    ));
                }
            }
        }
    }
}

fn drain_rest(
    client: &GatewayClient,
    run_id: &str,
    cursor: &mut u64,
    post_records: &impl Fn(Vec<Value>),
) {
    // Belt over the gateway's final drain: catch records appended in the
    // terminal-save window. Page until a short page — one page is not a
    // bound on the backlog.
    loop {
        match client.get_ledger(run_id, *cursor, 1000) {
            Ok((items, next)) => {
                *cursor = next;
                let short = items.len() < 1000;
                post_records(items);
                if short {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

fn finish(
    client: &GatewayClient,
    wake: &WakeHandle,
    store: &Store,
    root_run_id: &str,
    run_id: &str,
    is_root: bool,
) {
    if !is_root {
        return; // subrun streams end quietly; the root reports.
    }
    let status = client
        .get_run(run_id)
        .ok()
        .and_then(|v| v.get("status").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| "completed".into());
    let store = *store;
    let root = root_run_id.to_string();
    wake.post(move || {
        let mut current = false;
        store.fold.update(|f| {
            if f.root_run_id() != root {
                return; // a newer run took over; this outcome is history
            }
            current = true;
            f.run_terminal(&status);
        });
        if current {
            store.phase.set(Phase::Idle);
            store.run_started.set(None);
        }
    });
}

/// Fold one prior run tree (a `history_bundle`) into `fold` as a full-detail
/// turn: the user's prompt card, then the tree's ledgers root-first through
/// the normal fold (cycles, tool cards, answers — identical to the live
/// render). Terminal-run hygiene: an old run must never leave a pending
/// prompt behind, and an answerless failed run says so.
/// Returns true when the turn contributed anything.
pub fn rehydrate_run_into(
    fold: &mut crate::transcript::Fold,
    root_run_id: &str,
    bundle: &Value,
    failed: bool,
    effects_out: &mut Vec<FoldEffect>,
) -> bool {
    let before = fold.items.len();
    let prompt = bundle
        .get("input_data")
        .and_then(|i| i.get("prompt"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !prompt.is_empty() {
        fold.push_item(Item::User { text: prompt });
    }
    fold.begin_run(root_run_id);
    if let Some(ledgers) = bundle.get("ledgers").and_then(Value::as_object) {
        // Root first so the answer-source lanes are established; subruns
        // then in id order (per-run record order is what correctness needs —
        // each ledger array is already in ledger order).
        let mut keys: Vec<&String> = ledgers.keys().collect();
        keys.sort_by_key(|k| (k.as_str() != root_run_id, k.as_str().to_string()));
        for k in keys {
            // The live gateway wraps each run's records as
            // {run_id, total, items: [...]}; a bare array is tolerated for
            // older/other serializations (live shape verified 2026-07-21 —
            // the array-only reader silently folded NOTHING).
            let records = ledgers
                .get(k)
                .and_then(|v| v.get("items").or(Some(v)))
                .and_then(Value::as_array);
            if let Some(records) = records {
                for item in records {
                    // Each bundle item is a {cursor, record} ENVELOPE — the
                    // same wire shape the SSE stream sends per `step` event
                    // (abstractruntime's exporter wraps every ledger record;
                    // live-verified 2026-07-22). Folding the envelope itself
                    // rendered NOTHING: every status/effect/result read
                    // missed one level down. Bare records stay tolerated
                    // (older serializations, synthetic fixtures) — a real
                    // ledger record never carries a "record" key.
                    let rec = item.get("record").filter(|r| r.is_object()).unwrap_or(item);
                    for fx in fold.apply(k, rec) {
                        // FollowRun is meaningless here (the whole tree is
                        // in the bundle); images re-fetch normally.
                        if matches!(fx, FoldEffect::FetchImage { .. }) {
                            effects_out.push(fx);
                        }
                    }
                }
            }
        }
    }
    let has_answer = fold.items[before..].iter().any(|i| {
        matches!(
            i,
            Item::Assistant {
                final_answer: true,
                ..
            }
        )
    });
    if !has_answer && failed {
        fold.push_item(Item::Error {
            text: "(this turn ended without an answer)".into(),
        });
    }
    // A prior run is HISTORY: whatever wait it died holding must not prompt.
    fold.pending_wait = None;
    fold.activity.clear();
    fold.items.len() > before
}

// ---------------------------------------------------------------------------
// Catalog / discovery parsing (pure, tested)
// ---------------------------------------------------------------------------

pub const AGENT_INTERFACE_V1: &str = "abstractcode.agent.v1";

pub fn agent_workflows_from_bundles(v: &Value) -> Vec<Workflow> {
    let mut out = Vec::new();
    let items = match v.get("items").and_then(Value::as_array) {
        Some(i) => i,
        None => return out,
    };
    for b in items {
        let bundle_id = b
            .get("bundle_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if bundle_id.is_empty() {
            continue;
        }
        for ep in b
            .get("entrypoints")
            .and_then(Value::as_array)
            .unwrap_or(&Vec::new())
        {
            let interfaces = ep
                .get("interfaces")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let is_agent = interfaces
                .iter()
                .any(|i| i.as_str() == Some(AGENT_INTERFACE_V1));
            if !is_agent {
                continue;
            }
            if ep
                .get("deprecated")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let flow_id = ep
                .get("flow_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if flow_id.is_empty() {
                continue;
            }
            out.push(Workflow {
                bundle_id: bundle_id.to_string(),
                flow_id: flow_id.to_string(),
                name: ep
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(flow_id)
                    .trim()
                    .to_string(),
                description: ep
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            });
        }
    }
    out.sort_by_key(|a| a.label());
    out
}

pub fn choose_workflow(
    workflows: &[Workflow],
    preferred_bundle: Option<&str>,
    preferred_flow: Option<&str>,
) -> Option<Workflow> {
    if let (Some(b), Some(f)) = (preferred_bundle, preferred_flow) {
        if let Some(w) = workflows
            .iter()
            .find(|w| w.bundle_id == b && w.flow_id == f)
        {
            return Some(w.clone());
        }
    }
    if let Some(w) = workflows.iter().find(|w| w.bundle_id == "basic-agent") {
        return Some(w.clone());
    }
    workflows.first().cloned()
}

pub fn providers_from_discovery(v: &Value) -> Vec<ProviderInfo> {
    let mut out = Vec::new();
    let items = v
        .get("providers")
        .and_then(Value::as_array)
        .or_else(|| v.get("items").and_then(Value::as_array));
    for p in items.unwrap_or(&Vec::new()) {
        let name = p
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| p.get("id").and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let mut models = Vec::new();
        if let Some(list) = p.get("models").and_then(Value::as_array) {
            for m in list {
                let id = match m {
                    Value::String(s) => s.trim().to_string(),
                    other => other
                        .get("id")
                        .or_else(|| other.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                };
                if !id.is_empty() {
                    models.push(id);
                }
            }
        }
        out.push(ProviderInfo { name, models });
    }
    out
}

pub fn tools_from_discovery(v: &Value) -> Vec<ToolInfo> {
    let mut out = Vec::new();
    let items = v
        .get("tools")
        .and_then(Value::as_array)
        .or_else(|| v.get("items").and_then(Value::as_array));
    for t in items.unwrap_or(&Vec::new()) {
        let name = t
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let description = t
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        let toolset = ["toolset", "group", "category"]
            .iter()
            .find_map(|k| t.get(*k).and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_string();
        out.push(ToolInfo {
            name,
            description,
            toolset,
        });
    }
    // Group order first (files/web/system read naturally), name within.
    out.sort_by(|a, b| (&a.toolset, &a.name).cmp(&(&b.toolset, &b.name)));
    out
}

pub fn skills_from_response(v: &Value) -> Vec<SkillInfo> {
    let mut out = Vec::new();
    let items = v
        .get("skills")
        .and_then(Value::as_array)
        .or_else(|| v.get("items").and_then(Value::as_array));
    for s in items.unwrap_or(&Vec::new()) {
        let name = s
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        out.push(SkillInfo {
            name,
            description: s
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            trust: s
                .get("trust_level")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            blocked: s.get("blocked").and_then(Value::as_bool).unwrap_or(false),
        });
    }
    out.sort_by_key(|s| s.name.clone());
    out
}

/// MCP registry -> (servers, note). The note carries the gateway's own
/// empty-state guidance (how to declare a registry) or stays empty.
pub fn mcp_from_response(v: &Value) -> (Vec<McpServer>, String) {
    let mut out = Vec::new();
    for s in v
        .get("servers")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        let name = s
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        out.push(McpServer {
            name,
            url: s
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            description: s
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            auth_required: s
                .get("auth_required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    let note = v
        .get("warnings")
        .and_then(Value::as_array)
        .map(|w| {
            w.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();
    (out, note)
}

/// Model ids from the per-provider models route (`{models: [...]}`; entries
/// are strings, tolerating `{id|name}` objects like the bulk route).
pub fn models_from_provider_models(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for m in v
        .get("models")
        .and_then(Value::as_array)
        .or_else(|| v.get("items").and_then(Value::as_array))
        .unwrap_or(&Vec::new())
    {
        let id = match m {
            Value::String(s) => s.trim().to_string(),
            other => other
                .get("id")
                .or_else(|| other.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
        };
        if !id.is_empty() {
            out.push(id);
        }
    }
    out
}

/// The gateway's default text route: `output.text` first (what generates
/// replies), `input.text` as the fallback — mirroring the server's own
/// `resolve_gateway_provider_model` order.
pub fn default_text_route(v: &Value) -> Option<(String, String)> {
    let routes = v.get("routes").and_then(Value::as_array)?;
    for key in ["output.text", "input.text"] {
        for r in routes {
            if r.get("key").and_then(Value::as_str) != Some(key) {
                continue;
            }
            let provider = r
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let model = r
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if !provider.is_empty() && !model.is_empty() {
                return Some((provider, model));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn agent_workflows_filter_interface_and_deprecated() {
        let v = json!({"items": [
            {"bundle_id": "basic-agent", "entrypoints": [
                {"flow_id": "81795ea9", "name": "basic-agent", "interfaces": ["abstractcode.agent.v1"]}]},
            {"bundle_id": "coding-agent", "entrypoints": [
                {"flow_id": "coding-agent", "interfaces": ["abstractcode.coding.v1"]},
                {"flow_id": "coder", "name": "coder", "interfaces": ["abstractcode.agent.v1"]}]},
            {"bundle_id": "old", "entrypoints": [
                {"flow_id": "x", "interfaces": ["abstractcode.agent.v1"], "deprecated": true}]}
        ]});
        let flows = agent_workflows_from_bundles(&v);
        assert_eq!(flows.len(), 2);
        assert!(flows
            .iter()
            .any(|w| w.bundle_id == "coding-agent" && w.flow_id == "coder"));
    }

    #[test]
    fn workflow_choice_prefers_saved_then_basic_agent() {
        let flows = vec![
            Workflow {
                bundle_id: "coding-agent".into(),
                flow_id: "coder".into(),
                name: "coder".into(),
                description: String::new(),
            },
            Workflow {
                bundle_id: "basic-agent".into(),
                flow_id: "81795ea9".into(),
                name: "basic-agent".into(),
                description: String::new(),
            },
        ];
        let picked = choose_workflow(&flows, Some("coding-agent"), Some("coder")).unwrap();
        assert_eq!(picked.bundle_id, "coding-agent");
        let fallback = choose_workflow(&flows, None, None).unwrap();
        assert_eq!(fallback.bundle_id, "basic-agent");
        let missing_pref = choose_workflow(&flows, Some("gone"), Some("x")).unwrap();
        assert_eq!(missing_pref.bundle_id, "basic-agent");
    }

    #[test]
    fn provider_parsing_accepts_both_shapes() {
        let v = json!({"providers": [
            {"name": "lmstudio", "models": ["qwen3-4b", {"id": "gpt-oss-120b"}]},
            {"id": "ollama", "models": []}
        ]});
        let providers = providers_from_discovery(&v);
        assert_eq!(providers.len(), 2);
        assert_eq!(
            providers[0].models,
            vec!["qwen3-4b".to_string(), "gpt-oss-120b".to_string()]
        );
        assert_eq!(providers[1].name, "ollama");
    }

    #[test]
    fn tool_parsing_takes_first_description_line() {
        let v = json!({"tools": [
            {"name": "write_file", "description": "Write a file.\nLong details.", "toolset": "files"},
            {"name": "read_file"}
        ]});
        let tools = tools_from_discovery(&v);
        // Ungrouped ("") sorts before "files".
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[1].name, "write_file");
        assert_eq!(tools[1].description, "Write a file.");
        assert_eq!(tools[1].toolset, "files");
    }

    #[test]
    fn skills_parsing_carries_trust_and_blocked() {
        let v = json!({"skills": [
            {"name": "coredoc", "description": "Docs.\nMore.", "trust_level": "adopted"},
            {"name": "sketchy", "trust_level": "unknown", "blocked": true},
            {"name": ""}
        ]});
        let skills = skills_from_response(&v);
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "coredoc");
        assert_eq!(skills[0].trust, "adopted");
        assert!(!skills[0].blocked);
        assert!(skills[1].blocked);
    }

    #[test]
    fn mcp_parsing_keeps_servers_and_empty_state_note() {
        let (servers, note) = mcp_from_response(&json!({
            "servers": [{"name": "kb", "url": "http://x/mcp", "auth_required": true}],
            "warnings": []
        }));
        assert_eq!(servers.len(), 1);
        assert!(servers[0].auth_required);
        assert!(note.is_empty());
        let (none, hint) = mcp_from_response(&json!({
            "servers": [],
            "warnings": ["no MCP server registry declared (create mcp_servers.json)"]
        }));
        assert!(none.is_empty());
        assert!(hint.contains("mcp_servers.json"));
    }

    #[test]
    fn rehydrate_folds_full_detail_and_leaves_no_pending_prompt() {
        // A prior turn's bundle: prompt + a tree whose agent subrun carries
        // the cycle, a tool that DIED AWAITING APPROVAL (cancelled run), and
        // the answer. Full detail must render; the dead wait must NOT
        // prompt; the answer-source logic must find the subrun's answer.
        // Live bundle shape (verified 2026-07-21): each run's ledger is a
        // WRAPPED object {run_id, total, items: [...]} — the first reader
        // took bare arrays only and silently folded nothing.
        let bundle = json!({
            "input_data": {"prompt": "add a test"},
            "ledgers": {
                "root1": {"run_id": "root1", "total": 1, "items": [
                    {"run_id": "root1", "node_id": "n1", "status": "waiting",
                     "effect": {"type": "start_subworkflow", "payload": {}},
                     "result": {"wait": {"reason": "subworkflow",
                                          "details": {"sub_run_id": "agent1"}}}}
                ]},
                "agent1": {"run_id": "agent1", "total": 4, "items": [
                    // Real ledgers carry started+completed pairs; the
                    // STARTED reason record is what establishes the
                    // answer-source agent lane.
                    {"run_id": "agent1", "node_id": "reason", "status": "started",
                     "effect": {"type": "llm_call", "payload": {}}},
                    {"run_id": "agent1", "node_id": "reason", "status": "completed",
                     "effect": {"type": "llm_call", "payload": {}},
                     "result": {"content": "thinking about tests", "model": "m1",
                                 "usage": {"input_tokens": 900, "output_tokens": 12}}},
                    {"run_id": "agent1", "node_id": "act", "status": "waiting", "step_id": "s9",
                     "effect": {"type": "tool_calls",
                                 "payload": {"tool_calls": [{"name": "write_file"}]}},
                     "result": {"wait": {"reason": "user", "wait_key": "tool_approval:z",
                        "details": {"mode": "approval_required",
                                     "tool_calls": [{"name": "write_file",
                                                      "arguments": {"p": "t.rs"}}]}}}},
                    {"run_id": "agent1", "node_id": "end", "status": "completed",
                     "effect": {"type": "flow", "payload": {}},
                     "result": {"output": {"answer": "test added"}}}
                ]}
            }
        });
        let mut fold = crate::transcript::Fold::new();
        let mut fx = Vec::new();
        // parents wiring rides the subworkflow wait record above.
        let contributed = rehydrate_run_into(&mut fold, "root1", &bundle, false, &mut fx);
        assert!(contributed);
        assert!(fold
            .items
            .iter()
            .any(|i| matches!(i, Item::User { text } if text == "add a test")));
        assert!(fold
            .items
            .iter()
            .any(|i| matches!(i, Item::Thinking { content, .. } if content.contains("thinking"))));
        assert!(fold
            .items
            .iter()
            .any(|i| matches!(i, Item::Tool { name, .. } if name == "write_file")));
        assert!(fold.items.iter().any(
            |i| matches!(i, Item::Assistant { text, final_answer: true } if text == "test added")
        ));
        assert!(
            fold.pending_wait.is_none(),
            "a prior run's dead wait must never prompt after restore"
        );
        assert_eq!(fold.stats.effective_model, "m1");
    }

    #[test]
    fn rehydrate_unwraps_cursor_record_envelopes() {
        // Live bundle shape (verified 2026-07-22): each ledger item is a
        // {cursor, record} ENVELOPE — the same wire shape as SSE `step`
        // events. Folding the envelope itself rendered nothing. Bare
        // records stay tolerated in the same array.
        let bundle = json!({
            "input_data": {"prompt": "hi"},
            "ledgers": {
                "root1": {"run_id": "root1", "total": 2, "items": [
                    {"cursor": 1, "record":
                        {"run_id": "root1", "node_id": "act", "status": "started",
                         "effect": {"type": "tool_calls",
                                     "payload": {"tool_calls": [{"name": "read_file", "call_id": "c1"}]}}}},
                    // Bare record (older serializations): must still fold.
                    {"run_id": "root1", "node_id": "act", "status": "completed",
                     "effect": {"type": "tool_calls",
                                 "payload": {"tool_calls": [{"name": "read_file", "call_id": "c1"}]}},
                     "result": {"results": [{"call_id": "c1", "success": true, "output": "data"}]}}
                ]}
            }
        });
        let mut fold = crate::transcript::Fold::new();
        let mut fx = Vec::new();
        assert!(rehydrate_run_into(
            &mut fold, "root1", &bundle, false, &mut fx
        ));
        let tool = fold
            .items
            .iter()
            .find_map(|i| match i {
                Item::Tool { name, status, .. } => Some((name.clone(), *status)),
                _ => None,
            })
            .expect("tool card folded from the enveloped record");
        assert_eq!(tool.0, "read_file");
        assert_eq!(tool.1, crate::transcript::ToolStatus::Ok);
    }

    #[test]
    fn default_route_prefers_output_text_then_input_text() {
        let v = json!({"routes": [
            {"key": "input.text", "provider": "lmstudio", "model": "ornith-1.0-35b"},
            {"key": "output.image.text_to_image", "provider": "mlx-gen", "model": "flux"}
        ]});
        assert_eq!(
            default_text_route(&v),
            Some(("lmstudio".into(), "ornith-1.0-35b".into()))
        );
        let with_out = json!({"routes": [
            {"key": "output.text", "provider": "openai", "model": "gpt-5.2"},
            {"key": "input.text", "provider": "lmstudio", "model": "ornith-1.0-35b"}
        ]});
        assert_eq!(
            default_text_route(&with_out),
            Some(("openai".into(), "gpt-5.2".into()))
        );
        assert_eq!(default_text_route(&json!({"routes": []})), None);
    }
}
