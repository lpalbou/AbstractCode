# R-Type benchmark at verified gpt-5.4 MEDIUM — final report

**Date:** 2026-08-04. **Matrix:** `untracked/rtype-medium2/` (15 new cells: 5 relay arms × 3)
merged with the reused `untracked/rtype-bench/` cells for codex / opencode / pi (already at
genuine medium). **Rubric:** frozen, sha `ab7fe7e35cb232aa`, identical for both datasets;
fixture suite PASS (margin ≥ 0.25, no attack over 0.5).

## Route verification (wire, not store)

**437/437 generation requests in the benchmark window carried `reasoning_effort=medium` on
`gpt-5.4`** — relay `inbound_request` log, all 30 sessions, residents excluded by session key.
No API key; relay subscription-backed; codex reused cells ran on the operator's ChatGPT
subscription at medium (as specified). This is the first matrix in the campaign where every
arm is wire-verified on the same model and effort.

Test–retest of the scorer on the new cells: 14/15 exact, max |Δ| 0.024, mean |Δ| 0.0016.

## Merged per-arm results (score = mean of two scoring passes)

| arm | harness | flow | n | mean | SD | range | wall s | LLM | tools | files real | KB real | clean exits |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| codex | codex-cli | native | 3 | **0.822** | 0.088 | 0.720–0.883 | 461 | ? | ? | 12–16 | 38–50 | 3/3 |
| tui-multi | abstractcode | multiagent-coding:multiagent-coder | 3 | **0.818** | 0.061 | 0.754–0.875 | 1242 | 45 | 136 | 13–16 | 71–80 | 3/3 |
| pi | pi | native | 3 | 0.790 | 0.067 | 0.714–0.835 | 450 | ? | ? | 11–14 | 32–39 | 3/3 |
| abstractcode-coder | abstractcode | coding-agent:coding-agent | 3 | 0.749 | 0.015 | 0.737–0.766 | 499 | ? | ? | 6–8 | 34–44 | **0/3** |
| tui-basic | abstractcode | basic-agent | 3 | 0.745 | 0.056 | 0.681–0.787 | **321** | **9** | 23 | 9–11 | 30–34 | 3/3 |
| abstractcode-basic | abstractcode | local loop | 3 | 0.743 | 0.119 | 0.614–0.848 | 638 | ? | ? | 10–15 | 33–69 | 3/3 |
| opencode | opencode | native | 3 | 0.713 | 0.138 | 0.556–0.814 | 465 | ? | ? | 13–17 | 40–48 | 3/3 |
| tui-coder | abstractcode | coding-agent:coder | 3 | 0.708 | 0.092 | 0.617–0.800 | **1902** | 64 | 151 | 6–16 | 39–76 | 3/3 |

`?` = client prints no counts (UNKNOWN, not zero). Files/KB exclude agent dot-dirs.
codex/opencode/pi walls are from the earlier matrix (different machine-load regime; indicative).

## Reasoning-off → medium, same arms, same rubric

| arm | off | medium | Δ |
|---|---|---|---|
| tui-basic | 0.591 | 0.745 | **+0.155** |
| tui-multi | 0.689 | 0.818 | **+0.129** |
| abstractcode-basic | 0.637 | 0.743 | **+0.106** |
| abstractcode-coder | 0.701 | 0.749 | +0.048 |
| tui-coder | 0.671 | 0.708 | +0.037 |

Every arm improved. The gain is largest where the loop has no self-correction (basic loops)
and smallest where a verifier already iterates (coder flows) — reasoning effort and
verification loops act as partial substitutes. abstractcode-coder's *product* changed
dramatically at medium: 13–17 raw files per cell vs 2 at off.

## Statistics (merged, n=3/arm)

- F(7,16) = **0.766** (critical ≈ 2.66) — arms not distinguishable.
- ICC(1,1) = −0.085; pooled within-arm SD = 0.087.
- MDD at n=3 = 0.199; observed spread of arm means = **0.113** — below the detection floor.
- 0 of 28 pairwise comparisons reach uncorrected p<0.05.
- Required n for best-vs-worst (0.113): ≈ **10/arm**.

**No ranking claim is supported.** The point-estimate order above is descriptive only. What
medium changed is the *cluster*: the relay arms closed most of the gap to codex (top-to-
bottom spread compressed from 0.231 to 0.113), and abstractcode/code-tui arms are no longer
the bottom of the table — tui-multi is #2 by point estimate, 0.004 behind codex.

## Why tui-coder is (point-estimate) last, and recommendations

1. **The coder flow's verifier gates buy nothing at medium and cost 6×.** 1902 s and 64 LLM
   calls vs tui-basic's 321 s and 9 calls, for −0.037 score. Its medium gain was the smallest
   (+0.037): the review loop and model reasoning solve the same failure mode (premature
   completion), so paying for both pays twice for one fix. *Recommendation:* revisit
   `DEFAULT_REVIEW_ROUNDS=3` / the coder default set on 2026-08-01 — that ruling was based on
   reasoning-off data (contaminated by the effort bug). At medium, prefer `basic-agent`
   (cost) or `multiagent-coder` (quality point-estimate); re-run the workflow comparison at
   n≈10 before re-fixing a default.
2. **tui-coder-2 (0.617) lost concrete checks, not style points:** R4 weapon-fire evidence 0,
   R2/R3 partial (0.68/0.30). Same for abstractcode-basic-1 (R4=0). The failure mode is
   games whose firing produces no detectable projectile evidence — real product defects.
3. **abstractcode-coder still crashes on every run** (`ValueError: wait_key mismatch`,
   abstractruntime; 0/6 clean exits across both matrices) while producing scoring products
   (0.749 mean). Fixing that crash is the cheapest quality win in the family. Backlog 0233's
   neighbour; already filed.
4. **Fix the `--agent coder` flow mismatch** in the Python client: it resolves to
   `coding-agent:coding-agent`, not `coding-agent:coder`, so the old-client-vs-new-client
   control on the *same* flow still has never been run.
5. `__session_memory__` folds run at `thinking=None` (relay default `none`) in every tui/
   abstractcode cell — parity-neutral for the benchmark, but if memory quality matters it
   should inherit like everything else now does.
6. For a real ranking: **n=10/arm** on this rubric, all relay arms, one machine-load regime.

## Provenance

- Wire audit: `scripts/verify_wire_route.py` + session-keyed breakdowns (resident-agent
  traffic excluded; see `docs/reports/2026-08-03-thinking-child-run-inheritance-handoff.md`).
- Fix chain that made this matrix possible: abstractcore `reasoning_effort` map+emit
  (2026-08-03, wire-verified), gateway child-run `thinking` inheritance (gateway seat,
  validated via probe run `43ed6ec6`: root + 4 children medium), Python client gateway path
  (13/13 medium on probe).
- Scores: `med_a.json`/`med_b.json` (session scratch), scorer runs 362 s / 547 s.
