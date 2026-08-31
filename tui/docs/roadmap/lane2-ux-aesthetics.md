# Lane 2 — Aesthetics · Usability · Information Design (v0.3.0)

Findings for the roadmap, researched 2026-07-22 against `abstractcode`
v0.3.0. Method: full read of `src/ui/*.rs` + `src/transcript.rs` +
`src/commands.rs` + the engine's widget surface (`abstracttui` 0.2.x);
read-only pty drive of the release binary against the live gateway at
110×32 (12 captured frames: boot, `/` dropdown, help, theme picker, tools,
workspace, model picker, entities, queue, cache, composer draft); code +
snapshot study of the references — codex-rs/tui (incl. its 64 insta
snapshots), opencode `packages/tui` (SolidJS/opentui rewrite), pi
`packages/coding-agent`, DeepSeek-TUI (a codex fork; nothing beyond codex
+ a theme picker we already exceed), and the Python `abstractcode` sibling.
No code was changed; no gateway runs were started.

---

## What premium looks like

A premium agent TUI earns trust by narrating the work, not by dumping its
plumbing. Open codex mid-task and the transcript reads like an engineer's
log: "• Ran `cargo test`", "• Read `foo.rs`, `bar.rs`", "• Edited
`src/main.rs` (+3 -1)" with a tinted, line-numbered diff — every cell a
human sentence with the evidence folded underneath. The chrome whispers:
one status line ("• Working (12s · esc to interrupt)"), one footer hint
("? for shortcuts · 63% context left"), everything else is transcript.
Where we are strong — durable runs, honest truncation labels, a
deliberate tier/approval model, entity conversations no reference tool
even has, 26 audited themes with live preview — the missing layer is
almost entirely *presentation*: our default transcript face is raw JSON
argument previews and single-ink text blobs, file changes never render as
diffs, the composer's `@` knows entities but not files, and the numbers a
daily driver steers by (context %, cost) aren't on screen. The good news:
every gap below is a projection-layer change on data the fold already
holds, and the humanization machinery for half of it already exists in
`approval_view.rs` — it just isn't reused where users look most.

---

## Ranked summary

| ID | Sev | Finding | Effort |
|----|-----|---------|--------|
| UX-01 | P0 | Transcript tool cards show raw JSON; humanized rendering exists but only in the approval modal | S–M |
| UX-02 | P0 | File edits never render as diffs — not in the transcript, not in the approval (engine diff tinting unused) | M |
| UX-03 | P0 | `@` completion is entity-only: no `@file` mentions (codex + Python sibling both have them) | M |
| UX-04 | P1 | Approval modal: you approve a `write_file` without seeing the content; no deny-with-reason; "approve all" wording ambiguous | S–M |
| UX-05 | P1 | No context-window % or cost meter — the two numbers every reference footer carries | M |
| UX-06 | P1 | No type-to-filter in pickers: `/model` stage 2 is a 342-row arrow-key wall | M |
| UX-07 | P1 | Status-bar legend overflows (truncates at 110 cols); replace with codex's `?` progressive disclosure | S |
| UX-08 | P1 | Truncation notes point at "the run ledger" with no way to open it — no full-content viewer or export | M |
| UX-09 | P1 | First-run: no session header (version/model/cwd/workspace); header never shows the working directory | S |
| UX-10 | P2 | Answers pop in whole (record-level); no "writing answer…" activity while final text is in flight | S |
| UX-11 | P2 | Entity drive ratios render as `q 0/6 · p 0/2 · i 0/85` — cryptic to anyone but the author | S |
| UX-12 | P2 | Composer has no prompt affordance (no `❯`, no multiline row hint) | S |
| UX-13 | P2 | Repeated tool cards never coalesce (a 12-read burst = 12 cards; codex folds to one "Read" cell) | M |
| UX-14 | P2 | `/` dropdown: prefix-only match, ~6 visible rows, no argument hints | S |
| UX-15 | P2 | Boot notices accumulate as loose `·` info lines instead of one structured cell | S |
| UX-16 | P3 | Theme picker: title self-truncates at 44 cols ("Enter keeps…"); no accent swatch per row | S |
| UX-17 | P3 | Help modal: description column truncates under the scrollbar column; key gutter fixed at 18 | S |
| UX-18 | P3 | Glyph audit: 9 distinct card glyphs with mixed visual weight (`❯ ✦ ∴ ◆ ✎ ⚙ » ⊘ ▦`) | S |
| UX-19 | P3 | Modal panels are borderless (overlay-token ground only) — verify edge contrast across all 26 themes | S |

