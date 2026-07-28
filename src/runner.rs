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

use abstracttui::reactive::{Backoff, WakeHandle};
use serde_json::Value;

use crate::discovery::{
    agent_workflow_ids_from_bundles, agent_workflows_from_bundles, choose_workflow,
    default_text_route, mcp_from_response, models_from_provider_models, providers_from_discovery,
    skills_from_response, tools_from_discovery, workflows_with_interface, GOAL_INTERFACE_V1,
};
use crate::gateway::{GatewayClient, GwError, GwResult};
use crate::run_input::{build_input_data, StartOpts};
use crate::store::{CacheInfo, Conn, ImageEntry, Phase, SessionTotals, Store, Workflow};
use crate::transcript::{FoldEffect, Item, PendingWait};

/// Inline-render ceiling for fetched artifacts (images and offloaded
/// answers; exec's synchronous answer fetch reuses it).
pub const ARTIFACT_MAX_BYTES: usize = 8 * 1024 * 1024;
/// Default prior-turn replay depth at boot (`--replay-turns` overrides).
/// A cap exists because each turn costs one history-bundle fetch carrying
/// the run tree's COMPLETE ledgers — deep sessions get the newest N in full
/// detail; older turns stay one `--replay-turns` bump away.
/// Boot replays the LAST BLOC only (laurent's ruling, 2026-07-25:
/// "we only load the last bloc when loading a session... should a user
/// request previous history, we could stream rapidly that previous
/// history"). Single-turn bundles measure 10-15 MB on tool-heavy work,
/// so a 20-turn boot fetch was up to ~300 MB of HTTP before first
/// paint; older turns stream on demand (/history). `--replay-turns`
/// stays the bloc-size knob.
pub const REHYDRATE_DEFAULT_TURNS: usize = 5;

/// The transcript stub naming older not-yet-streamed turns — one
/// shared prefix so the /history prepend can find and replace it
/// (never a second drifting copy).
pub const OLDER_TURNS_STUB_PREFIX: &str = "(earlier history: ";

/// Consecutive status-less failures the idle PROBE tolerates before the
/// orb flips `Conn::Down` (~2 probe ticks ≈ a minute of silence). One
/// timed-out ping against a busy gateway is not gone-evidence — the
/// operator's false-"unreachable" report (2026-07-23) came from exactly
/// such single blips flipping the orb while `/api/health` answered in
/// ~1ms between them.
pub(crate) const PROBE_DOWN_AFTER: u32 = 2;
/// Same tolerance for a run's stream thread (stream connect + REST
/// fallback polls all count): three status-less failures in a row before
/// Down. A genuinely stopped gateway never waits for the threshold —
/// connect-refused is `is_gone()` and flips immediately.
pub(crate) const STREAM_DOWN_AFTER: u32 = 3;

/// The ONE policy for flipping the connection orb Down. Evidence-based:
/// connect-level failures (refused/DNS — `GwError::is_gone`) mean nobody
/// is there, flip now; status-less soft failures (timeouts, resets) flip
/// only after `threshold` consecutive occurrences (a busy gateway is not
/// a gone gateway); an HTTP answer of ANY code proves reachability and
/// never flips (the doctor's own reachability rule, `cli.rs`). This
/// replaces both the flip-on-first-error behavior and the fragile
/// `msg.contains("unreachable")` substring match (which our own Display
/// minted for every status-less error — and which an HTTP 500 detail
/// containing the word would also have tripped).
pub(crate) fn marks_gateway_down(
    e: &GwError,
    consecutive_soft_failures: u32,
    threshold: u32,
) -> bool {
    e.is_gone() || (e.status.is_none() && consecutive_soft_failures >= threshold)
}

/// Probe-driven catalog self-heal (the F1 lesson under the evidence-based
/// Down policy): with soft failures no longer flipping the orb, the UI's
/// Down→Ok reload edge cannot be the only recovery path — a boot whose
/// catalog load timed out against a busy gateway (or was refused by a
/// then-broken auth config fixed server-side later) would otherwise sit
/// with an empty catalog and a green orb forever. Fires only when a load
/// was ATTEMPTED and never succeeded; replays the last preference.
pub(crate) fn heal_catalog_if_missing(
    tx: &Sender<Cmd>,
    attempted: bool,
    loaded: bool,
    preference: &(Option<String>, Option<String>),
) {
    if attempted && !loaded {
        let _ = tx.send(Cmd::LoadCatalog {
            preferred_bundle: preference.0.clone(),
            preferred_flow: preference.1.clone(),
        });
        let _ = tx.send(Cmd::LoadTools);
    }
}

/// Human note when a catalog REFRESH changes the entrypoint set. The
/// /workflow picker is a static shell (labels computed at open), so an
/// operator looking at a stale-open picker must be TOLD the list moved —
/// silent refreshes were how a launch-time snapshot masqueraded as the
/// live catalog (operator incident 2026-07-25). Boot loads (prev empty)
/// stay silent; an unchanged refresh stays silent.
pub(crate) fn catalog_change_note(prev: &[Workflow], next: &[Workflow]) -> Option<String> {
    if prev.is_empty() {
        return None;
    }
    let keys = |ws: &[Workflow]| -> std::collections::HashSet<(String, String)> {
        ws.iter()
            .map(|w| (w.bundle_id.clone(), w.flow_id.clone()))
            .collect()
    };
    let prev_keys = keys(prev);
    let next_keys = keys(next);
    let added = next_keys.difference(&prev_keys).count();
    let removed = prev_keys.difference(&next_keys).count();
    if added == 0 && removed == 0 {
        return None;
    }
    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("{added} new"));
    }
    if removed > 0 {
        parts.push(format!("{removed} removed"));
    }
    // An OPEN /workflow picker renders the change live (reactive rows);
    // this note serves the far more common no-picker-open moment.
    Some(format!(
        "workflow catalog changed ({}) — /workflow lists the fresh set",
        parts.join(", ")
    ))
}

/// Debug is test-surface: harnesses assert on drained commands
/// (`panic!("wrong command: {other:?}")` in gateway::entities tests).
#[derive(Debug)]
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
        /// Pending attachments snapshotted at submit (paths + any refs
        /// cached from a prior failed start). Uploaded HERE on the worker
        /// thread — upload failure blocks the start and keeps custody
        /// with the UI (chips stay). Only the explicit plain-prompt path
        /// fills this; goal/queue-drain starts send empty.
        attachments: Vec<crate::store::PendingAttachment>,
        /// Gateway attachment cap SNAPSHOTTED ON THE UI THREAD at
        /// submit (0 = unknown → the client safety ceiling applies).
        /// Signals are UI-thread-stamped — a `max_attachment_bytes`
        /// read inside the worker's upload loop PANICS the runner
        /// (verify-pass NEW-1, probe-confirmed), so the value must
        /// ride the command.
        attachment_cap: u64,
    },
    /// Probe the session for a live run and attach to it if one exists —
    /// after REHYDRATING the session's prior turns from their run ledgers
    /// (quit/crash must come back to the same transcript).
    ProbeAttach {
        session_id: String,
        /// How many prior turns to replay in full detail (0 = none).
        replay_turns: usize,
    },
    /// `/history` — stream a PREVIOUS bloc of session turns and prepend
    /// them to the transcript (laurent's ruling: last bloc at boot,
    /// older history rapidly on request). `before` = created_at of the
    /// oldest already-restored turn (the store's history cursor).
    LoadHistory {
        session_id: String,
        before: String,
        count: usize,
    },
    /// `/status` server-truth probe: one `get_run`, result posted into
    /// `store.run_status_probe` (review P2-5 — client phase vs gateway
    /// status divergence, inspectable on demand).
    ProbeRunStatus {
        run_id: String,
    },
    /// Picker stage 3: probe one model's reasoning capability (async —
    /// the modal renders live rows from `store.reasoning_probe`).
    ProbeModelReasoning {
        provider: String,
        model: String,
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
    /// Fetch an OFFLOADED final answer (outputs >256 KB persist as
    /// `{"$artifact": id}` — the runtime's ledger offloader) and swap the
    /// placeholder card for the real words. The turn already concluded;
    /// a failed fetch labels the card honestly.
    FetchAnswer {
        run_id: String,
        artifact_id: String,
    },
    /// The answer landed: stop following helper subruns (wrapper bundles can
    /// keep status-watcher subflows polling long after the agent finished).
    StopFollows,
    /// Start the `/gpu` meter poller (its OWN thread — `gateway::gpu`;
    /// the command exists because the client lives on this loop).
    GpuEnable,
    /// Stop the `/gpu` meter poller (generation bump; stale posts no-op).
    GpuDisable,
    // -- entity collaboration lane (threads spawned per command; the
    // -- runner loop is never blocked behind a slow entity read) ----------
    /// Refresh the entity roster (async; last-good roster stays cached).
    LoadEntities,
    /// Load one entity's identity card (async, cached in the store).
    LoadEntityCard {
        name: String,
    },
    /// Open (or adopt via structured 409 → `GET /visit`) a visit.
    EntityOpen {
        name: String,
    },
    /// Send one visit turn on its own thread (600s read; recovery loop on
    /// timeout). Stale results are dropped by the convo/run/epoch guard.
    EntityTurn {
        name: String,
        run_id: String,
        epoch: u64,
        text: String,
    },
    /// One FLOW-BRAIN turn (summon-per-prompt of the entity-chat flow):
    /// summon → poll to terminal → guarded fold. No run identity at send
    /// time — the epoch is the staleness guard (`convo::guard_flow`).
    EntityFlowTurn {
        name: String,
        session_id: String,
        epoch: u64,
        text: String,
    },
    /// Close a visit with closed_by=operator (+ reflection render).
    EntityClose {
        name: String,
        run_id: String,
        epoch: u64,
        reason: String,
    },
    /// Leave a task in the entity's durable task inbox (works asleep).
    EntityTask {
        name: String,
        title: String,
    },
    /// Ensure the ONE conversation poller thread is running.
    PollConvos,
    Shutdown,
}

/// Queue the FETCH effects a REHYDRATION fold produced (offloaded-answer
/// and image fetches for restored placeholders). `FollowRun` is ignored
/// BY CONTRACT here: rehydration decides stream membership itself from
/// the bundle's ledger set — only the live stream path (`
/// apply_stream_records`) turns `FollowRun` into `Cmd::Follow`. This
/// block was spelled verbatim at both rehydration sites (probe + attach).
fn send_fetch_effects(tx: &Sender<Cmd>, effects: Vec<FoldEffect>) {
    for fx in effects {
        match fx {
            FoldEffect::FetchImage {
                run_id,
                artifact_id,
            } => {
                let _ = tx.send(Cmd::FetchImage {
                    run_id,
                    artifact_id,
                });
            }
            FoldEffect::FetchAnswer {
                run_id,
                artifact_id,
            } => {
                let _ = tx.send(Cmd::FetchAnswer {
                    run_id,
                    artifact_id,
                });
            }
            FoldEffect::FollowRun(_) => {}
        }
    }
}

struct Runner {
    client: GatewayClient,
    wake: WakeHandle,
    store: Store,
    tx: Sender<Cmd>,
    /// Stop flags for every live stream thread (flipped on new run/quit);
    /// the bool marks the root stream, which outlives `StopFollows`.
    stream_stops: Vec<(bool, Arc<AtomicBool>)>,
    /// Catalog-declared agent workflow ids (lane-1 fold contract,
    /// `Fold::set_agent_workflows`): parsed at catalog load and RETAINED
    /// here because rehydration builds fresh Folds worker-side — they
    /// need the declaration too, and the runner thread must never read
    /// UI-thread signals to recover it. Empty until the catalog loads
    /// (the fold degrades gracefully — structural id contract still binds).
    agent_workflow_ids: Vec<String>,
    /// Consecutive status-less probe/catalog failures (the `Conn::Down`
    /// persistence evidence — see `marks_gateway_down`). Reset by any
    /// successful gateway answer on this thread, HTTP errors included.
    soft_failures: u32,
    /// Catalog self-heal state (the F1 lesson under the new Down policy):
    /// with soft failures no longer flipping the orb, the UI's Down→Ok
    /// edge can't be the only reload trigger — a boot whose catalog load
    /// timed out against a busy gateway would sit with an EMPTY catalog
    /// and a green orb forever. The probe re-issues the load whenever the
    /// gateway answers while a previously ATTEMPTED load never succeeded,
    /// replaying the last requested workflow preference.
    catalog_attempted: bool,
    catalog_loaded: bool,
    catalog_preference: (Option<String>, Option<String>),
    /// Lazy probe: `None` until first session-bloc fetch; `false` after
    /// a 404 (pre-ship gateways keep the N-bundle fallback for the rest
    /// of this runner's life).
    session_bloc_available: Option<bool>,
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
                    agent_workflow_ids: Vec::new(),
                    soft_failures: 0,
                    catalog_attempted: false,
                    catalog_loaded: false,
                    catalog_preference: (None, None),
                    session_bloc_available: None,
                };
                while let Ok(cmd) = rx.recv() {
                    if matches!(cmd, Cmd::Shutdown) {
                        runner.stop_streams();
                        crate::gateway::entities::stop_poller();
                        crate::gateway::gpu::stop();
                        break;
                    }
                    runner.handle(cmd);
                }
            }));
            if let Err(payload) = result {
                let msg = panic_text(payload.as_ref());
                panic_wake.post(move || apply_worker_death(&store, &msg));
            }
        })
        .expect("spawn gateway-runner thread")
}

/// UI-thread application of a dead gateway worker (the runner loop
/// panicked): error card + notice + honest phase — and the history
/// lanes' in-flight flags reset. A panic mid `ProbeAttach`/`LoadHistory`
/// otherwise left `restoring`/`history_loading` armed forever: the idle
/// strip kept claiming "restoring…"/"streaming earlier history…",
/// `/history` and the scroll-top auto-loader died silently on the
/// in-flight guard, and the stub stayed frozen on "streaming N of M…".
/// Extracted (the `apply_start_binding` pattern) so the reset is
/// testable without panicking a live runner thread.
pub(crate) fn apply_worker_death(store: &Store, msg: &str) {
    store.fold.update(|f| {
        f.push_item(Item::Error {
            text: format!("gateway worker died: {msg} — restart the app to reconnect"),
        })
    });
    store.notify("gateway worker died — restart the app");
    // Degrade honestly: no command loop means no pause/
    // cancel/steer can be delivered — a spinner claiming
    // otherwise would lie (adversary finding 10). The run
    // itself continues durably server-side.
    store.phase.set(Phase::Idle);
    // History lanes tell the truth too: nothing can ever flip these
    // back once the command loop is gone.
    store.restoring.set(false);
    if store.history_loading.get_untracked() {
        store.history_loading.set(false);
        let older = store.older_turns.get_untracked();
        store.fold.update(|f| restore_history_stub(f, older));
    }
}

