# abstractcode vs abstractcode — why one iterates and the other stops

**Date:** 2026-07-30 · **Method:** three adversarial fable5 lanes (iteration/persistence,
workflow conformance, context delta) + live probing of the running gateway at
`127.0.0.1:8080` + wire inspection of 39 run-state files.
**Repos:** `abstractcode` (Python, local loop) vs `abstractcode` @ `6c0c8c8` (Rust, gateway client).

---

## The one-paragraph story

**abstractcode is not a worse coder. It is a client that never asked for the two
things that make abstractcode iterate.** Both clients run *the same Python ReAct loop* —
`abstractagent.adapters.react_runtime.create_react_workflow` — reached locally by
abstractcode and, for `react-agent@0.1.0`, materialized from a manifest-only bundle on
the gateway. The loop has a strict **verifier pass** that re-reads the transcript before
any tool-call-free answer is accepted as final and can force more tool calls; it is gated
on `_runtime.review_mode`, which **defaults to OFF**. abstractcode has always set it to
`True` with 3 rounds. abstractcode sent the key nowhere: `review_mode` appeared in
**zero of 31** gateway react runs, `review_count: 0` in every one — the verifier had
never run once on this lane. Second, the client's `--max-iterations` was **silently
discarded**: it rode as a flat top-level key, so abstractruntime seeded `_limits` with its
own default and abstractagent's resolver — which reads `_limits` first and treats
"the key is present" as proof the caller set it — obeyed **20**, not 50. Wire proof: 31
runs launched with `--max-iterations 50` all carry `scratchpad.max_iterations: 50` beside
`_limits.max_iterations: 20`. Mean iterations reached: **abstractcode 8.4, abstractcode
2.5**. Third, the client sent **no system prompt at all** (`system: String::new()` at both
call sites) while abstractcode injects project instructions every run. All three are now
fixed and verified on the wire.

---

## 1. What the two projects actually are

| | abstractcode | abstractcode |
| --- | --- | --- |
| Language / size | Python, 34k LOC (`react_shell.py` alone is 14.5k) | Rust, 35k LOC |
| Where the agent runs | **in-process**, local loop | **on the gateway**, as a workflow bundle |
| `--agent react` / `--workflow react-agent:react` | `ReactAgent` → `create_react_workflow` | manifest-only bundle (`flows: {}`, `metadata.native_loop_factory: "react"`) → `create_react_workflow` |
| Consequence | sets loop knobs directly as constructor args | must express every knob as `input_data` on the wire |

That last row is the whole story: **a knob abstractcode passes as a Python argument, the
TUI must name on the wire — and any knob it fails to name silently takes a server-side
default.** Three did.

---

## 2. Root causes, ranked

### P0-1 · The verifier gate was never armed — CONFIRMED, FIXED

`abstractagent/src/abstractagent/adapters/react_runtime.py:2244-2246`:

```python
raw_review = runtime_ns.get("review_mode") if isinstance(runtime_ns, dict) else None
review_mode = _boolish(raw_review) if raw_review is not None else False
if not review_mode:
    return StepPlan(node_id="maybe_review", next_node="done")
```

Default OFF. abstractcode arms it by default (`react_shell.py:251-252`
`review_mode: bool = True, review_max_rounds: int = 3`; `cli.py:1483-1484`). When armed,
the verifier is a real forced-work loop — it synthesizes tool calls and routes back into
`act` (`react_runtime.py:2411-2438`), and when executor-tagged tools are in the allowlist
it is explicitly told that *an artifact that was never executed is not verified*.
`review_mode` appears **nowhere** in abstractruntime or abstractgateway (verified by
exhaustive grep), so the client is the only possible sender.

