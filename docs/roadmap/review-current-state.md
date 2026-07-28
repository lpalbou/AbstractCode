# Current-state UX review: the sparse header, the empty frame, and what the maintainer actually saw

Status: independent skeptical investigation, 2026-07-22 (evening).
Method: read-only. Built `target/release/abstractcode-tui` (mtime 18:16:53,
self-reports v0.3.0 in `/help`; the maintainer's running process 58119 launched
18:17:20 from the same file — we tested the binary he is looking at). Drove it
in a pty (pyte VT screen, `TIOCSWINSZ`, `TERM=xterm-256color`, prefs isolated
via `ABSTRACTCODE_TUI_PREFS_FILE`) against the live gateway at
`http://127.0.0.1:8080`. No source edits, no gateway restarts, no runs started
(boot + replay + reattach GETs, typing, `/help`, Esc only). His real terminal:
Apple_Terminal, truecolor, **271×68** (`stty -f /dev/ttys041` → `68 271`), prefs
pin `coder` + `endpoint:airelay · gpt-5.6-sol`, session `acode-05452bd6bd3c`.

## Verdict up front

**He is seeing something real, and it is worse than a styling complaint.**
Three separate facts stack into the screen he photographed:

1. **The header is not buggy on a clean screen — it paints everything it
   promises.** But at his 271-column width it fills **24–27% of the bar**
   (57 cells of content, then a **180–193-column black gap**, then a
   session id drawn in the theme's faintest ink at **2.77:1 contrast** and a
   3-cell orb). Even fully painted, this bar photographs as "nearly empty."
2. **His exact screenshot — blank bar, faint session id, green dot, nothing
   else — is a reachable, stable state of the app**, reproduced here
   byte-for-byte. One external screen clear (Cmd+K class) plus one passing
   toast leaves the header in precisely that state **forever**: the damage
   tracker repaints only cells it believes changed, the header has zero
   reactive dependencies that fire during a pinned-route run, and the app has
   **no repaint affordance at all** (no Ctrl+L, no periodic refresh; the old
   Python app bound Ctrl+L → `clear_output` + `invalidate`,
   `abstractcode/fullscreen_ui.py:3494`).
3. **First launch fills 22% of the rows of a 36-row terminal** (8 of 36
   non-blank; 28 blank), teaches nothing about workspace / tools / permission
   tier / skills / MCP, shows a composer with **no placeholder text ever**
   (a real bug, see §4.2), and takes **10.3 seconds** to tell you which model
   "gateway defaults" means. The Python app's first paint carried ~9 dense
   fact lines (provider, model, agent, base URL, workspace+mode, state file,
   tool count + permission mode, skills, MCP) plus a two-row footer with
   dropdowns and a context meter. His gut comparison is correct.

The recent waves did **not** make the header emptier (§6) — it was born
minimal. One wave-added teaching feature (phase-aware composer placeholder)
turns out to be dead pixels due to an engine rule, which means the
discoverability work shipped in 0.3.0 renders nothing.

---

## 1. Reproduction: what actually paints (both sizes, real bytes)

Fresh prefs (true first launch), live gateway, settled state (t+6s).

### 120×36 — full screen, verbatim

```text
+------------------------------------------------------------------------------------------------------------------------+
| ▲ AbstractCode  basic-agent  ·  gateway defaults                                                  acode-29ebdfd7b822 ● |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                     ▲ AbstractCode                                                     |
|                                                                                                                        |
|                             describe a task below — the agent runs durably on the gateway                              |
|                                                                                                                        |
|                          /help commands · /workflow agents · /model providers · /theme looks                           |
|                                                                                                                        |
|                                                rendered by AbstractTUI                                                 |
|                                                                                                                        |
|                           · session acode-29ebdfd7b822 · durable memory lives on the gateway                           |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|                                                                                                                        |
|▐                                                                                                                      ▌|
| enter send  esc esc cancel  ctrl+d details  pgup/dn scroll  ctrl+t theme  /help comm… Dark (Abstract) · 127.0.0.1:8080 |
+------------------------------------------------------------------------------------------------------------------------+
[density] blank rows: 28/36
```

