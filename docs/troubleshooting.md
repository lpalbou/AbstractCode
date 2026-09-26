# Troubleshooting

Run `abstractcode doctor` first. It separates the three failures that look
alike from inside the interface: the gateway is unreachable, your credentials
are wrong, or the workflow you asked for is not installed.

## Connection

**"Connection refused" or the client sits at connecting.**
The gateway is not running or is not where the client is looking. Confirm it
directly, then check which URL the client resolved:

```bash
curl -sS http://127.0.0.1:8080/api/health
abstractcode doctor
```

Resolution order is `--gateway`, then the environment, then the login store at
`~/.abstractcode/gateway.json`, then `http://127.0.0.1:8080`. `doctor` prints
which one it used, which is usually the answer when a saved value is shadowing
the one you expect.

**401 or 403 from a remote gateway.**
The token is missing, expired, or belongs to a different gateway. Re-verify and
re-persist it:

```bash
abstractcode login --gateway https://gateway.example.com --token <TOKEN>
```

`login` checks the credentials before writing them, so a failure here is about
the credentials rather than the file.

**The browser client refuses to change the Gateway URL.**
By design, when the request does not arrive from loopback. The interface can
only reconfigure the gateway for a local connection; otherwise the
server-configured URL wins. Set
`ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1` behind your own access
control to permit it. Behind a reverse proxy, see [`web.md`](web.md) — every
peer is the proxy there, so loopback proves nothing.

## Runs

**A run stops and nothing happens.**
It is waiting for you. Approval-gated tools pause until you approve or reject,
and an agent can also ask a question. Look for the pending card in the
transcript. Because waits are durable, you can also answer from the other
client.

**A run seems stuck with no pending prompt.**
Check the run on the gateway rather than trusting the client's view — server
truth is authoritative. `/history` replays what the ledger actually recorded.

**The client disconnected but the run mattered.**
It kept running. Reattach:

```bash
abstractcode --resume
```

**The workflow is not found.**
The bundle is not installed on that gateway. `doctor` lists what is available.
Pick one that is, or use the gateway default (`--workflow default`, or
**Gateway default** at the top of the workflow list). A headless `exec` whose
saved workflow is gone exits with code 2 rather than running another agent.

**"no default agent workflow".**
You have not picked a workflow and the gateway has no usable default for
`abstractcode.agent.v1`; the message quotes the gateway's reason when it gives
one. Pick a workflow from the list (or `--workflow bundle:flow`), or ask the
gateway's operator to set its default agent workflow. The browser shows "gateway
does not report a default agent workflow" when the gateway's workflow list
carries no default at all; update AbstractGateway to get the **Gateway
default** entry.

**Replies appear only when they are complete.**
Streaming is off for this run, or the gateway does not support live replies.
Check the **Stream replies** setting (browser) or `/stream` (terminal). When the
gateway does not advertise live replies, the setting is shown disabled with the
reason, and a saved On adds one note to the conversation. A one-line
"unavailable" note on a single model call means the gateway ran that call
without streaming (for example, a call that must return structured output); the
reply still arrives when the call completes. See [`web.md`](web.md#stream-replies).

## Workspace files

**The Files tab or `/files` reports HTTP 404.**
The gateway does not offer workspace browsing. Update AbstractGateway; the run
itself is unaffected.

**There is no Open folder button, or `o` does nothing.**
Opening the folder happens on the gateway's machine, so it is offered only when
the gateway says you are on that machine and allows it (the open action is
admin-only). Otherwise the client shows the absolute path with "on the gateway
host <name>" and a copy action.

**The browser client answers HTTP 400 "Cannot determine the client address of
this connection".**
The app server could not read the address of the incoming connection, which it
must forward to the gateway. Connect to the app server directly or through a
reverse proxy that opens a normal TCP connection to it.

## Terminal

**No colour, broken box drawing, or a garbled layout.**
Ask the client what it detected:

```bash
abstractcode --caps
```

That reports the capabilities it resolved for your terminal. `Ctrl+L` or
`/redraw` recovers a screen corrupted by another program writing over it.

**`Shift+Enter` does not insert a newline.**
It depends on the kitty keyboard protocol, which not every terminal supports.
`Ctrl+J` always works, and the composer hint names the best available chord for
your terminal.

**Selection or copy does nothing.**
Terminals differ in what they permit. The client falls back to the host
clipboard where OSC 52 is not honoured; holding Shift or Option while dragging
hands selection to the terminal itself instead.

## Building and installing

**`cargo install abstractcode` fails on an old toolchain.**
The crate declares Rust 1.87 as its minimum. Run `rustup update`.

**The browser client fails to build after a dependency change.**
Reinstall from the lockfile rather than the manifest:

```bash
cd web && npm ci
```

If the failure mentions an `@abstractframework/*` package resolving outside
`web/`, `npm test` will name it: `dependency_hygiene.test.ts` fails when a build
config points at another checkout instead of the published package.

## Preferences

**Your theme or model choice did not persist.**
Preferences live in `~/.abstractcode/prefs.json`. Set
`ABSTRACTCODE_PREFS_FILE` to relocate it. An existing
`~/.abstractcode-tui/prefs.json` is read once and saved forward to the current
path.
