# Lane 1 — Performance · Reliability · Concurrency

Findings for the roadmap, researched 2026-07-22 against `abstractcode-tui`
v0.3.0 (post-release tree). Read-only pass: every finding cites the code it
stands on; reference comparisons come from reading
`codex-rs` (production Rust agent TUI, tokio), `opencode` (TS/Bun
server+TUI), `pi` (TS agent/orchestrator), and the Python `abstractcode`
sibling — not from memory of them.

Method note on measurements: the fold was timed against the real captured
fixtures (`cargo test --release --test run_tree_replay`: two full run trees,
287 KB + 235 KB, fold + assertions in **≤ 10 ms** total), and the full suite
is green in ~15 s. Costs that could not be measured without writing new
benchmark code are labeled *estimated* and each finding carries a measurable
success criterion so the fix wave can verify itself.

Effort scale: **S** < 1 day · **M** 1–3 days · **L** > 3 days.

---

## Summary table (ranked by value ÷ effort)

| # | Finding | Sev | Effort | One-line improvement |
|---|---------|-----|--------|----------------------|
| F1 | Catalog/tools never reload after a boot-time gateway outage — the "reconnects automatically" promise is broken for everything except the orb | P1 | S | Re-issue `LoadCatalog`/`LoadTools` on the `Conn::Down → Ok` transition |
| F2 | Runner command FIFO has head-of-line blocking: `Resume`/`Cancel`/`Start` queue behind `ProbeAttach` (≤ 20 history-bundle fetches), `LoadCatalog` (≤ 11 requests), and `FetchImage` (8 MB download + decode) | P1 | M | Spawn bulk commands on their own threads (the entity-lane pattern, already proven in-tree); keep control commands on the loop |
| F3 | Image memory unbounded: full-resolution RGBA bitmaps (up to ~67 MB each) retained forever, never pruned across sessions | P1 | S–M | Downscale at decode time on the worker to the mosaic ceiling (~0.1–0.5 MB), cap the entry list |
| F4 | `finish()` labels an unreachable-gateway terminal as `"completed"` → false Success outcome → queue drains against a dead gateway | P2 | S | Honest unknown: retry the status probe briefly, then report failed-unknown |
| F5 | Entity conversation items are unbounded (no `MAX_ITEMS` twin); convos are never removed in-session | P2 | S | Reuse the fold's chunked-truncation discipline per convo |
| F6 | Boot rehydration is serial: ≤ 20 sequential `history_bundle` fetches (whole run-tree ledgers) before the live-run reattach and before the first `Start` can run | P2 | M | Fetch newest-first, bound parallelism (~4), reattach the live run *before* replaying history |
| F7 | Malformed SSE `step` JSON is silently skipped while the cursor advances past it — silent record loss with no counter/label | P2 | S | Count + surface (`#FALLBACK` convention); never advance the cursor past an unparsed record silently |
| F8 | In-flight blocking HTTP is uncancellable; assessment of the std-thread model vs codex's tokio | P2 | L (deliberately deferred) | Keep the thread model; document the bounded-zombie windows; tokio+reqwest only if stream fan-out grows |
| F9 | Stale "model call Nm — provider may be slow" hint: `llm_inflight_since` survives `begin_run` and rehydration | P3 | S | Clear it in `begin_run` and at the end of `rehydrate_run_into` |
| F10 | Rendering-scale audit: the claimed bounds hold; two watch-items (feed rebuild cadence at cap, per-paint mosaic allocation) | P3 | — | No action now; criteria recorded for when scale grows |

