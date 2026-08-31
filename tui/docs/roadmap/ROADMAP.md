# abstractcode — Roadmap (final, sign-able)

Status: synthesis of the five roadmap documents (2026-07-22), for maintainer
sign-off. Detail backing lives in the five sources — item ids are carried
forward so every row traces to its finding:
`lane1-engineering.md` (F-series) · `lane2-ux-aesthetics.md` (UX-series) ·
`lane3-observability-features.md` (OBS/NEW-series) · `critique-feasibility.md`
(corrected efforts, dependency spine) · `critique-value.md` (value tiers,
phasing, M-series misses). Efforts below are the **feasibility-corrected**
ones; priorities are the **value-corrected** ones. Effort scale: S < 1 day ·
M 1–3 days · L > 3 days.

**If you sign, here's what happens first (in order):**

1. `Item::Tool` reshape + humanized cards (UX-01) lands **first and alone** —
   it is the hub node every later card feature rides (summary, raw args,
   provenance, timings in one shape change).
2. The trust bundle ships the same week: honest truncation labels
   (`finish_reason=length`, `retried ×N`), F4 honest unknown-terminal,
   F1 catalog self-heal, F7 counted SSE skips, F9 stale-hint clear.
3. Gateway ask **GW-A** (declared context window in discovery) is filed on
   agora so the context-% meter can land render-when-present in Wave 2.
4. Approval modal v0 (full content block, deny-with-reason, relabel) +
   attention pings (the app calls you back) close out Wave 1's core.
5. The three in-flight signed plans (interaction model, entities v1, tier
   policy) continue untouched in parallel; this roadmap sequences around
   them and never duplicates them.

---

## 1. Identity and thesis (the strategic frame)

**Ruling: abstractcode is mission control for agents that outlive your
terminal — a durable-gateway cockpit and a fleet seat. It is not a codex
clone and will not compete on standalone-agent polish.**

Two users: (a) the human operator at a terminal — real today, driving 5h+
coding runs daily; (b) the headless fleet seat — a real contract (the Python
`abstractcode bridge` protocol), but its `serve` subcommand is planned work
(interaction plan item 4), so (b) is honored, not optimized for, until it
ships. The fleet seat ships before fleet observability.

**The thesis: buy the coding-agent parity floor once, cheaply — then spend
everything else on what only a durable gateway can do.** The floor (legible
tool sentences, diffs, `@file`, context-%, `?` help) is almost entirely
projection-layer work on data the fold already holds; below that floor the
moat goes unvisited because the operator keeps codex open for real work. The
moat (durable resume — shipped; the subrun tree, the gateway-wide runs board,
session history + server-side summaries, wait/budget instrumentation, entity
conversations, the serve seat) is what no process-bound tool can copy. The
maintainer's own codex fork is the revealed preference: given full freedom
over codex he added subagent observability, context observability, and
durable memory — the depth wave, none of the polish wave. The parity line is
drawn hard: no token-streaming imitation, no backtrack-fork, no shell
passthrough, no sidebar (each has a recorded "needs contract/ruling" or
"wrong form factor" reason — see §7).

The biggest collective miss of all three lanes, fixed here: **a cockpit for
runs that outlive your attention must call you back** (M1, Wave 1).

---

## 2. Adjudications (where the two critiques disagreed)

Both critiques agreed on most verdicts; every disagreement is resolved here
with the reason. Everything both rejected is dropped (§7). Every merge both
identified is folded into a single item.

