# Entity collaboration + MCP plan (@name · /entities · parallel tracking · MCP)

Status: plan-cycle-2 output (2026-07-22) — cycle-1 claims verified against
gateway source (`entity_visits.py`, `entities.py`, `routes/entities.py`),
the live gateway at 127.0.0.1:8080, and this crate's `wire_feed`/`store`/
`root()`/`runner`. Implementation-ready. Changed decisions are marked
"(cycle-2:)". No code written against this document.

## Binding rulings honored (AGENTS.md + gateway source, verified)

- Entity visit turns are non-interruptible mid-turn; steering requires the
  not-yet-built H5 "steer rite" (gateway 403s inject_guidance on visit
  runs). v1 steering = BETWEEN TURNS (held draft, sent as the next turn).
- One life, one summon: a second open 409s; adopt the live visit instead.
- Quit ≠ goodbye: visits stay PARKED on quit. (cycle-2: verified — a
  standing `entity-visit-reaper` daemon thread exists in
  `EntityVisitHost.__init__`; `DEFAULT_VISIT_IDLE_S = 3600`; idle close is
  graceful server-side. The older "client must close" note is superseded.)
- Turn responses are SYNCHRONOUS (`_TURN_MAX_TICKS = 400` — minutes) and
  HTTP 200 can carry body status:"failed" — transport success ≠ operation
  success.
- No per-turn token totals; honest spend deltas come from /cognition
  (`spend.lifetime` / `spend.live_visit` — live-verified shape).
- (cycle-2:) Opening a visit on an ASLEEP entity does not refuse — it
  WAKES the entity (B1 ruling, `woke_for_visit` in `_preflight`) and close
  restores the prior state. Only `paused` (hard freeze), a live
  visit/chat, the opening-grace window, a non-yielding loop, and a refused
  prelude 409 the open. The client should say so honestly when the cached
  roster shows `asleep`: "castor was asleep — this visit wakes him; close
  restores his sleep."

## Item 1 — @name summoning

DECISION: `@name` = visit conversation (primary UX); `/task <name>
<title>` = durable task-inbox delegation (works while asleep; no visit).

`/task` confirmation copy — ADOPTED VERBATIM from the entity seat's
answer (commons 4312; engagement is boundary-scale, never mid-turn):
"Recorded on his desk — he takes it up at his next boundary: day end,
wake check, or visit close." Never say "immediately", never "he has been
notified", never a fixed minutes estimate (the wake cadence is a server
dial clients must not shadow).

- New `src/mention.rs`: leading-@ parse against the cached roster
  (case-insensitive slug match); unknown name → honest notice, draft
  preserved; bare `@name` opens/focuses without sending; mid-prompt @
  never routes (it is plain text at submit).
- Visit lane endpoints (verified in `routes/entities.py`):
  `POST /entities/{name}/visit/open` → `{run_id, visit_id, session_id,
  participants, prelude_warnings}`; `POST .../visit/{run_id}/turn` →
  `{run_id, reply, turn_n, status, tool_details, tools_ran?, memories?,
  notices?, output?, error?}`; `.../close`, `.../tick`; `GET .../visit`
  (status) → `{"open": false}` or `{open: true, run_id, session_id,
  visit_id, turn_n, status, workflow_arm}`; `GET
  .../visit/{run_id}/transcript` (works on live AND terminal runs;
  assistant turns carry tail-anchored `tool_details`).

### Adopt-on-409 (cycle-2: mechanism corrected — no prose matching)