### 100×30 — full screen, verbatim

```text
+----------------------------------------------------------------------------------------------------+
| ▲ AbstractCode  basic-agent  ·  gateway defaults                              acode-35847d75fb3f ● |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                           ▲ AbstractCode                                           |
|                                                                                                    |
|                   describe a task below — the agent runs durably on the gateway                    |
|                                                                                                    |
|                /help commands · /workflow agents · /model providers · /theme looks                 |
|                                                                                                    |
|                                      rendered by AbstractTUI                                       |
|                                                                                                    |
|                 · session acode-35847d75fb3f · durable memory lives on the gateway                 |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|                                                                                                    |
|▐                                                                                                  ▌|
| enter send  esc esc cancel  ctrl+d details  pgup/dn scroll  ctrl… Dark (Abstract) · 127.0.0.1:8080 |
+----------------------------------------------------------------------------------------------------+
[density] blank rows: 22/30
```

### The header IS complete — per-cell ink dump (row 0, settled)

```text
fg=accent       '▲ AbstractCode'
fg=text         '  basic-agent'
fg=text_faint   '  ·  '
fg=text_muted   'gateway defaults (lmstudio · ornith-1.0-35b)'   <- appears at t+10.3s
fg=(fill)       '                      '
fg=text_faint   'acode-29ebdfd7b822 '
fg=ok           '●'
```

So at these sizes, on a **clean** screen, the wordmark, workflow, and route
all render exactly as `src/ui/chrome.rs:110-114` intends. The left side is
neither clipped nor eaten by the right-measured-first clip
(`chrome.rs:95-100`: at 120 cols, `clip_to` ≈ col 98 — plenty). Anyone
asserting "the header renders blank on launch" is wrong. What he photographed
is something else — §2.

### His exact configuration (271×68, prefs-pinned route, resumed session)

```text
[header settled] 75 non-space cells of 271 = 27% fill
[header exact]  | ▲ AbstractCode  coder  ·  endpoint:airelay · gpt-5.6-sol                    …(180 blank columns)…                    acode-05452bd6bd3c ●|
[header]        longest interior blank gap: 180 columns
```

The transcript below it was fully dense (0/68 blank rows — the replayed
prior turn fills the pane; the transcript itself is not the problem).

---

## 2. The screenshot state, reproduced: wipe + partial repaint = his exact bar

The engine's damage contract (`abstracttui/docs/design/01-damage-contract.md`)
is "repaint only damaged regions; idle emits zero bytes"
(`abstracttui/src/app/acceptance.rs:110`). That is architecturally excellent —
and it means the app **trusts the terminal to keep every cell it ever
painted**. When that assumption breaks (Cmd+K "clear screen" in Terminal.app,
`printf '\033c'` from a stray process, a terminal glitch), nothing heals.

Experiment (120×36, idle, then one benign toast):

```text
===== before wipe (settled) =====
row  0: | ▲ AbstractCode  basic-agent  ·  gateway defaults (lmstudio · ornith-1.0-35b)   acode-f77e54bcf4cb ●|
row 12..21: (empty-state guidance)
row 34: |▐ … ▌|      row 35: (status bar)

===== 5s after external wipe, idle =====
[blank rows: 36/36]                                  <- nothing self-heals, ever, while idle

===== toast visible (unknown theme) =====
row  0: |                                             acode-f77e54bcf4cb ●|
row  1: |                          unknown theme: zzz (try /theme)|

===== after toast expired =====
row  0: |                                             acode-f77e54bcf4cb ●|     <- HIS SCREENSHOT
row 35: |     r send  esc esc cance|
[blank rows: 34/36]
```

**Row 0 after the toast is, cell for cell, the maintainer's photo**: an
otherwise blank dark bar with only the faint session id and the green orb at
the far right. Mechanism: the toast rests top-right on rows 0–1
(`abstracttui/src/app/popups.rs:184-186`); when its layer is removed, only the
**vacated rect** is damaged, so the root repaints only those columns — which
happen to contain exactly the session id + orb. The left ~85 columns of the
header were never damaged, so they stay blank.

