# Orchestration cards — the coding workflows, how each works, how they differ

One card per orchestration. "Measured profile" = Zelda benchmark, gpt-5.4 medium,
n=3 post-fix runs unless noted (v2 numbers from the 2026-08-01 v2 matrix, n=2).
All are gateway bundles, `abstractcode.agent.v1` chat entries, runnable as
`abstractcode-tui exec "<prompt>" --workflow <bundle:flow>`.

---

## react-coder — the plain loop
**Bundle:** `react-coding:react-coder` · **Anatomy:** one LLM in a
think→tool→observe loop (`llm_call` + `tool_calls`), no agent node, no in-loop
verifier. The loop ends when the model answers without tool calls, the budget
runs out, or a queued steer extends it.

- **How it decides "done":** the model claims it; an OPTIONAL post-loop
  `verify_command` decides `passed`, and default wait-mode routes the finish
  claim to a human gate. The benchmark ran ungated with no verify_command —
  in THAT posture, nothing checks.
- **Memory:** the growing transcript (UNBOUNDED as of the 2026-08-02 ADR-0026
  purge — the 24000-char default tail bound was removed; an operator who wants
  one sets the `max_chars` pin explicitly and gets a loud in-band trim marker).
- **Measured profile:** 10 llm calls · 32 KB · 5 min · scores 0.60–0.81 · 1 screen.
- **Character:** cheapest and fastest; stops at its first satisfactory draft —
  the canonical premature-completion pattern. Output is playable but shallow.
- **Use when:** small, well-specified tasks where a review pass would cost more
  than a redo.

## ralph-coder — fresh context, workspace memory
**Bundle:** `ralph-coding:ralph-coder` · **Anatomy:** the SAME fixed prompt every
cycle, each cycle a **fresh subflow run** (no transcript carry). Memory lives in
the workspace: read `PLAN.md`/`PROGRESS.md` first, append `PROGRESS.md` last.
A deterministic gate ends the loop: `verify_command` exit 0 **and** the `DONE:`
marker in PROGRESS (with `verify_command` empty the completion is stamped
`marker-only-unverified` — the honest degradation added in 0.1.2).

- **v0.2.0 (tested, n=4): scores [0.385, 0.636, 0.75, 0.795] mean 0.641, calls 55-112 — warm-start works, instability remains; last for greenfield.** Changes: warm start (each cycle's prompt mechanically embeds the
  last PROGRESS entries + a workspace listing — fresh context, re-derivation
  tax reduced ~31% at n=2, not eliminated) and early stop (two consecutive verified-green, unchanged-workspace
  cycles conclude the loop instead of burning to max_cycles; settling requires
  a verify_command — marker-less runs never settle).
- **Measured profile (0.1.2):** 109 llm calls · 32 KB · 22 min · 0.3 KB/call ·
  widest variance (0.955 and 0.28 in one arm).
- **Character:** immune to context poisoning by construction; pays for it in
  re-derivation. The workspace IS the agent's mind.
- **Use when:** work that outlives any context window (UNTESTED hypothesis — no
  overflow runs exist); not greenfield builds that fit in context.

