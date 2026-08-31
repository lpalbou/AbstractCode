# Was the Python abstractcode better? A regression audit of the Rust port

Status: independent comparative investigation, 2026-07-22. Read-only; both
codebases read directly. The hypothesis under test — the maintainer's claim
that the Python `abstractcode` (in `abstractframework/abstractcode`) was
"much better and optimized" than this Rust port — is taken seriously, not
defended against.

Scope compared: Python `abstractcode/fullscreen_ui.py` (4,368 lines),
`react_shell.py` (13,516 lines), `cli.py`, `theme.py`, `terminal_markdown.py`,
`cache_meter.py`, `tool_permissions.py` (~20.7k lines of core) vs Rust
`abstractcode/src/**` (~21.0k lines) on the `abstracttui` engine.

---

## Verdict up front

**The maintainer is substantially right — on features, in-run feedback, and
transcript information access, the Python app is ahead by a wide margin.**
He is wrong on one word: "optimized." The Python UI is *hardened* (it reached
its current smoothness only after months of measured cache/scroll waves:
per-keystroke re-tokenization, 26ms window reformats, Bresenham wheel judder —
all documented fixes in its own comments); the Rust engine is architecturally
*cleaner* (damage tracking, 0-byte idle frames, keyed O(changed) feed updates)
and starts from a better rendering foundation. But a great renderer showing
less information, with fewer features, that froze on "Done" during a 5-hour
run, feels worse — and did.

Roughly: the Rust port carries **~40% of the Python command surface** (24
completions in `commands.rs:132` vs ~60 commands in `fullscreen_ui.py:56-120`),
**none of the live generation feedback**, and **a fraction of the per-item
transcript detail** — while adding durable-run survival, entities, queueing,
and images that the Python app cannot have. The gap is mostly a
projection-layer problem (fixable in the client), with three named exceptions
that need ecosystem contracts (token stream, context window, markdown tables).

---

## 1. Information density and layout

### Python frame (what one screen shows)

Layout (`fullscreen_ui.py:1929-2105 _build_layout`): scrollback · separator ·
conditional attachments-chip bar · 3-row composer with `> ` prompt · status
bar (left+right) · help bar. Floats: a 10-row command completion menu with
descriptions, plus **seven anchored dropdown popovers** with
click-outside-to-close.

The status bar left (`fullscreen_ui.py:2595-2686 _get_status_formatted` +
`react_shell.py:1540-1852`) packs, in one row:

- braille spinner + **shimmer** activity text (moving 3-char highlight,
  `fullscreen_ui.py:2656-2677`),
- `[agent ▼]` dropdown, `[permission-mode ▼]` dropdown (danger-styled when
  full-auto, `2639`), `[provider ▼] [model ▼]` live buttons,
- `Context(next): 5,554/128,000 tk (4%)` — a **context-percentage meter
  against the model's real window** (`react_shell.py:1813-1818`), the number
  the Python code itself calls "the one number that changes decisions
  mid-session" (`1806-1810`),
- `| skills 2 | mcp 3` presence badges (`1825-1830`),
- an optional **GPU utilization meter** (`1832-1849`).

The status bar right (`fullscreen_ui.py:3182-3223`) adds `[skills 2 ▼]
[mcp 3 ▼] [cache auto ▼] [theme ▼]` badge buttons. The help bar names the key
gestures, width-aware (`1992-2009`).

In the transcript: every tool call is a **clickable fold region** (▶/▼ toggle,
whole header clickable, `fullscreen_ui.py:1601-1615`, Ctrl+O for the latest),
every answer carries a clickable `[ copy ]` button (`1577-1580`), URLs are
linkified and clickable (`1627`), a **live activity line renders under the
question** with spinner glyph + shimmer + elapsed seconds spliced post-cache
per frame (`1820-1865`), and drag-select copies via OSC 52 + native clipboard
(`1146-1304`).

### Rust frame

Layout (`ui/mod.rs:262-337`, `CHROME_ROWS = 4` at `ui/mod.rs:38`): 1-row
header · transcript feed · 1-row activity strip · 1-4-row composer · 1-row
status bar.

- Header (`chrome.rs:13-149`): wordmark · workflow · provider·model (with the
  honest "gateway defaults (resolved)" form) · entity chips with elapsed ·
  session id · connection orb.
- Activity strip (`chrome.rs:152-461`): spinner + activity + cycle + elapsed
  + `12k↑ 3k↓ tk` + `ctx N` + `cache N` + `N tools` + queue count + a 16-col
  output-token sparkline; pending waits own the strip; a ≥60s model call
  names itself (`417-426`).
