# Architecture

AbstractCode is two clients over one durable control plane. Neither client runs
the coding agent; both observe and steer a run that lives on
[AbstractGateway](https://github.com/lpalbou/abstractgateway).

## Shape

```mermaid
flowchart LR
    subgraph term["Terminal"]
        tui["Terminal client<br/><code>tui/</code> — Rust<br/>crate <code>abstractcode</code>"]
    end
    subgraph browser["Browser"]
        spa["Browser client<br/><code>web/src/</code> — React"]
    end
    proxy["App server<br/><code>web/bin/server.js</code><br/>session exchange, CSRF,<br/>same-origin proxy"]
    gw["<b>AbstractGateway</b><br/>durable runs, sessions,<br/>run ledger, workspaces"]
    subgraph server["Gateway host"]
        rt["AbstractRuntime<br/>executes the run"]
        core["AbstractCore<br/>providers and tools"]
    end

    tui -- "HTTP: start, commands,<br/>discovery, workspace files" --> gw
    gw -- "SSE: ledger steps<br/>+ live reply deltas" --> tui
    spa -- "same-origin <code>/api/gateway/*</code>" --> proxy
    proxy -- "HTTP + SSE, X-Forwarded-For<br/>= browser socket address" --> gw
    gw --> rt --> core
```

The terminal client talks to the gateway directly. The browser client talks
only to its own app server, which exchanges gateway credentials for an
app-scoped session and forwards every API call and stream to the gateway.

Both clients speak the same surface, so a session is portable between them: a
run gated on approval in the terminal can be approved in the browser, and a run
started in the browser can be reattached from the terminal.

## The run stream

Each client follows a run through one Server-Sent Events stream,
`GET /runs/{id}/ledger/stream`. It carries two kinds of frame:

- **Ledger steps** (`event: step`, with an `id:` cursor) are the durable record.
  Everything a client renders about a run comes from them, and a reconnect
  resumes from the last cursor.
- **Live reply deltas** (`event: llm.delta` and `event: llm.delta_end`, with no
  `id:`) carry the model's text while it is being written. They appear only
  when the gateway advertises `streaming.deltas` in
  `GET /discovery/capabilities` and the run asked to stream. They never move
  the cursor and are never stored; the recorded `llm_call` step replaces the
  live text when the call completes.

```mermaid
sequenceDiagram
    participant C as Client (terminal or browser)
    participant G as AbstractGateway
    participant R as AbstractRuntime
    C->>G: POST /runs/start (input_data._runtime.stream)
    C->>G: GET /runs/{id}/ledger/stream
    R-->>G: text deltas for the model call
    G-->>C: event: llm.delta (call_id, seq, text)
    R->>G: llm_call step written to the ledger
    G-->>C: event: step (id: cursor)
    G-->>C: event: llm.delta_end (reason)
    Note over C: the recorded step replaces the live bubble
    G-->>C: event: done (root run finished)
```

The run's `_runtime.stream` value comes from the client's **Stream replies**
setting: nothing for "Gateway default" (the gateway's own default applies),
`false` for Off, and `true` for On only when the gateway advertises live
replies. See [`web.md`](web.md#stream-replies) and the terminal client's
[`/stream` reference](../tui/docs/api.md#streamed-replies-stream).

## The thin-client contract

Everything a client does goes through the gateway, and therefore the runtime.
Four rules follow, and they are binding on both clients:

1. **Server truth is the only truth about runs.** A client renders what the
   ledger says. It never invents a status the gateway did not report; unknown
   renders as unknown.
2. **Decisions are communicated, never executed locally.** Approvals, answers,
   cancels, pauses, and steering all travel as durable gateway commands, so they
   are traceable in the ledger and answerable from any client.
3. **Interface overlays on server truth must be honest and labelled.** Where a
   client's rendering deliberately diverges from raw server state, the
   divergence is named where you read it.
4. **Client-held state stays a client concern** — rendering, input, credentials,
   local preferences — plus intent you have not submitted yet, such as a
   composer draft. The moment intent becomes work, it is a gateway run.

The consequence you can rely on: start a task, disconnect, reconnect later, and
find the run where it actually got to.

## Repository layout

```text
tui/     the terminal client — Rust crate `abstractcode`, a workspace member
web/     the browser client — npm `@abstractframework/code`
docs/    documentation for the project as a whole
```

The Cargo workspace lives at the repository root with `tui` as its only member.
That keeps `target/` at the root while scoping `cargo` packaging to the crate,
so ordinary churn under `web/` cannot block a release.

The two clients version and release independently, each under its own tag
prefix — `v<version>` for the terminal client, `web-v<version>` for the browser
client. See [`../CONTRIBUTING.md`](../CONTRIBUTING.md).

## Boundaries

- **No client-side agent loop.** Neither client decides what the model does next;
  the runtime does.
- **No shared code between the clients.** They are separate implementations of
  the same wire contract in different languages, deliberately: each is idiomatic
  for its platform. The contract they share is the gateway's, documented in
  [`api.md`](api.md).
- **The gateway chooses the default workflow.** A client that follows the
  gateway default starts runs with `flow_id: "@default"` and the agent
  interface; the gateway resolves it at every run start and reports what it
  started. Neither client keeps a fallback workflow of its own. See
  [`workflows.md`](workflows.md#the-gateway-default).
- **Files live on the gateway host.** Both clients browse a run's workspace
  through the gateway's workspace routes and show its absolute path together
  with the host it is on.
- **Shared browser components are package dependencies.** `web/` consumes
  AbstractUIC packages from the npm registry, without sibling-checkout aliases,
  so the checkout builds independently.

## Browser ownership

```mermaid
flowchart LR
    shell["AbstractCode workspace shell"] --> chat["AbstractUIC WorkflowChat"]
    shell --> kit["AbstractUIC themes / auth / settings"]
    shell --> adapter["App transport + catalog adapters"]
    chat --> state["Transport-injected WorkflowSessionController"]
    state --> adapter
    adapter --> proxy["Same-origin session proxy"]
    proxy --> gateway["Gateway auth / registry / policy / commands"]
    gateway --> runtime["Runtime + Flow nodes + tools"]
```

`panel-chat` owns reusable presentation and replay/stream state, including
questions, approvals, event waits, and structured results. It does not store
credentials, select workspace scope, or invoke tools. Code supplies the HTTP
adapter, workflow catalog, session navigation, schema inputs, and inspector.
The app server owns session exchange and CSRF/origin checks. On every request it
sends to the gateway (API calls, run streams, status, sign-in and sign-out) it
sets `X-Forwarded-For` to the browser connection's socket address, replacing
any incoming `X-Forwarded-For`, `Forwarded` or `X-Real-IP` value, and sets
`X-AbstractFramework-App-Proxy: code`. The gateway uses these to decide whether
the browser sits on its own machine (for example before offering **Open
folder**). A connection whose address cannot be determined is refused with
HTTP 400.

Run lifecycle comes from gateway run snapshots; a completed ledger step does
not imply a completed workflow. Ledger records own replayable messages and
activity. UI-only status events never replace authoritative run status.
Account and session changes abort or isolate pending requests and local queues.

The entrypoint (`web/src/main.tsx`) mounts `web/src/workspace/app.tsx`. The
modules under `web/src/ui/` are not mounted by the application.
