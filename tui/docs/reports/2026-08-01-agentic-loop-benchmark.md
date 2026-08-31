# Agentic-loop benchmark — full report, conclusions, recommendations

**Date:** 2026-08-01 · **Route (every run):** `gpt-5.4`, reasoning `medium`, `endpoint:airelay`
(local subscription relay, `http://127.0.0.1:8317/v1`, no API keys) · **Client:**
`abstractcode-tui 0.4.0` (`exec`, headless, `--ungated --permissions all`, uncapped wall time)
· **Task:** the Zelda prompt (rich, ~10 stated requirements) plus a thin-prompt lane
("Create a small browser game… something fun with a canvas").

**Corpus:** ~60 valid agent runs across three matrices (pre-fix baseline 11, post-fix 15,
cross-client 9, review A/B 6, thin lanes, specstd lanes), every artifact scored by a
reproducibility-proven behavioral scorer, screenshot-reviewed (film strips, exploration
walks, drawn-text capture), and VLM-judged (fixed rubric, temp 0). Isolation: per-run
workspaces outside the framework tree, git-init'd against project-root walks, stray-write
and grader-read detectors, route verified per run from the runtime's durable store.

---

## 1. What was measured

Six loop designs, one client, one model, one prompt:

| arm | workflow | design |
|---|---|---|
| react | `react-coding:react-coder` | plain llm_call+tool_calls loop |
| ralph | `ralph-coding:ralph-coder` | fixed prompt, FRESH context per cycle, workspace-file memory |
| coder | `coding-agent:coder` | builder + independent verifier + deterministic gates |
| multi | `multiagent-coding:multiagent-coder` | scouts→planner→builder→doc pipeline |
| spec | `spec-coding:spec-coder` | coder's Loop 1 + requirements-coverage Loop 2 (DigitalArticle design) |
| specstd | `spec-std-coding:spec-std-coder` | spec + field-standards derivation for thin prompts |

Plus, earlier in the campaign: the `review_mode` A/B (react-agent, n=3/arm) and a
cross-client matrix (abstractcode 0.3.9, opencode 1.18.10, pi 0.83.0).

## 2. Reliability: the fixes worked

Pre-fix: 11/12 valid with three structural defects driving behaviour (marker-only Ralph
completion, fabricated canvas evidence, smallest-change-only repair charter).
Post-fix: **15/15 valid, zero discards**, no phantom-repair loops, no false completions.
The specstd lane additionally survived two harness-inflicted failures (a RestrictedPython
generator-expression in a fold; a same-second workspace collision between lanes) — both
diagnosed from ledgers and fixed; neither was an agent defect.

## 3. Efficiency: loop structure buys effort; only *checking* structure buys output

Post-fix means (3 runs each):

| arm | llm calls | KB | mins | **KB / llm call** |
|---|---|---|---|---|
| react | 10 | 32 | 5 | 3.2 |
| ralph | 109 | 32 | 22 | **0.3** |
| coder | 52 | 193 | 26 | **3.7** |
| multi | 120 | 84 | 57 | 0.7 |
| spec | 109 | 312 | 46 | 2.9 |

- **Ralph's fresh-context design pays ~9× react's tokens for the same output size.**
  The fixes made its cycles complete and record honestly; they cannot make re-derivation
  free. This is inherent, not a bug: workspace-file memory (PLAN/PROGRESS) is a lossy,
  expensive substitute for context.
- **coder converts calls to artifact best**; **spec buys the largest products ever produced
  here** (287–332 KB, 28–40 files, three-for-three) at near-coder efficiency.
- **multi's coordination overhead never pays for itself** — most calls, most tools, longest
  wall time, middling artifacts, and quality flat with the others.

## 4. Quality: what the screenshots and VLM settle

Behavioral scores overlap across arms (0.28–1.0, medians ~0.8); coder-2 posted the
campaign's only **1.0** (VLM 3/3/3/3). VLM judgments are flat at sprites ~3/5, animation
~2/5 for every arm. Per-arm distinctives from the visual evidence: spec shows the most
living-world motion (up to 24 autonomous regions) and NPC evidence; multi draws the most
dialogue text (27 strings); ralph has the widest variance (0.955 and a 0.28 dud in one arm).