And they stay blank **indefinitely**, because in his configuration the header
never repaints during a run:

- The header `dyn_view` re-runs only when a tracked signal changes
  (`chrome.rs:15-70`). With provider+model pinned in prefs (his case), the
  route arm `(false,false)` at `chrome.rs:69` never reads the fold — so the
  constantly-updating run fold does **not** wake the header. Workflow,
  session, conn, chips: all static mid-run.
- The connection probe runs **only while idle** (`src/ui/mod.rs:1254-1260`),
  and `set_if_changed` (`runner.rs:378`) suppresses same-value writes — so
  even at idle the orb never redraws unless the gateway actually flaps.
- There is **no repaint affordance**: no Ctrl+L binding in the app (grep:
  zero hits), no engine-level manual damage-all exposed to users, no periodic
  refresh. The engine full-repaints only on resize
  (`abstracttui/src/app/acceptance.rs:211` `resize_forces_full_repaint…`) and
  theme switch.

Mid-run it is even starker. Wiping the screen while attached to his live run
(271×68, run at cycle 30, in a long model call):

```text
===== 20s after wipe, live run still streaming =====
row 65: | ⠈                             49|
[blank rows: 67/68]
```

Twenty seconds later the only recovered pixels were the spinner glyph and two
digits of the elapsed counter — the only cells whose values changed. A user
in front of that terminal has a running 9-hour agent job and a screen that is
67/68 blank. The old Python app recovered from this with one Ctrl+L.

**Root cause classification**: not a chrome.rs bug — a missing recovery story
for externally-lost cells, plus a header whose reactive dependency set can go
completely static for hours. Damage-tracked TUIs need at least one of:
(a) a Ctrl+L / `/redraw` damage-all binding, (b) full repaint on focus-in
events (the engine already parses focus reporting), or (c) a slow periodic
repaint of the fixed chrome rows.

(Also reproduced but distinct: toasts rest at rows 0–1 top-right
(`popups.rs:184-186`), so "reattaching to live run …" briefly replaces the
header's right cluster; it repaints correctly on expiry — cosmetic. The
`engine: caps: 16color` toast in these captures is a pty artifact
(`COLORTERM` unset in the harness); his truecolor terminal never shows it.)

---

## 3. Even without the wipe: the bar he photographed is 73–76% void by design

At 271 columns (his window — not exotic; it's a maximized window on a wide
display) the fully-painted header is:

- a ≈57-column left cluster, a **193-column blank gap**, a ≈20-column right
  cluster — 66 non-space cells of 271 = **24% fill** (27% while the transient
  "reattaching to live run …" toast overlapped the bar).
- The right cluster is `text_faint` `#666666` on surface `#16213e` =
  **2.77:1 contrast** — below the 3:1 WCAG floor for UI components; "faint
  session id" is the objectively correct description of the brightest thing
  in the right two-thirds of that bar.
- For the first **10.3 seconds** of a default-route launch the route segment
  reads just `gateway defaults` — the resolved model
  (`(lmstudio · ornith-1.0-35b)`) lands only after, inside the same
  `load_catalog` call, providers discovery **plus up to six serial
  per-provider model backfills** complete (`runner.rs:424-455`;
  `capability_defaults` runs after them at `runner.rs:457`; boot timeline
  measured: workflow at t+0.47s, route at t+10.32s).

Nothing else in the app ever populates that gap: no workspace root, no tool
count, no permission tier, no context meter, no queue count (idle), no
elapsed-cost summary. Compare the Python footer, which spent this exact real
estate on an agent dropdown, a permission-mode dropdown (rendered in a danger
style for full-auto — `fullscreen_ui.py:2595-2646`), a live context/cache
meter, and a help row.

---

## 4. The whole screen, honestly

### 4.1 First launch is 78% dead rows

