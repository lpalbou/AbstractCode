# Reference: CLI, commands, keys

## CLI

```
abstractcode [OPTIONS]                    launch the TUI
abstractcode exec "<prompt>" [OPTIONS]    headless one-shot run
abstractcode login [OPTIONS]              verify + persist credentials
abstractcode doctor [OPTIONS]             diagnose the connection
abstractcode --caps                       terminal capability report
abstractcode --help | --version
```

### Options

| Option | Meaning | Default |
| --- | --- | --- |
| `--gateway <URL>` | Gateway base URL | flag > env > login store > `http://127.0.0.1:8080` |
| `--token <TOKEN>` | Bearer token | flag > env > login store |
| `--session <ID>` | Durable session id | a fresh mint (`acode-<hex>`); `--resume`/`--continue` reopens the last one |
| `--ungated` | Run a gating-capable workflow unattended (`gating_mode=auto`, skips its approval pauses); also `--no-gate`/`--auto`. REFUSED unless `--permissions` is set on the same command line | gated |
| `--reasoning <LEVEL>` | Reasoning effort: `none\|minimal\|low\|medium\|high\|xhigh\|auto` (also `--thinking`; validated at launch; works on `exec` too) | gateway default |
| `--workflow <bundle[:flow]>` | Agent workflow | saved pick, else `basic-agent` |
| `--provider <NAME>` | Provider override | gateway defaults |
| `--model <NAME>` | Model override | gateway defaults |
| `--workspace <PATH>` | Requested workspace root | current directory |
| `--no-workspace` | Send no workspace root | — |
| `--workspace-mode <M>` | Workspace access mode | server default |
| `--theme <ID>` | Start theme | `ABSTRACTTUI_THEME`, else saved pick |
| `--animation <on\|off>` | Launch animation; **persisted** to `prefs.json` (`animation`). Also skipped when stdout is not a tty, `NO_COLOR`/`TERM=dumb` is set, or `ABSTRACTTUI_NO_SPLASH` is set | on |
| `--max-iterations <N>` | Ask the server for a specific iteration budget | none — **this client sets no budget**. Absent the flag the server's own default applies (the same one every client gets); the hard ceiling is the runtime's, enforced at run start |
| `--replay-turns <N>` | Prior turns replayed in full detail at boot (0 disables) | 20 |
| `--permissions <level>` | tool permissions for the invocation: `read` \| `write` \| `all` | prefs level |
| `--require-approval <t>` | gate these tools regardless of level (comma list, repeatable); exec denies them | — |
| `--no-prompt-cache` | exec: opt the run out of the runtime prompt cache (`_runtime.prompt_cache=false`) | server truth (on) |
| `--timeout <SECS>` | exec: overall deadline | 900 |

### Environment

| Variable | Meaning |
| --- | --- |
| `ABSTRACTCODE_GATEWAY_URL` / `ABSTRACTFLOW_GATEWAY_URL` / `ABSTRACTGATEWAY_URL` | Gateway URL (first set wins; beats the login store) |
| `ABSTRACTCODE_GATEWAY_TOKEN` / `ABSTRACTGATEWAY_AUTH_TOKEN` / `ABSTRACTFLOW_GATEWAY_AUTH_TOKEN` | Bearer token |
| `ABSTRACTCODE_GATEWAY_CONNECTION_FILE` | Login store path (default `~/.abstractcode/gateway.json`) |
| `ABSTRACTCODE_PREFS_FILE` | Preferences path (default `~/.abstractcode/prefs.json`) |
| `ABSTRACTTUI_THEME` | Start theme id |
| `ABSTRACTTUI_NO_SPLASH` | Set to anything but `0` to skip the launch animation for one run (the persisted switch is `--animation off`) |

### Exit codes (`exec`)

`0` completed · `1` failed · `2` usage/config error · `124` timeout ·
`130` cancelled.

## Slash commands (composer)

