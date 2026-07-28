# Lane A diagnosis — "the agent never finishes work" (2026-07-23)

Status: live evidence captured against the production gateway
(127.0.0.1:8080) on 2026-07-23 00:45–02:10. Every run id below is real and
still queryable (`GET /api/gateway/runs/{id}` / `…/history_bundle`).

> Cycle-2 adversarial review (same day) audited this lane's fixes and
> closed five follow-on holes (rehydrate discovery-order binding, exec
> cancel exit code, two fabricated-"no readable answer" drains, a
> caller-dependent unknown-status guard) — findings, refuted attacks,
> and the residual pre-cycle-failure gap live in
> `cycle2-review-conclusion.md`. Note for §1/exec: a CANCELLED
> answer-source subrun now exits 130 (was 0).
>
> **The residual pre-cycle-failure gap is SUPERSEDED (2026-07-23,
> conformance lane — see §1a below)**: the answer source now binds
> STRUCTURALLY from the parent's spawn records, so an agent child that
> dies before its first cycle is bound from birth and concludes. The
> cycle heuristic survives only as a labeled `#FALLBACK` for ledgers
> that predate the spawn-record declaration fields.

## Verdict: all three of (a)/(b)/(c) exist, each with a different owner

The maintainer's symptom ("runs never conclude, spinner forever, tokens
stuck at 0") decomposes into **three distinct populations** among the
gateway's recent runs, plus one token-display defect:

| # | Population | Class | Owner |
|---|---|---|---|
| 1 | Answer-source agent subrun **terminally failed**, root parks forever on the status poller | **(b) — client bug, fixed this lane** | us |
| 2 | Coder tree parked on a **deep-subrun tool approval** for hours | (c)-shaped but honest: the TUI DOES surface the approval (verified below); unattended = waits forever by design | nobody (durable-run semantics) |
| 3 | Wrapper roots stay `waiting` forever after the answer (eternal `ac-update-status` poller) | server-side design; the client already concludes from the agent subrun's answer | gateway (resource concern, see §5) |
| 4 | Tokens/ctx read 0 while a proxy provider reports **splitless normalized usage** — the REAL split sits in `result.raw_response.usage` | **(b) — client extraction gap, fixed this lane** | us |

## 1. The reproduced P0 (class b): failed agent subrun never concludes the turn

Live tree, session `acode-ptysmoke-1784707419` (created 2026-07-22 08:03):

- Root `76fc3fcb-d3ae-…` (`basic-agent@0.0.3:81795ea9`) — status **waiting**
  15+ hours later, parked on node-5 (the status-poller subflow).
- Agent subrun `9c5cad22-2a22-43fa-9047-9370b2f9b73f`
  (`visual_react_agent_basic-agent_0_0_3_81795ea9_node-2`) — status
  **failed**. Its whole ledger (3 records):

```
reason  started    llm_call    08:03:44  attempt: 1
reason  failed     llm_call    08:03:44  ERROR=LMStudio API error (400): {"error": "Model unloaded."}
reason  completed  emit_event  08:04:13
```

- Root ledger after the failure — the wrapper flow **absorbed** the agent
  failure and moved on to the eternal poller:

```
node-2  waiting    start_subworkflow  wait=subworkflow:9c5cad22-…   ← the agent
node-2  completed  resume                                            ← resumed PAST the failure
node-5  started    start_subworkflow                                 ← the status poller
node-5  waiting    start_subworkflow  wait=subworkflow:97b4dde0-…   ← waits forever
```

What the client did with this (pre-fix):

- `Fold::apply` pushed the error card for the failed record but flips
  `finished` **only when `rec_run == root_run_id`** — the agent subrun is
  not the root, so the turn never concluded.
- `runner::finish()` — the only place a run's **terminal status** is
  reported — began with `if !is_root { return; }`, so the agent stream's
  `done` event (the run IS terminal: failed) was swallowed silently.
- The root never terminates (poller), so the root-side conclusion never
  arrives either. Composer captured forever; tokens at 0 (the failure hit
  at cycle 1, before any usage receipt). **This is the maintainer's exact
  screenshot state.**

