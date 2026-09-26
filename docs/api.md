# API and integration points

AbstractCode has no server of its own. Both clients integrate through
[AbstractGateway](https://github.com/lpalbou/abstractgateway), over plain HTTP
and Server-Sent Events, and that gateway surface is the contract to program
against.

## Command-line surface

The terminal client is the scriptable entry point.

```bash
abstractcode                              # launch the interface
abstractcode exec "<prompt>" [OPTIONS]    # headless one-shot run, prints events
abstractcode login [OPTIONS]              # verify and persist gateway credentials
abstractcode doctor [OPTIONS]             # diagnose the gateway connection
abstractcode --caps                       # print the terminal capability report
```

`abstractcode --help` prints the full option list. The two that matter most for
integration:

- `exec` runs a prompt to completion without an interface and streams events to
  stdout, which is what bench harnesses and orchestrating agents consume. Its
  exit code reflects the run's terminal status.
- `doctor` is the supported way to assert an environment is wired correctly
  before running anything else.

Configuration resolves in a fixed order — explicit flag, then environment, then
the login store at `~/.abstractcode/gateway.json`, then the default
`http://127.0.0.1:8080`. `doctor` prints which source won.

## Library surface

The terminal client also builds as a Rust library:

```toml
[dependencies]
abstractcode = "0.5"
```

```rust
use abstractcode::gateway::GatewayClient;
```

`GatewayClient` is the gateway transport — starting runs, streaming the ledger,
and submitting durable commands. The interface modules above it are internal and
change freely between releases; treat the crate's library surface as
unstable while the project is pre-alpha.

## Gateway surface

Both clients use the same endpoints. This is the integration contract:

| Purpose | Shape |
|---|---|
| Start a run | `POST` a run request with the workflow (or `@default` plus the agent interface), prompt, and options; the response names the resolved workflow |
| Stream a run | `GET` the run's ledger stream as SSE (`event: step`, terminated by `event: done`); with streaming on, `llm.delta` / `llm.delta_end` frames carry live reply text |
| Resolve a wait | `POST` a durable command — approve, reject, or answer |
| Steer a run | `POST` a guidance command against the live run |
| Pause / cancel | `POST` the corresponding durable command |
| Session history | `GET` the session's history bundle for replay |
| Discovery | `GET` the available workflows (with the gateway's default agent workflow), providers, models, tools, skills, and capabilities such as live replies |
| Workspace files | `GET` a run's workspace location, a folder listing, and file content (with `Range`) |
| About | `GET` the gateway's AbstractFramework and package versions |

Two properties follow from the gateway owning all of this, and both are relied
on by the clients:

- **Commands are durable and idempotent by id.** A command survives a client
  disconnect, and re-submitting one does not double-apply it. This is why a
  wait raised in one client can be resolved in another.
- **The ledger is the record.** Rendering is a replay of it. A client that
  reconnects mid-run recovers the full state by replaying the ledger rather
  than by holding state across the gap.

See the gateway's own documentation for exact paths, payload schemas, and
authentication modes; it owns those definitions, and pinning them here would
guarantee drift.

## Workflow contract

A run names a workflow bundle, or asks for the gateway's default agent workflow
(see [`workflows.md`](workflows.md#the-gateway-default)). Agents exposed to
AbstractCode implement the `abstractcode.agent.v1` interface, described in
[`workflows.md`](workflows.md). Workflow-driven interface events — status lines,
messages, and tool-execution cards — are described in [`ui_events.md`](ui_events.md).

## Browser integration

The browser client is published as `@abstractframework/code` and also runs as a
served application:

```bash
npx @abstractframework/code            # http://127.0.0.1:3002
ABSTRACTCODE_GATEWAY_URL=... npx @abstractframework/code
```

See [`web.md`](web.md) for its configuration surface and
[`deployment-web.md`](deployment-web.md) for hosting it.