Verified: every visit refusal is `VisitRefused(status, detail)` →
FastAPI `{"detail": "<human prose>"}`. There is NO structured error-code
field, and at least six distinct 409 prose bodies exist on the open path
alone (live visit "…one life, one summon; continue it with /turn…",
hosted chat, opening-grace posture, paused, loop-not-yielded, "summon
refused: …"). String-matching them would be brittle and is NOT needed:

- The client keys on `GwError.status == Some(409)` (already structured in
  `gateway::GwError`) and then calls `GET /entities/{name}/visit`:
  - `open: true` → ADOPT: take `run_id`/`turn_n` from the STATUS body
    (structured), `GET .../transcript` to rehydrate, focus the convo.
    (cycle-2:) rehydration honesty: `_visit.history` is a sliding window
    (last ~10 turns, per the transcript endpoint's own docstring) — when
    `turn_n` exceeds the rendered turns, prepend one Info line ("earlier
    turns live in the entity's memory, not this window").
  - `open: false` → non-adoptable refusal (paused / opening-grace /
    hosted-chat / prelude-refused): render the 409 `detail` verbatim as
    the honest notice. Never guess which case it was.
- Gateway ask #5 (new): machine-readable `code` field on visit-lane
  refusals (e.g. `visit_open`, `chat_open`, `paused`, `opening_grace`,
  `loop_busy`, `prelude_refused`). The client works without it; the ask
  is robustness for every future client.

### Per-turn thread (cycle-2: spec completed against runner.rs evidence)

One thread PER TURN, never the runner command loop (a 600s blocking turn
would starve Probe/LoadTools/Start behind it). Verified plumbing: the
existing stream threads are spawned from the RUNNER thread (not the UI
thread) and post through a CLONED `WakeHandle` — `WakeHandle` is
`Clone + Send + Sync` and `post()` is callable from any thread; only
`wake_handle()` (minting) must happen on the UI thread (lib.rs already
mints it at mount). `UiCtx` gains a `wake: WakeHandle` so the UI submit
path can spawn turn threads directly.

- Spawn: `std::thread::Builder::new().name(format!("visit-turn-{name}"))`
  with clones of (client, wake, name, run_id, turn_epoch, text). The
  turn uses a DEDICATED ureq agent (5s connect / 600s read) built once in
  `gateway/entities.rs` — never the shared 60s-read agent.
- Stale guard (the `is_following` twin): each `EntityConvo` carries
  `turn_epoch: u64`, bumped on every send/adopt/close. Every posted
  closure re-checks `convo exists && convo.run_id == run_id &&
  convo.turn_epoch == epoch` before touching state; a late result from an
  abandoned/ended convo applies NOTHING (the runner's stale-stream rule,
  mirrored — and the 2026-07-21 lesson: EVERY closure the thread posts
  gets the guard, not just the happy path: reply, timeout-notice,
  recovery outcomes, panic notice may skip it since it only notifies).
- Panic surfacing: body wrapped in `catch_unwind`; on panic post a
  notice via the wake handle (mirror `runner.rs::spawn_stream`).
- Read timeout → post "turn still running server-side" Info + enter the
  recovery loop ON THE SAME THREAD: `GET /visit` every 5s; when
  `status == "waiting"` → `GET /transcript`, diff by `turn_n`, fold the
  new turn; when `open: false` (idle-close or failure raced us) → fetch
  the transcript once more for the final state and mark the convo
  Closed/Failed with the last words rendered. Body `status:"failed"` on
  the turn response → error card + convo stays adoptable (the gateway
  finalizes failed runs; the next @name opens fresh).
- Quit mid-turn (cycle-2: verified safe): identical posture to stream
  threads — no join; `app.run()` returns and the process exits.
  Engine contract confirmed (`term/waker.rs`): "a waker outliving its
  terminal is harmless: wake() becomes a no-op against a closed channel";
  posted closures land in the Arc'd queue and are simply never drained —
  signals are never touched after teardown. Server-side the turn
  completes durably; the next launch adopts via @name → 409 → /visit →
  /transcript. No new teardown machinery.

`/end [name] [reason]` closes with closed_by=operator and renders the
close output (reflection summary). (cycle-2:) `/end` while a turn is in
flight is REFUSED client-side ("turn in flight — /end when it parks"):
close during a live turn races the drive loop server-side and has no
honest outcome to render. Quit during TurnRunning leaves everything
parked (ruled posture).

### Transcript mapping (unchanged, one addition)

Per-conversation `Vec<Item>` reusing the existing `Item` enum: reply →
`Assistant{final}`, `tool_details` → Tool cards, notices/prelude
warnings → Info, body `error` → Error. (cycle-2:) memories/records/diary
counts render as an always-visible one-line Info chip ("· 3 memories ·
1 diary entry"); the FULL texts go behind the details toggle via ONE new
variant `Item::Probe { title, body }` treated exactly like `Thinking` in
`is_visible` (details-gated). Cost enumerated: `render_item` +
`is_visible` + `fingerprint` + the `visibility_mirror_matches_render_item`
test — four mechanical touch points; the mirror test pins the pair.

## Item 2 — /entities discovery

DECISION: one modal — roster list + async detail card on select.

- (cycle-2: latency evidence corrected after live re-measurement +
  gateway-source reading.) The roster (GET /entities) took 18.5s and
  10.7s live this cycle. The cost is NOT lock contention: `list_entities`
  holds `_open_lock` only for per-entity dict lookups; the expense is the
  per-warm-home `cognition_drives()` fold (memory's `cognition_health`
  over the full self/diary/life ladder) running OUTSIDE the lock, ~2-4s ×
  5 warm homes. Consequence verified live: per-entity reads DURING an
  in-flight roster fetch answered normally — /visit 9.6ms-0.74s (first
  touch warms), /cognition 17-122ms, /state 17ms. The chips design
  survives a hanging roster.
- One real serialization hazard found (cycle-2:): `get_home` /
  `get_entity_runtime` CONSTRUCT the home/runtime INSIDE the global
  `_open_lock` on a cold miss — one slow cold open briefly blocks other
  entities' first-touch reads and the roster's dict gets. Bounded
  (seconds), but it shapes the poller: ONE sequential poller thread
  (natural backpressure, no overlapping polls), slow-lane timeouts.
  Folded into gateway ask #1.
- TUI posture unchanged: dedicated slow-lane ureq agent (5s connect /
  30s read), last-good roster cached in prefs (modal opens instantly
  with "as of HH:MM — refreshing…"), timeout → cached + honest note.
- Row: name · state (+liveness) · pending_tasks · drives ratios when
  present; error rows render as labeled broken homes (the roster serves
  `{slug, error}` rows for unreadable/moved homes — verified shape).
  Detail card renders the compositor sections with provenance behind the
  details toggle. Footer: [Enter] talk (@name) · [t] leave a task ·
  [e] end visit.
- `/entities <name>` deep-links to the card. '@' completion reads only
  the cached roster (never triggers a synchronous fetch).
- Roster fields verified live: `entities: [...]` top-level key (not
  `items`); entries carry `slug`, `handle` ("castor@10.0.0.215"),
  `state{state, liveness, mode, reason}`, `files`, optional
  `pending_tasks` (absent ≠ 0), optional `drives` (warm homes only).

## Item 3 — Parallel tracking

DECISION: v1 = status chips + focus switcher + between-turns steering;
v1.5 = per-entity life feed from the replay SSE; mid-turn cognition
channels BLOCKED on gateway ask #2 (no endpoint serves the per-home run
ledger).

### Conversation model (cycle-2: the feed mechanism, specified exactly)

Cycle-1 claimed "the session-switch path already exercises this" — FALSE
as stated. Session switch replaces `store.fold` wholesale and the sync
effect notices via its len-shrink trigger; a FOCUS switch changes which
item source the feed mirrors, and `wire_feed`'s sync state (the seen
fingerprint vec) lives inside the effect closure with NO focus dimension.
Unfixed, a switch to an entity convo with ≥ as many items as the agent
fold never rebuilds: same-index items with equal fingerprints are SKIPPED
(stale agent cards rendered inside the entity conversation), and the seen
bookkeeping is silently cross-contaminated. The fix:

- Store: `focus: Signal<Focus>` (`Focus::{Agent, Entity(String)}`) +
  `convos: Signal<Vec<EntityConvo>>` where
  `EntityConvo { name, run_id, session_id, items: Vec<Item>, status:
  ConvoStatus, held_draft: String, turn_epoch: u64, entity_state: String,
  last_spend: Option<u64> }` and
  `ConvoStatus::{Opening, Ready, TurnRunning{since}, Parked, Closed,
  Refused}`. All writes on the UI thread (turn threads post closures).
- ONE `FeedState`, ONE sync effect (created once in `root()` — never
  per-focus inside a dyn_view, which would leak effects). `SyncState`
  gains a `focus: Focus` field; the effect reads `store.focus.get()`
  FIRST and treats `focus != st.focus` exactly like a theme change: full
  rebuild — `st.seen.clear(); feed.clear(); re-push all` (the engine's
  documented rebuild seam, same cost as the details toggle). The item
  source is then `match focus { Agent => store.fold.with(..),
  Entity(n) => store.convos.with(..) }` over a shared `sync_items(&[Item])`
  body. Reactive property this buys (state it in code comments): signal
  reads are dynamic per run, so in Agent focus the effect tracks ONLY
  the fold + focus — background convo/poller updates never wake it; in
  Entity focus any convo write re-runs it and the fingerprint fast path
  no-ops (trivial at chat scale).
- Focus switch side effects: `follow.set(true)` (per-conversation scroll
  positions deliberately NOT preserved in v1); the shrink-clamp effect
  and the PgUp/PgDn `page` closure already read `feed.total_rows()` and
  need NO changes — they operate on whatever conversation is loaded.
- The details toggle stays app-global (one flag, both conversations);
  it is already a rebuild trigger and composes with the focus rebuild.
- Rejected alternative (recorded): one FeedState PER conversation with
  pane remount on switch — preserves per-convo scroll but multiplies
  engine typeset caches per open convo and adds a remount seam; switches
  are user-initiated and the rebuild is O(window). Revisit only if
  switch latency is ever felt.

### Every `store.fold` / `store.phase` consumer, enumerated (cycle-2:)

Focus-aware (must read `focus`):
1. `wire_feed` (transcript_view.rs) — item source + SyncState.focus (above).
2. `pane` empty-state memo + notices branch (transcript_view.rs) —
   `Focus::Entity(_)` is NEVER empty-state (a convo always holds ≥1 Info
   item from the open); memo reads focus.
3. `submit` routing (ui/mod.rs) — commands parse first (global); then
   `Focus::Entity`: Ready/Parked → send turn; Opening/TurnRunning → HOLD
   the draft with a banner ("held — sends when castor finishes this
   turn"), auto-send on turn completion (the ruled v1 steering);
   Closed/Refused → notice + offer reopen. `Focus::Agent`: unchanged
   phase switch (Running→steer, Starting→refuse, Idle→start).
4. `handle_escape` (ui/mod.rs) — entity focus + TurnRunning → honest
   "non-interruptible" notice, never the Esc-Esc cancel arm; composer
   clear + follow reset stay focus-independent.
5. `chrome::activity_strip` — entity focus renders the focused convo
   (status word · turn elapsed from `since` · held-draft marker ·
   spend delta when /cognition reports one). EXCEPTION (deliberate): a
   pending AGENT wait keeps owning the strip in ANY focus, prefixed
   "agent:" — an approval is urgent and the wait modal pops regardless.
6. `chrome::header` — chips row lives INSIDE the existing header line
   (print_clipped run, right of the route; no CHROME_ROWS change):
   `◆castor ✎3m · ◆ephemeral parked`, focused chip highlighted.
7. `chrome::status_bar` — legend swap in entity focus ("esc esc cancel" →
   "ctrl+e focus · /end close").
8. `chrome::composer` placeholder — focus-aware ("message castor —
   non-interruptible mid-turn; Enter holds during a turn").
9. `spawn_run_ticker` (ui/mod.rs) — tick condition extends to
   `phase != Idle || any convo Opening/TurnRunning` (entity turns need
   the spinner + elapsed display).

Agent-only, UNCHANGED (verified list — do not touch):
`start_run`, `steer`, `cancel_run`, `/pause`//`/resume` guards (they act
on the agent run regardless of focus; in entity focus they notify
honestly, e.g. "/cancel targets the agent run — entity turns are
non-interruptible"), `wire_wait_modals` + `auto_approve_wait` + the
approval/ask modals (agent-global; modal pops over any focus), the empty-
composer Enter wait-reopen (agent-global), `chat_messages` context build,
the boot Info push (lib.rs), `spawn_probe_ticker`, `wire_toasts`,
`wire_startup_notices`, `open_cache` stats, and ALL of `runner.rs` (the
runner never learns entities exist; entity HTTP lives in its own module).

`/new` + `/sessions` (cycle-2: decision): reset the AGENT conversation
only — entity convos are server-side visits and survive; both commands
force focus back to Agent. Entity convos are NOT auto-reopened at boot
(quit leaves them parked server-side; @name adopts). A `notice` at boot
when the roster cache remembers open convos is optional polish, cycle 5.

### Chips + poller (cycle-2: evidence-adjusted)

- Chips row as above; Ctrl+E cycles focus; `/focus <name|agent>` explicit.
- ONE poller thread, spawned lazily at first convo open, sequential
  (natural backpressure — no overlapping polls; the cold-open
  serialization hazard above makes overlap actively harmful). It cannot
  read signals: the UI thread maintains an `Arc<Mutex<PollerView>>`
  (open convo names + run_ids + stop flag) written on open/close/quit;
  the poller snapshots it, calls `GET /visit` per OPEN convo every 7s
  (9.6ms warm, ≤0.74s first-touch — measured), `GET /cognition` per open
  convo every 30s (spend deltas + drives), posts results through the
  wake handle with the same convo/run_id/epoch stale guard. Zero polling
  with none open; poller uses the slow-lane agent (30s read).
- A poll observing `open: false` on a convo we hold as Parked marks it
  Closed with an Info line (the reaper's idle close, surfaced honestly).
- Not built (rulings): pause/resume/cancel of a running turn; fabricated
  token counts; mid-turn progress (ask #2).

## Item 4 — MCP connect UX (cycle-2: v1 scope cut against the shipped modal)

Live shape verified: `GET /mcp/servers` → `{servers: [], source: null,
probed: false, warnings: ["no MCP server registry declared (create
/Users/.../mcp_servers.json with {...recipe...})"]}`.

Audit of the SHIPPED `open_mcp` modal: it already renders the count, name
+ url rows, an "(auth required)" badge, descriptions, the empty state,
the warnings note (wrapped), and the "tools appear in /tools" hint. The
cycle-1 "auth_required badges" work item is ALREADY BUILT — deleted.

v1 (small, worth shipping): parse `source` + `probed` into the store
(two fields on a new `McpRegistryInfo`), render ONE honesty header line —
"declared in <source> on the gateway host — not probed" (or "no registry
declared") — and format the recipe JSON out of the warning string as an
indented block instead of wrapped prose. "Copyable" costs nothing new:
engine screen-text selection (left-drag → OSC 52) is already enabled
app-wide (lib.rs).

Still rejected (stands from cycle-1): client-side editing of the
gateway-host config file (wrong machine, bypasses ownership); client-side
reachability probes (client-reachable ≠ gateway-reachable). v2 forms wait
on gateway ask #4 (write API + server-side probe lane); tokens via
`TextInput::masked` (verified present in abstracttui 0.2.1) and never
cached client-side.

## @-mention completion (cycle-2: exact provider rule, engine-verified)

Engine mechanics verified (`anchored_completion.rs`): a token triggers
when its FIRST cluster is the trigger char and the token is whitespace-
delimited — so `user@host` and `castor@10.0.0.215` never trigger ('u'/'c'
lead). Enter/Tab while the dropdown is open ACCEPT the highlighted
candidate (capture phase, stop_propagation) and never reach submit; a
provider returning empty CLOSES the dropdown, letting Enter submit.
Multiple `.trigger(char, provider)` registrations on one `Completion` are
supported (triggers is a Vec) — the '@' provider joins the existing '/'
one on the same builder.

The '@' provider rule (deliberately DIFFERENT from '/'):
1. NO whole-draft guard — the provider returns candidates for any
   @-token, leading (routing position) or mid-prompt (reference insert).
   Both insert `"@{slug} "` (trailing space). Mid-prompt, Enter-accept
   inserts the name and the NEXT Enter submits — standard editor
   behavior, documented in /help.
2. The '/' lane's rule-2 analog: a query that already EXACTLY equals a
   roster slug (case-insensitive) yields NO candidates — a fully-typed
   `@castor` submits (and routes) on the first Enter; the dropdown never
   swallows the send.
3. Candidates come from the CACHED roster only (empty cache = no
   dropdown, never a synchronous fetch); label = slug, detail = state
   word + pending_tasks when present.
Routing itself stays a SUBMIT-time parse in `mention.rs` (leading-@
only); the completion inserts text and never routes.

## New/changed modules

- `src/mention.rs` — leading-@ parse + completion provider (pure, tested).
- `src/convo.rs` — EntityConvo/ConvoStatus state machine + turn-response
  fold into items (pure, tested against live-shape fixtures).
- `src/gateway/entities.rs` — visit/roster/cognition client + the two
  dedicated agents (slow-lane 5s/30s; turn-lane 5s/600s).
- `src/entity_runner.rs` — turn threads + recovery loop + poller thread
  (the wake-posting half; no signals touched off-thread).
- `src/ui/entity_modals.rs` — /entities roster + detail card.
- Deltas: `store.rs` (focus, convos, McpRegistryInfo), `commands.rs`
  (/entities, /task, /end, /focus), `ui/mod.rs` (submit routing, Esc,
  ticker, /new focus reset), `ui/chrome.rs` (chips, placeholder, legend,
  '@' trigger), `ui/transcript_view.rs` (SyncState.focus, pane memo,
  Item::Probe rendering), `transcript.rs` (Item::Probe variant).
- `runner.rs` untouched. `lib.rs`: UiCtx gains `wake: WakeHandle`.

## External asks (updated; to post on agora)

- gateway #1 roster latency — SHARPENED with diagnosis: the cost is the
  per-warm-home `cognition_drives()` fold in `list_entities` (measured
  10.7-18.5s at 5 warm homes; per-entity reads stay fast — verified
  concurrently). The memory seat confirmed the mechanism from the engine
  side (commons 4311): the drives fold is a GLANCE read (O(records) by
  design, cache-at-the-panel cadence) consumed at list cadence, and the
  (home, current_seq) cache key is correct by construction (every
  drives-affecting write advances the seq; the folds are deterministic
  pure reads; current_seq() is O(1)). Ask: journal-seq-keyed drives
  cache (the `_COMMUNITIES_CACHE` precedent in routes/entities.py) or a
  `?drives=false` roster mode.
  STATUS (commons 4320, consumed 4323): FIXED gateway-side — seq-keyed
  `_ROSTER_DRIVES_CACHE` (entity-dir + journal seq; one fold per journal
  advance, shared across clients), pinned by tests, lands at the next
  gateway bounce. Re-time after the bounce and post numbers. Asks 2-4
  are QUEUED with gateway ownership (visit-ledger read behind env-kill
  phase 1; error codes fold into that wave; MCP write API rides the
  settings-registry work) — the plan's blocked-fence stands until their
  receipts. Secondary: cold home construction runs
  INSIDE the global `_open_lock` (`get_home`/`get_entity_runtime` miss
  path) — per-slug locks would stop one cold open briefly serializing
  other entities' first-touch reads.
- gateway #2 visit-run ledger endpoint (+SSE) for mid-turn progress —
  unchanged; blocks mid-turn cognition channels.
- gateway #3 steer-rite thread seat — unchanged (render honesty from
  `abstract.steer_seen` when H5 lands).
- gateway #4 MCP write API + server-side probe lane — unchanged.
- (cycle-2:) gateway #5 machine-readable `code` on visit-lane refusals —
  every `VisitRefused` serializes as prose-only `{"detail": ...}` today;
  the TUI works without it (status-code + GET /visit) but every client
  re-derives the same workaround.
- entity seat: task-inbox pickup semantics (what the /task confirmation
  copy may truthfully promise). /task ships with conservative copy
  ("recorded in castor's task inbox — pickup is his own act") if
  unanswered.
- abstracttui: nothing blocking (multi-trigger completion, masked input,
  tabs/badges verified present in 0.2.1).

## Build order (cycle-2: execution phase — 3 workers, 5 cycles)

Cycle 1 — foundations, fully parallel, all offline-testable:
- W1: `gateway/entities.rs` (client + two dedicated agents) + fixture
  files captured from the LIVE bodies this review recorded (roster,
  /visit open+closed, turn reply, transcript, /cognition, the 409
  details) + parse tests.
- W2: `convo.rs` (state machine, turn-response→items fold, held-draft
  rules, epoch semantics) + store additions (focus, convos) +
  `Item::Probe` in transcript.rs with the mirror-test extension.
- W3: `mention.rs` (parse + provider rules incl. exact-match close) +
  `commands.rs` additions + completions.

Cycle 2 — the highest-risk item first, on top of cycle 1:
- W1: `wire_feed` focus-awareness + pane memo + headless_ui tests that
  PIN the corruption cases: agent→entity→agent renders byte-identically
  after the round trip; same-length same-index unchanged-fingerprint
  switch still rebuilds; details toggle composes with focus.
- W2: `entity_runner.rs` turn thread + recovery loop + panic/stale
  guards; offline tests drive the posted-closure guards with scripted
  responses (the fold half is pure; thread mechanics get the live gate).
- W3: `/entities` modal on the cached roster + async refresh + detail
  card.

Cycle 3 — surface wiring (parallel, small seams):
- W1: submit routing + Esc + `/new`//session focus semantics + ticker.
- W2: chips in header + poller thread + spend deltas.
- W3: MCP v1 polish + '@' trigger wiring in chrome.rs + placeholder/
  legend/help text.

Cycle 4 — integration: headless Driver scripts (open→turn→switch→agent
run→switch back; held draft auto-send; 409-adopt against fixture
bodies; poller-observes-close), docs, /help.

Cycle 5 — live gate + defect burn-down + optional boot notice polish.

Externally blocked (build NOTHING behind these): mid-turn progress
(ask #2), steer-mid-turn (H5), MCP forms (ask #4).

### Live-test gating rule (cycle-2: verified)

Visits spend tokens and form memories in the visited entity's append-only
home. The sanctioned target is `doorcheck` — VERIFIED present on the live
roster this cycle (state=asleep, drives present, lifetime spend 12 llm
calls / 3 runs): a fixture entity whose purpose is exactly this — door
and gate checking (the walkthrough-gate lane). Rules:
- Live visit tests target doorcheck ONLY. castor, mnemosyne, hypnos,
  ephemeral are real lives — never open visits on them from tests.
- Opening WAKES an asleep doorcheck (B1); every test closes with
  closed_by=operator, reason="abstractcode-tui live gate", in a
  finally-style guard, restoring his sleep via the close's prior-state
  restore.
- The live gate is MANUAL (operator-run, one visit per gate run), never
  CI — same posture as the pty smoke.

## Ranking

Value: @name summon > chips/focus > /entities > /task > MCP polish.
Ship now: all v1 items. Blocked: mid-turn progress (ask 2),
steer-mid-turn (H5), MCP forms (ask 4).
