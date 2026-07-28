# Reference: CLI, commands, keys

## CLI

```
abstractcode-tui [OPTIONS]                    launch the TUI
abstractcode-tui exec "<prompt>" [OPTIONS]    headless one-shot run
abstractcode-tui login [OPTIONS]              verify + persist credentials
abstractcode-tui doctor [OPTIONS]             diagnose the connection
abstractcode-tui --caps                       terminal capability report
abstractcode-tui --help | --version
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
| `--max-iterations <N>` | Agent iteration budget | 50 |
| `--replay-turns <N>` | Prior turns replayed in full detail at boot (0 disables) | 20 |
| `--permissions <level>` | tool permissions for the invocation: `read` \| `write` \| `all` | prefs level |
| `--require-approval <t>` | gate these tools regardless of level (comma list, repeatable); exec denies them | — |
| `--timeout <SECS>` | exec: overall deadline | 900 |

### Environment

| Variable | Meaning |
| --- | --- |
| `ABSTRACTCODE_GATEWAY_URL` / `ABSTRACTFLOW_GATEWAY_URL` / `ABSTRACTGATEWAY_URL` | Gateway URL (first set wins; beats the login store) |
| `ABSTRACTCODE_GATEWAY_TOKEN` / `ABSTRACTGATEWAY_AUTH_TOKEN` / `ABSTRACTFLOW_GATEWAY_AUTH_TOKEN` | Bearer token |
| `ABSTRACTCODE_GATEWAY_CONNECTION_FILE` | Login store path (default `~/.abstractcode/gateway.json`) |
| `ABSTRACTCODE_TUI_PREFS_FILE` | Preferences path (default `~/.abstractcode-tui/prefs.json`) |
| `ABSTRACTTUI_THEME` | Start theme id |

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
| `/cache` | Prompt-cache + context status for the effective route |
| `/history [n\|all]` | Stream the PREVIOUS bloc of this session's turns from the gateway ledgers, prepended in full detail. Boot replays only the last bloc (`--replay-turns` sizes it, default 5); a stub line names how many earlier turns exist. **Scrolling to the top of the transcript auto-loads the previous bloc** — the stub becomes a live progress line and holding at the top cascades bloc-by-bloc until the session is fully loaded (Esc returns to the tail and stops the cascade). Failures name their cause — never a silent hole |
| `/status` | The status card: workflow, route, workspace, session, connection, client phase + run id + last outcome, and a LIVE gateway run-status probe — the one place client view vs server truth is inspectable (wrapper roots legitimately stay `waiting` after your turn concluded) |
| `/sessions` | Pick a recent session to continue (named by first prompt) |
| `/session [id]` | Show or switch the session id (switching probes for a live run to reattach) |
| `/details [full\|fold]` | Toggle the clean answers view (`Ctrl+D`): reasoning folds away and tool cards drop their DETAIL (argument line + result body) — the tool CALL stays as a one-line header, so the trace is never hidden; active, failed, and denied tools plus errors always keep their bodies. Thinking cards render as one-line gists by default — `/details full` expands them (content plus the labeled reasoning channel), `/details fold` returns to gists |
| `/gating [auto\|wait]` | Approval gating for gating-capable workflows (the multi-agent coder): `auto` runs unattended (skips the workflow's human-approval pauses), `wait` re-gates (the default). Selecting the coder also opens a gated/unattended choice. Rides `input_data.gating_mode`; tool approval is a SEPARATE axis (`/permissions`) |
| `/reasoning [level]` | Reasoning effort for the current route (`none\|minimal\|low\|medium\|high\|xhigh\|auto`; `default` clears). Bare `/reasoning` opens the dial — also stage 3 of `/model`: pick provider, then model, then effort. Non-reasoning models show a locked `none` (set-anyway override available while capability provenance is unserved); the choice is pair-coupled — changing provider or model resets it. Rides the run as `_runtime.thinking` (absent = gateway default) |
| `/export [md\|jsonl] [--details] [path]` | Export the agent transcript to a file: `md` (default) = archival markdown; `jsonl` = SFT training lines. `--details` adds reasoning + full tool cards. Bare `/export` auto-names in the cwd; never overwrites — see "Transcript export" below |
| `/attach [path\|clear]` | Stage a file for your NEXT message (chips above the composer; uploaded at send as `context.attachments`; **session uploads are permanent** server-side). Accepts `~`, quotes, escaped spaces, `file://`, relative paths. Bare `/attach` opens the file browser (nothing staged) or the pending manager (chips staged); `clear` discards. Dropping a file onto the terminal attaches directly — `Ctrl+O` undoes (chips out, path text back). Agent-lane only (v1) — see "Attachments" below |
| `/auto` | Removed (the session blanket had latent holes) — opens the `/permissions` report teaching the replacement |
| `/pause` | Pause the run tree durably on the gateway (stops at the next step boundary; survives quitting the client) |
| `/resume` | Resume a paused run tree |
| `/cancel` | Cancel the active run |
| `/steer <text>` | Explicit steering (plain Enter during a run steers too; buffered until the run's first reasoning cycle when it has not started cycling yet) |
| `/queue [text]` | Queue a prompt (FIFO): auto-runs after the current run **succeeds**; halts on failure/cancel (explicit resume). Persists per session and restores **paused** — a restore never auto-starts. Bare `/queue` opens the manager (keys under "Modal keys" below). Agent-lane only: under entity focus the visit's held-draft lane is the queue |
| `/goal [text\|stop]` | Start a goal run on a goal workflow (`abstractcode.goal.v1`): loops until verified done or `max_cycles` (prefs `goal_max_cycles`, default 8). Bare `/goal` shows status; `/goal stop` cancels durably. Ships dark until a goal bundle is published on the gateway |
| `/context [n\|off]` | Declare the model's context window in tokens (`262144`, `262k`, `1m`) — the footer meter becomes `ctx used/window tk (%, declared)`, warn ≥75% / error ≥90%, and the declaration rides runs as `_limits.max_tokens`. Bare `/context` reports; `off` clears. Persisted (`context_window`); `--max-tokens` declares for one session. Source-labeled "declared" — never a client capability table |
| `/redraw` | Force a full-screen repaint (`Ctrl+L`) — recovery from an external terminal clear (Cmd+K), which damage-tracked rendering cannot detect |
| `@name [text]` | Talk with a summoned entity: bare `@name` opens (or focuses) a durable visit; `@name <text>` opens and sends the first turn. An unknown name never becomes an agent prompt — the draft is preserved with a roster hint. A partial `@na` opens a completion dropdown (cached roster); accepting inserts the name and the NEXT Enter submits |
| `/entities [name]` | Entity roster + identity cards (opens instantly on the cached roster, refreshes async — the live fetch can be slow behind the gateway's drives fold). `[name]` deep-links to that card |
| `/brain <name>` | FLOW-BRAIN conversation with an entity: each message is one summon of the `entity-chat` VisualFlow through the entity door (poll to terminal; the structured `degraded`/`moment_error` contract renders as warn lines). Continuity rides the entity's own memory graph under one client-minted session id; the view is session-local, and `/end` closes it locally (no server visit exists). One conversation per entity: a live `@name` visit refuses with "/end first"; bare `/brain` reports the focused conversation's brain |
| `/task <name> <title>` | Leave a task on the entity's desk — durable, no visit needed, works while the entity sleeps. Pickup happens at the entity's own boundary (day end, wake check, or visit close), never "immediately" |
| `/end [name] [reason]` | Close the visit (the entity's reflection runs server-side; a visit that woke a sleeping entity restores the sleep). Refused mid-turn — entity turns are non-interruptible |
| `/focus <name\|agent>` | Switch conversation focus (`Ctrl+E` cycles) |
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
  turn's tools/cycles/steers as preview-bounded strings — deliberately
  not fabricated `tool_calls`: the client holds display previews, not
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
| `Ctrl+E` | anywhere | Cycle conversation focus: agent → entity visit 1 → … → agent. Cycle order is the order conversations were opened — it never changes with how the header paints chips |
| `Ctrl+T` | anywhere | Cycle theme |
| `Ctrl+L` | anywhere | Force a full-screen repaint (`/redraw`) — recovers from a terminal clear |
| `Ctrl+O` | while a drop's chips are still pending | Undo the newest file drop: chips out, the pasted path text back in the composer. Expires once the chips ride a run or are removed |
| `?` + Enter | empty composer | Open the keys + commands reference (the footer's `? keys + commands`) |
| `Ctrl+Q` | anywhere | Quit |
| `Ctrl+C` | anywhere | Clear the current prompt (if any) and arm quit — a second Ctrl+C within 2s quits (with a live run, the quit gate opens; a third press = leave & quit) |
| `↑↓` + `Enter`/`Space` | pickers | Move + choose — Space confirms too, single-select semantics (theme picker previews live) |
| `Tab` | anywhere | Move focus (composer ↔ transcript ↔ modal fields) |

### Modal keys

| Modal | Keys |
| --- | --- |
| approval | `a` approve · `A` approve all (sets permissions: `all`, sticky per session) · `d` deny · `f` toggle full JSON of the calls · `Esc` defer (the run keeps waiting; Enter on the empty composer reopens) |
| ask (agent question) | `Enter` send the answer · `Esc` keeps the run waiting |
| `/tools` | `Space` toggle · `a` all on · `n` all off · `p` cycle the per-tool pin (none → auto → ask → none; a pin beats the tier both ways) · `t` cycle the approval tier (read → write → all, persisted) · `Enter`/`Esc` close |
| `/queue` | `↑↓` select · `x` remove · `u`/`d` move up/down · `c` clear all · `r` resume a paused queue · `e` pop the prompt into the composer for editing · `Enter`/`Esc` close |
| `/attach` manager | `↑↓` select · `x` remove · `c` clear · `b` browse (file picker) · `Enter`/`Esc` close |
| `/attach` picker | type to filter · `↑↓` move · `Enter` descend into a folder / attach the file (marked set when non-empty) · `Space` mark files for multi-attach · `Backspace`/`←` parent folder (filter empty) · `Esc` close |
| `/entities` | `↑↓` browse (the identity card follows) · `Enter` talk (`@name`) · `t` leave a task (title prompt) · `e` end that entity's open visit · `Ctrl+D` show per-section provenance · `Esc` close |
| `/workspace` | `↑↓` move · `Space` select an access mode / remove an allowed path · type a path + `Enter` adds it (switches to `workspace_or_allowed` when needed) · `Esc` close |

## Transcript vocabulary

| Glyph | Item |
| --- | --- |
| `❯ you` | Your task |
| `∴ cycle N` | One reasoning cycle's model output (dim) |
| `? name · awaiting approval` | Tool paused on approval |
| `» name · running` | Tool executing on the gateway |
| `✓` / `✗` / `⊘` | Tool succeeded / failed / denied |
| `✦ assistant` | Answer (markdown; `(update)` for mid-run messages) |
| `↪ steer` | Guidance you sent mid-run |
| `▦` | Generated image (unicode mosaic) |
| `·` | Informational notice |

Previews are bounded; anything cut is labeled (`… (+N more lines …)` or
`[#TRUNCATION …]`). Toggle `/details full` to expand in place, or
`/export` to write the full transcript to a file. (Asks TO you are never
truncated — a question you cannot fully read is a question you cannot
answer.)

Entity conversations reuse the same vocabulary. After each entity turn a
`·` line counts what the probe reported (memories in context, diary
entries, tools ran); the full memory digests sit behind the details toggle
(`Ctrl+D`). Tool cards in entity transcripts come from the run's ledger
(`tool_details`), never from reply prose.

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
  hide), while `Ctrl+E` keeps cycling in open order regardless of paint
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
  (when the `/gpu` meter is on) · `skills N` · `mcp N` ·
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
supports it (auto = on when available; nothing to configure client-side).
`/cache` reports: the effective route, whether that provider/model supports
prompt caching (and in which mode), cache hits observed this run, and the
context size of the latest model call. Local providers (e.g. LM Studio)
often cache without reporting hit counts — the panel says so rather than
inventing zeros.