Section 9 of this document lists reference capabilities we lack entirely
(backtrack-edit, shell passthrough, external editor, transcript overlay,
image paste, sidebar/which-key) with adopt/skip recommendations.

---

## 1. First-run + empty state

**What a new user sees** (captured frame, 110×32, live gateway):

```
▲ AbstractCode  basic-agent  ·  gateway defaults              acode-0ad8… ●
                          ▲ AbstractCode
        describe a task below — the agent runs durably on the gateway
     /help commands · /workflow agents · /model providers · /theme looks
                        rendered by AbstractTUI
        · session acode-0ad8… · durable memory lives on the gateway
▐                                                                        ▌
 enter send  esc esc cancel  ctrl+d details  pgup/dn scroll  ctrl+t th… ●
```

The value proposition ("describe a task… durably") lands in 10 seconds,
the gateway-down variant teaches recovery (`empty_state`,
`transcript_view.rs:721-783`) — genuinely good bones. What's missing
against the references:

- **UX-09 (P1) — no session identity card.** Codex prints a
  `SessionHeaderHistoryCell` as the first transcript cell: version, model
  + reasoning effort, **working directory**, date
  (`history_cell.rs:928-1005`). Our version appears only inside `/help`
  (`modals.rs:1917-1923`), and the working directory appears *nowhere* in
  the main UI — it's only inside `/workspace` (`modals.rs:1650+`). For a
  coding agent, "which directory am I pointed at" is first-order
  information. The wordmark is also duplicated (header row + centered
  empty state, same `▲ AbstractCode` twice on one screen).
  - **Change**: replace the centered wordmark block with a compact session
    card: `▲ AbstractCode v0.3.0 · basic-agent · lmstudio · ornith-1.0-35b`
    / `dir ~/tmp/…/abstractcode · workspace server-managed · session
    acode-…` / one guidance line / one command hint line. Reuse the same
    card as a `/status` output later (codex `status/card.rs` pattern).
  - **Success**: at boot, model + directory + workspace mode readable
    without opening any modal; wordmark appears once.
  - **Effort**: S.

- **P2 (folded into UX-09)** — the header names the *route* but the
  provider/model only resolve after the first fetch lands
  (`chrome.rs:47-70` does this honestly); at boot it reads bare "gateway
  defaults" (frame 01) then upgrades (frame 02). Fine — but the header has
  no directory and no tier indicator (see §3).

- **Discoverability of the command surface is good**: `/` opens the
  dropdown, the empty state names the four highest-value commands, `/help`
  is complete (the full command surface + key table, `commands.rs:166-284`). Tier and
  workspace are *taught* only inside `/help` prose and modal footers —
  acceptable, but see UX-07 for making the accepted tier visible at rest.

## 2. The transcript

Cards (`transcript_view.rs:249-401`): `❯ you` (accent), `↪ steer` (warn),
`∴ cycle N` thinking (faint, 10-row cap, details-gated), tool cards
(status glyph + name + `args_preview` + faint `result_preview`, 6-row
cap), `✦ assistant` + real markdown body, `▦` image (mosaic), `·` info,
`✗ error`, `◈` probe. The engine feed does keyed updates, follow-tail,
and code-fence highlighting in markdown bodies. The details toggle
(Ctrl+D) folding finished tool + thinking cards is a genuinely good
clean-mode design, with the right exceptions (active/failed/denied stay).

