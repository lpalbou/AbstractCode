# Roadmap audit — did we solve the right problem?

Status: adversarial audit of `ROADMAP.md` + its five sources (2026-07-22),
commissioned after the maintainer looked at the RUNNING app and said it
"doesn't look super optimal" and that the OLD Python abstractcode was "much
better and optimized." His screenshot: a nearly-empty header bar — a session
id and a green dot floating on blank space. READ-ONLY audit; every claim
below carries a file receipt.

**Verdict up front: the roadmap is a good plan aimed at the wrong baseline.
It benchmarked against codex (~96 mentions across the six documents) and
never once benchmarked against the app's own predecessor (zero frames
captured, zero banner/footer/chrome citations from
`abstractcode/react_shell.py` or `fullscreen_ui.py`). The maintainer's
reaction is a predecessor-regression signal, and the roadmap has no item
for the thing he saw: an app whose persistent chrome carries almost no
information at rest.** The plan's items are mostly real and its honesty
discipline is right; its ordering and its definition of "the floor" are not.

---

## 1. The wrong baseline: codex-parity vs predecessor-parity

### What the lanes actually did

All three lanes name the Python `abstractcode` as a studied reference in
their method preambles (lane1:7, lane2:12, lane3:10 — "not from memory of
them"). The audit checked what they actually cited from it. Across all five
source documents plus the roadmap, the Python app appears in exactly **five
substantive, feature-level citations**:

| Citation | Where | What it is |
|---|---|---|
| `@file` mentions exist (`file_mentions.py`) | lane2:314, lane2:439 | feature checkbox |
| `/gpu` toggle exists, "the Rust TUI lost it in the port" | lane3:236 | feature checkbox |
| diff/semantic-palette lesson | lane3:401 | one render rule |
| bridge/serve JSONL protocol | lane3:434, value:49 | fleet contract |
| attachment upload lane | feas:77 | deferred item note |

That is the complete inventory. **Nothing in any document cites the Python
app's boot banner, footer, status composition, context meter, skills/MCP
presence lines, copy buttons, fold regions, clickable substrate bar, or
theme-swatch dropdowns** — i.e., nothing about how the predecessor *looks
and feels at rest*, which is precisely what the maintainer compared. Lane 2
captured 12 frames of the Rust app and **zero frames of the Python app**
(lane2:499-507). There is no evidence anyone opened
`abstractcode/react_shell.py` (the banner + status live there, not in
`fullscreen_ui.py`) — the one place a density comparison would have started.

Meanwhile codex saturates every document: lane 2's definition of "premium"
*opens* with "Open codex mid-task…" (lane2:19-25), its cross-cutting success
criterion is "a user who has driven codex for a month sits down…"
(lane2:491), Wave 2's definition of done is "the operator stops keeping
codex open" (ROADMAP:210), and the roadmap's thesis sentence is "buy the
**codex** parity floor once, cheaply" (ROADMAP:46). Mention counts: lane2 37,
lane1 24, value 15, lane3 12, ROADMAP 7, feasibility 1.

### Why this matters — the predecessor is the real incumbent

The maintainer does not keep codex open next to this app. He keeps **his own
Python abstractcode** — a mature product (`fullscreen_ui.py` alone is 4,368
lines; `react_shell.py` is larger) with two years of adversary-driven polish
recorded in AGENTS.md (activity shimmer, fold regions, copy buttons,
clickable footer dropdowns with theme swatches, mouse support, the cache
meter, session timeline, `/gpu`). The Rust TUI is one commit old
(`34b9447`, v0.3.0). The lanes treated the Python app as a *protocol sibling*
(the bridge contract) and a *feature checklist donor* (@file, /gpu); they
never treated it as **the incumbent whose users this app must not lose**.

The roadmap's own logic condemns this. Its central argument is: "below that
floor the moat goes unvisited because the operator keeps codex open for real
work" (ROADMAP:47-49). Correct structure, wrong constant. The operator's
floor is not "codex parity" — it is "**not a regression from the tool I
already built and trust**." The roadmap computed the floor against the wrong
tool, so the floor's contents are wrong: it contains diffs and `@file`
(codex's strengths) and omits chrome density, always-visible context/GPU
meters, and mouse/copy affordances (the Python app's strengths).

### The sharpest internal contradiction

The roadmap *cites* the maintainer's revealed preference and then *schedules
against it*. ROADMAP:52-55: "given full freedom over codex he added subagent
observability, context observability, and durable memory — the depth wave,
none of the polish wave." Now look at where those three things landed:

| His revealed preference | Roadmap slot |
|---|---|
| Context observability (`/context`) | UX-05a — **Wave 2, BLOCKED on gateway ask GW-A** |
| Subagent observability (`/agents`) | OBS-2 `/tree` — **Wave 3** |
| Durable memory | shipped (fair) |

The two observability features he personally added to codex are the two the
roadmap deferred or blocked — while Wave 1 leads with honesty labels and
tool-card JSON reshaping. The evidence for "depth over polish" was quoted in
support of the thesis and then not allowed to order the waves.

---

## 2. Does the roadmap address "it looks sparse/empty"? Mostly no.

### What the maintainer saw, in code

The header (`src/ui/chrome.rs:13-150`) renders, left: `▲ AbstractCode ·
workflow · route` (~40-50 cols of content), right: truncated session id +
connection orb. Everything between is `canvas.fill(rect, ' ', …)` — blank.
On a full-width terminal the middle ~60-70% of the header row is empty
space, and the right cluster is exactly what his screenshot shows: a session
id and a green dot on blank. The idle screen below it: a centered 4-line
empty state (`transcript_view.rs:721-786` — wordmark, one guidance line, one
command line, "rendered by AbstractTUI") floating in an empty pane, and a
status bar carrying a key legend + theme + gateway label
(`chrome.rs:623-706`). **At rest, the persistent chrome carries zero
numbers: no context, no tokens, no skills, no MCP, no GPU, no directory.**

The Python app at rest (`react_shell.py`):

- Boot banner (`:3790-3844`): wordmark + rule, `Provider: … Model: …
  Agent: …`, `Base URL:`, `Workspace: … (mode …)`, `State: … (store …)`,
  `Skills: N active (names…)`, `MCP: N servers (names…)`,
  `Context: AGENTS.md (N chars)`, help hint — **8-12 fact lines**, instantly.
- Always-visible footer (`_compose_status_text`, `:1793-1851`):
  `Context: 41,203/262,144 tk (16%) | skills 2 | mcp 1 | GPU ▓▓▓░ 28%`,
  plus **live clickable provider/model dropdown buttons** (substrate bar,
  with theme swatches in the theme dropdown, `fullscreen_ui.py:3100-3131`).

The comment at `react_shell.py:1820-1822` is the Python app's own design
law, learned from its own adversary review: *"the banner scrolls away; the
footer is the only always-visible surface"* — so the numbers live in the
footer. The Rust TUI re-learned the first half (a boot card) and skipped the
second half (persistent facts). That asymmetry **is** the screenshot.

### Scanning the roadmap for a density item

Every Wave-1 row, audited against "does this change what a glance shows":

| Item | Glance-visible at rest? |
|---|---|
| UX-01 tool cards | No — visible only mid-run with tool calls |
| OBS-1a honesty labels | No — mid-run |
| F4/F1/F7/F9 trust plumbing | No — failure-mode only |
| UX-04 approval v0, M1 pings | No — event-driven |
| **UX-09 session identity card** | **Partially** — a boot transcript cell (which scrolls away, like Python's banner); adds cwd/version/mode. The only density item in the entire roadmap. Ranked "high" (not must), item 9 of 14. |
| UX-07 footer + `?` overlay | **Negative** — replaces the six-hint legend with `? shortcuts · tier · theme · gateway`. Adds tier; net *removes* at-rest information, importing codex's "the chrome whispers" minimalism (lane2:24-25) — the exact opposite of the Python footer the maintainer prefers. |
| F3/F5/UX-11/POLISH-1 | No (POLISH-1 fixes looks-broken truncation — worthwhile, adjacent) |

Wave 2: ctx-% (UX-05a) is the one persistent-chrome number, and it is
blocked on a cross-team gateway ask; `/gpu` (OBS-6) renders **only while a
run is active** by design (ROADMAP:199), so the idle screen stays bare.
Wave 3 is modals. **Grep confirms it: `density`, `sparse`, `first
impression`, "fill the frame" appear nowhere in ROADMAP.md.** The wave named
"look premium" contains one at-rest item, ranked ninth.

The header itself — the literal subject of the screenshot — got a **P2,
folded into UX-09** in lane 2 (lane2:256-262: add cwd, dim the session id)
and does not appear as its own roadmap row at all.

### An unforced error the Python code exposes: ctx-% is not actually blocked

The feasibility critique verified no gateway route serves a context window
and correctly banned a client-shipped window table (the 2026-07-17
fabricated-selection class). Conclusion drawn: ctx-% is BLOCKED on GW-A
(ROADMAP:80, :198). But read the Python app's resolution chain
(`react_shell.py:1352-1381`): `_limits.max_tokens` (run vars) → CLI
`--max-tokens` → local capability lookup → default. The first rung is
**operator-declared**, and the roadmap's own NEW-4 already ships a `_limits`
passthrough in `run_input` (ROADMAP:197). An operator-declared window is not
a fabricated table — it is the same honesty class as Python's `--max-tokens`
flag, labeled at the source. **A `ctx N/M (%)` meter could ship in Wave 1
with zero gateway work** (`/context <window>` or config; GW-A upgrades it to
served truth later, render-when-present). The roadmap missed this
composition because nobody read how the predecessor computes the number the
maintainer is used to seeing.

---

## 3. Phasing: re-argued from the maintainer's revealed priority

The signal to phase on: the sole adoption gate for this product is one
person, and he judged it **at a glance against his own tool**. His revealed
priorities, in evidence order: (1) his screenshot → at-rest density and
first impression; (2) "much better and optimized" → overall parity with the
Python app (features *and* feel — copy/mouse affordances, meters, banner);
(3) his codex fork → context + subagent observability. None of these is
"diffs vs codex."

Wave 1 as signed leads with honesty plumbing (F4/F7/F9/F1 — real, cheap,
**invisible at a glance**) and a JSON-to-sentence reshape that only shows
mid-run. If the maintainer glances again after Wave 1 ships in full, the
idle app looks *nearly identical* to today's screenshot: same bare header,
same centered four lines, same legend footer (slimmer, even). That is a
roadmap that can execute perfectly and still fail its only gate.

The honest counter-consideration: "optimized" may also point at behavior
(boot rehydration is serial — F6; approvals can queue behind bulk fetches —
F2), and a glance is one data point, not a spec. But F6/F2 are
localhost-mild today by the lanes' own analysis (value:145-147), and the
screenshot is the concrete evidence in hand. The audit reads the reaction as
~80% "this looks thin next to my app," ~20% general regression unease. Both
call for the same correction.

---

## 4. Strategic honesty: is "don't chase codex, build the moat" a rationalization?

Steelman of the opposite thesis: **"The app must first not feel like a
regression from its own predecessor; codex is irrelevant until then."**

- The Python app is the incumbent. Its user (the only user) already paid
  the switching cost TO it; asking him to switch to a thinner tool because
  the thinner tool has a nicer *future* is exactly the trade users refuse.
- Every moat feature the roadmap defends (durable runs, entities, fleet
  seats) is reachable *through the Python app too* — it is the same gateway.
  The Rust TUI's differentiation against its real competitor is terminal
  excellence (abstracttui exists precisely for that), not gateway access.
  A moat argument that works against codex is *empty* against the sibling.
- The moat thesis produced a floor definition ("legible tool sentences,
  diffs, @file, context-%, ? help" — ROADMAP:46-47) that is a **codex
  feature list**. A predecessor floor would read: dense banner + persistent
  meters (context %, GPU, skills/MCP), copy/mouse affordances, `/gpu`,
  workspace facts on screen — different items, and notably *cheaper* (mostly
  chrome composition, no new data paths).
- The tell of rationalization: the roadmap quotes the maintainer's fork
  preferences as evidence for the thesis, then schedules those exact
  features last (§1 above). Evidence that argued for re-ordering was used
  as decoration.

Where the steelman overreaches: the moat framing was *not* invented to dodge
work — the durable/observability items (tree, runs board, budgets) are real
and genuinely unavailable in codex-class tools; the honesty discipline
(never fabricate diffs/windows) is the product's actual brand and must
survive any re-phasing; and some Python behaviors are structurally different
for a thin client (its `Context(next)` is an in-process estimate over
messages the Rust client never holds — parity must be receipts-honest, not
cosplay).

**Adjudication: both theses are right at different time scales, and the
roadmap inverted them.** Predecessor-parity governs the next month (the
adoption gate); the moat governs the quarter (the reason the port exists at
all). The roadmap wrote the quarter's strategy and skipped the month's. The
fix is not to delete the moat waves — it is to insert the missing first wave
and re-derive "the floor" from the tool the maintainer actually compares
against.

---

## 5. What should change — concrete

### 5a. New items (none exist in the roadmap today)

| Id | What | Effort | Receipt |
|---|---|---|---|
| **HDR-1** | Header earns its row: fill the blank span with facts — cwd (basename), workspace mode, at-rest token/session totals, skills/MCP counts when nonzero; dim the session id (lane2:260 already suggested it, got dropped). No blank-middle at ≥100 cols. | S | `chrome.rs:13-150` fills with spaces today |
| **REST-1** | Persistent facts at rest: the status bar (or a second chrome row) carries `ctx N/M tk (%) · N tk session · gpu N% · skills N · mcp N` whenever known — the Python footer contract ("the footer is the only always-visible surface", `react_shell.py:1820`). Idle ≠ empty. | S–M | `chrome.rs:623-706` carries only legend+theme+gateway |
| **CTX-0** | Context-% v0 from an **operator-declared window** (`/context <tokens>`, config, or `--max-tokens` → NEW-4's `_limits` passthrough), source-labeled. Unblocks UX-05a without GW-A; GW-A remains the upgrade to served truth. | S | Python chain `react_shell.py:1352-1381`; NEW-4 already ships `_limits` |
| **IDLE-1** | Idle dashboard: promote UX-09 to must and extend it — the empty state becomes the session card (version · workflow · route · cwd · workspace mode · state/store · skills · MCP · context sources), i.e. the Python banner's fact set, not four centered lines. | S | Python banner `react_shell.py:3790-3844`; empty state `transcript_view.rs:721-786` |
| **PAR-1** | **Predecessor re-baseline (do first, 1 day):** read `react_shell.py` + `fullscreen_ui.py` properly; capture side-by-side frames (idle / mid-run / approval) Python vs Rust; produce a parity inventory — every Python UI capability (banner facts, footer meter, substrate buttons, copy buttons, fold regions, mouse, dropdown swatches, cache meter, session timeline, /gpu) marked ship / adapt / drop-with-stated-reason. Silent drops are how this regression happened. | S | this audit found five feature citations total (§1) |

### 5b. Re-phasing

**Wave 0.5 — "Not a regression" (insert before the current Wave 1 core,
~1 week):** PAR-1 → HDR-1 + IDLE-1/UX-09 + REST-1 + CTX-0 + OBS-6 `/gpu`
pulled forward from Wave 2 (it is S, the Python app ships it, and it is one
of the two "is it computing" numbers the maintainer actually watches; keep
poll-while-active, but let the meter row exist at rest showing its last/idle
state honestly) + POLISH-1/UX-16/17 (the looks-broken truncation sweeps).
UX-01 keeps its "first and alone" slot **within** this wave — it is the
mid-run face and genuinely the right hub — but it no longer *is* the wave.

**Current Wave 1 trust bundle (F4/F1/F7/F9, OBS-1a, UX-04, M1):** unchanged
in content, lands second — cheap, right, and invisible at a glance; it must
not lead.

**UX-07 (footer slimming):** re-scope before shipping. Move key hints behind
`?`, yes — but the reclaimed row space goes to REST-1 facts, not to
whitespace. As written it makes the maintainer's complaint *worse*.

**Waves 2-3:** keep, minus the items pulled forward. Re-title Wave 2's DoD:
"the operator stops missing the Python app" is the gate; "stops keeping
codex open" is the stretch. Consider pulling OBS-2 `/tree` earlier within
Wave 3 → 2 if capacity allows — it is the maintainer's own fork's flagship
feature and the roadmap's best depth demo.

### 5c. What NOT to change

The honesty guardrails (§8 of the roadmap) survive untouched — REST-1/CTX-0
must render labeled sources and honest absence, never fabricated numbers;
the non-goals table stands (no streaming theater, no $ meter); the in-flight
signed plans are unaffected; the dependency spine (UX-01 as hub, F2 before
Wave-3 fetch lanes) is correct engineering and keeps its ordering *within*
the re-cut waves.

---

## Appendix — receipts index

- Codex anchoring: mention counts per doc (grep `codex`): lane2 37, lane1
  24, value 15, lane3 12, ROADMAP 7, feasibility 1. Python-sibling
  substantive citations: 5 (table in §1); zero chrome/density citations;
  zero Python frames captured (lane2:499-507 lists Rust-only captures).
- Rust at-rest surfaces: `src/ui/chrome.rs:13-150` (header; blank fill),
  `:623-706` (status bar: legend + theme + gateway only),
  `src/ui/transcript_view.rs:721-786` (4-line centered empty state).
- Python at-rest surfaces: `abstractcode/react_shell.py:3790-3844` (banner:
  provider/model/agent, base URL, workspace+mode, state+store, skills, MCP,
  context, help), `:1793-1851` (footer: `Context: N/M tk (%) | skills N |
  mcp N | GPU`), `:1820-1822` (the always-visible-footer law),
  `:1352-1381` (window resolution: `_limits` → CLI → caps → default),
  `fullscreen_ui.py:3100-3131` (clickable dropdowns + theme swatches),
  `:356-386` (copy buttons, fold regions, activity shimmer).
- Roadmap gaps: no `density`/`sparse`/`first impression` hits in ROADMAP.md;
  header improvements exist only as a P2 note folded into UX-09
  (lane2:256-262); UX-05a marked blocked (ROADMAP:80,198) despite NEW-4's
  `_limits` passthrough shipping the Python app's first resolution rung.
- Fork-preference contradiction: ROADMAP:52-55 vs slots at ROADMAP:198
  (UX-05a, W2-blocked) and ROADMAP:221 (OBS-2, W3).