At 120×36: 8 non-blank rows of 36 (~11% of cells carry ink). The used rows:
header (1), centered empty-state (5, double-spaced, one of which repeats the
wordmark already shown 12 rows above), composer strokes (1), status bar (1).
The activity strip row is **deliberately empty** at idle with zero runs
(`chrome.rs:312-315` returns a bare element) — a permanently reserved blank
line on first launch. The information a new user actually gets: workflow
name, session id (twice), theme name, gateway host, four slash-command
pointers. What they do not get anywhere on screen: provider/model (for 10s),
workspace root and mode, tool count, permission tier, skills, MCP — every one
of which the Python banner printed at boot (`react_shell.py:3790-3830`).

### 4.2 The composer placeholder NEVER renders — dead pixels from the 0.3.0 wave

`src/ui/mod.rs:343-355` builds phase-aware placeholder guidance ("describe a
task — Enter sends · Ctrl+J newline · /help", "Enter steers the run · …") and
threads it into the composer (`chrome.rs:505`). **It has never been visible in
any capture today** — idle, typing, or with a modal open. The engine paints a
TextArea placeholder only when the field is empty **and unfocused**
(`abstracttui/src/widgets/textarea.rs:404-410`), and this composer is
`.autofocus()` (`chrome.rs:542`) — focused from boot, refocused after every
modal close. So the one always-on-screen teaching surface added in the 0.3.0
discoverability work is structurally invisible: the composer renders as two
side strokes around a blank line. Verified by per-cell dump:

```text
[inks] composer row: fg=accent '▐'   fg=(fill) '   …blank…   '   fg=accent '▌'
```

This needs either an engine change (paint placeholder while focused, standard
in every GUI toolkit) or an app-side hint drawn beside the caret. Filed
against the same family as engine backlog items 0220-0250.

### 4.3 Status bar omits the one state with safety consequences

`chrome.rs:664-694`: key legend + theme + gateway host. The accepted tool
tier (`/tools tier read|write|all`) and the `/auto` blanket — the states that
decide whether the agent mutates files without asking — render **nowhere**
persistent. The Python footer displayed permission mode always, in a danger
style for full-auto ("full-auto must not render identically to read-only",
adversary F12 in its own comments). Regression against the predecessor.

### 4.4 In-run activity strip: good density, one readability bug

During his live run the strip showed
`⠁ working · cycle 30 · 33628s · 3.9M tk · 170 tools` — genuinely
informative. But `33628s` is nine hours twenty minutes rendered as raw
seconds (`chrome.rs:387`, `parts.push(format!("{}s", elapsed))`). Nobody can
read 33628s. The entity lane already has `fmt_elapsed` (`convo.rs`); the
agent strip does not use it.

### 4.5 Transient stale pixels during session restore (race, pixel-proven)

Booting into a session with replayable history, the centered empty-state
guidance and the arriving transcript **coexisted on the same rows** for
seconds:

```text
t+4.0s row 30: |1: # Self-check evidence         …    describe a task below — the agent runs durably o|
t+4.0s row 36: |✗ read_file {"file_path":"",…}   …    · session acode-05452bd6bd3c · durable memory liv|
```

One run cleared by t+6s; another still showed ghosts at t+12s (cleared only
by a resize). The empty-state prints with a transparent background
(`transcript_view.rs:735`) and the empty→feed swap does not reliably clear
the vacated centered text. Nondeterministic, but reproduced twice; at 271
cols the ghost is dead-center where the eye rests. (Resize itself settles
clean — after SIGWINCH shrink/grow the engine's full repaint left 0 torn rows
at t+2s and t+8s; earlier-observed tearing was capture-harness timing.)

---

## 5. First-impression verdict (the 10-second test)

Brutal version: **on first launch this reads as an empty prompt, not an agent
cockpit.** A 36-row terminal shows six sparse centered lines, a blank
reserved strip, an unlabeled input line (no placeholder, no `>` prompt), and
a bar that is one-quarter full. The wordmark appears twice before any fact
about capability appears once. The most valuable facts a cockpit shows —
what model, what workspace, what the agent may touch without asking — are
absent or 10 seconds late. Meanwhile the transcript rendering (cards, folds,
markdown, in-place tool status) is genuinely good *once a conversation
exists*, and mid-session density at his size was 68/68 rows used. The product
communicates its quality only after you already trust it with a task; the
first frame communicates "unfinished."

And the frame he actually photographed — the post-wipe header — communicates
worse than unfinished: it looks broken, because it is (as a recovery story).
His glance did not mislead him. The Python app, whatever its architecture,
never showed him that bar: it had Ctrl+L, a prompt glyph, a dense banner, and
a stateful footer.

Where his gut overshoots: "much better and optimized" — the Python UI's
smoothness is the product of months of documented fixes (its own comments
record per-keystroke re-tokenization, 26ms window reformats, wheel judder);
the Rust engine's rendering core (damage tracking, zero idle bytes, keyed
feeds) is the stronger foundation — today's wipe test observed exactly that
contract (an idle app emitted nothing for 5s), which is also why nothing
healed.
See `review-python-regression.md` for the full feature-parity audit — the gap
is a projection/chrome-layer problem, not an engine problem. But foundations
do not photograph. Screens do.

