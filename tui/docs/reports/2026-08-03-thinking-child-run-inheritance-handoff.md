# Handoff: `_runtime.thinking` is lost between root and child runs (gateway/runtime seat)

**Date:** 2026-08-03 (all timestamps UTC — the relay logs are UTC; `ps` is local; mixing
them cost this investigation an hour once already)
**From:** abstractcode seat
**For:** the gateway-seat agent investigating why bench requests reach the relay without
`reasoning_effort`
**Status of the wire:** every claim below is taken from the relay's own `inbound_request`
log (`~/.airelays/logs/YYYY/MM/DD-HH.log`, verbatim client bytes) and from the runtime
store (`/workspace/runtime/run_<id>.json`), never from client
intent.

---

## 0. TL;DR

Three independent defects stacked. One is fixed and wire-verified; two remain, and the
load-bearing one is yours:

| # | layer | defect | status |
|---|---|---|---|
| 1 | abstractcore `openai_compatible_provider.py` | requested `thinking` level silently dropped — never mapped to `reasoning_effort`, and the payload allowlist would have eaten it anyway | **FIXED + wire-verified** (details §2) |
| 2 | **gateway/runtime child-run spawn** | root run carries `_runtime.thinking="medium"`; the **child runs that do the actual LLM work carry `thinking=None`** | **OPEN — this is the one to chase** (§4) |
| 3 | abstractcode (Python client) gateway path | `--reasoning medium` never reaches the start request; even the ROOT run records `thinking=None` | OPEN, that client's seat (§6) |

Also material to anyone reading relay logs: **resident background agents generate
continuous ABSENT-effort traffic** that poisons any time-window attribution (§5).

---

## 1. Timeline of evidence (all 2026-08-03 UTC)

| time | event |
|---|---|
| ~17:00 | Relay-history audit: across 9,021 abstractcore-UA requests since 07-03, **zero** ever carried `reasoning_effort`. Five of eight benchmark arms ran with reasoning OFF while every layer above reported medium. |
| 20:00:33 | Defect #1 patched in abstractcore (two parts, §2). |
| 20:00:46 | Direct `create_llm("openai-compatible").generate(thinking="medium")` → wire shows `reasoning_effort=medium`, upstream `reasoning={"effort":"medium"}`. Part fixed. |
| 20:02:40 | Gateway restarted (it had imported abstractcore before the patch; 0 active runs at restart). |
| 20:04 | Gateway-path smoke (tui-basic): 3/3 requests at medium. **This smoke was misleading** — tui-basic is the one arm whose working child inherits thinking (§4). |
| 20:05:08 | 15-cell relaunch started (`untracked/rtype-medium/`). |
| 20:14 | Wire check on the window: 63 requests, **11 medium / 52 absent**, model gpt-5.4 on all. Relaunch **stopped** (~10 min in; partial cells preserved on disk, nothing deleted). |
| 20:16–20:33 | Per-arm isolation probe (§3). |
| 20:33+ | With every bench process dead: **32 requests in ~15 min, all ABSENT** — proof of resident background traffic (§5). |
| ~20:40 | Run-store differential (§4): roots medium, working children None. |

---

## 2. Defect #1 — abstractcore (FIXED, for reference and so nobody re-fixes it)

File: `abstractcore/abstractcore/providers/openai_compatible_provider.py` (uncommitted
working-tree edit, operator-approved; that seat also has unrelated uncommitted work in
the same file — do not clobber).

Two parts, because either alone is inert:

1. **Map** — in `_apply_provider_thinking_kwargs`, a branch gated on
   `self._model_reasoning_levels()` being non-empty **and** no chat-template surface
   being declared: maps the requested level to `kwargs["reasoning_effort"]`, mirroring
   `openai_provider.py:97-149`. gpt-5.4 declares `reasoning_levels:
   [none,low,medium,high,xhigh]` but **no** `thinking_control` block, so it previously
   fell through to `return kwargs, ThinkingControlHandling()` untouched.
2. **Emit** — in `_mutate_payload` (called by BOTH payload builders, sync + streaming):
   copy `reasoning_effort` into the payload. The builders compose from an explicit
   allowlist, so a kwarg set by the hook alone never reaches the wire. This was proven
   on the wire before shipping: with Part 1 only, inbound still had no field and
   upstream still got `reasoning: null`.
   Also added the missing `import warnings` (module didn't import it; the disable-
   fallback branch would have raised `NameError`).