/// Best-effort extraction of a panic payload's message.
pub(crate) fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
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
                attachments,
                attachment_cap,
            } => self.start_run(
                prompt,
                flow_id,
                bundle_id,
                session_id,
                *opts,
                attachments,
                attachment_cap,
            ),
            Cmd::ProbeAttach {
                session_id,
                replay_turns,
            } => self.probe_attach(&session_id, replay_turns),
            Cmd::LoadHistory {
                session_id,
                before,
                count,
            } => self.load_history(&session_id, &before, count),
            Cmd::ProbeRunStatus { run_id } => self.probe_run_status(run_id),
            Cmd::ProbeModelReasoning { provider, model } => {
                self.probe_model_reasoning(provider, model)
            }
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
            Cmd::FetchAnswer {
                run_id,
                artifact_id,
            } => self.fetch_answer(run_id, artifact_id),
            Cmd::StopFollows => self.stop_follow_streams(),
            // GPU meter lane: the poller runs on its own thread (a
            // metrics read must never starve Probe/Start on this loop).
            Cmd::GpuEnable => {
                crate::gateway::gpu::start(self.client.clone(), self.wake.clone(), self.store);
            }
            Cmd::GpuDisable => crate::gateway::gpu::stop(),
            // Entity lane: every handler spawns its own thread inside
            // gateway::entities (a 30-600s entity read must never starve
            // Probe/Start behind it on this loop).
            Cmd::LoadEntities => {
                let client = crate::gateway::entities::client_for(&self.client);
                crate::gateway::entities::spawn_load_entities(
                    client,
                    self.wake.clone(),
                    self.store,
                );
            }
            Cmd::LoadEntityCard { name } => {
                let client = crate::gateway::entities::client_for(&self.client);
                crate::gateway::entities::spawn_load_card(
                    client,
                    self.wake.clone(),
                    self.store,
                    name,
                );
            }
            Cmd::EntityOpen { name } => {
                let client = crate::gateway::entities::client_for(&self.client);
                crate::gateway::entities::spawn_open(
                    client,
                    self.wake.clone(),
                    self.store,
                    self.tx.clone(),
                    name,
                );
            }
            Cmd::EntityTurn {
                name,
                run_id,
                epoch,
                text,
            } => {
                let client = crate::gateway::entities::client_for(&self.client);
                crate::gateway::entities::spawn_turn(
                    client,
                    self.wake.clone(),
                    self.store,
                    self.tx.clone(),
                    name,
                    run_id,
                    epoch,
                    text,
                );
            }
            Cmd::EntityFlowTurn {
                name,
                session_id,
                epoch,
                text,
            } => {
                let client = crate::gateway::entities::client_for(&self.client);
                crate::gateway::entities::spawn_flow_turn(
                    client,
                    self.wake.clone(),
                    self.store,
                    self.tx.clone(),
                    name,
                    session_id,
                    epoch,
                    text,
                );
            }
            Cmd::EntityClose {
                name,
                run_id,
                epoch,
                reason,
            } => {
                let client = crate::gateway::entities::client_for(&self.client);
                crate::gateway::entities::spawn_close(
                    client,
                    self.wake.clone(),
                    self.store,
                    name,
                    run_id,
                    epoch,
                    reason,
                );
            }
            Cmd::EntityTask { name, title } => {
                let client = crate::gateway::entities::client_for(&self.client);
                crate::gateway::entities::spawn_task(
                    client,
                    self.wake.clone(),
                    self.store,
                    name,
                    title,
                );
            }
            Cmd::PollConvos => {
                let client = crate::gateway::entities::client_for(&self.client);
                crate::gateway::entities::ensure_poller(client, self.wake.clone(), self.store);
            }
        }
    }

    fn probe(&mut self) {
        let store = self.store;
        match self.client.ping() {
            Ok(_) => {
                self.soft_failures = 0;
                self.post(move || {
                    store.conn.set_if_changed(Conn::Ok);
                });
                self.heal_catalog_if_missing();
            }
            // An HTTP answer — any code — proves the gateway is REACHABLE
            // (the doctor's own rule): the orb claims connectivity, never
            // request health. Auth/server errors surface loudly on their
            // own lanes (catalog/tools toasts, stream refusals).
            Err(e) if e.status.is_some() => {
                self.soft_failures = 0;
                self.post(move || {
                    store.conn.set_if_changed(Conn::Ok);
                });
                self.heal_catalog_if_missing();
            }
            Err(e) => {
                self.soft_failures = self.soft_failures.saturating_add(1);
                if marks_gateway_down(&e, self.soft_failures, PROBE_DOWN_AFTER) {
                    let gone = e.is_gone();
                    let msg = e.to_string();
                    self.post(move || {
                        store.conn.set_if_changed(Conn::Down(msg, gone));
                    });
                }
                // else: one status-less blip (a timed-out ping against a
                // busy gateway) is not gone-evidence — keep the last state.
            }
        }
    }

    /// Re-issue the catalog/tools loads when the gateway answers but an
    /// ATTEMPTED catalog load never succeeded (see the field doc: soft
    /// failures no longer flip the orb, so the UI's Down→Ok self-heal
    /// cannot be the only reload path). Bounded: stops the moment a load
    /// succeeds; never fires before the boot's own load ran.
    fn heal_catalog_if_missing(&mut self) {
        heal_catalog_if_missing(
            &self.tx,
            self.catalog_attempted,
            self.catalog_loaded,
            &self.catalog_preference,
        );
    }

    fn load_catalog(&mut self, preferred_bundle: Option<String>, preferred_flow: Option<String>) {
        let store = self.store;
        // Heal bookkeeping (see `heal_catalog_if_missing`): remember the
        // requested preference so a probe-driven retry replays it.
        self.catalog_attempted = true;
        self.catalog_preference = (preferred_bundle.clone(), preferred_flow.clone());
        match self.client.list_bundles() {
            Ok(v) => {
                let workflows = agent_workflows_from_bundles(&v);
                // Goal catalog (disjoint interface): `/goal` lights up when
                // a goal bundle appears; zero entries = the honest dark
                // notice at dispatch (the bundle is flow-seat-owned).
                let goal_workflows = workflows_with_interface(&v, GOAL_INTERFACE_V1);
                // Lane-1 fold contract (`Fold::set_agent_workflows`): the
                // catalog's agent-interface entrypoint `workflow_id`s (the
                // run-facing `{bundle}@{version}:{flow}` form that spawn
                // records cite as `sub_workflow_id`). Declared into the
                // live fold below AND retained on the runner for the
                // fresh Folds rehydration builds worker-side.
                let agent_ids = agent_workflow_ids_from_bundles(&v);
                self.agent_workflow_ids = agent_ids.clone();
                let chosen = choose_workflow(
                    &workflows,
                    preferred_bundle.as_deref(),
                    preferred_flow.as_deref(),
                );
                self.soft_failures = 0;
                self.catalog_loaded = true;
                self.post(move || {
                    if let Some(w) = chosen {
                        // Never clobber a user selection made while loading.
                        if store.workflow.with(|cur| cur.flow_id.is_empty()) {
                            store.workflow.set(w);
                        }
                    }
                    // A refresh that changes the set says so (the open
                    // /workflow picker is a snapshot; see catalog_change_note).
                    if let Some(note) = store
                        .workflows
                        .with_untracked(|prev| catalog_change_note(prev, &workflows))
                    {
                        store.notify(note);
                    }
                    store.workflows.set(workflows);
                    store.goal_workflows.set(goal_workflows);
                    store.fold.update(|f| f.set_agent_workflows(agent_ids));
                    store.conn.set_if_changed(Conn::Ok);
                });
            }
            Err(e) => {
                // The toast names the failed LOAD honestly (Display words
                // the evidence class); the orb flips only on gone-evidence
                // — a slow bundles read against a busy gateway used to
                // brand the whole app "unreachable".
                let gone = e.is_gone();
                let msg = e.to_string();
                self.post(move || {
                    if gone {
                        store.conn.set_if_changed(Conn::Down(msg.clone(), true));
                    }
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
            // Attachment size cap (client pre-check; 0 stays "unknown" —
            // the server 413 remains the authority).
            let cap = v
                .get("policy")
                .and_then(|p| p.get("max_attachment_bytes"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if cap > 0 {
                self.post(move || store.max_attachment_bytes.set(cap));
            }
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
                let info = crate::entities::mcp_registry_info(&v);
                self.post(move || {
                    store.mcp_servers.set(servers);
                    store.mcp_note.set(note);
                    store.mcp_info.set(info);
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

    #[allow(clippy::too_many_arguments)]
    fn start_run(
        &mut self,
        prompt: String,
        flow_id: String,
        bundle_id: String,
        session_id: String,
        mut opts: StartOpts,
        mut attachments: Vec<crate::store::PendingAttachment>,
        attachment_cap: u64,
    ) {
        self.stop_streams();
        let store = self.store;
        // Upload pending attachments BEFORE the run exists (design §4.3:
        // custody stays with the UI until the run starts). Reuse refs
        // cached from a prior failed start of the SAME session — retry
        // never mints duplicate artifacts. ANY failure blocks the start:
        // silently sending without what the user attached is the lie.
        if !attachments.is_empty() {
            for item in attachments.iter_mut() {
                let cached = item
                    .uploaded
                    .as_ref()
                    .is_some_and(|(sid, _)| *sid == session_id);
                if cached {
                    continue;
                }
                item.uploaded = None; // a foreign-session ref is unusable
                                      // The Starting window names its work (visibility review
                                      // P1-3): a multi-second upload was a dead state — the
                                      // spinner moved, nothing else did. Cleared implicitly:
                                      // begin_run wipes activity when the run starts, and the
                                      // failure path replaces the strip via phase Idle.
                {
                    let label = format!(
                        "uploading {} ({})…",
                        item.name,
                        crate::paths::human_size(item.size)
                    );
                    self.post(move || store.fold.update(|f| f.activity = label));
                }
                // Stat-before-read belt: the attach-time pre-check saw
                // the file as it WAS — one grown past the cap (or the
                // safety ceiling when the cap is unknown) since then
                // must not buffer whole on this thread first. The cap
                // rode the command (UI-thread snapshot): reading the
                // signal HERE panics the worker (thread stamp — the
                // verify-pass NEW-1 P0).
                let ceiling = if attachment_cap > 0 {
                    attachment_cap
                } else {
                    crate::ui::attachments::CLIENT_SAFETY_CEILING_BYTES
                };
                let missing = || {
                    format!(
                        "{} no longer exists — remove it (/attach) or restore the file",
                        item.name
                    )
                };
                let outcome = std::fs::metadata(&item.path)
                    .map_err(|_| missing())
                    .and_then(|m| {
                        if m.len() > ceiling {
                            Err(format!(
                                "attachment upload refused: {} is now {} — over the {} limit — attachments kept; fix and resend",
                                item.name,
                                crate::paths::human_size(m.len()),
                                crate::paths::human_size(ceiling)
                            ))
                        } else {
                            std::fs::read(&item.path).map_err(|_| missing())
                        }
                    })
                    .and_then(|bytes| {
                        self.client
                            .upload_attachment(&session_id, &item.name, &bytes)
                            .map_err(|e| {
                                // Display form, NOT compact_reason: the
                                // gateway's 413 detail ("Attachment too
                                // large (N bytes > M bytes)") is the
                                // documented contract, and this card is
                                // user-facing only — Item::Error never
                                // folds into chat_messages, so the
                                // URL-in-label model-safety rule does
                                // not apply here.
                                format!(
                                    "attachment upload failed: {} — {e} — attachments kept; fix and resend",
                                    item.name
                                )
                            })
                    });
                match outcome {
                    Ok(r) => item.uploaded = Some((session_id.clone(), r)),
                    Err(msg) => {
                        // Cache successful siblings back (by path — the
                        // user may have edited chips mid-flight), revert
                        // the phase, keep custody. SESSION-GUARDED like
                        // both terminal siblings (apply_start_binding /
                        // apply_start_failure): uploads can hold this
                        // thread ~30s/file — a /new + fresh submit in
                        // that window must not eat the OLD failure's
                        // error card or have its Starting phase flipped
                        // back to Idle (double-start window; the
                        // stale-closure currency rule, 2026-07-21).
                        let done = attachments.clone();
                        let started_session = session_id.clone();
                        self.post(move || {
                            if store.session_id.with_untracked(|s| *s != started_session) {
                                store.notify(format!(
                                    "an attachment upload from the previous session failed late: {msg}"
                                ));
                                return;
                            }
                            merge_cached_refs(&store, &done);
                            store.phase.set(crate::store::Phase::Idle);
                            store.fold.update(|f| {
                                // The "uploading …" activity dies with the
                                // attempt — it would otherwise resurface in
                                // the NEXT submit's Starting window
                                // (begin_run clears activity only once the
                                // run actually starts).
                                f.activity.clear();
                                f.push_item(Item::Error { text: msg.clone() })
                            });
                        });
                        return;
                    }
                }
            }
            opts.attachments = attachments
                .iter()
                .filter_map(|a| a.uploaded.as_ref().map(|(_, r)| r.clone()))
                .collect();
        }
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
                let tx = self.tx.clone();
                let started_session = session_id.clone();
                let sent = attachments.clone();
                self.post(move || {
                    // Session gate for BOTH halves: a mismatch means the
                    // binding cancels the late run — the 📎 line must
                    // not land in the NEW session's transcript for a
                    // turn that never happened there (the boundary
                    // discard already emptied its pending list).
                    let current = store.session_id.with_untracked(|s| s == &started_session);
                    apply_start_binding(&store, &tx, &rid, &started_session);
                    // Custody transfers: the sent batch leaves the pending
                    // list (chips added mid-flight survive) and the 📎
                    // line records what rode this turn.
                    if current && !sent.is_empty() {
                        clear_sent_attachments(&store, &tx, &sent);
                    }
                });
                // The stream spawns regardless (the runner cannot see the
                // UI-thread session check): a mismatch-cancelled run's
                // records drop at the fold's root guard, and the next
                // start's stop_streams reaps the thread.
                self.spawn_stream(run_id.clone(), run_id, true);
            }
            Err(e) => {
                // Classification decided HERE where the typed error lives —
                // the UI-thread half must never re-derive it from message
                // text (the old `contains("unreachable")` substring).
                let gone = e.is_gone();
                let msg = e.to_string();
                let started_session = session_id.clone();
                let done = attachments.clone();
                self.post(move || {
                    // Keep custody; cache any refs minted before the start
                    // failed so the retry reuses them (no duplicates).
                    merge_cached_refs(&store, &done);
                    apply_start_failure(&store, &msg, gone, &started_session);
                });
            }
        }
    }

    /// Session-history bloc route when present; `None` means fall back to
    /// per-run `history_bundle` fan-out (404 caches unavailable).
    fn fetch_session_bloc(
        &mut self,
        session_id: &str,
        before: Option<&str>,
        limit: usize,
    ) -> Option<GwResult<Value>> {
        if self.session_bloc_available == Some(false) {
            return None;
        }
        match self
            .client
            .session_history_bloc(session_id, before, limit)
        {
            Ok(v) => {
                self.session_bloc_available = Some(true);
                Some(Ok(v))
            }
            Err(e) if e.status == Some(404) => {
                self.session_bloc_available = Some(false);
                None
            }
            Err(e) => Some(Err(e)),
        }
    }

    fn probe_attach(&mut self, session_id: &str, replay_turns: usize) {
        let store = self.store;
        // The idle strip says what this window is doing — the whole
        // probe is up to ~21 HTTP fetches, and "no runs yet" during it
        // was a lie about a session with history in flight (visibility
        // review P2-7). Cleared on EVERY exit path below.
        self.post(move || store.restoring.set(true));
        // One RAII-ish guard: every `return` clears it via this closure.
        let clear_restoring = {
            let wake = self.wake.clone();
            move || wake.post(move || store.restoring.set(false))
        };
        // List WIDE (the list endpoint is light — run summaries only):
        // the boot fetches full bundles for the newest BLOC only, but
        // the count of older turns must be KNOWN and NAMED (audit: the
        // old `list_limit = replay_turns` silently clipped older turns
        // out of existence — an adjacent silent-loss lane).
        let list_limit = 200u32;
        let v = match self.client.list_runs(session_id, list_limit) {
            Ok(v) => v,
            Err(e) => {
                clear_restoring();
                // A silent return here resumed the session EMPTY with no
                // hint that history exists (adversary P1-4): say so where
                // the user reads.
                let msg = e.to_string();
                self.post(move || {
                    store.fold.update(|f| {
                        f.push_item(Item::Error {
                            text: format!(
                                "session history could not be restored from the gateway ({msg}) — /sessions and re-select to retry"
                            ),
                        });
                    });
                });
                return;
            }
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
        // worker-side into a fresh Fold, swapped in with one post — which
        // REPLACES the store fold, so catalog state must be re-declared
        // here or the swap would wipe it (and the restored ledgers need
        // the declaration DURING folding for answer-source recognition).
        let mut fold = crate::transcript::Fold::new();
        fold.set_agent_workflows(self.agent_workflow_ids.iter().cloned());
        let mut effects: Vec<FoldEffect> = Vec::new();
        let mut replayed = 0usize;
        let mut failed_restores = 0usize;
        // THE LAST BLOC (laurent's ruling): full-detail bundles for the
        // NEWEST `replay_turns` prior turns only; older turns are named
        // in a stub and stream on demand (/history). `items` is
        // chronological ASC, so the bloc is the TAIL.
        let prior: Vec<&Value> = items
            .iter()
            .filter(|r| {
                let rid = r.get("run_id").and_then(Value::as_str).unwrap_or("");
                !rid.is_empty() && Some(rid.to_string()) != live_run_id
            })
            .collect();
        let bloc_start = prior.len().saturating_sub(replay_turns);
        let mut older_count = bloc_start;
        // The history cursor: created_at of the oldest turn the bloc
        // restores — /history streams turns strictly BEFORE it.
        let mut history_cursor = prior
            .get(bloc_start)
            .and_then(|r| r.get("created_at").and_then(Value::as_str))
            .map(str::to_string);
        if older_count > 0 {
            fold.push_item(Item::Info {
                text: history_stub_text(older_count),
            });
        }
        if replay_turns > 0 {
            let mut used_bloc = false;
            if let Some(bloc_result) = self.fetch_session_bloc(session_id, None, replay_turns) {
                match bloc_result {
                    Ok(bloc) => {
                        used_bloc = true;
                        older_count = bloc
                            .get("older_remaining")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize;
                        history_cursor = bloc
                            .get("cursor_after")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        let (r, f) = fold_session_bloc_turns(&mut fold, &bloc, &mut effects);
                        replayed += r;
                        failed_restores += f;
                    }
                    Err(e) => {
                        fold.push_item(Item::Error {
                            text: format!(
                                "session history bloc could not be restored from the gateway ({e})"
                            ),
                        });
                        failed_restores += 1;
                    }
                }
            }
            if !used_bloc {
                for run in prior.iter().skip(bloc_start) {
                    let rid = run.get("run_id").and_then(Value::as_str).unwrap_or("");
                    match self.client.history_bundle(rid, false, 0) {
                        Ok(bundle) => {
                            let failed = matches!(
                                run.get("status").and_then(Value::as_str).unwrap_or(""),
                                "failed" | "cancelled"
                            );
                            if rehydrate_run_into(&mut fold, rid, &bundle, failed, &mut effects) {
                                replayed += 1;
                            }
                        }
                        Err(e) => {
                            fold.push_item(Item::Error {
                                text: format!(
                                    "one prior turn could not be restored — run {}: {e}",
                                    &rid[..rid.len().min(8)]
                                ),
                            });
                            failed_restores += 1;
                        }
                    }
                }
            }
        }
        if replayed > 0 || failed_restores > 0 {
            // The summary counts HONESTLY (audit: failure markers used
            // to count as "replayed" — "replayed 9" over 7 real turns).
            let failures = if failed_restores > 0 {
                format!(" ({failed_restores} could not be restored — errors above name the cause)")
            } else {
                String::new()
            };
            fold.push_item(Item::Info {
                text: format!(
                    "replayed {replayed} prior turn(s) in full detail from the gateway{failures}"
                ),
            });
            let session = fold.session;
            let probed_session = session_id.to_string();
            let cursor = history_cursor.clone();
            self.post(move || {
                // Session guard (adversary P1-2): the probe is a wide
                // window (up to ~21 HTTP fetches); a session switch or
                // /new mid-probe must not receive the OLD session's
                // restored history/totals — same rule as
                // apply_start_binding's late-start guard.
                if store.session_id.with_untracked(|s| *s != probed_session) {
                    return;
                }
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
                    total_tokens: session.total_tokens,
                    runs: session.runs,
                });
                store.history_cursor.set(cursor.clone());
                store.older_turns.set(older_count);
            });
            // Images and offloaded answers from prior turns re-render/
            // re-fetch through the normal paths (the fetch resolves the
            // restored placeholder after the fold swap posts — same UI
            // wake queue, so ordering holds).
            send_fetch_effects(&self.tx, effects);
        }

        clear_restoring();
        if let Some(run_id) = live_run_id {
            let paused = items.iter().any(|r| {
                r.get("run_id").and_then(Value::as_str) == Some(run_id.as_str())
                    && r.get("paused").and_then(Value::as_bool).unwrap_or(false)
            });
            // Back-date the elapsed clock to the run's REAL start: the
            // attach used to anchor at Instant::now(), so a reattached
            // 2-hour run displayed "3s" — elapsed-since-attach, not
            // elapsed-since-start (bug (e) diagnosis, 2026-07-22).
            let started_at = items
                .iter()
                .find(|r| r.get("run_id").and_then(Value::as_str) == Some(run_id.as_str()))
                .and_then(|r| r.get("created_at").and_then(Value::as_str))
                .and_then(crate::protocol::parse_rfc3339_utc)
                .and_then(|st| std::time::SystemTime::now().duration_since(st).ok())
                .and_then(|dur| std::time::Instant::now().checked_sub(dur));
            let rid = run_id.clone();
            self.post(move || {
                store.paused.set_if_changed(paused);
                store.notify(format!(
                    "reattaching to live run {}",
                    &rid[..rid.len().min(8)]
                ));
            });
            self.attach(run_id, started_at, session_id.to_string());
        }
    }

    /// `/history` — stream the PREVIOUS bloc (laurent's ruling): fetch
    /// full bundles for the newest `count` turns strictly BEFORE the
    /// cursor, fold them into a scratch fold, and PREPEND its items to
    /// the live transcript (fold run-state untouched — items only).
    /// The stub line is replaced with the updated older-count (or
    /// removed at zero); failures are cause-named Error items, same as
    /// the boot bloc.
    fn load_history(&mut self, session_id: &str, before: &str, count: usize) {
        let store = self.store;
        let limit = count.max(1);
        if let Some(bloc_result) = self.fetch_session_bloc(session_id, Some(before), limit) {
            match bloc_result {
                Ok(bloc) => {
                    let remaining = bloc
                        .get("older_remaining")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    let new_cursor = bloc
                        .get("cursor_after")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let mut scratch = crate::transcript::Fold::new();
                    scratch.set_agent_workflows(self.agent_workflow_ids.iter().cloned());
                    let mut effects: Vec<FoldEffect> = Vec::new();
                    let (streamed, failed) =
                        fold_session_bloc_turns(&mut scratch, &bloc, &mut effects);
                    if streamed == 0 && failed == 0 {
                        let probed = session_id.to_string();
                        self.post(move || apply_history_none_older(&store, &probed));
                        return;
                    }
                    let probed_session = session_id.to_string();
                    let scratch_session = scratch.session;
                    self.post(move || {
                        if store.session_id.with_untracked(|s| *s != probed_session) {
                            return;
                        }
                        store
                            .fold
                            .update(|f| prepend_history_items(f, scratch.items, remaining));
                        store.totals.update(|t| {
                            t.input_tokens += scratch_session.input_tokens;
                            t.output_tokens += scratch_session.output_tokens;
                            t.total_tokens += scratch_session.total_tokens;
                            t.runs += scratch_session.runs;
                        });
                        store.history_cursor.set(new_cursor.clone());
                        store.older_turns.set(remaining);
                        store.history_loading.set(false);
                        let fail_note = if failed > 0 {
                            format!(" ({failed} could not be restored — errors name the cause)")
                        } else {
                            String::new()
                        };
                        store.notify(format!(
                            "streamed {streamed} earlier turn(s){fail_note} — {remaining} more on the gateway"
                        ));
                    });
                    send_fetch_effects(&self.tx, effects);
                    return;
                }
                Err(e) => {
                    let msg = e.to_string();
                    let probed = session_id.to_string();
                    self.post(move || apply_history_list_failure(&store, &probed, &msg));
                    return;
                }
            }
        }
        let v = match self.client.list_runs(session_id, 200) {
            Ok(v) => v,
            Err(e) => {
                let msg = e.to_string();
                let probed = session_id.to_string();
                self.post(move || apply_history_list_failure(&store, &probed, &msg));
                return;
            }
        };
        let mut items = v
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        items.sort_by(|a, b| {
            let ka = a.get("created_at").and_then(Value::as_str).unwrap_or("");
            let kb = b.get("created_at").and_then(Value::as_str).unwrap_or("");
            ka.cmp(kb)
        });
        // Turns strictly BEFORE the cursor (ISO strings compare
        // lexicographically — the WAIT_UNTIL house rule).
        let older: Vec<&Value> = items
            .iter()
            .filter(|r| {
                r.get("created_at")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c < before)
            })
            .collect();
        if older.is_empty() {
            let probed = session_id.to_string();
            self.post(move || apply_history_none_older(&store, &probed));
            return;
        }
        let bloc_start = older.len().saturating_sub(count.max(1));
        let remaining = bloc_start;
        let new_cursor = older
            .get(bloc_start)
            .and_then(|r| r.get("created_at").and_then(Value::as_str))
            .map(str::to_string);
        let mut scratch = crate::transcript::Fold::new();
        scratch.set_agent_workflows(self.agent_workflow_ids.iter().cloned());
        let mut effects: Vec<FoldEffect> = Vec::new();
        let mut streamed = 0usize;
        let mut failed = 0usize;
        for run in older.iter().skip(bloc_start) {
            let rid = run.get("run_id").and_then(Value::as_str).unwrap_or("");
            if rid.is_empty() {
                continue;
            }
            match self.client.history_bundle(rid, false, 0) {
                Ok(bundle) => {
                    let run_failed = matches!(
                        run.get("status").and_then(Value::as_str).unwrap_or(""),
                        "failed" | "cancelled"
                    );
                    if rehydrate_run_into(&mut scratch, rid, &bundle, run_failed, &mut effects) {
                        streamed += 1;
                    }
                }
                Err(e) => {
                    scratch.push_item(Item::Error {
                        text: format!(
                            "one earlier turn could not be restored — run {}: {e}",
                            &rid[..rid.len().min(8)]
                        ),
                    });
                    failed += 1;
                }
            }
        }
        let probed_session = session_id.to_string();
        let scratch_session = scratch.session;
        self.post(move || {
            if store.session_id.with_untracked(|s| *s != probed_session) {
                return; // stale (session switched mid-stream)
            }
            store
                .fold
                .update(|f| prepend_history_items(f, scratch.items, remaining));
            // Session totals grow by the streamed turns' spend.
            store.totals.update(|t| {
                t.input_tokens += scratch_session.input_tokens;
                t.output_tokens += scratch_session.output_tokens;
                t.total_tokens += scratch_session.total_tokens;
                t.runs += scratch_session.runs;
            });
            store.history_cursor.set(new_cursor.clone());
            store.older_turns.set(remaining);
            store.history_loading.set(false);
            let fail_note = if failed > 0 {
                format!(" ({failed} could not be restored — errors name the cause)")
            } else {
                String::new()
            };
            store.notify(format!(
                "streamed {streamed} earlier turn(s){fail_note} — {remaining} more on the gateway"
            ));
        });
        send_fetch_effects(&self.tx, effects);
    }

    /// Capability probe for the reasoning picker stage: one GET, result
    /// posted as a `ReasoningProbe` (supported=None on any failure —
    /// the picker OFFERS with a caveat, never fabricates a lock from a
    /// failed probe; contract-v1 three-state coupling).
    fn probe_model_reasoning(&self, provider: String, model: String) {
        let store = self.store;
        let probe = match self.client.model_capabilities(&provider, &model) {
            Ok(v) => {
                // Tolerant shape: the route serves the registry dict
                // (possibly nested under "capabilities").
                let caps = v.get("capabilities").unwrap_or(&v);
                let supported = caps.get("thinking_support").and_then(Value::as_bool);
                let levels: Vec<String> = caps
                    .get("reasoning_levels")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let source = caps
                    .get("capability_source")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                crate::store::ReasoningProbe {
                    provider,
                    model,
                    supported,
                    levels,
                    source,
                }
            }
            Err(_) => crate::store::ReasoningProbe {
                provider,
                model,
                supported: None,
                levels: Vec::new(),
                source: String::new(),
            },
        };
        self.post(move || store.reasoning_probe.set(Some(probe)));
    }

    fn probe_run_status(&self, run_id: String) {
        let store = self.store;
        let line = match self.client.get_run(&run_id) {
            Ok(v) => {
                let status = v.get("status").and_then(Value::as_str).unwrap_or("?");
                let paused = v.get("paused").and_then(Value::as_bool).unwrap_or(false);
                let node = v.get("current_node").and_then(Value::as_str).unwrap_or("");
                let mut s = status.to_string();
                if paused {
                    s.push_str(" (paused)");
                }
                if !node.is_empty() {
                    s.push_str(&format!(" · node {node}"));
                }
                s
            }
            Err(e) => format!("probe failed: {}", e.compact_reason()),
        };
        self.post(move || store.run_status_probe.set(Some((run_id, line))));
    }

    fn pause(&self, run_id: String) {
        send_verb_blocking(
            &self.client,
            &self.wake,
            self.store,
            crate::store::QuitVerb::Pause,
            run_id,
            &crate::gateway::mint_command_id(),
        );
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

    fn attach(
        &mut self,
        run_id: String,
        started_at: Option<std::time::Instant>,
        probed_session: String,
    ) {
        self.stop_streams();
        let store = self.store;
        // BACKLOG-FIRST (adversary P1-3): replay the live run's already-
        // written history through the same chronological fold terminal
        // turns use, then stream from the bundle's cursors. Streaming
        // everything from 0 replayed per-run in follow order (the
        // misorder class) and — worse — a conclusion inside the root's
        // backlog fired StopFollows against follower streams still
        // posting THEIR backlogs, dropping most of a wrapper turn's
        // detail (measured 85-97% on the real coder tree).
        if let Ok(bundle) = self.client.history_bundle(&run_id, false, 0) {
            let mut backlog = crate::transcript::Fold::new();
            backlog.set_agent_workflows(self.agent_workflow_ids.iter().cloned());
            let mut effects: Vec<FoldEffect> = Vec::new();
            let report = rehydrate_live_backlog_into(&mut backlog, &run_id, &bundle, &mut effects);
            let backlog_finished = backlog.finished;
            let rid = run_id.clone();
            self.post(move || {
                // Session guard (adversary P1-2, same rule as the probe
                // swap and apply_start_binding): a session switch between
                // the probe and this post must not flip the NEW session
                // to Running on the OLD session's run. The streams
                // spawned below still start, but with the swap skipped
                // the fold follows nothing of theirs — every record
                // drops at is_following and the threads die at terminal.
                if store.session_id.with_untracked(|s| *s != probed_session) {
                    return;
                }
                store.run_id.set(rid.clone());
                // A backlog that already carries the conclusion (waiting
                // wrapper roots park long after the agent answered — the
                // standing basic-agent state) reattaches IDLE with the
                // answer on screen; a genuinely mid-turn run reattaches
                // Running.
                store.phase.set(if backlog_finished {
                    Phase::Idle
                } else {
                    Phase::Running
                });
                let anchor = started_at.unwrap_or_else(std::time::Instant::now);
                store.run_started.set(Some(anchor));
                store.elapsed_secs.set(anchor.elapsed().as_secs());
                store.fold.update(|f| {
                    // Same splice as the probe swap: prior turns + boot
                    // items stay ABOVE the live turn's backlog.
                    let mut restored = backlog;
                    let mut items = std::mem::take(&mut f.items);
                    items.append(&mut restored.items);
                    restored.items = items;
                    *f = restored;
                });
            });
            send_fetch_effects(&self.tx, effects);
            // Streams resume AFTER the bundle: the root and every ledger
            // the bundle carried start at their cursors; tree members the
            // records declared but whose ledger entry the bundle lacked
            // (spawned moments before the fetch) start at 0 — their
            // discovering record was consumed by the backlog fold, so no
            // live record will re-announce them.
            let in_bundle: std::collections::HashSet<String> =
                report.cursors.iter().map(|(k, _)| k.clone()).collect();
            for (rid, cursor) in &report.cursors {
                self.spawn_stream_at(run_id.clone(), rid.clone(), rid == &run_id, *cursor);
            }
            if !in_bundle.contains(&run_id) {
                self.spawn_stream_at(run_id.clone(), run_id.clone(), true, 0);
            }
            for sub in report.discovered {
                if !in_bundle.contains(&sub) {
                    self.spawn_stream_at(run_id.clone(), sub, false, 0);
                }
            }
            return;
        }
        // FALLBACK (bundle fetch failed): the pre-backlog behavior — the
        // prompt from input_data, everything streamed from 0. Mis-ordered
        // for deep trees but never silent.
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
            if store.session_id.with_untracked(|s| *s != probed_session) {
                return;
            }
            store.run_id.set(rid.clone());
            store.phase.set(Phase::Running);
            // Honest elapsed on reattach: anchor at the run's gateway
            // created_at when known (back-dated Instant), else now. The
            // seconds counter also resets immediately — the ticker only
            // corrects it on its next 120ms tick, and a stale value from
            // a previous run must never flash.
            let anchor = started_at.unwrap_or_else(std::time::Instant::now);
            store.run_started.set(Some(anchor));
            store.elapsed_secs.set(anchor.elapsed().as_secs());
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
                            f.mark_wait_tools(approved);
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
        send_verb_blocking(
            &self.client,
            &self.wake,
            self.store,
            crate::store::QuitVerb::Cancel,
            run_id,
            &crate::gateway::mint_command_id(),
        );
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
                    // F3: downscale HERE, on the worker thread — the
                    // transcript renders a ≤14-row mosaic, and retaining
                    // a full 4096² decode (~67 MB RGBA) forever bought
                    // nothing but memory.
                    let bitmap = Arc::new(downscale_for_transcript(bitmap));
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

    /// Fetch an offloaded final answer's artifact content and swap the
    /// placeholder card (`Fold::resolve_offloaded_answer`). Content-
    /// addressed by artifact id — no run/session staleness guard needed:
    /// the placeholder is transcript history, and a no-match is a no-op.
    ///
    /// Runs on its OWN thread with a bounded jittered retry (Lane B fix,
    /// 2026-07-23): the one-shot fetch on this command loop lost the
    /// answer FOREVER to a single transport reset in a gateway-bounce
    /// window — while the client was authenticated and the artifact
    /// stayed on the gateway. Retries cover transient classes only
    /// (status-less transport, 408/429/5xx); a hard 4xx will not heal by
    /// waiting. Failure text is `compact_reason()` — URL-free by
    /// contract, because the label travels into `context.messages`.
    fn fetch_answer(&self, run_id: String, artifact_id: String) {
        let store = self.store;
        let client = self.client.clone();
        let wake = self.wake.clone();
        let aid_for_spawn_failure = artifact_id.clone();
        let spawned = std::thread::Builder::new()
            .name("answer-fetch".into())
            .spawn(move || {
                let outcome = fetch_answer_with_retry(&client, &run_id, &artifact_id);
                let aid = artifact_id.clone();
                wake.post(move || {
                    store
                        .fold
                        .update(|f| f.resolve_offloaded_answer(&aid, outcome));
                });
            });
        if spawned.is_err() {
            // Thread spawn failure (fd/thread exhaustion): label the card
            // honestly instead of leaving the placeholder forever.
            self.post(move || {
                store.fold.update(|f| {
                    f.resolve_offloaded_answer(
                        &aid_for_spawn_failure,
                        Err("the client could not start the fetch".to_string()),
                    )
                });
            });
        }
    }
}

/// Fetch an offloaded answer's artifact with a bounded jittered retry —
/// shared by the TUI runner (own thread) and headless exec (synchronous).
/// Transient classes retry (status-less transport, 408/429/5xx — the
/// gateway-bounce window that lost the 2026-07-23 answer); hard 4xx fails
/// fast (a 404 will not heal by waiting). The Err carries
/// `GwError::compact_reason()` — URL-FREE by contract, because failure
/// text lands in transcript cards that replay into `context.messages`.
pub(crate) fn fetch_answer_with_retry(
    client: &GatewayClient,
    run_id: &str,
    artifact_id: &str,
) -> Result<String, String> {
    let mut backoff = Backoff::default();
    let mut outcome: Result<String, String> = Err("not attempted".to_string());
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(backoff.next_delay());
        }
        match client.artifact_bytes(run_id, artifact_id, ARTIFACT_MAX_BYTES) {
            Ok((bytes, content_type)) => {
                outcome = crate::protocol::answer_text_from_artifact(&bytes, &content_type)
                    .ok_or_else(|| "the artifact carries no readable answer text".to_string());
                break;
            }
            Err(e) => {
                let transient = e.status.is_none()
                    || matches!(e.status, Some(408) | Some(429) | Some(500..=599));
                outcome = Err(e.compact_reason());
                if !transient {
                    break;
                }
            }
        }
    }
    outcome
}

impl Runner {
    /// Spawn a stream thread for one run of the active tree.
    fn spawn_stream(&mut self, root_run_id: String, run_id: String, is_root: bool) {
        self.spawn_stream_at(root_run_id, run_id, is_root, 0);
    }

    /// Spawn a follow stream starting AFTER `start_cursor` records — the
    /// live-attach seam (adversary P1-3): a reattach replays the already-
    /// written backlog through the chronological fold and streams only
    /// what comes after it. Live follows (Cmd::Follow) and fresh starts
    /// keep cursor 0.
    fn spawn_stream_at(
        &mut self,
        root_run_id: String,
        run_id: String,
        is_root: bool,
        start_cursor: u64,
    ) {
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
                stream_run(
                    client,
                    wake,
                    store,
                    tx,
                    root_run_id,
                    run_id,
                    is_root,
                    stop,
                    start_cursor,
                );
            }));
            if let Err(payload) = result {
                let msg = panic_text(payload.as_ref());
                let short = panic_run[..panic_run.len().min(8)].to_string();
                panic_wake.post(move || {
                    // User-facing wording: the run's LIVE STREAM died
                    // (the thread name keeps the ledger spelling for
                    // ops/panic traces).
                    store.notify(format!("live stream for run {short} died: {msg}"));
                });
            }
        });
    }
}

