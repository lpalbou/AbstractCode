//! App-scale reactive state: a store struct of signals provided as context.
//!
//! All signals are written on the UI thread only — worker threads post
//! closures through `WakeHandle` (the engine rule).

use std::sync::Arc;
use std::time::Instant;

use abstracttui::prelude::*;
use abstracttui::widgets::Bitmap;

use crate::transcript::Fold;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Starting,
    Running,
}

/// Gateway connection state. `Down` carries the evidence-worded message
/// (from `GwError`'s kind-aware `Display`) plus `gone: bool` — `true` only
/// on connect-level proof (refused/DNS/host-down, `GwError::is_gone()`),
/// `false` when the down-mark came from the soft-failure threshold
/// (repeated timeouts against a gateway that may just be busy). Display
/// sites use the flag to pick honest words ("unreachable" vs "not
/// responding") instead of re-deriving from message substrings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conn {
    Unknown,
    Ok,
    Down(String, bool),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Workflow {
    pub bundle_id: String,
    pub flow_id: String,
    pub name: String,
    pub description: String,
}

impl Workflow {
    /// Whether this workflow exposes the human-approval gating control
    /// (the multi-agent coder's `gating_mode` pin). Heuristic on the
    /// bundle id today — the coder is the one gating-capable workflow;
    /// this is the SINGLE place to swap for a gateway-served capability
    /// marker when flow ships one (the design's `abstractcode.gated.v1`
    /// interface), so the modal trigger moves in one edit.
    pub fn supports_gating(&self) -> bool {
        self.bundle_id.contains("multiagent-coding") || self.flow_id.contains("multiagent-cod")
    }

    pub fn label(&self) -> String {
        if self.name.is_empty() {
            format!("{}:{}", self.bundle_id, self.flow_id)
        } else {
            self.name.clone()
        }
    }
}

/// What the capability probe learned about one (provider, model) pair.
/// `supported: None` = probe failed or capability genuinely unknown —
/// the picker OFFERS with a caveat (three-state coupling, contract v1);
/// it never fabricates a lock from absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningProbe {
    pub provider: String,
    pub model: String,
    pub supported: Option<bool>,
    pub levels: Vec<String>,
    /// Match provenance when served ("exact"/"alias"/"default"/"" —
    /// core's capability_source ask; empty until the registry serves it).
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderInfo {
    pub name: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    /// Gateway grouping ("files", "web", "system", MCP server name, …).
    pub toolset: String,
    /// Server-served capability tier ("tier2_world", …) — informational
    /// (`None` until the gateway bounce that adds the field). ALL
    /// core-registry tools are tier2_world by the ruled boundary; the
    /// finer approval dial below is the real discriminator.
    pub tier: Option<String>,
    /// Server-served per-tool approval default ("auto" | "ask"). `None` =
    /// not served (the client name table classifies instead).
    pub approval: Option<String>,
    /// Served `risk_rank` (core's band: observe=1 act=2 outreach=3
    /// destroy=4). Floors the tier mapping — see
    /// `tool_policy::server_tier` (the c5028 transitional-belt rule).
    pub risk_rank: Option<u8>,
    /// Served `enabled: false` (tool-tiers item H, the full-catalog
    /// surfacing fix — this seat's c4555 consumer commitment): the row
    /// EXISTS on the gateway but a gate disables it. VISIBLE, never
    /// grantable: the /tools modal renders it with its gate and refuses
    /// toggles/pins; run allowlists and the auto-approve expansion
    /// exclude it (disabled rows always ask — the F3 clamp, client
    /// side). `false` (the derive default) = enabled: the gateway only
    /// stamps the field on disabled rows.
    pub served_disabled: bool,
    /// The named gate that would enable a served-disabled row (env var /
    /// config knob — served as `enable_gate`).
    pub enable_gate: String,
    /// The gateway's one-line reason for the disablement.
    pub why_disabled: String,
}

/// The two durable verbs the quit modal can send (leave sends nothing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitVerb {
    Pause,
    Cancel,
}

/// Quit-with-live-run state machine (design: untracked/reviews/
/// quit-modal-design.md). `None` = no quit in flight; `Choosing` = the
/// modal is up; `Delivering` = a verb was sent and the app quits only
/// on the gateway's ACCEPTANCE (the durable command store's 2xx —
/// never "the run finished pausing"); `Failed` = honest failure state
/// offering quit-anyway/stay.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum QuitState {
    #[default]
    None,
    Choosing,
    Delivering {
        verb: QuitVerb,
        run_id: String,
        gen: u64,
    },
    /// The gateway ACCEPTED the verb; the app is exiting — exists so
    /// the post-teardown echo can say "paused durably"/"cancel
    /// accepted" instead of misreading a resolved delivery as
    /// unconfirmed.
    Acked {
        verb: QuitVerb,
        run_id: String,
    },
    Failed {
        verb: QuitVerb,
        run_id: String,
        /// True when the failure is DEFINITIVE (the gateway answered
        /// with an error / the worker is dead) — the command will NOT
        /// land. False = timeout: the request may still be in flight
        /// in this app and can land if the user stays. The Failed copy
        /// splits on it (audit P2: "may still land" was false for the
        /// definitive arm).
        definitive: bool,
        error: String,
    },
}