| Conflict | Ruling | Reason |
|---|---|---|
| UX-01 wave placement (feasibility: W2 · value: W1) | **Wave 1, first and alone** | The face is the quick win the wave is named for; the corrected M effort is accepted because the reshape is the hub enabler — deferring it defers five consumers. |
| UX-10a "writing answer…" label (value: keep · feasibility: it's a prediction) | **Feasibility wins** | The client cannot know if the in-flight call yields an answer or more tools; a prediction rendered as status breaks the receipts discipline. Ship "model call Ns · N tok/s" inside OBS-1a instead. |
| NEW-1 `/watch` entity feed (feasibility: priced M+ · value: skip) | **Value wins — parking lot** | The purpose-built observer app already exists with the exact honesty rules; entities plan stages the feed as v1.5. Trigger: post-entities-v1 demand. |
| F6 rehydration (feasibility: W2, harder-than-written M · value: nice, W3) | **Wave 3 at M** | Value's priority (gateway is localhost today), feasibility's effort (attach-first merge is a new state-preserving fold surface). Sequenced before OBS-4, which shares the path. |
| F5 + UX-11 (feasibility: W1 · value: batch with entities in W3) | **Wave 1** | The entities-v1 build is landing `convo.rs` and the roster modal *now* — bound the convo and word the drives at birth instead of retrofitting a shipped leak. |
| NEW-4 budgets (feasibility: W1 · value: W3 instrument panel) | **Wave 2** | Unattended runs spend today (4 multi-hour waiting roots found live), so it shouldn't wait for Wave 3 — but Wave 1 stays trust+face themed; NEW-4 pairs with the Wave-2 meter work as one strip pass. |
| OBS-7 wait visibility (feasibility: W1 · value: W3) | **Wave 3** | Its tenant (the goal bundle) isn't live; S-effort whenever; mandatory before `/goal` lights up — Wave 3's DoD carries it. |
| OBS-6 `/gpu` (feasibility: W1 · value: W2 "slips in") | **Wave 2** | Not a trust fix, not face polish; rides F1's edge-trigger pattern which lands in Wave 1. |
| OBS-5 artifacts (feasibility: M confirmed · value: nice, browser waits) | **Descoped OBS-5a in Wave 3** | Artifact cards + save-to-disk ship (silently dropping non-image artifacts is a completeness defect); the browser modal parks on demonstrated need. |
| UX-05 context-% (lane: client-only M · feasibility: blocked) | **Split** | Feasibility verified no gateway route serves a context window and no pricing data exists. NEW-4 ships now as the honest core; ctx-% ships render-when-present behind gateway ask GW-A; the $ meter is a non-goal (§7). |
| OBS-1b `/stats` (feasibility sequence: W2) | **Wave 3** | It shares the per-run aggregation with OBS-2 `/tree`; co-scheduling buys the work once. |

**Decisions taken (signing ratifies them; strike to reverse):**

- **D-1 — `@file` dependency posture**: hand-rolled bounded walk (depth/entry
  caps, hardcoded `.git`/`target`/`node_modules` excludes) — keeps the crate's
  3-dependency discipline. The `ignore` crate (true gitignore semantics) only
  if the walk proves insufficient in use; that reversal reprices UX-03 M→L.
- **D-2 — Ctrl+T stays theme.** The Wave-3 pager binds `o` on a focused card
  + `/inspect`; rebinding a shipped chord invalidates muscle memory for
  marginal gain and the `?` overlay covers discoverability.
- **D-3 — Wave 1 carries one M** (the UX-01 reshape) by design; it is the
  enabler, not scope creep.

---

## 3. In-flight signed work (sequence around, never duplicate)

These proceed in parallel with their own build orders; roadmap items that
touch their surfaces are marked with the dependency.

| Plan | Ships | This roadmap's relation |
|---|---|---|
| `plan-interaction-model.md` | steer/queue + pending_steer, Ctrl+J multiline, `/goal` client (dark, `finish_on_root_only`), **serve subcommand** | OBS-3/OBS-4 must extend the goal defense (§8); M4 rides serve's build; queued-next preview rides the queue work. Engine asks E1–E4 already filed. |
| `plan-entities-mcp.md` | @name visits, `/entities` roster, chips + poller + focus, MCP v1 polish; entity feed named v1.5 | F5 bounds `convo.rs` at birth; UX-11 words the roster; NEW-1 parked as the plan's own v1.5 trigger. Gateway asks #1 (fixed, awaiting bounce) – #5 already filed. |
| `tier-policy-agora-facts.md` | accepted-tier → run policy; discovery `tier`/`approval` fields (in-tree, land at next gateway bounce) | UX-07's footer shows the accepted tier at rest; the client drops its #FALLBACK name table the day the fields serve. |

---

## 4. The dependency spine (enablers first)

```
UX-01 Item::Tool reshape (summary + raw args + provenance + timings)
  ├─► OBS-1a tool-duration suffix        (Wave 1, same wave, after)
  ├─► UX-02/04a/NEW-5 diff design        (Wave 2)
  ├─► UX-08p pager + M2 payload inspect  (Wave 3)
  └─► UX-13 coalescing                   (parked)

F2 spawn-per-bulk (runner FIFO)
  ├─► OBS-3 /runs · OBS-4 history · OBS-5a artifacts · NEW-2 /summary
  │    · UX-08p fetch lane               (every new fetch lane piles onto
  │                                       the FIFO until F2 lands)
  └─► F6 parallel rehydration ──► OBS-4  (shared rehydrate path)

F1 Down→Ok edge-trigger pattern ──► OBS-6 poll gating
OBS-1a fold fields (gen_time/finish_reason/attempt/timestamps)
  ──► OBS-1b /stats · OBS-2 per-run stats · NEW-3 export timings
goal defense (finish_on_root_only, interaction plan) ──► OBS-3 adoption
  + OBS-4 viewing (must re-derive the flag or the iteration-1 false-finish
  P0 returns)
GW-A gateway ask (context window in discovery) ──► UX-05a ctx-%
entities-v1 build (in-flight) ──► F5 · UX-11 · (NEW-1 trigger)
serve build (in-flight) ──► M4 unattended-honesty bundle
flow goal bundle (external, M5) ──► /goal lights up; OBS-2/OBS-7/NEW-4 are
  its instrument panel, built before the tenant arrives
```

---

## 5. The waves

### Wave 1 — "Never lie, look premium"

The honest-and-premium quick-win bundle: the cockpit stops lying
(truncation, false-completed, silent loss, stale hints, broken reconnect
promise), the default face stops being JSON, and the app calls you back.

| # | Id | What | Effort | Tier | Depends on | Acceptance criterion |
|---|---|---|---|---|---|---|
| 1 | **UX-01** | `Item::Tool` reshape (intent summary computed at STARTED/wait time · raw args · provenance run_id+step_id/call_id · timing fields) + humanized sentence cards via `intent_summary` | **M** | must | none — **first and alone** | A finished session read top-to-bottom shows zero `{"` outside code fences; slim/argless terminal records still render the STARTED-time summary; 283-test suite + fingerprint test green. |
| 2 | **OBS-1a** | Honesty labels: `finish_reason≠stop` on answers/cycles, `retried ×N`, per-cycle `gen_time`/tok-s, strip reads "model call Ns · N tok/s" (absorbs UX-10a, corrected wording), batch-labeled tool durations | S | must | UX-01 (tool-duration suffix only; llm-side labels independent) | A max_tokens-cut answer renders "answer cut by token limit"; tool durations are batch-labeled or single-call-only and never include human approval time unlabeled. |
| 3 | **F4** | Honest unknown-terminal: retry status probe, then `run_terminal("unknown")` + Failed outcome (+ `drain_rest` contract doc line) | S | must | none | Scripted test: stream ends, `get_run` refuses → unknown-status card, `last_outcome == Failed`, queue pauses with items kept. |
| 4 | **F1** | Catalog/tools re-issue on `Conn::Down → Ok` (edge-triggered) | S | must | none | Kill gateway → launch → start gateway: a `Start` succeeds within one probe period (≤30s), no app restart; headless test pins the flip→command. |
| 5 | **F7** | Counted SSE-skip notice (notice half only; + the REST-lane page-parse sibling noted in the same PR) | S | must | none | Corrupt `step` between two good ones → both good records fold, one counted + noticed skip. |
| 6 | **F9** | Clear `llm_inflight_since` in `begin_run` + end of `rehydrate_run_into` | S | must | none | Rehydrated fold with a dangling started llm_call reports `None`; unit-pinned. |
| 7 | **UX-04** | Approval v0: full `write_file`/`edit_file` content as highlighted block in the modal body (no diff yet — modal already holds full args) · deny-with-reason (04b) · "always allow (session)" relabel (04c) | S | must | none | An edit approval shows every line of the content without leaving the modal; a denial can carry a reason without touching the composer. |
| 8 | **M1** | Attention pings: bell/OSC-9/99 on approval-wait, ask-user, run-terminal while unfocused (engine verbs + DEC 1004 focus events verified present in abstracttui; config toggle) | S | must | none | A blocked approval on an unfocused terminal raises a desktop notification/bell within one frame of the wait record; focused sessions stay silent. |
| 9 | **UX-09** | Session identity card at boot: version · workflow · route · **cwd** · workspace mode · session (absorbs UX-15 boot-notice fold; wordmark once) | S | high | none | Model + directory + workspace mode readable at boot with no modal opened. |
| 10 | **UX-07** | `?` shortcuts overlay + slim footer (`? shortcuts · accepted-tier · theme · gateway`) — tier surfaces at rest | S | high | tier fields (render-when-present) | Footer fits at 80 cols with zero truncation; every removed hint reachable within one `?`. |
| 11 | **F3** | Image downscale at decode to the mosaic ceiling + entry cap (+ reuse one MosaicRenderer per block) | S–M | high | none | Stored bitmap dimensions ≤ ceiling; ten 4096² PNGs cost ≤10 MB resident (vs est. ~670 MB). |
| 12 | **F5** | Entity convo bounds — the fold's chunked truncation as a shared helper, landed with the in-flight entities build | S | nice | entities-v1 `convo.rs` | Convo pushed past `MAX_ITEMS + TRUNCATE_CHUNK` holds ≤ bound, `#TRUNCATION` notice first, newest item intact. |
| 13 | **UX-11** | Drive ratios in words (`questions 0/6 · problems 0/2 · interests 1/61`, or legend line) in roster + identity card | S | nice | entities-v1 roster modal | No bare `q/p/i` visible to a first-time user. |
| 14 | **POLISH-1** | UX-12 composer `❯` glyph · UX-14 fuzzy `/` dropdown + row-cap lift · UX-16 theme-picker title fix · UX-17 help-modal widths | S (batch) | nice | none | `/wf` finds `/workflow`; no self-truncating modal titles; help descriptions clear the scroll gutter. |

**External asks (Wave 1):**

- **GW-A (gateway, NEW — file on agora week 1):** "Serve the declared
  context window per model in `GET /discovery/providers` model entries (or
  the capability route the client already reads) so thin clients can render
  `ctx used/window (%)` honestly. Render-when-present contract; absence
  degrades to today's raw `ctx Nk`." Unblocks UX-05a in Wave 2.
- **Engine (abstracttui, nice-to-have, non-blocking):** per-row ink API on
  `List` (UX-16 accent swatch); confirmed NOT needed: notify verbs + focus
  events already shipped.

**Definition of done (Wave 1):** a codex driver sees sentences not JSON,
reviews the full content of what he approves, is never lied to about
completion/truncation/retries/connection, and the app calls him back when it
needs him. All items headless-test-pinned; 283-suite green; no new
dependency.

---

### Wave 2 — "The coding floor"

The M items that make it the daily driver — the operator stops keeping
codex open. F2 lands here because every Wave-3 fetch lane piles onto the
FIFO until it does.

| # | Id | What | Effort | Tier | Depends on | Acceptance criterion |
|---|---|---|---|---|---|---|
| 1 | **F2** | Spawn-per-bulk on the runner: `ProbeAttach`, `FetchImage`, `LoadCatalog` discovery tail move off the FIFO (entity-lane pattern); control commands stay loop-owned; **identity guard** on late `Attach` (run/session unchanged, no newer start) | M | high | none — **enabler for Wave 3** | Scripted gateway stalling `history_bundle` 5s: a `Resume` during the stall completes <100 ms; new race test pins the stale-ProbeAttach guard. |
| 2 | **UX-02 / UX-04a / NEW-5** | The one diff design (merged): args-derived −old/+new hunks for `edit_file` find/replace · syntax-highlighted content for fresh `write_file` · server-provided diffs passed through · `(+N −M)` counts · the same block in the approval-modal body · `files_touched` fold + `/files` + "N files changed" on the final card. **Governing rule (NEW-5): the client never fabricates old bytes or context lines.** | M | must | UX-01 (raw args) | An edit approval and its finished card show tinted hunks with counts; a fresh write shows highlighted content; nothing rendered as a diff that the client didn't honestly have; `/files` lists per-run touched paths. |
| 3 | **UX-03 / NEW-6** | `@file` mentions (merged): bounded local walk under the workspace root per D-1, entities-first collision rules, dropdown disambiguation (`◆ castor — entity` vs `src/main.rs — file`), **locality gate** (remote gateway → provider off + `/help` says why) | M | must | mention infra (entities plan, shipped) · D-1 | Typing `@main` offers `src/main.rs`; accepted mention round-trips into a successful `read_file`; remote-gateway posture never offers paths the agent can't read. |
| 4 | **NEW-3 / UX-08e** | `/export [md\|json] [path]` (merged): md from fold items **with `#TRUNCATION` header when previews/caps truncated**, json = `history_bundle` verbatim; collision-safe default path | S | high | OBS-1a (timings in md, optional) | Export of a capped session carries the truncation label; json round-trips the bundle byte-verbatim. |
| 5 | **UX-06** | Type-to-filter in the shared `Picker` (index remap for `on_choose`) — serves `/model`, `/theme`, `/workflow`, `/sessions` | M | high | none | `/model` stage 2: typing narrows 342 rows live; selection resolves to the correct original index (test-pinned). |
| 6 | **NEW-4** | Token budgets: `/budget <n>tk [session\|run]` strip thresholds (warn, never auto-cancel) + `_limits` passthrough in `run_input` so the server warns too | S | high | none | Crossing a threshold renders one warn-tinted strip notice; `_limits` visible in the started run's input_data. |
| 7 | **UX-05a** | Context-window % in the strip (`ctx 41k/262k (16%)`, warn ≥75%, error ≥90%) — **render-when-present** on GW-A's field; absence keeps today's honest `ctx Nk` | S | must (when unblocked) | **GW-A (gateway)** | With the field served: % visible during every run, red before overflow; without it: no fabricated window, no client-shipped table. |
| 8 | **OBS-6** | `/gpu` status-bar meter, polled only while a run/turn is active (idle frames stay 0-cost); `supported:false` renders once | S | nice+ | F1 pattern | Meter appears during runs on the local-inference gateway; zero polls while idle (test-pinned). |
| 9 | **POLISH-2** | UX-18 glyph audit (one weight family + mapping table in `docs/architecture.md`) · UX-19 modal edge contrast (client-side `Block` border; 26-theme audit coordinated with the engine's contrast harness) | S (batch) | nice | none | Glyph table documented; no theme where the modal edge melts into the transcript. |

**External asks (Wave 2):** none new. GW-A consumed here when it lands
(filed Wave 1). Engine contrast-harness coordination for UX-19 is
engine-adjacent, non-blocking.

**Definition of done (Wave 2):** one real coding task driven end-to-end in
this TUI with no reason to switch away: file edits reviewed as honest
hunks before approval, prompts point at files with `@`, context fullness
visible (or honestly absent), approvals/cancels never queue behind bulk
fetches. The operator stops keeping codex open.

---

### Wave 3 — "Mission control"

The moat made visible: the goal-loop and fleet instrument panel, built
before its tenants arrive (flow's goal bundle, the serve seat's fleets).

| # | Id | What | Effort | Tier | Depends on | Acceptance criterion |
|---|---|---|---|---|---|---|
| 1 | **OBS-2** | `/tree` subrun modal: one row per followed run (indent by parent · workflow/node · status · cycles · per-run tokens · elapsed · last activity) + strip chip `· N subruns active`; per-run stats accumulate in `Fold::apply` | M | high | OBS-1a fields; Fold seam care (§8) | A basic-agent run shows root + agent loop + pollers as a tree; per-run token split sums to the session total; transcript-filter v2 deliberately out. |
| 2 | **OBS-3** | `/runs` gateway board, **slim v1**: rows (`status · workflow · session · age · waiting-reason`) · Enter adopts (same-session) · `c` cancel with confirm · **`finish_on_root_only` re-derived at adoption** from the run's workflow id | M | high | F2; goal defense | The 4-multi-hour-waiting-roots case is visible in one command; adopting a goal-bundle run does NOT reintroduce the iteration-1 false finish (new test); loaded-scope counts labeled, never "all time". |
| 3 | **UX-08p / M2** | Full-content pager (merged): `o` on a focused tool card / `/inspect` opens the full text the truncation notes point at (provenance from UX-01 + `get_ledger` fetch lane) — **plus M2**: the same pager opens an `llm_call`'s request side (sizes, message count, declared tools/skills) — "what did the model actually see" | M | high | UX-01 provenance · F2 · D-2 | Every `#TRUNCATION`/"full text in the run ledger" label in the app resolves in ≤2 keys; a cycle card can open its request payload from the ledger. |
| 4 | **F6** | Boot rehydration: attach live run first (state-preserving prepend merge — new fold surface), newest-first bundle fetches with bounded parallelism (~4), byte budget | M | nice | F2 | Against 50 ms-latency scripted client: live-run records render <1 s, full history <3 s; merge preserves `followed`/`parents`/cycles (test-pinned). |
| 5 | **OBS-4** | Session run-history browser: session → turns list → Enter rehydrates that run **read-only** (banner; steer/cancel/approve/queue-drain refuse to target the dead run; live state restored on exit) | M | high | F2 · F6 · goal defense | A past turn's full transcript opens and no mutating action can reach it; exiting restores the live run's view; queue drain never fires from viewing mode. |
| 6 | **M3** | Server-side session discovery: `/sessions` lists gateway-known sessions (v1: derived client-side from `GET /runs` grouped by session, loaded-scope-labeled) | S–M | high | OBS-3 fetch lane | "Attach from anywhere" true on a machine that never started the session; soft gateway ask filed for a first-class session index. |
| 7 | **NEW-2** | `/summary` (+`/ask-run` follow-up): server-side ledger-grounded run summary, rendered as a durable markdown card; spend labeled, never auto-fired; one manual probe of response shapes before building the render | S–M | high | F2 | A 5-hour run summarizes in one command with a visible "this spent tokens on the gateway's route" label. |
| 8 | **OBS-1b** | `/stats` run breakdown: time in model vs tools vs waiting-on-user, slowest calls, per-model split — with the splitless-usage provenance labels ("provider reports no input/output split") | M | nice | OBS-1a; shares per-run aggregation with OBS-2 | Every number in `/stats` traces to ledger receipts; absence renders as a labeled state, never zero. |
| 9 | **OBS-7** | Wait/schedule visibility: `wait_until` folds into the strip ("sleeping until HH:MM — resumes itself"); schedule metadata in OBS-3 rows | S | high | none — **before the goal bundle lights up** | A run parked on `wait_until` never reads as hung. |
| 10 | **OBS-5a** | Artifact cards (descoped): non-image artifacts on a final answer render a card (name · kind) instead of being dropped; `s` saves to disk via streaming download (no 8 MB inline cap on disk saves; `access_action` labeled) | S–M | nice | F2 | No artifact named by a final's meta is silently invisible; a 50 MB artifact saves to disk without truncation. |
| 11 | **M4** | Unattended-honesty bundle (conformance, rides the serve build): exit-code truth (`fold.failed` fix — in the serve plan), F4/F7/OBS-1a labels inherited by exec/serve event streams, budgets available headless — one named deliverable: "the headless seat meets the same never-lie bar as the TUI" | S | high | serve subcommand (in-flight) | Conformance fixtures assert a truncated answer, an unknown terminal, and a counted skip surface identically through the JSONL lane. |

**External asks (Wave 3):**

- **Flow seat (already asked, commons 4302 — tracked as M5):** goal bundle id
  + input contract + the `answer_user` interim-results constraint. This
  wave's instrument panel (OBS-2, OBS-7, NEW-4) is built so `/goal`
  demonstrates the moat the day the bundle publishes. Strategic dependency,
  tracked on the roadmap, not a build item.
- **Gateway (soft):** first-class session index (`GET /sessions`-shaped:
  session id, last activity, root-run count) — M3 works without it via
  `/runs` derivation; the ask is efficiency + completeness beyond the
  loaded scope.
- **Gateway (reference, already filed):** entities asks #2 (visit-run ledger
  for mid-turn progress) and #5 (machine-readable visit refusal codes)
  remain queued gateway-side; nothing in this wave builds behind them.

**Definition of done (Wave 3):** the durable/multi-agent story is something
you can watch, not something you read about in the architecture doc: every
followed tree visible as a tree, every gateway run visible from the
terminal, every past turn reopenable, every truncation label resolvable
in-app, and the goal-loop instrument panel ready before its tenant arrives.

---

## 6. Honest cost summary

| Wave | Items (rows) | S | S–M | M | Client-only | Blocked on a seat |
|---|---|---|---|---|---|---|
| 1 — Never lie, look premium | 14 | 12 | 1 (F3) | 1 (UX-01) | 14 of 14 | none (GW-A *filed*, not consumed) |
| 2 — The coding floor | 9 | 5 | — | 4 | 8 of 9 | UX-05a on GW-A (gateway) |
| 3 — Mission control | 11 | 2 | 3 | 6 | 11 of 11 hard-buildable | M4 waits on serve (in-flight, this repo); wave DoD references flow's goal bundle (M5) |
| **Total** | **34** | **19** | **4** | **11** | ~33 | 1 hard (UX-05a), 2 soft |

Sequencing estimate (S <1d, M 1–3d; ranges, no fabricated precision):
Wave 1 ≈ 1.5–2.5 weeks · Wave 2 ≈ 2–3.5 weeks · Wave 3 ≈ 3–5 weeks —
**≈ 7–11 weeks single-threaded**; the repo's proven 3-worker/5-cycle
pattern compresses wall time, minus whatever the in-flight plans (queue/
serve, entities v1, tier) still occupy. Waves are value/risk phases, not
calendar promises.

---

## 7. Non-goals (deliberately cut — the reason, one line each)

| Item | Reason |
|---|---|
| $ cost meter (UX-05b) | No pricing fields exist anywhere in the framework registry; a dollar figure would be a fabricated number in an app whose brand is receipts. Trigger: pricing table lands framework-side AND paid routes become routine. |
| Staged answer-reveal animation (UX-10b) | Streaming theater — animating an already-received answer imitates a transport capability that doesn't exist; also fights F10's feed-churn guidance. Never. |
| Client-side OTEL exporter | Instruments the wrong end: the gateway's durable ledger is the telemetry substrate and outlives every client; if wanted, it's a gateway feature. |
| NEW-1 `/watch` entity live feed | Duplicates the purpose-built observer app for an audience of one; entities plan already stages it as v1.5. Trigger: post-entities-v1 demand ("the poller isn't enough"). |
| UX-13 tool-card coalescing | Feasibility-corrected M→L: fights the keyed-update/finish-matching machinery (~15 pinned tests); UX-01's sentences + clean-mode folding cover ~80%. Trigger: post-UX-01 noise persists. |
| F8 tokio migration | Bounded-zombie windows are accepted, documented costs. Trigger: gateway token-delta streaming or >~12 concurrent followed streams. |
| F7 cursor/re-serve half | Halt-at-gap + REST refetch is M for a server-authored-JSON failure mode; the counted notice (shipped, Wave 1) is the honest fix. |
| OBS-5 full artifact browser modal | Save-to-disk + artifact cards (OBS-5a) are the useful part; a browse-everything modal duplicates the web observer. Trigger: demonstrated need. |
| Audit-tail pane · checkpoints/undo · run comparison · fleet panel | Lane 3's own discipline, both critics endorsed: doctor/observer cover the tail; no runtime snapshot machinery exists to fake; export+diff covers comparison; no fleet exists yet (trigger: first real fleet). |
| Sidebar · leader-key/which-key · OSC-133 · DeepSeek adoption | Wrong form factor for a single-column 80–110-col cockpit; nothing to adopt. |
| Image paste · Esc-backtrack fork · `!` shell passthrough | Each needs a contract or ruling first (attachment lane demand; session-fork/seed contract; thin-client boundary ruling). Parked with those triggers. |
| Real token streaming (gateway lane) | Not filed as an ask: it's a major gateway/runtime lane, and the client half is already specced for the day it exists (F10 watch-item 2: `push_stream` + commit cadence, never fold items). Trigger: maintainer wants live answer text. |

---

## 8. Risk register

**Load-bearing seams six agents just hardened — items touching them carry
the test-suite obligation (283 tests green + the named pins extended):**

| Seam | Touched by | The rules that must survive |
|---|---|---|
| `Fold::apply` + `Item::Tool` | UX-01, OBS-1a/b, OBS-2, UX-02/04a/NEW-5, F6-merge | Answer-lane (root or first-level cycling run only); delegate-pollution guards; wait identity = (wait_key, step_id) never key-only; slim terminal records arrive argless (summaries computed at STARTED time); `tool_key` matching + `finish_tool`'s oldest-unfinished-same-name fallback; the fingerprint test extends with every `Item::Tool` field added. |
| `wire_feed` | UX-01 render, UX-08p, POLISH-2, (parked UX-13) | Index-keyed fast path (items vanishing mid-list force rebuilds); the visibility-mirror test is the tripwire; the in-flight entities focus dimension (SyncState.focus) composes with every change here. |
| Runner FIFO + stream lattice | F2, F6, OBS-3/4/5a/NEW-2 fetch lanes | Ordering contracts (outcome-before-phase, stop_streams-before-spawn) are test-pinned; F2's spawned ProbeAttach needs the identity guard (a late Attach must not stop a newer run's streams); **every stale-outcome closure re-checks currency** (`fold.root_run_id()`/`is_following`/epoch) — the rule that already bit once per lane. |
| Goal defense | OBS-3 adoption, OBS-4 viewing | `finish_on_root_only` must be re-derived for adopted runs and honored in viewing mode, or the iteration-1 false-finish P0 the interaction plan just specced returns through the side door. Pin with tests in both items. |
| Entity convo epochs | F5, UX-11, anything near `convo.rs` | Turn-epoch stale guards on every posted closure; the in-flight entities build owns the files — coordinate cycle ownership, don't cross-edit. |