/// UI-thread half of a successful start: bind the new run to the store —
/// UNLESS the session changed while the start's HTTP round trip was in
/// flight (`/new` or `/sessions` raced it; cycle-3 audit, cell (f)). A run
/// started FOR session A must never bind into session B's fresh view (the
/// old behavior streamed the orphan run's transcript into the new session
/// and captured its composer): it is cancelled durably instead, and no
/// signal moves. Extracted for unit tests (the closure runs via wake.post).
pub(crate) fn apply_start_binding(
    store: &Store,
    tx: &Sender<Cmd>,
    run_id: &str,
    started_session: &str,
) {
    if store.session_id.with_untracked(|s| s != started_session) {
        let _ = tx.send(Cmd::Cancel {
            run_id: run_id.to_string(),
        });
        store.notify("a run from the previous session started late — cancelled");
        return;
    }
    store.run_id.set(run_id.to_string());
    store.phase.set(Phase::Running);
    store.run_started.set(Some(std::time::Instant::now()));
    store.elapsed_secs.set(0);
    store.paused.set_if_changed(false);
    store.fold.update(|f| f.begin_run(run_id));
    store.conn.set_if_changed(Conn::Ok);
}

/// UI-thread half of a failed start. A failed START is a failed run for
/// the queue lane: pause instead of burning the rest of the queue against
/// a refusing gateway (plan: item popped + paused, no retry loop). Outcome
/// BEFORE phase — the phase flush runs the drain effect synchronously
/// (ordering contract, see `stream_run`). Session-guarded like the success
/// half: a stale failure from the PREVIOUS session must not pause the NEW
/// session's queue or error-card its fresh transcript.
///
/// `gateway_gone` is the runner-side `GwError::is_gone()` classification —
/// this half must never re-derive it from message text: the old
/// `msg.contains("unreachable")` matched OUR OWN Display wording for every
/// status-less error (read timeouts against a busy gateway included), and
/// would also have matched an HTTP 500 whose gateway-served detail merely
/// contained the word (e.g. a proxied "provider endpoint unreachable").
pub(crate) fn apply_start_failure(
    store: &Store,
    msg: &str,
    gateway_gone: bool,
    started_session: &str,
) {
    if store.session_id.with_untracked(|s| s != started_session) {
        store.notify(format!(
            "a start from the previous session failed late: {msg}"
        ));
        return;
    }
    store.last_outcome.set(crate::store::RunOutcome::Failed);
    store.phase.set(Phase::Idle);
    store.fold.update(|f| {
        f.push_item(Item::Error {
            text: format!("run start failed: {msg}"),
        })
    });
    if gateway_gone {
        // Only gone-evidence reaches this branch (see the caller's
        // classification) — the flag is `true` by construction.
        store.conn.set_if_changed(Conn::Down(msg.to_string(), true));
    }
}

