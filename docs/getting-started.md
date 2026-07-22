# Getting started

From install to your first gateway-hosted agent run.

## Prerequisites

- A running [AbstractGateway](https://github.com/lpalbou/abstractgateway) —
  the control plane that hosts the agent workflows, executes tools, and
  stores durable runs. Locally: `abstractgateway serve` (defaults to
  `http://127.0.0.1:8080`).
- At least one LLM provider the gateway can reach (LM Studio, Ollama, or a
  configured endpoint profile), and at least one catalog workflow
  implementing the `abstractcode.agent.v1` interface — the stock
  `basic-agent` bundle ships with the gateway.
- Rust (2021 edition toolchain) if installing from source.

## Install

```sh
cargo install abstractcode-tui
```

## Connect once

```sh
abstractcode-tui login --gateway http://127.0.0.1:8080 --token <your-token>
```

`login` takes the URL and token from flags or environment (it never prompts),
verifies them against `GET /api/gateway/ping`, and only then saves them to
`~/.abstractcode/gateway.json` (mode 0600 on unix). That store is shared with
the Python `abstractcode` CLI — logging in with either client covers both. Environment variables (`ABSTRACTCODE_GATEWAY_URL`,
`ABSTRACTCODE_GATEWAY_TOKEN`, or the `ABSTRACTGATEWAY_*` equivalents)
override the store when set.

Check the connection end to end:

```sh
abstractcode-tui doctor
# [1/3] reachability   ✓ server up (auth enforced)
# [2/3] authentication ✓ ping ok
# [3/3] catalog        ✓ 10 agent workflow(s): basic-agent:81795ea9, …
# Result: HEALTHY
```

`doctor` prints WHICH source provided the URL and token (flag, env, or the
login store) — the antidote to a stale export silently pointing you at the
wrong gateway.

## First run

```sh
abstractcode-tui
```

Type a task and press Enter:

> Create a file named hello.txt containing "hello", then reply DONE.

You will see, live: the agent's reasoning cycles (dim `∴` blocks), tool
cards updating in place (`»` running → `✓`/`✗`), and the final answer
rendered as markdown. When the agent wants to run a mutating tool
(`write_file` here), a **tool approval modal** opens with the exact call and
arguments — press `a` to approve, `d` to deny. The run waits durably on the
gateway until you answer.

## The five things worth knowing on day one

1. **Type while it works to steer.** Anything you submit during a run is
   folded into the agent's next reasoning cycle as guidance. `Esc Esc`
   cancels the run.
2. **Sessions are durable and server-side.** Each conversation has a session
   id (shown in the header); the gateway replays prior turns into new runs.
   `/new` starts a fresh session; `/session <id>` switches. If you relaunch
   while a run is still active, the app reattaches to it automatically.
3. **Pick your agent and route.** `/workflow` lists every agent workflow on
   the gateway; `/model` lists providers and models (leave it on "gateway
   default" to use the server's routing). Both choices persist across
   launches.
4. **Where files land.** Under the gateway's default (server-managed)
   workspace policy, tools execute in the gateway's workspace root or a
   managed per-session folder — the app tells you this at startup. To make
   the gateway honor client workspace paths (`--workspace`), set
   `ABSTRACTGATEWAY_ALLOW_CLIENT_WORKSPACE_SCOPE=1` on the gateway
   (trusted/local setups).
5. **Themes.** `/theme` opens a live-preview picker over the 26 built-in
   AbstractTUI themes; `Ctrl+T` cycles. Your pick persists.

## Headless one-shots

For scripts and CI:

```sh
abstractcode-tui exec "List the files in the workspace and summarize them" \
  --approve-all --timeout 300
```

Events print as they fold; the final answer prints under an `answer` rule.
Exit codes: 0 completed, 1 failed, 124 timeout, 130 cancelled. Without
`--approve-all`, mutating tools are denied with an explanation the agent
sees (it will finish as best it can without them). Ask-user waits receive an
honest "no interactive user is present" response.

## Where to next

- [architecture.md](architecture.md) — how the client works (threading, the
  ledger fold, the gateway contract)
- [api.md](api.md) — every command, key binding, and CLI option
- [faq.md](faq.md) · [troubleshooting.md](troubleshooting.md)
