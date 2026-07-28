# Lane 3 — Realtime monitoring · observability · new functionality

Status: research findings for roadmap sign-off (2026-07-22). READ-ONLY lane:
no code was written. Evidence comes from this crate's shipped v0.3.0 code
(`src/transcript.rs`, `src/protocol.rs`, `src/runner.rs`, `src/ui/chrome.rs`),
the gateway source at `abstractgateway/src/abstractgateway/routes/`
(`gateway.py`, `entities.py`, `entity_replay.py`), live read-only probes of
the gateway at 127.0.0.1:8080 (probed 16:29–16:35Z), and the reference tools
(codex-rs `otel` + `memory-observer`, opencode `packages/stats`, pi
`packages/orchestrator`, `abstractobserver`, the Python `abstractcode`).

Already-planned work is NOT re-proposed here. The three standing plans this
document builds on: `docs/design/plan-interaction-model.md` (queue,
Ctrl+J, /goal client, **serve subcommand**), `docs/design/plan-entities-mcp.md`
(@name visits, /entities, chips+poller, **v1.5 entity life feed**, MCP v1
polish + gateway asks #1–#5), `docs/design/tier-policy-agora-facts.md`
(tier/approval passthrough). Where a finding touches one, it says so.

Legend per finding: **Value** (why the operator cares) · **Evidence**
(data verified available / reference pattern) · **Feature** (concrete
shape) · **Effort** S/M/L · **Dependency** (client-only vs gateway ask).

---

## Ranked summary

Two families. "OBS" = observability of existing runs (surface data the
client already receives or can GET today). "NEW" = net-new capability.

| # | Finding | Family | Effort | Dependency |
|---|---------|--------|--------|------------|
| 1 | OBS-1 Step timings + call economics truth (`duration_ms`, `gen_time`, tok/s, `finish_reason`, attempt) | OBS | S | client-only |
| 2 | OBS-2 Live subrun tree (`/tree`) — where the work and the tokens actually are | OBS | M | client-only |
| 3 | OBS-3 Gateway activity board (`/runs`) — every run this gateway is executing, stuck runs included | OBS | M | client-only |
| 4 | NEW-1 Entity live feed (`/watch <name>`) over `/replay/stream` SSE | NEW | M | client-only (endpoint live) |
| 5 | OBS-4 Session run-history browser — reopen any past turn's full transcript | OBS | S/M | client-only |
| 6 | NEW-2 Run summary + "chat with the run" (`/summary`, ledger-grounded Q&A) | NEW | S/M | client-only (endpoints live) |
| 7 | OBS-5 Artifact browser + non-image artifacts (`/artifacts`, save-to-disk) | OBS | M | client-only |
| 8 | NEW-3 Transcript export (`/export` markdown/JSON) | NEW | S | client-only |
| 9 | NEW-4 Token budgets + limit warnings (client thresholds + `_limits` passthrough) | NEW | S | client-only (dollar pricing would need a table — see honesty note) |
| 10 | OBS-6 Gateway-host GPU meter (`/gpu`) | OBS | S | client-only (endpoint live) |
| 11 | NEW-5 Files-changed summary per run (+ diff-aware tool cards) | NEW | M | client-only |
| 12 | NEW-6 `@file` mentions from the workspace root | NEW | M | client-only (local-gateway posture) |
| 13 | OBS-7 Wait/schedule visibility (wait_until deadlines, scheduled runs) | OBS | S | client-only |
| 14 | Noted, not recommended now: OTEL export, audit-tail view, checkpoints/undo, /plan preset | — | — | see Part D |

The biggest structural fact behind the whole lane: **the ledger already
carries nearly everything** (`started_at`/`ended_at` on every record,
`gen_time`/`finish_reason`/`model`/`usage`/`trace_id` on every completed
llm_call, `attempt` on every step, the full subrun topology through
subworkflow waits) — and the fold throws most of it away. Rows 1–3 are
"stop discarding", not "go fetch".

---

## Part A — Observability of existing runs

### OBS-1 · Step timings + call economics truth (rank 1)

**Value.** The operator's long runs (a live coding-agent run today spans
5h+) currently report cumulative tokens, a cycle count, and one elapsed
number. Nothing answers: how long does each model call take, is the
provider degrading over the run, what's the tok/s, did that answer stop
because the model was DONE or because it hit max_tokens, did a call
silently retry? These are the questions the operator actually asks when a
run feels wrong — and they are also this app's next trust increment (the
"0↑ 0↓ tk for five hours" splitless-usage bug fixed today was exactly a
call-economics blind spot).