/// Structured pause/cancel outcome (the quit sequencer's ack channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbAck {
    pub verb: QuitVerb,
    pub run_id: String,
    pub ok: bool,
    /// On failure: TRUE when the command definitively did NOT land
    /// (every attempt was refused/answered — server spoke or connect
    /// never made); FALSE when any attempt was AMBIGUOUS (timeout /
    /// transport after the request may have left — the command may
    /// have landed with only the response lost). Derived from error
    /// CLASSES by the send authority, never from message text (D2:
    /// a blanket `definitive: true` overclaimed for transport-
    /// exhausted retries).
    pub definitive: bool,
    pub error: String,
}

/// One file staged for the NEXT plain-prompt send (attachments design
/// §4.1). Validated at ATTACH (exists, regular, ≤ cap when known);
/// uploaded at SEND on the worker thread — send-time upload is the only
/// shape that survives `/new` session rotation and makes removing a chip
/// a true no-op (session uploads are permanent server-side).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PendingAttachment {
    /// Absolute, canonicalized at attach.
    pub path: String,
    /// File name for chips + notices.
    pub name: String,
    /// Attach-time stat (display + pre-check; the upload re-reads).
    pub size: u64,
    /// Send-time upload result cached for retry-after-start-failure:
    /// (session_id at upload, the WHOLE ref object). A cached ref is
    /// reused only while the session matches — never re-uploaded, never
    /// carried across sessions.
    pub uploaded: Option<(String, serde_json::Value)>,
}

/// The ONE `ToolInfo → ToolClass` projection (policy-relevant fields
/// only; `why_disabled` is render-side and deliberately not carried).
/// It was hand-copied in `Store::tool_classes` and exec's discovery
/// mapping — when `risk_rank` landed, both sites had to be found and
/// touched (the agreement-by-coincidence class this codebase names).
impl From<&ToolInfo> for crate::tool_policy::ToolClass {
    fn from(t: &ToolInfo) -> Self {
        crate::tool_policy::ToolClass {
            name: t.name.clone(),
            approval: t.approval.clone(),
            tier: t.tier.clone(),
            served_disabled: t.served_disabled,
            enable_gate: t.enable_gate.clone(),
            risk_rank: t.risk_rank,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub trust: String,
    pub blocked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpServer {
    pub name: String,
    pub url: String,
    pub description: String,
    pub auth_required: bool,
}

/// Prompt-cache posture for the effective provider/model route.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CacheInfo {
    pub provider: String,
    pub model: String,
    pub supported: bool,
    /// "keyed" | "local" | … (the gateway's capability answer).
    pub mode: String,
}

/// One GPU utilization sample from the gateway host (`/host/metrics/gpu`).
#[derive(Debug, Clone, PartialEq)]
pub struct GpuSample {
    /// Top-level `utilization_gpu_pct` (0–100).
    pub util_pct: f64,
    /// First GPU's name ("Apple M5 Max"); empty when the host names none.
    pub name: String,
}

/// `/gpu` meter state (OBS-6). The DATA half lives here — the poller
/// thread (`gateway::gpu`) posts transitions; the status-bar render
/// matches on this enum. Honesty contract: `Unsupported` means the host
/// SAID so (`supported:false`, or the endpoint is absent) and polling
/// has STOPPED — the meter must never fabricate a number; `Error` keeps
/// the last failure visible while polling continues (transient).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum GpuMeter {
    /// Toggled off (`/gpu`) — zero polling, renders nothing.
    #[default]
    Off,
    /// Enabled; the first sample is in flight.
    Pending,
    Ready(GpuSample),
    /// The gateway host cannot serve GPU metrics (reason). Poller stopped.
    Unsupported(String),
    /// The last poll failed (message); the poller keeps trying.
    Error(String),
}

/// Upper bound on retained image entries (F3): with decode-time
/// downscaling (`runner::downscale_for_transcript`) each entry is
/// ≤ ~0.7 MB, so the worst case stays ~22 MB instead of unbounded
/// full-resolution bitmaps across a whole session.
pub const IMAGE_ENTRY_CAP: usize = 32;

#[derive(Clone)]
pub struct ImageEntry {
    pub artifact_id: String,
    pub bitmap: Option<Arc<Bitmap>>,
    pub error: String,
}

/// One queued prompt (`/queue <text>`): FIFO. PERSISTED per session id
/// (prefs `session_queues` slot, write-through on every mutation) — the
/// queue contract is "piling up requests that each gets executed", and a
/// silent drop at quit broke it. Safety is the RESTORE POSTURE, not
/// non-persistence: any restore (boot, session switch) lands PAUSED and
/// never auto-starts, so persistence costs zero unattended token spend.
///
/// Thin-client conformance (class ii, 2026-07-23): queued prompts are
/// CLIENT-HELD future work — un-submitted composer text, plural. The
/// gateway has NO completion-chained queue primitive today
/// (`POST /runs/schedule` is time-based only), so nothing server-side
/// could hold them; other apps see the work only once each item starts
/// as a normal traceable gateway run. The `/queue` help line names this
/// locality; the server-side primitive ask is on the record in
/// docs/roadmap/conformance-ledger-asks.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPrompt {
    /// Stable identity for modal edits (remove/reorder survive drains).
    pub id: u64,
    pub text: String,
}