/// `/history` prepend (items-only): the live fold's RUN STATE (root,
/// waits, inflight clocks) belongs to the present and is never touched
/// by history. The old stub (ours by prefix) is replaced with the
/// updated remaining-count — or dropped at zero. Pure over the fold so
/// the bloc mechanics are unit-testable without a gateway.
pub fn prepend_history_items(
    f: &mut crate::transcript::Fold,
    scratch_items: Vec<Item>,
    remaining: usize,
) {
    let mut new_items = scratch_items;
    if let Some(pos) = f
        .items
        .iter()
        .position(|i| matches!(i, Item::Info { text } if text.starts_with(OLDER_TURNS_STUB_PREFIX)))
    {
        f.items.remove(pos);
    }
    if remaining > 0 {
        new_items.insert(
            0,
            Item::Info {
                text: history_stub_text(remaining),
            },
        );
    }
    new_items.append(&mut f.items);
    f.items = new_items;
}

/// Canonical stub text for `older` remaining turns — ONE producer for
/// the boot probe, the prepend rewrite, and the failure restore (the
/// dispatch-time "streaming…" rewrite is the only other writer and is
/// always replaced by one of these).
pub fn history_stub_text(older: usize) -> String {
    format!(
        "{OLDER_TURNS_STUB_PREFIX}{older} earlier turn(s) in this session — keep scrolling up to load them)"
    )
}

/// Failure restore: put the canonical text back on a stub the dispatch
/// rewrote to "streaming…" (a retryable list failure must not leave a
/// frozen progress claim).
pub fn restore_history_stub(f: &mut crate::transcript::Fold, older: usize) {
    if older == 0 {
        return;
    }
    if let Some(Item::Info { text }) = f
        .items
        .iter_mut()
        .find(|i| matches!(i, Item::Info { text } if text.starts_with(OLDER_TURNS_STUB_PREFIX)))
    {
        *text = history_stub_text(older);
    }
}

/// UI-thread application of a `/history` LIST failure (retryable): the
/// loading flag drops and the stub returns to its canonical (still
/// true) text — a stub frozen on "streaming…" would lie forever.
/// SESSION-GUARDED like the success post: a switch/`/new` already reset
/// the history lanes, and this failure describes the OLD session — a
/// late post must not notify about (or touch a stub in) a session the
/// load never described.
pub(crate) fn apply_history_list_failure(store: &Store, probed_session: &str, msg: &str) {
    if store.session_id.with_untracked(|s| *s != probed_session) {
        return;
    }
    store.notify(format!("history list failed: {msg}"));
    store.history_loading.set(false);
    let older = store.older_turns.get_untracked();
    store.fold.update(|f| restore_history_stub(f, older));
}

/// UI-thread application of "nothing older on the gateway" (list
/// drift): the stub promised older turns the gateway does not have —
/// remove it rather than leave a claim nothing can satisfy.
/// SESSION-GUARDED (same rule as the failure post above): a stale
/// none-older landing after a session switch used to zero the NEW
/// session's `older_turns` and strip its freshly-restored stub.
pub(crate) fn apply_history_none_older(store: &Store, probed_session: &str) {
    if store.session_id.with_untracked(|s| *s != probed_session) {
        return;
    }
    store.older_turns.set(0);
    store.history_loading.set(false);
    store.fold.update(|f| {
        f.items.retain(|i| {
            !matches!(i, Item::Info { text }
                if text.starts_with(OLDER_TURNS_STUB_PREFIX))
        });
    });
    store.notify("no earlier history on the gateway for this session");
}

/// THE one pause/cancel send authority (quit-delivery plan v2): one
/// bounded HTTP submit with a CALLER-MINTED command_id (the durable
/// store's dedup key — a same-id retry is exactly-once safe, runtime
/// receipt c5541), one same-id retry on TRANSIENT transport errors,
/// then the structured `VerbAck` + toasts posted to the UI thread.
/// Callers: the worker's Cmd::Pause/Cancel handlers (slash commands),
/// and the quit modal's dedicated `quit-verb-send` thread — which
/// exists because this call must never queue behind the worker's
/// sequential loop at quit time (an in-flight artifact fetch could
/// hold it for minutes against a healthy gateway).
pub fn send_verb_blocking(
    client: &GatewayClient,
    wake: &WakeHandle,
    store: Store,
    verb: crate::store::QuitVerb,
    run_id: String,
    command_id: &str,
) {
    let typ = match verb {
        crate::store::QuitVerb::Pause => "pause",
        crate::store::QuitVerb::Cancel => "cancel",
    };
    // AMBIGUOUS = the request may have LEFT and only the response was
    // lost (timeout / body-level transport). Unreachable (connect never
    // made) and HTTP statuses (the server spoke) are unambiguous. Any
    // ambiguous attempt makes a final failure NON-definitive — the
    // command may have landed (adversary D2: a blanket "will NOT land"
    // overclaimed exactly there).
    let ambiguous = |e: &crate::gateway::GwError| e.status.is_none() && !e.is_gone();
    let mut saw_ambiguous = false;
    let mut outcome =
        client.submit_command_with_id(command_id, &run_id, typ, serde_json::json!({}));
    if let Err(e) = &outcome {
        saw_ambiguous |= ambiguous(e);
        if e.is_transient() {
            // SAME id: the dedup key makes the retry exactly-once even
            // if the first attempt was accepted and only its response
            // was lost.
            outcome =
                client.submit_command_with_id(command_id, &run_id, typ, serde_json::json!({}));
        }
    }
    if let Err(e) = &outcome {
        saw_ambiguous |= ambiguous(e);
    }
    match outcome {
        Ok(_) => wake.post(move || {
            match verb {
                crate::store::QuitVerb::Pause => {
                    store.paused.set(true);
                    // "Accepted", not "paused": application lands at
                    // the runner's next poll + step boundary (server
                    // audit — an in-flight LLM call finishes first,
                    // its result ledgered for resume).
                    store.notify(
                        "pause accepted — the run holds at its next step boundary (durable; /resume continues it)",
                    );
                }
                crate::store::QuitVerb::Cancel => store.notify("cancel requested"),
            }
            // Structured ack for the quit sequencer (design §3.2):
            // toast text is never matched.
            store.verb_ack.set(Some(crate::store::VerbAck {
                verb,
                run_id: run_id.clone(),
                ok: true,
                definitive: true,
                error: String::new(),
            }));
        }),
        Err(e) => {
            let msg = e.to_string();
            let definitive = !saw_ambiguous;
            wake.post(move || {
                store.notify(format!("{typ} failed: {msg}"));
                store.verb_ack.set(Some(crate::store::VerbAck {
                    verb,
                    run_id: run_id.clone(),
                    ok: false,
                    definitive,
                    error: msg.clone(),
                }));
            });
        }
    }
}

/// Cache uploaded refs back into the LIVE pending list, merged BY PATH —
/// the user may have removed/added chips while the worker uploaded, so a
/// wholesale replace would clobber their edits. Only the `uploaded` slot
/// transfers; a chip removed mid-flight stays removed (its artifact is
/// already minted server-side — session uploads are permanent — but it
/// rides no run unless re-attached).
pub fn merge_cached_refs(store: &Store, done: &[crate::store::PendingAttachment]) {
    store.pending_attachments.update(|live| {
        for item in live.iter_mut() {
            if item.uploaded.is_none() {
                if let Some(d) = done
                    .iter()
                    .find(|d| d.path == item.path && d.uploaded.is_some())
                {
                    item.uploaded = d.uploaded.clone();
                }
            }
        }
    });
}

