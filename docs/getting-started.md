# Getting started

AbstractCode needs a reachable [AbstractGateway](https://github.com/lpalbou/abstractgateway).
The gateway runs the coding agent; the clients connect to it.

## 1. Run a gateway

```bash
pip install abstractgateway
abstractgateway serve            # defaults to http://127.0.0.1:8080
```

The gateway ships working agent bundles out of the box, including
`coding-agent:coder`, which the terminal client uses by default.

## 2. Install a client

### Terminal

```bash
cargo install abstractcode
```

Requires Rust 1.87 or newer. Then:

```bash
abstractcode                     # launch against http://127.0.0.1:8080
abstractcode --gateway https://gateway.example.com
```

### Browser

```bash
npx @abstractframework/code      # serves on http://127.0.0.1:3002
```

Set the Gateway URL in the interface, or start the server with
`ABSTRACTCODE_GATEWAY_URL` already pointing at it.

## 3. Check the connection

```bash
abstractcode doctor
```

`doctor` reports whether the gateway is reachable, which credential source was
used, and which workflows are available. Run it first whenever something is not
behaving — it distinguishes "the gateway is down" from "your token is wrong"
from "the workflow you asked for is not installed".

## Credentials

A remote gateway usually requires a token. Persist one so you do not repeat it:

```bash
abstractcode login --gateway https://gateway.example.com --token <TOKEN>
```

This verifies the credentials before writing them to
`~/.abstractcode/gateway.json`. Both `--gateway` and `--token` can also come
from the environment, and a token passed on the command line always wins over
the stored one.

The browser client keeps credentials differently: when you supply a gateway
user and token, the server exchanges them for a gateway browser session and
stores only app-scoped session cookies, so the raw token is never persisted in
browser settings. See [`web.md`](web.md).

## Your first run

Type a task and press Enter. What you see:

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
choice is remembered in `~/.abstractcode/prefs.json`.

## Where to go next

- [`architecture.md`](architecture.md) — how the pieces fit together
- [`api.md`](api.md) — the gateway surface the clients speak
- [`troubleshooting.md`](troubleshooting.md) — when something does not work
- [`../tui/README.md`](../tui/README.md) — the terminal client in depth, including keys and themes