- **UX-01 (P0) — tool cards are raw JSON.** `args_preview` is a one-line
  JSON dump capped at 200 chars (`transcript.rs:19,1001`;
  `value_preview`), rendered as the card's detail
  (`transcript_view.rs:313-314`). Details mode is ON by default
  (`store.rs:300`), so *the default face of a working session is JSON
  fragments*: `execute_command  {"command":"cargo test --lib","cwd":…}`.
  Codex renders every exec as a sentence with bash highlighting — "• Ran
  `cargo test --lib`" (`exec_cell/render.rs`), reads as "• Read foo.rs",
  searches as "• Searched pattern in dir". **The sentence-builder already
  exists in our codebase**: `approval_view.rs:205-264` (`intent_summary`)
  produces exactly "write src/main.rs" / "run a shell command in /tmp/x"
  and is only used by the approval modal.
  - **Change**: route transcript tool cards through `intent_summary` +
    first-class command extraction (the `$ cmd` treatment,
    `approval_view.rs:64-86`): header = `✓ write src/main.rs`, detail =
    the tool name + key params, body (details mode) = result preview.
    Keep the raw JSON one keypress away (see UX-08).
  - **Success**: a finished session read top-to-bottom contains zero `{"`
    sequences outside code fences.
  - **Effort**: S–M (pure projection change; the fold already carries
    name + args `Value` via the runner — note `Item::Tool` stores only
    the *preview* string today, so the fold needs to keep the summary or
    the raw args per card).

- **UX-02 (P0) — no diff rendering anywhere.** The engine ships diff
  tinting (`FeedBlock::Code { lang: "diff" }` → `code::diff_token_color`,
  engine `feed.rs:80-89`, `code.rs:61-90`) and our transcript never uses
  it: tool results render as a single-ink faint text block
  (`transcript_view.rs:318-320`). A `write_file`/`edit_file` outcome — the
  most consequential thing an agent does — is indistinguishable from any
  other blob. Codex's most-snapshotted surface is exactly this:
  "• Edited example.txt (+1 -1)" with line-numbered, tinted hunks
  (`diff_render.rs:79-200`, snapshot `apply_update_block.snap`); opencode
  renders the same diff inside the *permission* dialog with full diff
  theming (`routes/session/permission.tsx:33-80`).
  - **Change**: (a) for `edit_file`/`write_file` tool cards, derive a
    unified diff client-side when the result carries before/after or the
    args carry find/replace + path (compute what we can honestly; label
    what we can't); render via `FeedBlock::Code{lang:"diff"}` with a
    `(+N -M)` count in the header line; (b) same block inside the
    approval modal body (see UX-04). Where only the new content is known
    (fresh `write_file`), render the content as a syntax-highlighted
    `Code` block with `lang` from the file extension instead of faint
    plain text — still a massive upgrade.
  - **Success**: an edit approval and its finished card both show tinted
    +/- lines with a count; write of a new file shows highlighted content.
  - **Effort**: M (diff computation + two render sites; engine work zero).

- **UX-13 (P2) — no coalescing.** An agent skims 12 files → 12 cards with
  12 JSON previews. Codex coalesces sequential reads into one cell that
  dedupes names ("coalesced_reads_dedupe_names" snapshots,
  `history_cell.rs`). With UX-01's sentences in place, fold consecutive
  same-tool OK cards (read/list/search families) into one card listing
  targets: `✓ read 12 files  foo.rs, bar.rs, … (+9)`.
  - **Success**: a 12-read burst occupies ≤3 transcript rows in clean
    mode and expands under details. **Effort**: M (fold-level change; the
    keyed feed handles in-place replace already).

- **UX-10 (P2) — answers pop in whole.** The gateway ledger streams
  step *records*, not token deltas (verified: no delta shape in
  `protocol.rs`), so a long answer appears at once after a silent gap.
  Codex streams markdown with a commit animation; we can't without a
  gateway lane — but the fold already knows a model call is in flight
  (`llm_inflight_since`, used at `chrome.rs:417-426` only after 60s).
  - **Change**: while an llm_call is in flight and the run's last cycle
    has produced tool results, set the activity to `writing…` (with
    elapsed) instead of generic "working"; keep the 60s slow-provider
    upgrade. Optionally: when the answer record lands, reveal
    section-by-section (markdown block-level) over ~300ms — the engine
    feed can push the answer as several keyed blocks. File the
    token-streaming lane as a gateway ask, honestly labeled.
  - **Success**: the strip never reads generic "working" while the model
    is composing the final answer. **Effort**: S (activity); M (reveal).

- **Streaming/final distinction is good**: `assistant (update)` (muted)
  vs `assistant` (ok ink) (`transcript_view.rs:327-336`) — keep.

- **UX-15 (P2) — boot notices as loose lines.** Session id, workspace
  policy, replay notes each land as separate `·` info items
  (`Item::Info`, prefix `· `, `transcript_view.rs:371-378`) and linger in
  scrollback. With UX-09's session card most of these fold into one
  structured cell; keep `Info` for genuinely transient notices.
  **Effort**: S.

## 3. Activity strip + header + status bar (`chrome.rs`)

Strong content: wait-owns-the-strip (`chrome.rs:156-189`), pause line,
entity-focus mirror, queue depth in every phase, goal prefix, cycle,
elapsed, in/out tokens, ctx tokens, cache, tool count, slow-model
callout, output sparkline. This is more *truth* than codex's one-liner —
but it's presented as one long `·`-joined string that regularly clips,
while codex's `• Working (12s • esc to interrupt)` + queued `↳` lines
under it read instantly.

- **UX-05 (P1) — the missing numbers: context % and cost.** We show
  `ctx 41k` (last input tokens, `chrome.rs:400-404`) but never *against
  the model's window* — codex's footer says "63% context left"
  (`footer.rs:context_window_line`), pi's footer color-codes the %
  (>90% red) and adds `$0.412` cost + cache-hit %
  (`pi footer.ts:100-157`). At 40k-in on a 32k-window local model the
  user learns about overflow from an error.
  - **Change**: resolve the effective model's context window (gateway
    capability probe — the `/cache` lane already resolves the effective
    route + `served by` model, `modals.rs:1445+`), render `ctx 41k/262k
    (16%)` in the strip, warn-tinted past 75%, error past 90%. Cost:
    where the provider reports it (openrouter/anthropic price sheets are
    gateway knowledge), show `$0.04`; local providers honestly show
    nothing. **Success**: context % visible during every run; turns red
    before an overflow error can surprise. **Effort**: M (needs the
    window figure via gateway; the rest is arithmetic).