/// Text buffered while the run has NO cycling target yet (Starting, or
/// Running before the first reason-cycle record). Delivery keys on the
/// fold's `cycling_target()` landing PLUS a run-identity predicate — a
/// root-targeted steer is silently never folded on wrapper bundles (the
/// agent loop drains guidance in a SUBRUN), and a stale previous-run
/// cycle must never satisfy delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSteer {
    /// `fold.root_run_id()` at buffer time.
    pub armed_at_root: String,
    /// Armed during Starting: deliver only once a NEW root began
    /// (`root != armed_at_root`). Armed while Running: deliver only
    /// while the SAME root lives (`root == armed_at_root`).
    pub armed_while_starting: bool,
    pub text: String,
}

/// The active `/goal` run for this session (client half of the goal-agent
/// contract; the bundle is flow-seat-owned and may not be published yet).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GoalState {
    pub text: String,
    /// Empty while the start is in flight; bound by `wire_goal` when the
    /// run reaches Running (starts are phase-serialized, so the next
    /// Running run IS the goal run).
    pub run_id: String,
}

/// How the last run ended — written by the runner at terminal points,
/// CONSUMED (reset to None) by the queue-drain effect. Take-semantics
/// makes the drain edge-triggered: a resume must not re-pause against a
/// stale Failed, and a replayed Success must not double-drain.
///
/// Semantics note (thin-client conformance): `Success` means "the TURN
/// concluded with a usable conclusion" — on wrapper bundles the ROOT run
/// may still be open on the gateway at that moment (it finalizes
/// server-side; the transcript overlay says so). This mailbox is client
/// scheduling state, never a claim about the root's server status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunOutcome {
    #[default]
    None,
    Success,
    Failed,
    Cancelled,
}

/// Session-scope token totals (across runs; per-run stats live in the fold).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Cumulative total tokens: the only honest number for providers that
    /// report no input/output split (the coder-run shape, bug (e)).
    pub total_tokens: u64,
    pub runs: u64,
}

