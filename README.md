# abstractcode-tui

AbstractCode on [AbstractTUI](https://github.com/lpalbou/AbstractTUI): a reactive
terminal client for the [AbstractGateway](https://github.com/lpalbou/abstractgateway)
control plane. The coding agent runs **durably on the gateway**; this binary is the
cockpit — it starts runs, streams their ledgers live, renders reasoning cycles,
tool calls, and answers as they happen, resolves tool-approval and ask-user waits,
steers the agent mid-run, and keeps a durable session with server-side history.

Status: **0.4.0** (the conclusion + presence wave: the "run never
finishes" P0 class fixed at its roots, header/footer instrumentation
with a declared context meter, `/gpu`, and `Ctrl+L`/`/redraw` screen
recovery — on top of 0.3.0's control-and-collaboration wave: prompt
queue, goal loops, summoned-entity conversations, tiered tool
approvals, workspace scope).

## What it looks like

```
 ▲ AbstractCode  basic-agent · lmstudio · qwen3.6-35b · ⌂ snake · skills 2   acode-93db824cec83 ●
 ❯ you
   Create a snake game in rust, then run the tests.
 ∴ cycle 1
   I will scaffold the crate first…
 » write_file · running   {"file_path":"src/main.rs", …}
 ✓ cargo_build            exit 0 — 0 warnings
 ✦ assistant
   Done — the game builds clean and all 12 tests pass. …
 ⠹ cycle 3 · model call 4s · 39 tok/s (last call) · 42s · 8.1k↑ 612↓ tk   ▂▃▅▂▇
❯ ▐ describe a task — Enter sends · Ctrl+J newline · /help                    ▌
 ctx 8.1k/262k tk (3%, declared) · 12k tk session · skills 2 · ? keys   nord · …
```

- **Live activity**: reasoning cycles, tool cards that update in place
  (awaiting approval → running → ✓/✗), status lines, token counts, and a
  per-cycle output sparkline — streamed over SSE from the run ledger into
  the engine's keyed `Feed` widget (windowed paint, follow-tail pinning
  that disengages while you read scrollback and re-pins at the bottom).
- **A real composer**: multiline (grows 1..4 rows), `Ctrl+J` newline in
  every terminal (`Alt+Enter` too; `Shift+Enter` wherever the kitty
  keyboard protocol is live — kitty/Ghostty/foot from startup, iTerm2
  ≥ 3.5, VS Code/Cursor and Warp via the mid-session probe — and the
  composer hint names the best chord for YOUR terminal), multi-line
  paste inserts whole, `↑`/`↓` recall sent messages, and a `/` command
  completion dropdown at the caret (Tab/Enter accept — a fully-typed
  command always submits on the first Enter).
- **Select and copy in-app**: drag to select rendered text; releasing
  copies via OSC 52 (Shift/Option-drag still reaches the terminal's
  native selection).
- **Approvals**: mutating tools pause the run durably; a focus-trapped modal
  shows the exact calls + arguments; `a` approves, `d` denies, `A` approves
  the batch AND sets permissions to `all` (sticky per session —
  `/permissions read` restores prompting).
- **Steering**: type while the agent works — Enter folds your guidance into
  its next reasoning cycle (the gateway's durable steer sidecar). Guidance
  sent before the run starts cycling is buffered and delivered on the
  first cycle, never dropped.
- **A prompt queue**: `/queue <text>` lines up the next task (FIFO); each
  queued prompt runs as its own turn after the current run succeeds, with
  the finished answer in its context. Failure or cancel pauses the queue;
  it persists per session and always restores paused (`/queue` manages).
- **Goal loops**: `/goal <text>` starts a self-verifying run that keeps
  cycling until its own checks pass or the cycle budget runs out; `/goal`
  shows status, `/goal stop` cancels durably. Requires a goal workflow
  (`abstractcode.goal.v1`) published on the gateway — without one, `/goal`
  says so and starts nothing.
- **Summoned entities**: `@name` opens a durable visit with a gateway
  entity (`/entities` lists the roster with identity cards); each
  conversation gets its own transcript and header chip, and `Ctrl+E`
  cycles focus between the agent and open visits. Entity turns are
  non-interruptible and stream no mid-turn progress (a server-side
  boundary, rendered honestly) — typing during a turn holds one draft and
  auto-sends it when the turn parks. `/task` leaves work on an entity's
  desk without a visit; `/end` closes the visit (its reflection runs
  server-side).
- **Permissions**: `/permissions read|write|all` sets the persisted
  auto-approval level — proven read-only tools, then workspace file
  writes, then everything. Per-tool pins (`p` in `/tools`) override the
  level in both directions, and gateway-disabled tools never run. `all`
  auto-approves arbitrary shell commands and network egress: a
  deliberate, eyes-open choice, never the default. Sticky per session.
- **Workspace scope**: `/workspace` shows where the agent's tools may
  touch the filesystem (root, access mode, allowed paths) and extends it —
  the fix for red "Path escapes workspace_root" refusals.
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
  resolves to (and the model that served the last call) plus the working
  directory, workspace mode, and capability counts; the footer is a
  persistent instrument row (context meter, session tokens, `/gpu` meter,
  skills/MCP counts — the key legend lives behind `?`); the activity
  strip shows live context size (`ctx`), cache hits, and the in-flight
  model call with its last-call tok/s; `/cache` reports the prompt-cache
  posture for the effective route (auto = on when available).
- **A context meter you declare**: `/context 262k` declares the model's
  window (persisted; `--max-tokens` for one session) and the footer
  reads `ctx 41k/262k tk (15%, declared)` — warn at 75%, error at 90%.
  No declaration = the honest absolute; the label always names the
  source (the client never ships a fabricated capability table). The
  declaration also rides runs as `_limits.max_tokens`.
- **Screen recovery**: `Ctrl+L` (or `/redraw`) force-repaints the whole
  frame — the recovery from a terminal clear (Cmd+K) that a damage-
  tracked renderer cannot otherwise see — and an externally cleared
  screen also heals itself at the next focus round-trip (engine
  redraw-on-focus-gained, on by default here).
- **Detail on demand**: `Ctrl+D` (or `/details`) toggles between the full
  live view (reasoning cycles, tool cards, results) and a clean answers-only
  view — finished tool cards fold away; active, failed, and denied tools
  plus errors always stay visible.
- **Transcript export**: `/export` writes the conversation to a file —
  readable archival markdown as shown, or SFT-ready JSONL (OpenAI chat
  schema, one line per completed turn); `--details` adds reasoning + full
  tool cards. Never overwrites.
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
- `@name` talks with a summoned entity when the gateway hosts them
  (`/entities` lists the roster); `Ctrl+E` cycles conversation focus

## Headless one-shots

```sh
abstractcode-tui exec "Summarize the workspace layout" \
  --provider lmstudio --model qwen/qwen3.6-35b-a3b --permissions all
```

Prints transcript events as they fold; exit codes: 0 completed answer,
1 failure, 2 usage/config error, 124 timeout, 130 cancelled.
`--permissions <read|write|all>` sets the tool level for the invocation
(`--require-approval <names>` adds per-tool gates that deny headlessly);
without a raised level, mutating tools are denied with an explanation the
model sees.
Ask-user waits get an honest "no interactive user" refusal so unattended
runs never stall.

## Options

```
--gateway <URL> --token <TOK>     connection (flag > env > login store)
--session <ID>                    durable session id (default: fresh session)
--resume                          reopen the last session (`--continue` alias)
--workflow <bundle[:flow]>        agent workflow (default: saved or basic-agent)
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

## Benchmark smoke mode

`scripts/zelda_headless_bench.py` includes a smoke mode for checking the real
selected benchmark client without running the full Zelda prompt. Set
`ZELDA_BENCH_SMOKE` to an exact-answer prompt:

```sh
ZELDA_BENCH_SMOKE='Reply with exactly: smoke-ok' \
ZELDA_BENCH_TIMEOUT_S=120 ZELDA_BENCH_MAX_ITER=3 \
python3 scripts/zelda_headless_bench.py code-1
```

On success, stdout is exactly:

```text
smoke-ok
```

The smoke path still launches the selected child (`abstractcode` for `code-1`
or the release `abstractcode-tui` binary for `code-tui-1`). It exits 0 and
prints the requested answer only when every selected run exits successfully
and its captured final answer contains that value; otherwise it exits nonzero
without printing a success value. As in full benchmark mode, run logs and
reports are written under `untracked/zelda-bench/`.

Controls:

- Select lanes with `code-1`, `code-tui-1`, `code-2`, or `code-tui-2`; omit
  lane arguments to run the four-step matrix.
- `ZELDA_BENCH_CODE_LOOP` and `ZELDA_BENCH_CODE_AGENT` select the local
  `abstractcode` loop and agent.
- `ZELDA_BENCH_TUI_LOOP` selects a built-in TUI workflow mapping (`basic`,
  `react`, `codeact`, `memact`, or `multi-coder`), while
  `ZELDA_BENCH_TUI_WORKFLOW` overrides it with an explicit workflow reference.
- `ZELDA_BENCH_PROVIDER`, `ZELDA_BENCH_MODEL`, `ZELDA_BENCH_BASE_URL`,
  `ZELDA_BENCH_REASONING`, `ZELDA_BENCH_MAX_ITER`, and
  `ZELDA_BENCH_TIMEOUT_S` configure the child run.
- TUI benchmark lanes require `target/release/abstractcode-tui`; build it with
  `cargo build --release`. Gateway credentials come from
  `~/.abstractcode/gateway.json`, falling back to `ABSTRACTGATEWAY_URL` and
  `ABSTRACTGATEWAY_AUTH_TOKEN`.

## Repository structure

- `src/` — Rust CLI, gateway client, durable-run fold, policy, storage, export,
  and TUI modules.
- `tests/` — integration, replay, policy, and headless UI coverage.
- `scripts/` — live checks and benchmark drivers, including
  `zelda_headless_bench.py`.
- `docs/` — getting started, command/key reference, architecture,
  troubleshooting, design notes, and reports.
- `untracked/` — generated benchmark artifacts and logs; not application
  source.

## Development

```sh
cargo test          # unit + headless UI (real pipeline, no pty) + replay
cargo clippy --all-targets
cargo build --release                              # required by TUI benchmark lanes
ACODE_GATEWAY_TOKEN=… python3 scripts/pty_live_smoke.py   # live E2E
```

## License

MIT. See [LICENSE](LICENSE).