**Not broken — do not "fix":** the stale-stream guard lattice
(`fold.is_following` + convo epoch guards), panic surfacing on every worker
thread, the cursor-resumable SSE + polling fallback (stronger than
opencode's snapshot-resync model), zero HTTP on the UI thread, the bounded
transcript fold, and the decision **not** to adopt the engine's
`channel_source`/`bounded_source` for ledger records (re-examined below —
the decision is correct).

---

## Architecture verdicts (the questions the brief asked directly)

### Threading model: std::thread + mpsc + WakeHandle vs codex's tokio

The hand-rolled model is the right call for this app *today*, with one
correction needed (F2). Reasons:

- The engine's reactive graph is single-threaded by contract
  (`abstracttui/docs/live-data.md:10-13`); every cross-thread write already
  funnels through `WakeHandle::post`, which coalesces bursts into one frame
  (waker dedup at `abstracttui/src/reactive/scheduler.rs:46-51`). A tokio
  runtime would add `Send` bounds and an executor without changing that
  boundary.
- Thread census is bounded and named: 1 runner + 1 root stream + ~2–3
  subrun streams per turn (they exit on their run's terminal) + 1 entity
  poller + 1 thread per in-flight entity op. Every spawn goes through a
  `catch_unwind` that posts the death to the UI
  (`src/runner.rs:958-971`, `src/gateway/entities.rs:872-901`) — codex gets
  the same property from tokio task supervision; we get it manually and it
  is test-pinned (`panic_fold_travels_the_wake_queue_end_to_end`,
  `src/gateway/entities.rs:997-1041`).
- What codex actually does differently that matters: its submission loop
  (`codex-rs/core/src/codex.rs:3812+`) is *also* a serial `while recv`,
  but handlers never block on network I/O — long work spawns as tasks, so
  `Op::Interrupt` is always prompt. Our loop blocks inside `handle()` on
  synchronous HTTP. That delta is F2, and it is fixable without tokio.

Adopt tokio only if the stream fan-out grows past ~a dozen concurrent
sockets per session (thread-per-stream stops being cheap) — see F8.

### The `channel_source`/`bounded_source` decision (re-examined)

The 0.2.0 decision (`src/runner.rs:11-23`) stands. Ledger records are
ordered state deltas folded into `Fold`; the engine's bounded lanes drop or
coalesce under pressure (`docs/live-data.md:42-87`), and a dropped record is
a lost tool result or a lost wait — silent corruption. `wake.post` is the
control lane, unbounded by contract, and the producer posts **one closure
per network read** (`src/runner.rs:1130-1138`), so a burst costs one wake
and one frame regardless of record count — the same guarantee opencode
hand-rolls with its 16 ms batch flush (`opencode/packages/tui/src/context/sdk.tsx:52-79`).
The one scenario that would change this verdict: the gateway starting to
stream *token deltas* (tens of events/sec). The engine is already ready for
that shape (`FeedState::push_stream`/`stream_append` — only the open tail
block re-typesets, `abstracttui/src/widgets/feed.rs:236-290`), and codex's
500 ms commit-tick pacing (`codex-rs/tui/src/chatwidget.rs:759`,
`app.rs:1039-1048`) is the pattern to copy for commit cadence, not a
bounded ring.

---

## Findings in detail

### F1 · P1 · Catalog never self-heals after a boot-time outage — effort S

**Evidence.** `Cmd::LoadCatalog` is sent exactly once, at mount
(`src/lib.rs:219-222`); no other site re-issues it (verified by grep — the
`/workflow` command only opens the picker, `src/ui/mod.rs:610`). The idle
probe ticker re-sends only `Cmd::Probe` (`src/ui/mod.rs:1254-1260`), which
restores the orb (`Conn::Ok`) but nothing else. Meanwhile the empty-state
screen promises "the app reconnects automatically once the gateway answers"
(`src/ui/transcript_view.rs:762`). If the gateway was down (or slow) at
launch: the orb goes green when it returns, but `store.workflows` stays
empty, so every `Start` refuses with "no agent workflows on this gateway"
(`src/ui/mod.rs:413-423`) until the app is restarted. Same staleness for
skills, the default route, and the cache probe. (`/tools` accidentally
self-heals because the command re-sends `LoadTools`, `src/ui/mod.rs:612-615`.)

**Reference.** opencode reconnects its event stream with capped exponential
backoff and re-runs `sync.start()` — a full state resync — after every
reconnect (`opencode/packages/tui/src/context/sdk.tsx:82-117`). codex
surfaces "Reconnecting… N/M" and retries with jittered backoff
(`codex-rs/core/src/codex.rs:9352-9377`, `util.rs:37-42`). Both treat
reconnection as *state* recovery, not just liveness recovery.

**Improvement.** An effect (or runner-side latch) that observes the
`Conn::Down → Conn::Ok` transition and re-issues
`LoadCatalog{saved prefs}` + `LoadTools` once per transition. Keep it
edge-triggered (the take-semantics mailbox pattern already used by
`last_outcome`) so a flapping connection doesn't storm the gateway.

**Success criterion.** Kill the gateway, launch the TUI, start the gateway:
within one probe period (≤ 30 s) a `Start` succeeds with no app restart.
Pin it with a headless test using a scripted client (`Conn` flip → catalog
command observed on the channel).

### F2 · P1 · Head-of-line blocking on the runner command FIFO — effort M

**Evidence.** One loop serializes every command
(`src/runner.rs:187-193`), and these handlers run blocking HTTP inline:

- `ProbeAttach` — `list_runs` + up to **20** `history_bundle` fetches
  (default `REHYDRATE_DEFAULT_TURNS = 20`, `src/runner.rs:46`), each
  carrying a run tree's complete ledgers (the captured coder tree is
  287 KB; 20 turns ≈ multiple MB over a remote link), then `input_data`
  (`src/runner.rs:629-752`).
- `LoadCatalog` — `list_bundles` + `discovery_providers` + up to 6
  per-provider model backfills + `capability_defaults` + `workspace_policy`
  (`src/runner.rs:389-486`); a *hanging* (not refusing) provider endpoint
  costs up to 60 s each (read timeout, `src/gateway/mod.rs:83`).
- `FetchImage` — up to 8 MB download + PNG/JPEG decode inline
  (`src/runner.rs:898-945`).

`Resume` (answering a tool approval!), `Cancel`, `Pause`, `Steer`, and
`Start` ride the same FIFO (`src/runner.rs:261-291`). Normal mid-run
approvals are fast (streams live on their own threads and the loop idles),
but the tail cases are user-felt: approve a tool right after a session
switch and the resume waits behind 20 bundle fetches; cancel while three
generated images download and the cancel waits behind them. The entity lane
already fixed this for itself — every entity command spawns its own thread
precisely so "a 30-600s entity read must never starve Probe/Start behind it
on this loop" (`src/runner.rs:292-295`, `src/gateway/entities.rs:231-233`) —
so the brief's "600 s entity turn blocks Start" is already false today; the
remaining blockers are ProbeAttach/LoadCatalog/FetchImage.

**Reference.** codex's serial submission loop stays responsive because
handlers spawn long work as tasks and return
(`codex-rs/core/src/codex.rs:3812+`; interrupt handled inline at the top of
the match). pi threads `AbortSignal` through every level so control can
preempt bulk work (`pi/packages/agent/src/agent-loop.ts:35-631`).

**Improvement.** Extend the entity-lane pattern to the three bulk commands:
`ProbeAttach`, `FetchImage`, and the discovery tail of `LoadCatalog` (the
provider-models backfill + cache probe). Each is already self-contained and
posts results through guarded closures, so the move is mechanical; the only
state to keep loop-owned is `stream_stops` (have the spawned ProbeAttach
send a new `Cmd::Attach{run_id}` back to the loop for the stream spawn —
the `tx` clone is already threaded everywhere). Do **not** move
`Start`/`Resume`/`Cancel`/`Pause`/`Steer`: they are short one-shot POSTs
and their ordering relative to `stop_streams` matters.

**Success criterion.** A headless test with a scripted gateway that stalls
`history_bundle` for 5 s: a `Resume` submitted during the stall completes in
< 100 ms. Live: session-switch on a 20-turn remote session, then immediate
approval — modal-to-resume latency < 200 ms (today: up to the full
rehydration time).

### F3 · P1 · Image memory unbounded (full-res bitmaps, never pruned) — effort S–M

**Evidence.** `fetch_image` decodes the artifact and stores the full
`Bitmap` behind an `Arc` in `store.images`
(`src/runner.rs:908-922`, `src/store.rs:94-99,197`). The 8 MB cap
(`ARTIFACT_MAX_BYTES`, `src/runner.rs:41`) bounds the *compressed* size:
a legal 4096×4096 PNG decodes to **67 MB** RGBA (`Bitmap` is RGBA8; the
engine's decoders impose no dimension cap — `abstracttui/src/gfx/decode.rs`,
`png.rs`). Entries are upserted by artifact id and never removed — not on
`/new`, not on session switch (grep: no `images.set(Vec::new())` or retain
anywhere). Ten generated images in an image-heavy session ≈ up to ~0.7 GB
resident *(estimated from format math, not measured)*. Rendering needs a
tiny fraction of that: the mosaic paint bilinearly resamples the source to
`cols × 2·rows` subpixels per paint (~100×28 cells → ~5.6 K samples,
`abstracttui/src/gfx/mosaic.rs:266-315`), so the full-res copy buys nothing
after decode — display resolution is capped at `IMAGE_ROWS = 14`
(`src/ui/transcript_view.rs:30`).

**Improvement.** In the worker, right after decode, downscale to the mosaic
ceiling (e.g. max 400×280 px ≈ 450 KB RGBA — comfortably above any terminal
cell grid the 14-row block can use) via the engine's
`resize_bilinear` and store only the small bitmap. Add a modest cap on
`store.images` (e.g. 64 entries, drop-oldest-without-visible-item) as the
belt. Optional third step: reuse one `MosaicRenderer` per image block
instead of constructing one per paint (`src/ui/transcript_view.rs:232-238`
allocates renderer + scratch + patch Vec every frame the image is visible —
~20 KB/frame churn; the engine type is explicitly designed for reuse,
`mosaic.rs:231-233`).

**Success criterion.** RSS delta after rendering ten 4096² PNGs ≤ 10 MB
(today: est. ~670 MB). Measurable with a headless test that decodes ten
synthetic large images through the fetch path and reads
`/proc/self`/`task_info` RSS, or simply by asserting stored bitmap
dimensions ≤ the ceiling.

### F4 · P2 · Terminal-status fallback lies "completed" — effort S

**Evidence.** `finish()` resolves the final status with
`unwrap_or_else(|| "completed".into())` when `get_run` fails
(`src/runner.rs:1253-1257`). A gateway that dies exactly at run end (or an
auth token that expires mid-run — the 401 path calls `finish` too,
`src/runner.rs:1167-1180`) yields `run_terminal("completed")` → `finished`
without `failed` (`src/transcript.rs:893-911`) → `last_outcome = Success`
(`src/runner.rs:1278-1284`) → the queue drain starts the next item against
a gateway that cannot serve it. Blast radius is bounded (the failed start
pauses the queue, `src/runner.rs:1004-1028`), but the transcript records a
success that never happened and one queued item burns a start attempt.

**Improvement.** On `get_run` failure inside `finish`, retry the status
probe 2–3× with short backoff; if still unreachable, report
`run_terminal("unknown")` → error card "run ended but its final status
could not be read — check the gateway" and write `RunOutcome::Failed` so
the queue pauses instead of draining. This matches the codebase's own
labeled-degradation convention.

**Success criterion.** Scripted-client test: stream ends, `get_run` refuses
→ fold shows the unknown-status card, `last_outcome == Failed`, queue
pauses with items kept.

### F5 · P2 · Entity conversations grow without bound — effort S

**Evidence.** `EntityConvo.items: Vec<Item>` (`src/convo.rs:46-58`) has no
truncation — grep shows no `TRUNCATE`/`MAX_`/`drain`/`truncate` anywhere in
`convo.rs`, and the store comment pins "closed transcripts stay readable;
never removed in-session" (`src/store.rs:255-257`). The agent fold caps at
`MAX_ITEMS = 500` + chunked hysteresis (`src/transcript.rs:22-26,343-371`)
precisely because unbounded items also make the `wire_feed` fingerprint
scan grow linearly (`src/ui/transcript_view.rs:593-619` iterates every
item per convo write; convo writes arrive per 7 s poll +
per turn). A day-long visit plus several convos accumulates items and
re-scan cost with no ceiling. Chat cadence keeps this small in practice —
it is a slow leak, not a cliff.

**Improvement.** Apply the fold's chunked truncation to convo items (same
constants, same standing `#TRUNCATION` notice — the transcript endpoint
keeps the full history server-side, so nothing is lost). One shared helper
rather than a second copy.