#[derive(Clone, Copy)]
pub struct Store {
    pub fold: Signal<Fold>,
    pub phase: Signal<Phase>,
    pub conn: Signal<Conn>,
    pub session_id: Signal<String>,
    pub run_id: Signal<String>,
    pub workflow: Signal<Workflow>,
    pub workflows: Signal<Vec<Workflow>>,
    pub provider: Signal<String>,
    pub model: Signal<String>,
    /// Gating mode for the current session ("" = the workflow default,
    /// gated; "auto" = unattended, skip the workflow's human pauses). Set
    /// by the workflow-select modal or /gating; sent as input_data.gating_mode.
    pub gating_mode: Signal<String>,
    /// Verifier-before-conclude for this session (`_runtime.review_mode`):
    /// before a tool-call-free response is accepted as final, a strict
    /// verifier re-reads the transcript and can force more tool calls.
    /// Seeded from `--review`/`--no-review` (default ON — abstractcode's
    /// long-standing default) and toggled by `/review`.
    pub review_mode: Signal<bool>,
    /// Verifier round budget (`_runtime.review_max_rounds`); `/review rounds N`.
    pub review_rounds: Signal<u32>,
    /// Reasoning effort override ("" = gateway default). The third leg
    /// of the route triple; provider/model changes reset it (coupling
    /// rule — an effort may only apply under the model it was chosen
    /// for).
    pub reasoning: Signal<String>,
    /// Per-model reasoning capability probe result for the picker's
    /// third stage: (provider, model, probe). None while in flight.
    pub reasoning_probe: Signal<Option<ReasoningProbe>>,
    pub providers: Signal<Vec<ProviderInfo>>,
    pub tools: Signal<Vec<ToolInfo>>,
    pub tools_error: Signal<String>,
    /// Tools the user switched OFF (persisted per session; `/tools`).
    pub disabled_tools: Signal<Vec<String>>,
    /// EPHEMERAL: this session has no persisted tool-prefs slot yet, so
    /// camera tools (privacy-sensitive) should be seeded OFF once the tool
    /// inventory loads (operator ask: camera off by default). Cleared the
    /// moment the seed runs OR when a session with a saved slot loads —
    /// never re-seeds a session the user has already shaped.
    pub camera_seed_pending: Signal<bool>,
    /// Gateway skill shelf (`/skills`).
    pub skills_catalog: Signal<Vec<SkillInfo>>,
    pub skills_error: Signal<String>,
    /// Skill names attached to every run (persisted; `input_data.skills`).
    pub selected_skills: Signal<Vec<String>>,
    /// Gateway MCP server registry (`/mcp`), plus its honest empty-state note.
    pub mcp_servers: Signal<Vec<McpServer>>,
    pub mcp_note: Signal<String>,
    /// Prompt-cache capability for the effective route (None until probed).
    pub cache: Signal<Option<CacheInfo>>,
    /// `/gpu` meter state (OBS-6): written only by the UI thread (the
    /// poller posts closures through the wake handle, generation-guarded
    /// in `gateway::gpu` so a disabled poller's late sample never lands).
    pub gpu: Signal<GpuMeter>,
    /// OPERATOR-DECLARED context window in tokens (CTX-0; 0 = not
    /// declared). Seeded from `--max-tokens`/prefs; `/context` edits +
    /// persists. Drives the `ctx N/M (P%)` meter — always labeled
    /// "declared", never a client capability table.
    pub context_window: Signal<u64>,
    /// tok/s of the newest COMPLETED llm_call (OBS-1a-live), measured
    /// client-side by `wire_llm_meter`: the cumulative-OUTPUT delta
    /// across the call's started→completed transition over the
    /// client-observed wall window. The numerator is receipt-true in
    /// both usage shapes — splitless receipts add nothing to
    /// `stats.output_tokens`, so they never mint a rate (honest
    /// absence; the cycle-2 P1-A overstatement — total tokens over wall
    /// time — is structurally unreachable). Network + ledger latency
    /// ride the denominator, so a measured rate slightly UNDERSTATES
    /// provider throughput (conservative, labeled "(last call)"). None
    /// until a split-usage call completes; cleared at session
    /// boundaries.
    pub last_call_rate: Signal<Option<f64>>,
    /// The gateway's configured default text route (provider, model) — what
    /// "gateway defaults" actually resolves to (capability input.text route).
    pub default_route: Signal<(String, String)>,
    pub images: Signal<Vec<ImageEntry>>,
    pub totals: Signal<SessionTotals>,
    pub run_started: Signal<Option<Instant>>,
    pub elapsed_secs: Signal<u64>,
    /// Pending toast texts; a UI effect drains them into Toast overlays.
    pub notices: Signal<Vec<String>>,
    /// Bumped by Esc; two within a second cancels the run.
    pub last_esc: Signal<Option<Instant>>,
    /// Transcript VERBOSITY (operator directive 2026-08-19: /details
    /// toggles the full tool call vs. just the call + a status tag).
    /// true = full cards — wrapped args, result bodies, thinking
    /// content + the labeled reasoning channel. false (default) = the
    /// collapsed view — cycle rules + thinking gists + one-line tool
    /// calls with right-aligned status words. The thinking itself and
    /// every called tool stay visible in BOTH states; this signal
    /// gates detail, never existence. Toggled by Ctrl+D / /details;
    /// /details full|fold set it directly.
    pub show_details: Signal<bool>,
    /// The active run tree is PAUSED on the gateway (durable /pause).
    pub paused: Signal<bool>,
    /// Files staged for the NEXT plain-prompt send (chips above the
    /// composer). Uploaded at SEND; kept on failure; cleared on started;
    /// discarded (with a notice) at session boundaries.
    pub pending_attachments: Signal<Vec<PendingAttachment>>,
    /// Gateway attachment size cap (`/workspace/policy` →
    /// `max_attachment_bytes`); 0 = unknown → no client pre-refusal
    /// (the server 413 stays the authority).
    pub max_attachment_bytes: Signal<u64>,
    /// Quit-with-live-run flow (quit-modal design, 2026-07-25): the
    /// modal's state machine. `Delivering.gen` guards the timeout job
    /// (a stale timer must never fail a NEWER delivery).
    pub quit_state: Signal<QuitState>,
    /// Structured pause/cancel outcome posted by the ONE send authority
    /// (`runner::send_verb_blocking` — reached from the worker's slash-
    /// command handlers AND the quit modal's dedicated send thread; both
    /// post via wake, so the write lands on the UI thread). The quit
    /// sequencer matches verb + run_id; outside a quit nothing reads
    /// it. Toast text is never matched — the error-substring class
    /// stays banned.
    pub verb_ack: Signal<Option<VerbAck>>,
    /// `/status` server-truth probe result: `(run_id, status-line)` from
    /// a live `get_run` at modal open — client phase vs gateway status
    /// divergence made inspectable (visibility review P2-5). `None` =
    /// no probe yet / probing.
    pub run_status_probe: Signal<Option<(String, String)>>,
    /// /history cursor: `created_at` of the OLDEST restored turn — the
    /// next bloc streams turns strictly before it. None = nothing
    /// restored yet / no older history.
    /// A history bloc is streaming (auto-load or /history) — the stub
    /// line renders progress, the strip names it, and the scroll-top
    /// auto-loader refuses to double-dispatch while true. Reset on
    /// session switches and on every runner completion path.
    pub history_loading: Signal<bool>,
    pub history_cursor: Signal<Option<String>>,
    /// Older turns known to exist on the gateway beyond the restored
    /// bloc (the boot lists wide, fetches the last bloc only — the
    /// ruling's shape). Drives the stub + /history availability.
    pub older_turns: Signal<usize>,
    /// Session-history rehydration in flight (boot / session switch):
    /// the idle strip says "restoring session history…" instead of the
    /// "no runs yet" lie while up to ~21 bundles fetch (visibility
    /// review P2-7).
    pub restoring: Signal<bool>,
    /// Drop-undo slot: (raw paste text, paths attached from it). Armed
    /// when a dropped-path paste is consumed into chips; Ctrl+O undoes —
    /// removes those chips and puts the RAW text into the composer
    /// (the pasted-path-as-prose escape hatch). One level, newest wins.
    pub paste_undo: Signal<Option<(String, Vec<String>)>>,
    /// The open attachment preview (`/attach preview`, `p` in the
    /// manager). Minted on the UI thread as `Loading`, filled by the
    /// worker's loader thread; `None` = no preview open. The modal
    /// renders this signal, so the body can arrive after the frame that
    /// opened it. See [`crate::preview`].
    pub preview: Signal<Option<crate::preview::PreviewState>>,
    /// Monotonic mint for `PreviewState::seq` — the staleness guard
    /// that keeps a slow loader from overwriting a newer preview.
    pub preview_seq: Signal<u64>,
    /// PERSISTED permissions level ("read"|"write"|"all"; "" reads as
    /// "read"): batches at-or-below it auto-approve (`/permissions`;
    /// the c5028 consolidation — the old session-scoped /auto blanket
    /// signal is DELETED, its three latent holes with it). Mirrors
    /// `prefs.tool_approval.accepted_tier` (at-rest key unchanged:
    /// documented hand-editable for headless).
    pub accepted_tier: Signal<String>,
    /// Per-tool approval pins (name → "auto"|"ask"), persisted.
    pub tool_overrides: Signal<Vec<(String, String)>>,
    /// Live workspace access mode for new runs ("" = server-managed:
    /// send nothing). Seeded from --workspace-mode/prefs; edited by
    /// `/workspace`; persisted.
    pub workspace_mode: Signal<String>,
    /// Extra allowlisted roots sent as `workspace_allowed_paths`
    /// (`/workspace`; persisted; applies in workspace_or_allowed mode).
    pub workspace_allowed: Signal<Vec<String>>,
    // -- queue / steer lane ---------------------------------------------
    /// FIFO prompt queue (`/queue <text>`); drains as NEW runs when a run
    /// completes successfully. Persisted per session (prefs
    /// `session_queues`, write-through); every restore lands PAUSED.
    pub queue: Signal<Vec<QueuedPrompt>>,
    /// Halted after a failure/cancel/restore; explicit resume (`r`).
    pub queue_paused: Signal<bool>,
    /// Text submitted with no cycling target yet (Starting, or Running
    /// pre-first-cycle), buffered until the fold's cycling target lands
    /// (delivered as a steer into the CYCLING run) or the run dies
    /// (error/info-carded). The old behavior DROPPED the text.
    pub pending_steer: Signal<Option<PendingSteer>>,
    /// Terminal outcome mailbox for the drain effect (take-semantics).
    pub last_outcome: Signal<RunOutcome>,
    /// Monotonic id mint for `QueuedPrompt::id`.
    pub queue_next_id: Signal<u64>,
    /// One-shot composer seed (queue modal `e` pops an item into the
    /// composer; root() owns the TextAreaState and drains this).
    pub composer_seed: Signal<Option<String>>,
    /// One-shot RESTORE of an undelivered steer (2026-08-20). Distinct
    /// from `composer_seed` on purpose: the seed REPLACES the draft
    /// because the operator asked for it, while a restore must never
    /// clobber words typed since the failure — root() drops it when the
    /// composer is non-empty, and the error card keeps the text either
    /// way, so nothing is ever lost.
    pub steer_restore: Signal<Option<String>>,
    // -- /goal lane -------------------------------------------------------
    /// The active goal (text + bound run id), persisted per session.
    pub goal: Signal<Option<GoalState>>,
    /// Catalog entrypoints carrying the GOAL interface
    /// (`abstractcode.goal.v1`) — disjoint from `workflows` (agent.v1).
    pub goal_workflows: Signal<Vec<Workflow>>,
    // -- entity collaboration lane -------------------------------------
    /// Which conversation the transcript pane mirrors (agent or entity).
    pub focus: Signal<crate::convo::Focus>,
    /// Every entity conversation of this session (open, parked, closed —
    /// closed transcripts stay readable; never removed in-session).
    pub convos: Signal<Vec<crate::convo::EntityConvo>>,
    /// Cached entity roster (last-good; `/entities` + '@' completion read
    /// this and NEVER trigger a synchronous fetch).
    pub entities: Signal<Vec<crate::entities::EntityInfo>>,
    /// "HH:MM" (UTC) label of the roster snapshot; empty = never fetched.
    pub entities_as_of: Signal<String>,
    pub entities_loading: Signal<bool>,
    pub entities_error: Signal<String>,
    /// Identity cards by slug (async-filled; browsing hits the cache).
    pub entity_cards: Signal<Vec<(String, crate::entities::EntityCard)>>,
    /// MCP registry honesty (source path + probed flag) for `/mcp`.
    pub mcp_info: Signal<crate::entities::McpRegistryInfo>,
}

