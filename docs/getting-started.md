# Getting started

AbstractCode needs a reachable [AbstractGateway](https://github.com/lpalbou/abstractgateway).
The gateway runs the coding agent; the clients connect to it.

## 1. Run a gateway

```bash
pip install abstractgateway
abstractgateway serve            # binds 0.0.0.0:8080 by default
```

The gateway ships working agent bundles out of the box. Its default agent
workflow is the shipped `basic-agent` until its operator chooses another, and
both clients use that gateway default unless you pick a workflow yourself.

## 2. Install a client

### Terminal

```bash
cargo install abstractcode
```

Requires Rust 1.87 or newer. Then:

```bash
abstractcode                     # launch against http://127.0.0.1:8080
abstractcode --gateway-url https://gateway.example.com
```

### Browser

```bash
npx @abstractframework/code      # binds 127.0.0.1:3002; open http://127.0.0.1:3002
```

The web server finds a local gateway by itself (the pointer
`~/.abstractframework/gateway.json` that `abstractgateway serve` writes), or
start it with `--gateway-url <url>`. The gateway can also serve it for you at
`/apps/code/` (its console's **Apps** page); see
[Web deployment](deployment-web.md).

The web server listens on `127.0.0.1` by default (`--host 0.0.0.0` accepts
other machines); the gateway listens on every interface unless you pass
`abstractgateway serve --host 127.0.0.1`.

## 3. Check the connection

```bash
abstractcode doctor
```

`doctor` reports whether the gateway is reachable, which credential source was
used, and which workflows are available. Run it first whenever something is not
behaving — it distinguishes "the gateway is down" from "your token is wrong"
from "the workflow you asked for is not installed".

## Credentials

A gateway asks every client to sign in. When it refuses this one, the
terminal client says so before it opens (`not signed in to <gateway>`) and
prints the way to sign in; it exits without starting.

On the gateway's own computer (including over SSH to it), open it signed in
without handling a token:

```bash
abstractgateway apps tui-command code
```

It prints a one-use line (valid 2 minutes); run it and AbstractCode opens
signed in. That sign-in lives in the gateway's memory and ends when the
gateway restarts; the app then shows "not signed in".

To sign in once for good, save a token with `login` (on the gateway's
computer, `abstractgateway-config bootstrap-admin --print-token` prints the
admin token):

```bash
abstractcode login --token <value>
```

A remote gateway usually requires a token too. Persist one so you do not repeat it:

```bash
abstractcode login --gateway-url https://gateway.example.com --token <TOKEN>
```

This verifies the credentials before writing them to
`~/.abstractcode/gateway.json`. Both `--gateway-url` and `--token` can also come
from the environment, and a token passed on the command line always wins over
the stored one.

The browser client keeps credentials differently: when you supply a gateway
user and token, the server exchanges them for a gateway browser session and
stores only app-scoped session cookies, so the raw token is never persisted in
browser settings. See [`web.md`](web.md).

## Your first run

Type a task and press Enter. What you see:

The terminal client includes the specialized views below. In the browser, use
the transcript for questions and answers and **Activity** for tool arguments
and step results; see the [web guide](web.md) for its workflow.

- **Reasoning cycles** as the agent works, with token counts and a per-cycle
  output sparkline.
- **Tool cards** that update in place: awaiting approval, then running, then a
  result. Tools are approval-gated by default — nothing touches your files
  until you say so.
- **A final answer**, which by default a verifier re-reads before accepting, and
  can send back for more work (`--no-review` turns this off).

Useful while a run is in flight:

| Action | How |
|---|---|
| Approve or reject a tool | the prompt in the transcript |
| Steer without restarting | type guidance and send |
| Pause or cancel | `/pause`, `/cancel` |
| Reattach to the last session | `abstractcode --resume` |
| List the commands | `/help` |

## Choosing a workflow and model

```bash
abstractcode --workflow coding-agent:coder --provider lmstudio --model qwen3.6-35b
```

Both are optional: without them, the gateway's defaults apply and your last
choice is remembered in `~/.abstractcode/prefs.json`. `--workflow default`
selects the gateway's default workflow explicitly; see
[`workflows.md`](workflows.md#the-gateway-default).

In the browser, open **Settings → Model & behavior** (the gear, or the rail's Settings icon). Its
**Workflow** picker starts with **Gateway default**; the same category holds
model, reasoning, MTP and **Stream replies** choices.

## Where the files are

The agent reads and writes files on the gateway host, in the run's workspace.
The browser's **Files** panel (right rail) and the terminal's `/files` command show that
folder: its absolute path, the host it is on, and a preview of each file. When
the gateway runs on another machine, the terminal client does not send your
local folder as the workspace; the agent works in a gateway-side session folder
instead (see the terminal [getting started](../tui/docs/getting-started.md)).

## Where to go next

- [`architecture.md`](architecture.md) — how the pieces fit together
- [`api.md`](api.md) — the gateway surface the clients speak
- [`troubleshooting.md`](troubleshooting.md) — when something does not work
- [`../tui/README.md`](../tui/README.md) — the terminal client in depth, including keys and themes