**The wall no loop design broke: distinct screens = 1 in 14 of 15 post-fix runs.**
Even spec's 300 KB products play as a single screen — while its own coverage table
truthfully reports "maps → 22 matches". The maps exist **as code**; play never reaches
them. Verification-by-grep saturates at "the code mentions it".

This is the benchmark's central lesson: **static evidence proves presence; only runtime
evidence proves reachability.** (External confirmation that the wall is breakable:
opencode-3 produced 14 distinct screens, pi-2 a working quest system, both under the same
model — so this is a loop/verification gap, not a model ceiling.)

## 5. The review_mode A/B and the cross-client matrix (earlier phases, same campaign)

- `review_mode` on react-agent: verifier demonstrably ran (2× llm calls, 3× tools),
  **no detectable product-quality effect at n=3** (pre-registered NOISE verdict).
  Verification pressure without *spec-shaped* checks does not move quality.
- Cross-client: abstractcode-tui (no-review) median 0.922 vs abstractcode 0.752,
  opencode 0.765, pi 0.921 — **all arms overlap; no client separates**. The original
  "abstractcode is a much better coder" premise was not supported; abstractcode is the
  most *consistent* (range 0.077), pi the most bimodal (two near-best + one 0.0 broken
  delivery). Confound noted: abstractcode's local path silently drops `reasoning=medium`
  (abstractcore has no thinking mapping for this model) — its number is a floor.

## 6. specstd: derived field-standards (thin prompts)

Design: identify the field, derive its standard practices as probe-able requirements up to
`min_reqs`, stated always first, enforcement parity, `source` column in the report.
Validated behaviours: thin prompt → 2 stated + 8 derived ("browser canvas game");
rich prompt → zero derivation (mechanical, not asked). Iterations forced by evidence:
0.1.1 widened probe patterns (alternations); 0.1.2/0.1.3 widened GLOBS (a `*.js` glob
read a single-file inline-JS game as missing input/loop/collision that were present
18/14/20 times — count-0 from a narrow lens is not absence).

Results (n=3 per lane, gpt-5.4 medium):

| lane | runs (score · screens) | mean | spec baseline |
|---|---|---|---|
| Zelda (rich) | 0.907 · 1 / 0.650 · 1 / 0.825 · 2 | 0.794 | 0.825 (parity as designed — derivation ≈0 on rich prompts) |
| thin | 0.410 · 1 / 0.410 · 1 / 0.410 · 9 | 0.410 | 0.470 |

- **Behavioral scores: no detectable improvement** on either lane at n=3 — the thin-lane
  means overlap (0.410 vs 0.470) and the rich lane is parity, as the design predicts.
- **Artifact scope up on thin prompts**: specstd built 26–105 KB / 8–12 files vs spec's
  26–76 KB, and one specstd thin run produced a **9-distinct-screen world** — the only
  multi-screen thin-prompt game in the campaign. Derived standards demonstrably push
  more built structure; the 4-check behavioral score does not reward scenes/persistence,
  so the delta shows in the visual evidence, not the score.
- NPC evidence 3/3 on the Zelda lane (vs mixed elsewhere).
- Verdict: the mechanism works (extraction, enforcement parity, honest headlines), and its
  quality payoff is bounded by the same two limits as spec — static evidence class and the
  judge-tool outage. Fix those before judging the concept further.

Open defect (0.1.4): the judge second-opinion lane engages but its agent session mounts no
read-only tools, so judge/aesthetic items grade UNVERIFIED instead of being rescued or
refuted. Honesty held (no unquoted rescue fired); capability did not.

## 7. RANKING — which loop is the better coder, and why

Verdict synthesized across five axes: quality (behavioral + VLM + visual facts), quality
FLOOR (worst run), efficiency (KB/llm-call), completeness (files/scope/coverage honesty),
reliability (variance, failure modes). Zelda task class, gpt-5.4 medium, n=3.

