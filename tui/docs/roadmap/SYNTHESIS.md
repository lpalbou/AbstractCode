# SYNTHESIS — the corrected verdict and path forward

Status: definitive synthesis, 2026-07-22. Inputs: `ROADMAP.md`, the three
lanes (`lane1-engineering.md`, `lane2-ux-aesthetics.md`,
`lane3-observability-features.md`), both critiques (`critique-feasibility.md`,
`critique-value.md`), the two post-reaction investigations
(`review-python-regression.md`, `review-roadmap-audit.md`), the three
in-flight signed plans (`docs/design/plan-interaction-model.md`,
`plan-entities-mcp.md`, `tier-policy-agora-facts.md`), plus a **fresh
live-header ground-truth capture performed for this document** (release
binary, pyte VT screen, live gateway, 120×36 / 100×30 / 200×50 — frames
reproduced in §2; `review-current-state.md` was never written, so nothing
here leans on it). This document supersedes `ROADMAP.md`'s wave order; it
keeps the roadmap's items, ids, honesty guardrails, and risk register.

---

## 1. Executive verdict

**The maintainer is right about the thing that matters and wrong about one
word.** Right: as a daily tool today, the Python `abstractcode` is better —
it carries ~60 commands to our 24, live 10 Hz generation feedback where we
go dark between ledger records, a context-% meter against the model's real
window, per-answer token/tok-s footers, per-tool durations, expandable
transcript detail, markdown tables — and its persistent chrome carries
facts while ours carries blank space (measured below: at rest our frame is
75–90% empty rows and the header row is two-thirds blank at 120 cols).
Wrong on "optimized": the Rust engine is architecturally ahead (damage
tracking, 0-byte idle frames, keyed O(changed) feed updates — test-pinned),
while the Python app's smoothness is a hand-built cache tower added
incident by incident; what he is perceiving as "unoptimized" is *missing
information*, not missing performance. And right about a third thing he
didn't say directly: **my roadmap aimed at the wrong baseline** — it
benchmarked codex ~96 times, never once captured a frame of his own tool,
had no item for at-rest density, and deferred the two observability features
(context %, subrun tree) his own codex fork proves he values most. The
corrected plan below re-baselines against the predecessor, inserts a
"Parity + Presence" wave first, files the token-streaming gateway ask the
old roadmap deliberately refused to file, and keeps everything that was
genuinely right: the trust bundle, the honesty guardrails, the moat waves,
and the three in-flight signed plans.

---

## 2. The sparse header — ground truth and root cause

### What the screen actually shows (captured this session)

Release binary, live gateway, isolated prefs, pyte-rendered. At 120×36,
t=4s after launch:

```
  0 | ▲ AbstractCode  basic-agent  ·  gateway defaults (lmstudio · ornith-1.0-35b)                        …synthesis-probe ●
  1 |
    | (rows 1–11 fully blank)
 12 |                                                     ▲ AbstractCode
 14 |                             describe a task below — the agent runs durably on the gateway
 16 |                          /help commands · /workflow agents · /model providers · /theme looks
 18 |                                                rendered by AbstractTUI
 20 |                         · session acode-synthesis-probe · durable memory lives on the gateway
 21 |                 · workspace: gateway-managed — files land in the gateway's workspace (details: /help)
    | (rows 22–33 fully blank)
 34 |▐                                                                                                                      ▌
 35 | enter send  esc esc cancel  ctrl+d details  pgup/dn scroll  ctrl+t theme  /help comm… Dark (Abstract) · 127.0.0.1:8080
--- density: 9/36 rows carry content; 27 rows fully blank ---
```

At 200×50 (a full-screen terminal — the maintainer's likely condition),
t=5s, before the route resolves:

```
  0 | ▲ AbstractCode  basic-agent  ·  gateway defaults                                    …synthesis-probe ●
    |                              ^ left run ends col 49        ~132 blank columns ^     ^ right cluster
--- density: 5/50 rows carry content; 45 rows fully blank (90%) ---
```

That second frame **is the screenshot**: on a wide terminal the left run
occupies ~50–77 columns, the session id + orb sit at the far right, and the
eye landing anywhere right of center sees exactly "a session id and a green
dot floating on blank space."

### Root cause — a content-budget gap PLUS a redraw defect (corrected post-synthesis)

CORRECTION (folding in `review-current-state.md`, the last investigation to
land): my first pass here concluded "no paint bug — the left side always
renders." That is true on a CLEAN first paint but INCOMPLETE, and the fuller
truth better explains the maintainer's *exact* screenshot (a blank bar with
only a faint session id + green dot, and nothing on the left at all). There
are TWO root causes stacked:

**(A) A redraw defect that reproduces the screenshot byte-for-byte.** After
any EXTERNAL screen clear (Cmd+K / terminal clear / a resize race) plus one
passing toast, the header stays blank *forever*: the engine repaints only
DAMAGED cells, the header's reactive signals are fully STATIC during a
pinned-route run (nothing re-triggers the header's `dyn_view` once workflow/
route/session settle), and the app has **no Ctrl+L / redraw affordance** —
which the Python app *did* have (`abstractcode/fullscreen_ui.py:3494`). A
wiped 271×68 screen recovered only a spinner glyph + two elapsed digits
after 20s. AUDIT CORRECTION (2026-07-23, independent verifier): the defect
is real and pty-proven, but attributing the maintainer's photo to it is a
HYPOTHESIS, not established fact — his frame (271×68) is not in the repo,
no evidence shows his terminal experienced an external clear, and the
fully-painted-but-27%-full header is a competing sufficient explanation.
Also: the old app's Ctrl+L (`fullscreen_ui.py:3494`) was a CONTENT clear
through another model-diff renderer — whether it recovered from an
external wipe was never tested; "the Python app recovered with one
Ctrl+L" is unverified. The defect stands on its own evidence either way. Related dead-on-arrival defect: the 0.3.0 phase-aware
composer placeholder NEVER renders — the engine paints placeholders only
when a `TextArea` is UNFOCUSED (`abstracttui/.../textarea.rs:404`) and the
composer is `.autofocus()`, so the composer is two strokes around a blank
line.

**(B) The content-budget gap (below) — why even a correctly-painted header
is 24% full** at his 271-col width (57 cols of content, a ~193-col void,
session id at 2.77:1 contrast, below the 3:1 floor). Both are real; (A)
makes it *empty*, (B) makes it *sparse even when full*.

The left side DOES paint correctly on a clean frame (wordmark accent,
workflow, route with the honest `gateway defaults (lmstudio · ornith-1.0-35b)`
resolution upgrading at ~4s — though route resolution lags 10.3s behind six
serial provider backfills, `runner.rs:424-457`). The right-measured-first
clip (`chrome.rs:95-108`) is not the defect. The content-budget half:

1. `chrome.rs:74` — `canvas.fill(rect, ' ', …)` then the header prints
   ~50–82 chars of content and *nothing else*. No cwd, no workspace mode,
   no counts. Everything between the route and the session id is
   deliberate blank (`chrome.rs:110-141`).
2. `chrome.rs:312-316` — the idle activity strip renders an **empty
   element** on a fresh session (`totals.runs == 0 && queued == 0` →
   blank row).
3. `src/ui/transcript_view.rs:721-783` — the empty state is 5–6 centered
   lines (wordmark **duplicated** — it is already in the header row), not
   a fact card.
4. `chrome.rs:623-710` — the status bar carries a key legend + theme +
   gateway host: useful once, then permanent dead weight; it truncates at
   120 cols (`/help comm…` in the capture) while carrying **zero numbers**.

Net: at rest the persistent chrome shows *no context, no tokens, no skills,
no MCP, no GPU, no directory* — while the Python predecessor's law, written
in its own code after its own adversary review, is the opposite:
*"the banner scrolls away; the footer is the only always-visible surface"*
(`abstractcode/react_shell.py:1820-1822`, verified this session), and its
footer renders `Context: 41,203/262,144 tk (16%) | skills 2 | mcp 1 |
GPU ▓▓▓░ 28%` plus clickable provider/model dropdowns at all times. The
Rust port re-learned the banner half (a boot card was planned as UX-09,
ranked 9th of 14) and skipped the persistent-facts half entirely. That
asymmetry is the maintainer's screenshot, and no item in the signed
ROADMAP.md changes it — Wave 1 could have shipped perfectly and the idle
app would look identical to the frame above.

---

## 3. What the port LOST vs Python (the regression, verified)

Full receipts in `review-python-regression.md`; every row below was
re-verified against both codebases. Python command inventory counted this
session: **63 command entries** (`fullscreen_ui.py` COMMANDS) vs **24
commands + 1 subentry** (`src/commands.rs:132` COMPLETIONS).