/// Custody transfer on a STARTED run: the sent batch leaves the pending
/// list (by path — chips attached mid-flight survive) and a 📎 Info line
/// lands after the user card recording what rode this turn. `Info` never
/// folds into `chat_messages`, so the line can never leak into
/// client-carried context.
pub fn clear_sent_attachments(
    store: &Store,
    tx: &Sender<Cmd>,
    sent: &[crate::store::PendingAttachment],
) {
    store.pending_attachments.update(|live| {
        live.retain(|a| !sent.iter().any(|s| s.path == a.path));
    });
    // The drop-undo slot dies with the send: a Ctrl+O after the chips
    // rode a run would remove nothing and inject stale path text while
    // claiming "drop undone" (P1-2, probe-confirmed) — the attachment
    // is permanent server-side, so there is nothing left to undo.
    store.paste_undo.set(None);
    let line = sent
        .iter()
        .map(|a| format!("{} ({})", a.name, crate::paths::human_size(a.size)))
        .collect::<Vec<_>>()
        .join(" · ");
    store.fold.update(|f| {
        f.push_item(Item::Info {
            text: format!("📎 {line}"),
        });
        // IMAGE attachments echo as a mosaic preview (the attachments
        // design's v2 echo, completed on the operator's ask): the ref
        // carries the artifact + its owning session-memory run, so the
        // normal FetchImage lane renders exactly what was attached.
        for a in sent {
            if let Some((_, r)) = a.uploaded.as_ref() {
                if let Some((run_id, artifact_id)) = image_attachment_ref(r) {
                    f.push_item(Item::Image {
                        run_id: run_id.clone(),
                        artifact_id: artifact_id.clone(),
                        label: format!("attached image: {}", a.name),
                    });
                    let _ = tx.send(Cmd::FetchImage {
                        run_id,
                        artifact_id,
                    });
                }
            }
        }
    });
}

/// (owning run id, artifact id) when this upload ref is an IMAGE —
/// modality first (the server's own classification), content_type
/// prefix as the fallback for older gateways.
pub(crate) fn image_attachment_ref(r: &Value) -> Option<(String, String)> {
    let is_image = r
        .get("modality")
        .and_then(Value::as_str)
        .map(|m| m == "image")
        .unwrap_or_else(|| {
            r.get("content_type")
                .and_then(Value::as_str)
                .is_some_and(|c| c.starts_with("image/"))
        });
    if !is_image {
        return None;
    }
    let artifact_id = r.get("$artifact").and_then(Value::as_str)?.to_string();
    let run_id = r.get("run_id").and_then(Value::as_str)?.to_string();
    Some((run_id, artifact_id))
}

/// The honest overlay rendered when the turn concludes from a SUBRUN's
/// records while the wrapper ROOT run has not reached a terminal status.
///
/// Thin-client doctrine (maintainer, 2026-07-23): the composer release is
/// a CLIENT decision layered over server truth — the gateway's root run
/// often stays `waiting` long after the agent answered (basic-agent
/// 0.0.2/0.0.3 park forever on a status poller; 44 live waiting roots at
/// the audit). The client must never imply the root completed; this line
/// names the divergence exactly once, right where the turn concludes.
/// (See docs/design/thin-client-conformance.md, class ii.)
pub const SUBRUN_CONCLUSION_NOTE: &str =
    "turn concluded — the wrapper root run stays open on the gateway and finalizes server-side";

