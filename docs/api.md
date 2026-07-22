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
| `--session <ID>` | Durable session id | last used, else minted (`acode-<hex>`) |
| `--workflow <bundle[:flow]>` | Agent workflow | saved pick, else `basic-agent` |
| `--provider <NAME>` | Provider override | gateway defaults |
| `--model <NAME>` | Model override | gateway defaults |
| `--workspace <PATH>` | Requested workspace root | current directory |
| `--no-workspace` | Send no workspace root | — |
| `--workspace-mode <M>` | Workspace access mode | server default |
| `--theme <ID>` | Start theme | `ABSTRACTTUI_THEME`, else saved pick |
| `--max-iterations <N>` | Agent iteration budget | 50 |
| `--replay-turns <N>` | Prior turns replayed in full detail at boot (0 disables) | 20 |
| `--approve-all` | exec: auto-approve tool batches | deny |
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
| `/tools` | Enable/disable gateway tools (`Space` toggles; checked set = the run's exact allowlist; untouched = workflow defaults) |
| `/skills` | Attach gateway skills to your runs (`Space` toggles; sent as `input_data.skills`) |
| `/mcp` | The gateway's MCP server registry (read-only; their tools appear in `/tools` once declared) |
| `/cache` | Prompt-cache + context status for the effective route |
| `/sessions` | Pick a recent session to continue (named by first prompt) |
| `/session [id]` | Show or switch the session id (switching probes for a live run to reattach) |
| `/details` | Toggle the clean answers view (`Ctrl+D`): reasoning and finished tool cards fold away; active, failed, and denied tools plus errors always stay visible |
| `/auto` | Toggle auto-approve of tool batches (session-scoped, never persisted; also armed by `A`/"approve all" in the approval modal) |
| `/pause` | Pause the run tree durably on the gateway (stops at the next step boundary; survives quitting the client) |
| `/resume` | Resume a paused run tree |
| `/cancel` | Cancel the active run |
| `/steer <text>` | Explicit steering (plain Enter during a run steers too) |
| `/quit` | Exit |

Anything that is not a command is a task (when idle) or steering guidance
(while a run is active).

## Keys

| Key | Context | Effect |
| --- | --- | --- |
| `Enter` | composer | Send task / send steering |
| `Alt+Enter` | composer | Insert a newline (Shift+Enter too on kitty terminals); the composer grows to 4 rows |
| `↑` / `↓` | composer at buffer edge | Recall sent messages (input history) |
| `Tab` / `Enter` | `/` completion dropdown | Accept the highlighted command (Esc dismisses; a fully-typed command submits directly) |
| `Esc` | composer with text | Clear the composer |
| `Esc Esc` | while running | Cancel the run (within 900ms) |
| `PgUp` / `PgDn` | anywhere | Scroll the transcript (PgDn to the tail re-sticks) |
| mouse wheel | transcript | Scroll (unsticks from the tail) |
| `Ctrl+D` | anywhere | Toggle detail view (thinking + tool results vs answers only) |
| `Ctrl+T` | anywhere | Cycle theme |
| `Ctrl+Q` / `Ctrl+C` | anywhere | Quit |
| `a` / `A` / `d` / `Esc` | approval modal | Approve / approve all (auto-approve this session) / deny / defer (Enter on the empty composer reopens) |
| `Enter` | ask modal | Send the answer (Esc keeps the run waiting) |
| `↑↓` + `Enter` | pickers | Move + choose (theme picker previews live) |
| `Space` | `/tools`, `/skills` | Toggle the highlighted entry (`a` all on / `n` all off in `/tools`) |
| `Tab` | anywhere | Move focus (composer ↔ transcript ↔ modal fields) |

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
`[#TRUNCATION …]`) — the full text always lives in the gateway run ledger.

## Status surfaces

- **Header**: wordmark · workflow · route · session id · connection orb
  (green ok / red down). With no override the route names what "gateway
  defaults" resolves to — the gateway's configured text route, replaced by
  the model that actually served once a run reports it.
- **Activity strip** (while running): spinner · current activity · cycle ·
  elapsed · live token counts · `ctx` (input tokens of the latest model
  call — the live context size) · `cache` (tokens served from the provider
  cache, when reported) · tool count · per-cycle output sparkline. When
  idle: session totals + last context size.
- **Status bar**: key legend · theme · gateway host (+ error detail when the
  connection drops).

## Caching and context

The gateway enables prompt caching automatically per run when the provider
supports it (auto = on when available; nothing to configure client-side).
`/cache` reports: the effective route, whether that provider/model supports
prompt caching (and in which mode), cache hits observed this run, and the
context size of the latest model call. Local providers (e.g. LM Studio)
often cache without reporting hit counts — the panel says so rather than
inventing zeros.