- **UX-07 (P1) — the legend is a run-on sentence.** Six key hints + theme
  + gateway host truncate even at 110 cols (captured: `ctrl+t th… ●`);
  at 80 cols the tail disappears entirely (the code clips honestly,
  `chrome.rs:652-700` — but honest clipping of a legend is still a lost
  legend). Codex ships a one-item footer — "? for shortcuts" + context %
  — and `?` opens a two-column shortcut overlay (`footer.rs:130,216-306`).
  - **Change**: default footer = `? shortcuts · <accepted-tier> ·
    <theme> · <gateway>` (tier surfaces at rest — today the accepted tier
    is invisible outside `/tools`); `?` (composer-empty) opens the
    existing help modal, or better a compact two-column overlay of just
    the keys. Keep the phase-swapped teaching (enter steers / /queue) as
    the *placeholder* text, which already does this job
    (`ui/mod.rs:343-355`). **Success**: footer fits at 80 cols with zero
    truncation; every removed hint reachable within one `?`. **Effort**: S.

- **Header (P2, folded into UX-09)**: `▲ AbstractCode · workflow · route ·
  chips · session · orb` is well-ordered and the chip paint plan
  (whole-or-`+N`, focused-first) is thoughtful (`chrome.rs:115-141`).
  Missing: the working directory (codex shows it) and any hint that
  `ctrl+e` cycles the chips (the legend only shows it under entity
  focus). Consider dimming the session id further — it's the least
  actionable header item but visually competes with the route.

## 4. Approval modal (`approval_view.rs` + `modals.rs:249-428`)

The rewrite is strong: per-call cards, tool + needed tier headline,
intent sentence, first-class `$ command`, aligned param rows, honest
truncation, `f` full JSON, tier line that re-renders live, Esc-defers-
not-denies (a genuinely better contract than most tools), batch
separators (`── call 1/3 ──`). Server-truth tier preference
(`build_call_views_with`) is the right call. Compared with codex/opencode
three gaps remain:

- **UX-04a (P1) — you can't see what you're approving for file writes.**
  A `write_file` card shows `content  fn main() { (+2 more lines)`
  (`format_scalar`, `approval_view.rs:146-159`) — the *first line* of the
  file. Codex renders the full patch inline in the approval
  (`approval_overlay.rs` → `DiffSummary`); opencode renders the tinted
  diff in the permission dialog. This is the single highest-stakes
  approval and we show 1 line of it. **Change**: for
  `write_file`/`edit_file` calls, the modal body renders the UX-02 diff/
  highlighted-content block (scrollable — the body scroll exists).
  **Success**: an edit approval shows every changed line without leaving
  the modal. **Effort**: S once UX-02 lands.

- **UX-04b (P2) — deny is mute.** Deny sends a fixed `"Denied by user"`
  (`modals.rs:295`). Codex's overlay offers "No, and tell Codex what to
  do differently" — the deny that *teaches*. **Change**: `d` denies as
  today; `D` (or a second stage after `d`) opens a one-line reason input
  appended to the payload. **Success**: a denial can carry a reason
  without touching the composer. **Effort**: S.

- **UX-04c (P2) — "approve all (A)" is ambiguous.** In a batch of 3
  calls, "approve all" reads as "approve these 3", but it actually arms
  session-wide auto-approve (`modals.rs:315-322`). Codex words it "Yes,
  and don't ask again this session". **Change**: relabel button + hint to
  `always allow (session) (A)`; keep the toast. **Effort**: S.

- Worth keeping as-is: modal-over-scrim (codex uses a bottom pane that
  keeps the transcript visible — attractive, but our activity-strip
  "approval needed — Enter opens" + Esc-defer already covers the
  "glance at context first" need, and a modal fits the engine's overlay
  model). Not worth relitigating now.

## 5. Composer

Multiline TextArea (1–4 rows), Enter/Ctrl+J/Alt+Enter/Shift+Enter matrix
handled honestly per wire (`chrome.rs:483-620`), history recall at buffer
edges, block paste, phase-aware placeholder teaching steer/queue, `/`
completion with two carefully-reasoned Enter rules, drag-selection
clearing on type. Solid. Gaps:

