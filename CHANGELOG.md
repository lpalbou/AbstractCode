# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org/).

## [0.2.0] - 2026-07-22

The AbstractTUI 0.2.0 adoption wave: the engine now owns the machinery this
app hand-rolled on 0.1.0, and every workaround for the three engine bugs
this project reported (0220/0230/0240 — all fixed upstream) is deleted.
Hardened by four independent review passes (two correctness/architecture,
two live UX against a real gateway), whose findings are folded below.

### Changed (engine adoption)

- **Transcript is `widgets::Feed`**: keyed item cards with in-place updates
  (tool cards flip states without re-rendering neighbors), windowed paint,
  and a MEASURED content extent — the hand-rolled item column, its
  `MarkdownView::rows` height math, and both autoscroll effects are
  deleted. Appends stop being O(items): a fingerprint-driven sync pushes
  exactly the items that changed, and details/theme/session flips rebuild
  through the engine's `clear()` seam.
- **Follow-tail is the engine's** (`Scroll::follow_tail`): pinned to the
  bottom through appends and resizes, wheel-scroll up disengages, reaching
  the bottom (or PgDn/Esc/send) re-arms. A shrink clamp keeps a scrolled-up
  view honest when the content collapses under it (details toggle, session
  switch) — the pane can never render blank.
- **Multiline composer** (`widgets::TextArea`): grows 1..4 rows with the
  draft, Enter submits, Alt+Enter (and Shift+Enter on kitty terminals)
  inserts a newline, multi-line paste inserts whole, ↑/↓ recall submitted
  history at the buffer edges. Drafts, caret, and history survive theme
  rebuilds.
- **`/` command completion at the caret** (`app::anchored::Completion`):
  partial commands offer candidates with hints (Tab/Enter accept, Esc
  dismisses, typing refilters); a fully-typed command or alias submits on
  the FIRST Enter, and prompts that merely mention a `/token` — anywhere,
  including command arguments — never complete.
- **In-app text selection + clipboard** (engine 0270, filed by this
  project): drag paints a selection, release copies via OSC 52; Esc/click
  clears. Native terminal selection stays one Shift/Option-drag away.
  Known engine limit (filed as 0290): right after a drag-selection, a
  leading `c` or bare Enter is consumed as a copy key until another key
  lands.
- **Timers are `reactive::interval`**: the spinner/elapsed ticker and the
  idle connection probe ride the engine's cancellable interval (the run
  ticker starts/cancels on phase transitions, so an idle app stays
  zero-wakeup). The engine's startup-notices lane (capability fallbacks;
  zero-collapse layout diagnostics in debug builds) surfaces as toasts.
- Inline images render as unicode-mosaic custom blocks inside the feed
  (aspect-corrected). Honest regression from 0.1.0, filed upstream as
  0280: `Feed` custom blocks cannot host protocol-grade (kitty/iTerm2)
  images yet; the mosaic ladder is the in-feed rendering until then.
- Deleted: `UiCtx::tree` + `focus_composer` (plain `.autofocus()` now
  works inside dyn regenerations and re-fires on theme rebuilds), the
  absolute-positioned composer + spacer layout (a focus-ordering hack),
  and the composer's redundant outer border (the TextArea's own side
  strokes are the frame — all 4 rows are usable and the caret line is
  always visible).

### Fixed (review folds)

- The first-launch screen teaches again: boot notices no longer bury the
  centered guidance (an Info-only transcript counts as "empty"; notices
  render dim below the guidance). The gateway-down recovery screen is
  reachable in production, not just in tests.
- The header names the effective route in ONE format: `provider · model`
  survives the first run (the provider used to vanish once a served model
  was known).
- The status bar's key legend clips cleanly under the right-side text at
  80 columns (no more mid-word overprinting).
- Image artifacts upsert by id with STICKY SUCCESS: session revisits no
  longer leak bitmap copies, a transient fetch error can no longer shadow
  a later successful fetch — and a re-fetch error can never degrade an
  already-decoded image back to an error card (artifacts are immutable).
- Transcript truncation is chunked (hysteresis): at the 500-item cap the
  view drains in blocks of 100 instead of per-push, keeping feed updates
  amortized O(changed) instead of quadratic during long runs.
- A stale ledger stream can no longer zero the session-totals display
  (the stale-stream guard now covers every signal it touches).
- A runner-thread panic degrades honestly: the error card is joined by an
  Idle phase (no spinner claiming control that no longer exists), and
  ledger-stream panics surface as notices instead of silent freezes. A
  submit AFTER the worker died reverts to Idle with an error card instead
  of wedging the composer in "starting run" against a dead channel.