Three more trees in the same state on the box: `ab54a569…`/`1a8512db…`
(roots later cancelled by hand) and `4c05ab25…` under a 0.0.2 root.

Fix shipped (this lane): `Fold::subrun_terminal(run_id, status)` — when
the runner observes a **followed subrun** reach a terminal status, the
turn concludes iff that subrun is the bound ANSWER-SOURCE agent run.
Keyed on RUN status, never on failed records (effect failures retry and
absorb without killing the run — `_absorb_failure`, attempt N); helper
subruns and goal iterations (`finish_on_root_only`) never conclude from
there. `runner::finish()` now reports subrun terminals through it, and
exec's polling loop checks the answer-source run's status each sweep.

## 1a. Structural answer-source binding (2026-07-23 conformance lane — supersedes the pre-cycle residual)

The original binding was a **behavior heuristic**: `agent_run_id` bound
on a child's FIRST reason-cycle record, so an agent that died before
cycling never bound and the turn hung (the cycle-2 "residual"). The
maintainer's correction — *the ledger already knows; never guess* —
holds, verified against the runtime source and the live gateway:

- **The parent's own ledger declares the child's workflow at spawn.**
  The subworkflow wait record carries
  `result.wait.details.{sub_run_id, sub_workflow_id, wrap_as_tool_result}`
  and `effect.payload.workflow_id` (abstractruntime
  `core/runtime.py::_handle_start_subworkflow`, both sync and async wait
  shapes; live-verified on root `76fc3fcb…`: the agent child declares
  `visual_react_agent_basic-agent_0_0_3_81795ea9_node-2`, the helper
  children declare `basic-agent@0.0.3:15f19f7f`).
- **Agent workflows are recognizable from structure, two ways.**
  (1) The runtime's deterministic Agent-node id contract
  `visual_react_agent_{flow}_{node}`
  (`visualflow_compiler/visual/agent_ids.py` — the docstring pins the
  ids as stable across hosts for third-party clients). (2) The catalog:
  every `GET /bundles` entrypoint carries its run-facing `workflow_id`
  (`{bundle}@{version}:{flow}`, minted at `routes/gateway.py`), so the
  entrypoints tagged `abstractcode.agent.v1` form an id set —
  `Fold::set_agent_workflows`, wired by the runner at catalog load.
- **Tool-mode children are structurally excluded.** `delegate_agent`
  spawns run the parent's OWN workflow id (abstractagent
  `react_runtime.py`), so workflow-id matching alone would mis-bind a
  root-level agent's delegate — but the runtime stamps
  `wrap_as_tool_result` into the wait details for exactly this case: a
  tool observation by contract, never an answer source. Such children
  never bind, **even when they cycle** (a structural fact beats
  behavior; this also fixes a pre-existing mis-bind the cycle heuristic
  had for cycling root-delegates).
- **Binding rule** (`Fold::bind_agent_run`): a child spawned BY THE ROOT
  whose declared workflow is agent-shaped binds at the spawn record —
  before any cycle. First-wins on normal runs; goal trees
  (`finish_on_root_only`) follow the newest iteration's spawn. Deep
  spawns (parent ≠ root) never bind — the coder tree's depth-2/3
  verifier agents stay excluded exactly as before.
- **The cycle heuristic is demoted to a labeled `#FALLBACK`** for spawn
  records that predate the declaration fields (it can no longer adopt a
  ledger-declared tool child).
- **Answer-record eligibility hardened the same way**
  (`protocol::is_run_output_record`): a SYNC subworkflow's completion
  record carries the CHILD's output on the parent's ledger
  (`result = {sub_run_id, output}`) and no longer reads as the parent's
  final answer; the run's own end is the runtime's terminal marker
  (`result.completed == true`, `_append_completion_record` — every live
  flow end on this gateway carries it; marker-less completed+output
  records stay accepted as a labeled `#FALLBACK` for pre-marker
  ledgers).