- **UX-03 (P0) — no `@file` mentions.** Our `@` trigger completes
  *entities only* (`chrome.rs:590-606`, `mention.rs`). The Python sibling
  ships `@file` workspace mentions (`abstractcode/file_mentions.py`);
  codex has an async fuzzy file-search popup on `@`
  (`bottom_pane/file_search_popup.rs`). For a coding agent, pointing the
  model at a file is *the* core prompt gesture — its absence is the
  biggest parity gap in daily use.
  - **Change**: extend the `@` provider: entities first (they're few),
    then workspace-file candidates from a cached, gitignore-aware file
    index (walk the workspace root once at boot + on demand, cap depth;
    the mention inserts a relative path; the prompt carries it verbatim —
    the gateway agent's read tools resolve it). Disambiguate visually in
    the dropdown (`◆ castor — entity` vs `src/main.rs — file`). Entity
    names win exact-match collisions (current adopt rules stay).
  - **Success**: typing `@main` in a prompt offers `src/main.rs`;
    accepted mention round-trips into a successful `read_file` without
    the model guessing paths. **Effort**: M (index + provider + tests;
    no engine work — the anchored completion already supports it).

- **UX-12 (P2) — no prompt affordance.** The composer is a bare row with
  `▐ ▌` side strokes (captured frame 12) — no `❯` prompt glyph, no
  visual "this is where you type" beyond the placeholder, no row-count
  hint when the draft goes multiline (codex `›`; pi gives user input a
  background bubble). **Change**: accent `❯` glyph in the composer's
  left gutter (focus-aware: accent when focused, faint when a modal owns
  input); optional `2/4` row badge right-aligned while multiline.
  **Effort**: S.

- **UX-14 (P2) — dropdown polish.** Prefix-only filtering
  (`c.starts_with(query)`, `chrome.rs:578-588`) — codex fuzzy-matches
  (`command_popup.rs:13`); `/wf` finds nothing here. ~6 visible rows for
  a 25-command surface (captured frame 02). No argument ghost after
  accept (codex renders arg placeholders for prompts,
  `prompt_args.rs`). **Change**: subsequence/fuzzy match; lift the panel
  cap to ~10 rows; append a dim usage hint for commands with args
  (`/steer <guidance>`) in the detail column (already partially there).
  **Effort**: S.

## 6. Entity surfaces

This is novel territory (no reference has it) and mostly legible: chips
with elapsed, focus cycling, per-conversation strip lines with honest
non-interruptibility wording, hold-the-draft semantics taught in the
placeholder + help, the roster modal opening instantly on cache with an
"as of HH:MM — refreshing…" title (`entity_modals.rs:1-27` — an
excellent honesty pattern), identity cards with sectioned content +
provenance behind Ctrl+D, `[Enter] talk · [t] task · [e] end` footer.

- **UX-11 (P2) — drive ratios are author-speak.** Roster rows render
  `q 0/6 · p 0/2 · i 0/85` (`entities.rs:54-66`); the captured frame
  shows five entities of it. Nobody outside this workspace decodes
  q/p/i (questions/problems/interests) or `closed/total`. **Change**:
  full words at roster width (`questions 0/6 · problems 0/2 · interests
  1/61`), or keep the compact form and add one legend line above the
  footer (`q questions · p problems · i interests — closed/total`).
  Same treatment where drives appear in the identity card. **Effort**: S.

- **Chip glyph semantics (P3, folded into UX-18)**: `◆castor ✎12s` — the
  `✎` (writing) for a running turn is cute but unexplained; states
  `ready/parked/closed/refused` are words (good). One help line under
  "entity turns" naming the chip states would close it.

- `/task` and `/end` prompts are clear; the task prompt's "recorded
  durably; pickup happens at the entity's own boundary" is exactly the
  right expectation-setting sentence. Keep.

## 7. Theming + visual polish

26 themes with contrast audits, live-preview picker with Esc-revert
(`modals.rs:564-606`) — ahead of every reference (codex themes are a
fork-me feature; DeepSeek-TUI's four themes are its headline addition).
Semantic tokens are used consistently across cards (ok/warn/error/faint
discipline is real). Polish items:

- **UX-16 (P3)**: the picker title "theme — ↑↓ previews live · Enter
  keeps · Esc reverts" self-truncates at its own 44-col width (captured:
  `Enter keeps…`) — widen to ~56 or shorten the title and move the key
  hints to a hint row. Add a per-row accent swatch (`●` in each theme's
  accent — needs a custom row painter or engine `List` rich rows) so the
  list previews *color* before you move the selection.

- **UX-17 (P3)**: help modal description column truncates under the
  scrollbar (`(persiste┃` in the captured frame) — the avail width at
  `modals.rs:1940` doesn't subtract the scroll gutter; and the 18-col key
  gutter wastes width for short keys.

- **UX-18 (P3)**: glyph audit. Card/chrome glyphs in use: `▲ ❯ ↪ ∴ ✦ ▦
  ◈ ✗ ⊘ » ✓ ? ◆ ✎ ⚙ ● ◌ ⏸`. They render at mixed visual weights and a
  few are near-synonyms (`✗` error card vs `✗` conn-down orb; `?`
  awaiting-approval vs `?` shortcuts convention elsewhere). Pick one
  weight family, document the mapping in a table in `docs/architecture.md`
  (the engine's damage contract doc is the precedent), and align
  info/probe/thinking on faint-glyph + faint-label consistently.

- **UX-19 (P3)**: modal panels are ground-fill only (engine `popups.rs:92`
  — `overlay` token over a scrim, no border). On themes where `overlay` ≈
  `bg` luminance the panel edge can melt into the transcript. Audit all
  26 (the engine's contrast harness is the right home) and if any fail,
  give Modal an optional hairline `border` stroke.

## 8. Keyboard model

Consistent and mostly memorable: Enter send/steer/hold, Esc
clear→defer→double-Esc cancel, Ctrl+D details, Ctrl+E focus cycle,
Ctrl+T theme, Ctrl+J newline, PgUp/PgDn scroll, modal letters (a/A/d/f),
picker arrows+Enter+Esc. The Esc ladder is well-designed (draft > defer >
cancel-with-confirm). Frictions:

- **Ctrl+T is spent on theme** — codex uses it for the *transcript
  overlay* (see UX-08); theme cycling is a rare act that already has
  `/theme`. Consider freeing Ctrl+T for the transcript viewer and letting
  the theme live in `/theme` + the footer.
- **`?` is unbound** (composer-empty context) — the cheapest
  discoverability win in the codex playbook (UX-07).
- **Esc-Esc cancels here; in codex Esc-Esc *backtracks*** (edit your
  previous message and fork). Different verbs on the same chord will trip
  codex migrants; nothing to change now, but if backtrack ever lands
  (§9), it must not share the chord with cancel.
- Modal keys are taught in-modal everywhere (hint rows) — good; the
  `/tools` modal's `p pins auto/ask · t cycles tier` is dense but its two
  hint rows carry it.

## 9. What the references do that we don't (adopt / skip)

**Adopt (ranked):**

1. **`@file` mentions** (codex `file_search_popup`, Python sibling) —
   UX-03 above. The core coding-agent gesture.
2. **Inline diffs with counts** (codex `diff_render`, opencode
   permission diff) — UX-02/UX-04a.
3. **Humanized tool sentences + read coalescing** (codex exec/read
   cells) — UX-01/UX-13.
4. **Context-left % (+ cost where known)** (codex footer %, pi cost +
   cache-hit + color ramp) — UX-05.
5. **`?` shortcuts overlay + one-line footer** (codex
   `FooterMode::ShortcutOverlay`) — UX-07.
6. **Transcript overlay / full-content viewer** (codex `Ctrl+T`
   `pager_overlay` + `save_transcript.rs` writes a file) — UX-08: an `o`
   key on a focused tool card (or `/transcript`) opening a scrollable
   full-text pager for exactly the content our truncation notes point at,
   plus `/export` writing a markdown transcript. Our "full text in the
   run ledger" labels (`transcript_view.rs:196-199`, `modals.rs:69`) are
   honest but currently dead ends inside the app.
7. **Deny-with-feedback** (codex approval options) — UX-04b.
8. **Session header cell + tips** (codex `SessionHeaderHistoryCell`,
   opencode home tips) — UX-09.

**Consider later (M/L, real value, not this wave):**

- **Esc-backtrack: edit a previous user message and fork the session**
  (codex `app_backtrack.rs`). Powerful, but our session semantics are
  gateway-durable; forking needs a session-seed contract first.
- **External editor for the composer** (codex Ctrl+X; opencode
  editor integration) — S–M and beloved by heavy users.
- **Image paste into the composer** (codex `paste_image`) — the gateway
  attachment lane exists (Python sibling uploads); M.
- **`!` shell passthrough** (codex ShellCommands) — runs a local command
  and injects output; useful, but crosses our thin-client boundary
  (local exec vs gateway exec) — needs a deliberate ruling.
- **Queued-messages preview under the status line** (codex `↳ first /
  ↳ second` + "alt+↑ edit"): we have `/queue` + count in the strip; a
  one-line preview of the *next* queued prompt would close the gap for S.

**Skip (deliberate):**

- **Sidebar** (opencode todo/files/mcp/context panes): wrong for our
  single-column cockpit at 80–110 cols; the activity strip + modals
  cover it. Revisit only for a wide-screen layout mode.
- **Leader-key + which-key dock** (opencode): heavyweight input model;
  our surface is ~25 slash commands + ~8 chords — a `?` overlay suffices.
- **OSC 133 semantic zones** (pi): nice-to-have terminal integration,
  near-zero user-perceived value here.
- **DeepSeek-TUI**: nothing to adopt beyond its codex base; its headline
  addition (theme picker) is a surface we already lead.

---

## Cross-cutting success criterion for the wave

A user who has driven codex for a month sits down at abstractcode,
runs one real task, and (a) never sees raw JSON in the default view,
(b) reviews a file edit as a tinted diff before approving it, (c) points
the agent at a file with `@`, (d) always knows how full the context is,
and (e) finds every keybinding within one `?`. Everything else in this
document is polish behind those five.

## Appendix — captured frames

Read-only pty captures (110×32, live gateway, isolated prefs) backing
the findings above: boot/empty state, `/` dropdown, `/help`, `/theme`
(+preview), `/tools`, `/workspace`, `/model` (2 stages), `/entities`,
`/queue`, `/cache`, composer draft. Reproduce with a pyte-driven pty
harness (the repo's `scripts/pty_features_check.py` conventions:
`ABSTRACTCODE_PREFS_FILE` isolation; no prompts sent, so no runs
started).
