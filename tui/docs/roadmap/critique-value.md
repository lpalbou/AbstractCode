# Roadmap critique — value & strategic coherence (adversarial pass)

Status: critique of the three research lanes (2026-07-22), from a product-value
standpoint. Feasibility is a sibling critique's job; nothing here re-litigates
effort estimates or code claims. READ-ONLY: no code changed.

Inputs read in full: `lane1-engineering.md`, `lane2-ux-aesthetics.md`,
`lane3-observability-features.md`, `plan-interaction-model.md`,
`plan-entities-mcp.md`, `README.md`; reference positioning skimmed at source
(`~/projects/gh/codex` — the maintainer's own "Open Codex Memory" fork,
`opencode`, `pi`).

## TL;DR

1. The lanes are individually honest but collectively unshaped: ~45 findings,
   three internal rankings, no single story. The story is: **buy the coding-agent
   parity floor once, cheaply — then spend everything else on what only a
   durable gateway can do.**
2. Two users exist on paper; one exists today. Almost everything in lanes 2–3
   serves the human operator. The fleet seat is real-but-future — findings that
   serve it get built *when serve ships*, not before.
3. Challenged: the $ cost meter (no pricing data behind it — lane 3 admits it),
   the `/watch` entity live feed (duplicates the purpose-built observer app for
   an audience of one), the staged answer-reveal animation (streaming theater).
4. Championed: `finish_reason=length` truncation honesty (a real, live-verified
   trust defect buried inside a stats finding), `@file` mentions (lane 3
   under-ranked it at 12; lane 2 is right that it's P0), deny-with-reason,
   `/export`, and F4's false-"completed" (it breaks the queue's headline promise).
5. Phasing: Wave 1 "never lie, look premium" (all-S trust+face fixes), Wave 2
   "the coding floor" (diffs, @file, viewer), Wave 3 "mission control" (tree,
   runs board, history, summaries — the moat). Parking lot with named triggers.
6. Biggest collective miss: **attention pings** — a cockpit for runs that
   outlive your attention must call you back when it needs you. Nobody proposed
   it. Second miss: "what did the model actually see" inspectability.

---

## 1. Who is this for — and who is it *not* for yet

**User (a): the human operator at a terminal.** Today this is one person — the
maintainer — driving real 5h+ coding runs through the gateway daily (lane 3's
live evidence: a 485-step coding tree, four multi-hour waiting roots, a
token-accounting bug fixed the same day). His revealed preferences are on the
record in his own codex fork: depth over speed, subagent observability
(`/agents`), context observability (`/context`), durable memory. He is a codex
driver, so lane 2's "codex migrant sits down" success criterion is legitimately
calibrated — it is not a hypothetical persona.

**User (b): the headless fleet seat.** Real consumer (the Python
`abstractcode bridge` drives protocol-v1 children today), but the Rust `serve`
subcommand is **planned, not built** (interaction plan item 4). Until it ships,
(b) is a contract to honor, not a user to optimize for.

Findings that quietly optimize for users we don't have yet:

- **A fleet panel / `--fleet` view** — lane 3 already defers it correctly
  ("revisit after first real fleet use"). Endorsed.
- **NEW-1 `/watch` entity live feed** — the entity-curious operator exists, but
  he already has a purpose-built app for exactly this (abstractobserver's
  entity view, with the render-honesty rules NEW-1 proposes to import). The
  entities plan already stages it as v1.5 *after* chips+poller ship. Building a
  second live-memory renderer before the first one shows insufficient is
  observability nobody will watch. Defer on demand.
- **Jittered backoff, `include_metrics` surfaces, OTEL** — fleet-scale
  concerns; lanes 1 and 3 both self-deferred these. Endorsed.

Everything else in the three lanes serves user (a), with a small set that
serves both because the headless lanes inherit it (F1, F4, F7, budgets,
exit-code truth).

---

## 2. Value re-ranking (across lanes, by operator-perceived value)

Tier vocabulary: **must** = gates trust or the daily face, do first · **high**
= earns a roadmap slot now · **nice** = batch when touching the area · **skip**
= don't spend now; trigger named in §3/§5.