**1. coder (`coding-agent:coder`) — the best coder today.**
Only 1.0 of the campaign; best floor of the heavy arms (0.795); best calls-to-artifact ratio (3.7 KB/call; note KB/call understates fresh-context arms — in tokens spec 0.1.0 cost ~2.6× coder); NPC evidence in 3/3 runs; half the cost of every
other heavy arm. Why it wins: ONE builder kept honest by an INDEPENDENT verifier with
deterministic gates — maximum checking per token, no coordination overhead.

**2. spec (`spec-coding:spec-coder`) — the most complete builder; the design to invest in.**
Matches coder's quality (0.825 ×3 — tightest quality spread of all arms) while building
2-10× more game (287-332 KB, 28-40 files, most living-world motion, and the only honest
coverage table). Costs 2× coder. Why second and not first: today its extra scope is not yet
extra PLAYABLE scope (screens=1 — the reachability wall). The moment Loop 2 gains runtime
probes, this design should take first place — nothing else has its completeness mechanism.

**3. specstd (`spec-std-coding:spec-std-coder`) — spec plus thin-prompt insurance.**
On rich prompts it IS spec (by design, measured parity). On thin prompts it is the only
arm that raised the bar: 2× built content and the campaign's only multi-screen
thin-prompt world (9 screens). Ranked third only because its edge applies to one prompt
class; pick it whenever user prompts are under-specified.

**4. multi (`multiagent-coding:multiagent-coder`) — capable, uneconomical.**
Quality indistinguishable from coder/spec (0.795-0.825), most dialogue text, the only
Zelda arm to touch 3 screens — at 2.3× coder's calls and wall time, 0.7 KB/call. The
scouts+planner add cost, not quality, on greenfield tasks. Keep for brownfield (real
codebases to scout); do not default to it for greenfield.

**5. react (`react-coding:react-coder`) — the efficient baseline, not a contender.**
10 calls, 5 minutes, playable-but-shallow output (0.60-0.81, 32 KB). Right choice for
small tasks; structurally unable to reach large-scope quality (nothing pushes it past its
first satisfactory draft — the premature-completion pattern this campaign started from).

**6. ralph (`ralph-coding:ralph-coder`) — wrong tool for this task class.**
Worst efficiency (0.3 KB/call), worst dud (0.28), widest variance, 10× react's cost for
react's output size. Fresh-context cycling spends most of each cycle re-deriving state.
Its honest niche: tasks that exceed context (very long repairs), not greenfield builds.

**Thin-prompt axis:** specstd > spec > rest (untested; expected to trail — nothing else
derives missing requirements).

## 8. IMPROVEMENTS — per design, concrete

**spec → 0.2.0 (highest-leverage work in the programme):**
1. **Runtime-probe evidence class** in Loop 2: a `browser_probe`-driven scripted walk
   emitting `SCREENS <n>`, `INTERACT <id> <ok>`, `HUD <changed>` lines as gate facts.
   Requirements like "vast maps" then demand *visited* screens. The machinery exists and
   is proven (the visual-review driver); it moves inside the loop as a gate.
2. **Auto-derived reachability requirement** for game/UI fields: "every declared
   scene/screen reachable via input from the start state" — closes the code-vs-play gap
   generically, not per-prompt.
3. **Budget rebalance:** Loop-1 rounds 3→2, spec rounds 3→4 — the measured failure mode
   is uncovered scope, not broken builds (15/15 built and ran).
4. Absorb specstd as a `derive_standards` pin (one bundle, no drift; rich-prompt behavior
   already identical).

**coder → adopt, then converge:** ship coder as today's default coding workflow; when
spec 0.2.0 lands with runtime probes, spec becomes the default and coder remains its
Loop 1. One lineage, not two.

**specstd → 0.1.4:** judge-session tool mounting (runtime fix below) + keep the widened
glob/pattern doctrine; then merge into spec per above.

