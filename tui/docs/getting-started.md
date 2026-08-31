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
cargo install abstractcode
```

## Connect once

```sh
abstractcode login --gateway http://127.0.0.1:8080 --token <your-token>
```

`login` takes the URL and token from flags or environment (it never prompts),
verifies them against `GET /api/gateway/ping`, and only then saves them to
`~/.abstractcode/gateway.json` (mode 0600 on unix). That store is shared with
the Python `abstractcode` CLI — logging in with either client covers both. Environment variables (`ABSTRACTCODE_GATEWAY_URL`,
`ABSTRACTCODE_GATEWAY_TOKEN`, or the `ABSTRACTGATEWAY_*` equivalents)
override the store when set.

Check the connection end to end:

```sh
abstractcode doctor
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
abstractcode
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
   Launching starts a **fresh** session; continuity is explicit — `--resume`
   (or `--continue`) reopens the last session, `--session <id>` opens a named
   one, and `/sessions` switches in-app. `/new` starts a fresh session
   mid-flight. When you reopen a session whose run is still active, the app
   reattaches to it automatically.
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

6. **Instruments.** Declare your model's context window once —
   `/context 262k` — and the footer meter becomes `ctx used/window (%)`
   (warns at 75%). `/gpu` toggles a gateway-host GPU meter, and
   `/resources` (alias `/host`) opens the gateway host's memory +
   resident-model panel — with model unload/lock and a context
   estimate — and feeds a footer `mem NN%` segment (on gateways that
   declare the `host_state` contract). If your
   terminal ever gets externally cleared (Cmd+K), `Ctrl+L` (or
   `/redraw`) repaints everything, and the screen also heals itself at
   the next focus round-trip (click away and back).

## Working with entities

If your gateway hosts summoned entities, `@name` opens a durable **visit**
(`/entities` lists the roster with identity cards; `Enter` there does the
same as `@name`). The visit gets its own transcript and a header chip;
`Alt+E` cycles focus between the agent and open visits, and everything you
submit under entity focus is that visit's next turn.

Two honesty rules to know up front: entity turns are **non-interruptible**
and stream **no mid-turn progress** (both server-side boundaries — the app
shows elapsed time, never a fake spinner of activity). Typing during a turn
holds one draft and auto-sends it when the turn parks. `/end` closes the
visit — the entity's reflection runs server-side, and a visit that woke a
sleeping entity restores the sleep. To hand work over *without* a visit,
`/task <name> <title>` leaves it on the entity's desk; pickup happens at the
entity's own boundary.

## Permissions

Tired of approving every `read_file`? `/permissions <read|write|all>` sets
the persisted auto-approval level: `read` auto-approves only proven
read-only tools (the default; read-only `git` commands are proven and
approved server-side by the runtime's refiner), `write` adds workspace
file mutations, and
`all` auto-approves everything — **including arbitrary shell commands and
network egress**. Use `all` deliberately, on gateways you trust with the
workspace they expose; it is sticky per session and seeds new sessions.
Per-tool pins (`p` in `/tools`) override the level in both directions: pin
`auto` to lift one tool above the level, pin `ask` to force a prompt below
it — pins gate even at `all`, and gateway-disabled tools never run.

## Headless one-shots

For scripts and CI:

```sh
abstractcode exec "List the files in the workspace and summarize them" \
  --permissions all --timeout 300
```

Events print as they fold; the final answer prints under an `answer` rule.
Exit codes: 0 completed, 1 failed, 124 timeout, 130 cancelled. Without a
raised `--permissions` level, mutating tools are denied with an explanation
the agent sees (it will finish as best it can without them);
`--require-approval <names>` gates specific tools even at `all`. Ask-user
waits receive an honest "no interactive user is present" response.

## Exporting transcripts

`/export` writes the current agent conversation to a file, serving two
goals in one command: **archival** — bare `/export` (or `/export md`)
produces a readable markdown document mirroring the transcript as shown,
auto-named in your working directory; and **training** — `/export jsonl`
produces SFT-ready JSONL (OpenAI chat schema, one self-contained training
example per completed turn; failed/unanswered turns are skipped and
counted, so every line is a clean user→assistant pair for SFT or CPT
pipelines). Add `--details` to include reasoning cycles and full tool
cards, and an explicit path to choose the destination
(`/export jsonl --details data/run1.jsonl`; tildes are not expanded, and
the parent directory must already exist). Exports never overwrite an
existing file. A transcript whose older items were dropped from view is
disclosed honestly per format: the markdown header says so; JSONL stays
schema-pure (no header line), so the on-screen notice carries the
warning — the earliest turns are missing from every line's prefix. Full
format spec in [api.md](api.md#transcript-export-export).

## Attaching files

Three ways to hand the agent a file:

- **Drag & drop** — drop a file from Finder (or any file manager) onto
  the terminal window: it attaches directly as a chip above the composer
  (nothing lands in your draft). `Ctrl+O` undoes a drop — the chip goes
  away and the pasted path text appears in the composer instead.
- **`/attach <path>`** — accepts `~`, quotes, escaped spaces, `file://`
  URLs, and relative paths. Bare `/attach` opens a file browser (type to
  filter, `Enter` descends/picks, `Space` marks several); with chips
  already staged it opens the pending manager instead.
- **`exec --attach <path>`** — headless parity (repeatable; any upload
  failure exits 1 before the run starts).

The chips row is interactive: **click a file's name** to preview it,
**click the `×`** to unstage it. Preview opens the file itself — a
scrolling, line-numbered view for text documents, the picture for PNG
and JPEG. **`/attach preview <path>`** works on any file, staged or not;
inside the pending manager, `p` (or `Enter`) previews the chip under the
cursor and `x` removes it.

Chips upload at SEND time and ride the run as `context.attachments`.
Text-like files inline straight into the model's context (up to 120 KB
each, server-side); PDFs extract on demand; images need a
vision-capable model route; other binaries the agent can list but
usually not read — the attach notice tells you which. An upload failure
blocks the send and keeps your chips (fix and resend; already-uploaded
siblings are reused, never duplicated). Session uploads are permanent
on the gateway: removing a chip before sending is the moment to change
your mind. Details in [api.md](api.md#attachments-attach-drag--drop-exec---attach).

## Where to next

- [architecture.md](architecture.md) — how the client works (threading, the
  ledger fold, the gateway contract)
- [api.md](api.md) — every command, key binding, and CLI option
- [faq.md](faq.md) · [troubleshooting.md](troubleshooting.md)
