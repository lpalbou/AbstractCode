# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org/).

## [Unreleased]

### Added (the boot animation, 2026-08-21)

- **A launch animation.** Three brand-gradient planes fly in, overshoot
  and lock into the ascending **A** of the Abstract house mark; the
  impact throws sparks and fires a hairline across the stage; the
  `ABSTRACT CODE` wordmark then resolves under it in the SAME half-block
  letterforms the idle screen carries — so the splash does not cut to
  the app, it lands into it. ~1.9 s, and **any key skips it**. Two
  renderers behind one storyboard: truecolor terminals get the real
  software-3D mark (perspective slabs, lambert shading, depth fog,
  motion afterglow, 2× supersampled), everything else gets the same
  three planes rasterized with coverage antialiasing and mosaicked to
  half-blocks. Typography, timeline and beats are shared, so the two
  lanes cannot drift. Preview either without launching the client:
  `cargo run --example splash -- --2d|--3d`.
- **`--animation <on|off>`, and it STICKS.** The flag sets the launch
  animation and PERSISTS the choice to `prefs.json` (`animation`), so
  `abstractcode-tui --animation off` once disables it for good. The
  engine's boot gate still applies on top and can only ever say no
  harder: no tty, `NO_COLOR`, `TERM=dumb`, `ABSTRACTTUI_NO_SPLASH` or a
  terminal that reports itself dumb all skip it silently. The animation
  plays BETWEEN mount and run — the gateway probe, catalog, tool
  inventory and entity roster are already in flight while it plays, so
  it spends wall-clock the client was spending anyway.

### Fixed (the chips row and the strip, 2026-08-21)

- **The attachment chip IS the button now.** Clicking a staged file's
  name opens its preview; the row no longer carries an instruction tail
  telling you which command to type. Each chip is its own element, so
  the layout owns the hit rectangles and the click lands on the file you
  pointed at. When more chips are staged than the terminal can show, the
  row ends in `+N more` rather than cutting a staged file off the edge.
- **Chip names are capped at 20 characters** (the rest is an ellipsis),
  so one long filename — a screenshot, a dated report — no longer owns
  the row that is supposed to show every staged file. Display only: the
  chip still previews its own file, and the full name is spelled out in
  the preview header and the `/attach` manager (which also carries the
  whole path).
- **Each chip carries a `×` that unstages it.** Removing a file no
  longer means opening the manager: click the `×` beside its name. The
  glyph is its own element with destructive ink on hover, so a click
  can never resolve to the wrong action, and it goes through the same
  removal authority as the manager's `x` — including the rule that
  clears an armed drop-undo, which must never outlive the chips it
  names. Both chip actions key on the file's canonical PATH rather than
  its position in the row, so a click always acts on the file you
  pointed at even if the staged set changed underneath it.
- **The strip no longer puts one cycle's words in another cycle's
  mouth.** A reasoning cycle's text reaches the client only in its
  RESULT record, so while cycle 2 was thinking, the newest words the
  client held were cycle 1's — and the strip printed them as
  `thinking (cycle 2) — "I'll inspect the project structure…"` while
  cycle 2 was actually writing something else. The words are still
  shown, now attributed: the live cycle's own words keep the em-dash,
  and anything earlier reads `· last: "…"`. Attribution is by RUN as
  well as by number — cycle counts are per run while the displayed
  number is a maximum across them, so in a delegate tree or a goal loop
  the old comparison could name another agent's cycle. A gist from a
  run that is not the one cycling is neither shown nor allowed to
  displace the cycling lane's own words, and a tool-only cycle (calls,
  no prose) no longer blanks the label.

### Added (look at an attachment before it rides a run, 2026-08-21)

- **Attachments preview: text documents and pictures, from the real
  bytes on disk.** The engine can draw images and wrap text, and a chip
  was still only a filename and a byte count taken on trust — a wrong
  file, or a picture that turned out to be a screenshot of the wrong
  window, only became visible after the upload was permanent.
  `/attach preview` now opens the file itself: a scrolling, line-
  numbered document for text (source, markdown, JSON, logs, SVG, CSV),
  or the picture drawn in the mosaic ladder for PNG and JPEG.
- Three ways in, all the same preview: `/attach preview` (the staged
  chip), `/attach preview <n>` (one of several staged chips, 1-based),
  and `/attach preview <path>` — any local file, which is the useful
  one BEFORE attaching. Inside the attachment manager, `p` or `Enter`
  previews the chip under the cursor; `Esc` closes as everywhere else.
  `↑↓`, `PgUp`/`PgDn` and `Home`/`End` scroll, and the hint row keeps
  saying which row of how many you are on.
- The preview never claims more than it read. A file bigger than 512 KB
  previews its first 512 KB and the header SAYS so (`showing the first
  512.0 KB of 3.1 MB`); invalid UTF-8 is labeled, not silently
  swallowed; tabs expand so indented source previews as it is written.
- A format the engine cannot draw is NAMED, and named as a preview
  limit rather than an attachment problem: GIF, WebP, BMP and TIFF say
  "the preview draws PNG and JPEG; this file still attaches and uploads
  normally", and a PDF says the gateway extracts its text server-side.
  Magic bytes decide, never the extension — a `.txt` holding PNG bytes
  previews as the picture it is.
- Reading and decoding happen on their own worker thread, so opening a
  100 MP photo paints "reading…" instead of freezing the frame, and a
  slow load that lands after you moved to another file is dropped
  instead of repainting the newer preview.
- What the preview CHANGES to make a file readable, it also says: an
  ANSI-colored build log renders as text with "ANSI color codes hidden"
  in the header rather than as `[32mok` garbage, and a UTF-16 document
  is transcoded and labeled instead of being dismissed as binary.
  Carriage-return line endings split into lines (a CR-only file used to
  preview as one run-on line), and line numbers size themselves to the
  file so a 120 000-line log numbers its rows truthfully.
- Resizing the terminal re-wraps the document instead of cutting each
  row at the width the modal originally asked for, the preview fits
  terminals down to 20×8, and the body is freed as soon as its modal
  goes — including when another modal replaces it.

### Fixed (engine bump `abstracttui` 0.3.3 → 0.3.6, 2026-08-20)

- **Ordinary photos render instead of an error.** Progressive JPEG is
  what phone cameras, image editors and "save for web" write by
  default, and the engine's decoder read only baseline frames — so an
  image dropped on the composer, or an image artifact a tool produced,
  came back as `image decode failed: parse: jpeg: progressive JPEG not
  supported (baseline only)` in the card where the picture belonged.
  Progressive frames now decode, to the same picture and the same
  quality as the baseline encode of it. Multi-scan sequential JPEGs
  decode too. PNG and baseline JPEG are byte-for-byte unchanged.
- The decoder's message for a format it genuinely cannot read now names
  what it can: `image: unrecognized format (magic ...); PNG and JPEG
  decode, GIF/WebP/AVIF/TIFF do not`. GIF, WebP, AVIF and TIFF still do
  not decode — the message just stops saying "baseline JPEG".
- Transcript pictures already chose their cell density from the
  terminal's proved capabilities (quadrants on a Unicode + truecolor
  terminal, half blocks otherwise), and `tests/image_pipeline.rs` now
  pins that end to end alongside the decode floor.
- Headless SVG captures no longer distort in Chromium-based viewers
  (engine `Screenshot::to_svg` run-padding fix); test tooling only.

### Added (typing anywhere reaches the prompt, 2026-08-17)

- **Typing, pasting, or dropping a file returns focus to the composer and
  keeps what arrived.** Focus can sit on the transcript — after a click in
  the scrollback or a `Tab` — and typing from there now lands the
  character in the draft and brings focus back with it. `/` opens the
  command dropdown the same way, pasted text inserts with newlines
  normalized, and a dropped file becomes an attachment chip with focus
  returned so you can type the prompt that goes with it.
- Navigation is unchanged: `PgUp`/`PgDn`, arrows and the wheel scroll the
  transcript, `Ctrl` and `Alt` chords reach their shortcuts, and modals
  own their own keys.

### Changed (engine bump `abstracttui` 0.3.1 → 0.3.3, 2026-08-17)

- **Copying while a run streams copies exactly what you highlighted.** A
  live screen selection now holds the transcript still for the length of
  the drag and returns it to the live tail when the region clears, so
  rows that arrive mid-drag no longer move under the highlight. Engine
  behavior; no configuration needed.
- **Editor chords in the composer**: word motion (`Alt+B`/`Alt+F`,
  `Ctrl+←`/`Ctrl+→`), line motion (`Ctrl+A`/`Ctrl+E`, `Home`/`End`) and
  word delete (`Ctrl+W`, `Alt+D`, `Alt`/`Ctrl+Backspace`,
  `Alt`/`Ctrl+Delete`). Hold `Shift` on any motion to extend the
  selection.
- **Scrollbars keep a visible thumb** on long transcripts, and a
  successful copy reports its size (`copied 240 characters (3 lines) to
  the clipboard`) instead of naming the route it took.
- **Migration — conversation focus cycles on `Alt+E`.** `Ctrl+E` is now
  move-to-line-end in the composer, so the focus-cycle binding moved to
  `Alt+E` (Option+E on macOS with "Option as Meta/Esc+"). This is the
  only binding the new chords affect. `/focus <name|agent>` switches
  conversations on every terminal and needs no modifier setting.

### Fixed (selection copy actually reaches the clipboard, 2026-08-16)

- **Engine bump `abstracttui` 0.2.20 → 0.3.1.** Drag-select copied
  nothing on macOS Terminal.app, the VS Code/Cursor integrated terminals
  and Warp: through 0.2.x the engine's only copy route was OSC 52, which
  those terminals ignore (the env pass advertises the capability for the
  kitty/WezTerm/ghostty/foot/iTerm2/Windows Terminal lineage only), and
  the one-time "copies may be ignored" notice was the only sign — every
  later copy in the session failed silently. 0.2.25 added the host
  clipboard fallback (`pbcopy` / `wl-copy` / `xclip` / `clip.exe`),
  default-on through `RunConfig::platform_clipboard`, which our plain
  `App::run()` already uses: no code change here, and the bump carries
  every 0.2.21–0.3.1 fix with it (List accessories, `Element::on_paste`,
  theme modes, Block close affordance, the drawer ✕ hit region).

### Fixed (asks render full + no ledger pointers in user-facing text, 2026-07-26)

Operator rulings after a live screenshot showed a plan-approval ask cut
mid-sentence with a "full arguments in the run ledger" marker and no
visible way to respond:

- **An ask is NEVER truncated**: the ask modal renders the whole prompt
  — wrapped once at the final width, scrollable when long (engine
  `Scroll`, auto-hidden bar). The modal sizes to content up to the
  viewport clamp and ALWAYS reserves the input + hint rows (a height
  squeeze shrinks the question region, never the response affordances —
  the old fixed 70x13 panel capped the prompt at 5 lines AND pushed the
  input/hint below the panel bottom). ↑↓/PgUp/PgDn scroll the prompt
  from the modal root while the TextInput keeps focus (it leaves those
  keys unconsumed); the wheel drives the same offset signal; the clamp
  recomputes against the live panel height so the tail stays reachable
  across resizes. Blank lines in the prompt survive as paragraph breaks.
- **Response affordances unmissable**: the answer input shows its
  placeholder while focused-and-empty (`placeholder_while_focused` —
  the yield-to-caret default left a bare caret box), and the hint row
  now always fits un-ellipsized (the old 88-char hint was cut
  mid-sentence at the default width); when the prompt scrolls, the hint
  advertises the scroll keys. No approve/deny buttons invented: a
  free-text ask wants a text response.