| Command | Effect |
| --- | --- |
| `/help` | Command + key reference modal |
| `/new` | Fresh session (new durable id, cleared view) |
| `/theme [id]` | Live-preview theme picker, or set directly |
| `/workflow` | Pick the agent workflow from the gateway catalog |
| `/model` | Pick provider + model from gateway discovery |
| `/tools` | Enable/disable gateway tools (`Space` toggles; checked set = the run's exact allowlist; untouched = workflow defaults). In-modal: `p` cycles a per-tool approval pin, `t` cycles the tier — see the modal keys below |
| `/permissions [read\|write\|all]` | THE tool-permission surface (bare = report): batches classifying at-or-below the level auto-approve. `read` = proven read-only tools only; `write` adds workspace file mutations; `all` auto-approves everything, **including arbitrary shell and network egress** — deliberate use only. Per-tool `ask` pins and gateway-disabled tools still gate. Sticky per session (`/tools tier` remains a spelling alias) |
| `/workspace` | Inspect + edit the filesystem scope tools may touch: root (from `--workspace`/cwd), access mode, allowed paths. Mode + paths persist and ride every run |
| `/skills` | Attach gateway skills to your runs (`Space` toggles; sent as `input_data.skills`) |
| `/mcp` | The gateway's MCP server registry (read-only; their tools appear in `/tools` once declared) |
| `/cache` | Prompt-cache + context metrics: route, latest call, run, session |
| `/resources` | Gateway-host resources (`/host` is an alias): memory (RAM/device meter bars, GPU utilization when the host supports it, the gateway's process RSS, host name), resident models (modality label, tri-state residency — an unreported residency reads `unknown`, never "no" — size, `ctx N` with `*` = calibrated, 🔒 = residency lock, `default`), session prompt caches, totals. Admin actions on the selected model: `u` unload (two-step confirm; a 409 `model_locked` refusal offers `f` force), `k` lock/unlock, `e` context estimate, `r` refresh — keys under "Modal keys" below. Data is fetched at open, on `r`, and after a mutation — never polled; a failed refresh keeps the last snapshot marked STALE. Requires the gateway's declared `host_state` contract (`/discovery/capabilities`) — older gateways get an honest "not supported". Feeds the footer's `mem NN%` segment |
| `/history [n\|all]` | Stream the PREVIOUS bloc of this session's turns from the gateway ledgers, prepended in full detail. Boot replays only the last bloc (`--replay-turns` sizes it, default 5); a stub line names how many earlier turns exist. **Scrolling to the top of the transcript auto-loads the previous bloc** — the stub becomes a live progress line and holding at the top cascades bloc-by-bloc until the session is fully loaded (Esc returns to the tail and stops the cascade). Failures name their cause — never a silent hole |
| `/status` | The status card: workflow, route, workspace, session, connection, client phase + run id + last outcome, and a LIVE gateway run-status probe — the one place client view vs server truth is inspectable (wrapper roots legitimately stay `waiting` after your turn concluded) |
| `/sessions` | Pick a recent session to continue (named by first prompt) |
| `/session [id]` | Show or switch the session id (switching probes for a live run to reattach) |
| `/details [full\|fold]` | Toggle transcript verbosity (`Ctrl+D`): `fold` (the default) renders each tool call as ONE line — glyph, name, status word (`ok`/`failed`/`running`/…), faint args hint — and thinking as a capped gist; `full` expands the whole card UNCUT (arguments in full on their own rows, the `│`-guttered result body entire, thinking content plus the labeled reasoning channel — nothing shortened). Thinking and every called tool stay visible in BOTH states — the toggle gates detail, never existence; errors always keep their `↳` bodies |
| `/gating [auto\|wait]` | Approval gating for gating-capable workflows (the multi-agent coder): `auto` runs unattended (skips the workflow's human-approval pauses), `wait` re-gates (the default). Selecting the coder also opens a gated/unattended choice. Rides `input_data.gating_mode`; tool approval is a SEPARATE axis (`/permissions`) |
| `/reasoning [level]` | Reasoning effort for the current route (`none\|minimal\|low\|medium\|high\|xhigh\|auto`; `default` clears). Bare `/reasoning` opens the dial — also stage 3 of `/model`: pick provider, then model, then effort. Non-reasoning models show a locked `none` (set-anyway override available while capability provenance is unserved); the choice is pair-coupled — changing provider or model resets it. Rides the run as `_runtime.thinking` (absent = gateway default) |
| `/export [md\|jsonl] [--details] [path]` | Export the agent transcript to a file: `md` (default) = archival markdown; `jsonl` = SFT training lines. `--details` adds reasoning + full tool cards. Bare `/export` auto-names in the cwd; never overwrites — see "Transcript export" below |
| `/attach [path\|preview\|clear]` | Stage a file for your NEXT message (chips above the composer; uploaded at send as `context.attachments`; **session uploads are permanent** server-side). Accepts `~`, quotes, escaped spaces, `file://`, relative paths. Bare `/attach` opens the file browser (nothing staged) or the pending manager (chips staged); `preview [n\|path]` opens the file itself (text or PNG/JPEG — staged or not); `clear` discards. Dropping a file onto the terminal attaches directly — `Ctrl+O` undoes (chips out, path text back). Agent-lane only (v1) — see "Attachments" below |
| `/auto` | Removed (the session blanket had latent holes) — opens the `/permissions` report teaching the replacement |
| `/pause` | Pause the run tree durably on the gateway (stops at the next step boundary; survives quitting the client) |
| `/resume` | Resume a paused run tree |
| `/cancel` | Cancel the active run |
| `/conclude [note]` | Ask the agent to **wrap up now** and answer from what it already has — the missing verb between `/pause` (freeze) and `/cancel` (throw away). The loop stops reasoning at its next boundary and runs its tool-free conclusion, so the turn ends with a real answer plus what is left to do. A `note` is quoted to the model verbatim. Server-side verb (`POST /commands {type:"conclude"}`): the same request from AbstractObserver, the console or a chat bridge behaves identically, and the turn reports `stop_reason.code = "operator_conclude"` — not a failure, not a spent budget |
| `/steer <text>` | Explicit steering (plain Enter during a run steers too; buffered until the run's first reasoning cycle when it has not started cycling yet). A send that hits a transport failure is retried once with the same command id — the gateway dedups on it, so the retry is exactly-once — and if it still fails your words are kept: they ride the error card and return to the composer when it is empty |
| `/queue [text]` | Queue a prompt (FIFO): auto-runs after the current run **succeeds**; halts on failure/cancel (explicit resume). Persists per session and restores **paused** — a restore never auto-starts. Bare `/queue` opens the manager (keys under "Modal keys" below). Agent-lane only: under entity focus the visit's held-draft lane is the queue |
| `/goal [text\|stop]` | Start a goal run on a goal workflow (`abstractcode.goal.v1`): loops until verified done or `max_cycles` (prefs `goal_max_cycles`, default 8). Bare `/goal` shows status; `/goal stop` cancels durably. Ships dark until a goal bundle is published on the gateway |
| `/context [n\|off]` | Declare the model's context window in tokens (`262144`, `262k`, `1m`) — the footer meter becomes `ctx used/window tk (%, declared)`, warn ≥75% / error ≥90%, and the declaration rides runs as `_limits.max_tokens`. Bare `/context` reports; `off` clears. Persisted (`context_window`); `--max-tokens` declares for one session. Source-labeled "declared" — never a client capability table |
| `/redraw` | Force a full-screen repaint (`Ctrl+L`) — recovery from an external terminal clear (Cmd+K), which damage-tracked rendering cannot detect |
| `@name [text]` | Talk with a summoned entity: bare `@name` opens (or focuses) a durable visit; `@name <text>` opens and sends the first turn. An unknown name never becomes an agent prompt — the draft is preserved with a roster hint. A partial `@na` opens a completion dropdown (cached roster); accepting inserts the name and the NEXT Enter submits |
| `/entities [name]` | Entity roster + identity cards (opens instantly on the cached roster, refreshes async — the live fetch can be slow behind the gateway's drives fold). `[name]` deep-links to that card |
| `/brain <name>` | FLOW-BRAIN conversation with an entity: each message is one summon of the `entity-chat` VisualFlow through the entity door (poll to terminal; the structured `degraded`/`moment_error` contract renders as warn lines). Continuity rides the entity's own memory graph under one client-minted session id; the view is session-local, and `/end` closes it locally (no server visit exists). One conversation per entity: a live `@name` visit refuses with "/end first"; bare `/brain` reports the focused conversation's brain |
| `/task <name> <title>` | Leave a task on the entity's desk — durable, no visit needed, works while the entity sleeps. Pickup happens at the entity's own boundary (day end, wake check, or visit close), never "immediately" |
| `/end [name] [reason]` | Close the visit (the entity's reflection runs server-side; a visit that woke a sleeping entity restores the sleep). Refused mid-turn — entity turns are non-interruptible |
| `/focus <name\|agent>` | Switch conversation focus (`Alt+E` cycles) |
| `/quit` | Exit — with a live agent run, the quit gate opens first (see "Quitting with a live run") |

Anything that is not a command is a task (when idle) or steering guidance
(while a run is active). Under **entity focus**, submitted text is that
visit's next turn — or the held draft while a turn runs (later text
replaces the hold; it auto-sends when the turn parks).

### Transcript export (`/export`)

`/export [md|markdown|jsonl] [--details] [path]` — every token optional.
Bare `/export` writes markdown, auto-named
`abstractcode-export-<sid8>-<YYYYMMDD-HHMMSS>.md` in the current
directory. The format word wins over the path's extension; a conflict
(`/export md out.jsonl`) refuses. A known extension alone infers the
format. `--details` is the export's own flag — the `Ctrl+D` view toggle
never changes the output. v1 exports the **agent-lane** transcript
(entity visits are separate conversations).

- **Markdown** (archival): a header (session, workflow, timestamp, item
  counts, and an explicit incompleteness note when the client view has
  truncated older items), then the conversation with `## User` /
  `## Assistant` markers. Default mirrors the clean view plus one-line
  tool activity summaries; `--details` adds reasoning cycles (quoted) and
  full tool cards (fenced args + result previews). Errored tools keep
  their full card in both modes. Images are referenced by artifact id,
  never bytes.
- **JSONL** (SFT/CPT training): OpenAI chat schema, **one line per
  completed turn**, each line carrying the cumulative message prefix up
  to and including that turn's final answer — every line is a
  self-contained training example; the last line is the whole session
  (take just it for whole-session/CPT use). Unanswered turns
  (failed/cancelled runs) are **skipped** — a dangling user prompt is
  provider-hostile — and counted in the notice, never written to the
  file. Default lines carry **only** `messages` (strict validators
  accept them as-is); `--details` adds a `details` side field (that
  turn's tools/cycles/steers as full strings — deliberately
  not fabricated `tool_calls`: the client holds a humane rendering, not
  wire-faithful call structures; full traces live in the gateway run
  ledgers).

Honesty bounds: bodies export as rendered — prompts/answers full text,
tool args/results preview-bounded upstream by the client fold. Writes
refuse existing files (never overwrite), never create parent
directories, and `~` is not expanded. Paths with spaces are not
supported (composer grammar); one path token max.

### Attachments (`/attach`, drag & drop, `exec --attach`)

Files stage as PENDING chips (validated at attach: exists, regular
file, within the gateway's size cap when it declares one) and upload at
SEND on the worker thread — removing a chip before sending is a true
no-op. Refs ride the run as `context.attachments` (the agent lane's
preferred media key); text-like files inline into the model's context
server-side (120 KB/item), PDFs extract on `open_attachment`, images
need a vision-capable route, other binaries are listable-not-readable
(the attach notice says which).

- **The chips row is interactive**: a staged file's NAME opens its
  preview, and the `×` beside it unstages the file. Removing a chip
  before the send is a true no-op (nothing has been uploaded yet).
  Names are capped at 20 characters plus an ellipsis so one long
  filename cannot own the row; the preview header and the manager spell
  the full name out.
- **Custody**: an upload or start failure blocks the send and KEEPS the
  chips (error card names the server detail); refs minted before the
  failure are cached, so the retry never duplicates artifacts. Chips
  clear only when the run actually starts — a `📎` line then records
  what rode the turn (and rehydrates on session restore). Send
  snapshots the staged set: a chip removed while the upload is already
  in flight still rides that run (you pressed send with it staged);
  removal governs the NEXT send.
- **Drag & drop**: dropping files onto the terminal arrives as a paste
  of their paths; a verified drop (engine spelling classifier +
  client existence check) attaches directly — nothing lands in the
  composer — with `Ctrl+O` as the undo (chips out, the raw path text
  back). Ambiguous pastes (prose, unescaped-space multi-drops on raw
  terminals, nonexistent paths) insert as text, byte-identical.
  Folders refuse (drop files, not folders).
- **Image attachments** render a mosaic preview in the transcript when
  they ride a run (and again on session restore) — you see exactly
  what was attached. Reminder: the image reaches the MODEL only on a
  vision-capable route.
- **Preview** (click the chip, `/attach preview [n|path]`, or `p`/`Enter`
  in the manager) opens the file's real bytes BEFORE it rides anything: a
  scrolling, line-numbered document for text-like files, or the picture
  itself for PNG/JPEG (same mosaic ladder as the transcript). Magic
  bytes decide the kind, never the extension. It is a look, not an
  attach — `/attach preview <path>` works on a file that is not staged
  and stages nothing. Bounds are stated, never silent: text previews
  the first 512 KB of a larger file and the header says so; invalid
  UTF-8 is labeled; tabs expand to 4 columns; long lines WRAP (the
  wrapped tail carries no line number, so the numbers stay a true index
  into the file) and re-wrap when the terminal is resized. ANSI escape
  sequences are removed so colored logs read, and the header says they
  were; BOM'd UTF-16 is transcoded and labeled. Known limit: BOM-less
  UTF-16 is indistinguishable from binary and refuses as such. The engine draws PNG and JPEG only — GIF/WebP/BMP/TIFF
  and PDFs say so by name and remind you the attachment itself still
  uploads and works. Reading and decoding run on their own thread, so a
  large photo shows `reading…` rather than freezing the interface.
- **Boundaries**: session uploads are PERMANENT server-side (the
  session's attachment index has no delete surface). `/new` and session
  switches discard pending chips with a notice. Chips never ride
  steers, `/queue` drains, or `/goal` runs; entity lanes refuse (v1).
- **Headless**: `exec --attach <path>` (repeatable) uploads before the
  run starts and exits 1 on any failure — nothing spent.

### Quitting with a live run

The agent runs on the gateway — quitting this client never stops it.
Every quit gesture (`Ctrl+Q`, `/quit`, double-`Ctrl+C`) funnels through
one gate: idle quits instantly; a live agent run opens a modal with
three verbs — **leave it running** (Enter — the default: nothing is
sent, the run continues durably, relaunching this session reattaches),
**pause, then quit** (`p` — durable gateway pause; `/resume` after
relaunch continues it), **cancel it, then quit** (`c`). `Esc` stays.

Honesty mechanics: pause/cancel quit only after the gateway ACCEPTS the
durable command (up to 8s) — a failure shows an honest state offering
quit-anyway/stay instead of pretending delivery. Repeat quit gestures
always resolve to the safe verb (leave), so hammering `Ctrl+C`×3 /
`Ctrl+Q`×2 exits at worst one press slower than before — and cancel is
never reachable by repetition. A run concluding while the modal is open
auto-quits (queued prompts are held back and restore paused next
launch). Entity visits never gate — visits park on quit by design;
reopening resumes them. Honest limit: closing the terminal window
(SIGHUP/SIGKILL) shows no modal — the run continues and boot reattach
recovers it.

## Keys

| Key | Context | Effect |
| --- | --- | --- |
| `Enter` | composer | Send task / send steering |
| `Ctrl+J` | composer | Insert a newline — works in **every** terminal (it is the LF byte on the legacy wire); the composer grows to 4 rows |
| `Alt+Enter` | composer | Insert a newline (Option+Enter on macOS with "Option as Meta/Esc+"); `Shift+Enter` also works wherever the kitty keyboard protocol is live (kitty/Ghostty/foot from startup; iTerm2 ≥ 3.5, VS Code/Cursor, Warp via the mid-session probe) |
| `↑` / `↓` | composer at buffer edge | Recall sent messages (input history) |
| `Tab` / `Enter` | `/` and `@` completion dropdowns | Accept the highlighted entry (Esc dismisses; a fully-typed command or `@name` submits directly on the first Enter) |
| `Esc` | composer with text | Clear the composer |
| `Esc` | scrolled up | Jump back to the live tail — that press is consumed (it never arms cancel) |
| `Esc Esc` | while running, at the tail | Cancel the run (within 900ms). Under entity focus it explains instead: entity turns are non-interruptible |
| `PgUp` / `PgDn` | anywhere | Scroll the transcript (PgDn to the tail re-sticks) |
| mouse wheel | transcript | Scroll (unsticks from the tail) |
| `Ctrl+D` | anywhere | Toggle detail view (thinking + tool results vs answers only) |
| `Alt+E` | anywhere | Cycle conversation focus: agent → entity visit 1 → … → agent. Cycle order is the order conversations were opened — it never changes with how the header paints chips. Option+E on macOS with "Option as Meta/Esc+"; `/focus <name\|agent>` needs no modifier setting. (`Ctrl+E` is move-to-line-end in the composer) |
| `Ctrl+T` | anywhere | Cycle theme |
| `Ctrl+L` | anywhere | Force a full-screen repaint (`/redraw`) — recovers from a terminal clear |
| `Ctrl+O` | while a drop's chips are still pending | Undo the newest file drop: chips out, the pasted path text back in the composer. Expires once the chips ride a run or are removed |
| `?` + Enter | empty composer | Open the keys + commands reference (the footer's `? keys + commands`) |
| `Ctrl+Q` | anywhere | Quit |
| `Ctrl+C` | anywhere | Clear the current prompt (if any) and arm quit — a second Ctrl+C within 2s quits (with a live run, the quit gate opens; a third press = leave & quit) |
| `↑↓` + `Enter`/`Space` | pickers | Move + choose — Space confirms too, single-select semantics (theme picker previews live) |
| `Tab` | anywhere | Move focus (composer ↔ transcript ↔ modal fields) |
| any character | transcript focused | Return focus to the composer and keep the character — typing is never dropped because focus wandered. `/` opens the command dropdown the same way |
| paste / file drop | transcript focused | Same: pasted text lands in the draft, a dropped file becomes an attachment chip, and focus returns so you can type the prompt that goes with it |
| `Ctrl+A` / `Ctrl+E` | composer | Move to line start / line end (`Home` / `End` also work) |
| `Alt+B` / `Alt+F` | composer | Move one word left / right (`Ctrl+←` / `Ctrl+→` also work). Hold `Shift` on any form to extend the selection |
| `Ctrl+W` / `Alt+D` | composer | Delete the word before / after the caret (`Alt+Backspace`, `Ctrl+Backspace`, `Alt+Delete`, `Ctrl+Delete` also work) |

### Modal keys

| Modal | Keys |
| --- | --- |
| approval | `a` approve · `A` approve all (sets permissions: `all`, sticky per session) · `d` deny · `f` toggle full JSON of the calls · `Esc` defer (the run keeps waiting; Enter on the empty composer reopens) |
| ask (agent question) | `Enter` send the answer · `Esc` keeps the run waiting |
| `/tools` | `Space` toggle · `a` all on · `n` all off · `p` cycle the per-tool pin (none → auto → ask → none; a pin beats the tier both ways) · `t` cycle the approval tier (read → write → all, persisted) · `Enter`/`Esc` close |
| `/queue` | `↑↓` select · `x` remove · `u`/`d` move up/down · `c` clear all · `r` resume a paused queue · `e` pop the prompt into the composer for editing · `Enter`/`Esc` close |
| chips row | click a staged file's name to preview it · click its `×` to unstage it |
| `/attach` manager | `↑↓` select · `Enter`/`p` preview · `x` remove · `c` clear · `b` browse (file picker) · `Esc` close |
| attachment preview | `↑↓` scroll · `PgUp`/`PgDn` page · `Home`/`End` ends · `Enter`/`Esc` close |
| `/attach` picker | type to filter · `↑↓` move · `Enter` descend into a folder / attach the file (marked set when non-empty) · `Space` mark files for multi-attach · `Backspace`/`←` parent folder (filter empty) · `Esc` close |
| `/resources` | `↑↓` move (model, cache and totals rows are all reachable; admin keys act on the selected MODEL row) · `u` unload → `y`/`Enter` confirms · `f` force-unload (confirm labeled FORCED) · `n`/`Esc` cancel an armed confirm · `k` lock/unlock residency · `e` context estimate (inline result) · `r` refresh (re-probes capabilities while the contract is unconfirmed) · `Enter`/`Esc` close |
| `/entities` | `↑↓` browse (the identity card follows) · `Enter` talk (`@name`) · `t` leave a task (title prompt) · `e` end that entity's open visit · `Ctrl+D` show per-section provenance · `Esc` close |
| `/workspace` | `↑↓` move · `Space` select an access mode / remove an allowed path · type a path + `Enter` adds it (switches to `workspace_or_allowed` when needed) · `Esc` close |

## Transcript vocabulary

The transcript reads as TURNS: a `══ you ══…` rule opens each one, a
`══ assistant ══…` rule closes it with the answer (markdown), and the
reasoning cycles between them are delimited by `── cycle N · 41s ·
29k↑ 512↓ tk · 92% cached ──…` rules carrying that model call's
duration, token cost, and — when the provider reports it — how much of
the prompt was served from cache instead of recomputed. The model's own
thinking leads every cycle; the cycle's tool calls stack directly
beneath it, one line each.

| Element | Item |
| --- | --- |
| `══ you ══…` | Your task (turn opener) |
| `── cycle N · cost ──…` | One reasoning cycle: the thinking, then its tools |
| `✓ name · ok  args…` | Tool call: glyph, name, status word, faint args hint |
| `? · awaiting approval` / `» · running` | Tool paused / executing |
| `✗ · failed` / `⊘ · denied` / `◌ · interrupted` | Failed / denied / run ended first |
| `↳ …` | A tool error, attached under its row (error ink) |
| `│ …` | Tool result output (full mode gutter) |
| `══ assistant ══…` | The answer (markdown; `✦ assistant (update)` mid-run) |
| `↪ steer` | Guidance you sent mid-run |
| `▦` | Generated image (unicode mosaic) |
| `·` | Informational notice |

Three voices, three markings: bare indented prose is the model
thinking, `│` is tool output, `↳` is an error. `/details` (or `Ctrl+D`)
toggles verbosity — folded shows one-line tool calls with status tags
and thinking gists; full shows args, result bodies, and the labeled
reasoning channel. Thinking and every called tool stay visible in BOTH
states.

**`/details full` truncates nothing** (2026-08-20). *Every* body renders
whole: your own prompts and steers, tool arguments, result bodies, tool
errors, thinking and its reasoning channel, error and info notices,
memory-probe digests, and an image's fetch error. No row cap, no
`[#TRUNCATION …]`, no elision of any kind. The client keeps the full
text the ledger reported — nothing is shortened on the way in either —
so the full view has everything to show; use the scrollback (or
`/export`, which writes the uncut arguments in its detailed mode) to
read a long one. A failed tool shows its error AND its output: the
error says that it failed, the body says why.

The FOLDED view is a summary by definition: there, bodies are bounded
and anything cut is labeled (`… (+N more lines)`, `… (+N words of
reasoning · /details)`) — result bodies elide middle-out so their final
lines (the `wc` total, the test verdict) survive, and a tool row stays
one line. Asks TO you are never truncated in either mode, and neither
is the approval modal's `f` JSON view — a question you cannot fully
read is a question you cannot answer.

Entity conversations reuse the same vocabulary. After each entity turn a
`·` line counts what the probe reported (memories in context, diary
entries, tools ran); the full memory digests sit behind the details toggle
(`Ctrl+D`). Tool cards in entity transcripts come from the run's ledger
(`tool_details`), never from reply prose.

## What this client decides, and what it only shows

AbstractCode's TUI is a **thin host**. The gateway is the single place run
semantics live, because the same session is meant to be watched from
AbstractObserver, a web client, or a chat bridge, and all of them must show the
same answer to "did it finish, and what do I do about it?" A host that derives
that answer locally has to be kept in sync with every other host by hand — and
this one already got it wrong once, reading `outcome: "iteration_budget"` alone
while the additive `conclusion_forced` sat beside it, telling operators to
raise a budget that still had 38 of its 50 iterations unspent.

So the loop's terminal node authors the verdict and this client renders it:

| Field on `result.output` | Who writes it | What the host does |
| --- | --- | --- |
| `stop_reason.code` | agent loop | `final_answer`, `iteration_budget`, `stuck_repeat`, `stuck_oscillation` — carried, never interpreted for wording |
| `stop_reason.finished` | agent loop | drives the ⚠/✓ glyph and the `exec` exit code |
| `stop_reason.label` | agent loop | printed verbatim in the fixed chrome (`last run: …`) and as the headless one-liner |
| `stop_reason.headline` + `.remedy` | agent loop | printed verbatim as the conclusion card |
| `stop_reason.budget_exhausted` | agent loop | whether the iterations were actually SPENT — false for a stuck-loop stop; carried so no host re-derives the cause |
| `notices[].{code,severity,text}` | agent loop | `text` printed verbatim; `severity` picks the ink (`error` renders as an error line, anything else as info); `code` is the stable key |

**Legacy engines** (before this contract) send no `stop_reason`. The host then
reports the bare fact — "the agent STOPPED, it did not finish" — and says
explicitly that the engine reported no reason. It does not guess one: an
exhausted budget and a stuck-loop stop both arrive as
`outcome: "iteration_budget"`, and inventing the difference is the bug above.

## Status surfaces

- **Header**: wordmark · workflow · route · entity chips · cockpit facts ·
  session id · connection orb (green ok / red down). With no override the
  route names what "gateway defaults" resolves to — the gateway's
  configured text route, replaced by the model that actually served once
  a run reports it. The facts span carries the workspace directory's
  basename, the workspace access mode, `skills N · mcp N` (when nonzero),
  and the session token total at rest. One chip per entity conversation
  (`◆castor parked`, `◆castor ✎42s` while a turn runs); chips render
  whole or collapse into an honest `+N` tail at narrow widths — the
  **focused** chip always renders (painted first when it would otherwise
  hide), while `Alt+E` keeps cycling in open order regardless of paint
  order; facts paint after chips and clip first.
- **Activity strip** (while running): spinner · current activity · cycle ·
  the in-flight model call (`model call 14s · 41 tok/s (last call)` —
  elapsed from the started record, rate from the previous completed
  call's usage receipt; ≥60s appends "provider may be slow") · elapsed ·
  live token counts · `ctx` (input tokens of the latest model call — the
  live context size) · `cache` (tokens served from the provider cache,
  when reported) · tool count · per-cycle output sparkline. When idle:
  session totals + last context size; a fresh session shows its session
  line instead of a blank row.
- **Status bar**: the persistent instrument row — `ctx used[/window] tk
  ([%,] declared)` (the window comes from `/context`/`--max-tokens`;
  warn ink ≥75%, error ≥90%; no declaration = the honest absolute) ·
  session tokens (`N↑ N↓ tk session` when the provider reports the
  split; the honest `N tk session` total when it doesn't) · `gpu N%`
  (when the `/gpu` meter is on) · `mem N%` (host RAM from the last
  `/resources` fetch — graded on the rounded percent, warn ≥75% / error
  ≥90%; `mem N%*` marks a stale snapshot after a failed refresh; absent
  when no percent is known) · `skills N` · `mcp N` ·
  `? keys + commands`, plus theme + gateway host (+ error detail when
  the connection drops). The key legend lives behind `?` (bare `?` +
  Enter) and `/help`; the phase/focus teachings live in the composer
  placeholder. When the row is too narrow for every instrument,
  segments drop WHOLE from the right — never `…` fragments.
- **Screen recovery**: `Ctrl+L` / `/redraw` force-repaints the whole
  frame — recovery from an external terminal clear (Cmd+K), which a
  damage-tracked renderer cannot detect on its own.

## Caching and context

The gateway enables prompt caching automatically per run when the provider
supports it. Interactive sessions take the server default; there is nothing to
configure. `/cache` is a scrollable panel (`↑↓`/`PgUp`/`PgDn`) reporting three
scopes:

- **route** — the requested route, the model that actually served the run,
  whether that provider/model supports prompt caching (and in which mode), the
  pair the capability probe asked about, and how many calls carried a
  provider-reported hit count.
- **latest model call** — context sent, its new-vs-carried split with
  percentages, cache hits, output tokens, call time and output tok/s.
- **this run** and **session (every run in this conversation)** — model and
  tool calls, token totals, re-send amplification (input sent per output
  token), cache hits against reported input, cumulative new-vs-carried,
  peak context with a count of context resets, and model time with the
  per-call average.

Token counts are exact (`29,200`), not the chrome's rounded `29k`: the panel is
the detail view opened when the rounded number was not enough.

Two kinds of number live there and are never mixed silently. Cache hits are
provider-reported; local providers (e.g. LM Studio) often cache without
reporting hit counts, and the panel says "never reported by this provider"
rather than inventing a zero that would read as a cold cache — once any call
reports a count, a later zero is labeled as a real miss. The new-vs-carried
split is DERIVED client-side from consecutive context sizes (the carried part
is the prefix a cache can serve), and is labeled "derived" everywhere it
appears. A context that shrank is counted as a reset, and credits nothing as
carried — the cached prefix is gone.

### `--no-prompt-cache`

`exec --no-prompt-cache` opts a headless run out of the runtime prompt cache by
sending `_runtime.prompt_cache = false` in the run's start vars. Absent, the run
takes the gateway default (on). It exists so one gateway can serve both sides of
an A/B measurement: the cached and uncached lanes then differ only by that key.

```bash
# uncached lane
abstractcode exec "<prompt>" --session bench-off --workflow react-agent:react \
  --provider mlx --model mlx-community/Qwen3-4B-Instruct-2507-4bit \
  --no-project-context --no-review --permissions read --max-iterations 4 \
  --no-prompt-cache

# cached lane: the same command without --no-prompt-cache
```

**Scope.** The flag reaches the model calls of the run `exec` starts, and only
those.

- **It is `exec`-only.** The interactive client always takes the server default.
- **It does not reach flow-graph bundles.** `coding-agent`, `basic-agent` and
  `multiagent-coding` — including the default workflow — run their agent loop in
  an Agent-node child run. The visual-flow compiler builds that child's
  `_runtime` namespace from an explicit inheritance list: provider, model,
  thinking, audio policy, transcription language, skills block and
  `prompt_cache_binding` are carried across; **`prompt_cache` is not.** The child
  therefore resolves the posture for itself and takes the gateway default,
  whatever the parent was sent. Flow-graph bundles cannot be A/B'd from the
  client today.
- **Use `--workflow react-agent:react` for an A/B.** Its model calls run in the
  run `exec` starts, so the posture governs them.

**Verify the lane from the ledger, never from the flag.** On providers with an
in-process cache, every model call that received a cache key carries
`metadata.prompt_cache`. Absence of that metadata on *every* call is what proves
an uncached lane; presence on any call in a lane you believe is off means the
flag did not reach it. Check before trusting any comparison built on it.

### What to expect when the cache is on

Caching removes prompt prefill, not decoding, so the gain grows with prefix
length and shrinks on short prompts with long answers, and it varies by model
architecture.

Measured through this client over three-turn sessions at ~5.6k-token contexts: a
full-attention 4B served 96% of its prompt tokens from cache after the first
turn, and two hybrid-attention models (4B and 27B) served about 47%, spending one
turn rebuilding where the transcript first grows. Turns after the first were
roughly twice as fast on those models, though the ratio moves by up to ×2 between
repeat runs — treat it as an order of magnitude, not a figure. The first turn is
slightly slower, because it builds the prefix cache the later turns reuse.

GGUF models are the exception to watch: llama.cpp reuses the prefix on its own,
and setting a cache key replaces that reuse rather than adding to it. The
recorded comparison came out slower with the key, but it ran on CPU (Metal
offload is disabled once PyTorch is in the process), so it is not a verdict for a
GPU-offloaded GGUF host. Benchmark your own workload before enabling it there.

See
[AbstractCore's prompt-caching guide](https://github.com/lpalbou/AbstractCore/blob/main/docs/prompt-caching.md)
for the per-model support matrix, the pending measurement list, and the
measurement method.