- Boot notices ellipsize to the pane on the first-launch screen (no more
  mid-word run-off at 80 columns); wrapped notices hang-indent under one
  `·` bullet instead of bulleting every continuation line; the activity
  strip no longer names the cycle twice when the gateway status text
  already includes it.
- The kept one-tick modal-close deferral is re-justified against 0.2.0
  reality: engine 0250 (List activation) landed, but `Button` still
  writes its own signal after `on_click` returns — a mouse-approved modal
  closing synchronously would still crash.

### Changed (engine 0.2.1 follow-up, same day)

- **Pickers activate through the engine** (`List::on_activate`, the fix
  for the 0250 report this project filed): Enter, Space, or a click on
  the selected row confirms in the theme/workflow/provider/model/sessions
  pickers; arrow movement still only browses. The app-side root-Enter
  shortcuts are deleted — activation semantics and disposal safety are
  the engine's now.
- Diff tinting comes free: assistant answers containing ```diff/patch
  fences render added/removed/hunk lines through the theme's semantic
  inks (engine `text::DiffLexer`; no app code — Feed markdown fences
  route automatically).
- MSRV 1.71 → 1.87 (inherited from abstracttui 0.2.1).
- Evaluated, deliberately NOT adopted (engine ask 0296 filed): the new
  `app::select` faces (`Select`/`Combobox`/`MultiSelect`) open only from
  their own trigger rows — a command-summoned picker (`/theme`, `/model`)
  would cost an extra keystroke and duplicate Escape-revert logic across
  the modal and popup layers. The pickers stay `List`-in-`Modal` until
  the faces gain a programmatic open. The kept one-tick modal-close
  deferral also survives 0.2.1 (re-verified: `Button`'s mouse path still
  writes its own signal after `on_click` returns).
- `TextInput::masked` noted and unused: this client has no in-TUI secret
  entry (login is CLI-only, flags/env — never prompts).

### Upstream findings filed (abstracttui backlog, first-app series)

- 0280 Feed custom blocks cannot host widgets (protocol images degrade to
  mosaic) · 0290 selection region lingers after the release-copy and eats
  `c`/Enter · 0292 completion triggers fire on any mid-text token (no
  position policy) · 0294 anchored panels place short candidate lists over
  the chrome below the caret instead of flipping up · 0296 select faces
  need a programmatic open (command-summoned pickers cannot adopt them).

## [0.1.0] - 2026-07-21

First release — the first application built on AbstractTUI. Hardened before
publication by four independent adversarial reviews (concurrency/fold logic,
gateway-protocol conformance, user experience, packaging), whose findings are
folded below rather than shipped as known issues.

### Added (capability-visibility wave, operator feedback)

- `/tools` is now a selector, not a listing: `Space` toggles gateway tools
  on/off (grouped by toolset, `a`/`n` for all), persisted per user. Untouched
  = the workflow's own defaults; once customized, the checked set is exactly
  the allowlist each run receives (`input_data.tools`).
- `/skills` attaches gateway skills to your runs (`input_data.skills`,
  resolved gateway-side); trust levels shown, blocked skills refuse honestly.
- `/mcp` shows the gateway's MCP server registry — including the gateway's
  own guidance when none are declared.
- `/cache` reports the prompt-cache posture for the effective route
  (supported/mode; the gateway auto-enables caching per run), cache hits
  when the provider reports them, and the latest context size.
- `/sessions` picks a recent session to continue (sessions are named by
  their first prompt and remembered across launches).
- Context + cache telemetry in the activity strip: `ctx <n>` (input tokens
  of the latest model call) and `cache <n>` (tokens served from cache).
- The header names what "gateway defaults" resolves to: the gateway's
  configured text route, replaced by the model that actually served once a
  run reports it.

### Fixed (capability-visibility wave)

- `cargo test` could overwrite the operator's real preferences file: the
  headless harness persisted through the default prefs path, clobbering the
  saved theme and writing fixture routes ("qwen-a") into
  `~/.abstractcode-tui/prefs.json`. Preferences now carry their path
  explicitly and default-constructed prefs are ephemeral no-ops — the
  pollution class is structurally dead (regression-tested).
- Selector modals (`/tools`, `/skills`) could silently cut the bottom of a
  long list with no way to reach it: the row window used precomputed chrome
  arithmetic instead of the rect the layout actually granted. Windows now
  size against the real rect, follow the cursor, and show honest `↑/↓ N
  more` overflow markers on the edges (live finding, regression-tested).
- `/details` off is now a real answers-only view: finished tool cards fold
  away entirely instead of leaving a wall of headers (which made the toggle
  look like a no-op on tool-heavy runs). Active, failed, and denied tools
  and all errors remain visible in both views.
- Approve all: the approval modal gains `A` / "approve all (A)" — approves
  the batch and auto-approves later batches for the SESSION (auto-resumed
  with a toast naming the tools). `/auto` toggles it; it never persists and
  resets on `/new` or a session switch; a failed auto-resume falls back to
  the prompt instead of retrying forever.

### Added (durable session restore + pause/resume)

- Quit/crash → relaunch → same state: boot now REHYDRATES the session's
  prior turns IN FULL DETAIL from their gateway run ledgers — prompts,
  reasoning cycles, tool cards, and answers fold through the exact same
  code path as live streaming (the details toggle governs display; session
  token totals restore too), then the live run reattaches — including its
  original user prompt (the ledger replay alone showed an answer with no
  question). Client-carried conversation context is rebuilt from the
  restored transcript, so follow-ups keep their memory across restarts.
  Depth: last 20 turns by default (`--replay-turns N`, 0 disables) — each
  turn costs one history-bundle fetch carrying complete run-tree ledgers.
  Live-caught shape fact: the bundle wraps each run's ledger as
  `{items: [...]}`; the reader tolerates bare arrays too.
- `/pause` and `/resume`: durable gateway-side pause of the whole run tree
  (`pause` command; stops at the next step boundary, keeps nothing burning,
  survives quitting the client) and the matching resume. The paused state
  owns the activity strip, and reattach detects an already-paused run.

### Fixed (overnight operator findings — two fable5 sub-investigations)

- Restored transcripts froze every large tool card at "? awaiting approval":
  two stacked defects — history-bundle ledger items are `{cursor, record}`
  envelopes (the fold received the envelope and matched nothing), and the
  runtime's ledger dedup replaces >4KB payload fields on waiting/completed
  records with `$slim` markers, so completions carrying results in
  `result.results` never matched the payload-based pairing. The fold now
  unwraps envelopes and builds result views from the self-describing result
  rows — which also fixes the live-stream sibling defect (slimmed
  completions left cards stuck at "running"). Regression fixture is a
  byte-faithful sanitized slice of a real bundle.
- The `/model` picker's model stage was dead to input: modal replacement
  left the OLD layer alive-and-key-eating for a tick (equal-z dispatch
  prefers the oldest layer), and a re-entrant effect flush mid-replace could
  leak a zombie approval modal that swallowed keys forever behind the
  visible list. Modal replacement is now atomic (synchronous layer
  retirement, deferred scope disposal, one epoch bump), stage 2 opens on
  the same keystroke, and a parked approval returns after the picker
  closes.
- Provider-endpoint profiles (e.g. `endpoint:airelay`) showed no models:
  the bulk discovery route served `models: []` for them (gateway-side gap,
  reported via agora and fixed same-hour server-side). The client backfills
  through the gateway's own per-provider models route — reused, not
  re-derived — bounded, and kept as harmless double-coverage for gateways
  predating the server fix.
- Slow inference now names itself: a single model call past 60s adds
  "model call NmSSs — provider may be slow" to the activity strip (live
  finding: a 27B MLX model at ~0.25 tok/s looked idle for 19 minutes).
- `/help` documents terminal-native text selection (hold Shift while
  dragging; Option on macOS Terminal/iTerm) — the engine's mouse capture
  blocks plain drag-select; engine-level selection + OSC 52 copy filed as
  AbstractTUI backlog 0270 rather than hand-rolled here.

### Fixed (live operator findings, same evening)

- A pending approval could end up with NO modal and no visible way back
  (live incident: "awaiting approval" card, no prompt). Three-part fix:
  the wait prompt now RETURNS automatically when a modal opened over it
  (picker/help) closes; a pending wait owns the activity strip with a loud
  "⏸ approval needed — press Enter" line that later records cannot
  overwrite; and Enter on the empty composer reopens the prompt (existing
  path, now discoverable). Regression-tested end to end.

### Fixed (post-wave fable5 adversarial review, 13 findings folded)

- Stale disabled-tool names (from another gateway) could underflow the
  `/tools` title arithmetic (u64::MAX in release, panic in debug) and
  silently hold the client in explicit-allowlist mode: counts and the
  run-start gate now use the disabled ∩ inventory intersection, and any
  toggle prunes stale names.
- The header truncated session ids with a byte slice — a multibyte id
  (`--session`, `/session`, prefs) paniced the render loop every frame;
  truncation is now char-safe.
- Delegate-child model calls no longer relabel the header's served-model or
  the `ctx` chip: the "latest" telemetry folds only from the answer-source
  lane, while cumulative token totals still cover the whole run tree.
- `/cache` no longer substitutes the gateway default route under a
  provider-only override, and the verdict line now names the exact
  provider · model pair it probed.
- The `/tools` untouched title no longer claims "workflow defaults apply"
  for the full inventory (the workflow's own pin decides — and it may be a
  subset); the title says so.
- Preferences saves are atomic (temp + rename): a crash or concurrent
  instance can no longer tear the file into a state that silently minted a
  fresh session id over the operator's continuity.
- `/help` scrolls; its tail (including the newest commands) was silently
  clipped at 80x24.
- Read-only info modals no longer paint a phantom selection bar on their
  first row; selector cursors clamp when the inventory shrinks mid-modal;
  `recent_sessions` deduplicate at load; modal titles/hints ellipsize
  instead of hard-clipping; the live pty scripts clean up their temp prefs
  and the features check's isolation proof now asserts a write only the
  binary can make.

### Added (post-review field findings)

- Conversation memory for live follow-ups: each run carries the completed
  visible turns as `context.messages` (client messages win), immune to
  wrapper bundles whose root runs finalize slowly; server-side session
  history still covers restarts. Proven live by a two-turn pty check
  (`scripts/pty_memory_check.py`).
- Detail toggle: `Ctrl+D` / `/details` switches between the full live view
  and a clean answers-only view (thinking hidden, tool cards collapsed to
  headers; errors always visible). Persisted per user.

### Fixed (pre-release review wave)

- Repeated `ask_user` prompts on the runtime's stable wait key now re-prompt
  (wait identity = key + step occurrence); answered occurrences stay
  deduplicated across stream replays.
- Event-shaped waits (`evt:{scope}:{scope_id}:{name}`) parse the NAME
  segment, so `abstract.ask` event waits prompt correctly.
- A delegated sub-agent's result can no longer end the turn: answers are
  accepted only from the root or the first-level agent run.
- Stale outcomes from a previous run (terminal reports, resume results) can
  no longer flip the composer or clobber the current run's prompts.
- Tool cards: per-run dedup state, id-less call pairing (oldest-first), and
  approval-wait cards no longer duplicate.
- The live activity strip renders reliably (spinner, status, cycle, elapsed,
  token counts); chrome fills the viewport; header/status-bar text clips
  instead of overprinting at narrow widths.
- Modal ergonomics: Esc DEFERS approvals and questions (Enter on the empty
  composer reopens them; `d` remains the only deny); approval modals focus
  the argument scroll, name the run id, and clamp above the composer.
- Connection failures teach recovery (empty state names `doctor`/`login`;
  fatal HTTP stream errors stop retrying and say why; `doctor` reports an
  empty agent catalog as degraded).
- `exec`: exit code reflects the run outcome, honest deadline handling,
  credential hints on 401/403.
- Event-contract parity: `abstract.tool_execution`/`abstract.tool_result`
  render tool cards; `abstract.status` clears on empty; message/answer
  levels render as warnings/errors; media-only finals finish the turn.
- Packaging: MSRV declared (1.71), login store created 0600 from birth,
  fixtures sanitized, documentation claims aligned with behavior.

### Added

- Reactive TUI client for AbstractGateway: durable agent runs with live
  ledger streaming (SSE + polling fallback), reasoning-cycle rendering,
  in-place tool cards, markdown answers, inline generated images.
- Tool-approval and ask-user modals resolving durable gateway waits;
  optimistic clearing with restore-on-failure.
- Mid-run steering (type while running; `inject_guidance` to the cycling
  subrun) and `Esc Esc` cancellation.
- Durable sessions with server-side history replay (`use_session_history`);
  automatic reattach to a live run of the session at startup.
- `/workflow`, `/model`, `/theme` (live-preview picker over 26 themes),
  `/tools`, `/session`, `/new`, `/help` commands.
- `exec` headless one-shot subcommand (polling transport, CLI wait policy),
  `login` (shared store with the Python CLI), `doctor`, `--caps`.
- Session/run token stats with a per-cycle output sparkline.
- Headless UI test suite over AbstractTUI's capture harness, a real-ledger
  replay test, and a live pty smoke script.