| Finding | Lane | Tier | User | One-line justification |
|---|---|---|---|---|
| UX-01 humanized tool cards | 2 | **must** | (a) | The default face of every session is JSON fragments; the sentence-builder already exists in-tree. Highest visible value per effort in the entire set. |
| UX-02 + UX-04a diffs / approval content | 2 | **must** | (a) | File edits are the highest-stakes agent act and today you approve them blind on 1 line. Trust + review speed in one. |
| UX-03 ≡ NEW-6 `@file` mentions | 2+3 | **must** | (a) | The core coding-agent prompt gesture; two lanes converged independently. Lane 3's rank 12 is wrong; lane 2's P0 is right. |
| Trust bundle: OBS-1 `finish_reason`+`attempt` · F4 false-"completed" · F7 silent SSE loss · F9 stale hint · F1 catalog self-heal | 1+3 | **must** | both | Five small fixes with one theme: the cockpit never lies. A truncated answer rendering as complete, a false Success draining the queue against a dead gateway, and a broken "reconnects automatically" promise are all trust defects, all effort-S. |
| UX-05a context-window % | 2 | **must** | (a) | Overflow surprises on local models are real; the one number every reference footer carries. (The $ half: see §3 — skip.) |
| OBS-2 `/tree` subrun tree | 3 | **high** | (a), later (b) | The identity feature: kills the tree-blindness bug class (wrong model label, "Done" while the tree worked) and is the natural instrument for goal loops and fleets. The maintainer added `/agents` to his own codex fork — this is his revealed preference. |
| OBS-3 `/runs` gateway board | 3 | **high** | (a) now, fleet console later | The only view no standalone tool can have; catches silently-stuck/spending runs (4 live examples found). Keep v1 slim: list · adopt · cancel — do not rebuild the web observer. |
| NEW-2 `/summary` (+`/ask-run` later) | 3 | **high** | (a) | The honest answer to "what did the 5-hour run actually do"; endpoints live; token cost labeled, never auto-fired. |
| NEW-3 `/export` + UX-08 full-content viewer | 2+3 | **high** | (a) | Two lanes converged; the app's own truncation labels currently point at a ledger you cannot open. Receipts culture is a maintainer value on record. |
| OBS-4 session run-history browser | 3 | **high** | (a) | Reopen any past turn for receipts; the rehydration path already exists — this is discoverability of a shipped capability. |
| F2 head-of-line blocking (bulk vs control) | 1 | **high** | (a) | Protects the two most latency-sensitive acts (approve, cancel) from waiting behind bulk fetches. User-felt in exactly the moments trust is on the line. |
| UX-06 type-to-filter pickers | 2 | **high** | (a) | A 342-row arrow-key wall in `/model` is daily friction for an operator who actually switches models. |
| UX-07 `?` overlay + honest footer | 2 | **high** | (a) | Cheapest discoverability win; also fixes a legend that truncates at the default width. |
| UX-09 session card with cwd | 2 | **high** | (a) | "Which directory is the agent pointed at" is first-order for a coding tool and currently appears nowhere. |
| UX-04b deny-with-reason | 2 | **high** (promoted from P2) | (a) | The deny that teaches — changes agent behavior, not just pixels. Underrated by its own lane. |
| NEW-4 token budgets (tokens only) | 3 | **high** | both | Unattended spend guard; cost discipline is a recorded maintainer ruling. Becomes must the day `/goal` or fleet seats go live. |
| OBS-7 wait/schedule visibility | 3 | **high** | (a) | A goal run pacing itself with `wait_until` must not look hung. Cheap now; mandatory before the goal bundle lands. |
| F3 unbounded image memory | 1 | **high** | (a) | Correctness hygiene at S effort; not experience-changing, just do it. |
| OBS-6 `/gpu` meter | 3 | **nice+** (promoted over artifacts) | (a) | On a local-inference deployment, "is the model actually computing" is the daily slow-call anxiety; the Python sibling had it. Rank it above the artifact browser for *this* operator. |
| F6 parallel boot rehydration | 1 | nice | (a) | Real, but the gateway is localhost today; value rises with remote gateways. |
| F5 entity convo truncation | 1 | nice | (a) | Slow leak, honest fix, batch with entity work. |
| UX-10a "writing answer…" activity | 2 | nice | (a) | Honest dead-air fix. (The staged reveal half: skip — §3.) |
| UX-11 drive-ratio words | 2 | nice | (a) | Legibility for the entity roster; batch with entities v1. |
| UX-12 composer `❯` · UX-14 fuzzy dropdown · UX-15 boot-notice fold | 2 | nice | (a) | Real polish, batch into Wave 1/2 where files are already open. |
| UX-16/17 self-truncating modals | 2 | nice (batch early) | (a) | P3 by lane, but `(persiste┃` makes the app look broken — cheap enough to sweep in Wave 1. |
| UX-13 read coalescing | 2 | nice (re-evaluate) | (a) | With humanized cards + details-fold shipped, the 12-card problem may shrink below M-effort worth; re-judge after UX-01. |
| OBS-5 artifact browser | 3 | nice | (a) | Save-to-disk half is the useful part; full browser waits for demonstrated need. |
| External editor (Ctrl+X class) | 2 §9 | nice | (a) | Heavy-user favorite, small; slot when composer work reopens. |
| Queued-prompt preview line | 2 §9 | nice | (a) | One-line close of a real gap; rides the queue work. |
| UX-18 glyph audit · UX-19 modal-edge audit | 2 | nice | (a) | Batch as one theme-polish pass. |
| $ cost meter (UX-05b) | 2 | **skip** | — | No pricing fields exist in the registry (lane 3 verified); deployment is mostly local. Trigger: a pricing table lands framework-side *and* paid providers become routine. |
| NEW-1 `/watch` entity live feed | 3 | **skip (defer)** | (a)? | Duplicates the purpose-built observer app; entities-plan chips/poller ship first. Trigger: post-entities-v1 demand ("the poller isn't enough"). |
| UX-10b staged answer reveal | 2 | **skip** | — | Animating an already-received answer manufactures the appearance of streaming. Theater; the honest activity label is the fix. |
| F8 tokio migration | 1 | **skip** | — | Lane 1's own deferral with named thresholds is correct. |
| Image paste · backtrack-fork · `!` shell passthrough · sidebar · leader-key · OSC-133 | 2 §9 | **skip** | — | Each needs a contract or a ruling first (attachment lane, session-seed, thin-client boundary) or is wrong for the form factor. Lane 2's own skips endorsed. |
| OTEL · audit tail · checkpoints · run comparison · fleet panel | 3 D | **skip** | — | Lane 3's own discipline; endorsed as-is. |
| F10 render audit | 1 | no action | — | Watch-items recorded; correct. |