impl Store {
    pub fn create(cx: Scope) -> Store {
        Store {
            fold: cx.signal(Fold::new()),
            phase: cx.signal(Phase::Idle),
            conn: cx.signal(Conn::Unknown),
            session_id: cx.signal(String::new()),
            run_id: cx.signal(String::new()),
            workflow: cx.signal(Workflow::default()),
            workflows: cx.signal(Vec::new()),
            provider: cx.signal(String::new()),
            model: cx.signal(String::new()),
            gating_mode: cx.signal(String::new()),
            review_mode: cx.signal(crate::cli::DEFAULT_REVIEW_MODE),
            review_rounds: cx.signal(crate::cli::DEFAULT_REVIEW_ROUNDS),
            reasoning: cx.signal(String::new()),
            reasoning_probe: cx.signal(None),
            providers: cx.signal(Vec::new()),
            tools: cx.signal(Vec::new()),
            tools_error: cx.signal(String::new()),
            disabled_tools: cx.signal(Vec::new()),
            camera_seed_pending: cx.signal(false),
            skills_catalog: cx.signal(Vec::new()),
            skills_error: cx.signal(String::new()),
            selected_skills: cx.signal(Vec::new()),
            mcp_servers: cx.signal(Vec::new()),
            mcp_note: cx.signal(String::new()),
            cache: cx.signal(None),
            gpu: cx.signal(GpuMeter::Off),
            context_window: cx.signal(0),
            last_call_rate: cx.signal(None),
            default_route: cx.signal((String::new(), String::new())),
            images: cx.signal(Vec::new()),
            totals: cx.signal(SessionTotals::default()),
            run_started: cx.signal(None),
            elapsed_secs: cx.signal(0),
            notices: cx.signal(Vec::new()),
            last_esc: cx.signal(None),
            // Collapsed by default: the readable scan view (thinking
            // gists + tagged one-line tool calls); /details expands.
            show_details: cx.signal(false),
            paused: cx.signal(false),
            quit_state: cx.signal(QuitState::None),
            verb_ack: cx.signal(None),
            run_status_probe: cx.signal(None),
            history_loading: cx.signal(false),
            history_cursor: cx.signal(None),
            older_turns: cx.signal(0),
            restoring: cx.signal(false),
            pending_attachments: cx.signal(Vec::new()),
            max_attachment_bytes: cx.signal(0),
            paste_undo: cx.signal(None),
            preview: cx.signal(None),
            preview_seq: cx.signal(0),
            accepted_tier: cx.signal(String::new()),
            tool_overrides: cx.signal(Vec::new()),
            workspace_mode: cx.signal(String::new()),
            workspace_allowed: cx.signal(Vec::new()),
            queue: cx.signal(Vec::new()),
            queue_paused: cx.signal(false),
            pending_steer: cx.signal(None),
            last_outcome: cx.signal(RunOutcome::None),
            queue_next_id: cx.signal(1),
            composer_seed: cx.signal(None),
            steer_restore: cx.signal(None),
            goal: cx.signal(None),
            goal_workflows: cx.signal(Vec::new()),
            focus: cx.signal(crate::convo::Focus::Agent),
            convos: cx.signal(Vec::new()),
            entities: cx.signal(Vec::new()),
            entities_as_of: cx.signal(String::new()),
            entities_loading: cx.signal(false),
            entities_error: cx.signal(String::new()),
            entity_cards: cx.signal(Vec::new()),
            mcp_info: cx.signal(Default::default()),
        }
    }