| Axis | Python (`abstractcode`) | Rust (this app) | Verdict |
|---|---|---|---|
| Command surface | 63 entries: fork/rewind, compact/spans/expand, recall, @file+/files, blacklist, spawn, plan/review, vars/logs/snapshot, usage, cache stats/purge, copy, links/open, system/max-tokens, gpu… | 24: core run control + queue/goal/entities/sessions | **Python, ~2.5×** — triaged item-by-item in §5.4 |
| Live generation feedback | `on_token` fan-out → 10 Hz "3/50 · generating (12,345 chars)… ‹rolling tail›" (`react_shell.py:904-921, 1030-1060`) | nothing between ledger records; frozen "thinking (cycle 3)" + elapsed; ≥60s slow-call hint | **Python, decisive** — the single biggest "feels worse"; see §6 verdict |
| Context meter | `Context(next): 5,554/128,000 tk (4%)` vs the real window, source-resolved `_limits → CLI → local registry → default` (`react_shell.py:1352-1381`) — "the one number that changes decisions mid-session" | `ctx 41k` absolute only | **Python** — and NOT actually blocked for us (§5.1 CTX-0) |
| At-rest chrome density | boot banner 8–12 fact lines + always-visible footer (ctx-%, skills, mcp, GPU) + clickable dropdowns | header ~82 chars + blank strip + 4 centered lines + key legend; 75–90% blank rows measured | **Python** — the screenshot |
| Per-answer stats | footer per answer: `in= cached= out= tok/s elapsed prefix-reuse%` (`react_shell.py:2289-2355`) | cumulative strip + idle session summary only | **Python** |
| Per-tool detail | duration (approval dwell excluded), expandable full args/output folds, copy buttons, clickable links | name + JSON args preview + 6-row capped result; no expand, no duration, no copy — "full text in the run ledger" is a dead end in-app | **Python** |
| Cache instrument | `cache_meter.py` (301 lines): per-call table, prefix-reuse %, latency split, purge | static `/cache` info modal | **Python** |
| Markdown answers | tables (box-drawing) + mermaid (ASCII) + links (`terminal_markdown.py:222,288`) | engine md-lite: headings/lists/quotes/fences/diff-tint/syntax highlight — tables deliberately literal (`abstracttui/src/render/md.rs:14-17`, verified) | **Python** for agent prose; Rust for code |
| Event fidelity | in-process semantic step events — no inference layer, the bug class doesn't exist | fold over generic ledger records; heuristics burned twice in week one (dead `0↑0↓` counters; "Done · 5h" stuck strip) | **Python** — the permanent thin-client tax; paid down only by contracts (§6, GW-B; `docs/ui_events.md` direction) |
| Rendering machinery | prompt_toolkit + hand-built cache tower (each layer a documented incident fix) | damage tracking, 0-byte idle, keyed O(changed) feed, correct unicode/kitty keyboard — by construction | **Rust** — "optimized" belongs to this side |

The "feels worse" diagnosis in one sentence: **a better renderer showing
less information, with fewer features, that went blind during the exact
5-hour run the maintainer watched** (both live incidents were fold-inference
gaps a loop-owning app cannot have), **and an idle frame that is mostly
blank where his tool's is packed** — of course it feels like a regression.
It is one, on the axes he lives in.

## 4. What the port GAINED (protect these — they are why it exists)

1. **Runs survive the client** — quit mid-run, the agent keeps executing on
   the gateway; reattach + stream/poll fallback + stale-run guards. The
   Python loop dies with the process.
2. **Entity conversations** — @name visits, chips/focus, roster + identity
   cards, `/task` desk delegation, `/end` with reflection (~3,700 lines the
   Python app has no equivalent of).
3. **Persisted `/queue`** and **`/goal`** — durable FIFO across quits;
   goal-loop client with the `finish_on_root_only` defense (shipped).
4. **Durable server-side sessions** — `/sessions` + `use_session_history`
   replay; the Python state file is a local pointer listed by nothing.
5. **Images in the transcript** (mosaic artifact cards).
6. **A better approval surface** — modal cards, `f` full JSON, tier-honesty
   line, Esc-defers (Python approvals are typed y/n into a blocking prompt).
7. **26 contrast-audited themes**, live picker; semantic tokens throughout.
8. **Connection honesty** — orb, auto-reconnect, doctor/login, teaching
   empty state.
9. **Multi-conversation concurrency** — agent run + N entity visits.
10. **The engine itself** — the rendering foundation the Python app spent
    two years approximating with caches.

The corrected plan spends nothing that regresses these; the entities /
interaction-model / tier plans continue untouched (§5.6).

---

## 5. The corrected roadmap

**Re-baselined: the incumbent is the Python `abstractcode`, not codex.**
The floor is now defined as *"not a regression from the tool the maintainer
already built and trusts"*; codex parity remains the Wave-2 stretch. The
old roadmap's biggest structural error — evidence quoted, then scheduled
against (his fork added context + subagent observability; the roadmap
blocked one and parked the other in Wave 3) — is reversed: context-% ships
in the first wave with an honest source, and `/tree` moves up.

Effort scale unchanged: S < 1 day · M 1–3 days · L > 3 days. "Client-only"
means no other seat blocks it. Existing ids carried; new ids: HDR/REST/
CTX/IDLE/PAR (from the audit), R-CMD (command parity), GW-B (token-delta
ask), E5 (engine md-tables ask).