Wire proof after both parts + gateway restart:
```
INBOUND   python-httpx/0.28.1  gpt-5.4  reasoning_effort=medium
UPSTREAM  gpt-5.4              reasoning={"effort": "medium"}
```

**Consequence for you:** any LLM call whose kwargs actually carry `thinking="medium"`
now reaches the relay correctly. Every ABSENT request below is therefore a call site
where the value was **never passed**, not a provider drop. That inversion is what makes
the remaining bug attributable at all.

---

## 3. Per-arm isolation probe (raw data)

Method: each arm run ALONE via the real harness
(`CB_OUT=probe-<arm> CB_PARALLEL=1 python3 scripts/bench_clients.py --clients <arm>
--repeats 1 --smoke`), bracketed by UTC timestamps; relay traffic then bucketed by
window. Outputs preserved under `untracked/probe-<arm>/`.

| arm | window (UTC) | reqs in window | medium | absent |
|---|---|---|---|---|
| tui-basic | 20:16:15–20:19:01 | 13 | 2 | 11 |
| tui-coder | 20:19:01–20:24:35 | 69 | 5 | 64 |
| tui-multi | 20:24:35–20:29:33 | 49 | 8 | 41 |
| abstractcode-basic | 20:29:33–20:30:04 | 8 | 3 | 5 |
| abstractcode-coder | 20:30:04–20:32:39 | 23 | 0 | 23 |

**Do NOT read this table at face value** — the windows are contaminated by resident
traffic (§5). The corrected per-cell reading, via each cell's own session, is:

- **tui-basic**: its own run made 2 LLM calls (`runs.json llm=2`) and the wire shows
  exactly 2 medium in-window (session `208bb49a…`). **Its own calls: ALL medium.** ✓
- **abstractcode-basic** (local loop, no gateway): its 3 "Iteration: N/20" calls are all
  medium (session `c5b21613…`). **Defect #1's fix works for this arm.** ✓
- **abstractcode-coder**: its own session `5cfd87dc…` = 14 requests, **0 medium** —
  including its "Iteration: N/30" root-loop calls. Client-side (§6). ✗
- **tui-coder / tui-multi**: see §4 — their roots say medium, their working children say
  None. The verifier calls visible in their windows
  (`sys: "You are a rigorous, independent code verifier"`, `response_format` set,
  1x in coder's window, 4x in multi's) are **ABSENT** — consistent with children not
  inheriting. ✗

Shape note: medium and absent requests are otherwise byte-identical in key-set
(`messages, model, prompt_cache_key, stream, temperature, tool_choice, tools, top_p`),
same persona prompt, `stream: false`. Nothing about the payload path differs — only
whether `thinking` was present at the call site.

---

## 4. Defect #2 — child runs lose `thinking` (YOUR TARGET), with a built-in differential

Runtime store, the three probe runs (`/workspace/runtime/`):

```
tui-basic   root 43f20ff7  session acode-f159becff269  thinking=medium
  child dc41c81e  thinking=None    flow=basic-agent@0.0.4:15f19f7f
  child 68312896  thinking=medium  flow=visual_react_agent_basic-agent_0_0_4_81795ea9_node-2
  child 7220375f  thinking=None    flow=basic-agent@0.0.4:15f19f7f

tui-coder   root ff6b5240  session acode-0c026d89be56  thinking=medium
  child 46c260da  thinking=None    flow=coding-agent@0.2.6:coding-agent

tui-multi   root 011021ae  session acode-21a32de246ae  thinking=medium
  child c10a6081  thinking=None    flow=multiagent-coding@0.0.18:multiagent-coding
```

The decisive pair: **same parent, same session, one child inherits and two do not.**
`run_68312896` (the `…_node-2` visual-wrapper agent child) carries `thinking=medium`;
`run_dc41c81e` / `run_7220375f` (`basic-agent@0.0.4` children) carry `None`. So there are
(at least) two child-spawn paths in gateway/runtime, and only one of them copies
`_runtime.thinking` from the parent. Diffing how `68312896` was created versus
`dc41c81e` — same parent run, files side by side in the store — should hand you the
exact code path.