    pub fn notify(&self, text: impl Into<String>) {
        let text = text.into();
        self.notices.update(|n| n.push(text));
    }

    /// Mint a stable id for a queued prompt.
    pub fn mint_queue_id(&self) -> u64 {
        let id = self.queue_next_id.get_untracked();
        self.queue_next_id.set(id.wrapping_add(1));
        id
    }

    /// Reset the STEER half of the lane at a session boundary (/new,
    /// session switch). Returns the dropped buffer so callers can echo it
    /// visibly. The QUEUE is deliberately NOT touched here: it is stashed
    /// per session (prefs write-through) and swapped by the session
    /// boundary itself — a pending steer is moment-bound guidance for a
    /// run that no longer matters, a queue is durable work.
    pub fn reset_steer_lane(&self) -> Option<PendingSteer> {
        let dropped = self.pending_steer.get_untracked();
        self.pending_steer.set(None);
        self.last_outcome.set(RunOutcome::None);
        dropped
    }

    /// The live inventory as tool-policy classification facts (name +
    /// served tier/approval). Empty server fields fall back to the name
    /// table inside `tool_policy`. Read untracked — callers use this at
    /// discrete moments (run start, approval decision), not reactively.
    /// The ONE effective-user-disabled predicate (cycle-3 adversary
    /// P2-2: the run-start "customized?" decision and the /tools title
    /// carried it as two textual copies — agreement by coincidence is
    /// the divergence class this wave just fixed once already): a
    /// user-disabled NAME counts only when it matches an ENABLED
    /// inventory row. Served-disabled matches are a server fact, not a
    /// customization — the row cannot run either way.
    pub fn effective_user_disabled(inventory: &[ToolInfo], disabled: &[String]) -> usize {
        disabled
            .iter()
            .filter(|d| {
                inventory
                    .iter()
                    .any(|t| t.name == **d && !t.served_disabled)
            })
            .count()
    }