---

## 6. Was there a render regression from the 0.2.0 → 0.3.0 waves? No — and one dead-on-arrival addition

The repo has exactly one commit (`34b9447`, Cargo.toml 0.2.0); the 0.3.0
wave is the uncommitted working tree. Diffing `src/ui/chrome.rs`:

- **Header**: the waves added entity chips + a spin read gated behind
  `any_turn`. With zero conversations (his screenshot; every capture here)
  the chip loop paints nothing and the header output is **byte-identical to
  0.2.0**. The right-measured-first clip, the faint session ink, and the
  giant center gap all exist at HEAD too. Not a regression — a birth defect.
- **CHROME_ROWS = 4** and the **borderless composer** are both already in
  HEAD (`git show HEAD:src/ui/mod.rs` → `CHROME_ROWS: i32 = 4`;
  `HEAD:src/ui/chrome.rs:377-378` → `BorderKind::None`). The waves did not
  strip a frame the maintainer remembers; there was none at 0.2.0 either.
- **Activity strip**: HEAD also rendered an empty element at idle-with-zero
  runs. The waves added queue/goal/entity segments — strictly more content,
  never less.
- The one regression-shaped find: the 0.3.0 wave's **composer placeholder is
  invisible** (§4.2) — teaching that was designed, built, tested at the
  state level, and never painted. The "discoverability" plan item shipped
  zero pixels on the main screen.

---

## 7. What would change the picture (priority order, smallest-first)

1. **Repaint affordance** — bind Ctrl+L (and `/redraw`) to a full-screen
   damage-all; consider full repaint on terminal focus-in. Kills the
   photographed failure mode outright. (App + possibly one engine hook.)
2. **Make the composer look like an input** — engine: paint placeholder while
   focused (or app: draw a `❯` prompt glyph + hint when empty). Restores the
   0.3.0 teaching work.
3. **Fill the header gap with cockpit facts** — workspace mode, tool tier
   (danger-styled for `all`/auto), tool/skill counts, context tokens. The
   180 idle columns at his width are the natural home for exactly the state
   §4.3 says is missing.
4. **Humanize elapsed** (`9h20m` not `33628s`) — one-line fix, reuse
   `convo::fmt_elapsed`.
5. **Resolve the default route before the provider backfill** — reorder
   `capability_defaults` ahead of the six serial `provider_models` calls
   (`runner.rs:424-470`); the header would name the model in <1s instead of
   10.3s.
6. **First-launch empty state** — replace the double-spaced hint poem with a
   compact status card (route, workspace, tier, tools, skills, MCP — the
   Python banner's facts) so the first frame reads as a cockpit.
7. **Chase the empty→feed ghost** (§4.5) — ensure the pane swap damages and
   clears the full pane rect.

---

*Captures archived: /tmp/acode_capture_120x36.txt, /tmp/acode_capture_100x30.txt,
/tmp/acode_his_271x68.txt (+ wipe, timeline, ghost, resize-settle scripts in
/tmp/acode_*.py). Gateway untouched; no runs started; prefs isolated.*
