# FAQ

For the authoritative command list, run `/help` inside the terminal client, or
`abstractcode --help` for the command-line surface.

## What is AbstractCode?

An agentic coding assistant with two clients — a terminal application and a
browser application — over a coding agent that runs durably on
[AbstractGateway](https://github.com/lpalbou/abstractgateway).

## Do I need a gateway?

Yes. Neither client runs the agent itself. The gateway can run on your own
machine:

```bash
pip install abstractgateway
abstractgateway serve
```

## Which client should I use?

Whichever suits the moment — they are interchangeable, not alternatives. Both
speak the same durable commands against the same sessions, so you can start a
task in the terminal and resolve its approval prompt in the browser, or the
reverse.

## Can I close the client while a run is going?

Yes. The run lives on the gateway. Reattach with `abstractcode --resume`, or
open the session in the browser. This is the point of the architecture rather
than a side effect of it.

## Will it edit my files without asking?

No. Tools are approval-gated by default: the run pauses and waits for you.
`/gate` and `/permission` adjust the policy, and `--permissions` sets it for a
headless `exec` run. Because the wait is durable, an approval you miss is still
there when you come back.

## What is review mode?

Before a response with no tool calls is accepted as final, a verifier re-reads
the transcript and can send the agent back for more work. It is on by default;
`--no-review` disables it and `/review` toggles it mid-session.
`--review-rounds` sets the budget, default 3.

## How do I choose the model or provider?

```bash
abstractcode --provider lmstudio --model qwen3.6-35b
```

Or `/model` inside the session. Without either, the gateway's defaults apply.
Your choice is remembered in `~/.abstractcode/prefs.json`.

## What is a workflow?

A named agent bundle the run executes, `coding-agent:coder` by default.
`--workflow <bundle[:flow]>` selects another, and `abstractcode doctor` lists
what the gateway has installed. See [`workflows.md`](workflows.md).

## Where are my settings stored?

- `~/.abstractcode/gateway.json` — gateway URL and token, written by
  `abstractcode login`
- `~/.abstractcode/prefs.json` — theme, model, workflow, last session

`ABSTRACTCODE_PREFS_FILE` relocates the second. A build older than the rename
wrote `~/.abstractcode-tui/prefs.json`; that file is read once and saved
forward.

## Can I run it without an interface?

```bash
abstractcode exec "add tests for the parser" --permissions all
```

`exec` streams events to stdout and exits with a code reflecting the run's
terminal status, which is what bench harnesses and orchestrating agents use.

## Why does the browser client refuse to change the Gateway URL?

Only a request arriving from loopback may reconfigure it; otherwise the
server-configured URL is authoritative. This is deliberate — deciding from the
`Host` header instead would let a remote client claim to be local and turn the
server into a relay for its own session cookies. See
[`web.md`](web.md) for the environment variables that adjust this.

## My terminal renders it badly. What now?

Run `abstractcode --caps` to see what the client detected, and `Ctrl+L` or
`/redraw` to recover a corrupted screen. [`troubleshooting.md`](troubleshooting.md)
covers the common cases, including why `Shift+Enter` works only in some
terminals.

## What are the current limitations?

- **Pre-alpha.** Interfaces and behavior may change between releases.
- **A gateway is required.** There is no offline or standalone mode.
- **The library surface is unstable.** The crate publishes its modules, but only
  the command-line surface is treated as a contract for now.
- **The two clients are not feature-identical.** They implement the same wire
  contract independently, and each is ahead of the other in places.