    /// Every tool this session could actually be granted: the inventory
    /// minus server-disabled rows minus the user's `/tools` opt-outs.
    ///
    /// This is the materialized form of "the default tool set" — needed for
    /// bundles that require an EXPLICIT `tools` list and treat a missing one
    /// as an empty one (see the goal lane), where sending nothing means
    /// sending nothing rather than "your defaults, please".
    pub fn grantable_tool_names(&self) -> Vec<String> {
        let disabled = self.disabled_tools.get_untracked();
        self.tools.with_untracked(|inv| {
            inv.iter()
                .filter(|t| !t.served_disabled)
                .map(|t| t.name.clone())
                .filter(|n| !disabled.contains(n))
                .collect()
        })
    }

    pub fn tool_classes(&self) -> Vec<crate::tool_policy::ToolClass> {
        self.tools.with_untracked(|inv| {
            inv.iter()
                .map(crate::tool_policy::ToolClass::from)
                .collect()
        })
    }

    pub fn image_for(&self, artifact_id: &str) -> Option<ImageEntry> {
        self.images
            .with(|imgs| imgs.iter().find(|e| e.artifact_id == artifact_id).cloned())
    }

    /// Insert or replace the image entry for its artifact id — UPSERT,
    /// never append: session revisits re-request the same artifacts (the
    /// fold's dedup resets with the fold), and append-only entries both
    /// leaked bitmaps and let a transient error entry permanently shadow
    /// a later successful fetch, because `image_for` returns the first
    /// match (adversary finding 7, 2026-07-22).
    ///
    /// Success is STICKY: artifacts are immutable, so an already-decoded
    /// bitmap stays valid forever — a transient re-fetch error must not
    /// clobber it (last-wins would degrade a rendered image to an error
    /// card on a gateway hiccup). Successful decodes always replace.
    ///
    /// Bounded (F3): at most [`IMAGE_ENTRY_CAP`] entries, oldest-inserted
    /// evicted first. An evicted artifact still in the scrollback renders
    /// its placeholder again (a session revisit re-fetches it); the cap
    /// exists because decoded bitmaps are the client's dominant retained
    /// allocation and the list previously grew forever.
    pub fn upsert_image(&self, entry: ImageEntry) {
        self.images.update(|imgs| {
            match imgs.iter_mut().find(|e| e.artifact_id == entry.artifact_id) {
                Some(slot) => {
                    if entry.bitmap.is_some() || slot.bitmap.is_none() {
                        *slot = entry;
                    }
                }
                None => imgs.push(entry),
            }
            if imgs.len() > IMAGE_ENTRY_CAP {
                let overflow = imgs.len() - IMAGE_ENTRY_CAP;
                imgs.drain(..overflow);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abstracttui::widgets::Bitmap;

    #[test]
    fn queue_lane_mints_ids_and_steer_reset_leaves_the_queue_alone() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = Store::create(cx);
            let a = store.mint_queue_id();
            let b = store.mint_queue_id();
            assert_ne!(a, b, "ids are unique");
            store.queue.update(|q| {
                q.push(QueuedPrompt {
                    id: a,
                    text: "one".into(),
                });
                q.push(QueuedPrompt {
                    id: b,
                    text: "two".into(),
                });
            });
            store.queue_paused.set(true);
            store.pending_steer.set(Some(PendingSteer {
                armed_at_root: "r1".into(),
                armed_while_starting: true,
                text: "buffered".into(),
            }));
            store.last_outcome.set(RunOutcome::Failed);

            // The steer reset drops the moment-bound buffer + mailbox but
            // NEVER the queue (queues are stashed per session, not reset).
            let dropped = store.reset_steer_lane();
            assert_eq!(dropped.map(|p| p.text).as_deref(), Some("buffered"));
            assert_eq!(
                store.queue.with_untracked(|q| q.len()),
                2,
                "the queue survives a steer-lane reset"
            );
            assert!(store.queue_paused.get_untracked(), "pause flag untouched");
            assert!(store.pending_steer.get_untracked().is_none());
            assert_eq!(store.last_outcome.get_untracked(), RunOutcome::None);
            // Ids keep minting past a reset (identity never recycles
            // within a session — modal edits key on it).
            assert!(store.mint_queue_id() > b);
        });
        root.dispose();
    }

    #[test]
    fn image_upsert_replaces_by_artifact_id_and_keeps_good_bitmaps() {
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = Store::create(cx);
            let entry = |id: &str, bitmap: Option<Arc<Bitmap>>, error: &str| ImageEntry {
                artifact_id: id.into(),
                bitmap,
                error: error.into(),
            };
            let bitmap = || {
                Some(Arc::new(Bitmap::new(
                    1,
                    1,
                    abstracttui::prelude::Rgba::BLACK,
                )))
            };

            // A transient error entry must NOT permanently shadow a later
            // successful fetch (`image_for` returns the first match).
            store.upsert_image(entry("a1", None, "image fetch failed: timeout"));
            store.upsert_image(entry("a2", None, ""));
            store.upsert_image(entry("a1", bitmap(), ""));
            assert_eq!(
                store.images.with_untracked(|v| v.len()),
                2,
                "upsert never grows"
            );
            let a1 = store.image_for("a1").expect("entry exists");
            assert!(a1.bitmap.is_some(), "success replaced the error entry");
            assert!(a1.error.is_empty());

            // Success is sticky: a transient error on a session-revisit
            // re-fetch must not clobber the already-decoded bitmap
            // (artifacts are immutable; the old pixels are still true).
            store.upsert_image(entry("a1", None, "image fetch failed: 503"));
            let a1 = store.image_for("a1").expect("entry exists");
            assert!(
                a1.bitmap.is_some(),
                "a good bitmap survives a transient re-fetch error"
            );
            assert_eq!(store.images.with_untracked(|v| v.len()), 2);

            // A fresh successful decode still replaces (same artifact,
            // same pixels — replacement is harmless and keeps one entry).
            store.upsert_image(entry("a1", bitmap(), ""));
            assert_eq!(store.images.with_untracked(|v| v.len()), 2);

            // An error for an artifact with NO good bitmap does land
            // (the honest failure state renders in the transcript).
            store.upsert_image(entry("a3", None, "decode failed"));
            let a3 = store.image_for("a3").expect("entry exists");
            assert!(a3.bitmap.is_none());
            assert_eq!(a3.error, "decode failed");
        });
        root.dispose();
    }

    #[test]
    fn image_list_is_capped_evicting_oldest_first() {
        // F3: the entry list is bounded — a long session's images must
        // not accumulate bitmaps forever. Oldest-inserted evict first;
        // the newest CAP entries survive.
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = Store::create(cx);
            for i in 0..(IMAGE_ENTRY_CAP + 8) {
                store.upsert_image(ImageEntry {
                    artifact_id: format!("art-{i}"),
                    bitmap: None,
                    error: String::new(),
                });
            }
            assert_eq!(
                store.images.with_untracked(|v| v.len()),
                IMAGE_ENTRY_CAP,
                "the list never exceeds the cap"
            );
            assert!(
                store.image_for("art-0").is_none(),
                "the oldest entry evicted"
            );
            assert!(
                store
                    .image_for(&format!("art-{}", IMAGE_ENTRY_CAP + 7))
                    .is_some(),
                "the newest entry survives"
            );
            // An upsert of an EXISTING id never evicts (no growth).
            store.upsert_image(ImageEntry {
                artifact_id: format!("art-{}", IMAGE_ENTRY_CAP + 7),
                bitmap: None,
                error: "retry".into(),
            });
            assert_eq!(store.images.with_untracked(|v| v.len()), IMAGE_ENTRY_CAP);
        });
        root.dispose();
    }

    #[test]
    fn gpu_meter_defaults_off() {
        // OBS-6: the meter starts OFF (zero polling until /gpu).
        let (root, ()) = abstracttui::reactive::create_root(|cx| {
            let store = Store::create(cx);
            assert_eq!(store.gpu.get_untracked(), GpuMeter::Off);
        });
        root.dispose();
    }
}
