# Getting started

AbstractCode needs a reachable [AbstractGateway](https://github.com/lpalbou/abstractgateway).
The gateway runs the coding agent; the clients connect to it.

## 1. Run a gateway

```bash
pip install abstractframework    # or abstractframework[apple] / abstractframework[gpu] for local models
abstractgateway serve            # binds 0.0.0.0:8080 by default
```

Use AbstractGateway 0.13.0 or later to archive conversations and to see the
gateway's default voice routes in the clients. The browser client 0.15.0 needs
AbstractGateway 0.20.0 or later for shared unread markers, the Avatar view's live
line and Findings, and the `Paused · N` automation section. `@` mentions and the
reply's author (terminal 0.15.0, browser 0.16.0) need AbstractGateway 0.21.0.

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
| Reopen the gateway's newest session | `abstractcode --resume` |
| List the commands | `/help` |

## Choosing a workflow and model

```bash
abstractcode --workflow coding-agent:coder --provider lmstudio --model qwen3.6-35b
```

Both are optional: without them, the gateway's defaults apply and your last
choice is remembered in `~/.abstractcode/prefs.json`. `--workflow default`
selects the gateway's default workflow explicitly; see
[`workflows.md`](workflows.md#the-gateway-default).

In the browser, open the rail's **Agent** panel. Its **Workflow** picker starts
with **Gateway default**; the **Model** panel holds model, reasoning, MTP,
**Stream replies** and **Iteration limit** choices. These panels apply to the conversation (or the
automation) selected in the sidebar; see [`web.md`](web.md#working-in-the-interface).
In the terminal, `/settings` opens the same panels.

## Mentioning an entity

If your gateway hosts entities (AbstractGateway 0.21.0), you can bring one into a
conversation by name, in either client. Type `@` in the message box: a list shows the entities your account
can see. Type more of a name to narrow it, then pick one (arrow keys to move, Enter
or Tab to pick, Esc to close the list). The pick becomes a chip; a name you type
after `@` without picking it stays ordinary text. The first time you name an entity
in a conversation, a line tells you "Castor will read this conversation's history."

The list starts with the conversation's agent, under its workflow's name (for example
"Basic agent"). A message is for the names it carries:

- **Entities only:** the agent stays out. Before you send, a line says "The agent
  stays out of this message."; the gateway keeps your words as a turn of the
  conversation, no agent reply starts (no spinner, nothing to wait for), and only
  the entities answer.
- **Entities and the agent:** your message starts the agent's turn, and the agent
  answers only the part meant for it, or says nothing.
- **No chip:** the agent answers, as always.

While the agent is working on a reply, a message you send guides that reply, so the
agent reads it whatever its chips; the line does not show then.

Then each entity you picked is asked in turn, in the order you picked them, each once the previous one has
answered, so a later entity reads the earlier answers. An entity picked twice is
asked once. Every reply says who wrote it: in the browser, the bubble shows the
agent's workflow name with an "agent" tag, or the entity's name, its orb and an
"entity" tag; in the terminal, the reply reads `✦ basic-agent · agent` or
`● Castor · entity`, in the entity's colour. The answers belong to the conversation,
so they show again when you reopen it, in either client.

- **In the browser**, "Castor is answering…" shows above the message box while an
  entity answers, and its answer appears once the current reply has ended. You can
  send your next message once the entities have answered; Stop ends the asking and
  says who was not asked ("Stopped before asking Hypnos.").
- **In the terminal**, a message that is only one chip opens that entity's own
  conversation instead (a visit; see
  [working with entities](../tui/docs/getting-started.md#working-with-entities)). A
  conversation asks one message's entities at a time: send another message with
  chips meanwhile and its entities are asked next ("Pollux: asked when the mentions
  before end").

When a mention cannot be answered, the client says so:

- "Castor is in another conversation." The entity is busy elsewhere: it is skipped
  and the next one is asked.
- "Castor cannot be mentioned here." Your account cannot mention that entity: it is
  skipped and the next one is asked.
- An entity that cannot answer right now says so, and the asking stops. Your message
  goes back into the message box with only the entities that did not answer still as
  chips. Send it again unchanged to ask only those: the agent is not asked again, and
  an entity that already answered is never asked twice. Different words are a new
  message.
- "This gateway does not offer mentions yet." The gateway cannot take mentions: the
  `@` list then offers no names.

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