- **No user-facing text points at the ledger** ("the ledger is for
  everything to work, but a user will most likely never read the
  ledger"): every user-visible string naming the run/gateway ledger was
  reworded to a client action or dropped — truncation markers now say
  "shortened for display"; the approval note says "f shows the full
  JSON"; the transcript drop notice teaches scroll-to-top//history; the
  no-readable-answer conclusions point at /status and /history; the
  unreadable-terminal error keeps /doctor only; the offloaded tool
  preview names the artifact plainly; the export INCOMPLETE note
  teaches reopen + scroll-to-top + re-export; entity turn/visit failure
  cards, the summon-timeout notice, stream errors, and replay notices
  drop the ledger clause. The offload fetch-failure label carries no
  pointer at all (it replays into model context — the 2026-07-23
  instruction-kit incident forbids retry framing there). Code comments
  and API paths keep the ledger spelling (ops truth). Pinned by a fold
  sweep test plus headless asserts on both modals.

### Fixed + Added (replay integrity — the lost-turns incident + bloc history, 2026-07-25)

The operator's report ("zero trace of full-rtype-modern... sessions I
can't replay") root-caused by live probe + a fable5 audit, and fixed:

- **THE ROOT CAUSE — a hidden 10 MiB reader limit**: `ureq`'s
  `into_string()` errors at exactly 10,485,760 bytes; single-turn
  history bundles measure 10.7–14.3 MB on tool-heavy work, so the
  operator's BIGGEST turns were deterministically unreplayable — and
  `probe_attach` DISCARDED the cause it had in hand, printing a
  cause-less "(could not be restored)" marker. All JSON lanes now read
  through one bounded reader (`read_body_capped`, 256 MiB ceiling —
  a ceiling against unbounded bodies, not a size model) with a typed
  `BodyTooLarge` error naming path/cap/Content-Length on exceed —
  NEVER truncation (laurent's ruling: truncation violates the ADRs),
  never gateway-down evidence, never retried. Also exposed and fixed:
  `get_ledger` pages (exec's primary transport — the cap could burn
  the whole 900s deadline into rc 124), `attach()`'s live backlog, the
  entity visit-transcript reader. `artifact_bytes` was already the
  correct explicit-cap pattern.
- **No silent failing (the operator's demand)**: restore-failure
  markers are Error items carrying the FULL cause; the "replayed N"
  summary counts honestly (failure markers no longer count as replayed
  — the audit caught "replayed 9" over 7 real turns) and names the
  failure count. All-failures sessions still swap the fold in (the
  markers must render).
- **LIVE-PROVEN**: the incident session (acode-5cb499234d9e) now
  restores both previously-lost turns (14.3 MB + 13.0 MB bundles) —
  pty proof 5/5.
- **Bloc history (laurent's ruling: "we only load the last bloc when
  loading a session... stream rapidly that previous history" on
  request)**: boot lists the session WIDE (200 runs — the old
  list-limit clip silently erased older turns from existence) but
  fetches full bundles for the newest bloc only (`--replay-turns`,
  default now 5); a stub line names the earlier turns
  ("(earlier history: N earlier turn(s)… /history streams the previous
  bloc)"). **`/history [n|all]`** streams the previous bloc from the
  gateway ledgers and PREPENDS it in full detail — items-only (the
  live fold's run state is never touched), stub replaced with the
  updated count, session totals grow by the streamed spend, cause-named
  errors for any turn that fails. Cursor semantics: lexicographic
  `created_at` (the WAIT_UNTIL house rule).
- **Server-audit client follow-ups (same hour)**: stream-resume
  cursors now come from the ledger envelope's own `cursor` field —
  the folded-record count diverges on tail-windowed ledgers and a
  count-based resume re-served duplicates; ledgers whose wrapper
  admits more records than the window carried render an honest
  "(N older records omitted)" Info line (server-side omission is
  silent — the client names what the wrapper admits); offloaded tool
  outputs (`$artifact` refs, >256 KB append-time offloads) render as
  "(large output offloaded to artifact …)" instead of raw ref JSON
  (also closes an instruction-kit lane). The audit measured the 14.3MB
  bundle: 5.95 MB is a slim-dedup MISS (an appended system message
  defeats byte-identity 0-of-59 times — runtime/agent's R3 find),
  and gzip+replay-profile would take 14.29 MB → 0.27 MB (52.6×);
  recommendations posted to the seats with the R4 in-band-warnings
  integrity ask.
- Server receipts on the hub: runtime named the size dominators
  (LLM_CALL STARTED payloads re-carrying full message arrays —
  quadratic per session), said YES to a labeled replay-profile
  projection, and enumerated the integrity boundaries (replay must use
  `mode=full` — tail-mode $slim orphaning; no rotation/pruning exists;
  timeline derives from the same records). GZIP is the gateway's lane
  (~10:1 on these bodies).

### Changed (quit verbs ride a dedicated send — the operator-validated plan, 2026-07-25)

The shared plan (`plans/quit-delivery-contract.md` on the hub; both
seats verified, operator validated) implemented:

- **Dedicated one-shot send**: choosing pause/cancel in the quit modal
  spawns a `quit-verb-send` thread (UiCtx's client clone + the wake
  handle) instead of enqueuing on the worker — the verb can no longer
  queue behind minutes of in-flight HTTP (image fetches, ~30s/file
  uploads, history probes) against a healthy gateway. Transit is one
  HTTP round-trip; delivery survives a busy or dead worker.
- **One send path**: the quit lane never also enqueues on the worker
  (two sends would mint two `command_id`s — the durable store's dedup
  would not collapse them). Slash-command `/pause` `/cancel` stay on
  the worker deliberately (no quit-time urgency).
- **`command_id` minted at the choice site + reused on retry**: one
  same-id retry on transient transport errors — exactly-once by the
  gateway's dedup (runtime receipt c5541). HTTP-status errors never
  retry (the server spoke).
- **One send authority**: `runner::send_verb_blocking` serves both the
  worker handlers and the dedicated thread — acks/toasts can never
  drift between lanes.
- Tests: the pause test inverted per the plan (the worker channel gets
  NOTHING; the real thread against a dead port fails definitively and
  honestly); sequencer ack-matching + stale-ack tests moved to
  deterministic state-level shapes.
- **Scroll-to-top auto-loads history (operator UX ruling, 2026-07-25)**:
  reaching the top of a scrolled-up transcript automatically streams
  the previous bloc of turns — no `/history` incantation needed. The
  stub line becomes a live progress indicator ("streaming N of M
  earlier turn(s)…"), the idle strip names the stream, and holding at
  the top cascades bloc-by-bloc until the session is fully loaded (Esc
  jumps to the tail and stops the cascade). One dispatcher serves the
  slash command and the auto-loader (shared one-in-flight guard); the
  runner's completion paths own stub honesty — success rewrites it,
  a retryable list failure restores the canonical text, and "nothing
  older on the gateway" removes a stub that would otherwise promise
  turns nothing can deliver.
- **Optional coder gating exposed (operator request, 2026-07-27)**: the
  multi-agent coder already ships a `gating_mode` pin (wait|auto, default
  wait); the client now exposes it. Selecting the coder opens a
  gated-vs-unattended choice (default gated); `/gating auto|wait` and a
  `--ungated` headless flag set it; the flag is REFUSED unless
  `--permissions` is also given (an unattended run that skips approval
  pauses must not also run tools under an unstated posture), printing an
  unattended banner. `input_data.gating_mode="auto"` is sent only when
  unattended — absent = gated, byte-parity for existing callers. `/status`
  shows a `gating` row only when unattended, so it is never a silent
  surprise. Switching to a non-gating workflow resets the mode.
- **Long-tool observability (operator: "not a timeout, an observability
  issue")**: the status strip now NAMES the running command
  (`tool call 2m10s · execute_command: npm test`) and escalates as time
  passes — ≥1m gateway-side note, ≥5m "long for a shell command", ≥15m
  "possibly stuck; the model is blocked on this result". Replaces the
  static "large scans can take minutes" that showed for eight hours
  during a wedged browser probe while nothing named what was stuck. The
  root fix (the failure reaching the model so it retries) is flow's
  probe construction + core's timeout output capture — filed to those
  seats; this is the human-facing early cue.
- **Coder fix-cycle progress on the strip (flow multiagent-coding 0.0.7)**:
  the workflow now emits a stable "build cycle N of M" line at the top of
  each repair cycle; the fold surfaces it on the status strip (the glance
  surface) as well as a persistent transcript card, so the fix budget is
  visible as it ticks instead of only when it runs out. Keyed on the
  committed prefix — real intermediate answers stay transcript-only.
- **Ctrl+D clean view keeps the tool CALLS (operator ruling, 2026-07-26)**:
  toggling details off used to hide finished successful tool cards
  ENTIRELY — the whole call trace vanished ("did it even do anything?").
  Now a called tool is ALWAYS shown; the clean view drops only the
  DETAIL (the argument line + the result body), collapsing a wall of
  finished tools to one scannable header line each. Errors, running,
  awaiting-approval, and denied states stay fully visible in both views
  as before. The visibility mirror + feed-order rebuild pins were
  repurposed onto thinking cards (the element that still flips
  visibility on the toggle).
- **Reasoning is a first-class selection axis (operator directive
  c5710)**: the `/model` picker gains a third stage — provider, then
  model, then reasoning effort — with a live per-model capability probe
  (declared reasoning models offer their own levels; registry
  non-reasoners show a locked `none` with a labeled set-anyway
  override; unknown capability offers best-effort — locks rest on
  provenance, never on registry absence). `/reasoning [level]` is the
  fast path (`default` clears), `--reasoning`/`--thinking` covers TUI +
  headless exec, and the choice is pair-coupled: changing provider or
  model resets it. The override rides the EXISTING wire
  (`_runtime.thinking` — one vocabulary: "reasoning" is the UI word,
  `thinking` the wire key), so it works against today's gateway with
  zero server changes. The route label and `/status` name the triple.
- **Thinking folded-by-default + examinable (same directive)**: thinking
  cards render as one-line gists (naming what expansion holds); 
  `/details full` expands content AND the reasoning channel as a
  labeled block — fixing a real defect where the reasoning channel was
  silently DROPPED whenever a cycle also had content; `/details fold`
  returns to gists. Replay parity is free (projection-side rendering
  over ledger `result.reasoning`).
- **Launch starts a FRESH session (operator ruling, 2026-07-26)**: the
  boot no longer auto-reopens the last session — continuity is an
  explicit act: `--resume`/`--continue` reopens the last session,
  `--session <id>` opens a named one, and the in-app `/sessions`
  picker switches anytime (bloc replay included). The minted id is
  still saved so "last" stays known. Headless `exec` already minted
  fresh; the two front doors now agree.
- **History bundles fetch `detail=replay`**: the client's bundle
  fetches ride runtime's replay profile — measured LIVE on the 14.3MB
  incident bundle: 4.1x smaller un-compressed (33.1x with gzip), with
  fold-equivalence PROVEN (1,890 records projected onto the fold's
  exact read set, zero mismatches). Pre-profile gateways ignore the
  query param harmlessly.
- **`/brain` summons declare `caller_kind: human` (gateway seat
  contract, c5603)**: flow-brain turns are typed by a human, so the
  summon carries the human-wins preemption signal — a human summon
  preempts an agent-held seat at the turn boundary; pre-contract
  gateways ignore the field harmlessly.
- **Raster alignment with core's canonical set (c5574)**: `.tif`/`.tiff`
  now upload as `image/tiff` (the octet-stream gap made a declared
  image ride modality "file" server-side), and the attach caveat scopes
  confidence honestly — PNG/JPEG/GIF/WebP get the plain vision note;
  TIFF/BMP say most vision providers reject them even though core's
  gate accepts them.
- **Auto-load adversary fixes (same day, fable5 review)**: a runner
  panic mid-load no longer leaves `history_loading`/`restoring` stuck
  true (worker-death cleanup resets the history lanes and restores the
  stub); PageUp on a transcript that FITS the pane no longer disengages
  follow at offset 0 (follow now derives from geometry, mirroring the
  engine's wheel rule — one no-op keypress used to cascade the whole
  session in); the list-failure and none-older completion posts gained
  the success path's stale-session guard; a dead-worker dispatch
  restores the stub text it rewrote; `/history` during an in-flight
  bloc says so instead of dying silently. Client also RULED OUT on the
  operator's vision incident (all four surfaces audited: run-input
  pinning, attachment kinds/content-types, error rendering, camera
  seeding — findings in `untracked/reviews/vision-and-autoload-review.md`).
- **Server `warnings[]` rendered (runtime R4, same day)**: history
  bundles now carry in-band degradation warnings (tail windows,
  torn-row skips, subtree discovery failures — runtime receipt c5558);
  the fold renders them as Info lines AHEAD of the transcript,
  schema-tolerant and capped, including on ledger-less bundles. A
  replay that cannot be complete now says so in the transcript itself.
- **Post-ship adversary fixes (D1–D4)**: failure honesty is now
  CLASS-derived — `VerbAck.definitive` marks whether the command
  definitively did NOT land (refused/HTTP status on every attempt) vs
  may still land (any ambiguous timeout/transport attempt: the request
  may have left with only the response lost); a blanket
  `definitive: true` overclaimed exactly there. The exactly-once
  contract gained its missing pin: a mock-gateway test captures both
  HTTP attempts (drop-then-200) and asserts the retry reuses the SAME
  `command_id` with zero worker involvement. The late-ack test now
  drives state directly (pressing `p` spawned a real send thread whose
  err-ack could race the synthetic one). Stale worker-queue rationale
  scrubbed from `quit.rs`/`store.rs` comments and corrected in the
  design doc. Accepted deviations recorded: no `Delivering.command_id`
  field (consumer-less), raw thread spawn without panic surfacing
  (bounded by the 8s timer).

### Fixed (quit-gate audit findings — the operator's delivery worry, 2026-07-25)

The operator challenged the delivery-guarantee claim ("the gateway keeps
the states — why would pause not work?"); a client-side fable5 audit
CONFIRMED the claim true and correctly scoped (the gateway holds every
state it HAS; the quit-time verb exists only inside the client until
its HTTP POST lands — the loss window is real and large: the worker's
sequential loop can hold a pause behind 0ms→60s+ of queued HTTP) and
CONFIRMED the thin-client model for leave/disconnect (leave sends
nothing; SSE close is client-side only; no detach call exists; boot
reattach closes the loop). Three findings fixed:

- **P1 — the post-teardown echo never printed**: the outcome mirror
  existed but was never read after `app.run()` returned — a successful
  pause-then-quit exited with NO confirmation at all. One read at
  teardown prints it ("run … paused durably — /resume after relaunch
  continues it" / "NOT confirmed — relaunch to check").
- **P2 — Failed-state copy split per cause**: "stays queued and may
  still land" was FALSE for definitive failures (the gateway answered
  with an error; no retry exists for pause/cancel) — definitive
  failures now say "was not accepted — the run keeps executing";
  timeouts say "may still be in flight — staying keeps it alive (a
  late confirmation still quits)". The stderr echo splits the same way.
- **P2 — late acks are honored**: an ack landing AFTER the 8s timeout
  used to leave the modal claiming "not confirmed" beside a "paused
  durably" toast — a matching ok-ack in the Failed state now completes
  the declared intent (Acked → quit). Test-pinned.

Server-side confirmations on the hub (gateway seat, same hour): the
durable command store IS the operator's proposed (b) queue — commands
write durably BEFORE the 2xx, `command_id` dedup covers retries, and a
client dying after the 2xx (or a gateway restart) changes nothing. One
nuance routed to runtime: the append flushes but does not fsync
(power-loss-only window; fsync recommended, their call).

### Added (quit-with-live-run gate — leave / pause / cancel, 2026-07-25)

Operator ask, refined by a fable5 adversarial design pass
(`untracked/reviews/quit-modal-design.md`): quitting the thin client
never stops the gateway-side run — the quit gestures now say so and
offer the choice.

- **One gate, three gestures**: `Ctrl+Q`, `/quit`, and double-`Ctrl+C`
  funnel through `request_quit` — idle quits instantly
  (byte-identical to before); a live agent run (Running/Starting,
  waiting-on-you, durably paused, goal runs) opens the modal. Entity
  visits never gate (ruled: visits park on quit; a mention line rides
  the modal when one is open); queued prompts never gate (persisted,
  restored paused).
- **The three verbs + stay**: leave running (Enter — the default and
  the only choice that changes nothing; relaunch reattaches), pause
  then quit (`p`, hidden for already-paused runs), cancel then quit
  (`c` — never a default, never reachable by hammering), Esc stays.
  Repeat quit gestures always resolve to leave — `Ctrl+C`×3 always
  exits, at worst one press slower than before.
- **The delivery guarantee (the designer's silent-no-op trap)**:
  pause/cancel are worker-thread HTTP commands and the worker dies
  un-joined with the process — quitting immediately would deliver the
  verb only by luck. Choosing a verb enters Delivering: the app quits
  only on the gateway's ACCEPTANCE (structured `VerbAck` posted by the
  worker — toast text is never matched), bounded by an 8s timeout into
  an honest Failed state ("the command stays queued in this app and
  may still land if you stay; quitting now abandons it") with
  quit-anyway/stay. A dead worker fails immediately with the honest
  wording — no fake spinner.
- **Races closed**: a run concluding under the open modal auto-quits
  (intent was declared), and the queue drain is suppressed while a
  quit is in flight — a Success conclusion must not start new work
  under a quitting user (the item persists and restores paused).
  Stale acks (earlier /pause of another run) are ignored by
  verb+run_id match.
- **Post-teardown honesty**: one stderr line names the outcome where
  the user can still read it — leave ("still executing — relaunching
  reattaches"), acked pause/cancel ("paused durably" / "cancel
  accepted, applies at the next step boundary"), or quit-anyway ("NOT
  confirmed — relaunch to check").
- **Amendment flagged for the operator**: double-`Ctrl+C` with a LIVE
  run now opens the gate instead of quitting outright (the 2026-07-23
  two-presses ruling kept two presses NECESSARY; they are no longer
  SUFFICIENT mid-run). The designer's alternative (bypass + stderr
  teach line) is a one-line change if rejected.
- **Engine finding (fixed app-side)**: a draw-only element under
  nested `grow(1.0)`/`basis(Cells(0))` measures zero and never paints
  — the modal body renders as intrinsic `line(1)` rows instead.
- Six headless tests pin the gate (instant idle quit, teach line +
  Esc-stays + leave-sends-nothing, ack-then-quit, failed-ack +
  stale-ack-ignored + quit-anyway, conclusion-auto-quit +
  drain-suppression, repeat-gesture + unbound-Starting verb disable).

### Added (deferred items completed — image previews, /status, cycle intent, 2026-07-25)

Operator ask: "complete all prior tasks, including on file and image
attachments" — the recorded deferrals, closed:

- **Image-attachment previews** (attachments design v2 echo): an image
  chip riding a run renders a mosaic preview in the transcript
  (`attached image: photo.png`) through the normal artifact-fetch
  lane, live AND on session restore (`input_data.context.attachments`
  rehydration). Non-image kinds keep the `📎` line only.
- **`/status`** (visibility review P2-5): the status card —
  `status_card_rows` (built "for a future /status", now claimed) +
  client phase/run id/last outcome + a LIVE `get_run` server-truth
  probe fired at the gesture and rendered when it lands. The one place
  the documented client-view vs server-status divergence (wrapper
  roots parked `waiting` on pollers) is inspectable on demand.
- **Cycle intent on the strip** (visibility review P2-1): "thinking
  (cycle 30)" now carries the model's own newest words — `thinking
  (cycle 30) — "fixing the end_line computation…"` (48-char gist;
  lifetime rides the activity label, so tool transitions hide it for
  free).
- **Conclusion bell**: NOT buildable app-side — the engine detects the
  OSC 9/99 notify channels but exports no emitter, and the damage
  contract gives the presenter byte custody. Filed as abstracttui
  first-app/0290 (`app::notify()`); the in-app halves (done marker +
  `last run:` segment) shipped in the visibility wave.

### Changed (reactive pickers — the c5483 claim receipt, 2026-07-25)

- **Picker rows are LIVE**: `/workflow` and `/model` (provider stage)
  rebuild their rows from the store signals while OPEN — a catalog
  refresh landing mid-open renders in place, retiring the static-shell
  limit the 13-row parity incident named (code's on_open live-rendering
  footer hook was the reference pattern). Choose-side contract:
  activation RE-READS the source signal, so an entry that appeared
  mid-open is selectable and an index into rebuilt rows can never
  desync against an open-time snapshot. Row recipes are shared helpers
  (`workflow_row`/`provider_rows`) — one recipe for the open-time
  snapshot and the live rebuild, no drift. Static-source pickers
  (theme, sessions, model stage 2) deliberately stay snapshots. The
  catalog-change notice now says "/workflow lists the fresh set"
  instead of teaching a reopen that is no longer needed.

### Added + Fixed (run-state visibility wave — "did it finish? what is it doing?", 2026-07-25)

The operator's mid-run complaint ("no visual cues or understanding of
where we are — did it finish? what is it doing?") went to a fable5
adversary (`untracked/reviews/run-visibility-review.md`; verdict:
justified — 1 P0, 4 P1, 8 P2). Applied, all S-cost:

- **Terminal done marker (P1-1)**: every conclusion now pushes a loud
  transcript line — `✓ done · 9m14s · 31 llm calls · 38 tools (5 ✗) ·
  1.2M↑ 18k↓ tk` (✗ failed / ⊘ cancelled) — exec/TUI parity where the
  TUI previously rendered NOTHING (the wall just stopped; the pty
  harness's only turn-boundary signal was the composer placeholder, by
  elimination). Exactly-once via the existing `finished` guards;
  replayed history keeps ledger-true facts but omits elapsed
  (fold-time instants are not durations). The idle strip leads with
  `last run: done · …`, so the answer to "did it finish?" lives in
  fixed chrome too.
- **The stale Starting clock lie (P0)**: boot-attach to a parked
  wrapper root or a mid-run session switch left an hours-old
  `run_started` behind — the next submit rendered "starting run ·
  9h20m" for a one-second-old task. Submit now anchors the clock
  (Starting ticks honestly from 0); session resets clear it.
- **Scrolled-up honesty (P1-2)**: the strip now says `scrolled up ·
  Esc returns to live` while the run appends below a reading user, and
  the idle line says `scrolled up — Esc jumps to the newest` when the
  conclusion landed off-screen — the `follow` signal was written five
  times and rendered nowhere.
- **Esc jump never cancels (P2-8)**: the Esc that re-arms follow is
  CONSUMED — double-tapping Esc from scrollback to "get back down"
  used to arm-and-fire run cancel with a 900ms-old toast as the only
  warning. Help + api.md teach the gesture.
- **Starting/upload dead window (P1-3)**: attachment uploads name
  themselves on the strip (`uploading report.pdf (1.2 MB)…`) instead
  of a frozen spinner; the anchored clock covers the rest.
- **Tool-failure visibility (P2-2)**: `Stats.tool_failures` counts ✗
  results; the strip renders `38 tools (5 ✗)` so a failure streak is
  visible without reading the card wall.
- **Honesty polish**: approval/pause strip lines carry the run clock
  (`run 2h` — a forgotten approval visibly ages, P2-4); the ≥60s
  model-call hint says `gateway not responding` instead of blaming the
  provider when the connection is the known problem (P2-3); the boot
  rehydration window says `restoring session history…` instead of the
  "no runs yet" lie (P2-7); `session: 1 run` grammar (P2-9).
- **Splash notice clamp (found by this wave's own test)**: the splash
  renders notices unclamped (no Scroll on that branch) — a notice
  flood overflowed the column and flex-crushed the activity strip to
  zero height. Newest 8 render with an `(+N earlier notices)` line.
- Deferred (M-cost, recorded): newest-thinking preview on the strip
  (P2-1), `/status` with server-truth run status (P2-5), optional
  conclusion bell (P2-6).

### Fixed (attachments verify pass — NEW-1 P0, 2026-07-25)

- **The worker never reads signals (verify-pass NEW-1, probe-confirmed
  P0)**: the fix pass's stat-before-read belt read
  `store.max_attachment_bytes` INSIDE the worker's upload loop —
  signals are UI-thread-stamped, so the first real attachment send
  panicked the runner ("gateway worker is dead") before any upload;
  invisible to the suite because no test drives `Runner::start_run`
  on the spawned thread. The cap now rides `Cmd::Start` as a
  UI-thread snapshot (`attachment_cap`), pinned by the custody test.
  The workspace's own recurring class ("the single-writer rule must
  follow the feature into every lane") — this time introduced BY a
  review-driven fix; verify passes exist for exactly this.
- **Live pty smoke** (`scripts/pty_attach_smoke.py`, 7/7 PASS): boots,
  stages a chip via the `/tmp` symlink spelling, sends — the worker
  uploads on its own thread (no dead-worker banner), the run starts,
  the `📎` record lands, the model answers the planted codeword from
  the attached content, and the turn concludes to idle. Also verified
  live: the extension-guessed content type serves `modality: text`
  (the first cut's octet-stream demoted text files to modality
  "file"). Note: the first smoke run failed one check on an
  ENVIRONMENT error (LMStudio refused to load the harness's default
  model) — rerun on the gateway default route passed clean; the run
  tree's failed agent subrun named the model-load error verbatim.

### Fixed (attachments fix pass — fable5 implementation review, 2026-07-25)

The implementation adversary's verdict was SHIP-WITH-FIXES
(`untracked/reviews/attachments-impl-review.md`; two defects
probe-confirmed). All five P1s + nine P2s applied:

- **Undo keys on canonical paths (P1-1, probe-confirmed)**: the undo
  slot stored the pasted SPELLING while chips store canonical paths —
  on macOS `/tmp`→`/private/tmp` symlinks the Ctrl+O retain never
  matched: the chip stayed AND the path text was restored (both halves
  of the notice were lies). `attach_path` now returns the canonical
  path it staged and the drop hook records that. Regression test drops
  through the `/tmp` symlink spelling.
- **The undo slot dies with the send (P1-2, probe-confirmed)**:
  nothing cleared `paste_undo` when the chips rode a run — a later
  Ctrl+O removed nothing, injected stale path text, and claimed "drop
  undone". Cleared in `clear_sent_attachments`, `/attach clear`,
  manager removals, and guarded in `undo_drop` itself (agent-focus
  gate + removed-count check — zero removals means nothing to undo).
- **Upload-failure closure is session-guarded (P1-3)**: the worker's
  failure post ran unconditionally while both terminal siblings guard
  on the started session — a `/new` + fresh submit during a ~30s/file
  upload window would eat the OLD failure's error card and have its
  Starting phase flipped (double-start window). Now the
  `apply_start_failure` shape: mismatch → notice, never a card/phase
  write. The SUCCESS closure equally gates the `📎`/clear half (P2-1:
  a mismatch-cancelled start no longer 📎-lines the new session).
- **413 detail verbatim (P1-4)**: the upload error card used
  `compact_reason()` ("HTTP 413"), discarding the gateway's
  `Attachment too large (N bytes > M bytes)` detail all three docs
  promise — exactly on the cap-unknown path where the server is the
  only authority. Display form restored (the card is user-facing only;
  `Item::Error` never folds into `chat_messages`).
- **The custody lane is tested (P1-5)**: four new tests — the
  symlinked-prefix undo regression, Ctrl+O-after-send dead-key,
  `merge_cached_refs` sibling-caching/removed-chip/foreign-session
  predicates, and removal-kills-the-undo-slot.
- P2 sweep: `📎` line rehydrates from `input_data.context.attachments`
  on session restore (P2-2, test-pinned); `/help` drop wording matches
  the shipped attach-directly behavior (P2-3); undo inserts a
  separator before restored text instead of gluing it to prose (P2-4);
  drops emit ONE consolidated notice (was N+1 toasts; refusals still
  notify per file with reasons) (P2-5); a 512 MB client SAFETY CEILING
  (labeled as such, never presented as a server rule) guards the
  cap-unknown path against gigabyte transient allocations, with a
  stat-before-read belt at send time for files grown since attach,
  both lanes (P2-6); exec `--attach` uploads run INSIDE the
  `--timeout` budget (P2-7); mid-flight chip-removal semantics
  documented (send snapshots the staged set) (P2-8); `.svg` uploads as
  `application/xml` so it inlines readably instead of riding the
  raster VLM media path (P2-9); the multipart `filename` FIELD strips
  control bytes too — a newline in one's own filename would have been
  prompt injection via the "Stored session attachments" system message
  (P2-10); the Ctrl+O key row names its expiry (P2-11).

### Added (file attachments — /attach, drag & drop, exec --attach, 2026-07-25)

- **`/attach` + chips + send-time upload**: files stage as PENDING
  chips above the composer (validated at attach: exists, regular file,
  under the gateway's `max_attachment_bytes` when declared — unknown
  cap defers to the server 413) and upload at SEND on the worker
  thread, riding runs as `context.attachments` (whole upload refs —
  the agent lane's preferred media key, the abstractflow-proven shape).
  Send-time upload is the only shape that survives `/new` session
  rotation and makes chip removal a true no-op (session uploads are
  permanent server-side; the gateway attachment index has no delete).
  Design: `untracked/reviews/attachments-design.md` (fable5 adversary).
- **Custody rules (fixing the assistant's precedent defect)**: chips
  clear only when the run STARTS (a `📎` Info line records what rode
  the turn — `Info` never folds into `chat_messages`, so it cannot
  leak into client-carried context). Upload failure blocks the send,
  keeps every chip, error-cards the server detail verbatim; refs
  minted before the failure cache back into their chips (merged BY
  PATH — mid-flight chip edits survive) so a retry reuses them and
  never mints duplicate artifacts. Vanished files name themselves.
  Session boundaries discard chips with a notice; steers, `/queue`
  drains, and `/goal` runs never carry them (each lane says so);
  entity lanes refuse (v1 — visit turns have no attachment surface).
- **Drag & drop (engine seam, abstracttui 0.2.19+ attachment wave)**:
  a file dropped onto the terminal arrives as a bracketed paste of its
  path; the composer's `TextArea::on_paste` hook classifies it with
  the ENGINE's cross-terminal spelling corpus
  (`input::paste::classify` — pure string half) and this client
  resolves existence/kind (the ruled split). A verified drop attaches
  directly (`PasteAction::Consume` — nothing lands in the draft) with
  a notice naming **Ctrl+O** as the undo (chips out, raw path text
  back). Ambiguity inserts as text byte-identical (the classifier's
  asymmetry policy: prose, nonexistent paths, raw-space multi-drops);
  folders refuse with "drop files, not folders". NOTE: the design doc
  ruled insert-and-offer before the engine's `Consume` existed; with
  the strict classifier + existence gate, attach-with-undo serves the
  dominant real-drop case better and stays one keypress from intent in
  the pasted-path-as-prose case — deviation recorded here deliberately.
- **`/attach` browser + manager**: bare `/attach` opens the engine
  `FilePicker` (breadcrumb, type-to-filter, Space multi-mark, Enter
  descends/commits) rooted at the workspace; with chips staged it
  opens the pending manager (`x` remove, `c` clear, `b` browse).
  Every picker/typed/dropped path funnels through ONE `attach_path`
  validation. Typed args accept `~`, quotes, escaped spaces,
  `file://`, and relative paths (explicit intent) — the drop detector
  accepts only absolute/`~`/`file://` shapes.
- **Kind honesty at attach**: text-like files inline server-side
  (120 KB/item; a >200 KB staged total warns once), PDFs extract on
  `open_attachment`, images need a VLM route, other binaries are
  listable-not-readable — the attach notice names the caveat. The
  upload now sends an extension-guessed `content_type` (the server
  derives artifact MODALITY from it; the hardcoded octet-stream of the
  first cut demoted .txt to modality "file").
- **Headless parity**: `exec --attach <path>` (repeatable) uploads
  before `runs/start` and exits 1 on any failure — nothing spent;
  each upload prints `attached <name> (<size>) as <id8>…`.
- **Gateway client**: `upload_attachment` (hand-rolled multipart —
  ureq 2.x has none; golden-encode tested, header filename sanitized
  against CR/LF/quote injection) parsing the ref from `artifact` first,
  `attachment` fallback; `max_attachment_bytes` seeded from
  `/workspace/policy` at probe.
- **Live proof**: `exec --attach "secret brief.txt"` (space in the
  name) against the live gateway — the model answered both planted
  facts with ZERO tools (content inlined into its context);
  `GET /runs/{id}/input_data` showed the whole ref under
  `context.attachments[0]` with the session-memory run id + sha256.
- **Engine dependency**: abstracttui `0.2.20` (crates.io) — the
  attachment wave (`on_paste`/`PasteAction`, `input::paste::classify`,
  `FilePicker`) published same-day on our consumption report; the
  build's TEMPORARY path dep lasted hours and is gone (full gate
  re-run green on the registry pin).

The abstracttui 0.2.6 adoption wave: five engine releases landed our
filings (0290/0293/0295/0296 in 0.2.2, 0297 in 0.2.3, 0299 + 0291 in
0.2.6), so this wave DELETES the app-side machinery the engine now owns
and takes the new surface our Card system motivated (0102 rich lines).

### Changed

- **abstracttui `0.2.8` → `0.2.9`** (crates.io; additive, MSRV stays
  1.87 — the app compiled unchanged). The engine landed both filings
  from the 0.2.8 assessment: **0288** — the shifted-letter wire fold
  now runs at EVERY chord-match site (verified at source in the
  published crate: `tree.rs` normalizes the event chord AND each
  registered chord, `KeyEvent::means_char` covers ChoicePrompt
  letters, `Actions`/`KeyState::pressed_chord` likewise), so ONE
  registration matches both wire spellings of Shift+letter — the
  approval modal's 0286 double registration (`plain(Char('A'))` +
  `SHIFT+Char('a')`) is DELETED; the single canonical
  `plain(Char('A'))` remains, still pinned end-to-end by
  `approve_all_fires_on_the_kitty_shift_a_spelling_and_covers_the_next_batch`
  (raw `CSI 97;2u` bytes through the real parser). **0287** — 
  `ChoicePrompt::body(|cx| view)` + `body_rows(n)` shipped (a real
  reactive, Scroll-wrappable display region; options allocated
  first). ChoicePrompt was therefore RE-ASSESSED for the approval
  modal and is still NOT adopted — the original blockers are gone,
  three engine API holes remain (filed as first-app/0271): (1) the
  gate's panel width is content-derived with no caller knob and the
  body is invisible to `measure` — our options land it at ≈45 cols
  while the approval cards are built for 72 (the 2026-07-22
  readability fix would clip); (2) no non-option key vocabulary and a
  hardcoded hint row — the `f` cards↔JSON toggle could neither fire
  (unmatched letters die unconsumed inside the focus trap) nor be
  advertised; (3) Esc-defer survives outcome-wise
  (`ChoiceOutcome::Cancelled` is distinct from the Deny option and
  wirable to `dismissed_wait`) but the rendered vocabulary lies — a
  forced "Cancel" button + "Esc cancels" hint beside a real Deny on a
  consent surface — and `ChoicePromptHandle` cannot distinguish
  host-retire (UiCtx's replace/auto-close paths, which must reopen
  later) from user-dismiss (which must stay closed). Checks that DID
  pass at source: `option_key` a/A/d with the kitty fold, exactly-once
  `on_resolve` with close-before-callback, body dyn_view reactivity.
- **abstracttui `0.2.6` → `0.2.8`** (crates.io): the engine fixed our
  first-app/0285 — the screen-text selection layer now claims a mouse
  gesture only once it DRAGS, so plain clicks pass through to buttons
  with select mode on (verified in the published crate before
  adoption: Down/Up pass, first drag emits `SelectionAct::Claim`, and
  the driver cancels the routed pointer press so a drag that started
  on a button un-presses without firing). The app-side workaround is
  DELETED: `open_modal` no longer disables select mode and
  `close_modal` no longer re-enables it — the boot enable in `lib.rs`
  is the single writer, drag-copy inside modals works again, and the
  regression test is rewritten to pin the new truth
  (`approval_buttons_are_clickable_with_select_mode_on`: select mode
  STAYS enabled while the approval modal is up and a real SGR click
  still fires the approve button through the enabled layer). 0.2.8's
  new `ChoicePrompt` decision gate was assessed for the approval modal
  and NOT adopted: its prompt body is a truncating plain string (no
  scrollable per-call cards, no `f` JSON toggle, no reactive tier
  line) and its `option_key` uppercase shortcuts are dead on
  kitty-protocol terminals — filed engine-side as first-app/0287
  (body View slot) and 0288 (kitty spelling at the non-chord match
  site).
- **abstracttui `0.2.1` → `0.2.6`** (crates.io; no breaking API changes,
  MSRV stays 1.87). One behavior change taken deliberately: Feed
  markdown items typeset the full doc vocabulary — GFM tables, task
  lists, `~~strikethrough~~` — so agent answers carrying pipe tables
  render as TABLES instead of raw `| a | b |` text (pinned:
  `assistant_answer_tables_typeset_instead_of_raw_pipes`).
- **Card headers → engine rich lines** (0.2.3 `FeedItem::rich_lines`,
  the span model our 0102 filing motivated): every transcript header
  row (user/steer/thinking/tool/assistant/image/error/probe) is now a
  multi-ink `RichLine` typeset through the engine's span-preserving
  wrap — ~70 lines of custom header drawing deleted. Honest deltas: a
  long tool args-preview now WRAPS at draw width instead of ellipsizing
  (bounded by the upstream 200-char cap; the truncate knob is filed as
  engine first-app/0283), and body-carrying cards gained one interior
  blank row (the engine's block rhythm — uniform with the assistant
  card's long-standing markdown shape). Capped bodies stay in a
  slimmed body-only custom block (`CappedBody`): the width-aware row
  cap + "… (+K more lines)" marker + hang indent have no engine
  equivalent yet (filed as first-app/0283).
- **Stream retry backoff is now jittered-exponential**
  (`reactive::Backoff`, full jitter, 500 ms base ×2 capped at 30 s) —
  replacing the linear `(500 × errors).min(5000)` ms hand-roll the
  engine's `reactive::connection` module doc names as the
  thundering-herd failure mode: on a gateway restart, N per-run stream
  threads all retried in lockstep. Reset on every successful read —
  every parsed SSE step event (bytes parsed prove the gateway alive),
  a clean idle close, and each successful REST fallback poll — so
  grown attempts never survive a long healthy stream into its next
  hiccup. Two deliberate behavior
  deltas beyond jitter, both directions of the trade: a fully dead
  gateway's retry gaps grow toward 30 s (was capped at 5 s — recovery
  after a long outage can wait one draw, ≤30 s, mean ~15 s at full
  growth), and a broken-SSE/healthy-REST gateway is polled HOTTER
  (post-reset draws in [0, 500 ms] — records at near-live latency; a
  liveness-over-load choice). `exec`'s three fixed 800 ms gateway-error
  sleeps ride the same `Backoff` now — fleet `exec` (the swarm
  bridges) is the real multi-process herd, and the per-instance
  entropy seed decorrelates processes too; every `exec` draw is
  clamped to the remaining `--timeout` budget, so backoff growth can
  never stretch the exit-124 deadline.
- **Capability honesty (0.2.2 `use_caps`/`current_caps`, our 0295)**:
  the mosaic image block reads the LIVE probe-upgraded capabilities at
  draw time instead of fabricating `unicode_ok`/`truecolor`; the
  composer placeholder teaches the BEST newline chord per terminal
  (Shift+Enter where the kitty keyboard protocol is live — flips
  mid-session at the probe upgrade — else the universal Ctrl+J); the
  `/help` Shift+Enter claim ("needs kitty/Ghostty") was STALE since
  0.2.2 and now names the probe-upgraded reality (iTerm2 ≥ 3.5,
  VS Code/Cursor, Warp included).

### Removed (engine owns the job now)

- **Ctrl+J composer machinery** (0.2.2 folded Ctrl+J into TextArea
  under every submit policy): `insert_newline_at_caret`, the
  composer-element shortcut, and its unit test — the behavior test
  passes through the engine path.
- **Selection-clear `on_change` hack** (0.2.2, our 0290: EVERY copy now
  ends the drag gesture and clears the region): typing after a copy
  routes normally without app help.
- **Modal retire one-tick disposal deferral** (0.2.3, our 0297: the
  disposal-safety law is engine-wide): `retire()` collapses to
  `m.close()` — layer + scope go together, synchronously safe from
  widget callbacks. `open_modal`'s atomic-replace ordering stays (that
  contract is about reactive observers mid-flush, not disposal).
- **The veil/heal machinery** (0.2.6, our 0299 shipped): Ctrl+L +
  `/redraw` now call the engine's `request_full_redraw()` (real
  poison-prev + presenter-invalidate — the first heal frame re-anchors,
  protocol images re-place, the transcript pane heals too), and
  `set_redraw_on_focus_gained(true)` at boot auto-heals an externally
  cleared screen at the next focus round-trip. Deleted: the
  translucent-veil `force_redraw`/`veil_and_vacate`, the ~5s
  `heal_chrome_rows` chrome-band heartbeat (and its chrome-only scope
  limits), and the period-injectable ticker seam that existed to test
  it. `scripts/pty_redraw_heal_verify.py` is marked SUPERSEDED.
- **Focused-placeholder overlay** (0.2.6, our 0291:
  `placeholder_while_focused(true)`): the engine paints the hint beside
  the caret while the composer is focused-and-empty — the ~50-line
  absolute overlay dies; exactly one renderer paints in each state.

### Added (full-catalog surfacing — tool-tiers item H client half, c4555 commitment)

- **Served-disabled tools are visible, never grantable**: the gateway
  now serves the FULL tool catalog — gate-disabled rows arrive
  `enabled: false` + `enable_gate` + `why_disabled` (absent `enabled` =
  enabled). This client consumes the field end to end: the `/tools`
  modal renders disabled rows dim with their gate (`send_email
  [disabled on this gateway — gate: …]`), counts them separately
  (`N available · K gated off server-side`), and refuses toggles/pins
  with a notice naming the gate; run allowlists exclude them; and the
  approval belt CLAMPS them to ask — never auto, not at tier `all`,
  not through a served `approval: auto` fact, not through a stale
  persisted `auto` pin (the F3 clamp, client side: what a disabled row
  would do if granted is a question that should never be answerable).
  Applies to the TUI and `exec` policy expansion alike. Cycle-2
  adversary hardening: the run-start "customized?" predicate and the
  /tools title now share ONE effective-disabled rule (a stale
  user-disable on a row the gateway later gate-disabled no longer
  flips the run into explicit-allowlist mode while the title says
  "untouched" — that divergence could silently WIDEN the agent's tool
  set past the workflow's baked pin), and the approval card names a
  served-disabled call's state + gate (`⚠ disabled on this gateway —
  gate: … (approval cannot run it)`) instead of rendering a tier line
  that implies approvability the gateway will refuse.
- **The per-tool approval dial's live spelling is `approval_default`**
  (found by the post-bounce wire pass, 2026-07-23): the parser read
  only the older `approval` key, so the gateway's served truth was
  silently unread and the name-table `#FALLBACK` classified everything.
  `tools_from_discovery` now reads `approval_default` first (legacy
  `approval` kept as the fallback spelling) — live-verified against the
  bounced gateway: 50/50 rows read (12 auto / 38 ask, disabled rows
  clamped to ask server-side), and the server-truth-preferring
  classification path finally engages.

### Added (splash logo + centered idle screen — IDLE-2, operator ask)

- **The boot/idle screen is now a splash**: a two-row half-block
  `ABSTRACT CODE` logotype under a breathing two-row `▲` mark, the
  version as a faint tagline completing the lockup (moved out of the
  fact card — brand metadata, not an operational fact; the row stays
  in `status_card_rows` for a future `/status`), with a slow theme-ink
  shimmer sweeping the letters (~11s per pass, exactly two mark
  breaths per sweep — harmonized 2:1 with an exact-wrap cosine, so
  there is no f32 precision horizon on week-long idles). The whole
  identity block (lockup · guidance · fact card · notices · hints) is
  vertically CENTERED — reversing the earlier top-anchor decision on
  evidence: the anti-ghosting rationale predates the 0.2.6 engine's
  hardened damage contract and the focus-gained/Ctrl+L full-redraw
  heals, and the block re-seats only when a rare boot notice arrives.
- **Theme-derived with a SEPARATION FLOOR** (refinement adversary): on
  ~7 themes the raw `muted → accent` lerp was near-invisible
  (catppuccin family ~1.2:1, one-dark 1.16:1); the highlights now walk
  the accent toward `text`, then toward the theme's own luminance pole
  when `text` itself cannot separate — audited across all themes in
  `theme_contrast_audit` (floors sit just under the weakest theme's
  measured ceiling: monokai's near-white muted tops out at 1.30:1).
- **Honest degradation on BOTH axes** (refinement adversary P1 — the
  engine's 0240 flex-shrink class relearned: default-shrink rows on a
  short pane overprinted card text, "sessionce…", at 72×20): every
  content row is `shrink(0.0)` and the column clips, so short panes
  top-align and drop bottom rows WHOLE — notices render ABOVE the
  static hints row so a session echo ("buffered guidance dropped")
  outlives rediscoverable help text, and the logo is the last casualty,
  never the first. Panes narrower than the logotype render the one-row
  `▲ AbstractCode` brand line, vertically centered in its box.
- **Zero-wakeup discipline kept**: the animation ticker exists ONLY
  while the splash is visible (one hoisted predicate shared by the
  render branch and the ticker), resets to frame 0 on every entrance
  (deterministic fade-up), halves its cadence to 300ms on a dead
  gateway (a screen that may sit for hours), and dies with the first
  conversation item — pinned by the byte channel (bounded-poll
  emissions while visible, provable silence after), plus a short-pane
  no-overprint pin at 72×20. Byte-idle-asserting tests leave the
  splash first (`Harness::leave_splash` — a live shimmer tick races
  idle asserts and could masquerade as an emission under test).

### Filed engine-side (abstracttui first-app backlog)

- 0281 — Scroll never re-clamps a bound offset on content shrink (our
  hand-rolled shrink-clamp effect is the workaround to delete).
- 0282 — `FeedState::sync` source shape too narrow: a borrow-based
  `sync_with(cx, read_fn, spec)` ask, with this app's `Signal<Fold>` /
  focus-selected sources as the evidence (`wire_feed` stays until
  then — adopting today would mean a store restructure or a
  clone-mirror per fold write).
- 0283 — Capped preview blocks: width-aware `max_rows` + honest
  overflow marker on Text/Rich feed blocks (+ hang-indent and
  tight-rhythm notes) — the one feature keeping `CappedBody` alive.

### Deliberately not adopted (verified against this codebase)

- `FeedState::sync` as shipped (source-shape mismatch — see 0282),
  `Driver::suspend` (unreachable through `App::run`), `TimeSeries`/time
  axes (our `output_series` is per-call indexed), key-state/PushToTalk/
  Meter/AudioScope (no held-key gestures or audio here), markdown
  in-flow images (path-based lazy decode; our images are HTTP-fetched
  in-memory bitmaps — 0280 remains the real ask), `SelectHandle::open`
  faces (our List-in-Modal pickers keep live-preview-on-movement +
  Esc-revert + two-stage flows the Select family lacks), and the
  `reactive::connection` lifecycle for per-run streams (must be
  constructed on the UI thread; our streams are worker-side —
  `Backoff` alone is the drop-in half; the `ConnState` status-bar orb
  is a possible second pass).

### Added (tools modal: per-category toggle, per-session sticky prefs, camera off by default — 2026-07-23)

- **Per-category toggle** (`c` in `/tools`): flips every grantable tool in
  the cursor's toolset on/off in one keystroke (toggle by current state),
  leaving other categories untouched; a category that is entirely
  gate-disabled reports the gate instead of no-op. Pure client logic over
  `disabled_tools` — no engine capability needed. Pinned by
  `tools_category_toggle_flips_a_whole_toolset`.
- **Tools-modal prefs are STICKY PER SESSION** (operator ask): the
  disabled set, per-tool approval pins, and accepted tier now persist
  keyed by session id (`session_tool_prefs`, mirroring the existing
  `session_queues`/`session_goals` slot pattern) — each session remembers
  its own tool activation and loads it on switch/resume. A brand-new
  session seeds from the global baseline (backward-compatible: the
  top-level `disabled_tools`/`tool_*` fields still track the latest setup
  for legacy/headless readers). Pinned by
  `prefs_round_trip_with_capability_fields`.
- **Camera tools OFF by default** (operator ask, privacy): a fresh
  session seeds every `camera` toolset tool into its disabled set once the
  inventory loads (one-shot per session — enabling camera afterward
  sticks). This is the CLIENT half; the gateway/workflow-default half
  (camera served default-off) is flagged to the gateway/camera seat.
  Pinned by `camera_tools_seed_off_for_a_fresh_session`.

### Fixed (unrecognized tools never ride the /auto blanket, 2026-07-23)

- **The `/auto` blanket now excludes tools the client cannot identify**:
  forensics on the agent-quality incident found the session blanket
  auto-approving a workflow-authored, NONEXISTENT tool (`browser_probe`)
  at machine speed — a session "approve everything" was never consent to
  run a tool with zero classification source (absent from both the
  gateway inventory and the client's name tables). Recognized top-tier
  tools (`execute_command`, shell, `fetch_url`, anything the gateway
  served) still ride the blanket; an unrecognized name surfaces the
  prompt with a notice naming why. Tier auto-approve is unchanged (an
  unrecognized name classifies to `All` and only the top accepted tier
  clears it — a deliberate choice, never a blind blanket). Pinned by
  `unrecognized_tool_never_rides_the_auto_blanket`.

### Fixed (ctx meter honesty under zero-poisoned usage, 2026-07-23)

- **The ctx chip derives an estimate instead of freezing stale**: relays
  in the gpt-5.6-sol class report `input_tokens: 0` with a real total;
  the meter's refusal to update on zeros silently kept the PREVIOUS
  call's number on screen — the live incident froze "ctx 4.0k" while
  the wire carried ~137k tokens, and the stale reading corroborated a
  wrong root-cause hypothesis for a full investigation cycle. Now a
  zero-split usage with a non-zero total renders `ctx ~N`
  (total − output, labeled as an estimate) — an approximation stated as
  one, never a stale number presented as fresh.

### Fixed (reattach door: backlog-first, session guards, honest failures — 2026-07-23, adversary wave 2)

- **Reattach to a live run replays its backlog chronologically and
  streams from the bundle's cursors** (adversary P1-3): the attach door
  streamed every run from cursor 0 — per-run in follow order (the same
  misorder class the terminal-replay fix killed) — and a conclusion
  inside the root's replayed backlog fired `StopFollows` against
  follower streams still posting their history, measurably dropping
  85–97% of a wrapper turn's detail. Now: one `history_bundle` fetch,
  the backlog folds through the same two-pass chronological core
  (pending waits and inflight clocks SURVIVE — a parked approval
  re-prompts, the tool/model clocks show honest elapsed), and streams
  resume after the bundle. A backlog already carrying the conclusion
  reattaches IDLE with the answer on screen. Bundle-fetch failure falls
  back to the old stream-from-0 behavior (mis-ordered but never
  silent).
- **Session switches can no longer be contaminated by an in-flight
  probe** (adversary P1-2): the probe's history swap and the attach
  binding now carry the same session guard as `apply_start_binding` —
  a `/new` or `/sessions` switch during the probe window drops the
  stale posts instead of receiving the old session's history, totals,
  or Running phase.
- **Restore failures are visible** (adversary P1-4): a failed run-list
  fetch renders an error card ("session history could not be
  restored…"), and a failed per-turn bundle fetch leaves an honest
  "(one prior turn could not be restored — run xxxxxxxx)" marker
  instead of a silent hole.
- **Replay keys on ARRIVAL time** (adversary P2-5): completed records
  sort by `ended_at` (fallback `started_at`) — a 518s tool batch's
  completion replays where it landed live, not at its start position.
- **Catalog declarations survive session resets** (adversary P2-7):
  `reset_session_state` preserves the agent-workflow id set, so a
  switch straight onto a live catalog-id run keeps structural
  answer-source binding instead of degrading to the id-prefix
  fallback.

### Fixed (session resume replays live order, 2026-07-23)

- **Resumed sessions now replay in LIVE chronological order** (operator
  report: "when resuming the session I do not get the same messages as
  before" — the final report sat mid-transcript with the whole tree's
  cycles/tools rendered after it). `rehydrate_run_into` folded whole
  ledgers sequentially in discovery order, so the root's ledger — which
  ENDS with the final answer — folded first and every child's records
  landed after the answer. Now two passes: the BFS discovery walk
  collects every record with a per-ledger carried-forward `started_at`
  key (against a scratch fold, preserving the F1 answer-binding
  invariant), then a stable sort by that key reproduces the live
  interleave — the report is again the last thing on screen.
  Timestamp-less captures degrade to exact discovery order by sort
  stability. Pinned by
  `rehydrate_orders_records_chronologically_across_tree_ledgers`.

### Changed (Ctrl+C semantics, 2026-07-23)

- **Ctrl+C clears the prompt first; two consecutive presses quit**
  (operator ruling): the first press erases the composer draft (if any)
  and arms quit with a notice; a second press within 2s quits. Owned as
  a global ACTION so it shadows the engine's default
  Ctrl+C-instant-quits everywhere — including while a modal is open.
  Mid-selection, the selection layer still owns Ctrl+C (copy), and
  release-copy clears the region so the next press reaches the app.
  `/quit` and Ctrl+Q are unchanged. Pinned by
  `ctrl_c_clears_the_draft_then_two_consecutive_presses_quit`.

### Added (long-tool honesty + layout breathing room, 2026-07-23)

- **In-flight tool clock on the activity strip**: a running tool batch
  now carries `tool call Ns` (shared elapsed humanizer), and at ≥60s
  teaches where the time goes — `— executing gateway-side (large scans
  can take minutes)`. Root cause of the ask: a `search_files` over a
  workspace with ~254k unignored files (`.cg_rounds/` unhidden by
  `include_hidden:true` + `target/` missing from the tool's ignore
  sets) executed for 8m39s gateway-side while the strip said only
  "running search_files" — ledger-proven real execution that read as a
  client hang. Mirrors the model-call clock: per-run map, oldest-batch
  anchor, armed on `started` (back-dated from the record's
  `started_at` so a reattach mid-scan reports honest elapsed), cleared
  on waiting/completed/failed, re-armed by the client at its own
  APPROVED resume (the runtime completes the original step with no
  second started record — without the re-arm, `/auto`-blanket and
  tier-belt lanes would run clockless), rolled back when a resume is
  refused, and dropped at every conclusion boundary including
  conclusion-by-answer (wrapper trees never reach run_terminal) and
  terminal subruns (before the early returns).
- **One breathing row between the transcript and the control panel**
  (operator ask): a fixed spacer above the activity strip;
  `CHROME_ROWS` 4→5 keeps the pane-height estimates honest.

### Fixed (approval modal input — two live P0s, 2026-07-23)

- **"Approve all" was a dead key on kitty-protocol terminals** (the
  maintainer's "why does it keep asking when I selected approve all"): a
  shifted letter has TWO wire spellings — legacy bakes the shift into
  the char (`Char('A')`, no mods), the kitty keyboard protocol reports
  the base identity plus the modifier (`Char('a')` + SHIFT) — and the
  chord was registered for the legacy spelling only, so on
  kitty/Ghostty/iTerm2-class terminals Shift+A routed nowhere, the
  session blanket never set, and every later batch prompted again. Both
  spellings now registered on the approval modal (engine normalization
  ask filed as abstracttui first-app/0286). Pinned end-to-end by
  `approve_all_fires_on_the_kitty_shift_a_spelling_and_covers_the_next_batch`
  (raw `CSI 97;2u` bytes → blanket set → next batch auto-resumes
  promptless).
- **Approve / approve all / deny buttons were unclickable** (every
  Button in the app, in fact): the engine's screen-text selection layer
  owns every left Down/Up AHEAD of overlay routing while select mode is
  on — even a drag-less click — and this app arms select mode at boot,
  so no click could ever reach a widget. Select mode now YIELDS while a
  modal is open (`open_modal` disables, `close_modal` re-enables;
  single-writer with boot — drag-copy inside modals trades away, native
  Shift/Option drag still works). The durable fix is engine-side
  click-through, filed as abstracttui first-app/0285; the toggles delete
  when it ships. Pinned by
  `approval_buttons_are_clickable_select_mode_yields_to_modals` (drives
  a real SGR left click on the approve button).

### Added (/export — transcript export for archival + SFT/CPT training, 2026-07-24)

- **`/export [md|jsonl] [--details] [path]`** writes the agent-lane
  transcript to a file. Markdown (default) is a readable archival
  document mirroring the view — header with session/workflow/timestamp/
  item counts and an explicit incompleteness note when the fold has
  truncated older items, `## User`/`## Assistant` turn markers, one-line
  tool activity summaries (full cards for errored tools), images by
  artifact reference. JSONL is SFT-ready: OpenAI chat schema, one line
  per completed turn carrying the cumulative message prefix (every line
  a self-contained training example; the last line = the whole session
  for CPT). Unanswered turns are skipped (pairing semantics parity-pinned
  against `Fold::chat_messages`) and counted in the notice, never written.
  `--details` adds reasoning cycles + full tool cards (markdown) / a
  `details` side field with preview-bounded tool/cycle/steer data (JSONL —
  deliberately not fabricated `tool_calls`; default lines carry only
  `messages` so strict validators accept them). Bare `/export` auto-names
  `abstractcode-export-<sid8>-<stamp>.md` in the cwd; format word wins
  over extension with conflicts refused. Writes are atomic-new only
  (never overwrite, never create parent dirs, `~` refused). New
  `src/export.rs` (pure renderers + one fs helper, spec in the module
  doc); `Fold::truncated()` getter; 17 unit + 2 headless tests.

### Added (/brain — flow-brain entity conversations, the c5190/c5280 proof, 2026-07-25)

- **`/brain <name>`**: converse with an entity through the FLOW BRAIN —
  each message is one summon of the `entity-chat` VisualFlow
  (`entity-life` bundle) through the production entity door
  (`POST /entities/{name}/summon` → poll to terminal → the structured
  `answer`/`degraded`/`moment_error` contract, never success alone).
  Continuity rides the entity's own memory graph under one client-minted
  session id; the view is session-local and `/end` closes it locally
  (there is no server visit). The existing `@name` visit lane is
  byte-untouched.
- Structural honesty inherited from the reference implementation's
  adversary findings: degraded turns render as warn lines from STRUCTURED
  fields (never bracket-parsed prose); one conversation per entity with
  no brain switching under a live thread (the chimera class);
  `fold_reopen` flips the brain WITH the transport; substrate overrides
  omitted (the gateway resolves the home's stored mind); epoch
  inheritance on closed-replace keeps stale in-flight posts dead (the
  end→reopen→send race, test-pinned).
- **Live proof PASSED** (scripts/pty_flowbrain_proof.py, 12/12): taught
  `veya` a unique token in one process; a completely fresh process —
  fresh pty, fresh prefs, fresh session id — asked for it and got
  `saffron-kestrel-42` back from her memory graph. Frames (txt + png) in
  untracked/reports/flowbrain-proof/; the done-rule report is
  docs/reports/flowbrain-conversation-proof.md. Six new headless tests
  drive the lane through the real interface.
- **fable5 adversary folded** (verdict SHIP-WITH-FIXES; all P0/P1
  applied): the stale-post chimera closed three ways (epoch inheritance
  on replace + session id in the guard + auto-send sid read under the
  guard); `/brain` first-word parse + roster check; the summon moved to
  the long-read lane with "outcome unknown" honesty (a start-timeout
  used to say "refused" while the run ran, inviting double-sends); the
  zero-turn `/end` note no longer claims memory persisted; session-id
  entropy; the terminal parse extracted pure (`parse_summon_output`).
  The live proof re-ran green on the fixed build.

### Fixed + Changed (consolidation round 2 — independent /export review + lane extractions, 2026-07-24)

A second fable5 survey gave the fresh `/export` feature its first
INDEPENDENT read (its builder had self-reviewed) and re-measured the
round-1 deferrals. Report: `untracked/reviews/consolidation-2026-07-24-round2.md`.

- **Fixed: Failed tools with an empty error string kept their evidence**
  in default markdown exports — `Failed` is minted from `success: false`
  even when the error string is empty (the failure text rides the result
  preview, e.g. `execute_command` with a non-zero exit), and the
  renderer's error-string test dropped exactly the material an archival
  export exists to keep. "Errored" is now STATUS-based, matching the
  live view's rule; pinned by a new unit test.
- **Fixed: unresolved workflows rendered `- workflow: :`** in the export
  header (the Default workflow's `label()` is `":"`, not `""` — the
  third `label()` consumer needing the guard chrome/transcript_view
  already carry).
- **Fixed: the JSONL truncation notice claimed a header that does not
  exist** — JSONL is schema-pure by design, so the on-screen notice now
  carries the format-specific warning (the earliest turns are missing
  from every line's prefix); module doc + getting-started scoped the
  header claim to markdown.
- The empty-transcript refusal now names the LANE ("the agent transcript
  has no conversation") — under entity focus the user may be looking at
  a rich visit while the agent lane is empty.
- The chat_messages parity pin gained the divergence-prone shapes
  (stacked open users; double final answers in one turn).
- **Extracted `ui/goal.rs` (211 lines) and `ui/queue_lane.rs` (432
  lines)** from `ui/mod.rs` (2,428 → 1,857 lines of genuinely root-level
  concerns) — the round-1 deferral lifted now that the tree is stable;
  visibility-only changes, no logic moved inside functions,
  `queue_preview` re-exported so external consumers are untouched.

### Changed (routine consolidation pass — fable5 survey, 2026-07-24)

One-sitting cleanup from an adversarial consolidation survey (6 P1 + 6 of
8 P2 applied; report at `untracked/reviews/consolidation-2026-07-24.md`).
Zero behavior change; net ~-500 lines from `runner.rs` alone.

- **Post-wave residue deleted**: the orphaned `/auto`-blanket cluster
  (`is_recognized_tool`/`batch_unrecognized_names`/`KNOWN_SYSTEM_TOOLS` —
  zero consumers since the blanket died), the dead `args` parameter chain
  on the four classification functions (values were discarded; kept for a
  "signature stability" no consumer needed), a write-only `answered` vec
  in exec's wait loop, a dead `let _` triple in `finish_tool`, and seven
  stale comments asserting the retired git proof / deleted blanket as
  present-tense fact (two design docs gained dated SUPERSEDED notes).
- **Duplication killed**: ONE `From<&ToolInfo> for ToolClass` projection
  replaces the hand-copied field mapping in `Store::tool_classes` and
  exec's discovery path (both had to be found when `risk_rank` landed);
  boot's inlined mirror of the session-slot seeding logic collapsed onto
  `ui::seed_tool_pref_signals` (the two copies had already drifted on the
  blank-tier rule); the rehydration fold-effect dispatch block
  (`send_fetch_effects`) and the modal gate-suffix formatting
  (`gate_suffix`) each got one authority.
- **`src/discovery.rs`**: the ~420 lines of pure catalog/discovery
  parsers (+ their 11 tests) moved out of `runner.rs` — headless exec
  importing its parsers from the TUI worker module was a wrong-direction
  dependency, and the section was already banner-marked as a separate
  concern. `runner.rs` drops from 3,577 to ~2,880 lines of strictly
  worker concerns.
- **`.cg_rounds/` gitignored** (17 GB of round snapshots one `git add -A`
  away from staging).

### Removed (the client read-only-git proof — retirement rung fired, 2026-07-24)

- **`is_readonly_git` and its attack corpus are DELETED** (~330 lines of
  proof + ~200 lines of corpus tests): core declared
  `risk_refiner=git_read_only@v1` on `execute_command`'s inventory row
  (c5057) and runtime implements the same two-stage proof at the APPROVAL
  POINT — proven read-only git now auto-approves server-side and never
  generates a wait, so the client proof's only remaining effect was
  duplicating a decision at the wrong seat (the prover and the executor
  are one party again — the lane-1 "prover ≠ executor" finding). In this
  client `execute_command` now always classifies `all`: against a
  PRE-refiner gateway a read-only git command prompts once — the honest
  price of not owning the logic. `ClassSource::GitProof` retired with it;
  git commands with no served facts are name-table classified and carry
  the `#FALLBACK` label like any other table decision.

### Changed (/permissions consolidation — the c5028 converged contract, client half, 2026-07-23)

The operator's thin-client ruling operationalized: the client READS served
facts and FORWARDS user decisions; policy logic retires in lockstep with
server capabilities (four-pass fable5-verified position; the seats' halves
shipped same-hour: core serves the posture ladder + launched its
completeness adversary, runtime built the git-read refiner + verified
stamp passthrough + declared the comms carve-out, gateway adopted all four
verification findings).

- **`/permissions [read|write|all]` is THE tool-permission surface** (bare
  reports; sticky per session; `/tools tier` stays a spelling alias; the
  modal's `t` key cycles the same one apply path). The **`/auto` session
  blanket is DELETED** — three of its five unique behaviors were latent
  holes (ask-pin bypass, served-disabled-clamp bypass, empty-batch
  auto-approve); its spellings open the `/permissions` report instead of
  vanishing or silently setting a now-persistent level.
- **"Approve all" (`A`) sets permissions `all`** — one lane, honestly
  labeled at the gesture ("A — sets permissions: all"): the old blanket
  died at session end; the level persists per session and seeds new ones
  (operator-disclosed hazard). Ask pins and gateway-disabled rows still
  gate at `all`, and the `Tier::All` description now says so instead of
  over-claiming "no prompts".
- **Rank-band floor** (the converged contract's transitional-belt rule):
  the client parses served `risk_rank` and classifies
  `max(band, approval)` — the band floors the level (observe=read,
  act=write, outreach+=all) and `approval_default` can only tighten. Kills
  the comms hole (an outreach row served `auto` under the 2026-02-21
  carve-out no longer classifies read) without downgrading server `ask`
  verdicts (the analyze_media shape). Rank-less gateways keep the
  approval-only mapping.
- **`approved_by` stamping (R3)**: every resume payload names its decider —
  human clicks stamp `approved_by: "user"`, policy auto-resumes stamp
  `approved_by: "policy"` + the admitting rule, headless exec stamps
  policy + rule on approvals AND denials. Runtime verified verbatim
  passthrough into the wait result + ledger — auto-clicks are now
  ledger-distinguishable from human decisions.
- **Headless flags**: `--permissions <read|write|all>` (validated at
  parse) + `--require-approval <names>` (repeatable, comma lists; denies
  headlessly even at `all` — a deliberate pin never silently dissolves).
  `--approve-all`/`--auto-approve` are hard parse errors teaching the
  replacement (the semantics changed: the old flag bypassed pins). ONE
  resolved level feeds both the server-side policy expansion and the
  wait-loop resolver.
- **Honest headless reasons**: an empty/nameless batch denies as
  "unreadable batch — fail-closed" and a served-disabled tool denies
  naming ITS GATE (both used to produce the self-contradictory "needs
  tier 'all', accepted 'all'").
- Docs (README, api, getting-started, troubleshooting, faq, llms.txt)
  reworded; the at-rest prefs key `tool_approval.accepted_tier` is
  deliberately UNCHANGED (documented hand-editable for headless).

### Fixed (offloaded-answer loss + the fetch_url 401 incident — Lane B root cause, 2026-07-23)

The ledger-verified chain: a 17-cycle run's flow-end output (445 KB with a
4 KB answer inside) offloaded whole to an artifact; the client's ONE-SHOT
artifact fetch hit a transport reset in a gateway-bounce window; the
failure label — embedding the full artifact URL via error Display — was
stored as ASSISTANT words; the next turn replayed it in
`context.messages`; the operator said "try again to reach the gateway";
the model called `fetch_url` on the URL from "its own" prior message and
the gateway's auth middleware correctly answered 401.

- **Failure text never travels with a URL again**: `GwError::compact_reason()`
  (evidence-worded, URL-free) is the only error text allowed into
  transcript cards; the failure label itself (`offload_failure_label`) is
  neutral — no artifact id, no URL, no "unreachable" over-claim, no retry
  framing — because final-answer cards replay into later turns'
  `context.messages` as assistant words.
- **Bounded jittered retry** (`fetch_answer_with_retry`, shared by the TUI
  runner and headless exec): transient classes (status-less transport,
  408/429/5xx) retry up to 3 attempts; hard 4xx fails fast. The TUI fetch
  moved OFF the runner command loop onto its own thread (a retrying fetch
  on that loop would starve Probe/Start — the GPU-meter rule).
- **Late-truth reconcile**: once a turn concluded on an offloaded answer
  whose card still holds placeholder/failure text, a LATER answer-source
  flow end carrying the response INLINE swaps the card in place (the
  wrapper ROOT completed minutes after the incident's subrun with the
  full answer in `output.response` — the fold's `finished` gate used to
  ignore it). Works identically on live streams and session-resume
  replay. Pinned by `late_root_flow_end_reconciles_a_lost_offloaded_answer`.

### Fixed (false "gateway unreachable" claims — operator report, 2026-07-23)

- **The words**: `GwError` now carries an evidence class (`GwErrorKind`,
  derived from ureq's own `ErrorKind` + the io source kind — structural,
  never message-text matching). `Display` words each class honestly:
  "gateway unreachable" is reserved for connect-level evidence
  (refused/DNS/TLS — nobody at the address); a request that timed out
  says "gateway timed out"; a mid-transfer break says "gateway request
  failed". Previously every status-less error — read timeouts against a
  merely BUSY gateway included — rendered as "gateway unreachable", so
  every toast/error card repeating a slow call's error claimed the
  gateway was gone while `/api/health` answered in ~1ms (the live
  report: the gateway stalls under in-process inference load; each
  60/75s read timeout minted a fresh false claim).
- **The orb**: `Conn::Down` now requires gone-EVIDENCE (one shared
  policy, `runner::marks_gateway_down`): connect-refused/DNS flips
  immediately; status-less soft failures (timeouts, resets) flip only
  after a run of consecutive occurrences (2 idle probes / 3 stream-lane
  failures — REST fallback polls count, so a wedged-but-accepting
  gateway still surfaces in minutes); an HTTP answer of ANY code proves
  reachability and never flips (the `doctor` command's own rule —
  previously a ping answering 500, or an HTTP 5xx on the stream route,
  flipped the orb to "unreachable" on the first blip). Down→Ok flap
  toasts ("gateway is back — refreshing the catalog") and per-flip
  catalog/tools reloads shrink accordingly — the volume the operator
  saw WAS that flapping.
- **The substring trap**: `apply_start_failure` matched
  `msg.contains("unreachable")` — a wording our own `Display` minted for
  every status-less error, and which an HTTP 500 whose gateway-served
  detail contained the word (a proxied "provider endpoint unreachable")
  would also have tripped. The classification bit now travels from the
  typed-error site on the runner thread; message text is never
  consulted.
- **Sticky false Down healed by SSE**: once a stream error flipped
  `Conn::Down`, only a successful REST fallback poll ever set `Ok` back
  mid-run (the idle probe is phase-gated) — a Down that healed via SSE
  reconnect stayed red for the rest of the run. Every proof of life
  (parsed SSE event, clean idle/close, successful poll) now clears the
  evidence counters and the orb, transition-gated so healthy streams
  post nothing.
- **The display sites (HOLE A, closing the wave)**: `Conn::Down` now
  carries the evidence flag (`Down(msg, gone)`), so the two surfaces
  that used to stamp their own words over the classified message are
  honest: the status card words a soft-threshold Down "not responding"
  ("unreachable" is gone-evidence-only), and the idle-splash recovery
  block renders the evidence-worded message VERBATIM instead of
  prefixing "gateway unreachable —" onto a timeout (it would have
  rendered "gateway unreachable — gateway timed out: …"). Recovery
  advice follows the evidence too: only a GONE gateway teaches
  "start one: abstractgateway serve"; a not-responding one says it is
  running but likely busy. Pinned by
  `soft_down_says_not_responding_never_unreachable`.
- Pinned by real-socket classification tests (an actually-refused
  connect vs a bound-but-silent listener; both timeout io kinds;
  HTTP-status errors) plus policy tests
  (`down_policy_requires_gone_evidence_or_persistence`,
  `start_failure_flips_conn_on_classification_not_message_text`). The
  entity turn lane's `is_read_timeout` recovery trigger now keys on the
  structural kind first (text kept as a belt — over-matching lands in
  the safe recovery poll).

## [0.4.0] - 2026-07-23

The conclusion + presence wave: the "run never finishes" P0 class fixed
at its three roots (offloaded answers, failed answer subruns, unreadable
terminal statuses), a cockpit that states facts instead of blanks
(header facts, instrument footer, declared context meter, `/gpu`),
screen recovery for externally-cleared terminals (`Ctrl+L`/`/redraw` +
a chrome self-heal heartbeat), and reliability seams (catalog self-heal,
counted SSE skips). Built by six concurrent agent lanes in two waves,
then hardened by a three-reviewer cycle-2 adversarial pass whose
integration findings close this section.

### Fixed (run conclusion — the "never finishes" P0)

- **Offloaded final answers now conclude the turn** (the maintainer's
  "currently the agent never even finishes work", root-caused live): a
  heavy agent turn's final output (answer + full message history +
  scratchpad) exceeds the runtime ledger offloader's 256 KB inline cap
  and persists as `result.output = {"$artifact": id}` — the read surface
  serves the ref unresolved by design, so the fold saw no answer text,
  `finished` never flipped, and the composer stayed captured with the
  spinner ticking until the operator cancelled (live evidence: run
  `c61e4ac9…`, agent answered at minute 3, cancelled at hour 5; the
  wrapper root can never conclude on its own because its status-poller
  subflow loops `wait_until` forever). The fold now concludes on the
  offloaded flow end immediately (placeholder card naming the artifact),
  and a new `FetchAnswer` effect downloads
  `/runs/{run}/artifacts/{id}/content` and swaps the real words in — in
  the live TUI, exec, and prior-turn rehydration. A failed fetch labels
  the card honestly; the composer is never held hostage by the fetch.
  Live-verified end-to-end against the original evidence run: the real
  answer text renders and the composer frees.
- **Flow ends without a readable output key conclude honestly**: an
  answer-lane completion record (`result.completed == true` — written
  only by the runtime's terminal appenders) whose output carries no
  conventional text key now flips `finished` with an honest "completed
  without a readable final answer" note instead of leaving the turn open
  forever. The resume/job completion wrapper (`output.result`) is also
  read as answer text now.
- **F4 — unreadable terminal status no longer fabricates "completed"**:
  when the root stream ends and the run status cannot be read (gateway
  restarting, token expired mid-run), the client retries briefly and
  then reports the honest `unknown` (error card + Failed outcome) — the
  old path labeled it a success and drained the queue against a dead
  gateway.
- **F9 — stale "model call Nm — provider may be slow" hint**: the
  in-flight LLM marker is now cleared at run boundaries (`begin_run`,
  terminal, prior-turn rehydration), so a turn that died mid-call can no
  longer label an idle session as slow-working. In-flight calls are also
  tracked per run: a parallel lane's fast completion no longer hides a
  slow call still running elsewhere (the strip anchors on the oldest
  live call).

### Added (presence + density lane)

- **Header cockpit facts (HDR-1)**: the blank middle (two-thirds of the
  bar at 120 cols; ~193 blank columns at 271) now carries `⌂ cwd-basename
  · workspace-mode · skills N · mcp N · session-tk` (counts when nonzero,
  tokens at rest) — dim values, painted after entity chips so chips keep
  priority. The session id moved from the faint tier (measured 2.77:1,
  below the 3:1 UI floor) to muted ink.
- **Instrument-row footer (REST-1)**: the status bar is now the
  always-visible facts row — `ctx used/window tk (%, declared) · N tk
  session · gpu N% · skills N · mcp N · ? keys + commands` (each segment
  renders when known; absence is omission, never a fabricated zero). The
  key legend moved behind `?` (a bare `?` + Enter opens the reference;
  `/help` unchanged) — the Python predecessor's footer law: the one
  always-visible surface carries numbers. The phase/focus teachings live
  in the composer placeholder, which is now actually visible (below).
  The idle activity strip on a fresh session shows the session line
  ("no runs yet — Enter sends the first task") instead of a reserved
  blank row. MCP registry loads at boot so the counts exist before the
  first `/mcp`.
- **Operator-declared context window (CTX-0)**: `/context <tokens>`
  (accepts `262144`, `262k`, `1m`; persisted), `/context off`, bare
  `/context` reports; `--max-tokens <N>` declares for one session. The
  footer meter becomes `ctx 41k/262k tk (15%, declared)` — warn ink
  ≥75%, error ≥90%; declared-but-unmeasured renders `ctx —/262k tk
  (declared)`; undeclared keeps the honest absolute `ctx 41k tk`. The
  declaration rides runs as top-level `_limits.max_tokens` (ADR-0008's
  canonical total-window key). Always source-labeled "declared" — never
  a client-shipped capability table.
- **Boot/idle identity card (IDLE-1)**: the empty state is now a fact
  card (version · workflow · route · cwd · workspace · session · gateway
  + connection state · skills names · MCP names · context source) with
  the wordmark deduped (it rendered twice: header + empty state). Boot
  notices stay beneath it. Builder (`status_card_rows`) is reusable as
  the future `/status` output. First-frame density: 9/36 → ~18/36 rows.
- **`Ctrl+L` + `/redraw` force-repaint (HDR-2a)**: recovers from
  external screen clears (Cmd+K / `printf '\033c'`) — the damage tracker
  repaints only cells it believes changed, so a wiped terminal stayed
  blank FOREVER (the maintainer's blank-header screenshot, reproduced
  byte-for-byte in review-current-state §2). Mechanism until the engine
  ships a public poison-prev verb (filed as abstracttui
  first-app/0300): a one-tick glyphless translucent VEIL (bg black at
  alpha 2 — integer blending shifts every visible color channel by ≥1,
  so both the veil frame and the restore frame re-emit every cell
  carrying visible ink, while glyphs never blink and the ±1/255 shift
  is imperceptible). Ctrl+L also works while a MODAL is open (modal
  trees swallow unconsumed keys before root shortcuts; a second
  binding on the engine action registry — which runs last, only for
  keys nothing consumed — catches exactly that case, and the pair can
  never double-fire — test-pinned via veil layer count in the cycle-2
  review). Pty-proven live: 36/36 blank rows → full frame back on one
  keystroke, scene byte-identical.
- **Chrome self-heal heartbeat (HDR-2b)**: while a run or entity turn
  is active, the fixed chrome rows (header + strip + composer + status)
  re-emit through the same veil every ~5s — a wiped screen's cockpit
  heals with NO input by the next beat. Live pty proof, real run:
  wipe → t+3s the header is still blank while ~25 header dyn re-runs
  tick (model-side re-renders CANNOT re-emit byte-identical cells — the
  diff's over-approximation contract compares cells inside damage rects
  too, which is why a redraw-epoch signal was rejected as a fix) →
  t+8s the header/strip/composer/status rows are back. Deliberately
  chrome-band-only: a full-frame auto-heal would decay iTerm2/sixel
  image placements (engine beneath-repaint rule) and re-emit whole
  frames twice per beat; the transcript self-heals as content streams,
  and Ctrl+L covers the rest. Idle apps have no heartbeat (the run
  ticker owns it), preserving the engine's zero-wakeup idle guarantee.
- **In-flight model-call ticker (OBS-1a-live)**: the activity strip
  names the running call from second zero — `model call 14s · 41 tok/s
  (last call)` (elapsed from the started record; rate from the previous
  completed call's usage receipt over the client-observed window —
  conservative and provenance-labeled, never a projection). Replaces
  the frozen dead-air "working"; the ≥60s "provider may be slow" hint
  stays.

### Fixed (presence + density lane)

- **Composer placeholder was dead pixels (HDR-2c)**: the engine paints
  TextArea placeholders only when empty AND unfocused, and the composer
  autofocuses — the 0.3.0 phase-aware teaching never rendered once. An
  app-side hint now draws while the composer is empty + focused (offset
  past the caret cell; content-derived height so a non-empty draft's
  caret clicks are never intercepted).
- **Empty-state ghost rows**: the idle card now fills its whole pane
  rect opaquely per regeneration — centered lines painted on
  transparent ground could interleave stale pixels when the block
  shifted by a row (review-current-state §4.5 class; live-caught in
  this wave's pty capture).
- **Chrome instruments drop whole, never fragment (POLISH-1)**: when a
  row is too tight, the status bar drops whole segments right-to-left
  and the header drops whole facts — neither self-truncates into `…`
  fragments any more (the old legend rendered "/help comm…" at 120
  cols and read as broken; SYNTHESIS §2 baseline). Workflow/route/chips
  keep priority over facts; the right clusters (theme·host, session·orb)
  are never sacrificed. One prefix-fit rule (`chrome::prefix_fit`),
  test-pinned at 100 cols. The model-call ticker's elapsed also goes
  through the one shared humanizer now (`model call 9h20m`, never a
  minutes-only spelling past an hour).
- **`--context-window <N>`** accepted as a CLI alias of `--max-tokens`
  (the spelling matches the `/context` command and the prefs key).

### Added (reliability + GPU lane)

- **`/gpu` meter (OBS-6)**: toggles gateway-host GPU polling
  (`GET /host/metrics/gpu`, live-verified shape) on a dedicated poller
  thread — ~3s cadence while a run/entity turn is active, ~30s idle,
  zero requests when off. Data lands in `store.gpu` (`GpuMeter`:
  Off/Pending/Ready/Unsupported/Error) for the status-bar segment.
  Honesty: `supported:false` (or a 404) STOPS polling and says why —
  never a fabricated number; transient errors keep the last honest
  state visible and keep trying. Stale samples from a toggled-off
  poller are generation-gated and can never overwrite `Off`.
- **Catalog self-heal (F1)**: `LoadCatalog` + `LoadTools` re-issue on
  every `Conn::Down → Ok` edge (edge-triggered, never per-probe) — a
  gateway that was down at launch used to leave the app refusing every
  task forever ("no agent workflows") while the orb promised
  reconnection. The heal re-resolves the saved workflow preference and
  never clobbers a selection made while offline.
- **Counted SSE-skip notice (F7)**: a malformed `step` record on the
  ledger stream (undecodable JSON, or an envelope with no record
  object) now surfaces as a counted transcript notice ("N undecodable
  ledger record(s) skipped…") while the good records around it keep
  folding — it used to vanish silently, invisibly and permanently.

### Fixed (reliability + GPU + polish lane)

- **Image memory bounded (F3)**: artifact images downscale AT DECODE on
  the worker thread to the transcript's mosaic ceiling (contain-fit in
  1024×168 px, aspect preserved, never upscaled) — a 4096² PNG retained
  ~67 MB of RGBA forever for a 14-row mosaic. The image entry list is
  also capped (32, oldest-inserted evicted first).
- **Elapsed readability (POLISH-1)**: hours-long runs render `9h20m`
  (and `3m05s`), never a raw `33628s` — one shared formatter
  (`convo::fmt_elapsed`) serves the activity strip, entity chips, and
  goal status.
- **Fuzzy `/` completion + taller dropdown (POLISH-1/UX-14)**: `/wf`
  finds `/workflow` (prefix matches rank first, then subsequence
  matches); the dropdown shows 10 rows instead of 6.
- **Composer prompt glyph (POLISH-1/UX-12)**: an accent `❯` gutter marks
  where you type.
- **Theme picker title (POLISH-1/UX-16)**: no longer self-truncates —
  key hints moved to a hint row; modal widened 44→56.
- **Help modal columns (POLISH-1/UX-17)**: the key gutter now sizes to
  the longest key (20-char keys used to overprint their own
  description) and descriptions stay clear of the scrollbar column.
- **Help modal descriptions wrap instead of truncating**: several help
  entries are longer than the description column at the default width
  (the Ctrl+J newline note is ~197 chars vs a ~69-col column) — the old
  single-row ellipsis silently ate their tails, making the help screen
  the one place whose own help was unreadable. Long descriptions now
  wrap into continuation rows (key beside the first slice only); a unit
  test reassembles every REAL help entry from its rendered slices at
  the real default geometry, so a future longer line cannot regress
  the fit.

### Fixed (cycle-2 integration review, 2026-07-23)

- **SSE parser: a CRLF split across read chunks no longer fabricates an
  event boundary**: a stream chunk ending exactly on the CR of a CRLF
  pair swallowed the CR, and the LF arriving at the head of the next
  chunk parsed as an EMPTY line — the SSE dispatch signal — so one
  `data: …\r\n` line at a read boundary became a premature event plus a
  phantom (on the ledger stream: a skipped record). The parser now
  carries the chunk-final CR and swallows exactly one leading LF on the
  next push; lone-CR terminators at boundaries stay intact
  (regression-pinned, `gateway/sse.rs`).
- **Esc on the ask-user prompt actually defers**: it closed the modal
  without recording the dismissal, and the wait-modal effect reopened
  the same prompt on the very epoch bump the close fired — Esc was a
  no-op blink. It now records the deferral exactly like the approval
  prompt (the run keeps waiting durably; Enter on the empty composer
  reopens). Regression-pinned headlessly, including the
  fails-without-the-fix proof.
- **One session-boundary reset authority**: `/new` and `/sessions` had
  duplicated the nine-write session reset block line-for-line — the
  drift class where one path gains a reset and the other silently
  forgets. Extracted into `reset_session_state` with the
  touched/not-touched contract documented (persisted prefs, gateway
  state, and entity visits deliberately survive; `context_window` is a
  persisted global and never resets).
- **Ctrl+L single-fire pinned**: the root-shortcut + action-registry
  pair's "can never double-fire" routing claim is now a test (a double
  fire would stack a second veil layer at z=2000; the test asserts
  exactly one at z=1000).
- **GPU poller stop-latency comment honesty**: cadence/stop flips apply
  within ~250ms of SLEEPING time only — a thread mid-HTTP-call is
  uninterruptible for up to the 60s read timeout (bounded; its posts
  were always generation-gated). The module doc now says so instead of
  overclaiming.

### Fixed (final-verifier caveats, 2026-07-23)

- Headless `exec` with an explicit `--workflow` that does not exist now
  refuses with exit 2 and prints the real catalog, instead of silently
  running `basic-agent` with exit 0 (automation pinning a specific agent
  never noticed a different one ran). The prefs lane keeps its fallback —
  a stale saved preference degrading to the default remains the
  interactive contract, and the header names what ran.
- Scrubbed the last `agw_` token-prefix remnants (3×, inside a captured
  self-check command) from `tests/fixtures/coder_run_tree.json`; the
  tree-wide credential grep is clean.

### Fixed (cycle-3 hardening pass, 2026-07-23)

- **Splitless tok/s fabrication (cycle-2 P1-A)**: the strip's
  `N tok/s (last call)` numerator read `output_series.last()`, which
  carries the call's TOTAL tokens for splitless receipts (the sparkline
  substitution) — on splitless providers the rate divided prompt+output
  by wall time, overstating throughput. `wire_llm_meter` now uses the
  cumulative-OUTPUT delta across the started→completed transition:
  splitless receipts add nothing there, so they yield honest absence
  (no rate segment); split receipts keep the conservative output/wall
  rate. Regression-pinned headlessly (splitless → no rate; split →
  output-true rate).

### Changed (cycle-3 hardening pass, 2026-07-23)

- **One rate authority (cycle-2 P2-H)**: the fold's consumer-less
  record-truth pair `Fold::live_llm_call()`/`last_call_rate()` (plus
  their `live_call`/`last_rate` slots, writers, and unit tests) is
  removed — chrome deliberately renders the client-clock twins
  (`llm_inflight_since` + `store.last_call_rate`), which are monotonic
  and skew-proof. The record-truth parsers
  (`protocol::started_at_epoch_ms`/`gen_time_ms_from_record`) survive,
  tested, for a future gen_time-truth upgrade.
- **`goal_status` adopts the shared token formatting**: `/goal` status
  carried a third hand-rolled token-fold copy (raw `12000↑ 300↓ tk`,
  and `0 tk` claimed before any receipt) — now `chrome::fmt_tokens` +
  the strip's render-when-known rule (all-zero totals omit the part).
- **Text-helper visibility hygiene**: `transcript::one_line`/`bounded`
  are `pub(crate)` and `convo.rs` drops its byte-identical private
  copies; `value_preview` narrowed `pub` → `pub(crate)` (in-crate
  consumers only; `offload_placeholder` stays `pub` — integration
  tests consume it).

## [0.3.0] - 2026-07-22

The control-and-collaboration wave: prompt queueing, goal loops (client
half), summoned-entity conversations, tiered tool approvals, and
workspace scope control — built and hardened the same day by nine
agent passes (three build lanes × three cycles, each cycle
adversarially reviewed), with a live end-to-end verification of every
feature against a real gateway closing the wave.

### Added (entity collaboration lane, 2026-07-22)

- **Talk with summoned entities beside the agent**: `@name` opens (or
  adopts, via the structured 409 → `GET /visit` path) a durable visit;
  `@name <text>` sends a turn; mid-prompt `@` completion inserts names
  from the cached roster (never a synchronous fetch). Conversations are
  first-class: the transcript pane mirrors the FOCUSED conversation
  (`/focus <name|agent>`, `Ctrl+E` cycles), header chips show every
  conversation's state (focused chip highlighted, running turns carry
  elapsed), and the activity strip narrates the focused visit.
- **Non-interruptible turns, honestly**: entity turns run server-side and
  cannot be cancelled — Enter during a turn HOLDS the draft and auto-sends
  when the turn parks (every park path shares one send authority); Esc
  and agent-run commands under entity focus say what they actually target.
- **`/entities [name]`** — roster modal with identity cards (cached,
  async refresh); **`/task <name> <title>`** leaves a durable task on the
  entity's desk (works while asleep); **`/end [name] [reason]`** closes
  the visit (reflection runs; close restores prior sleep when the visit
  woke the entity). Closed transcripts stay readable in-session; the next
  `@name` opens fresh.
- **One background poller** keeps open conversations honest (parked/
  closed/turn-count drift from other clients folds in); stale results
  from abandoned turns drop at a convo/run/epoch guard — the entity-lane
  twin of the fold's `is_following` rule.

### Fixed (entity lane — cycle-2 adversarial review, 2026-07-22)

- **Held drafts auto-send on EVERY park boundary**: the hold banner
  promised "sends when the turn parks", but only the normal turn-reply
  path honored it — a draft held during Opening (open success AND
  adopt-on-409) or recovered through the read-timeout lane sat in the
  slot forever while the strip kept promising the send. All park paths
  now share one auto-send authority (`take_held_for_send`/`dispatch_held`
  in `gateway/entities.rs`; `fold_recovery_parked` returns the draft).
- **Dropped held drafts surface, never vanish**: a failed/cancelled turn,
  a server-side close (poll or recovery), a transport error, a refused
  open, and `/end` all CLEARED the held draft silently — typed words
  lost. Every drop now renders the text in an Error card naming why
  (`surface_dropped_draft`, the agent lane's pending-steer honesty rule).
  Transport errors deliberately surface instead of auto-sending: the
  predecessor message may never have arrived.
- **Adopt chronology + stale wake claims** (`fold_adopt`): prior items now
  stay ABOVE the adopted transcript (a reopened conversation rendered its
  old visit BELOW the new one); the reopen-spelling "opening a new
  visit…" line is dropped like the fresh one; the "was asleep — this
  visit wakes" note and `woke_for_visit` clear on adopt — a 409 proves
  the entity was already awake in a live visit, so close must not claim
  "prior sleep restored" on a guess.
- **Recovery honesty when the server shows no new turn**
  (`fold_recovery_parked`): the old `.max(1)` diff re-rendered the
  PREVIOUS turn's reply, misattributed as the answer, when `turn_n` had
  not advanced (a read-timeout racing an undelivered POST). Now an
  honest Info line says the message may not have arrived.
- **`status:"cancelled"` turn bodies close the conversation** (the
  gateway's crash-orphan recovery outcome) instead of falling into the
  parked arm and self-healing a poll later.
- **Transcript user turns split chrome from words** (live-gate finding:
  `_visit.history` stores the RENDERED user message): adopt/rehydrate
  presented ~20 lines of presence + dated MEMORIES prompt chrome as the
  visitor's own words. `entities::split_rendered_user` puts the raw words
  on the user card and the MEMORIES block behind the details toggle
  (probe parity with live turns); unrecognized structure renders whole —
  the failure direction is chrome shown, never words hidden.
- **Panicked entity threads land actionable states**: a dying
  open/turn/close thread now posts a guarded recovery fold (Refused /
  transport-error / error card) beside the death toast — a panicked turn
  thread used to strand TurnRunning forever (/end refused, chip
  spinning); a dead poller now clears its started-latch so the next open
  respawns it; a dead roster refresh clears the modal's "refreshing…".
- **`/entities` modal Ctrl+D actually works**: the footer promised
  provenance behind Ctrl+D, but modal layers swallow keys before root
  shortcuts (engine overlay dispatch) — the modal now binds it.
- **`/task` confirmation copy pinned character-exact**
  (`TASK_CONFIRMATION_COPY` + test): the entity seat's verbatim wording,
  never "immediately"/"notified"/a minutes estimate.
- **Entity card lines carry content, not just titles**: identity values
  rendered as bare titles ("values: shared_vulnerability") and
  engine-minted placeholders as literal "traits: trait-0"; title+statement
  now combine with redundancy folds (truncated interest titles yield to
  the full statement; placeholders yield to the body).
- **Header chips truncate whole, with an honest `+N`**: 3+ conversations
  at 100 cols rendered a mangled "◆eph…" tail fragment; hidden chips now
  collapse to a count.
- **Held-draft strip marker is status-honest**: it renders only while a
  hold can exist (Opening/TurnRunning, with per-state wording) — beside
  "parked" it promised a send that had already happened or never would.
- **Live-shape fixtures**: `close_reply.json` re-captured from the
  cycle-2 doorcheck gate (clean close through the 600s lane;
  reflection_notices empty — the noisy cycle-1 shape stays pinned
  inline); `transcript.json` user turn carries the real rendered form.

### Added (queue/steer model — plan item 1, 2026-07-22; reconciled to the cycle-2 plan)

- **`/queue <text>` + manager modal** (`src/ui/queue_modal.rs`): a FIFO
  prompt queue. Enter keeps STEERING while a run is active (the
  latency-sensitive path stays zero-friction); `/queue` lines up the next
  task. On a successful run completion the head drains as a NEW run whose
  `StartOpts` build at drain time — the context carries the just-finished
  answer. Failure/cancel PAUSES the queue (items kept; explicit resume via
  the modal's `r`); a queued START that fails — HTTP or client refusal —
  RESTORES the item at head and pauses (nothing was spent; `r` retries);
  a manual run while paused proceeds without auto-resuming. The dequeue
  runs as a deferred job and HOLDS while a wait is pending (a wait can arm
  after the composer frees; a drain-started run would wipe the prompt and
  orphan it) — the wait's resolution re-fires the drain. Manager keys:
  ↑↓ select, `x` remove, `u`/`d` reorder, `c` clear, `r` resume, `e`
  pop-to-composer, Esc close.
- **The queue persists per session** (cycle-2 reversal of drop-on-quit):
  write-through to a prefs slot (`session_queues`, keyed like
  `recent_sessions`) on every mutation; `/new` and session switches STASH
  it with the session it belongs to (visible echo) and load the target's
  stash; quit keeps it (courtesy stderr note). EVERY restore — boot or
  switch — lands PAUSED and never auto-starts: safety lives in the
  restore posture, not in dropping the user's queued work.
- **No-cycling-target submits buffer instead of dropping**: text entered
  while a run is starting OR before its first reasoning cycle becomes
  `pending_steer`, delivered on the NEW tree's first reason-cycle record
  into the CYCLING subrun (`Fold::cycling_target()` + a run-identity
  predicate — a root-targeted steer is silently never folded on wrapper
  bundles, and a stale previous-run cycle can never satisfy delivery).
  Disposal is visible: start failed → error card with the text; run
  finished before any cycle → info card; session boundaries echo the drop.
- **Queue is agent-lane only**: `/queue` under entity focus refuses,
  pointing at the visit's held-draft lane; queue strip hints render under
  agent focus only; the drain runs regardless of focus.
- **Discoverability**: the activity strip appends `N queued` / the paused
  notice in every phase; the status-bar legend swaps while running
  (`enter steer · /queue later`); the composer placeholder is
  phase-swapped under agent focus; `/help` + completion entries.

### Added (Ctrl+J newline — plan item 2, 2026-07-22)

- **`Ctrl+J` inserts a newline at the caret** — registered on the
  COMPOSER's own element (never root: a root shortcut fires with focus
  anywhere and would inject into an unfocused composer). It is the LF
  byte on the legacy wire, so it works in every terminal; `Alt+Enter`
  and kitty-protocol `Shift+Enter` keep working. Placeholder, `/help`,
  README, and the docs key table teach the honest matrix (WezTerm ships
  with the kitty protocol off — no unconditional claim).

### Added (/goal client half — plan item 3, 2026-07-22; dark until the flow seat publishes)

- **`/goal <text>`** starts a goal run on a catalog workflow implementing
  `abstractcode.goal.v1` (discovery generalized:
  `workflows_with_interface` — one parser, agent + goal interfaces), with
  `input_data {goal, max_cycles, use_session_history}` (`max_cycles` from
  prefs `goal_max_cycles`, default 8). `/goal` shows status (text, cycle,
  elapsed, tokens); `/goal stop` cancels durably. Zero goal workflows →
  an honest notice naming the interface; the feature lights up on catalog
  load when a bundle appears.
- **`Fold::finish_on_root_only`** (the P0 defense): while set, `finished`
  fires only on the ROOT's own flow end/terminal — an agent-subrun
  answer-shaped flow end renders as a NON-final card instead of releasing
  the composer (goal bundles start one cycling subrun per iteration;
  without this the loop read finished at iteration 1). The goal and its
  run id persist per session (`session_goals`), so a restart that
  reattaches to a live goal run restores the flag; a stale recorded goal
  clears via `/goal stop`.

### Fixed (coder-workflow stats + conclusion — maintainer bug e, 2026-07-22)

- **"0↑ 0↓ tk" on splitless-usage providers**: the live coder run's
  provider (gpt-5.6-sol) reports `{"input_tokens": 0, "output_tokens": 0,
  "total_tokens": N}` — only totals. The fold now accumulates
  `total_tokens` (per-run + session), the strip/idle summary show the
  honest `N tk` total when the split is absent, and the sparkline
  substitutes per-call totals. Regression-pinned by a fixture distilled
  from the live bug run's own ledgers
  (`tests/fixtures/coder_run_tree.json`, depth-3 tree, 513k tk).
- **Sticky "Done" activity**: wrapper-bundle helpers emit per-round
  `abstract.status {"value": "Done"}` events; terminal-sounding texts
  (done/finished/cancelled/failed) now CLEAR the activity line instead of
  sticking for hours while the tree keeps working ("Done · cycle 12 ·
  17880s" read as concluded — it wasn't).
- **Report-only flow ends conclude the turn**: coding-agent/coder end
  outputs carry `{report, passed, …}` with no answer/response key; the
  report is now the answer-text fallback, so the turn concludes with a
  real final card instead of ending silently on the terminal-status poll.
- **Reattach elapsed honesty**: attaching to a live run now back-dates
  the elapsed clock to the run's gateway `created_at` (was
  elapsed-since-attach: a reattached 2-hour run displayed "3s"); the
  seconds counter also resets at attach/start so a previous run's stale
  value never flashes. `fold.failed` is now actually set on failed
  roots/terminals (was declared, never written; exec exit codes and the
  queue's pause-on-failure read it). Cycle-2 verification pinned the flag
  semantics with a unit test: root failure sets it, subrun failures never
  do, cancel is not a failure, and `begin_run` resets it.

### Added (approval tiers + workspace UX — maintainer bugs a/b/d, 2026-07-22)

- **Persistent tool-approval tiers** (`src/tool_policy.rs`): batches whose
  every call classifies at-or-below the accepted tier (`read` < `write` <
  `all`) resume without a prompt — "if the highest tier is accepted,
  nothing is ever asked". Persisted as `prefs.json tool_approval`
  (`accepted_tier` + per-tool `overrides` `{"name": "auto"|"ask"}`), set
  via `/tools tier <t>` or the `t` key in `/tools`; `/auto` (session
  blanket) composes on top. Classification is client-side by tool NAME +
  ARGUMENTS (#FALLBACK: dies when the gateway serves tier fields — ask
  commons 4336); `execute_command` is never below `all` except the
  adversary-hardened read-only-git proof, ported faithfully from the
  Python `abstractcode` precedent with its full attack corpus as tests.
  Raising the tier re-decides an open prompt immediately. Headless `exec`
  reads the same prefs: at-or-below-tier batches approve, above-tier
  batches DENY naming the rule (`--approve-all` still overrides).
- **Human-readable approval modal** (`src/ui/approval_view.rs`): per call
  a headline (tool name + needed tier), a one-line intent summary, and
  aligned `key value` parameter rows (strings unquoted, long/multiline
  values truncated with honest markers); `execute_command` shows the
  COMMAND string first-class (`$ …`). Batches of 2+ get `── call i/N ──`
  separators. `f` toggles the full pretty JSON; a tier line names why the
  prompt exists ("tier: write accepted — this batch needs: all").
- **`/workspace` command + modal**: shows the run's workspace root,
  picks the access mode (server-managed default / `workspace_only` /
  `workspace_or_allowed` / `all_except_ignored` — the gateway's real
  vocabulary), and manages extra allowed roots sent as
  `workspace_allowed_paths`. Mode + allowed paths persist in prefs.json
  (headless `exec` reads them too); adding a path auto-picks
  `workspace_or_allowed` (the only mode that uses it) with a notice. The
  modal states plainly that the GATEWAY enforces policy and may clamp
  client paths. `docs/troubleshooting.md` gained entries for the red
  "Path escapes workspace_root" refusals and for over-eager approval
  prompts.

### Fixed (cycle-3 whole-system audit — cross-lane seams, 2026-07-22)

The three feature lanes above were built by six concurrent agents; this
pass audited their COMPOSITIONS as one system (goal × queue × tier ×
entity focus × session boundaries) and pinned each cell with a test.

- **Steering between goal iterations no longer vanishes into a dead
  run**: a goal iteration's own flow end now clears the fold's cycling
  target (its guidance inbox died with the run — a terminal run never
  drains a steer), so text typed between iterations BUFFERS and delivers
  into the NEXT iteration's first cycle. Under `finish_on_root_only`
  the answer-source lane also FOLLOWS the live iteration: iteration 2+
  results render as non-final cards and ctx/model telemetry stays
  honest (first-wins had them folding from iteration 1's dead run).
- **A start landing after `/new` or `/sessions` can no longer hijack the
  fresh session**: the start's HTTP round trip races session boundaries —
  the late Ok used to bind the orphan run into the NEW session's view
  (capturing its composer and streaming a foreign transcript), and a
  late start FAILURE wrote the outcome that paused the new session's
  queue. Both outcome posts are now session-guarded; an orphan run is
  cancelled durably with a notice.
- **Dead state deleted** (one authority per concern):
  `entity_actions::composer_placeholder`'s `Focus::Agent` arm
  (production-dead — `ui::agent_placeholder` owns agent placeholders —
  and drifted from the live Ctrl+J teaching; the helper is entity-only
  `entity_placeholder(name)` now), `Fold::steer_target()` (the root
  fallback the pending-steer lane exists to avoid; no production caller
  left), and `UiCtx.workspace_mode` (an unread copy of the
  `store.workspace_mode` signal, the one authority).
- **Cross-lane matrix pinned by tests**: goal runs hold the queue
  through every iteration and the drain starts the next item as a
  NORMAL agent run after the goal's root ends; goal runs carry the
  persisted tier policy / workspace scope / skills through the shared
  StartOpts path (and no client transcript); queued items expand the
  tier policy AT DRAIN TIME (a mid-run `/tools tier` change reaches the
  drained run); agent approvals prompt, auto-approve, and name their
  lane on the strip even while an entity conversation is focused;
  session boundaries reset exactly the session-scoped lanes (queue
  stash + paused restore, steer echo-drop, goal slot follows its
  session, `/auto` off, focus home) while persisted prefs and entity
  visits survive. One `build_input_data` test loads EVERY StartOpts
  surface at once and asserts the `_runtime` map composes additively
  (provider + model + tool_policy, nothing clobbered); one prefs test
  round-trips every field, another feeds every field wrong-typed and
  asserts defaults (never a panic, never a widened approval posture);
  `/help` + the completion list are pinned to cover every new command
  exactly once.

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

### Changed (0.2.1 review pass, same day)

- The five single-select picker modals (theme, workflow, provider, model
  stage, sessions) share ONE `Picker` shell (labels + start index +
  `on_choose`, optional hint/live-preview/cancel hooks) — the
  activation/Esc wiring and its disposal-safety rationale now live in one
  place. Behavior byte-identical; sizes stay caller-computed.
- `Fold::mark_wait_tools` drops its silently-ignored `wait_key`
  parameter: tool cards carry no wait identity and only one wait is ever
  pending, so a key parameter promised key-scoped marking the
  implementation cannot deliver.
- Diff tinting is proven at the pixel level: a headless test reads the
  modeled screen's cell inks and asserts a ```diff fence renders
  added/removed lines in the theme's ok/error inks and context lines in
  the body ink. The headless harness now injects FIXED terminal
  capabilities (truecolor + unicode) instead of env detection — the
  host's TERM can no longer steer what tests assert.
- Space confirming in single-select pickers is documented (api.md key
  reference).

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