---

## 3. Value honesty — challenges and champions

**Challenged (lower than presented):**

1. **The cost meter** (inside UX-05, P1). Lane 3's own evidence dismantles it:
   `model_capabilities.json` carries zero pricing fields, and the deployment is
   mostly local where cost ≈ tokens + time. A dollar figure without data behind
   it is the definition of low-value dressed as high — worse, it would be a
   fabricated number in an app whose brand is "render honesty from receipts."
   Context-% stays must; dollars wait for a pricing table.
2. **NEW-1 `/watch`**. The lane's rank 4 reflects observability enthusiasm, not
   demand. The abstractobserver entity view already exists, purpose-built, with
   the exact honesty rules NEW-1 would re-implement; the entities plan already
   stages chips+poller as v1 and the feed as v1.5. Two live-memory renderers
   for one watcher is a lane optimizing for itself.
3. **UX-10's reveal animation**. Block-by-block reveal of a fully-received
   answer imitates token streaming that isn't happening. This app's
   differentiator is that it never fakes; keep the "writing…" activity label,
   drop the animation, file the real token-delta lane as the gateway ask lane 2
   itself proposes.
4. **OBS-3 scope risk**. The runs board is high-value *as a slim board*. The
   moment it grows filters/metrics/facets it duplicates the web observer at
   TUI-render cost. v1 = rows + adopt + cancel, nothing else.
5. **Lane 1's F6/F2 framing**. Both are real, but "P1/P2 engineering" reads
   hotter than the user feels: F2's user-felt windows are narrow (post-switch
   approval, cancel-during-download) and F6 only bites on remote gateways.
   Slotted high and nice respectively — behind the visible waves, not ahead.

**Championed (higher than presented):**

1. **`finish_reason=length` honesty** — real and live-verified (lane 3 read the
   field on today's run records). A truncated answer rendering as a confident
   final card is the single worst lie the app currently tells, and it's buried
   as one bullet inside a stats finding ranked alongside tok/s meters. Unbundle
   it; ship it in the first week. Same for `attempt > 1` retry visibility.
2. **`@file` mentions** — lane 3 ranked it 12 of 14; lane 2 made it P0. Lane 2
   is right: it's the highest-frequency prompt gesture in a coding tool, both
   references ship it, and the Python sibling already had it. The cross-lane
   disagreement itself is evidence the lanes ranked by lane identity, not user
   value.
3. **F4 false-"completed"** — filed as P2 reliability, but it breaks the *queue's*
   headline promise ("each queued prompt runs after the current one succeeds")
   by draining the queue on a phantom success. Promote into the trust bundle.
4. **Deny-with-reason (UX-04b)** — P2 by lane, but it's the only finding in the
   set that improves the *agent's* behavior mid-run rather than the human's
   view of it. The serve protocol already carries deny reasons; the interactive
   lane should match.
5. **`/export` (NEW-3)** — S effort, converges with UX-08, and directly serves
   the operator's documented receipts culture (transcripts quoted in posts,
   commits, reviews daily).