Prior intel that may shortcut it (from the 2026-07-30 parity analysis, unverified
against today's tree): the visualflow compiler rebuilds each child's `_runtime` and
copies a **fixed key set** said to include `thinking`
(`abstractruntime/visualflow_compiler/compiler.py:1347-1396` at the time); child agents
spawned via the **subworkflow effect** path may not go through that copy. The wire says
whatever path spawns `basic-agent`/`coding-agent`/`multiagent-coding` children does NOT
propagate it.

Why it matters for the benchmark: for `tui-coder` and `tui-multi`, **the root run makes
few or no LLM calls — the children do the building and verifying.** Root-level medium
with child-level None means those arms effectively ran with reasoning OFF, again, even
after Defect #1 was fixed. tui-basic escapes only because its working child happens to
be on the inheriting path.

### Validation recipe (what "fixed" must look like)

1. `CB_OUT=probe2-tui-coder CB_PARALLEL=1 python3 scripts/bench_clients.py --clients tui-coder --repeats 1 --smoke`
2. Store check: every `run_*.json` whose `session_id` matches the new run — root AND all
   children — carries `thinking="medium"`.
3. Wire check: `python3 scripts/verify_wire_route.py --since <T0> --until <T1>` shows the
   cell's own session at 100% medium (filter by its `prompt_cache_key`; window totals
   will still include resident traffic, §5).
4. The gateway process must have been **restarted after** any abstractcore/runtime edit —
   it imports at startup. This exact trap produced a false "fixed" once already today.

---

## 5. Resident background traffic (measurement hazard, and possibly its own finding)

With every bench process dead (20:33+), the relay still received **32 generation
requests in ~15 min**, sessions `0ae0a0ec…`, `2d91b2b0…`, `5ceb9734…`, `0c6cbf17…`,
`4728904…`, `b45625fa…` — ReAct-persona, tool-bearing, **all ABSENT effort**. These are
long-lived gateway-resident agents (agora/entity services or similar — gateway seat
knows). Consequences:

- **Any time-window attribution of relay traffic is invalid.** My first FAIL verdict
  (11/63 medium) partially misread resident traffic as bench traffic. Attribution must
  key on `prompt_cache_key` (`session:<hash>`) or on the run store.
- If those residents are supposed to run with reasoning, they currently don't — same
  root causes. Not my call; flagging.
- They also burn relay quota continuously. Also not my call.

---

## 6. Defect #3 — abstractcode Python client (that client's seat, not yours)

`abstractcode exec --reasoning medium --agent coder` produces a gateway root run with
`_runtime.thinking = None` (store-confirmed pre-fix; probe session `5cfd87dc…` = 0/14
medium post-fix). The client parses the flag but never places it in the start request.
Its **local** loop (`--agent basic-agent`, no gateway) is fine post-Defect-#1 —
store-independent, wire-confirmed 3/3 medium. Fixing the gateway inheritance (#2) will
NOT fix this arm; the value is absent from the root.

---

## 7. Benchmark state (my seat, for the record)

- The 20:05 relaunch was stopped at ~10 min. Partial artifacts preserved:
  `untracked/rtype-medium/` (7 cell logs, 1 completed cell) + `untracked/rtype-medium.log`
  + probe outputs `untracked/probe-*`. **Nothing deleted** (operator constraint: no rm /
  no git-mutating ops — one pre-constraint commit `7abb916` disclosed already).
- Even had it finished: tui-coder-*/tui-multi-* invalid (children at None),
  abstractcode-coder-* invalid (root at None), tui-basic-*/abstractcode-basic-* likely
  valid. 6/15 usable is not a benchmark; stopping was right for the wrong reason.
- The full 15-cell relaunch resumes when §4's recipe passes end-to-end. `opencode`, `pi`
  (relay, medium, verified from their own stores) and `codex` (ChatGPT subscription,
  medium) stay reused from the existing matrix; they were never affected.
- Scorer frozen (three adversarial passes; fixture suite `tests/rtype_fixtures/`,
  margin +0.4854, drift +0.0000 — must still pass before any rescoring).
- New tool this incident produced: `scripts/verify_wire_route.py` — reads the relay's
  inbound+upstream logs, hour-file-pruned so it's ~1 s per window. Known limitation to
  fix when touched next: it should learn `--cache-key` filtering so per-cell verdicts
  don't require ad-hoc scripts (§5's lesson).

## 8. Open questions for the gateway seat

1. Which spawn path creates `basic-agent`/`coding-agent`/`multiagent-coding` children,
   and why does it drop `_runtime.thinking` when the `…_node-2` path keeps it?
2. Is the fix "copy `thinking` in that second path" or "children inherit the parent
   `_runtime` wholesale minus an explicit blocklist"? (The same gap already bit
   `system_prompt_extra` and `review_mode` — see `src/run_input.rs` comments. Three
   keys lost on the same edge suggests the blocklist design, not three one-off copies.)
3. Should resident agents run with an explicit effort? They currently send none, which
   after Defect #1's fix means the relay default — `none` for gpt-5.4.