**Honesty-discipline guardrails — any UI change violating one is refused in
review:**

1. No predictions rendered as status ("writing answer…" class) — receipts
   only.
2. No fabricated diff content: never render context lines or old bytes the
   client never had (NEW-5's rule governs all diff surfaces).
3. No client-shipped capability tables (context windows, pricing) — the
   2026-07-17 fabricated-selection class; render-when-present or render
   honest absence.
4. Truncation is always labeled (`#TRUNCATION`) — including exports; degraded
   paths warn (`#FALLBACK`), never silently substitute.
5. Absence of a receipt is a labeled state, never a zero (the splitless-usage
   lesson) — `/stats`, `/tree`, budgets all inherit it.
6. Tool/activity claims render from records and attributes, never from reply
   prose (marker-imitation lesson).
7. Bounded-zombie windows (≤75 s stream / ≤600 s entity turn) are documented
   accepted costs (`docs/architecture.md`) — any item changing the thread
   census updates that note.

---

## 9. Sign-off

Signing this roadmap sets in motion: Wave 1 (starting with the UX-01 reshape
alone), the GW-A gateway ask, and decisions D-1–D-3 as recorded. Waves 2–3
proceed in order behind their dependency gates; every item lands with its
acceptance criterion test-pinned; the parking lot re-opens only on its named
triggers.

- [ ] Maintainer sign-off
- [ ] D-1 (`@file`: hand-rolled walk, no new dependency) ratified
- [ ] D-2 (Ctrl+T stays theme; pager = `o` + `/inspect`) ratified
- [ ] GW-A ask approved for filing on agora