---

## 4. Strategic recommendation

**Identity: codex is a brilliant agent in your terminal; abstractcode is
mission control for agents that outlive your terminal.**

The app cannot and should not try to out-codex codex on standalone-agent UX —
codex owns years of transcript polish, and opencode owns mass-market
onboarding. But the durable-gateway story only pays if the operator actually
*lives* in this cockpit, and today's default face (raw JSON cards, blind
approvals, no diffs, no `@file`) sends him back to codex for real work. So the
strategy has two legs, in order:

1. **Buy the parity floor once, cheaply.** Legible tool sentences, diffs,
   `@file`, context-%, `?` help. This is not "chasing codex" — it's the minimum
   below which the moat is unvisited. Crucially, most of it is projection-layer
   work on data the fold already holds (lane 2's central observation), so the
   floor is affordable. Draw the line there: no token-streaming imitation, no
   backtrack-fork, no shell passthrough, no sidebar. Every parity item beyond
   the floor has a "needs contract/ruling" or "wrong form factor" reason
   already recorded by the lanes themselves.
2. **Spend everything else on what only we can do.** Durable resume (shipped),
   the subrun tree, the gateway-wide runs board, session history + server-side
   summaries, wait/budget instrumentation, entity conversations, and the fleet
   seat. No reference tool can copy these — their runs die with the process.
   The maintainer's own codex fork is the strongest evidence this is the right
   bet: when he had full freedom over codex, what he added was subagent
   observability, context observability, and durable memory — exactly the
   depth wave, none of the polish wave.

One sequencing rule falls out: **the fleet seat ships before fleet
observability** (the serve plan is signed work; OBS-3 doubles as its observer;
a dedicated fleet panel waits for a real fleet). And one dependency honesty:
the flagship durable story — goal loops running unattended for hours — is
blocked on the flow seat's bundle. Until it lands, the durable story the app
can *demonstrate* is resume + queue; Wave 3 builds the goal loop's instrument
panel (tree, waits, budgets) so the story lights up the day the bundle ships.

---

## 5. The sign-able phase shape

Waves are value/risk phases, not calendar promises. Already-signed plans
(interaction model, entities v1) proceed in parallel; the waves below slot
around them, never duplicate them.

**Wave 1 — "Never lie, look premium."** All S or S–M; visible in a week of
work; near-zero regression risk (projection-layer + small guards).

- UX-01 humanized cards · UX-04a approval shows full content (v0: highlighted
  content block — true diffs arrive in Wave 2) · UX-04c "always allow" wording
- Trust bundle: OBS-1 `finish_reason` + `attempt` labels · F4 honest
  unknown-terminal · F7 counted SSE skips · F9 stale-hint clear · F1 catalog
  self-heal
- UX-05a context-% (warn/error ramp) · UX-09 session card with cwd · UX-07 `?`
  overlay + footer · UX-16/17 truncation sweeps · F3 image downscale
- **M1 attention pings** (from §7): bell/OSC-9 on approval-wait, ask-user, and
  run-end while unfocused.

*Outcome sentence: a codex driver sees sentences not JSON, reviews what he
approves, is never lied to about completion/truncation/connection, and the app
calls him back when it needs him.*

**Wave 2 — "The coding floor."** The M items that make it the daily driver.

- UX-02 diffs (client-computes only what it honestly has — reconcile lane 2's
  "derive when args carry find/replace" with lane 3's "never fabricate old
  bytes": render diffs from what the result carries, compute only
  find/replace-shaped edits, label the rest) + NEW-5 files-changed summary
- UX-03/NEW-6 `@file` mentions (entity-first, gitignore-aware, local-posture
  gated) · UX-06 type-to-filter pickers · UX-14 fuzzy `/` matching
- NEW-3 `/export` + UX-08 full-content viewer (+ **M2** `llm_call` payload
  inspect, §7) · UX-04b deny-with-reason · F2 bulk-command spawning
- OBS-6 `/gpu` (S, slips in)

*Outcome sentence: the operator stops keeping codex open.*

**Wave 3 — "Mission control."** The moat made visible; the goal-loop and fleet
instrument panel, ready before its tenants arrive.

- OBS-2 `/tree` · OBS-3 `/runs` (slim v1) · OBS-4 history browser + **M3**
  server-side session discovery (§7) · NEW-2 `/summary`
- NEW-4 token budgets · OBS-7 wait/schedule visibility · F6 parallel boot
- **M4** unattended-honesty bundle for exec/serve (§7) — rides the serve plan's
  build, inherits Wave 1's trust fixes into the headless lanes
- Entity-lane niceties batched here: F5, UX-11

*Outcome sentence: the durable/multi-agent story is something you can watch,
not something you read about in the architecture doc.*

**Parking lot (named triggers, not "never"):** $ meter (pricing table exists +
paid routes routine) · NEW-1 `/watch` (post-entities-v1 demand) · fleet panel
(first real fleet) · UX-13 coalescing (post-UX-01 noise persists) · tokio (F8's
own thresholds) · image paste (attachment demand) · backtrack (session-seed
contract) · shell passthrough (thin-client ruling) · reveal animation (never —
theater).

---

## 6. Top 5 daily-experience changers

Ranked by how many minutes of the maintainer's actual day they touch:

1. **UX-01 humanized tool cards** — every minute of every session; the product
   stops looking like a debug view.
2. **UX-02 + UX-04a diffs before approval** — every file-edit approval goes
   from blind trust to two-second review.
3. **UX-03 `@file` mentions** — every prompt that names code stops making the
   model guess paths.
4. **M1 attention pings** — every 5h run stops requiring terminal-checking; the
   durable story finally *feels* durable.
5. **OBS-2 `/tree`** — every wrapper-bundle run stops being opaque; the
   "Done-but-still-working" confusion class dies.

Honorable mentions: the trust bundle (fires rarely, but each firing prevents an
expensive wrong conclusion) and `/summary` (turns end-of-run archaeology into
one paragraph).

---

## 7. What all three lanes missed

1. **M1 — Attention pings (the biggest miss).** The entire product premise is
   runs that outlive your attention — yet nothing calls you back. A blocked
   approval on a 5h run today waits until the operator happens to look.
   Terminal bell + OSC-9/OSC-777 notification on approval-wait, ask-user, and
   run-terminal while the app is unfocused is small, form-factor-native
   (codex ships notify hooks), and is arguably the single most
   durable-identity-aligned feature in this whole document. Wave 1.
2. **M2 — "What did the model actually see."** This workspace's own debugging
   culture says the ledger is ground truth for what reached the model, and the
   recurring failure class is context surprises (replayed history, skills
   blocks, language flips). No lane proposed surfacing an `llm_call`'s request
   side — sizes, message count, declared tools/skills — from a cycle card. v1
   is cheap once UX-08's pager exists: open the payload the ledger already
   holds. This is the explainability view; `/ask-run` (NEW-2) complements but
   doesn't replace receipts.
3. **M3 — Server-side session discovery.** `/sessions` lists locally-remembered
   ids, so "attach from anywhere" is only true on the machine that started the
   session. The gateway knows the sessions; the durability promise needs the
   list to come from it. Without this, the headline claim is half-true.
4. **M4 — The unattended-honesty bundle, assembled.** The pieces exist across
   lanes (F4 exit-code truth, F7 silent-loss counter, NEW-4 budgets, the serve
   plan's `fold.failed` fix) but nobody owns the statement "the headless seat
   meets the same never-lie bar as the TUI." Name it as one deliverable so the
   fleet seat inherits Wave 1 rather than re-discovering it. Notable imbalance:
   headless is half the product's stated identity and received roughly one
   paragraph of research attention across three lanes.
5. **M5 — The goal-loop end-to-end as a product risk.** `/goal` ships dark
   behind the flow seat's unpublished bundle. That's fine as engineering
   sequencing, but no lane flagged it as the *strategic* dependency it is: the
   moat story's flagship demo is blocked on another seat. Track it on the
   roadmap explicitly; build its instrument panel (OBS-2/OBS-7/NEW-4) so the
   demo works the day the bundle lands.
6. *(Minor)* Onboarding is thinner than the polish findings suggest — but
   `login` → `doctor` → the empty state is a serviceable first-run path, and
   UX-09 closes most of the remaining gap. Noted, not a wave item.

---

## 8. Credit where due

The lanes were honest about their own limits, which makes this critique's job
mostly re-shaping rather than debunking: lane 1's F8/F10 verdicts and
"not broken — do not fix" list, lane 2's §9 skip list (sidebar, leader-key,
DeepSeek), and lane 3's Part D (OTEL, audit tail, checkpoints, comparison) are
all correct self-restraint and are endorsed as-is. The single systemic bias is
predictable: each lane ranks its own lane's currency highest — engineering
ranks invisible correctness, UX ranks visible polish, observability ranks new
views. The re-ranking in §2 is the correction.