- Status bar (`chrome.rs:623-710`): context-sensitive key legend + theme +
  gateway + connection error.

### Comparison

| Axis | Python | Rust | Ahead |
|---|---|---|---|
| Chrome rows | ~6-7 | 4 (+composer growth) | Rust (more transcript space) |
| Context meter | **%-of-window, per-next-call estimate** (`react_shell.py:1813`) | absolute `ctx N tk` only — no window, no % | **Python** (Rust blocked on gateway: no declared context window; ROADMAP UX-05/GW-A) |
| Live generation feedback | 10 Hz char count + rolling text tail (`react_shell.py:1030-1060`) | none between ledger records | **Python** |
| Provider/model/agent switching | 7 clickable footer dropdowns w/ badges | `/model` `/workflow` keyboard modals | **Python** for density/mouse; Rust modals are fine keyboard-first |
| Skills/MCP presence | footer badges + buttons | none on chrome (modals only) | **Python** |
| GPU meter | footer segment (`/gpu`) | none (roadmap OBS-6) | **Python** |
| Session identity / connection | not in chrome (banner only) | header session id + connection orb | **Rust** |
| Entity/multi-conversation state | n/a | header chips + focus + spend | **Rust** (feature doesn't exist in Python) |
| Token sparkline | none | 16-col output sparkline | **Rust** |
| Queue visibility | n/a | strip count + paused state | **Rust** |
| Per-tool-card detail | args + result summary + **duration** + policy hint + expandable full args/output + copy | name + args preview + status + 6-row capped result (details mode), **no expand, no duration, no copy** | **Python**, clearly |
| Answer stats | per-answer footer: `in= cached= out= tok/s elapsed prefix-reuse%` (`react_shell.py:2289-2355`) | none per answer; cumulative strip + idle session summary | **Python** |
| Markdown answers | headings/code/**tables (box-drawing)**/**mermaid (ASCII)**/links (`terminal_markdown.py:222,288`) | engine md: headings/lists/quotes/fences/**diff-tinted fences**/syntax highlight — **tables deliberately unsupported** (`abstracttui/src/render/md.rs:14-17`) | **Python** for agent answers (tables are common); Rust for code fences |
| Images | none | mosaic image cards (`transcript_view.rs:218-241`) | **Rust** |

**Density verdict: yes, the Python frame carries more *useful* state** — the
context-% meter, live generation progress, GPU, badges, and per-item depth are
exactly what an operator watches during a long run. The Rust chrome is cleaner
and spends its rows on thin-client-specific truths (session, connection,
entities, queue) but loses the operator-facing instruments.

---

## 2. Feature parity — verified "Python has it, Rust doesn't"

Every item below was verified in both codebases (handler present in Python;
no equivalent found in Rust `commands.rs`/`ui/`/`runner.rs`).

| # | Feature | Python evidence | Rust status |
|---|---|---|---|
| R1 | **/fork** — branch a new session from an earlier turn (original archived, composer prefilled with the removed prompt) | `react_shell.py:11701-11831` (`_handle_rewind(fork=True)`) | absent |
| R2 | **/rewind [N]** — cut the transcript before turn N, with a numbered turn list | same handler, `11723-11737` | absent |
| R3 | **/compact [light\|standard\|heavy]** + **/spans** + **/expand** — conversation compression with archived, re-expandable spans | `react_shell.py:8649, 8881, 8908` | absent |
| R4 | **/recall** — recall memory spans by query/time/tags | `react_shell.py:9024` | absent |
| R5 | **/blacklist** — session-scoped path blacklist (overrides whitelist), with reset | `react_shell.py:9358-9407` | absent — `/workspace` (modals.rs:1650) has root/mode/**allowed** paths only, no deny-list |
| R6 | **@file attachments** — `@`-completion of workspace files into attachment chips, `/files`, `/files-keep`, pasted-path detection, chips bar | `fullscreen_ui.py:651-676, 1948-1957, 3225-3261`; `react_shell.py:5755, 5782` | absent — `@` is entities-only (`mention.rs:1-60`) |
| R7 | **Live token streaming** — `set_on_token` fan-out; "generating (12,345 chars)… ‹rolling tail›" at 10 Hz in the activity line + footer | `react_shell.py:904-921, 1030-1060` | absent by ruling (ROADMAP §7 "no token-streaming imitation"); only the ≥60s slow-call hint (`chrome.rs:417-426`) |
| R8 | **Per-answer stats footer** — timestamp, in/cached/out tokens, tok/s, elapsed, prefix-reuse% | `react_shell.py:2289-2355`, printed at `3119-3121` | absent |
| R9 | **Per-tool-call duration** — measured from exec start, approval dwell excluded | `react_shell.py:3534-3549` | absent — `Item::Tool` has no duration field (`transcript.rs:51`) |
| R10 | **Prompt-cache instrument** — ledger-driven meter (`cache_meter.py`, 301 lines): per-call table (`/cache stats`), prefix-reuse %, first/warm latency, `/cache purge`, mode control | `react_shell.py:6495, 6682, 7037, 7080, 11246` | `/cache` is a static info modal: route, supported?, cache hits, last ctx (`modals.rs:1445-1571`) |
| R11 | **/usage** — session accounting incl. call count + latency split | `react_shell.py:11246-11279` | partial: idle-strip session totals only (`chrome.rs:312-355`) |
| R12 | **Interactive transcript** — clickable per-item folds (full args + full output inline), `[ copy ]` buttons, clickable links, `/links`, `/open N` | `fullscreen_ui.py:784, 818, 1577-1615`; `react_shell.py:5146, 5157` | global Ctrl+D details toggle only; result previews hard-capped at 6 rows with "full text in the run ledger" (`transcript_view.rs:32, 196-199`) — **no way to see full output in-app**; no copy buttons; no links |
| R13 | **Markdown tables + mermaid** in answers | `terminal_markdown.py:222 (_render_table), 288 (_render_mermaid)` | engine md-lite deliberately excludes tables (`abstracttui/src/render/md.rs:14-17`) — pipe tables render as literal text |
| R14 | **/gpu** — GPU utilization meter in the footer | `react_shell.py:5035, 1832-1849` | absent (roadmap OBS-6) |
| R15 | **/spawn** — background subagents with list/tail/cancel | `react_shell.py:11161` | absent (`/task` is entity-desk delegation, not a subagent) |
| R16 | **/plan** and **/review** modes — TODO-first planning; self-check verifier rounds | `react_shell.py:5632, 5655` | absent (the `/goal` loop is a different, gateway-side mechanism) |
| R17 | **/system, /max-tokens, /max-messages, /config, /executor** — system-prompt override + generation/history limits + tool executor | `react_shell.py:4690, 5817, 5701` | absent |
| R18 | **/vars** (durable run-var inspector), **/logs runtime\|provider**, **/snapshot save/load/list**, **/memorize**, **/copy user\|assistant [turn]**, **/history**, **/status**, **/tool-specs**, **/conclude** | `react_shell.py:9141, 11833, 10809, 11555, 11324, 11298`; COMMANDS `fullscreen_ui.py:56-120` | all absent |
| R19 | **Custom theme authoring** — `/theme custom`, user theme files, env overrides | `theme.py:319-492` | 26 built-ins only (`abstracttui/src/theme/seeds.rs:62`), no custom |
| R20 | **Footer shimmer** — moving-highlight "reflect" animation on the spinner text | `fullscreen_ui.py:1799-1818, 2656-2677` | engine `Spinner` frames + plain label; no shimmer |

### Claims from the hypothesis that did NOT verify (honesty section)

- **"Prompt history persistence"** — not a regression. The Python composer
  history is `InMemoryHistory` (`fullscreen_ui.py:26, 649`) — it does **not**
  survive a restart either. Rust `TextAreaState::push_history` is equally
  session-scoped. Parity (both ephemeral).
- **"Transient status"** — parity in function. Python
  `set_transient_status` (2s auto-clear spinner slot,
  `fullscreen_ui.py:1306-1319`); Rust has toasts (`ui/mod.rs:1279
  wire_toasts` + `store.notify`). Different idiom, same job.
- **Approval UX** — the Rust port is *better*, not worse: a dedicated modal
  with human-readable per-call cards, `f` full-JSON flip, a/A/d keys, a
  tier-honesty line, and Esc-defers-without-denying
  (`modals.rs:249-433`). Python approvals are y/n/a/e/q typed into a blocking
  composer prompt (`react_shell.py:3178-3188`).
- **Tool-permission tiers** — deliberate near-parity: `tool_policy.rs` is a
  port of `tool_permissions.py` including the read-only-git two-stage proof
  and fail-closed unknown tools (its own header credits the Python
  precedent). One honest semantic gap: Python read-only mode **denies**
  mutation (integrity claim); the Rust tier only decides auto-vs-prompt.

---

## 3. Rendering quality and responsiveness

### Why the Python app *feels* live and the Rust port felt dead

The Python shell owns the loop in-process. Its UI consumes **semantic step
events** emitted by the agent adapter itself — `act` (tool name + args +
call_id), `observe` (result + success), `parse` (cycle content + reasoning +
iteration/max), `done` (answer), `status`, `ask_user`
(`react_shell.py:3380-3628`) — plus a **runtime `on_token` callback**
(`904-921`) and an in-process ledger subscription feeding the cache meter
(`895-899, 1017-1027`). Nothing is inferred; there is no transport.

The Rust port reconstructs all of that by **folding generic ledger records**
arriving over SSE (`transcript.rs:423 apply`), with heuristics for: which
node_id means a reasoning cycle (`448`), which run is the answer lane vs a
delegate child (`454-478, 495`), which `abstract.status` texts are
terminal-sounding noise (`556-575`), and which usage shapes are splitless
(`104-108`). Every one of those heuristics has already produced a **live
incident in its first days**:

- **dead token counters** — "0↑ 0↓ tk" across a five-hour coder run because
  the provider reported only `total_tokens` (fixed same day; comment at
  `transcript.rs:104-108`, `chrome.rs:388-398`);
- **"Done" stuck for hours** — the strip read "Done · cycle 12 · 17880s"
  (~5 h) while the tree kept working, because a wrapper bundle emitted
  `{"value": "Done"}` per round (fixed by clearing terminal-sounding statuses,
  `transcript.rs:566-571`);
- wrong model label / ctx from delegate children (`490-503`), swallowed
  repeated asks (wait identity = key+step_id), late answers from abandoned
  runs.

This bug **class does not exist in the Python app** — its events carry exact
semantics from the loop that produced them. That is the honest core of "the
Python one was better": during a long model call the Python UI shows
`3/50 · generating (12,345 chars)… ‹the text being written›` refreshed at
10 Hz (`react_shell.py:1030-1060`), while the Rust strip shows a frozen
"thinking (cycle 3)" plus an elapsed counter until the *entire* call
completes, because the gateway ledger only carries started/completed records
(`transcript.rs:439-445`) — token deltas never reach the wire.

### Where the Rust rendering is genuinely better

- The engine renders with **damage tracking and 0-byte idle frames**
  (test-pinned in abstracttui); the feed applies **keyed O(changed) updates**
  with content fingerprints (`transcript_view.rs:404-503, 522-641`).
- The Python UI achieves its smoothness through an accumulated cache tower —
  render-counter snapshot cache, formatted-window cache, dirty-range
  promotion, view-window virtualization with recenter hysteresis, post-cache
  activity/selection splices (`fullscreen_ui.py:1629-1788, 1820-1880`) — each
  layer added after a measured incident (per-keystroke O(session)
  re-tokenization, ~26ms re-window reformats, wheel judder; all documented in
  its own comments, e.g. `1671-1679`, `react_shell.py:1622-1628`). It works,
  but it is remediation, not architecture.
- Rust wheel/scroll/follow-tail, kitty keyboard protocol (real Shift+Enter),
  block paste, and unicode-width handling come from the engine and are
  correct by construction; Python's equivalents are hand-tuned prompt_toolkit
  workarounds.

So on raw rendering mechanics the Rust port is ahead; on **what is rendered
while you wait** the Python app wins decisively — and that is what an
operator perceives as "responsive."

---

## 4. What the Rust port does better (the fair list)

1. **Runs survive the client.** Quit mid-run and the agent keeps executing on
   the gateway; the TUI reattaches (`runner.rs` stream/poll fallback
   `1126-1215`, stale-run guards `979-1013`). The Python loop dies with the
   process — durable state lets `/resume` continue, but the in-flight call is
   lost and nothing progresses while the terminal is closed.
2. **Entity conversations** — `@name` visits, held drafts, non-interruptible
   turn honesty, focus cycling (Ctrl+E), roster + identity cards, `/task`
   desk delegation, `/end` with reflection (`convo.rs`, `entities.rs`,
   `gateway/entities.rs`, ~3,700 lines). The Python app has none of this.
3. **/queue** — persisted per-session FIFO prompt queue, restores paused,
   drains on success only (`store.rs:101-131`, `ui/mod.rs:1307-1483`).
4. **/goal** — goal-loop runs with status/stop (`ui/mod.rs:866-1045`).
5. **/sessions picker + durable server-side session history** — sessions
   live in the gateway store and replay via `use_session_history`; the Python
   app's state file is a local pointer, listed by nothing.
6. **Images in the transcript** — generated-image artifacts render as mosaic
   cells in-feed (`transcript_view.rs:218-241`).
7. **A better approval surface** (see honesty section above).
8. **Theme system** — 26 contrast-audited themes with a live picker; semantic
   tokens engine-wide vs Python's ANSI-constant styling.
9. **Connection honesty** — orb + auto-reconnect + poll fallback + doctor/
   login; the empty state teaches recovery (`transcript_view.rs:746-766`).
10. **Multi-conversation concurrency** — agent run + N entity visits at once,
    per-convo status chips; the Python shell is single-conversation.

---

## 5. Root cause: projection layer, or architecture?

Split the regression list by what it takes to fix:

**Pure projection-layer (client-only, the fold already holds the data):**
per-tool durations (records carry timestamps), per-answer stats footer
(usage already folds), richer tool cards with full args/results (payloads
arrive in the records; the 6-row cap is a choice), copy buttons / links,
`/usage` detail, cache stats table (ledger usage per call), item-level
folds instead of a global toggle. This is most of the felt gap, and the
existing ROADMAP (UX-01 hub reshape) already targets it.

**Ecosystem contracts required (not client-fixable alone):**
- **Live token streaming (R7)** — the gateway ledger has no token-delta
  channel; the runtime's `on_token` exists in-process only. Without a
  gateway streaming contract, the thin client is structurally blind between
  records. This is the single biggest "feels worse" item and it is an
  architectural consequence of the thin client *as the gateway wire exists
  today* — not intrinsic to thin clients.
- **Context-% meter** — needs the gateway to declare the model's context
  window (ROADMAP GW-A ask). Python reads model capabilities in-process.
- **Markdown tables** — needs engine md support (deliberate non-goal today,
  `md.rs:14-17`); the client cannot render what the engine parses as
  literal text.

**Product-scope decisions (features nobody ported):** fork/rewind, compact/
spans/expand, recall, @file attachments, blacklist, spawn, plan/review,
system/max-tokens/config, vars/logs/snapshot/memorize/copy. None are blocked
by the thin-client architecture — session timelines and attachments are
client-side transcript operations, and the gateway carries attachments and
run vars already. They are simply unbuilt (~35+ commands of daily-driver
surface).

**Permanent thin-client tax:** the fold-inference layer (`transcript.rs`,
1,882 lines + `protocol.rs`, 955) is new code the Python app never needed,
and every gateway wire quirk becomes a client bug there (the 5-hour
incident). This tax does not go away; it can only be paid down with
contracts (typed UI events from the gateway instead of heuristic folding —
the `docs/ui_events.md` direction).

---

## 6. The blunt verdict

**Feature surface: Python wins, large.** ~60 commands vs 24; the missing set
includes things a daily driver actually uses (fork/rewind, compact, @file,
cache instrument, per-answer stats, spawn). This is the strongest form of the
maintainer's claim and it is simply true.

**In-run feedback / perceived responsiveness: Python wins, large.** Live
token stream + exact step events vs record-granularity fold with inference
bugs (two of which burned a real 5-hour run this week). The thin client is
not doomed here, but it needs a gateway token/UI-event contract to ever
match, and the current roadmap explicitly declines to imitate token
streaming — that ruling is where the maintainer's experience and the
project's stated thesis directly conflict, and his experience is evidence
against the ruling.

**Transcript information access: Python wins.** Expandable full args/output
per tool, durations, copy, links, tables, mermaid — vs capped previews and a
global toggle that points you at "the run ledger."

**Rendering machinery / efficiency: Rust wins.** Damage-tracked engine,
0-byte idle, keyed updates, correct unicode/keyboard handling — against a
hand-built (if now battle-tested) cache tower on prompt_toolkit. "Optimized"
is the one word in the maintainer's claim that belongs to the Rust side.

**Durability / new capabilities: Rust wins.** Survive-quit runs, entities,
queue, goal, sessions, images — the Python app cannot do these, and they are
the reason the port exists.

**Net:** as a *daily coding cockpit today*, the Python app is the better
tool; the maintainer's intuition is correct on the axes he lives in
(features, density, feel). The Rust port is a better *foundation* with ~60%
of the product missing. Most of the gap is projection-layer work already on
the roadmap; the three contract gaps (token stream, context window, md
tables) need decisions above the client — and the "no token-streaming"
ruling in ROADMAP §7 should be re-examined against this week's lived
evidence, because "the parity floor" the roadmap wants to buy cheaply
demonstrably includes *seeing the model work*.
