# abstractcode-tui

AbstractCode on [AbstractTUI](https://github.com/lpalbou/AbstractTUI): a reactive
terminal client for the [AbstractGateway](https://github.com/lpalbou/abstractgateway)
control plane. The coding agent runs **durably on the gateway**; this binary is the
cockpit — it starts runs, streams their ledgers live, renders reasoning cycles,
tool calls, and answers as they happen, resolves tool-approval and ask-user waits,
steers the agent mid-run, and keeps a durable session with server-side history.

Status: **0.2.0** (the AbstractTUI 0.2.x adoption wave — the engine now owns
the transcript, composer, follow-tail, selection, and picker-activation
machinery this app hand-rolled in 0.1.0).

## What it looks like

```
 ▲ AbstractCode  basic-agent  ·  lmstudio · qwen3.6-35b   acode-93db824cec83 ●
 ❯ you
   Create a snake game in rust, then run the tests.
 ∴ cycle 1
   I will scaffold the crate first…
 » write_file · running   {"file_path":"src/main.rs", …}
 ✓ cargo_build            exit 0 — 0 warnings
 ✦ assistant
   Done — the game builds clean and all 12 tests pass. …
 ⠹ running cargo test  ·  cycle 3  ·  42s  ·  8.1k↑ 612↓ tk   ▂▃▅▂▇
▐ describe a task, steer a running one — /help · Alt+Enter newline        ▌
 enter send/steer  esc esc cancel  pgup/dn scroll  ctrl+t theme    nord · …
```

- **Live activity**: reasoning cycles, tool cards that update in place
  (awaiting approval → running → ✓/✗), status lines, token counts, and a
  per-cycle output sparkline — streamed over SSE from the run ledger into
  the engine's keyed `Feed` widget (windowed paint, follow-tail pinning
  that disengages while you read scrollback and re-pins at the bottom).
- **A real composer**: multiline (grows 1..4 rows), `Alt+Enter` newline,
  multi-line paste inserts whole, `↑`/`↓` recall sent messages, and a `/`
  command completion dropdown at the caret (Tab/Enter accept — a
  fully-typed command always submits on the first Enter).
- **Select and copy in-app**: drag to select rendered text; releasing
  copies via OSC 52 (Shift/Option-drag still reaches the terminal's
  native selection).
- **Approvals**: mutating tools pause the run durably; a focus-trapped modal
  shows the exact calls + arguments; `a` approves, `d` denies, `A` approves
  all (auto-approve for the session — `/auto` toggles it back off; never
  persisted).
- **Steering**: type while the agent works — Enter folds your guidance into
  its next reasoning cycle (the gateway's durable steer sidecar).
- **Sessions and memory**: one durable session id per conversation, and a
  `/sessions` picker over your recent ones (named by their first prompt).
  The client carries the live conversation into each run; the gateway
  replays prior turns server-side across restarts (`use_session_history`).
- **Crash-proof by construction**: quit or crash, relaunch, and you are
  back where you were — prior turns replay in full detail from their run
  ledgers (cycles, tool cards, answers), a live run reattaches with its
  prompt and full activity, pending approvals re-surface. `/pause` parks
  the run durably on the gateway (it survives quitting); `/resume`
  continues it.
- **Capability control**: `/tools` switches gateway tools on/off per run
  (checked set = the run's exact allowlist), `/skills` attaches gateway
  skills, `/mcp` shows the gateway's MCP server registry.
- **Honest telemetry**: the header names what "gateway defaults" actually
  resolves to (and the model that served the last call); the activity strip
  shows live context size (`ctx`) and cache hits; `/cache` reports the
  prompt-cache posture for the effective route (auto = on when available).
- **Detail on demand**: `Ctrl+D` (or `/details`) toggles between the full
  live view (reasoning cycles, tool cards, results) and a clean answers-only
  view — finished tool cards fold away; active, failed, and denied tools
  plus errors always stay visible.
- **26 themes** with a live-preview picker (`/theme`, last choice saved),
  markdown answers, inline images (generated artifacts render as unicode
  mosaic), and a zero-cost idle footprint — all from AbstractTUI.

## Install

```sh
cargo install abstractcode-tui
```

Rust 2021 (MSRV 1.87, inherited from AbstractTUI); macOS and Linux are the
live-verified platforms.
Windows is unverified for this crate (AbstractTUI itself compiles there, but
this client's TLS stack has not been built or run on Windows yet).

## Quickstart

You need a running AbstractGateway (the control plane that hosts the agent):

```sh
abstractgateway serve                                  # or use an existing one
abstractcode-tui login --gateway http://127.0.0.1:8080 --token <token>
abstractcode-tui doctor                                # reachability · auth · catalog
abstractcode-tui                                       # launch the TUI
```

`login` verifies against the gateway before saving (flags/env only — it never
prompts); the store is shared with the Python CLI.

The login store is **shared with the Python `abstractcode` CLI**
(`~/.abstractcode/gateway.json`) — log in once with either client.

Inside the app:

- type a task and press Enter — the agent workflow runs on the gateway
- `/workflow` picks the agent (any catalog entrypoint implementing
  `abstractcode.agent.v1`), `/model` picks provider + model, `/theme` restyles
- type while a run is active to steer it; `Esc Esc` cancels; `/new` starts a
  fresh session
- tool approvals and agent questions open as modals; the run waits durably
  (server-side) until you answer — even across client restarts

## Headless one-shots

```sh
abstractcode-tui exec "Summarize the workspace layout" \
  --provider lmstudio --model qwen/qwen3.6-35b-a3b --approve-all
```

Prints transcript events as they fold; exit codes: 0 completed answer,
1 failure, 2 usage/config error, 124 timeout, 130 cancelled.
`--approve-all` approves tool batches;
without it, mutating tools are denied with an explanation the model sees.
Ask-user waits get an honest "no interactive user" refusal so unattended
runs never stall.

## Options

```
--gateway <URL> --token <TOK>     connection (flag > env > login store)
--session <ID>                    durable session id (default: last used)
--workflow <bundle[:flow]>        agent workflow (default: basic-agent)
--provider <P> --model <M>        route override (default: gateway defaults)
--workspace <PATH>                requested workspace root (see note)
--theme <ID>                      start theme (ABSTRACTTUI_THEME works too)
--caps                            print the terminal capability report
```

**Workspace note**: the gateway's server-managed workspace policy (the default
posture) clamps client-provided paths — tools then execute in the gateway's
workspace root or a managed per-session folder, and the app tells you so at
startup. Set `ABSTRACTGATEWAY_ALLOW_CLIENT_WORKSPACE_SCOPE=1` on the gateway
to honor client workspace roots (trusted/local setups).

## How it relates to `abstractcode` (Python)

The Python [`abstractcode`](https://github.com/lpalbou/abstractcode) runs the
agent loop **in-process** (AbstractAgent + AbstractRuntime + AbstractCore) with
the gateway as its control plane. This port is a **thin client**: the agent
executes on the gateway and the TUI renders its durable run ledger. Same
workflows, same approvals, same steering — different execution home. Use the
Python CLI when you want local execution and local tools; use this when the
gateway is the execution home (shared runs, one durable transcript, attach
from anywhere).

## Documentation

- [docs/getting-started.md](docs/getting-started.md) — install to first run
- [docs/architecture.md](docs/architecture.md) — threading model, the ledger
  fold, gateway contract
- [docs/api.md](docs/api.md) — CLI options, slash commands, key bindings
- [docs/faq.md](docs/faq.md) and [docs/troubleshooting.md](docs/troubleshooting.md)
- Agent-oriented: [llms.txt](llms.txt)

## Development

```sh
cargo test          # unit + headless UI (real pipeline, no pty) + replay
cargo clippy --all-targets
ACODE_GATEWAY_TOKEN=… python3 scripts/pty_live_smoke.py   # live E2E
```

## License

MIT. See [LICENSE](LICENSE).