**Success criterion.** Existing truncation test generalized: a convo pushed
past `MAX_ITEMS + TRUNCATE_CHUNK` holds ≤ that bound with the notice first,
newest item intact.

### F6 · P2 · Boot rehydration is serial and blocks the live reattach — effort M

**Evidence.** `probe_attach` folds prior turns **oldest-first, one blocking
fetch at a time**, and only attaches the live run *after* the whole replay
(`src/runner.rs:672-752`). At 20 turns × (RTT + ~100-300 KB bundle) a
remote gateway spends seconds-to-tens-of-seconds before: (a) the live run's
stream attaches, (b) any queued `Start` can run (F2 shares this window).
JSON parse cost is negligible (measured: a 287 KB tree folds in ≈ 10 ms
release); the cost is network serialization.

**Reference.** opencode bootstraps by syncing the *current* session first
and lazily syncing others (`sync.tsx:445+`, per-session `syncingSessions`
map); codex renders resume history from a local rollout file (no network
per turn at all — different storage model, same "newest first, don't block
the live path" instinct).

**Improvement.** Three independently shippable steps: (1) reattach the live
run *before* replaying history (attach is one `input_data` fetch + stream
spawn; the replay currently prepends via `restored.items.append(&mut
f.items)` — the same merge works after attach); (2) fetch bundles
newest-first with a small parallel bound (4 threads — the fetches are
read-only and independent; fold application stays ordered by sorting before
the single post); (3) budget the replay by bytes as well as turns (a single
monster turn can dwarf 20 small ones).

**Success criterion.** Boot against a 20-turn remote session (or a scripted
client with 50 ms per-request latency): live-run records render < 1 s after
launch; full history restored < 3 s (today: strictly after ~20 × RTT+transfer).

### F7 · P2 · Silent record loss on unparseable SSE step events — effort S

**Evidence.** In `stream_ledger`, a `step` event whose JSON fails to parse
is silently skipped (`if let Ok(v) = serde_json::from_str` —
`src/gateway/mod.rs:440-446`); the *next* event's `on_cursor` then advances
the cursor past the lost record, so no reconnect will ever re-fetch it. No
counter, no notice — this violates the repo's own `#FALLBACK`/labeled-
degradation convention (which the truncation and preview paths follow,
`src/transcript.rs:152,164`). Likelihood is low (server-authored JSON), but
the failure is invisible and permanent per record; a fold that misses one
`waiting` record is exactly the "run never concludes / prompt never shows"
class the team just fixed a sibling of.

**Improvement.** Count parse failures per stream; on the first one, post a
notice ("ledger stream skipped an unreadable record — cursor N; the gateway
ledger keeps it") and keep going. Optionally: do *not* advance the cursor
for the failed event itself (advance only on successfully parsed events) so
the polling fallback re-reads it — the REST lane parses the same JSON, so
only a true wire-corruption case differs, and then the notice still fires.

**Success criterion.** Unit test: an SSE body with one corrupt `step`
between two good ones yields both good records, one counted+noticed skip,
and (if the cursor rule is adopted) a cursor that lets `get_ledger` re-serve
the middle record.

### F8 · P2 (assessment) · Cancellation of in-flight HTTP — deliberately deferred, L

**Evidence.** ureq is blocking; `stop` flags are only observed between
reads (`src/gateway/mod.rs:412-414`, `src/runner.rs:1126-1128`). Concretely:
an abandoned run's stream thread lives ≤ 75 s past its stop flag (stream
read timeout, `src/gateway/mod.rs:89-92`); an entity turn thread holds its
socket up to 600 s even after the convo is closed (results drop at the
epoch guard, the thread itself cannot be aborted —
`src/gateway/entities.rs:62-66`); quit does not join workers
(`src/lib.rs:271-289`) and relies on process exit to reap sockets — which
is fine for a TUI process, and codex/opencode ultimately do the same on
hard exit. The gateway-side run is correctly cancelled durably
(`Cmd::Cancel` → `/commands`), so nothing *user-visible* dangles; the cost
is bounded zombie sockets/threads.

**Reference.** pi threads `AbortSignal` through every loop level
(`agent-loop.ts`); codex drops futures and reqwest aborts the connection;
both get mid-request cancellation for free from their async stacks.

**Verdict.** Do not adopt tokio for this alone. The zombie windows are
bounded and harmless (≤ 75 s stream / ≤ 600 s entity turn, one socket
each), and the epoch/`is_following` guards make them behaviorally invisible.
Record the two windows in `docs/architecture.md` as accepted costs. Revisit
if (a) the gateway adds token-delta streaming (more sockets, longer lived),
or (b) goal trees start following > ~12 concurrent subruns
(thread-per-stream stops being cheap) — that is the tokio+reqwest
threshold, and it is an L-effort migration to plan, not to improvise.

### F9 · P3 · Stale slow-call hint across run boundaries — effort S

**Evidence.** `llm_inflight_since` is set on any followed run's
`llm_call started` (`src/transcript.rs:439-445`) and cleared only on
completion or `run_terminal` (`src/transcript.rs:896`). `begin_run` does
not clear it (`src/transcript.rs:241-262`), and `rehydrate_run_into` clears
`pending_wait` + `activity` but not this field (`src/runner.rs:1367-1369`).
Consequence: rehydrating a prior turn that died mid-LLM-call (failed run
with a started-but-never-completed `llm_call`) arms the hint at boot; 60 s
later the status strip shows "model call 1m00s — provider may be slow"
(`src/ui/chrome.rs:417-426`) on an idle session, and it persists until the
next run's first LLM completion.

**Improvement + criterion.** Clear the field in `begin_run` and at the end
of `rehydrate_run_into`; unit test pins both (rehydrated fold with a
dangling started call reports `llm_inflight_since == None`).

### F10 · P3 · Rendering-scale audit — claims verified, two watch-items, no action

**Measured/verified.**

- The fold is cheap: two real run trees (287 KB + 235 KB fixtures) fold in
  ≤ 10 ms total, release (`cargo test --release --test run_tree_replay`).
- Per-batch UI cost is O(items) but items are bounded: the fingerprint scan
  (`src/ui/transcript_view.rs:593-619`) touches ≤ 600 items
  (`MAX_ITEMS + TRUNCATE_CHUNK`), FNV over previews (~1 KB avg) ≈ sub-ms
  per posted batch *(estimated)*. The quadratic truncation trap was already
  fixed (chunked drains, `src/transcript.rs:343-371`) and is test-pinned.
- The engine Feed appends O(1) with prefix sums + windowed paint
  (`abstracttui/src/widgets/feed.rs:40-57,490-563`); in-place updates
  rebuild the prefix from the touched index (`feed.rs:223-232`) — worst
  case O(600) integers, noise.
- Damage-model comparison: codex-tui writes finished content to terminal
  scrollback (`insert_history.rs:27+`) and repaints only the live viewport;
  our engine keeps everything app-side but pays only visible rows per frame
  (windowing) and coalesces wakes per frame. Equivalent steady-state cost;
  codex's model gets native scrollback/search for free, ours gets keyed
  in-place updates (tool cards flipping status) that scrollback can never
  do. No adoption recommended.

**Watch-items (record, don't fix):**

1. At the item cap, every `TRUNCATE_CHUNK = 100` pushes trigger one full
   feed rebuild — ~500 `render_item` + typeset calls (markdown parse of up
   to 32 KB assistant bodies). Amortized ~5 re-typesets per push
   *(estimated 5–50 ms per rebuild)*. If cap-rate churn ever shows as a
   visible hitch, the fix is a ring-aware key scheme (slot keys modulo
   window, the live-data doc's recipe) — not a bigger cap.
2. If the gateway adds token-delta streaming, do **not** route deltas
   through fold items (one fingerprint + re-render per delta); use
   `FeedState::push_stream`/`stream_append` (open-tail-only re-typeset)
   with a commit cadence — codex paces commits at 500 ms
   (`chatwidget.rs:759`).

---

## Smaller observations (no roadmap slot needed)

- **Backoff shape.** Stream reconnect backoff is linear-capped
  (`(500 ms × errors).min(5 s)`, `src/runner.rs:1209-1211`) and unjittered;
  codex uses jittered exponential (`util.rs:37-42`), opencode 1 s→30 s
  exponential. With one client per user this is fine; add jitter only if
  fleets of TUIs ever share a gateway.
- **`drain_rest` asymmetry** (`src/runner.rs:1218-1240`): the belt runs on
  the poll-detected-terminal path but not on SSE `done` (`runner.rs:1141-1145`).
  Justified if the gateway drains before emitting `done` (the comment says
  so); worth one line in the gateway contract doc so a server-side change
  can't silently orphan tail records.
- **UI-thread file I/O**: prefs write-through on every queue mutation/send
  (`src/ui/mod.rs:1208-1213,1307-1318`) and roster cache writes in posted
  closures. Small JSON, atomic tmp+rename, sub-ms — fine; just keep large
  state out of `Prefs`.
- **Process-global `OnceLock` entity client + poller latch**
  (`src/gateway/entities.rs:220-226,734-735`): correct for one app per
  process; a future in-process re-mount (tests, embedded use) would get a
  stale client/dead poller. Note in the module docs.
- **exec lane**: 800 ms status polls with a deadline (`src/exec.rs:236-330`)
  — appropriate for headless CI; no change.

## What the references do better (adoption list)

| Pattern | Where they have it | Our gap | Adopt? |
|---|---|---|---|
| Control ops never blocked by bulk work | codex `submission_loop` + spawned tasks | F2 | Yes — via spawn-per-bulk, not tokio |
| Reconnect = state resync, not just liveness | opencode `sync.start()` after stream retry | F1 | Yes — catalog/tools re-issue on Down→Ok |
| Retry surfacing ("Reconnecting… N/M") | codex `notify_stream_error` | We flip the orb + one toast on first error only | Cheap add-on to F1 (P3) |
| Jittered exponential backoff | codex `util::backoff`, opencode sdk | Linear-capped, no jitter | Only if fleet-scale |
| Commit-paced token streaming | codex CommitTick 500 ms + engine `stream_append` | N/A until gateway streams deltas | Recorded as F10 watch-item 2 |
| Mid-request abort | pi AbortSignal / codex future-drop | Bounded zombie windows | No (F8) — revisit at the stated thresholds |