**multi:** add a skip-scouts branch when the workspace is empty (greenfield) — the scouts
are its cost center and contribute nothing there; adopt spec's Loop 2 as its verify stage.
Re-benchmark on brownfield before further investment.

**ralph:** reposition for context-overflow repair work. If kept for builds: inject the
previous cycle's PROGRESS tail + workspace listing INTO the cycle prompt (fresh context
with a warm start — kills the ~5-step re-derivation tax), and stop on two consecutive
verified-green cycles instead of the budget.

**react:** leave as the cheap lane; its client-side verifier (review_mode) measured no
effect, so do not spend more there.

**Cross-cutting (owners):** judge/agent nodes inside `while` subflows must mount their
`tools` pins (abstractruntime); thinking-control mapping for OpenAI-compatible relays —
apply or fail loudly (abstractcore); backlog 0232 sandboxing + fail-loud workspace clamp
(abstractgateway); child `_runtime`/`_limits` inheritance (abstractruntime, standing).

## 10. Conclusions

1. **Verification structure, not coordination structure, is what improves coding agents.**
   coder (one builder + independent verifier + gates) dominates efficiency; multi
   (scouts/planner pipeline) dominates cost. Spend tokens on *checking against the ask*,
   not on more agents.
2. **The DigitalArticle two-loop design is validated as the scope engine** — spec reliably
   converts requirement gaps into added content (2–10× more artifact than plain loops at
   similar call counts) and never claims done with items uncovered.
3. **Its current limit is evidence class, not concept**: grep/file probes verify code
   presence; the unreached-maps wall shows the missing tier is **runtime probes** —
   scripted play (screen-transition counting, interaction checks) as first-class gate
   evidence inside Loop 2.
4. **Three evidence principles are now empirically grounded** (every major failure this
   campaign violated one): (a) missing evidence is UNKNOWN, never false; (b) checks must
   compare against the *request*, not just execution; (c) repair charters must permit
   additions, or scope failures lock at prototype scale.
5. **Fresh-context loops (Ralph) are an anti-pattern for this task class** at 10× the
   token cost for equal output; their honest niche is contexts too long to carry, not
   tasks a context can hold.
6. **Model quality is not the binding constraint** — the same model produced 14-screen
   worlds under a different client. The loop and its verifier are the constraint.

## 11. Recommendations (per-package, unchanged)

**abstractflow (bundle owner)**
- Promote the three evidence principles into the shared verifier machinery (three-state
  everywhere; request-anchored MATCHES; additive repair) — shipped in coding-agent@0.2.6 /
  multiagent-coding@0.0.17; carry into every future coding bundle.
- **spec-coding 0.2.0: add the runtime-probe class to Loop 2** — browser_probe-driven
  scripted walk emitting `SCREENS <n>`, `INTERACT <ok>` lines as gate evidence; a "vast
  maps" item then requires *visited* screens, not code mentions. This is the highest-value
  next increment in the whole programme.
- spec-std 0.1.4: fix judge-session tool mounting; keep the glob doctrine.
- Ralph: document the token-tax envelope; default it off for tasks that fit context.

**abstractruntime**
- Child-session tool mounting for agent-node judges (the spec-std judge outage) — verify
  agent nodes inside `while` subflows receive their `tools` pins.
- The `review_mode`/`_limits` inheritance row from the earlier report stands (flow-graph
  children still don't inherit).

**abstractcore**
- Add the thinking-control mapping for OpenAI-compatible relays (abstractcode's silently
  dropped `reasoning=medium` — a routing directive must apply or fail loudly, not warn
  and proceed).

**abstractgateway**
- Backlog 0232 stands (silent workspace-root clamp → fail loudly; `execute_command`
  sandboxing; identity-based path containment).

**abstractcode-tui (this client — shipped this campaign)**
- `--param` (workflow input pins), `--review/--review-rounds`, project-context injection,
  bundle-only workflow resolution + honest refusals, ADR-0027 timeout compliance,
  iteration-budget honesty (⚠ stopped ≠ ✓ done, exit 125). Remaining: B4 + three lesser
  audit items (tracked).

**Benchmark methodology (keep)**
- Pre-registration + any-discard-voids; route verified from the runtime store, never from
  intent; workspaces outside the tree, git-init'd; grader unreachable from agents (the
  opencode benchmark-capture incident); infra failures retry the cell, never consume
  verdicts; probes judged against RUNTIME behaviour with seeded RNG + virtual clock
  (byte-reproducible across 45-run validation).