/// UI-thread half of one posted stream batch: fold the records, dispatch
/// fold effects, sync totals, and conclude the turn when the batch
/// finished it. Extracted from `stream_run`'s post closure for unit tests
/// (the `apply_start_binding` precedent — closures run via `wake.post`).
///
/// Stale-stream guard: the fold update early-returns for records from an
/// abandoned run — and then NOTHING else may touch signals either.
/// (Review finding: totals.set ran unconditionally, so one late batch
/// from a dead stream zeroed the session totals display.)
pub(crate) fn apply_stream_records(
    store: &Store,
    tx: &Sender<Cmd>,
    root: &str,
    rid: &str,
    records: &[Value],
) {
    let mut finished_now = false;
    let mut finished_failed = false;
    let mut session = crate::transcript::SessionStats::default();
    let mut current = false;
    store.fold.update(|f| {
        if f.root_run_id() != root || !f.is_following(rid) {
            return; // stale stream from a previous run
        }
        current = true;
        let was_finished = f.finished;
        for rec in records {
            for fx in f.apply(rid, rec) {
                match fx {
                    FoldEffect::FollowRun(sub) => {
                        let _ = tx.send(Cmd::Follow {
                            root_run_id: root.to_string(),
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
                    FoldEffect::FetchAnswer {
                        run_id,
                        artifact_id,
                    } => {
                        let _ = tx.send(Cmd::FetchAnswer {
                            run_id,
                            artifact_id,
                        });
                    }
                }
            }
        }
        finished_now = f.finished && !was_finished;
        finished_failed = f.failed;
        // Thin-client honesty overlay: a conclusion folded from a
        // NON-ROOT stream means the wrapper root is still open on the
        // gateway (its own stream would have concluded the turn
        // otherwise) — say so instead of silently freeing the composer
        // over a run other apps still see as waiting/running. Root-
        // stream conclusions render nothing (the root really ended).
        if finished_now && rid != root {
            f.push_item(Item::Info {
                text: SUBRUN_CONCLUSION_NOTE.into(),
            });
        }
        session = f.session;
    });
    if !current {
        return; // stale stream: no signal may change
    }
    store.totals.set(SessionTotals {
        input_tokens: session.input_tokens,
        output_tokens: session.output_tokens,
        total_tokens: session.total_tokens,
        runs: session.runs,
    });
    if finished_now {
        // The turn's answer landed. Wrapper bundles may keep helper
        // subflows polling after this; release the composer now and
        // stop the helper streams (the root stream stays to observe
        // the eventual terminal state).
        //
        // ORDERING CONTRACT: the outcome mailbox is written BEFORE
        // the phase flip — signal writes flush effects
        // synchronously outside a dispatch batch, and the
        // queue-drain effect keys on "phase Idle": flipping the
        // phase first ran the drain against an EMPTY mailbox
        // (test-caught: a failed run's queue drained instead of
        // pausing).
        store.last_outcome.set(if finished_failed {
            crate::store::RunOutcome::Failed
        } else {
            crate::store::RunOutcome::Success
        });
        store.run_started.set(None);
        store.phase.set(Phase::Idle);
        let _ = tx.send(Cmd::StopFollows);
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
    start_cursor: u64,
) {
    let mut cursor: u64 = start_cursor;
    // Conn::Down evidence for THIS stream thread: status-less failures in
    // a row (stream connects + REST fallback polls both count) and a
    // once-per-streak latch. Any proof of life — a parsed SSE event, a
    // clean idle/close, a successful REST poll — resets both and clears
    // the orb if this thread flipped it (before this, a Down healed by
    // SSE reconnect rather than REST stayed stuck for the whole run: the
    // idle probe is phase-gated and nothing else wrote Ok mid-run).
    let mut soft_failures: u32 = 0;
    let mut down_reported = false;
    // Jittered exponential backoff for the error path (engine 0.2.6 —
    // reactive::connection's module doc names our previous hand-roll,
    // `(500 * consecutive_errors).min(5000)` ms with NO jitter, as the
    // thundering-herd failure mode: when a gateway restarts, N per-run
    // stream threads all retried in lockstep). `Backoff` is pure math
    // (no scope, no thread affinity — safe on this stream thread): full
    // jitter in [0, min(30s, 500ms × 2^n)]. Reset on every successful
    // read — every parsed SSE step event (below; the reset keys on
    // bytes-parsed, not cursor movement — a replayed cursor still
    // proves the gateway alive), a clean idle close, and each
    // successful REST fallback poll — so a long-healthy stream can
    // never carry grown attempts into its next hiccup. Deliberate deltas vs the old hand-roll, both directions:
    // a dead gateway's retry gaps grow toward 30s (was capped 5s —
    // recovery after a long outage can wait one draw, ≤30s, mean ~15s),
    // while a broken-SSE/healthy-REST gateway is polled HOTTER (reset
    // per successful poll ⇒ draws in [0, 500ms], records at near-live
    // latency — a liveness-over-load choice).
    let mut backoff = Backoff::default();
    let post_records = |cursor_records: Vec<Value>| {
        if cursor_records.is_empty() {
            return;
        }
        let rid = run_id.clone();
        let root = root_run_id.clone();
        let tx = tx.clone();
        wake.post(move || apply_stream_records(&store, &tx, &root, &rid, &cursor_records));
    };

    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let wake_cursor = wake.clone();
        let outcome = client.stream_ledger(
            &run_id,
            cursor,
            &stop,
            // Every parsed step event IS a successful read (this
            // callback fires per parseable envelope, advancing or not —
            // bytes parsed prove the gateway alive): reset the backoff
            // schedule here so grown attempts never survive a healthy
            // stream into its next drop (the "reset on every successful
            // read" contract, made true at the source). The same proof
            // clears the Down evidence — transition-gated so a healthy
            // stream posts nothing.
            |c| {
                cursor = c.max(cursor);
                backoff.reset();
                if soft_failures > 0 || down_reported {
                    soft_failures = 0;
                    down_reported = false;
                    wake_cursor.post(move || {
                        store.conn.set_if_changed(Conn::Ok);
                    });
                }
            },
            // One post per network read: live records reach the UI at
            // arrival cadence (batching across reads held the approval
            // modal hostage — live-verified failure).
            &post_records,
            // F7: malformed/undecodable SSE step payloads are COUNTED and
            // surfaced, never silently dropped — a fold running on a
            // record stream with holes must say so (the cursor already
            // advanced past them; a resync would re-skip the same bytes).
            |n| {
                let wake = wake.clone();
                let run = run_id.clone();
                wake.post(move || {
                    store.fold.update(|f| {
                        // Stale-stream guard (the post_records rule): an
                        // abandoned run's skip notice must not land in
                        // the NEW run's transcript.
                        if !f.is_following(&run) {
                            return;
                        }
                        f.push_item(crate::transcript::Item::Info {
                            text: format!(
                                "{n} undecodable record(s) skipped on {run} — details may be incomplete"
                            ),
                        });
                    });
                });
            },
        );

        match outcome {
            Ok(true) => {
                // Gateway said done: the run is terminal. Drain the REST
                // tail first — records can land in the terminal-save
                // window after the stream's own final drain (the poll
                // path always had this belt; without it here a subrun's
                // flow-end record arriving in that window was never read
                // and `finish` concluded "completed without a readable
                // final answer" over an answered run — cycle-2 review
                // F4). Cursor-based: usually one empty page.
                drain_rest(&client, &run_id, &mut cursor, &post_records);
                finish(&client, &wake, &store, &tx, &root_run_id, &run_id, is_root);
                return;
            }
            Ok(false) => {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                // The stream lane answered (connect + headers + clean
                // idle/close): the gateway is talking — clear any Down
                // this thread flipped.
                if soft_failures > 0 || down_reported {
                    soft_failures = 0;
                    down_reported = false;
                    wake.post(move || {
                        store.conn.set_if_changed(Conn::Ok);
                    });
                }
                backoff.reset();
                // Idle or clean close without done: check status, reconnect.
                match client.get_run(&run_id) {
                    Ok(v) => {
                        let status = v.get("status").and_then(Value::as_str).unwrap_or("");
                        if matches!(status, "completed" | "failed" | "cancelled") {
                            drain_rest(&client, &run_id, &mut cursor, &post_records);
                            finish(&client, &wake, &store, &tx, &root_run_id, &run_id, is_root);
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
                    let run = run_id.clone();
                    wake.post(move || {
                        // Stale-stream guard (the post_records rule —
                        // this arm was the ONE unguarded poster, found
                        // by the 0.2.6 adversary pass): a stop set while
                        // stream_ledger was in flight must not land an
                        // abandoned run's error card in the NEW run's
                        // transcript.
                        if !store.fold.with_untracked(|f| f.is_following(&run)) {
                            return;
                        }
                        store.notify(format!("stream failed: {msg}"));
                        store.fold.update(|f| {
                            f.push_item(crate::transcript::Item::Error {
                                text: format!("run stream refused ({msg}) — check credentials (/doctor, /login)"),
                            });
                        });
                    });
                    if is_root {
                        finish(&client, &wake, &store, &tx, &root_run_id, &run_id, is_root);
                    }
                    return;
                }
                // Down policy (see `marks_gateway_down`): connect-refused
                // flips the orb NOW; status-less soft errors (a 75s header
                // timeout against a busy gateway, a mid-stream reset) flip
                // only after STREAM_DOWN_AFTER in a row; HTTP statuses
                // (5xx/429) prove reachability and never flip — every one
                // of those used to brand the app "gateway unreachable" on
                // the FIRST blip while /api/health answered in ~1ms.
                if e.status.is_none() {
                    soft_failures = soft_failures.saturating_add(1);
                }
                if !down_reported && marks_gateway_down(&e, soft_failures, STREAM_DOWN_AFTER) {
                    down_reported = true;
                    let gone = e.is_gone();
                    let msg = e.to_string();
                    wake.post(move || {
                        store.conn.set_if_changed(Conn::Down(msg, gone));
                    });
                }
                // Poll fallback: the run is durable server-side; keep folding
                // from the REST ledger until the stream comes back. Pacing is
                // the jittered backoff — a successful REST read resets it (the
                // gateway answered; subsequent polls draw near the 500ms base),
                // while a dead gateway grows the draw toward the 30s cap and N
                // stream threads decorrelate instead of retrying in lockstep.
                for _ in 0..8 {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    match client.get_ledger(&run_id, cursor, 500) {
                        Ok((items, next)) => {
                            soft_failures = 0;
                            down_reported = false;
                            backoff.reset();
                            wake.post(move || {
                                store.conn.set_if_changed(Conn::Ok);
                            });
                            cursor = next;
                            post_records(items);
                            if let Ok(v) = client.get_run(&run_id) {
                                let status = v.get("status").and_then(Value::as_str).unwrap_or("");
                                if matches!(status, "completed" | "failed" | "cancelled") {
                                    finish(
                                        &client,
                                        &wake,
                                        &store,
                                        &tx,
                                        &root_run_id,
                                        &run_id,
                                        is_root,
                                    );
                                    return;
                                }
                            }
                        }
                        Err(pe) => {
                            // Failed fallback polls are the same evidence
                            // stream: count them so a genuinely wedged
                            // gateway (accepting, never answering) still
                            // reaches the threshold instead of hiding
                            // behind "the stream will retry".
                            if pe.status.is_none() {
                                soft_failures = soft_failures.saturating_add(1);
                            }
                            if !down_reported
                                && marks_gateway_down(&pe, soft_failures, STREAM_DOWN_AFTER)
                            {
                                down_reported = true;
                                let gone = pe.is_gone();
                                let msg = pe.to_string();
                                wake.post(move || {
                                    store.conn.set_if_changed(Conn::Down(msg, gone));
                                });
                            }
                        }
                    }
                    std::thread::sleep(backoff.next_delay());
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
    tx: &Sender<Cmd>,
    root_run_id: &str,
    run_id: &str,
    is_root: bool,
) {
    // F4: never fabricate "completed" when the status cannot be read —
    // the old `unwrap_or("completed")` turned an unreachable gateway (or
    // a token that expired mid-run) into a false Success outcome, and the
    // queue drained against a dead gateway. Retry briefly (the gateway
    // may be mid-restart), then report the honest "unknown" (error card +
    // Failed outcome via `run_terminal`).
    let mut status: Option<String> = None;
    for attempt in 0..3u32 {
        if let Ok(v) = client.get_run(run_id) {
            if let Some(s) = v.get("status").and_then(Value::as_str) {
                let s = s.trim();
                if !s.is_empty() {
                    status = Some(s.to_string());
                    break;
                }
            }
        }
        if attempt < 2 {
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    let status = status.unwrap_or_else(|| "unknown".into());
    if !is_root {
        // SUBRUN terminal (the failed-agent P0, live tree 76fc3fcb…/
        // 9c5cad22…): the ANSWER-SOURCE agent run reaching a terminal
        // status IS the turn's conclusion when no conclusion record ever
        // folded — the wrapper root absorbs the failure and parks forever
        // on its status poller, so the old "subrun streams end quietly"
        // swallowed the only signal the turn would ever get. The fold's
        // `subrun_terminal` decides (helpers/goal iterations no-op); an
        // "unknown" status never concludes a turn from a subrun — a
        // transient status-read failure on a helper must not kill a
        // healthy run.
        if status == "unknown" {
            return;
        }
        let store = *store;
        let root = root_run_id.to_string();
        let rid = run_id.to_string();
        let tx = tx.clone();
        wake.post(move || {
            let mut concluded_now = false;
            let mut failed = false;
            store.fold.update(|f| {
                if f.root_run_id() != root || !f.is_following(&rid) {
                    return; // stale stream from a previous run
                }
                let was_finished = f.finished;
                f.subrun_terminal(&rid, &status);
                concluded_now = f.finished && !was_finished;
                failed = f.failed;
            });
            if concluded_now {
                // Same ordering contract as stream_run's finished_now
                // branch: outcome mailbox BEFORE the phase flip.
                store.last_outcome.set(if failed {
                    crate::store::RunOutcome::Failed
                } else if status == "cancelled" {
                    crate::store::RunOutcome::Cancelled
                } else {
                    crate::store::RunOutcome::Success
                });
                store.run_started.set(None);
                store.phase.set(Phase::Idle);
                let _ = tx.send(Cmd::StopFollows);
            }
        });
        return;
    }
    let store = *store;
    let root = root_run_id.to_string();
    wake.post(move || {
        let mut current = false;
        let mut was_finished = false;
        store.fold.update(|f| {
            if f.root_run_id() != root {
                return; // a newer run took over; this outcome is history
            }
            current = true;
            was_finished = f.finished;
            f.run_terminal(&status);
        });
        if current {
            // Outcome mailbox for the queue-drain effect — written BEFORE
            // the phase flip (ordering contract; see the finished_now
            // branch in `stream_run`). Skipped when the answer already
            // delivered it (finished_now fired earlier and the fold was
            // already finished) — the root's much-later terminal must not
            // re-trigger a drain that already ran.
            if !was_finished {
                store.last_outcome.set(match status.as_str() {
                    "completed" => crate::store::RunOutcome::Success,
                    "cancelled" => crate::store::RunOutcome::Cancelled,
                    _ => crate::store::RunOutcome::Failed,
                });
            }
            store.run_started.set(None);
            store.phase.set(Phase::Idle);
        }
    });
}

/// Decode-time pixel ceiling for transcript images (F3), contain-fit.
/// The in-feed mosaic renders at most IMAGE_ROWS (14) cell rows — ≤ 56 px
/// tall even on the densest glyph ladder (braille, 2×4 px/cell) — and a
/// few hundred cells wide; this box carries 3× headroom over that, so
/// the pre-scale is invisible in the rendered mosaic while a worst-case
/// entry drops from ~67 MB (4096² RGBA) to ≤ ~0.7 MB. Revisit when
/// protocol-grade rendering (engine backlog 0280) reaches feed blocks —
/// kitty/iTerm2 passthrough would want more resolution than this.
const IMAGE_PX_CEILING: (u32, u32) = (1024, 168);

/// Contain-fit `bitmap` within [`IMAGE_PX_CEILING`] (bilinear; aspect
/// preserved; never upscales). Pure — runs on the worker thread at
/// decode time, so the UI thread only ever sees bounded bitmaps.
pub fn downscale_for_transcript(
    bitmap: abstracttui::widgets::Bitmap,
) -> abstracttui::widgets::Bitmap {
    let (w, h) = (bitmap.width(), bitmap.height());
    let (cw, ch) = IMAGE_PX_CEILING;
    if w <= cw && h <= ch {
        return bitmap;
    }
    let scale = (cw as f64 / w.max(1) as f64).min(ch as f64 / h.max(1) as f64);
    let nw = ((w as f64 * scale).round() as u32).clamp(1, cw);
    let nh = ((h as f64 * scale).round() as u32).clamp(1, ch);
    bitmap.resize_bilinear(nw, nh)
}

/// Fold every turn embedded in a session-history bloc response (gateway
/// `GET /sessions/{id}/history/bloc`) through the same rehydration path
/// as per-run `history_bundle` fetches. Returns (replayed, failed).
fn fold_session_bloc_turns(
    fold: &mut crate::transcript::Fold,
    bloc: &Value,
    effects_out: &mut Vec<FoldEffect>,
) -> (usize, usize) {
    let mut replayed = 0usize;
    let mut failed = 0usize;
    if let Some(warnings) = bloc.get("warnings").and_then(Value::as_array) {
        for w in warnings {
            let text = w
                .as_str()
                .or_else(|| w.get("message").and_then(Value::as_str))
                .or_else(|| w.get("code").and_then(Value::as_str))
                .unwrap_or("partial history");
            fold.push_item(Item::Info {
                text: format!("session history note: {text}"),
            });
        }
    }
    let turns = bloc
        .get("turns")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    for turn in turns {
        let rid = turn.get("run_id").and_then(Value::as_str).unwrap_or("");
        if rid.is_empty() {
            continue;
        }
        let run_failed = matches!(
            turn.get("status").and_then(Value::as_str).unwrap_or(""),
            "failed" | "cancelled"
        );
        let Some(bundle) = turn.get("bundle") else {
            fold.push_item(Item::Error {
                text: format!(
                    "one prior turn had no bundle — run {}",
                    &rid[..rid.len().min(8)]
                ),
            });
            failed += 1;
            continue;
        };
        if rehydrate_run_into(fold, rid, bundle, run_failed, effects_out) {
            replayed += 1;
        }
    }
    (replayed, failed)
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
    push_attachments_line(fold, bundle, effects_out);
    fold.begin_run(root_run_id);
    // Replay: done summaries keep their ledger-true facts but omit
    // elapsed (fold-time instants are not turn durations).
    fold.replay = true;
    let _ = fold_bundle_chronologically(fold, root_run_id, bundle, effects_out);
    fold.replay = false;
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
    // F9: a replayed run that died mid-LLM-call (started with no
    // completion) must not arm the "model call Nm — provider may be
    // slow" hint on an idle, freshly-restored session.
    fold.clear_llm_inflight();
    fold.items.len() > before
}

/// What a chronological bundle fold discovered — the live-attach
/// currency: per-ledger record counts (stream cursors) plus every run
/// id the tree's own records declared (children a live attach must
/// follow even when their ledger entry is missing from the bundle).
pub struct BundleFoldReport {
    /// (run_id, LEDGER cursor of the last folded record) per bundle
    /// ledger — a stream resuming this run starts AFTER this cursor.
    /// The envelope's own `cursor` field, never the folded-record
    /// count (tail-windowed ledgers make the count diverge and a
    /// count-based resume re-serves duplicates — server audit).
    pub cursors: Vec<(String, u64)>,
    /// Run ids discovered via the tree's own spawn/wait records.
    pub discovered: Vec<String>,
    /// Honest-partiality notes: ledgers whose wrapper admits more
    /// records than the window carried (rendered as Info items ahead
    /// of the fold — never a silent hole).
    pub omissions: Vec<String>,
}

/// The shared TWO-PASS chronological fold of a history bundle's ledgers
/// (live P0, 2026-07-23: "when resuming the session I do not get the
/// same messages as before" — the report sat mid-transcript with the
/// whole tree's cycles/tools AFTER it).
///
/// PASS 1 — discovery walk (cycle-2 review F1, unchanged in spirit):
/// BFS from the root through the fold's own FollowRun effects, run
/// against a SCRATCH fold, collecting every record with a per-ledger
/// carried-forward ARRIVAL-time key (`ended_at` first — a completed
/// record reached the live screen when the step ENDED, so keying a
/// 518s tool batch by its start would replay it before sibling records
/// live rendered first (adversary P2-5) — `started_at` fallback).
/// Discovery order is the answer-binding invariant's floor (an unknown
/// parent reads as first-level; a deep child folding before its
/// discovering record would bind as the answer source). Ledgers
/// unreachable by discovery (trimmed captures) collect last, in id
/// order, so no recorded work is hidden.
///
/// PASS 2 — chronological STABLE sort by that key, then the real fold.
/// Live, records from every stream in the tree interleave by arrival;
/// folding whole ledgers sequentially put the root's ledger (which
/// ENDS with the final answer) first and every child's cycles/tools
/// after the answer. Real gateway records always carry timestamps, so
/// the sort reproduces the live interleave — and chronology implies
/// discovery order live (a child's first record cannot precede the
/// parent record that spawned it). Timestamp-less captures (trimmed
/// test fixtures) all key as "" and the STABLE sort preserves pass-1
/// order — the F1 behavior, byte-identical, by construction.
///
/// Wire-shape tolerances carried from the original reader:
/// `{run_id, total, items}` wrappers, bare arrays, `{cursor, record}`
/// envelopes, bare records.
fn fold_bundle_chronologically(
    fold: &mut crate::transcript::Fold,
    root_run_id: &str,
    bundle: &Value,
    effects_out: &mut Vec<FoldEffect>,
) -> BundleFoldReport {
    let mut report = BundleFoldReport {
        cursors: Vec::new(),
        discovered: Vec::new(),
        omissions: Vec::new(),
    };
    // Server-declared degradations (runtime R4, 2026-07-25: the bundle
    // now reports every degradation it survives — subtree discovery
    // failures, tail windows, torn-row skips; empty list = clean
    // export). A bundle that says it cannot be complete renders that
    // ahead of the fold — the operator's no-silent-failing ruling,
    // server half. Schema-tolerant: strings or objects (kind + detail
    // preferred), capped like the splash notices.
    if let Some(warnings) = bundle.get("warnings").and_then(Value::as_array) {
        const WARN_CAP: usize = 6;
        for w in warnings.iter().take(WARN_CAP) {
            let text = match w {
                Value::String(s) => s.trim().to_string(),
                Value::Object(o) => {
                    let kind = ["kind", "code", "warning"]
                        .iter()
                        .find_map(|k| o.get(*k).and_then(Value::as_str))
                        .unwrap_or("");
                    let detail = ["detail", "message", "text"]
                        .iter()
                        .find_map(|k| o.get(*k).and_then(Value::as_str))
                        .unwrap_or("");
                    match (kind.is_empty(), detail.is_empty()) {
                        (false, false) => format!("{kind}: {detail}"),
                        (false, true) => kind.to_string(),
                        (true, false) => detail.to_string(),
                        (true, true) => w.to_string(),
                    }
                }
                other => other.to_string(),
            };
            if !text.is_empty() {
                report.omissions.push(format!("(history export: {text})"));
            }
        }
        if warnings.len() > WARN_CAP {
            report.omissions.push(format!(
                "(history export: +{} more warning(s))",
                warnings.len() - WARN_CAP
            ));
        }
    }
    let Some(ledgers) = bundle.get("ledgers").and_then(Value::as_object) else {
        // A ledger-less bundle still surfaces its warnings.
        for note in &report.omissions {
            fold.push_item(Item::Info { text: note.clone() });
        }
        return report;
    };
    let mut scratch = crate::transcript::Fold::new();
    scratch.begin_run(root_run_id);
    let mut keyed: Vec<(String, String, Value)> = Vec::new(); // (ts, run_id, rec)
    let collect = |scratch: &mut crate::transcript::Fold,
                   run_id: &str,
                   entry: &Value,
                   keyed: &mut Vec<(String, String, Value)>,
                   report: &mut BundleFoldReport|
     -> Vec<String> {
        let mut discovered = Vec::new();
        let records = entry.get("items").or(Some(entry)).and_then(Value::as_array);
        let mut last_ts = String::new();
        let mut count: u64 = 0;
        // Resume cursor: the LAST envelope's own `cursor` field — the
        // ledger's absolute position. The folded-record COUNT diverges
        // on tail-windowed ledgers (server audit: a long-waiting root
        // crosses the 2,000-record window in ~75min; a count-based
        // resume then re-serves duplicates).
        let mut last_cursor: Option<u64> = None;
        if let Some(records) = records {
            for item in records {
                if let Some(c) = item.get("cursor").and_then(Value::as_u64) {
                    last_cursor = Some(c);
                }
                let rec = item.get("record").filter(|r| r.is_object()).unwrap_or(item);
                let ts = rec
                    .get("ended_at")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| {
                        rec.get("started_at")
                            .and_then(Value::as_str)
                            .filter(|s| !s.trim().is_empty())
                    });
                if let Some(ts) = ts {
                    last_ts = ts.trim().to_string();
                }
                for fx in scratch.apply(run_id, rec) {
                    if let FoldEffect::FollowRun(sub) = fx {
                        discovered.push(sub);
                    }
                }
                keyed.push((last_ts.clone(), run_id.to_string(), rec.clone()));
                count += 1;
            }
        }
        // Honest partiality (server audit: tail windows + torn-row
        // skips degrade to SILENT omission server-side — the client
        // must at least name what the wrapper admits to).
        if let Some(total) = entry.get("total").and_then(Value::as_u64) {
            if total > count {
                report.omissions.push(format!(
                    "({} older record(s) of run {} were not in this replay window)",
                    total - count,
                    &run_id[..run_id.len().min(8)]
                ));
            }
        }
        report
            .cursors
            .push((run_id.to_string(), last_cursor.unwrap_or(count)));
        discovered
    };
    let mut queue: Vec<String> = vec![root_run_id.to_string()];
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut qi = 0usize;
    while qi < queue.len() {
        let k = queue[qi].clone();
        qi += 1;
        if !visited.insert(k.clone()) {
            continue;
        }
        if let Some(entry) = ledgers.get(&k) {
            let found = collect(&mut scratch, &k, entry, &mut keyed, &mut report);
            for sub in &found {
                report.discovered.push(sub.clone());
            }
            queue.extend(found);
        }
    }
    let mut leftovers: Vec<&String> = ledgers
        .keys()
        .filter(|k| !visited.contains(k.as_str()))
        .collect();
    leftovers.sort();
    for k in leftovers {
        let _ = collect(
            &mut scratch,
            k,
            &ledgers[k.as_str()],
            &mut keyed,
            &mut report,
        );
    }
    // Stable by construction (`sort_by` is stable): equal keys keep
    // pass-1 discovery order.
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    for note in &report.omissions {
        fold.push_item(Item::Info { text: note.clone() });
    }
    for (_ts, rid, rec) in &keyed {
        for fx in fold.apply(rid, rec) {
            match fx {
                FoldEffect::FollowRun(_) => {}
                FoldEffect::FetchImage { .. } | FoldEffect::FetchAnswer { .. } => {
                    effects_out.push(fx)
                }
            }
        }
    }
    report
}

/// Rehydrate a LIVE run's already-written backlog (adversary P1-3: the
/// attach door replayed per-run in follow order — the same misorder the
/// terminal-replay fix killed — and a conclusion inside a replayed
/// backlog raced `StopFollows` against followers still posting their
/// history, measurably dropping 85-97% of a wrapper turn's detail).
/// Differences from the terminal path, all deliberate:
/// - `pending_wait` SURVIVES — a live run parked on an approval must
///   re-prompt after the swap;
/// - inflight clocks SURVIVE (back-dated by `inflight_anchor`) — the
///   strip shows honest elapsed for a mid-execution reattach;
/// - no "(this turn ended without an answer)" card — the turn is live;
/// - returns the per-ledger cursors + discovered ids so the caller
///   streams from where the bundle ends instead of replaying from 0.
pub fn rehydrate_live_backlog_into(
    fold: &mut crate::transcript::Fold,
    root_run_id: &str,
    bundle: &Value,
    effects_out: &mut Vec<FoldEffect>,
) -> BundleFoldReport {
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
    push_attachments_line(fold, bundle, effects_out);
    fold.begin_run(root_run_id);
    // Same replay rule as the terminal-path fold: a LIVE run's backlog
    // normally doesn't conclude, but a just-concluded race would push
    // a summary — its elapsed must not read as fold time.
    fold.replay = true;
    let report = fold_bundle_chronologically(fold, root_run_id, bundle, effects_out);
    fold.replay = false;
    report
}

/// Rehydrate the `📎` line (and image-attachment previews) from
/// `input_data.context.attachments` — the restore renders what the live
/// send recorded (filename from the ref; refs carry no size, so
/// name-only). Both bundle folds call this right after the user card,
/// mirroring the live `clear_sent_attachments` placement.
fn push_attachments_line(
    fold: &mut crate::transcript::Fold,
    bundle: &Value,
    effects_out: &mut Vec<FoldEffect>,
) {
    let refs: Vec<Value> = bundle
        .get("input_data")
        .and_then(|i| i.get("context"))
        .and_then(|c| c.get("attachments"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if refs.is_empty() {
        return;
    }
    let names: Vec<String> = refs
        .iter()
        .filter_map(|r| {
            r.get("filename")
                .and_then(Value::as_str)
                .or_else(|| r.get("$artifact").and_then(Value::as_str))
                .map(str::to_string)
        })
        .collect();
    if !names.is_empty() {
        fold.push_item(Item::Info {
            text: format!("📎 {}", names.join(" · ")),
        });
    }
    for r in &refs {
        if let Some((run_id, artifact_id)) = image_attachment_ref(r) {
            let name = r.get("filename").and_then(Value::as_str).unwrap_or("image");
            fold.push_item(Item::Image {
                run_id: run_id.clone(),
                artifact_id: artifact_id.clone(),
                label: format!("attached image: {name}"),
            });
            effects_out.push(FoldEffect::FetchImage {
                run_id,
                artifact_id,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            "input_data": {"prompt": "add a test",
                           "context": {"attachments": [
                               {"$artifact": "a1", "filename": "spec.md"}]}},
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
        // The 📎 record rehydrates from input_data.context.attachments
        // (impl-review P2-2): restores render what the live send
        // recorded, name-only (refs carry no size).
        assert!(
            fold.items
                .iter()
                .any(|i| matches!(i, Item::Info { text } if text == "📎 spec.md")),
            "attachments line rehydrates after the user card"
        );
    }

    #[test]
    fn history_prepend_replaces_the_stub_and_keeps_run_state() {
        use crate::transcript::Fold;
        // A live fold: stub + one restored turn + the live run's state.
        let mut f = Fold::new();
        f.push_item(Item::Info {
            text: format!("{OLDER_TURNS_STUB_PREFIX}5 earlier turn(s) in this session — keep scrolling up to load them)"),
        });
        f.push_item(Item::User {
            text: "turn N".into(),
        });
        f.begin_run("live-run");
        let root_before = f.root_run_id().to_string();
        // A streamed bloc of two older turns, 3 remaining beyond it.
        let scratch = vec![
            Item::User {
                text: "turn N-2".into(),
            },
            Item::User {
                text: "turn N-1".into(),
            },
        ];
        prepend_history_items(&mut f, scratch, 3);
        // Order: updated stub, then the streamed bloc, then the present.
        assert!(
            matches!(&f.items[0], Item::Info { text } if text.contains("3 earlier turn(s)")),
            "stub replaced with the updated count: {:?}",
            f.items[0]
        );
        assert!(matches!(&f.items[1], Item::User { text } if text == "turn N-2"));
        assert!(matches!(&f.items[2], Item::User { text } if text == "turn N-1"));
        assert!(matches!(&f.items[3], Item::User { text } if text == "turn N"));
        assert_eq!(
            f.items
                .iter()
                .filter(|i| matches!(i, Item::Info { text } if text.starts_with(OLDER_TURNS_STUB_PREFIX)))
                .count(),
            1,
            "exactly one stub"
        );
        assert_eq!(f.root_run_id(), root_before, "run state untouched");
        // Final bloc: remaining 0 → the stub disappears.
        prepend_history_items(
            &mut f,
            vec![Item::User {
                text: "turn 1".into(),
            }],
            0,
        );
        assert!(
            !f.items.iter().any(
                |i| matches!(i, Item::Info { text } if text.starts_with(OLDER_TURNS_STUB_PREFIX))
            ),
            "no stub once everything streamed"
        );
        assert!(matches!(&f.items[0], Item::User { text } if text == "turn 1"));
    }

    /// A runner panic mid `LoadHistory`/`ProbeAttach` must not leave the
    /// history lanes lying: pre-fix, the panic post reset only `phase`,
    /// so `restoring`/`history_loading` stayed armed forever — the idle
    /// strip claimed "streaming earlier history…", `/history` and the
    /// scroll-top auto-loader died silently on the in-flight guard, and
    /// the stub froze on "streaming N of M…".
    #[test]
    fn worker_death_resets_history_lanes_and_restores_the_stub() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            store.phase.set(Phase::Running);
            store.restoring.set(true);
            store.history_loading.set(true);
            store.older_turns.set(4);
            store.fold.update(|f| {
                f.push_item(Item::Info {
                    text: format!("{OLDER_TURNS_STUB_PREFIX}streaming 2 of 4 earlier turn(s)…)"),
                })
            });
            apply_worker_death(&store, "boom");
            assert_eq!(store.phase.get_untracked(), Phase::Idle);
            assert!(!store.restoring.get_untracked());
            assert!(!store.history_loading.get_untracked());
            store.fold.with_untracked(|f| {
                assert!(f.items.iter().any(
                    |i| matches!(i, Item::Error { text } if text.contains("gateway worker died"))
                ));
                assert!(
                    f.items.iter().any(|i| matches!(i, Item::Info { text }
                        if text.starts_with(OLDER_TURNS_STUB_PREFIX)
                            && text.contains("4 earlier turn(s)")
                            && !text.contains("streaming"))),
                    "the frozen 'streaming…' stub returns to canonical text"
                );
            });
            // Without an in-flight bloc, nothing touches the fold's stub
            // state (the flag gate keeps the restore one-shot).
            apply_worker_death(&store, "boom again");
            assert!(!store.history_loading.get_untracked());
        });
        root.dispose();
    }

    /// Stale `/history` posts (list failure / none-older) landing after
    /// a session switch must not touch the NEW session: pre-fix these
    /// two posts were the only unguarded ones in the lane — a stale
    /// none-older zeroed the new session's `older_turns` and stripped
    /// its freshly-restored stub, plus a wrong-session notify.
    #[test]
    fn stale_history_posts_never_touch_a_switched_session() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            store.session_id.set("session-B".into());
            store.older_turns.set(5);
            store
                .history_cursor
                .set(Some("2026-01-01T00:00:00Z".into()));
            store.fold.update(|f| {
                f.push_item(Item::Info {
                    text: history_stub_text(5),
                })
            });
            // Stale posts from a LoadHistory dispatched under session-A.
            apply_history_none_older(&store, "session-A");
            assert_eq!(
                store.older_turns.get_untracked(),
                5,
                "a stale none-older must not zero the new session's count"
            );
            store.fold.with_untracked(|f| {
                assert!(
                    f.items.iter().any(|i| matches!(i, Item::Info { text }
                        if text.starts_with(OLDER_TURNS_STUB_PREFIX))),
                    "the new session's stub survives the stale post"
                );
            });
            apply_history_list_failure(&store, "session-A", "boom");
            assert!(
                !store
                    .notices
                    .get_untracked()
                    .iter()
                    .any(|n| n.contains("history list failed")),
                "a stale failure never notifies the new session"
            );
            // Same-session applications keep their full behavior.
            store.history_loading.set(true);
            apply_history_list_failure(&store, "session-B", "boom");
            assert!(!store.history_loading.get_untracked());
            assert!(store
                .notices
                .get_untracked()
                .iter()
                .any(|n| n.contains("history list failed")));
            store.history_loading.set(true);
            apply_history_none_older(&store, "session-B");
            assert_eq!(store.older_turns.get_untracked(), 0);
            assert!(!store.history_loading.get_untracked());
            store.fold.with_untracked(|f| {
                assert!(
                    !f.items.iter().any(|i| matches!(i, Item::Info { text }
                        if text.starts_with(OLDER_TURNS_STUB_PREFIX))),
                    "the same-session none-older removes the stub"
                );
            });
        });
        root.dispose();
    }

    #[test]
    fn catalog_change_note_speaks_only_on_a_changed_refresh() {
        let wf = |b: &str, f: &str| Workflow {
            bundle_id: b.into(),
            flow_id: f.into(),
            name: String::new(),
            description: String::new(),
        };
        let boot = vec![wf("basic-agent", "81795ea9")];
        // Boot (prev empty): silent regardless of what arrives.
        assert_eq!(catalog_change_note(&[], &boot), None);
        // Unchanged refresh: silent (the common /workflow open).
        assert_eq!(catalog_change_note(&boot, &boot), None);
        // Entrypoints registered while the session lived: says so
        // (an open picker renders the change live; the note serves the
        // no-picker-open moment).
        let next = vec![
            wf("basic-agent", "81795ea9"),
            wf("entity-life", "entity-chat"),
            wf("multiagent-coding", "multiagent-coder"),
        ];
        let note = catalog_change_note(&boot, &next).expect("changed catalog notes");
        assert!(note.contains("2 new"), "{note}");
        assert!(note.contains("/workflow"), "{note}");
        // Tombstoned/unpublished bundles are a change too.
        let note = catalog_change_note(&next, &boot).expect("removal notes");
        assert!(note.contains("2 removed"), "{note}");
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
    fn fold_session_bloc_turns_rehydrates_each_embedded_bundle() {
        let bundle = json!({
            "input_data": {"prompt": "turn one"},
            "ledgers": {
                "r1": {"run_id": "r1", "total": 1, "items": [
                    {"run_id": "r1", "node_id": "a", "status": "completed",
                     "effect": {"type": "answer_user", "payload": {"text": "one"}},
                     "result": {"output": "one"}}
                ]}
            }
        });
        let bloc = json!({
            "cursor_after": "2026-07-28T08:00:00Z",
            "older_remaining": 2,
            "warnings": ["ledger tail truncated"],
            "turns": [
                {"run_id": "r1", "status": "completed", "bundle": bundle},
                {"run_id": "r2", "status": "failed"}
            ]
        });
        let mut fold = crate::transcript::Fold::new();
        let mut fx = Vec::new();
        let (replayed, failed) = fold_session_bloc_turns(&mut fold, &bloc, &mut fx);
        assert_eq!(replayed, 1);
        assert_eq!(failed, 1);
        assert!(fold.items.iter().any(|i| matches!(i, Item::User { .. })));
        assert!(fold
            .items
            .iter()
            .any(|i| matches!(i, Item::Info { text } if text.contains("truncated"))));
    }

    #[test]
    fn start_outcomes_from_a_previous_session_never_bind_or_pause_the_new_one() {
        // Cycle-3 audit, cell (f): /new — /sessions can race an in-flight
        // start's HTTP round trip. The late Ok must CANCEL the orphan run
        // (never bind it into the fresh session's view); the late Err must
        // not error-card the fresh transcript or write the Failed outcome
        // that would pause the NEW session's queue.
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
            store.session_id.set("session-B".into());

            // Ok landing for a run started under session-A.
            apply_start_binding(&store, &tx, "orphan-run", "session-A");
            match rx.try_recv() {
                Ok(Cmd::Cancel { run_id }) => assert_eq!(run_id, "orphan-run"),
                other => panic!("expected a durable cancel, got {other:?}"),
            }
            assert_eq!(
                store.run_id.get_untracked(),
                "",
                "the orphan run never binds"
            );
            assert_eq!(store.phase.get_untracked(), Phase::Idle);
            assert_eq!(
                store.fold.with_untracked(|f| f.root_run_id().to_string()),
                "",
                "begin_run never fires for the orphan"
            );

            // Err landing for a session-A start: no outcome, no error card.
            apply_start_failure(&store, "boom", false, "session-A");
            assert_eq!(
                store.last_outcome.get_untracked(),
                crate::store::RunOutcome::None,
                "a stale failure must not pause the new session's queue"
            );
            assert_eq!(store.fold.with_untracked(|f| f.items.len()), 0);

            // Same-session outcomes still work exactly as before.
            apply_start_binding(&store, &tx, "live-run", "session-B");
            assert_eq!(store.run_id.get_untracked(), "live-run");
            assert_eq!(store.phase.get_untracked(), Phase::Running);
            assert!(store.fold.with_untracked(|f| f.is_following("live-run")));
            store.phase.set(Phase::Idle);
            apply_start_failure(&store, "boom", false, "session-B");
            assert_eq!(
                store.last_outcome.get_untracked(),
                crate::store::RunOutcome::Failed
            );
        });
        root.dispose();
    }

    /// The false-"unreachable" regression, policy half (operator report
    /// 2026-07-23): the orb may claim Down only on gone-EVIDENCE —
    /// connect-refused now, or a run of status-less soft failures — and
    /// never on an HTTP answer (which PROVES reachability).
    #[test]
    fn down_policy_requires_gone_evidence_or_persistence() {
        let refused = GwError::unreachable("/ping: Connection Failed: refused");
        let timeout = GwError::timeout("/ping: Network Error: timed out reading response");
        let reset = GwError::transport("stream read failed: connection reset");
        let http = GwError::http(500, "internal error");

        // Connect-level evidence flips immediately, streak length aside.
        assert!(marks_gateway_down(&refused, 0, STREAM_DOWN_AFTER));
        assert!(marks_gateway_down(&refused, 1, PROBE_DOWN_AFTER));

        // One soft blip (the busy-gateway shape) NEVER flips…
        assert!(!marks_gateway_down(&timeout, 1, STREAM_DOWN_AFTER));
        assert!(!marks_gateway_down(&reset, 1, STREAM_DOWN_AFTER));
        assert!(!marks_gateway_down(&timeout, 1, PROBE_DOWN_AFTER));
        // …persistence does (the wedged-but-accepting gateway).
        assert!(marks_gateway_down(
            &timeout,
            STREAM_DOWN_AFTER,
            STREAM_DOWN_AFTER
        ));
        assert!(marks_gateway_down(
            &timeout,
            PROBE_DOWN_AFTER,
            PROBE_DOWN_AFTER
        ));

        // An HTTP answer of any code is reachability proof: never Down,
        // however long the error streak.
        assert!(!marks_gateway_down(&http, 99, STREAM_DOWN_AFTER));
    }

    /// F1 under the evidence-based Down policy: a boot whose catalog load
    /// failed SOFTLY (timeout against a busy gateway — no orb flip, so no
    /// Down→Ok reload edge) must still recover once probes reach the
    /// gateway; a loaded catalog (or one never attempted) must not churn.
    #[test]
    fn probe_heal_reloads_only_an_attempted_never_loaded_catalog() {
        let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
        let pref = (
            Some("basic-agent".to_string()),
            Some("81795ea9".to_string()),
        );

        // Boot's own load hasn't run yet: never pre-empt it.
        heal_catalog_if_missing(&tx, false, false, &pref);
        assert!(rx.try_recv().is_err(), "no heal before the boot load ran");

        // Attempted and failed: re-issue BOTH loads with the preference.
        heal_catalog_if_missing(&tx, true, false, &pref);
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
        assert!(matches!(rx.try_recv(), Ok(Cmd::LoadTools)));

        // Loaded: the heal is over, probes stay quiet.
        heal_catalog_if_missing(&tx, true, true, &pref);
        assert!(rx.try_recv().is_err(), "a loaded catalog never re-churns");
    }

    /// The substring half of the same regression: `apply_start_failure`
    /// used to match `msg.contains("unreachable")` — a wording our own
    /// Display minted for EVERY status-less error, and which a gateway
    /// HTTP 500 detail containing the word would also have tripped. The
    /// classification bit now travels from the runner thread; the text
    /// is never consulted.
    #[test]
    fn start_failure_flips_conn_on_classification_not_message_text() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            store.session_id.set("s".into());
            store.conn.set(Conn::Ok);

            // An HTTP failure whose detail CONTAINS the word: the old
            // substring match would have flipped Down over a gateway
            // that just ANSWERED with a 500.
            apply_start_failure(
                &store,
                "gateway HTTP 500: provider endpoint unreachable (proxied detail)",
                false,
                "s",
            );
            assert_eq!(
                store.conn.get_untracked(),
                Conn::Ok,
                "an answered start failure never claims the gateway is gone"
            );

            // Gone-classified (connect refused at the typed-error site):
            // Down still surfaces — the genuine-outage case survives.
            apply_start_failure(
                &store,
                "gateway unreachable: /runs/start: Connection Failed",
                true,
                "s",
            );
            assert!(
                matches!(store.conn.get_untracked(), Conn::Down(..)),
                "a refused start still flips the orb"
            );
        });
        root.dispose();
    }

    #[test]
    fn transcript_downscale_bounds_dimensions_and_preserves_aspect() {
        // F3: a huge decode is contained within the ceiling (never
        // retained full-res), aspect preserved; small images unchanged.
        let (cw, ch) = IMAGE_PX_CEILING;
        let big = abstracttui::widgets::Bitmap::new(2048, 2048, abstracttui::prelude::Rgba::BLACK);
        let scaled = downscale_for_transcript(big);
        assert!(
            scaled.width() <= cw && scaled.height() <= ch,
            "stored dims ≤ ceiling: {}x{}",
            scaled.width(),
            scaled.height()
        );
        assert_eq!(
            scaled.width(),
            scaled.height(),
            "square stays square (aspect preserved)"
        );

        // A wide panorama binds on width, not height.
        let wide = abstracttui::widgets::Bitmap::new(4096, 256, abstracttui::prelude::Rgba::BLACK);
        let scaled = downscale_for_transcript(wide);
        assert!(scaled.width() <= cw && scaled.height() <= ch);
        let aspect = scaled.width() as f64 / scaled.height() as f64;
        assert!((aspect - 16.0).abs() < 0.5, "aspect ~16:1, got {aspect}");

        // Already inside the ceiling: byte-identical passthrough (never
        // upscaled, never re-sampled).
        let small = abstracttui::widgets::Bitmap::new(320, 100, abstracttui::prelude::Rgba::BLACK);
        let kept = downscale_for_transcript(small);
        assert_eq!((kept.width(), kept.height()), (320, 100));
    }

    #[test]
    fn live_backlog_keeps_the_pending_wait_and_returns_cursors() {
        // Adversary P1-3: the live-attach door replays the backlog
        // through the SAME chronological fold, but with live semantics —
        // a parked approval must re-prompt after the swap (the terminal
        // path deliberately clears it), and the caller streams from the
        // bundle's cursors instead of 0.
        let bundle = json!({
            "input_data": {"prompt": "do the thing"},
            "ledgers": {
                "root1": {"run_id": "root1", "total": 2, "items": [
                    {"run_id": "root1", "node_id": "act", "status": "started",
                     "started_at": "2026-07-23T12:00:01+00:00",
                     "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                         {"name": "write_file", "arguments": {"path": "a"}}
                     ]}}},
                    {"run_id": "root1", "node_id": "act", "status": "waiting",
                     "started_at": "2026-07-23T12:00:02+00:00",
                     "effect": {"type": "tool_calls", "payload": {"tool_calls": [
                         {"name": "write_file", "arguments": {"path": "a"}}
                     ]}},
                     "result": {"wait": {"reason": "user",
                        "wait_key": "tool_approval:k1",
                        "details": {"mode": "approval_required", "tool_calls": [
                            {"name": "write_file", "arguments": {"path": "a"}}
                        ]}}}}
                ]}
            }
        });
        let mut fold = crate::transcript::Fold::new();
        let mut fx = Vec::new();
        let report = rehydrate_live_backlog_into(&mut fold, "root1", &bundle, &mut fx);
        assert!(
            fold.pending_wait.is_some(),
            "a live run's parked approval survives the backlog replay"
        );
        assert!(!fold.finished, "no conclusion in the backlog");
        assert_eq!(
            report.cursors,
            vec![("root1".to_string(), 2)],
            "streams resume after the bundle's records"
        );
        assert!(
            fold.items
                .iter()
                .any(|i| matches!(i, Item::User { text } if text == "do the thing")),
            "the prompt replays from the bundle's input_data"
        );
    }

    #[test]
    fn rehydrate_orders_records_chronologically_across_tree_ledgers() {
        // Live P0 (2026-07-23, "when resuming the session I do not get the
        // same messages as before"): folding whole ledgers sequentially
        // put the ROOT's ledger — which ends with the final answer —
        // first, and every child's cycles/tools AFTER the answer. Live,
        // records interleave by time and the answer is LAST. Timestamped
        // bundles must replay in that live order.
        let bundle = json!({
            "input_data": {"prompt": "build the thing"},
            "ledgers": {
                "root1": {"run_id": "root1", "total": 2, "items": [
                    {"run_id": "root1", "status": "waiting",
                     "started_at": "2026-07-23T12:00:01+00:00",
                     "result": {"wait": {"reason": "subworkflow",
                        "wait_key": "subworkflow:child1",
                        "details": {"sub_run_id": "child1"}}}},
                    // The root's own conclusion — LAST live, but sitting
                    // in the FIRST ledger of the bundle.
                    {"run_id": "root1", "node_id": "flow_end", "status": "completed",
                     "started_at": "2026-07-23T12:00:09+00:00",
                     "effect": {"type": "flow_output", "payload": {}},
                     "result": {"output": {"response": "THE REPORT"}, "completed": true}}
                ]},
                "child1": {"run_id": "child1", "total": 2, "items": [
                    {"run_id": "child1", "node_id": "reason", "status": "started",
                     "started_at": "2026-07-23T12:00:02+00:00",
                     "effect": {"type": "llm_call", "payload": {}}},
                    {"run_id": "child1", "node_id": "reason", "status": "completed",
                     "started_at": "2026-07-23T12:00:02+00:00",
                     "effect": {"type": "llm_call", "payload": {}},
                     "result": {"content": "thinking about it",
                                 "usage": {"input_tokens": 5, "output_tokens": 2}}}
                ]}
            }
        });
        let mut fold = crate::transcript::Fold::new();
        let mut fx = Vec::new();
        assert!(rehydrate_run_into(
            &mut fold, "root1", &bundle, false, &mut fx
        ));
        let answer_pos = fold
            .items
            .iter()
            .position(|i| matches!(i, Item::Assistant { text, .. } if text.contains("THE REPORT")))
            .expect("the answer replays");
        let thinking_pos = fold
            .items
            .iter()
            .position(|i| matches!(i, Item::Thinking { content, .. } if content.contains("thinking about it")))
            .expect("the child cycle replays");
        assert!(
            thinking_pos < answer_pos,
            "chronology: the child's cycle precedes the final answer (live order), got items {:#?}",
            fold.items
        );
        let last_content = fold
            .items
            .iter()
            .rev()
            .find(|i| {
                !matches!(i, Item::Info { text }
                    if text.starts_with("✓ ") || text.starts_with("✗ ") || text.starts_with("⊘ "))
            })
            .expect("items");
        assert!(
            matches!(last_content, Item::Assistant { text, .. } if text.contains("THE REPORT")),
            "the report is the last CONTENT item, as it was live (the done summary follows it)"
        );
    }

    #[test]
    fn rehydrate_folds_in_discovery_order_never_id_order() {
        // Cycle-2 review F1: bundle ledgers used to fold in id order, so a
        // deep cycling child whose run id sorts BEFORE its parent's ledger
        // folded parentless (unknown parent ⇒ treated first-level), bound
        // as the answer source, and its INTERMEDIATE flow end rendered as
        // the restored turn's final answer — reachable whenever the root
        // never completed (failed/cancelled roots; older waiting wrapper
        // roots). Ids here are adversarial: "aaa-child" < "mmm-level1".
        let bundle = json!({
            "input_data": {"prompt": "build the thing"},
            "ledgers": {
                "root1": {"run_id": "root1", "total": 1, "items": [
                    {"run_id": "root1", "status": "waiting",
                     "result": {"wait": {"reason": "subworkflow",
                        "wait_key": "subworkflow:mmm-level1",
                        "details": {"sub_run_id": "mmm-level1"}}}}
                ]},
                "mmm-level1": {"run_id": "mmm-level1", "total": 1, "items": [
                    {"run_id": "mmm-level1", "status": "waiting",
                     "result": {"wait": {"reason": "subworkflow",
                        "wait_key": "subworkflow:aaa-child",
                        "details": {"sub_run_id": "aaa-child"}}}}
                ]},
                "aaa-child": {"run_id": "aaa-child", "total": 2, "items": [
                    {"run_id": "aaa-child", "node_id": "reason", "status": "started",
                     "effect": {"type": "llm_call", "payload": {}}},
                    // Answer-shaped INTERMEDIATE end — must never conclude
                    // the restored turn or render as its final answer.
                    {"run_id": "aaa-child", "node_id": "done", "status": "completed",
                     "result": {"completed": true,
                                 "output": {"answer": "intermediate delegate result"}}}
                ]},
                // Unreachable by discovery (trimmed capture): still folds
                // (leftover lane), so recorded work is never hidden.
                "zzz-orphan": {"run_id": "zzz-orphan", "total": 1, "items": [
                    {"run_id": "zzz-orphan", "node_id": "reason", "status": "completed",
                     "effect": {"type": "llm_call", "payload": {}},
                     "result": {"content": "…", "usage": {"input_tokens": 7, "output_tokens": 3}}}
                ]}
            }
        });
        let mut fold = crate::transcript::Fold::new();
        let mut fx = Vec::new();
        // The live root was cancelled by hand (the stuck-tree cleanup
        // shape): rehydrate flags the turn failed.
        assert!(rehydrate_run_into(
            &mut fold, "root1", &bundle, true, &mut fx
        ));
        assert!(
            !fold.finished,
            "a depth-2 cycler's intermediate end must not conclude the restored turn"
        );
        assert_eq!(
            fold.answer_run_id(),
            None,
            "the grandchild folds AFTER its discovering parent and never binds"
        );
        assert!(
            !fold.items.iter().any(|i| matches!(
                i,
                Item::Assistant {
                    final_answer: true,
                    ..
                }
            )),
            "no false final answer: {:#?}",
            fold.items
        );
        assert!(
            fold.items.iter().any(
                |i| matches!(i, Item::Error { text } if text.contains("ended without an answer"))
            ),
            "the honest no-answer card renders for the failed turn"
        );
        // Leftover ledgers still contribute: the ONLY completed llm_call
        // in the bundle lives in the undiscovered orphan (the child's
        // reason record is `started` — no usage receipt).
        assert_eq!(fold.stats.llm_calls, 1, "orphan ledger folded last");
    }

    // -----------------------------------------------------------------
    // Thin-client conformance (lane 2, 2026-07-23): the composer release
    // is a CLIENT overlay on server truth — when the turn concludes from
    // a SUBRUN's records the wrapper root is still open on the gateway
    // (44 live waiting basic-agent roots at the audit), and the client
    // must say so instead of silently going Idle over a run other apps
    // still see as waiting. docs/design/thin-client-conformance.md.
    // -----------------------------------------------------------------

    fn wrapper_wait_record() -> serde_json::Value {
        json!({"run_id": "root1", "node_id": "n1", "status": "waiting",
               "effect": {"type": "start_subworkflow", "payload": {}},
               "result": {"wait": {"reason": "subworkflow",
                                    "details": {"sub_run_id": "agent1"}}}})
    }

    fn agent_answer_records() -> Vec<serde_json::Value> {
        vec![
            json!({"run_id": "agent1", "node_id": "reason", "status": "started",
                   "effect": {"type": "llm_call", "payload": {}}}),
            json!({"run_id": "agent1", "node_id": "end", "status": "completed",
                   "effect": {"type": "flow", "payload": {}},
                   "result": {"output": {"answer": "done!"}}}),
        ]
    }

    #[test]
    fn subrun_conclusion_renders_the_root_still_open_overlay() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
            store.session_id.set("s1".into());
            apply_start_binding(&store, &tx, "root1", "s1");

            // Root stream: the subworkflow wait discovers the agent subrun.
            apply_stream_records(&store, &tx, "root1", "root1", &[wrapper_wait_record()]);
            match rx.try_recv() {
                Ok(Cmd::Follow { run_id, .. }) => assert_eq!(run_id, "agent1"),
                other => panic!("expected a follow for the agent subrun, got {other:?}"),
            }
            assert_eq!(store.phase.get_untracked(), Phase::Running);

            // Agent stream: the answer concludes the turn — the overlay
            // must render AFTER the answer card, the outcome mailbox must
            // be Success, and helper streams must be stopped.
            apply_stream_records(&store, &tx, "root1", "agent1", &agent_answer_records());
            assert_eq!(store.phase.get_untracked(), Phase::Idle);
            assert_eq!(
                store.last_outcome.get_untracked(),
                crate::store::RunOutcome::Success
            );
            assert!(matches!(rx.try_recv(), Ok(Cmd::StopFollows)));
            store.fold.with_untracked(|f| {
                let answer_at = f
                    .items
                    .iter()
                    .position(|i| {
                        matches!(
                            i,
                            Item::Assistant {
                                final_answer: true,
                                ..
                            }
                        )
                    })
                    .expect("the answer card folded");
                let note_at = f
                    .items
                    .iter()
                    .position(
                        |i| matches!(i, Item::Info { text } if text == SUBRUN_CONCLUSION_NOTE),
                    )
                    .expect("the root-still-open overlay renders");
                assert!(
                    note_at > answer_at,
                    "the overlay follows the answer it annotates"
                );
            });
        });
        root.dispose();
    }

    #[test]
    fn root_stream_conclusion_never_claims_a_wrapper_note() {
        // A flow whose ROOT delivers the answer (coder-tree shape) really
        // ended — the overlay would be a false divergence claim there.
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            let (tx, _rx) = std::sync::mpsc::channel::<Cmd>();
            store.session_id.set("s1".into());
            apply_start_binding(&store, &tx, "root1", "s1");
            apply_stream_records(
                &store,
                &tx,
                "root1",
                "root1",
                &[
                    json!({"run_id": "root1", "node_id": "end", "status": "completed",
                          "effect": {"type": "flow", "payload": {}},
                          "result": {"output": {"answer": "root answer"}}}),
                ],
            );
            assert_eq!(store.phase.get_untracked(), Phase::Idle);
            store.fold.with_untracked(|f| {
                assert!(f.finished);
                assert!(
                    !f.items.iter().any(
                        |i| matches!(i, Item::Info { text } if text == SUBRUN_CONCLUSION_NOTE)
                    ),
                    "no overlay for a genuinely-ended root: {:#?}",
                    f.items
                );
            });
        });
        root.dispose();
    }

    #[test]
    fn stale_stream_batches_touch_no_signals() {
        // The stale-stream guard extends past the fold: a late batch from
        // an abandoned run must not sync totals, flip the phase, or push
        // the overlay into the NEW run's transcript.
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = crate::store::Store::create(cx);
            let (tx, _rx) = std::sync::mpsc::channel::<Cmd>();
            store.session_id.set("s1".into());
            apply_start_binding(&store, &tx, "root2", "s1");
            store.totals.set(SessionTotals {
                input_tokens: 7,
                output_tokens: 3,
                total_tokens: 10,
                runs: 1,
            });
            let before = store.fold.with_untracked(|f| f.items.len());
            // A batch for the PREVIOUS root: guard drops it wholesale.
            apply_stream_records(&store, &tx, "root1", "agent1", &agent_answer_records());
            assert_eq!(store.fold.with_untracked(|f| f.items.len()), before);
            assert_eq!(store.phase.get_untracked(), Phase::Running);
            assert_eq!(store.totals.get_untracked().input_tokens, 7);
        });
        root.dispose();
    }
}