## coder — builder + independent verifier + gates
**Bundle:** `coding-agent:coder` · **Anatomy:** ONE builder agent per round,
then an **independent verifier subflow**: deterministic gates first (delivery,
integration/references, orphans, DOM contract, self-report hash binding, plus a
bounded `browser_probe` execution for web artifacts — script syntax surfaces
only via the probe or the verifier's own commands), then an LLM verifier
judging BUILDS / EXECUTES / MATCHES against the original request, with the gate
outputs as ground truth (three-state since 0.2.6: missing probe diagnostics
read UNKNOWN, never False). Failures return to the builder as named defects;
repair is smallest-change-only, and a MATCHES-only gap escalates to a ONE-TIME
REBUILD (the additive repair charter lives in multi and spec, not here). Round
budget, honest stopped-open-failures report.

- **How it decides "done":** the verifier and gates do; the builder's own claim
  is never sufficient.
- **Measured profile:** 52 llm calls · 193 KB · 26 min · **3.7 KB/call (best)** ·
  scores 0.80–**1.0** · NPC evidence 3/3.
- **Character:** the efficiency frontier — maximum checking per token, no
  coordination overhead. Its blind spot: it verifies against the request but
  has no coverage *ledger*, so unstated-but-implied scope can slip.
- **Use when:** the default for real coding tasks today.

## multiagent-coder — the pipeline
**Bundle:** `multiagent-coding:multiagent-coder` · **Anatomy:** two scouts
(code + internet) → planner (structured plan, human gate optional) → builder
rounds with the coder-style verify subflow → PR/doc stage. Steering,
repair-history memory (untruncated since 0.0.17), additive repair charter.

- **v0.0.18 (tested, n=4): scores [0.647, 0.765, 0.822, 0.825] mean 0.765 at 106-141 calls — consistent, costliest; fast-path fires but Zelda-scale totals unchanged.** Changes: greenfield preflight — an empty workspace skips
  scouts+planner entirely ("the request is the plan"), recorded in the report;
  a dead probe FAILS SAFE to brownfield; Zelda-scale totals were unchanged by
  the fast-path (builder rounds dominate); brownfield keeps the full pipeline.
- **Measured profile (0.0.17):** 120 llm calls · 84 KB · 57 min · 0.7 KB/call ·
  scores 0.795–0.825 · most drawn dialogue text; only Zelda arm to reach 3
  screens once.
- **Character:** the most machinery per unit of quality; scouts add real value
  only when there is something to scout.
- **Use when:** brownfield tasks in unfamiliar codebases (UNTESTED hypothesis — no brownfield runs exist); not greenfield.

## spec-coder — two loops: build it, then prove it covers the ask
**Bundle:** `spec-coding:spec-coder` · **Anatomy (the DigitalArticle design):**
**Loop 1** = coder's build/verify machinery. **Loop 2** = a requirements ledger:
extraction (once, temp 0, strict schema) turns the USER PROMPT into ≤12 stated
(≤16 with derived) probe-able requirements with countable thresholds; each coverage round runs two
bounded probe batches (grep/file + run-probes) (`REQ <id> COUNT <c> MIN <m>` lines), routes unknowns to a
read-only judge (met requires a quoted file excerpt), and feeds unmet items back
as an ADDITIVE scope round. It can never conclude "done" with items uncovered:
exhaustion headlines `STOPPED: N requirements uncovered`.

- **v0.2.0 (tested, n=4): scores [0.56, 0.765, 1.0, 1.0] mean 0.831 — the only arm with two perfect runs — at 33-58 calls (mean 46, cheapest heavy arm); floor 0.56 is its open weakness.** Changes: a GENERAL runtime-evidence probe class (`type:"run"` — a
  bounded command whose output is the evidence: run the tests, invoke the CLI,
  curl the endpoint), a derived runtime-surface
  requirement for any artifact with a user-facing surface, specstd folded in as
  a `derive_standards` pin, budgets rebalanced (build 2, coverage 4).
- **Measured profile (0.1.0):** 109 llm calls · **312 KB (largest)** · 46 min ·
  2.9 KB/call · 0.825×3 (tightest quality spread) · most living-world motion.
- **Character:** the scope engine. Its 0.1.0 limit — static probes prove code
  presence, not runtime reachability — is exactly what 0.2.0's run-probes exist
  to close.
- **Known defect:** the judge lane mounts no tools inside while-subflows on
  current runtimes — judge-only/aesthetic items grade UNVERIFIED.
- **Use when:** completeness against the ask matters more than latency; thin or
  rich prompts (derivation covers the thin case).

## spec-std-coder — spec + field standards (folded into spec 0.2.0)
**Bundle:** `spec-std-coding:spec-std-coder` (standalone ≤0.1.3) · **Anatomy:**
spec with two-stage extraction: stated requirements first, then the FIELD is
identified ("browser canvas game", "CLI tool") and its standard practices are
derived as probe-able requirements up to `min_reqs` — stated always win the
cap, derived items carry `source: derived` + a practice note, enforcement is
identical. Rich prompts derive ≈nothing (mechanical, proven).

- **Measured profile:** rich-prompt parity with spec (0.794 vs 0.825 means);
  thin prompts: up to 2× built content (best run; ranges overlap at n=3) and the
  campaign's only 9-screen thin-prompt world; behavioral scores unchanged (the 4-check score doesn't reward scenes).
- **Character:** insurance against under-specified prompts — the user's 2-line
  ask is held to the field's 10-line bar.
- **Lesson it taught:** evidence lenses must be wide — narrow grep patterns and
  `*.js` globs turned present features into "evidence of absence" twice before
  the doctrine (alternation-rich patterns, glob `*`, count-0 routes to judge)
  landed.

---

## How they differ, in one table

| | loop count | who says "done" | memory | verification target | cost profile |
|---|---|---|---|---|---|
| react | 1 | model claim (+optional verify_command / human gate) | transcript | verify_command if set | minimal |
| ralph | N fresh | deterministic gate | workspace files | verify_command | very high |
| coder | rounds | independent verifier | transcript + round state | request (judged) | **best ratio** |
| multi | pipeline | verifier + gates | plan + repair history | request (judged) | highest |
| spec | 2 loops | coverage ledger | transcript + requirements vars | request (**itemized, probed**) | high, buys scope |
| specstd | 2 loops | coverage ledger | same + field standards | request + field practices | high, thin-prompt insurance |