## 12. Where everything lives

- Matrices: `untracked/workflow-bench*` (post-fix, specstd, thin lanes),
  `untracked/_baseline-prefix-*` (pre-fix), `untracked/client-bench` (cross-client),
  `untracked/zelda-ab` (review A/B).
- Per-run: `runs.json` (route, verdicts, wire keys), `scores.json`, visual evidence under
  `untracked/visual-review/<slug>/` (shots, film strips, facts.json, VLM verdict).
- The live matrix page (play every game, per-run facts, review sheets):
  **http://127.0.0.1:8899/**.
- Flow fixes shipped: `ralph-coding@0.1.2`, `coding-agent@0.2.6`,
  `multiagent-coding@0.0.17`, `spec-coding@0.1.0`, `spec-std-coding@0.1.3`.

## 13. v2 matrix — the improved orchestrations, tested (n=2, gpt-5.4 medium, parallel×3)

6/6 VALID; isolation held with three concurrent runs throughout.

| arm | v1 (n=3) | v2 (n=2) | delta |
|---|---|---|---|
| **spec 0.2.0** | 109 llm · 312 KB · 0.825×3 | **58/33 llm · 273/185 KiB · 1.0 / 0.765 (score order = rep order)** | **calls/tokens −60-70% (cause not isolated among round-cap 3→2, aesthetic-only-unmet stop, run-probes), KB/call 2.9→5.2, second 1.0 of the campaign** |
| **ralph 0.2.0** | 109 llm · 32 KB · 0.28–0.955 | 85/66 llm · 47/37 KiB · **0.385 / 0.795** (the 85-call run is the dud — audit-corrected pairing) | calls −31%, KB +34%; **variance persists** (another dud) |
| **multi 0.0.18** | 120 llm · 84 KB · 0.795–0.825 | 106/133 llm · 0.822 / 0.765 | greenfield fast-path fired (scouts skipped); builder rounds still dominate cost |

Mechanism evidence from the run stores (audited): run-type probes were DEFINED in 3 of 4
v2-era spec runs and EXECUTED with recorded evidence in 2 of 4 (an entry-exists check; a
node DOM check) — present and functional, not yet universal; new honest stop `aesthetic-only-unmet` prevents
burning scope rounds on judge-only leftovers. ralph v2's smoke proved warm-start (cycle 2's
prompt carries cycle 1's PROGRESS entry verbatim) and settled-two-green (2/4 cycles, 8 llm
calls); under Zelda load its call count dropped ~31% but the dud-run failure mode remains.
multi 0.0.18's fast-path proof: "scouting: skipped — greenfield", 24-call smoke; on Zelda
the savings are eaten by builder/verify rounds (and one run npm-installed 38.2 MB of dependencies — product-proper size for that cell
is 84.0 KiB after excluding node_modules; all published multi means use product-proper).

**The screens wall still stands in v2** (1 distinct screen everywhere): the general
run-probe class landed and fired, but one entry-level probe per run does not exercise
scene reachability — and per the operator's generality mandate nothing game-shaped was
added. Closing it generically now needs the browser_probe capability itself to grow a
GENERAL interaction/state-delta protocol (abstractcore), which run-probes can then invoke
for any UI-class artifact.

## 14. FINAL RANKING — consolidated, n=4 v2-era (audited)

Full v2-era results (12/12 VALID): spec calls 33-58, scores [0.56, 0.765, 1.0, 1.0]
mean 0.831 · ralph calls 55-112, scores [0.385, 0.636, 0.75, 0.795] mean 0.641 ·
multi calls 106-141, scores [0.647, 0.765, 0.822, 0.825] mean 0.765.