**Owners:** abstractcode (didn't send it — **fixed**), abstractagent (facade defaults
ON, workflow defaults OFF, and the native-loop factory picks the *workflow* default — so
every gateway react run silently loses the facade's behavior), abstractruntime (compiler
doesn't inherit the key for flow-graph bundles).

### P0-2 · `--max-iterations` was silently replaced by 20 — CONFIRMED, FIXED

A three-package seam:

1. `run_input.rs:81` sent the budget as a **flat** `max_iterations` key, and `_limits`
   only when a context window was declared.
2. `abstractruntime/core/runtime.py:1163-1165` — `if "_limits" not in vars:` seed the
   runtime's own defaults → `max_iterations: 20` (`core/config.py:51`).
3. `abstractagent/adapters/react_runtime.py:114-119` uses "limits already has the key" as
   its proxy for "the CALLER set it". The runtime set it, so the legacy seed is skipped
   and `resolve_max_iterations` (`generation_params.py:94-100`, **limits first**) returns 20.

**The perverse tell that pins the diagnosis:** a run that *also* declared a context window
got its budget honored, purely because `_limits` then existed and the runtime skipped its
seeding. `--max-iterations` only worked when `--max-tokens` was also set.

**Owners:** abstractcode (**fixed** — budget now rides `_limits`), abstractagent (the
caller-intent proxy is wrong whenever a runtime materializes defaults before the first
node, i.e. always on the gateway path), abstractruntime (seeding a *default* into the
namespace callers use to express *intent* destroys the distinction).

> **Coupling note:** in the 31 observed runs the highest iteration reached was 10, so the
> 20-cap was **latent** — it becomes binding the moment the verifier is armed, because a
> verifier that routes back to `act` consumes iterations. Fixing either alone would have
> traded one wall for another. Both are fixed.

### P0-3 · No system prompt, no project context — CONFIRMED, FIXED

`StartOpts.system` was declared and serialized but hard-coded empty at **both**
construction sites (`ui/mod.rs`, `exec.rs`). abstractcode composes
`_runtime.system_prompt_extra` = `[AGENTS.md][skills block]` on every run
(`react_shell.py:12972-13000`).

**Honest caveat, found adversarially:** in *this* monorepo the AGENTS.md half is inert —
the framework root file is 362,790 chars, over the 200k cap both clients enforce, and
`abstractcode/` is its own git repo with no AGENTS.md, so the upward walk stops
immediately. The fix is correct and live-verified, but its value here is for *other*
workspaces; it does not by itself change behavior in this repo.

### P0-4 · The interactive lane could run a different workflow than asked — CONFIRMED, FIXED

`runner.rs` called `choose_workflow` and never compared the result to the request.
Headless `exec` refuses on mismatch; the interactive flag lane accepted the substitution in
silence, with only the header wordmark as a tell. Directly against the stated contract that
workflow selection must be exact.

### P0-5 · Bundle-only `--workflow <bundle>` could never resolve — CONFIRMED, FIXED

`choose_workflow` required **both** halves, so a bundle-only ref fell through to the
basic-agent fallback, which `exec` then refused as *"not found on this gateway"* — a false
diagnosis for an installed bundle. `--workflow basic-agent` appeared to work only because
it coincided with the fallback. **Live before/after:** `--workflow react-agent` exit 2 →
now resolves `react-agent:react`.

### P0-6 · `/goal` ran its worker with ZERO tools — CONFIRMED, FIXED

`goal-agent@0.0.1`'s `worker` node has a **connected** `tools` pin and no
`pinDefaults.tools`. The runtime's "was tools specified?" test is
`"tools" in input_data` (`abstractruntime/.../visual/executor.py:3537`), so a connected pin
with a null value normalizes to `[]` and the node-default fallback is skipped. For every
other workflow `tools: None` correctly means "use the flow's defaults"; here it meant the
worker cycled with no tools and could never do or verify real work.

### P1-1 · "Ran out of budget" was rendered as "done" — CONFIRMED, FIXED

The loop writes a machine-readable verdict from two different terminal nodes —
`outcome: "final_answer"` vs `"iteration_budget"`, plus `review_skipped`
(`react_runtime.py:2488-2489`) — precisely so hosts need not parse prose. Neither string
appeared anywhere in `src/`. The client printed `✓ done` either way. **This is the client
itself claiming completion the loop never claimed**, and it is why P0-2 stayed invisible
for 31 runs.

### P1-2 · Cross-turn context is prose-only — CONFIRMED, NOT FIXED (design call)

The TUI carries only `(user prompt, final answer)` pairs, capped 40 msgs / 24,000 chars;
every tool observation from the previous turn is dropped. abstractcode carries the full run
message list including `role="tool"` messages. The server-side session seed has the same
prose-only shape, so this is a durable-sessions-v1 design property, not a client bug — but
it means the model re-reads files it already read and can contradict its own edits across
turns. **Fix needs a ruling** (see §6).

### P1-3 · Multi-coder gate 1 rejects the plan in headless runs — CONFIRMED, NOT FIXED

This is the mechanism behind the long-standing *"multi-coder exits scout-only, 0
write_file"*. `multiagent-coding`'s `gate1` is an `ask_user` with
`pinDefaults.choices = ["approve","revise","research"]`, and `gate1_parse` accepts **only**
`approve`/yes/ok/lgtm. Headless `exec` answers every ask-user wait with a fixed
*"No interactive user is present…"* string → never matches → burns `max_plan_revisions`
and ends before the coder phase. The client cannot even see the offered choices:
`WaitKind::Ask` carries only `prompt`, while the runtime **does** serve `choices`
(`executor.py:3328-3339`). Deliberately left for a scoped change (§6).

### P1-4 · Narrower tool set, and codeact may be structurally unable to act — CONFIRMED

`basic-agent@0.0.3` pins 9 tools; abstractcode's local default is 13 + skill readers.
Missing: `browser_probe` (abstractcode's own comment: *reading source cannot prove a page
runs*), `skim_files`, `skim_folders`, `self_improve`. This **compounds P0-1**: the
verifier's execution-proof clause only fires when executor-tagged tools are in the
allowlist, so the gateway path was missing both the verifier *and* its proof tool. Worse:
`execute_python` and `self_improve` are in abstractagent's `ALL_TOOLS` but appear in
**neither** the gateway's 51-tool `/discovery/tools` nor anywhere in
abstractruntime/abstractcore/abstractgateway — so `codeact-agent`, whose only executable
tool is `execute_python`, is selectable but probably cannot act.

### P2 · Also confirmed, not fixed here

- **coding.v1 pipelines are unreachable.** `multiagent-coding:multiagent-coding` and
  `coding-agent:coding-agent` carry `abstractcode.coding.v1`, which the client's selection
  filter excludes — along with every knob that exists only on those roots
  (`max_fix_cycles`, `max_review_rounds`, `build_command`, `run_command`, …). The refusal
  now *names the interface* instead of lying, but reachability is a policy call (§7).
- **`workspace_root` can be silently dropped.** The gateway pops it when it falls outside
  operator roots and serves no warning; `all_except_ignored` silently downgrades. This is
  the mechanism behind artifacts landing under the monorepo root instead of the client repo.
- **`goal-agent` has no headless entry** — `/goal` is TUI-only, so no CI path exercises the
  goal loop.
- **Dead-metadata levers.** `max_iterations_default` and `headless_policy` are allow-listed
  and written into native-loop manifests but **read by nothing**; the shipped
  `react-agent@0.1.0` manifest doesn't even carry them. There is currently no server-side
  lever for the react budget or its verifier.
- **Answer-source fallback treats an unknown parent as first-level** (`transcript.rs`
  `unwrap_or(true)`). In a multi-agent tree a scout could bind as answer source and
  conclude the turn. The behavior is *deliberate and documented* (it fixed a real dead-ctx
  bug in deep-cycling trees) and the failure is only PLAUSIBLE, never observed — so I did
  **not** flip it. Settling test in §7.

### Refuted (hypotheses killed with evidence)

1. **"The Rust client ends runs early / kills the server run."** No. `finished` is set only
   by the run's own completion record; every observed run was `status: completed`,
   `current_node: done`. The only `cancel` path is user-initiated. `done · (run … finalizes
   on the gateway)` is honest — the run is already terminal.
2. **"The bundle bakes a worse prompt."** No. `basic-agent` bakes none; the base ReAct
   prompt is abstractagent's, byte-identical for both clients. `react-agent` has no graph.
3. **"Skills are cosmetic in the TUI."** No — full chain verified into the prompt slots.
4. **"Reasoning never reaches the provider."** No — full chain verified.
5. **"The TUI starves the iteration budget."** Backwards: 50 vs abstractcode's 25 — the
   *plumbing* was broken, not the number.
6. **"`@file` mentions silently drop content."** Mostly no; both routes end in the same
   runtime attachment inlining.
7. **"`/goal` doesn't pass tools/workspace/tool_policy."** Too broad — it shares
   `agent_start_opts`, so workspace and tool_policy *do* ride. The real defect was narrower
   (P0-6).

---

## 3. Workflow conformance — empirical, live

`scripts/workflow_conformance_probe.py` against the running gateway (lmstudio /
qwen3-4b, trivial prompt, 150s cap). This probes **client plumbing**, not model quality.

| ref | before | after |
| --- | --- | --- |
| `basic-agent` | GREEN *(by accident — coincided with the fallback)* | GREEN |
| `basic-agent:81795ea9` | GREEN | GREEN |
| `react-agent` | **REFUSED** (exit 2, false "not found") | **GREEN** |
| `react-agent:react` | GREEN | GREEN |
| `codeact-agent:codeact` | GREEN | GREEN |
| `memact-agent:memact` | GREEN | GREEN |
| `multiagent-coding` | **REFUSED** | **GREEN** (318.6s) |
| `multiagent-coding:multiagent-coder` | answers, exit 124 at the cap | **GREEN** (282.6s) |
| `multiagent-coding:multiagent-coding` | REFUSED, false reason | refused, **names the interface** (by policy — §7) |
| `coding-agent:coder` | answers, exit 124 at the cap | **GREEN** (74.9s) |
| `coding-agent:coding-agent` | REFUSED, false reason | refused, **names the interface** (by policy — §7) |

**Final: 9 of 11 refs GREEN.** The two refusals are the deliberate `abstractcode.coding.v1`
interface filter, now refusing honestly instead of claiming the bundle is absent — a policy
question, not a defect (§7 item 3).

**Harness note — the earlier exit-124 rows were the probe's fault, not the client's.** A
first pass with a 150 s cap reported the three pipeline entrypoints as
`ANSWERED-BAD-EXIT` (answer seen, exit 124), and the same ref scored GREEN on one run and
124 on the next with no code change between them. Two causes, both harness-side: the cap sat
inside the pipelines' answer band, and probes were sharing the local model with other gateway
work. Re-run at 600 s without contention, all three pass comfortably — and
`coding-agent:coder` finishes in **74.9 s**, half the cap that had failed it, so contention
was the larger factor. The probe default is now 600 s
(`scripts/workflow_conformance_probe.py`): a conformance gate must measure whether a workflow
runs, never how close it sits to the harness boundary. **Lesson worth keeping: a timing-based
verdict from a loaded machine is not evidence.**

**Note for the operator:** `coding-agent:coder` is described by its own manifest as exactly
the machinery this investigation is about — *"a builder agent writes code, an independent
verifier runs build/execute/match gates each round, and failures are fed back as specific
reprompts until all gates pass or the round budget is spent."* It is reachable today, and it
turned in the **fastest pipeline time of the three (74.9 s)** — so the verification machinery
you were missing is not the slow option. It is a better default than `basic-agent` for real
coding work.

Its verification also happens **inside the bundle**, so unlike the `review_mode` fix it does
not depend on the abstractruntime inheritance change — which makes it the one lever that
improves flow-graph coding runs *today*.

---

## 4. What I changed in abstractcode

All shipped on `verify-cap-file`. **501 tests pass** (was 489; +12), clippy clean, fmt clean.

| # | Change | Files |
| --- | --- | --- |
| 1 | Arm the verifier: `_runtime.review_mode` + `review_max_rounds`, default ON/3 (abstractcode parity), `--review`/`--no-review`/`--review-rounds`, `/review` command, session signals | `run_input.rs`, `cli.rs`, `store.rs`, `commands.rs`, `ui/mod.rs`, `exec.rs`, `lib.rs` |
| 2 | Send the iteration budget in `_limits` beside `max_tokens`, so the runtime cannot out-vote the operator | `run_input.rs` |
| 3 | Project instructions (`AGENTS.md`) via `_runtime.system_prompt_extra` — the same wire key abstractcode uses; `--no-project-context` opts out | **new** `project_context.rs`, `run_input.rs`, `exec.rs`, `ui/mod.rs`, `cli.rs`, `lib.rs` |
| 4 | Resolve bundle-only workflow refs inside their bundle; refuse ambiguity rather than guess | `discovery.rs` |
| 5 | Honest refusals: distinguish *installed behind another interface* / *ambiguous* / *absent* | `discovery.rs`, `exec.rs` |
| 6 | Interactive lane refuses a substituted `--workflow` in the transcript | `runner.rs`, `lib.rs` |
| 7 | Surface the loop's verdict: `iteration budget exhausted — the agent STOPPED, it did not finish`, and a `#FALLBACK` when the verifier was skipped | `protocol.rs`, `transcript.rs` |
| 8 | `/goal` sends the materialized tool list (the goal bundle treats a missing list as an empty one) | `ui/goal.rs`, `store.rs` |
| 9 | Headless `exec` declares agent workflow ids to the fold | `exec.rs` |
| 10 | Two probes: workflow conformance, and an A/B proof of context injection | **new** `scripts/workflow_conformance_probe.py`, `scripts/project_context_live_verify.py` |

### Live verification (not just unit tests)

Run-state inspection of `react-agent@0.1.0:react` runs after the change:

```
review_mode=True  review_max_rounds=3  _limits.max_iterations=4  system_prompt_extra=Y
```

— against `review_mode=None`, `_limits.max_iterations=20` in all 31 prior runs. The
`max_iterations=4` is the probe's own `--max-iterations 4` being **honored for the first
time**.

**Reach, measured rather than reasoned.** Inspecting parent *and child* runs of the pipeline
probes shows exactly how far the two keys travel:

```
multiagent-coding@0.0.14:multiagent-coder   (ROOT)   review=True   lim_iter=6
multiagent-coding@0.0.14:multiagent-coding  (child)  review=None   lim_iter=20
coding-agent@0.2.4:coding-verify-gates      (child)  review=None   lim_iter=20
basic-agent@0.0.3:15f19f7f                  (child)  review=None   lim_iter=20
visual_react_agent_coding-agent_…           (child)  review=None   lim_iter=12/16/40
```

The root receives both keys; **no child inherits either**. This is live confirmation of the
compiler gap that until now had only been read out of source — and it is broader than
`review_mode`: the child `_limits` does not inherit the parent's iteration budget either,
falling back to the runtime's 20 (or a node-pinned value where a flow author set one). So
the premature-completion fix currently reaches **native-loop bundles only**
(react/codeact/memact, whose root vars *are* the loop's vars) and stops at the first
Agent-node boundary of every flow-graph bundle — including `basic-agent`, the default
workflow. That makes the one-row abstractruntime change in §6 the highest-leverage item in
this report.

No run errored on the unknown keys, confirming `_runtime` is a tolerant pass-through
namespace and the client-side send is safe to ship ahead of that change.

Project-context A/B (`project_context_live_verify.py`) — an AGENTS.md instruction the
prompt never mentions:

```
injected   : exit=0 token_seen=True  announced=True
opted out  : exit=0 token_seen=False
VERDICT: PASS — AGENTS.md reaches the model, and only when injected
```

The negative control is the load-bearing half: the token appears **only** when injected.

---

## 5. Reproducing

```bash
cargo test --release && cargo clippy --release --all-targets && cargo fmt --check
```

```bash
python3 scripts/workflow_conformance_probe.py
```

```bash
python3 scripts/project_context_live_verify.py react-agent:react
```

---

## 6. Requests to other packages

Ready to send as-is. Every claim below was verified against current source on 2026-07-30;
file:line references are to the checked-out tree.

### → abstractagent

> Forensics on why gateway-hosted react runs iterate ~3× less than in-process ones
> (mean 2.5 vs 8.4 iterations over 39 runs) found three defects in the native-loop lane.
> The client half is fixed and live-verified; these are yours.
>
> **(1) P0 — the facade and the workflow disagree on the verifier default, and the
> native-loop factory picks the losing one.** `agents/react.py:83-84` defaults
> `review_mode=True, review_max_rounds=3`; `adapters/react_runtime.py:2244-2246` defaults
> the *workflow* to `False`. A gateway-hosted `react-agent@0.1.0` never goes through the
> facade, so it silently loses the behavior every in-process caller gets. Result:
> `review_mode: None` and `review_count: 0` in **31 of 31** runs — the verifier has never
> executed on this lane. Please make `materialize_native_loop_spec` seed
> `_runtime.review_mode` / `review_max_rounds` from the factory (or manifest) defaults so
> the two doors agree, and state the intended default explicitly in one place.
>
> **(2) P0 — `_caller_set_budget` is the wrong proxy for caller intent.**
> `adapters/react_runtime.py:114-119` infers "the caller set the budget" from
> `_limits.max_iterations is not None`. On the gateway path abstractruntime materializes
> its own `_limits` **before** the workflow's first node
> (`abstractruntime/core/runtime.py:1163-1165` → 20), so the proxy is always true and the
> runtime's default out-votes the operator; `scratchpad.max_iterations` held the real 50 in
> all 31 runs while the loop obeyed 20. Please distinguish caller-provided limits from
> runtime-seeded defaults (a provenance marker, or take the max of `_limits` and the
> scratchpad seed).
>
> **(3) P1 — `system_prompt_extra` has multiple writers that clobber each other.**
> `agents/unattended.py:64` returns `{"system_prompt_extra": UNATTENDED_DIRECTIVE}` and its
> documented usage is `vars["_runtime"].update(overrides)` — a whole-value overwrite. This
> client now also writes that key (project instructions), as abstractcode has for a long
> time. Whoever writes second wins and the other instruction block vanishes silently.
> Please make the key **compositional** (accept a list, or expose
> `unattended_runtime_overrides(existing_extra=...)` that appends), and document the
> composition order.
>
> **(4) P1 — `ALL_TOOLS` names tools the gateway cannot execute, and omits the one the
> verifier needs.** `execute_python` and `self_improve` are in `ALL_TOOLS` but appear in
> neither the gateway's 51-tool `/discovery/tools` nor anywhere in
> abstractruntime/abstractcore/abstractgateway. `codeact-agent`'s only executable tool is
> `execute_python`, so it is selectable but structurally unable to act. Conversely
> `browser_probe` **is** registered server-side
> (`abstractruntime/integrations/abstractcore/default_tools.py:547-556`) but is absent from
> `ALL_TOOLS` — and the verifier's execution-proof clause only fires when executor-tagged
> tools are in the allowlist, so the gateway path lacked both the verifier and its proof
> tool. Please intersect native-loop default toolsets with the registered tool map at
> materialization and refuse (or warn) on the difference, and add `browser_probe`.
>
> **(5) P2 — `max_iterations_default` and `headless_policy` are write-only.**
> `adapters/native_loop_registry.py:42-43` allow-lists them and
> `build_native_loop_manifest:200-220` writes them, but nothing reads them; the shipped
> `react-agent@0.1.0` manifest omits them entirely. So there is no server-side lever for
> the react budget or verifier — `input_data` is the only door. Either honor them at
> materialization or drop them so they stop implying a control surface that does not exist.

### → abstractruntime

> **(1) P0 — seeding a default into the namespace callers use to express intent.**
> `core/runtime.py:1163-1165` fills `vars["_limits"]` with `RuntimeConfig` defaults whenever
> the key is absent. Downstream consumers then cannot tell an operator's declaration from
> your default, and abstractagent's react loop consequently obeyed 20 while the caller
> asked for 50 (31/31 runs). Compounding it, `core/vars.py:85-89` `get_limits` never
> back-fills keys into an existing dict, which produced a genuinely perverse tell: a run
> that *also* declared `max_tokens` got its iteration budget honored, purely because
> `_limits` then existed. Please either merge per-key (so a caller-provided `_limits` still
> receives your other defaults) or mark provenance so consumers can distinguish the two.
>
> **(2) P0 — child Agent runs inherit neither `review_mode` nor the iteration budget.**
> For Agent nodes the compiler rebuilds a fresh child `_runtime`
> (`.../compiler.py:1347-1352`) and then adds a fixed inherited set (`thinking`,
> `skills_block`, `temperature`, `seed`, `system_prompt_extra`, …). `review_mode` /
> `review_max_rounds` are not in it, and appear nowhere in abstractruntime or
> abstractgateway at all.
>
> **Measured live on 2026-07-30**, parent vs children of one pipeline run:
>
> ```
> multiagent-coding@0.0.14:multiagent-coder   (ROOT)   review=True   lim_iter=6
> multiagent-coding@0.0.14:multiagent-coding  (child)  review=None   lim_iter=20
> coding-agent@0.2.4:coding-verify-gates      (child)  review=None   lim_iter=20
> basic-agent@0.0.3:15f19f7f                  (child)  review=None   lim_iter=20
> ```
>
> Note the second column: the child `_limits` does **not** inherit the parent's iteration
> budget either — it falls back to your 20 (item 1 above) even when the root carries an
> explicit 6. So a client can set neither the verifier nor the budget for any agent that
> actually does the work inside a flow-graph bundle, `basic-agent` (the default workflow)
> included. Please add `review_mode` and `review_max_rounds` to the inherited list beside
> `thinking`, and let a child's `_limits` inherit the parent's rather than re-seeding
> defaults. This is the highest-leverage change in this report: one row plus one inheritance
> rule, and it is what makes the premature-completion fix reach the default workflow.
>
> **(3) P1 — connected-but-defaultless pins resolve to empty, not to the node default.**
> `.../visual/executor.py:3537` tests `tools_specified = "tools" in input_data`. A pin that
> is *wired* but carries `None` therefore counts as "specified", normalizes to `[]`, and
> skips the node-default fallback at `:3540`. That is how `goal-agent@0.0.1`'s worker ran
> with zero tools whenever a client sent no explicit list. Please treat an explicit `None`
> as unspecified (or distinguish "pin connected" from "value provided").
>
> **(4) P2 — durable sessions are prose-only.** The session seed carries `(user prompt,
> assistant answer)` pairs for completed root runs (40 msgs / 24,000 chars) and drops every
> `role="tool"` observation, so a multi-turn agent cannot see what it already did.
> abstractcode carries the full run message list. **This one needs a ruling, not a patch:**
> if prose-only is the intended v1 contract, the client should synthesize a per-turn work
> log instead; if not, the seed should carry a bounded tool tail. Please state which.

### → abstractgateway

> **(1) P1 — `workspace_root` is silently dropped or rewritten.**
> `routes/gateway.py:_sanitize_run_workspace_policy` (~:3340) pops `workspace_root` when it
> resolves outside the operator roots with overrides off, and silently downgrades
> `all_except_ignored` → `workspace_only`, serving **no warning**. Clients then cannot tell
> the operator where their files actually went — the mechanism behind bench artifacts
> landing under the monorepo root instead of the target repo. Please return
> `effective_workspace` plus a `warnings[]` array from `runs/start`; this client will print
> the effective root on the run banner as soon as it is served.
>
> **(2) P1 — `react-agent@0.1.0` ships without the metadata that would make it
> tunable.** `scripts/build_react_agent_bundle.py:38-49` writes metadata
> `{native_loop_factory, loop_family, publisher}` only — no `max_iterations_default`, no
> `headless_policy`, even though abstractagent allow-lists both. Combined with the
> abstractagent items above, there is no server-side lever for the react lane at all.
> Please add them to the builder once abstractagent honors them.
>
> **(3) P2 — a selectable bundle whose tools the gateway cannot execute.**
> `codeact-agent@0.1.0` is published, carries `abstractcode.agent.v1`, and appears in every
> client's picker, but its only executable tool (`execute_python`) is not in the gateway's
> registered tool map. Please validate a native-loop bundle's toolset against the registry
> at publish/load time and refuse or mark it, the same way publish-time validation of
> nonexistent tool references was requested in the earlier `browser_probe` finding.
>
> **(4) P2 — `input_data.tools` is ignored by native loops.** Nothing maps
> `input_data.tools` → `_runtime.allowed_tools` in `bundle_host`, so `/tools` is a no-op for
> react/codeact/memact while it works for flow-graph bundles. Please map it in
> `bundle_host` so tool selection means the same thing across all workflows.

### → abstractflow (bundle authoring)

> **(1) P0 — `goal-agent@0.0.1`'s `worker` has a connected `tools` pin with no
> `pinDefaults.tools`,** which resolves to an empty toolset for any client that doesn't send
> an explicit list (see the abstractruntime item 3). Please give `on_flow_start.tools` a
> concrete `pinDefaults` list as a belt, independent of the runtime fix.
>
> **(2) P1 — the multi-coder's gate 1 makes "anything else" mean reject.**
> `multiagent-coding`'s `gate1` is an `ask_user` offering
> `choices = ["approve","revise","research"]`, and `gate1_parse` accepts only
> `approve`/yes/ok/lgtm — so any unattended client's generic answer burns
> `max_plan_revisions` and the run ends **scout-only with 0 `write_file`**, the exact
> long-standing symptom. Please either fail closed with an explicit "unrecognized gate
> answer" verdict instead of silently counting it as a revision request, or treat a
> non-matching answer under `gating_mode=wait` as a hard error rather than feedback.
>
> **(3) P1 — declared pins that go nowhere.** `multiagent-coder`'s `map_input` never
> forwards its **required** `tools` pin; `coding-agent:coder`'s `tools` pin has **zero
> edges**. The gateway serves these in `input_schema`, so the served contract is lying to
> every client. Please wire them or drop them.
>
> **(4) P2 — the coding.v1 knobs are unreachable from chat clients.**
> `max_plan_revisions`, `max_fix_cycles`, `max_review_rounds`, `build_command`,
> `run_command`, `skills` exist only on the coding.v1 roots, which agent.v1 clients cannot
> select. Please surface the useful ones on the agent.v1 wrapper entrypoints.
>
> **(5) P2 — `basic-agent@0.0.3` pins 9 tools** vs abstractcode's local 13. Missing
> `browser_probe`, `skim_files`, `skim_folders`. Since `basic-agent` is the default
> workflow, this is the tool surface most runs actually get.

### → abstractcore

No new findings in this pass. The `search_files` output cap, `edit_file` clamp-vs-refuse,
and usage-split zero-fill items from `untracked/agent_quality_investigation.md` still stand
and are unaffected by this work.

---

## 7. Open decisions for you

1. **Default workflow for coding work.** `basic-agent` is the current default and is the
   *only* agent.v1 entrypoint with no verification machinery. `coding-agent:coder` ships a
   builder + independent verifier with per-round gates and reprompting, and works today.
   Should the TUI default to it for coding sessions?
2. **`review_mode` default ON — confirm.** I defaulted it ON to match abstractcode. It costs
   one extra verifier LLM call per candidate final answer. Cheap insurance against the
   reported symptom, but it is a real cost change on every run; `--no-review` and
   `/review off` opt out.
3. **Should coding.v1 pipelines be directly selectable?** They are installed and richer than
   their chat wrappers. Today the client refuses them (now with an honest reason). Widening
   the filter would let you drive the full pipeline deterministically; keeping it narrow
   keeps one interface contract. Your call.
4. **Prose-only cross-turn context** (P1-2) — patch the client with a synthetic work log, or
   push abstractruntime to carry a bounded tool tail? Depends on the durable-sessions ruling.
5. **The answer-source `unwrap_or(true)` heuristic.** I left it alone: it is deliberate,
   documented, and fixed a real bug, and the multi-agent failure is unverified. To settle
   it, replay a completed `multiagent-coding` ledger through the existing
   `run_tree_replay` test and assert `agent_run_id` is not a scout.
6. **The one A/B nobody has run.** Arming the verifier is a *behavior-authority* change, and
   this repo's own hard-won lesson is that those pass contract review and fail live. The
   decisive experiment: the Zelda prompt on `react-agent:react`, with and without
   `review_mode`, asserting `scratchpad.review_count >= 1` and comparing iterations and
   artifact size against baseline run `efeb46de`. I have the harness
   (`scripts/zelda_headless_bench.py`) but did not spend an hour of gpt-5.x on it without
   your go-ahead.

---

## 8. Biggest structural risk (worth its own line)

The gateway **already serves** `GET /api/gateway/bundles/{b}/flows/{f}/input_schema` — the
authoritative declared-pin contract for every workflow — and this client **never reads it**.
abstractcode's `flow_cli._required_entry_inputs` hard-fails on missing required pins; the
TUI discovers pin mismatches only when a run behaves oddly. Every P0 in this report is an
instance of the same class: *a knob the client failed to name took a server default and
nobody found out for 31 runs.* A cheap CI guard — fetch every entrypoint's schema and
assert each required pin is either sent by `build_input_data` or explicitly waived, with no
LLM calls — would have caught P0-2 and P0-6 the day they appeared, and would catch the next
one automatically. **I recommend this as the next piece of work.**