Regression pins: `tests/failed_agent_subrun.rs::`
`agent_dying_before_its_first_cycle_still_concludes` (the agent child's
ledger is EMPTY — zero records — and the turn still concludes from
`subrun_terminal` because the ROOT's spawn declaration alone bound it),
plus the transcript unit tests
(`spawn_declared_agent_binds_before_any_cycle…`,
`tool_mode_children_never_bind…`, `deep_agent_spawns_never_bind…`,
`catalog_declared_agent_workflow_binds…`,
`goal_spawn_binding_follows_the_live_iteration`,
`sync_subworkflow_completion_output_is_not_the_answer`).

## 2. The 12h coder run (class c: parked on an approval — and the TUI shows it)

Live tree, session `acode-05452bd6bd3c` (the maintainer's Jul-22 coder
run, ~5h of work then 12h parked):

```
coder            5f810f81  waiting  ← root,   waits on ↓ since 12:45
coding-agent     1a831cd4  waiting  ← level 1, waits on ↓
coding-verify    7f5ee18d  waiting  ← level 2, waits on ↓
verifier (agent) b67e2341  waiting  ← level 3: wait reason=user,
                                       tool_approval:eda4d01b…
                                       execute_command "cargo test --test headless_ui"
```

The run is **genuinely open server-side**: a tool approval nobody
answered, four levels deep. Verified with the release binary
(`scripts/pty_reattach_probe.py`, new): reattaching to that session
re-follows the tree transitively and **re-surfaces the approval** (tool
card `? execute_command … awaiting approval` renders). So for this
population the client is honest today; the run concludes the moment the
operator answers. Earlier approvals in the same tree show multi-hour
approval dwell gaps (12:46 → 15:42), matching an operator who had walked
away.

For comparison, the COMPLETED coder run `b7d86e08…` (Jul-21) proves the
coder shape concludes correctly when unblocked: its root `end` node
carries `result.output = {report: "# Coding agent result…", passed,
delivered, …}` and the fold's `report` fallback (shipped 2026-07-22)
reads it as the final answer.

Note the coder tree's cycling runs are at depth 2–3 (parents ≠ root), so
the `agent_run_id` answer-source lane deliberately never binds — answers
come from the root's own flow end. Correct for finish; it starved the
telemetry lane (see §4). (Still true under §1a's structural binding: the
coder root's only first-level child declares
`coding-agent@0.2.4:coding-agent`, which carries `abstractcode.coding.v1`
— not the agent interface — so nothing binds and the root stays the
answer source, now by declaration instead of by absence of cycles.)

## 3. Wrapper roots never terminate server-side (documented, not ours to fix)

`basic-agent@0.0.2/0.0.3` roots stay `waiting` forever after the agent
answers: node-5 starts a status-poller subflow that loops
`wait_until` (~1.5s period) with no exit edge. Live counts on the box at
02:00: **44 waiting basic-agent runs** (19 of them roots), the oldest
poller (`3b12912e…`, born Jul-21 13:07) at **54,918 ledger records and
still appending ~6 records/12s after 30+ hours**. My own pty-smoke run
from tonight (`1a632fa7…`) reached its answer correctly (turn concluded,
answer card rendered — the fold's agent-subrun answer path works) and
then joined this zombie population: root waiting, poller ticking.

Client handling is already correct (conclude on the agent's answer,
`StopFollows` the helpers, root stream keeps observing). The unbounded
poller ledgers are a **gateway/bundle-side** resource leak — flagged
here as evidence, not fixed by this lane (basic-agent@0.0.4 material:
the poller needs an exit edge or a bounded loop).

## 4. Tokens stuck at 0 / dead ctx (class b: splitless normalized usage with a raw split on the record)

The maintainer's 5h builder run `0312b41d…` (gpt-5.6-sol via a local
proxy, 127.0.0.1:8317): every completed `llm_call`'s **normalized** usage
is splitless —

```json
"usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 3180,
           "prompt_tokens": 0, "completion_tokens": 0}
```

— while the SAME record's `result.raw_response.usage` carries the real
split (provider-reported, Responses-API shape):

```json
{"input_tokens": 2904,
 "input_tokens_details": {"cache_write_tokens": 0, "cached_tokens": 0},
 "output_tokens": 276,
 "output_tokens_details": {"reasoning_tokens": 69},
 "total_tokens": 3180}
```

Tree totals for the stuck coder run: 77 llm calls, normalized
input=0 / output=0 / **total=3,937,440**. The strip's honest-total
fallback (shipped earlier) showed "N tk total", but in/out stayed 0 and
`last_input_tokens` (the ctx meter) never moved — "context not updating".

Fix shipped (this lane), general logic driven by the record contract:

- `protocol::usage_from_record` — when the normalized usage parses
  splitless (input==0 && output==0), read the split from
  `result.raw_response.usage` (object or JSON-string body; `$slim`'d /
  absent raw stays splitless-honest). Never fabricates: numbers come
  from the provider's own usage block on the same ledger record.
- `parse_usage` — nested `input_tokens_details.cached_tokens` joins the
  cached-token key chain (the Responses-API spelling above).
- Telemetry lane: ctx/model previously updated only from root or the
  first-level agent run; coder trees have NO first-level cycling run, so
  ctx/model stayed dead for 5 hours. Now, when no first-level agent is
  bound, the **currently cycling run** feeds ctx/model (the delegate-
  pollution rule is preserved: once a first-level agent exists, deeper
  children still never relabel).
- `result.gen_time` (milliseconds, abstractcore contract) + the repaired
  output split now feed `Fold::last_call_rate()` (tok/s of the last
  completed call) and `Fold::live_llm_call()` (started-at epoch ms of
  the in-flight call, from the record's own `started_at`; no ts → None)
  — the OBS-1a-live data half Lane B renders.
  [SUPERSEDED cycle-3: Lane B shipped on the client-clock twins instead;
  this fold pair never gained a consumer and was removed (cycle-2
  presence review P2-H). The parsers survive in `protocol::…`.]

## 5. Gateway-side findings (documented for the owning seats, NOT fixed here)

1. **Unbounded status-poller subflows** (basic-agent@0.0.2/0.0.3): runs
   like `3b12912e-ad10-41cb-b78f-8b61b1dabbb9` accumulate >54k ledger
   records at ~4 records/6s with no termination path, times 25+ live
   pollers on this box. Ledger storage + tick scheduling burn forever.
2. **The `visual_react_agent_…` agent subrun failing does not fail the
   wrapper root**: the wrapper resumes past it onto the poller (root
   `waiting` forever, output null). A root that can never produce output
   nor terminate is arguably a bundle bug (basic-agent lane).
3. `GET /runs/{id}/ledger?after=N` answers 422 when `after` exceeds the
   ledger head (observed while probing `1184e6a8…` with after=999999) —
   harmless for our cursors (monotonic from 0) but a sharp edge for
   clients that probe.

## What concludes correctly today (verified live tonight)

- basic-agent, happy path: pty smoke PASS end-to-end (approval → answer
  card → composer freed → file on disk) — release binary, live gateway;
  re-run PASS after the fixes.
- coder, happy path: root flow-end `{report,…}` output → final answer
  (fixture `coder_run_tree.json` + live run `b7d86e08…`).
- basic-agent, failed-agent path: **concludes after this lane's fix** —
  verified THREE ways: fixture replay (`tests/failed_agent_subrun.rs`,
  records captured from the live ledgers of `76fc3fcb…`/`9c5cad22…`),
  unit tests on `Fold::subrun_terminal`, and LIVE
  (`scripts/pty_failed_agent_verify.py` reattached the release binary to
  the real stuck session `acode-ptysmoke-1784707419`: reattach notice ✓,
  provider error card "Model unloaded" ✓, conclusion card "the agent run
  ended: failed" ✓ — previously it spun forever).
- coder, parked-on-approval: honestly open; approval re-surfaces on
  reattach (verified live against session `acode-05452bd6bd3c` —
  `scripts/pty_reattach_probe.py`).
- exec: happy path exits 0 with correct token line (`1979↑ 30↓ tk`,
  live run `df9cbfb0…`); `fold.failed` → exit 1 now also covers the
  failed-agent class (the loop polls `answer_run_id()` each sweep);
  never-concluding runs still hit the `--timeout` deadline → 124.