**1. coder (`coding-agent:coder`) — the dependable default.**
Highest floor of any arm in any era (0.795; the only heavy arm with no sub-0.6 run
across the campaign), mean 0.868 (n=3), ~52 calls. When failure is expensive, this
is the pick. Open improvement: port the additive repair charter (it lives in
multi/spec, not here — corrected claim).

**2. spec 0.2.0 (`spec-coding:spec-coder`) — the high-ceiling challenger.**
The only arm with TWO 1.0 runs; lowest call count of the heavy arms (mean 46);
every run faster than the fastest coder run; the coverage ledger and honest
stop states are unique to it. n=4 also exposed its cost: a 0.56 floor — variance
coder does not have. Choose it when completeness against the ask matters and a
retry is acceptable; do not default to it until the variance source is understood
(the 0.56 run is the next forensic target).

**3. multi 0.0.18 — consistent, expensive.**
Tightest v2 spread after coder (0.647-0.825) at the highest cost (106-141 calls).
Greenfield fast-path works but changes Zelda-scale totals by nothing — builder
rounds dominate. Brownfield value remains an untested hypothesis.

**4. specstd — merged into spec** (`derive_standards` pin); thin-prompt value stands.

**5. react — the cheap baseline.** Fine for small tasks; nothing checks in the
benchmark's ungated posture.

**6. ralph 0.2.0 — last for this task class, now with n=4 confidence.**
Warm-start and settled-stop verifiably work (cycle prompts carry prior PROGRESS;
early stop fires), calls fell ~30% — and quality still spans 0.385-0.795 with a
sub-0.5 run in both eras. Fresh-context instability survives the fixes.
Context-overflow niche stays an untested hypothesis.