**Evidence (all verified live).**
- Every ledger record carries `started_at`/`ended_at` (RFC3339) — the TUI
  already parses this format (`protocol::parse_rfc3339_utc`) but uses it
  only for reattach back-dating.
- Completed `llm_call` results carry `gen_time` (generation time in **ms**,
  AbstractCore `GenerateResponse.gen_time`), `finish_reason`, `model`,
  `usage`, `trace_id` — live record from run `0312b41d…` shows all of them;
  the fold reads only `usage` + `model`.
- Every record carries `attempt` (1-indexed, `StepRecord.attempt`) — a
  RetryManager resample is visible in the ledger and invisible in the TUI.
- `history_bundle.timeline` (verified: 485 entries for today's coding run)
  already serves flat per-step `duration_ms` across the WHOLE run tree —
  server-side confirmation this data is considered render-worthy.
- Reference: codex `codex-rs/otel` instruments exactly this set —
  `codex.api_request` with `duration_ms`, `http.response.status_code`,
  `attempt`; per-SSE-event durations; `codex.tool_result` with duration and
  success. We can render the same truths with zero new transport because
  our substrate (the durable ledger) already records them.

**Feature.**
- Tool cards gain a duration suffix (`✓ write_file · 0.3s`); thinking
  cards gain per-cycle latency + tok/s (`cycle 4 · 12.5s · 41 tok/s` from
  `gen_time` + output tokens).
- The activity strip's existing slow-call hint upgrades from wall-clock
  only to "model call 3m — 12 tok/s" when the previous calls establish a
  rate.
- `finish_reason != "stop"` renders an honest label on the answer/cycle
  ("answer cut by token limit (finish_reason=length)") — today a truncated
  answer renders as a complete one. This is a straight trust defect.
- `attempt > 1` renders "retried ×N" on the affected card.
- A `/stats` command (or extending `/cache`) shows the run breakdown:
  time in model vs tools vs waiting-on-user, slowest calls, per-model
  split — all foldable from records already in `Fold::apply`'s hands.

**Effort.** S (fold fields + render). **Dependency.** Client-only.

### OBS-2 · Live subrun tree — `/tree` (rank 2)

**Value.** Wrapper bundles run the real work in subruns (basic-agent:
agent loop + status pollers; coding-agent today: 11 runs — builder,
verifier, snapshot helpers). The fold FOLLOWS them all (one SSE each) and
knows every parent edge, but renders a flat interleaved transcript. When
the strip said "Done · cycle 12 · 17880s" while the tree kept working
(live incident, fixed by clearing terminal-sounding statuses), the honest
fix was defensive; the structural fix is showing the tree. Delegate-child
"pollution" bugs (wrong model label, wrong ctx) were all symptoms of
tree-blindness — the class dies when the tree is visible.

**Evidence.**
- `Fold.parents: HashMap<String, String>` + `followed` already hold the
  topology; `cycles` is per-run; usage deltas arrive tagged with
  `rec_run`. Nothing new needs fetching — per-run aggregation is a fold
  delta.
- Live tree today: root `5f810f81` (coding-agent) → 11 runs in the
  bundle's ledgers; its timeline holds 77 completed llm_calls and 85
  completed tool batches.
- Reference: pi `packages/orchestrator` is a supervisor with
  spawn/list/status over child agents — the industry shape for "show me my
  agents as a tree, not a log". Claude-code renders subagent activity as
  nested cards for the same reason.

**Feature.** `/tree` modal (and a strip chip `· 3 subruns active`):
one row per followed run — indented by parent, workflow/node label,
status glyph, cycles, tokens (per-run fold), elapsed, last activity.
Selecting a row could later filter the transcript to that run's cards
(v2; the modal alone is v1). Also fixes a scaling honesty point: with
per-run rows, per-run SSE health ("stream reconnecting") gets a place to
render.

**Effort.** M (per-run stats fold + modal; the data plumbing exists).
**Dependency.** Client-only. (If per-subrun SSE fanout ever hurts, the
gateway's `POST /runs/ledger/batch` exists precisely to absorb observer
fanout — an implementation option, not an ask.)

### OBS-3 · Gateway activity board — `/runs` (rank 3)

**Value.** The gateway is multi-client: the live probe found a
coding-agent root WAITING since 11:00Z and three basic-agent roots waiting
for hours — none visible from this TUI. The operator's recurring failure
mode (recorded across AGENTS.md incidents) is *background runs silently
spending or silently stuck*. Today the only console is the web observer;
the terminal operator flies blind. An activity board is also the natural
observer half of the fleet story: when `serve`/bridge seats (interaction
plan item 4) run headless through this same gateway, this board is where a
human watches the fleet.

**Evidence.**
- `GET /api/gateway/runs?limit&status&workflow_id&session_id&root_only`
  serves status, workflow, session, timestamps, `waiting.reason` (+
  `until`), paused flags, `ledger_len` (live-verified shape).
  `include_metrics=true` exists but returned `metrics: null` on the live
  index path — treat metrics as best-effort, render-when-present (same
  contract as tier fields).
- The web observer's Runtime Activity view consumes exactly this route
  (abstractobserver `docs/api.md`), so the render semantics are already
  established framework-side.
- Reattach machinery (`probe_attach`, `rehydrate_run_into`) already
  exists to act on a selected run.

**Feature.** `/runs` modal: recent runs across the gateway (default
root-only), each row `status · workflow · session · age · waiting-reason`;
Enter adopts a run of the CURRENT session (existing attach path); `c`
cancels a stuck run (existing durable command) with confirmation;
`s` switches to that run's session. A count chip when N runs are
running/waiting gateway-wide is optional polish. Honesty rules: loaded-
scope counts only (the observer's own caveat), never "all time".

**Effort.** M. **Dependency.** Client-only.

### OBS-4 · Session run-history browser (rank 5)

**Value.** `/sessions` today lists locally-remembered session ids with a
one-line label; there is no way to see what happened in a previous turn or
a previous session without the web observer. The operator habitually
re-opens old runs to extract receipts (commit messages, agora posts).

**Evidence.**
- `history_bundle?include_session=true` serves `session.turns` — ordered
  root runs with `run_id`, `prompt`, `status`, `kind`, timestamps
  (live-verified on today's coding session).
- `runner::rehydrate_run_into` already folds one prior run tree into a
  full-detail transcript — it runs today on reattach; pointing it at ANY
  chosen past run is the same code path.

**Feature.** Extend `/sessions` (or add `/history`): pick a session →
list its turns (prompt preview · status · when) → Enter rehydrates that
run's full transcript read-only (banner: "viewing a past run — /new to
leave"). "Compare" between two runs is deliberately NOT proposed (low
value/effort ratio in a TUI; export + diff outside covers it).

**Effort.** S/M (list UI; rehydration exists). **Dependency.** Client-only.

### OBS-5 · Artifact browser + non-image artifacts (rank 7)

**Value.** Runs produce more than images: generated files, audio/voice,
markdown reports, JSON. The TUI fetches ONLY images and only when an
event/final names them; everything else is invisible (at best a path in a
tool result preview). The operator gets artifacts out today by switching
to the web observer or the filesystem.

**Evidence.**
- `GET /runs/{id}/artifacts`, `GET /sessions/{id}/artifacts`, and the
  full `GET /artifacts/search` (modality, `semantic_kind`, `render_kind`,
  tags, sizes, provenance run/workflow/node, `include_stats` facets,
  pagination) are all live. Content route supports
  `access_action=preview|download|content` labeling (the observer labels
  access type so server-side access stats stay meaningful — we should too).
- The fold's `FetchImage` effect + `Item::Image` mosaic path is the
  pattern to generalize; `artifact_bytes` already exists in the client.

**Feature.** `/artifacts` modal (current run + session scope): rows
`kind · name/label · size · when`; Enter previews (text/markdown inline in
a details card; image via the existing mosaic; other kinds show metadata);
`s` saves to a local file (bytes via the existing content route, honest
size cap + `#TRUNCATION`-free full download to disk). Transcript addition:
when a final answer's meta carries non-image artifacts, render an artifact
card (name + kind + "open with /artifacts") instead of dropping it.

**Effort.** M. **Dependency.** Client-only.

### OBS-6 · Gateway-host GPU meter — `/gpu` (rank 10)

**Value.** On a local-inference gateway (this deployment: LMStudio/MLX on
the same box), "is the model actually computing?" is THE question during a
slow call — the recorded incident class ("MLX 27B at ~0.25 tok/s looked
idle") got a wall-clock hint; GPU utilization is direct evidence. The
Python abstractcode ships a `/gpu` toggle for exactly this; the Rust TUI
lost it in the port.

**Evidence.** `GET /api/gateway/host/metrics/gpu` live-verified:
`{"supported": true, "source": "ioreg", "utilization_gpu_pct": 28.0,
"gpus": [{"name": "Apple M5 Max", …}]}`.

**Feature.** `/gpu` toggles a status-bar meter (`gpu 28%`), polled every
few seconds ONLY while a run/turn is active (idle frames stay 0-cost —
the engine's idle contract). `supported: false` renders once, honestly.

**Effort.** S. **Dependency.** Client-only.

### OBS-7 · Wait/schedule visibility (rank 13)

**Value.** Runs that sleep (`wait_until`), scheduled runs, and paused
runs are silent in the transcript — a goal-loop run pacing itself with
wait_until (the /goal bundle shape) will look hung.

**Evidence.** Ledger `wait_until` effects carry the deadline;
`/runs/{id}` and the runs list serve `waiting: {reason, until}` and
schedule metadata (live-verified fields).

**Feature.** Fold `wait_until` records into the strip ("sleeping until
HH:MM — resumes itself") and render schedule metadata in OBS-3's rows.
Composes with the /goal client half (a pacing goal run should say so).

**Effort.** S. **Dependency.** Client-only.

---

## Part B — New capabilities

### NEW-1 · Entity live feed — `/watch <name>` (rank 4)

**Value.** The operator repeatedly asks "what is this entity doing right
now?" — today the answer lives in the separate entity web app. The
entities plan ships chips + a 7s/30s poller (status, spend deltas) and
names a **v1.5 per-entity life feed** without specifying it. This finding
is that specification, so the roadmap can price it.

**Evidence.**
- `GET /api/gateway/entities/{name}/replay/stream` is live SSE: `id:` =
  journal seq (float — host markers ride fractional seqs), `Last-Event-ID`
  resumes exactly, the stream NEVER terminates (a life has no terminal
  state) — contract verified in `routes/entity_replay.py`.
- `GET /entities/{name}/cognition` (live, 1.7s warm) already serves
  phase/liveness/spend/drives — the poller half is planned; the feed is
  the streaming half.
- Reference: `abstractobserver`'s entity view established the render
  honesty rules for exactly this stream — envelopes fold client-side,
  diary display blocks arrive REDACTED from the engine and are never
  reconstructed, origin labels distinguish own-time/work-time, liveness
  badges show wall-time-since-last-envelope (EventSource ignores
  keep-alives by design), tool claims render from attributes/host events
  only, never from reply prose.
- codex's `memory-observer` is the same product shape for codex memory
  (graph snapshot + live event tail with bounded limits) — both ecosystems
  converged on "memory needs a live observer".

**Feature.** v1.5 (post entities-plan v1): under `Focus::Entity(name)` a
details-gated live lane — one line per replay envelope ("formed episode ·
'…' ", "recalled 3 · committed 2", "diary (private — redacted)", "felt
person:laurent +2", host markers for summon/close/sleep) with a
wall-time-since-last-envelope badge. One dedicated never-ending stream
thread per WATCHED entity (not per open convo), stale-guarded like turn
threads; `Last-Event-ID` resume on reconnect. Render rules imported from
the observer verbatim (redaction honored; seq is a float; kind colors
degrade gray on unknown kinds).

**Effort.** M (SSE thread + envelope fold + lane render). **Dependency.**
Client-only — the endpoint is live. (Mid-TURN progress for visit runs
remains BLOCKED on gateway ask #2 from the entities plan — the replay
stream shows memory acts, not the turn's tool-by-tool progress; do not
conflate the two.)

### NEW-2 · Run summary + "chat with the run" (rank 6)

**Value.** After a 5-hour coding run, "what did it actually do?" requires
scrolling hundreds of cards. The gateway can already answer in prose,
grounded in the durable ledger — files touched, commands run, web
lookups, errors, per-run digests.

**Evidence.** `POST /runs/{id}/summary` (generates + persists a summary
as an `abstract.summary` ledger event; `include_subruns` walks the tree)
and `POST /runs/{id}/chat` (read-only Q&A grounded in the ledgers) exist
in `gateway.py` (8203/8295) with a rich server-side digest extractor
(steps, llm/tool counts, unique tools, tokens, files, commands, web,
errors). The web observer already consumes both. Not probed live (they
spend LLM tokens); source-verified.

**Feature.** `/summary` (current or last run; renders as an Info/markdown
card and — since the gateway persists it as a ledger event — it is
durable and re-renderable); `/ask-run <question>` for follow-ups. Both
label the spend ("summary generation used the gateway's default route").

**Effort.** S/M. **Dependency.** Client-only (endpoints exist; they cost
tokens — label, never auto-fire).

### NEW-3 · Transcript export — `/export` (rank 8)

**Value.** The operator's culture is receipts: agora posts, commit
messages, and reviews constantly quote run transcripts. Today that means
manual copy out of the terminal (or the web observer). Every reference
tool ships an export (opencode `/export`, codex session files).

**Evidence.** The fold holds the full item list; `history_bundle` serves
the lossless tree for the durable version; `Item` variants map 1:1 onto a
markdown structure (user/steer/thinking/tool/answer with status glyphs).

**Feature.** `/export [md|json] [path]` — markdown renders the
transcript with tool cards (+ OBS-1 timings when present) and an honest
header (session, run ids, workflow, model, token totals); json writes the
`history_bundle` verbatim (the replayable form). Default path
`./transcript-<runid8>.md`, collision-safe.

**Effort.** S. **Dependency.** Client-only.

### NEW-4 · Token budgets + limit warnings (rank 9)

**Value.** Long-running/queued/goal runs spend unattended. The operator
has no "tell me at 500k tokens" guard; the gateway's own runs have no
dollar meter. (The recorded cost-discipline rulings — own-time loops
off by default, "an unattended loop spends real tokens" — show budget
visibility is a maintainer value, not a nice-to-have.)

**Evidence.**
- Client side: `Fold.session`/`stats` already fold cumulative tokens —
  thresholds are a strip/notice delta.
- Runtime side: `_limits` run vars (`max_tokens`, `max_iterations`,
  `warn_*_pct`) + `Runtime.check_limits()` → `LimitWarning` exist
  (hybrid model: runtime warns, nodes enforce). The client can pass
  `_limits` in `input_data` today.
- Dollar costs: opencode's stats stack prices per-model
  (input/output/reasoning/cache_read microcents; models.dev table). Our
  registry (`model_capabilities.json`) carries **no pricing fields**
  (verified) — an honest dollar meter needs a pricing table (client-shipped
  or a gateway ask). On this mostly-local deployment cost ≈ tokens + time,
  so tokens-first is the right v1.

**Feature.** `/budget <n>tk [session|run]` — threshold warnings on the
strip (never auto-cancel; the queue's pause-on-failure is the precedent
for "warn, don't act"). Optionally pass `--max-tokens/--max-iterations`
through `_limits` at start so the SERVER side warns too. Dollar estimates:
defer; if wanted, a small static pricing map labeled "estimate".

**Effort.** S (client thresholds) — the `_limits` passthrough is a
run_input one-liner. **Dependency.** Client-only.

### NEW-5 · Files-changed summary + diff-aware tool cards (rank 11)

**Value.** For coding runs, the outcome IS the file delta. Today the
operator reconstructs it from write_file/edit tool cards with 700-char
previews. codex/claude-code both treat file changes as first-class
(diff views, changed-file lists).

**Evidence.** Tool args in the ledger carry paths (and contents) for the
workspace tools; the gateway's own digest extractor derives a `files`
list server-side (precedent for the fold doing the same client-side).
Slimmed terminal records (`$slim`) mean the STARTED record is the
reliable args source — the fold already sees both.

**Feature.** Fold a per-run `files_touched` set (tool name + path +
op class); `/files` lists it; the final answer card appends "N files
changed". Diff RENDERING only where a tool result already carries a diff
(render fenced diffs with the theme's ok/danger inks — the Python app's
semantic-palette lesson); client-side diff COMPUTATION (old vs new) is
deliberately out — the client never has the old bytes.

**Effort.** M. **Dependency.** Client-only.

### NEW-6 · `@file` mentions from the workspace root (rank 12)

**Value.** codex/claude/cursor all ship @-file mentions; prompts that
name exact paths ground the agent and skip a list_dir round-trip. The '@'
completion infrastructure shipped TODAY for entities — a second provider
on the same trigger is cheap.

**Evidence.** `mention.rs` + the multi-trigger `Completion` (engine
supports N providers per trigger vec); `/workspace` knows the workspace
root client-side. On this deployment gateway and client share a
filesystem, so a local walk under the root is truthful.

**Feature.** '@' completion offers `@path/to/file` candidates (bounded
local walk under the workspace root, gitignore-aware, cached per draft
session) when the token contains a `/` or matches no entity — entity
names keep priority. Inserts the path as plain text (the agent reads it
with its own tools; no client-side file reading into the prompt in v1).
HONEST LIMIT: when the gateway is remote, local paths may not exist
server-side — gate the provider on a "workspace is local" heuristic or a
config flag, and say so in /help.

**Effort.** M. **Dependency.** Client-only (local-gateway posture).

### The serve/bridge fleet seat — assessed, already planned

The prompt asks whether this app should BE a headless collaborative fleet
seat. Assessment: yes, and the interaction plan (item 4) already specifies
it to implementation depth (JSONL protocol-v1 parity so the Python
`abstractcode bridge --executable` drives this binary; event schema pruned
against the real consumer; batch-approval mapping; the `fold.failed`
exit-code fix). Nothing to add on the EXECUTION half. What this lane adds
is the OBSERVATION half the plan doesn't cover: when N seats run through
this gateway, OBS-3's activity board is the human's fleet console
(runs by session/actor, stuck seats, spend), and pi's orchestrator
(spawn/list/status/rpc supervisor) is the reference for the eventual
"seat manager" — a `--fleet` view is NOT worth building before the serve
subcommand exists and real fleets run. Recommendation: ship serve per the
plan; treat OBS-3 as its observer; revisit a dedicated fleet panel after
first real fleet use.

---

## Part C — Trust + honesty: where observability prevents being misled

The app's discipline is "render honesty from receipts, never prose".
These are the places where MISSING observability currently lets a wrong
impression stand:

1. **Truncated answers read as complete** — `finish_reason` is in every
   completed llm_call result and never rendered (OBS-1). A model stopped
   by max_tokens today produces a confident-looking final card.
2. **Silent retries** — `attempt` is recorded per step; a provider
   flapping through RetryManager resamples is invisible (OBS-1). codex
   logs attempt on every api_request for this reason.
3. **Usage provenance** — today's token-totals bug (splitless usage
   folding to "0↑ 0↓") was fixed by folding `total_tokens`; the general
   rule worth encoding in every future stats surface: *absence of a
   receipt is a labeled state, never a zero*. The strip already does this
   ("N tk total" vs the false split); OBS-1's `/stats` must carry the
   same provenance labels ("provider reports no input/output split",
   "cache hits not reported by this provider").
4. **Tree-blindness enables fabrication to pass** — a model claiming "I
   ran the tests" is checkable only against tool receipts; OBS-2's tree +
   OBS-1's timeline make "what actually executed, when, in which subrun"
   one glance instead of a scrollback hunt. (The fold already refuses to
   parse prose for tool claims — the marker-imitation lesson; the tree is
   the affirmative half.)
5. **Background spend** — OBS-3 catches orphaned/waiting runs (live
   evidence today: 4 multi-hour waiting roots) and NEW-4 puts a number on
   unattended spend before it surprises.
6. **Entity feeds must keep the observer's honesty rules** (NEW-1):
   diary redaction rendered as redaction, origin labels ("your own time")
   verbatim from the engine, liveness = wall-time since last envelope,
   never connection state.
7. **`include_metrics` and tier fields are render-when-present** — both
   verified absent/null on some live paths; every new surface built on
   them needs the #FALLBACK posture the tier work already established.

---

## Part D — Noted, deliberately not recommended now

- **OTEL exporter from the TUI** (codex parity): our telemetry substrate
  is the gateway's durable ledger, which outlives any client and already
  feeds every finding above. Client-side OTLP would instrument the
  wrong end. If fleet-scale metrics are ever wanted, the exporter belongs
  gateway-side (it sees every run) — file it there when a consumer exists.
- **Audit-tail view** (`GET /audit/tail`, live-verified): an HTTP request
  log (ts/method/path/status/duration_ms). Ops-useful, but `doctor` and
  the web observer cover it; a TUI pane would be noise. Fold a one-line
  gateway-latency readout into `doctor` at most.
- **Checkpoints / undo** (codex parity): needs workspace snapshot
  machinery that does not exist runtime-side; a client cannot fake it
  honestly. If wanted, it is a runtime/gateway ask, not a TUI feature.
- **/plan preset**: the read tier (tier-policy work) already provides the
  permission half of plan mode; a labeled preset is a one-liner whenever
  the interaction lane wants it — not an observability finding.
- **Run comparison view**: cost/benefit poor in a TUI; NEW-3 export +
  external diff covers the need.
- **MCP/skills management**: already planned (entities plan item 4 + its
  gateway ask #4); nothing new found beyond it.

---

## Appendix — evidence inventory

Gateway endpoints verified live this session (read-only, bearer principal
`agw_…`): `/ping`, `/runs` (+filters, `include_metrics` → null on index
path), `/runs/{id}/ledger` (records carry `started_at`/`ended_at`/
`attempt`/`step_id`/hash-chain fields; completed llm_call results carry
`content/reasoning/model/usage/gen_time/finish_reason/trace_id/metadata`
incl. `_provider_request` URL), `/runs/{id}/history_bundle` (top keys:
`input_data`, `ledgers` (paged per-run, 11 runs on today's coding tree),
`timeline` (485 flat entries with `duration_ms` — 77 completed llm_calls,
85 completed tool batches), `resolved_actions`, `session.turns`,
`workflow_snapshot`),
`/host/metrics/gpu` (Apple M5 Max, 28%), `/audit/tail` (request log),
`/entities` (5 entities, drives ratios), `/entities/castor/cognition`
(phase/spend/drives/personal). Source-verified (not fired): `POST
/runs/{id}/summary`, `POST /runs/{id}/chat`, `POST /runs/ledger/batch`,
`/artifacts/search|stats` (+ per-run/per-session lists, export,
access_action), `/entities/{name}/replay/stream` (SSE, `id:`=seq,
`Last-Event-ID`), entity `/tasks`, `/footprint`, `/communities`,
`/life_state`.

References mined: codex-rs `otel` (api_request duration/status/attempt,
per-SSE-event timing, tool_decision/tool_result events,
auto_compact_token_limit) and `memory-observer` (graph snapshot + bounded
live event tail over the memory store); opencode `packages/stats`
(token model incl. reasoning + cache_read tokens, per-model cost in
microcents — pricing-table pattern); pi `packages/orchestrator`
(supervisor spawn/list/status/rpc over child agents); `abstractobserver`
(the maximal gateway client: runs board, artifact search consumption,
access_action labeling, run summary/chat consumption, entity render
honesty rules); Python `abstractcode` (`/gpu` meter, bridge/serve fleet
lane, diff palette lesson).