### Wave 0 — "Presence + parity floor" (NEW — this is what changes first)

Everything a glance sees. All client-only; ~1 week; near-zero seam risk
(chrome + one strip field + prefs).

| # | Id | What | Effort | Why it matters | Depends on |
|---|---|---|---|---|---|
| 1 | **PAR-1** | Predecessor parity inventory — **delivered in §5.4 of this document** (ship/adapt/drop per Python capability). Remaining act: maintainer strikes/blesses rows | S (done here) | Silent drops are how this regression happened | none |
| 2 | **HDR-1** | Header earns its row: fill the blank middle with facts — cwd basename, workspace mode, `skills N · mcp N` when nonzero, session token total at rest; dim the session id. No blank-middle at ≥100 cols. Chips keep priority when convos exist | S | The literal screenshot (`chrome.rs:110-141` prints ~82 chars then blank) | none |
| 3 | **REST-1** | Persistent facts at rest: the status bar becomes the always-visible instrument row — `ctx N/M tk (%) · N tk session · gpu N% · skills N · mcp N` whenever known; key legend moves behind `?`. **This absorbs and re-scopes UX-07**: reclaimed space goes to facts, never whitespace (as signed, UX-07 made the complaint worse). Idle strip on a fresh session shows the session card line, not an empty element (`chrome.rs:314-316`) | S–M | The Python footer law: "the footer is the only always-visible surface" | CTX-0 for the % segment; OBS-6 for gpu |
| 4 | **CTX-0** | Context-% v0 from an **operator-declared window**: `/context <tokens>` + config + `--max-tokens` → NEW-4's `_limits` passthrough (pulled forward from Wave 2), rendered `ctx 41k/262k (16%) — window: declared`, warn ≥75% / error ≥90%. Source-labeled; absence keeps today's honest `ctx Nk`. GW-A later upgrades the label to `window: served`. **The old roadmap's "BLOCKED on GW-A" was wrong** — Python's own first resolution rung is operator-declared (`react_shell.py:1352-1381`); a declared window is the same honesty class as `--max-tokens`, not a fabricated capability table | S | The number his fork added to codex; overflow currently surprises via error | none (NEW-4's `_limits` lands with it) |
| 5 | **IDLE-1** | Boot/idle identity card — UX-09 promoted to must and extended: the empty state becomes the Python banner's fact set (version · workflow · route · **cwd** · workspace mode · session + gateway · skills names · MCP names · context source), wordmark deduped (it renders twice today — captured), boot notices folded in (absorbs UX-15). Reusable as `/status` output | S | 27 of 36 rows are blank at boot; the first impression is the product | none |
| 6 | **OBS-6** | `/gpu` meter (pulled from Wave 2): status-bar segment, poll ~3s while a run/turn is active, ~30s idle when toggled on (cheap gateway GET, still wake-on-change); `supported:false` renders once. Endpoint live-verified (`/host/metrics/gpu`, Apple M5 Max) | S | "Is the model actually computing" — the daily slow-call anxiety; Python ships it; the port lost it | F1's edge-trigger pattern (pattern only) |
| 7 | **OBS-1a-live** | In-flight call feedback, the honest interim while GW-B is negotiated: strip shows `model call 14s · 41 tok/s (last call)` from the first second of every llm_call (receipts: elapsed from the started record; rate from the previous completed call's `gen_time`), replacing the frozen generic "working" and the 60s-only hint | S | The dead-air window is the top "feels worse" driver; this is what a thin client can say without lying | none |
| 8 | **HDR-2** | **Fix the redraw defect that IS the screenshot** (from `review-current-state.md`): (a) a `Ctrl+L` / redraw affordance that force-repaints the whole frame (the Python app had one, `fullscreen_ui.py:3494`; the port dropped it) — recovers instantly from any external clear; (b) make the header's `dyn_view` re-trigger on a cheap heartbeat or focus-regain so a static pinned-route run cannot leave stale/blank chrome after a damage gap; (c) render the composer placeholder even while focused (or draw a hint line ourselves — the engine only paints it unfocused, so `.autofocus()` blanks it today). | S–M | A pty-proven defect and a plausible (unproven — see §2 audit correction) cause of his blank-bar capture; (B)/(C) also fix "wiped screen recovers only a spinner" | none (engine redraw verb exists; if not, small E-ask) |
| 9 | **POLISH-1** | The looks-broken sweeps (unchanged from Wave 1 row 14): composer `❯` glyph, fuzzy `/` dropdown + row cap, theme-picker title, help-modal widths; elapsed as `9h20m` not `33628s` | S batch | `(persiste┃` and a self-truncating legend read as broken | none |

**Asks filed in Wave 0 week 1 (both on agora, neither blocks the wave):**

- **GW-A (gateway):** serve the declared context window per model in
  discovery (or the capability route). Render-when-present; upgrades CTX-0's
  label from `declared` to `served`. (Carried from the old roadmap.)
- **GW-B (gateway+runtime, NEW — the re-examined ruling, §6):** a live
  token-delta side channel for the currently executing `llm_call`.
- **E5 (abstracttui, engine):** markdown **table** rendering in md-lite
  (`render/md.rs` lists tables as deliberate non-goals; agent prose is full
  of pipe tables and they render as literal text today). M engine-side, next
  engine release; the client consumes via dep bump. Mermaid stays parked.

**Definition of done (Wave 0):** the maintainer opens the app cold and the
frame is full of true facts — header names directory/mode/counts, footer
carries ctx-%/tokens/gpu/skills/mcp, the idle pane is his banner, and
during a model call the strip visibly ticks. *The screenshot cannot recur.*

### Wave 1 — "Never lie" (the trust bundle — content unchanged, now second)

Everything from the signed Wave 1 that wasn't pulled into Wave 0, same
acceptance criteria and test obligations as written in ROADMAP.md §5:

| # | Id | What | Effort |
|---|---|---|---|
| 1 | **UX-01** | `Item::Tool` reshape + humanized sentence cards — still **first and alone within its wave** (hub for durations, diffs, pager, files) | M |
| 2 | **OBS-1a** | Honesty labels: `finish_reason≠stop` ("answer cut by token limit"), `retried ×N`, per-cycle `gen_time`/tok-s, batch-labeled tool durations | S |
| 3 | **F4 · F1 · F7 · F9** | Honest unknown-terminal · catalog self-heal on Down→Ok · counted SSE skips · stale-hint clear | S each |
| 4 | **UX-04** | Approval v0: full content block in the modal, deny-with-reason, "always allow (session)" relabel | S |
| 5 | **M1** | Attention pings: bell/OSC-9 on approval-wait, ask-user, run-terminal while unfocused | S |
| 6 | **F3** | Image downscale at decode + entry cap | S–M |
| 7 | **F5 · UX-11** | Entity convo bounds + drive ratios in words — landed with the in-flight entities build, as signed | S |

Rationale for second, not first: every item here is real and cheap, but
invisible at a glance — the audit's finding stands that this wave could
ship perfectly and the idle app would look identical. It lands immediately
after Wave 0 because the two live incidents (dead counters, stuck "Done")
were trust failures the maintainer personally hit.

### Wave 2 — "The daily driver" (re-baselined: beat the Python app at its own game)

The gate is renamed per the audit: **"the operator stops missing the Python
app"** (codex is the stretch). Wave-2 items from the signed roadmap, plus
the regression closures that were missing entirely.

| # | Id | What | Effort | Depends on |
|---|---|---|---|---|
| 1 | **F2** | Spawn-per-bulk on the runner (enabler for every fetch lane; identity guard on late Attach) | M | none |
| 2 | **UX-02/04a/NEW-5** | The one diff design: args-derived hunks, highlighted fresh-write content, server-diff passthrough, `(+N −M)`, same block in approvals, `files_touched` + `/files`. Governing rule kept: never fabricate old bytes or context lines | M | UX-01 |
| 3 | **UX-03/NEW-6** | `@file` mentions (bounded walk per D-1, entities-first, locality gate) | M | mention infra (shipped) |
| 4 | **R-CMD-1** | **/fork + /rewind** — turn list → cut/branch; v1 is client-seeded: new session id + first-run `context.messages` = transcript ≤ turn N (client-provided messages win server-side; server history accumulates fresh after the seed). Labeled "forked from turn N". No gateway contract needed for v1; a first-class session-fork ask is filed only if v1 proves insufficient | M | fold's chat_messages (exists) |
| 5 | **R-CMD-2** | **/compact v1** — replace turns ≤ N in the *client-sent* context with a server-generated summary (`POST /runs/{id}/summary`, NEW-2's endpoint), rendered as a durable labeled card ("compacted: 12 turns → summary · spend labeled"); `/expand`-class re-inflation deliberately dropped (Python's span archive is an in-process concept — §5.4) | M | NEW-2 endpoint (live) |
| 6 | **R-CMD-3** | **Per-answer stats footer** (R8): each answer card ends `in 41k · cached 12k · out 1.2k · 38 tok/s · 31s` from that run's receipts; splitless usage renders the labeled total (never fake zeros) | S | OBS-1a fields |
| 7 | **R-CMD-4** | **Cache instrument** (R10): `/cache stats` per-call table (ctx, cached_tokens, reuse %, gen_time) folded from ledger usage receipts — the ADR-honest version of `cache_meter.py`; purge stays server-owned (dropped, §5.4) | M | OBS-1a fields |
| 8 | **UX-08p** | Full-content pager pulled forward from Wave 3 (R12 was a top regression: truncation labels are dead ends in-app): `o` on a focused card / `/inspect` opens the full text via provenance + `get_ledger`. M2 (llm request inspect) stays Wave 3 | M | UX-01 provenance · F2 · D-2 |
| 9 | **NEW-3/UX-08e** | `/export [md\|json]` with `#TRUNCATION` header honesty | S | OBS-1a optional |
| 10 | **UX-06** | Type-to-filter in the shared Picker (342-row `/model` wall) | M | none |
| 11 | **NEW-4** | `/budget <n>tk [session\|run]` warn thresholds (the `_limits` half already landed with CTX-0) | S | CTX-0 |
| 12 | **UX-05a** | ctx-% upgrade to served truth when GW-A lands (label flips to `window: served`; no client table, ever) | S | **GW-A (gateway)** |
| 13 | **R-CMD-5** | Small parity batch: `/usage` (session accounting modal), `/copy [answer\|user] [turn]` (OSC 52), `/links` + `/open N` (from markdown link spans — never prose-parsed URLs), `/status` (IDLE-1's card as a command), `/max-messages` (client context-build cap) | S batch | none |
| 14 | **POLISH-2** | Glyph audit + modal edge contrast (as signed) | S batch | none |
| 15 | **E5-consume** | Markdown tables render on engine dep bump (box-drawing per the engine's design language) | S (client) | **E5 (engine)** |

**Definition of done (Wave 2):** one real coding task driven end-to-end
with no reason to open the Python app: edits reviewed as honest hunks,
prompts point at files, every truncation label resolvable in-app, per-answer
receipts visible, fork/compact available, tables render. Codex-parity
language demoted to stretch.

### Wave 3 — "Mission control" (the moat — kept, minus pulled items)

As signed in ROADMAP.md §5 Wave 3, with three deltas: UX-08p moved up
(above), OBS-2 `/tree` is now **row 1 and must-tier** (it is the
maintainer's own fork's flagship feature; the old roadmap parked his
revealed preference), and R-CMD-6 joins.

| # | Id | What | Effort | Depends on |
|---|---|---|---|---|
| 1 | **OBS-2** | `/tree` subrun modal + `N subruns active` chip (promoted) | M | OBS-1a; Fold seam care |
| 2 | **OBS-3** | `/runs` gateway board, slim v1 (adopt · cancel · goal-defense re-derived) | M | F2; goal defense |
| 3 | **M2** | `llm_call` request-side inspect in the pager ("what did the model actually see") | S–M | UX-08p |
| 4 | **F6** | Boot rehydration: attach-first merge + bounded parallel fetches | M | F2 |
| 5 | **OBS-4 + R-CMD-6** | Session run-history browser (read-only viewing mode) + **/recall v1** riding it: search past turns by query/time across rehydrated bundles — the honest adaptation of Python's span recall | M | F2 · F6 · goal defense |
| 6 | **M3** | Server-side session discovery (`/sessions` from `GET /runs` grouping; soft gateway ask for a session index) | S–M | OBS-3 lane |
| 7 | **NEW-2** | `/summary` + `/ask-run` (spend-labeled, never auto-fired) | S–M | F2 |
| 8 | **OBS-1b** | `/stats` run breakdown with provenance labels | M | OBS-1a; shares OBS-2 aggregation |
| 9 | **OBS-7** | wait_until/schedule visibility (before the goal bundle lights up) | S | none |
| 10 | **OBS-5a** | Artifact cards + save-to-disk (descoped browser) | S–M | F2 |
| 11 | **M4** | Unattended-honesty bundle riding the serve build (exit-code truth, labels inherited by exec/serve, budgets headless) | S | serve (in-flight) |
| 12 | **R-CMD-7** | `/vars` read-only run-var inspector (input_data + run vars from existing endpoints) · `/tool-specs` from the inventory | S–M | OBS-3 lane |

**Definition of done (Wave 3):** unchanged from the signed roadmap — the
durable/multi-agent story is watchable, plus GW-B's client half lands here
if the gateway ships the channel by then (see §6).

### 5.4 Command-parity triage (PAR-1 executed — the full inventory)

Every Python command absent from the Rust client, dispositioned. "Adapt"
means the capability ships with thin-client-honest mechanics that differ
from Python's in-process ones.

**Ship (slotted above):** `/fork` `/rewind` (W2, adapt: client-seeded
context), `/compact` (W2, adapt: server-summary card, spend-labeled),
`/recall` (W3, adapt: rides OBS-4), `/usage` (W2), `/copy` (W2), `/links`
`/open` (W2), `/status` (W0 card + W2 command), `/gpu` (W0),
`cache stats` (W2, from ledger receipts), `/max-tokens` (→ `/context` +
`/budget`, W0/W2), `/max-messages` (W2), `/vars` (W3), `/tool-specs` (W3),
`@file`+`/files` (W2 — mention + files-touched; Python's attachment-chip
upload lane stays parked with the image-paste trigger), `/history` (W3 =
OBS-4), `/plan`/`/review` (W3 nice: read-tier preset + a labeled review
posture; the goal loop's review_mode is the server half).

**Already covered by a different spelling (no action):** `provider`→
`/model`, `agent`→`/workflow`, `permissions`→`/tools tier`, `whitelist`→
`/workspace` allowed paths, `task`→plain prompt, `auto-accept`→`/auto`,
`keys`→`/help`+`?`, `clear`→`/new`, `exit/q`→`/quit`.

**Verify-then-slot:** `/blacklist` (session deny-list overriding the
allowlist) — if the runtime workspace policy carries a deny field it is an
S passthrough in `/workspace`; if not, it is a small runtime ask. Same for
`/system` (system-prompt override via input_data). Both W2–W3, marked
"verify field, else file ask."

**Drop, with the reason on record:** `/executor` (the gateway owns
execution; a thin client choosing executors would be a lie), `cache purge`
(server-owned cache; ADR forbids client cache negotiation), `/spawn`
(delegation is a workflow/gateway feature; `/task` covers entity desks;
trigger: real demand for TUI-spawned subagents), `/conclude` (in-process
loop concept; `/cancel` covers), `/spans` `/expand` (Python's in-process
span archive has no honest thin-client twin; `/compact` v1's labeled card
is the replacement), `/memorize` `/memory` (the durable-session memory
lane differs; trigger: a session-memory surface ruling), `/logs
runtime|provider` (gateway-side truth; doctor + web observer own it),
`/flow` (superseded by gateway workflows), `/mouse` (engine mouse is
always on), `/snapshot save|load|list` (gateway sessions ARE the durable
state; OBS-4 is the browser), custom theme authoring (26 audited themes +
picker; trigger: a named palette request — parked, not never).

### 5.5 Honesty guardrails — carried forward verbatim, all of them

The eight rules from ROADMAP.md §8 stand unmodified and extend to the new
items: no predictions as status (OBS-1a-live shows elapsed + *last-call*
rate, never a projected completion); no fabricated diffs; **no
client-shipped capability tables** (CTX-0 is operator-declared and
source-labeled — the 2026-07-17 fabricated-selection class stays the hard
line; GW-A is the only path to "served"); truncation always labeled
(`/compact` cards, exports); absence is a labeled state, never zero
(per-answer footers on splitless providers); claims from records, never
prose (`/links` reads markdown spans, not regex over prose); bounded-zombie
windows stay documented; the risk-register seam obligations (Fold::apply,
wire_feed, runner FIFO, goal defense, convo epochs — ROADMAP §8 table)
bind every touching item, 283-suite + named pins green.

### 5.6 In-flight signed work — untouched

`plan-interaction-model.md` (serve subcommand remaining),
`plan-entities-mcp.md` (v1 build + gateway asks #1–#5),
`tier-policy-agora-facts.md` (fields land at the next gateway bounce) all
proceed exactly as signed; Wave 0 touches only chrome/prefs surfaces and
composes with the entities chips (HDR-1 keeps chip priority in the middle
span). D-1 (hand-rolled walk), D-2 (Ctrl+T stays theme), D-3 (one M per
early wave) stay ratified.

---

## 6. The token-streaming ruling, re-examined (the §7 reversal)

**The old ruling conflated two different things.** It was right that the
client must never *imitate* streaming (the reveal-animation ban stands —
theater, never), and wrong to leave the *real* lane unfiled with "Trigger:
maintainer wants live answer text." **The trigger has fired** — the
maintainer's reaction, and the week's two blind-window incidents, are that
evidence. Facts verified this session:

- **The wire carries no deltas.** The gateway ledger stream emits `event:
  step` per started/completed record only (`routes/gateway.py:8025-8131`);
  the only "delta" in that file is byte-offset ledger follow. Grep of the
  gateway routes: zero `on_token` hits. Between an `llm_call` started and
  completed record, a thin client is structurally blind — no heuristic can
  fix that honestly.
- **The Python feedback is an in-process privilege.** `react_shell.py:904-921`
  registers `set_on_token` on its own pooled clients; the callback never
  crosses a process boundary today.
- **The engine is ready.** `FeedState::push_stream` / `stream_append`
  (`abstracttui/src/widgets/feed.rs:236,274`) re-typeset only the open tail
  block; F10 watch-item 2 already specs the client half (stream lane +
  ~500 ms commit cadence, never fold items). Zero engine work needed.

**Verdict: file GW-B now, as a first-class gateway+runtime ask.** Shape:
the runtime's `llm_call` effect handler tees provider stream chunks into a
**volatile** channel (never ledger records — durability of every delta
would bloat the run's system of record; the completed record stays the
durable truth); the gateway serves it per run (`GET /runs/{id}/llm/stream`
SSE, or `event: llm_delta` multiplexed on the existing ledger stream and
marked non-durable). Two acceptable tiers, offered in the ask so the
owning seats can price them: (a) full text deltas — the codex-class
experience; (b) a coarse progress heartbeat (~2 s: chars/tokens so far) —
most of the "am I blind?" value at a fraction of the wire cost. Effort:
client M (specced), runtime+gateway owns the rest — this is the one
regression a thin client cannot close alone, so the ask must say why it
exists: *the thin-client architecture is the product's foundation, and this
is its only structural feel gap vs the in-process predecessor.*

Until GW-B lands: Wave 0's OBS-1a-live (elapsed + last-call tok/s from the
first second) + Wave 1's slow-call upgrades are the honest maximum. The
non-goal table entry is rewritten from "not filed" to "**filed as GW-B;
client half specced; imitation still banned**."

---

## 7. Honest cost and phasing

| Wave | Theme | Rows | Effort mix | Blocked on a seat |
|---|---|---|---|---|
| 0 | Presence + parity floor | 8 | 7 S · 1 S–M | none (GW-A/GW-B/E5 *filed*, not consumed) |
| 1 | Never lie | 7 bundles | 6 S/S–M · 1 M (UX-01) | none |
| 2 | The daily driver | 15 | 5 S · 10 M-class | UX-05a on GW-A; E5-consume on an engine release |
| 3 | Mission control | 12 | 4 S/S–M · 8 M | M4 on serve (in-flight); GW-B client half if the channel ships |

Sequencing estimate, same discipline as before (ranges, not promises):
Wave 0 ≈ **1 week** · Wave 1 ≈ 1.5–2 weeks · Wave 2 ≈ 3–4.5 weeks ·
Wave 3 ≈ 3–5 weeks — ≈ 9–12.5 weeks single-threaded; the repo's proven
3-worker/5-cycle pattern compresses wall time, minus what the in-flight
plans still occupy. The growth vs the old roadmap (~7–11 weeks) is the
regression-closure work the old plan didn't contain; it buys the only gate
that matters (the maintainer keeps using the app past week one).

**Non-goals revisited:** the token-streaming non-goal **does not survive as
written** — split per §6 (imitation banned forever; the real lane is now
GW-B, filed). Surviving unchanged, with their recorded reasons: $ meter (no
pricing data), reveal animation (never), client OTEL, NEW-1 `/watch`
(observer app exists; entities v1.5 trigger), sidebar/leader-key/OSC-133,
UX-13 coalescing (post-UX-01 re-judge), F8 tokio (thresholds stand), F7
re-serve half, OBS-5 full browser, image paste / backtrack-fork / `!` shell
passthrough (contract/ruling triggers stand — note `/fork` v1 above is the
session-seed shape, which may retire the backtrack trigger naturally).
New parking-lot entries from the parity triage: `/spawn`, custom themes,
mermaid, span re-expansion — each with its named trigger in §5.4.

---

## 8. If you sign, here's what changes first

1. **Days 1–5 — Wave 0 lands whole**: HDR-1 + REST-1 + CTX-0 + IDLE-1 +
   OBS-6 + OBS-1a-live + **HDR-2 (the redraw fix that kills the exact
   screenshot)** + POLISH-1. The next time you glance at the idle app, the
   header names your directory and mode, the footer reads
   `ctx 41k/262k (16%) · 128k tk session · gpu 28% · skills 2 · mcp 1`,
   the empty pane is your banner's fact card, a running model call ticks
   visibly, and Ctrl+L instantly redraws a frame that a terminal clear
   ever wipes. §5.4's drop list is in front of you to strike.
2. **Same week — three asks filed**: GW-A (context window in discovery),
   **GW-B (live token deltas — the reversed ruling)**, E5 (engine markdown
   tables).
3. **Then Wave 1** (trust bundle, UX-01 first within it), **then Wave 2**
   opening with F2 + the diff design + `@file` + `/fork` — the point where
   the Python app stops being the better daily tool, feature by feature,
   receipt by receipt.
4. The in-flight entities/interaction/tier builds never pause.

The Item::Tool reshape is no longer the first thing that happens — closing
the presence gap is. That is the correction this synthesis exists to make.

- [ ] Maintainer sign-off (supersedes ROADMAP.md §9's order; D-1/D-2/D-3
      carry forward)
- [ ] §5.4 drop list blessed (or rows struck back into waves)
- [ ] GW-A + GW-B + E5 approved for filing