**The one structural result that survives every audit:** loops that VERIFY against
the request (coder, spec, multi's verify stage) occupy every top position; loops
that don't (react) or that forget (ralph) occupy the bottom. Within the verifying
loops, tokens spent on coordination (multi) buy less than tokens spent on
checking (coder) or on itemized coverage (spec).

## 15. Scope of validity (adversarial review, applied)

All rankings derive from one prompt family (one rich Zelda prompt plus one thin
canvas-game prompt), one model+route (gpt-5.4 medium via a local relay), one client,
greenfield-only workspaces, and n=2-4 per cell; no brownfield, context-overflow, or
non-game task was run, so the ralph and multi "use-when" niches are hypotheses, not
measurements. Efficiency is reported as KB per llm call, a proxy that understates
fresh-context arms and breaks on dependency-installing runs; token counts exist in
runs.json and disagree with KB/call on two comparisons (spec-vs-coder, ralph-vs-coder).
Behavioral scores rest on 4 checks that do not reward scenes, persistence, or
reachability — the axis this campaign says matters most; the VLM judge is a single model
on a fixed rubric with no human calibration. Three P0 mechanics errors found in the first
edition of the cards (coder's repair charter, a nonexistent syntax gate, react's
"nothing checks") were corrected against flow sources on 2026-08-01.

## 16. Independent data audit (applied)

A read-only auditor recomputed every published figure from runs.json/scores.json/facts
and the runtime stores. Clean: v1 aggregates match to rounding; **route integrity 56/56**
(gpt-5.4/medium and era-correct bundle versions on every store-verifiable run); speed is
plausible (v1 averaged 3.08× concurrency, peak 4; v2/v2b ≤3; no archived product predates
its run). Corrected here: a v2 ralph score/call pairing error; a kB-vs-KiB inconsistency;
"run-probes fired" softened to defined 3/4, executed 2/4; multi v2 sizes restated
product-proper. Known limits: client-bench rows carry no run ids (route rests on
provenance.json only); the pre-fix baseline's runs.json paths point at a since-reused
directory (its LOCAL product copies are verified byte-consistent — use those, never the
paths). Defensible v2-era statistics: spec-v2's call reduction vs spec-v1 is the one
non-overlapping comparison (33–58 vs 81–130, n=4 vs 3); spec-v2 vs coder-v1 does NOT
separate on quality (0.765–1.0 vs 0.795–1.0) or calls (33–58 vs 48–55) — only on wall
time (every spec-v2 run faster than the fastest coder-v1); ralph is the only arm with a
sub-0.5 run in both eras (2/5 pooled).

## 17. Default-workflow confirmation: coder (TUI) vs abstractcode 0.3.9 react

Operator ask: confirm the new default (`coding-agent:coder` on abstractcode-tui) against
the ORIGINAL reference — old abstractcode with its local react loop — same Zelda prompt,
same relay, n=3, scored/reviewed by the same instruments. (First attempt VOID: the relay's
quota window closed mid-matrix and the harness mislabeled two instant "model not found"
deaths as VALID off leftover files — bench_clients gained the infra-classify+retry belt
and the matrix was rerun clean.)

| | scores | mean | floor | screens |
|---|---|---|---|---|
| abstractcode 0.3.9 react | 0.381 / 0.690 / 0.970 | 0.680 | **0.381** | 1-2 |
| **coder v1 (TUI default)** | 0.795 / 0.808 / 1.000 | **0.868** | **0.795** | 1 |

Every coder run beat two of three react runs; react's best (0.97) shows the plain loop CAN
match coder — once — while its floor (0.381, behav 1/4) is the premature-completion
pattern the default exists to prevent. Confirmation VERDICT: the default holds — coder's
floor is 2× react's, mean +0.19, at n=3 with ranges overlapping only via react's best run.
Standing caveat: abstractcode cannot apply reasoning=medium (known abstractcore gap), so
its numbers are a floor; even granting headroom, a default is chosen on floors, and the
floor gap is decisive.

## 18. Client changes landed with this confirmation

- Default workflow: saved pref → `coding-agent:coder` → basic-agent (test-pinned).
- The operator's stale saved pref (multiagent) was updated to coder — saved prefs
  deliberately shadow defaults, which is why the picker showed multi.
- `/workflow` picker: 120-col modal (was 84), descriptions readable (first sentence,
  74-char budget), and the list filtered to CODING-pickable flows — entity-lane and
  *-test entrypoints are hidden (fold recognition keeps the full set); filter applied
  identically to rows, live refresh, and activation indexing, pinned by the mid-open
  arrival test.

## 19. ISOLATION: is the gain the client, or the workflow?

Operator ask: run the OLD client (abstractcode 0.3.9) on the SAME coder workflow.
`--agent coder` resolves through the catalog to coding-agent's `coder` entrypoint
(`workflow_agent.py:331-335`) but executes on abstractcode's OWN embedded runtime — so
this isolates the WORKFLOW DESIGN across two clients and two runtimes. n=3, same prompt,
same relay, same instruments. (All three runs hit a closed quota window first and were
auto-retried by the infra belt — the classify-and-retry fix earning its keep.)

| arm | scores | mean | floor |
|---|---|---|---|
| abstractcode 0.3.9 + **react** | 0.381 / 0.690 / 0.970 | 0.680 | 0.381 |
| abstractcode 0.3.9 + **coder** | 0.694 / 0.925 / 0.970 | **0.863** | 0.694 |
| abstractcode-tui + **coder** | 0.795 / 0.808 / 1.000 | **0.868** | 0.795 |

**The workflow is the cause; the client is not.** Same client, swapping react→coder:
+0.183 mean (0.680→0.863) and floor +0.313 (0.381→0.694). Same workflow, swapping
client: **+0.005** (0.863 vs 0.868) — indistinguishable at n=3. The improvement this
campaign chased lives entirely in the orchestration, and it TRANSFERS: any client that
can run the bundle gets it. abstractcode's coder arm also produced the campaign's first
4-distinct-screen coder artifact (rep3).

Practical consequences: (a) the TUI's new default is correct but not privileged — the
same win is available to abstractcode users today via `--agent coder`; (b) all remaining
loop investment (spec's runtime probes, the additive charter port) pays out across every
client at once; (c) claims of "client X is a better coder" should be restated as "client
X defaults to a better workflow" — the original premise of this whole investigation.
