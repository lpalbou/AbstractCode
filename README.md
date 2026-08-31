# AbstractCode

An agentic coding assistant that runs **durably on a server**, with two clients
you can use interchangeably: a terminal application and a browser application.

The coding agent itself runs on [AbstractGateway](https://github.com/lpalbou/abstractgateway).
Both clients are thin: they start runs, stream the run ledger live, render
reasoning cycles and tool calls as they happen, resolve tool-approval and
ask-user prompts, and steer a run while it is in flight. Because the work lives
on the gateway rather than in the client, you can start a task in the terminal,
close your laptop, and pick the same session up in the browser.

Status: **pre-alpha.** Interfaces and UX may change.

## The two clients

| | Terminal | Browser |
|---|---|---|
| Install | `cargo install abstractcode` | `npx @abstractframework/code` |
| Source | [`tui/`](tui/) | [`web/`](web/) |
| Built with | Rust, [AbstractTUI](https://github.com/lpalbou/AbstractTUI) | TypeScript, React, Vite |
| Docs | [`tui/README.md`](tui/README.md) | [`docs/web.md`](docs/web.md) |

Both talk to the gateway over HTTP and SSE only, and both resolve the same
durable commands, so a run gated on your approval in one client can be approved
from the other.

## Quick start

You need a reachable AbstractGateway. To run one locally:

```bash
pip install abstractgateway
abstractgateway serve            # binds 0.0.0.0:8080; reach it at http://127.0.0.1:8080
```

Then start whichever client you prefer:

```bash
# Terminal
cargo install abstractcode
abstractcode                     # or: abstractcode doctor, to check the connection

# Browser
npx @abstractframework/code      # binds 0.0.0.0:3002; open http://127.0.0.1:3002
```

`abstractcode doctor` diagnoses the gateway connection and prints which
credential source it used. See [`docs/getting-started.md`](docs/getting-started.md)
for credentials, remote gateways, and first-run configuration.

## What you can do

- **Durable runs** — pause, cancel, resume, and reattach to a session that
  keeps running while the client is closed.
- **Approval-gated tools** — tools stop and wait for you by default; approve or
  reject from either client.
- **Steering** — inject guidance into a run without restarting it.
- **Workflows** — run a named agent bundle, `coding-agent:coder` by default.
- **Review mode** — before a tool-call-free answer is accepted as final, a
  verifier re-reads the transcript and can force more work.
- **Live activity** — reasoning cycles, tool cards that update in place, token
  counts, and context metering, streamed from the run ledger.

## Documentation

- [`docs/getting-started.md`](docs/getting-started.md) — install, connect, first run
- [`docs/architecture.md`](docs/architecture.md) — how the clients and gateway fit together
- [`docs/api.md`](docs/api.md) — the gateway surface both clients speak
- [`docs/web.md`](docs/web.md) — the browser client in depth
- [`docs/faq.md`](docs/faq.md) — recurring questions and known limits
- [`docs/troubleshooting.md`](docs/troubleshooting.md) — symptoms and fixes
- [`tui/docs/`](tui/docs/) — terminal client reference, keys, and design notes

## The AbstractFramework ecosystem

AbstractCode is one part of **AbstractFramework**:

- [AbstractFramework](https://github.com/lpalbou/AbstractFramework) — the ecosystem hub
- [AbstractGateway](https://github.com/lpalbou/abstractgateway) — the durable control plane AbstractCode talks to
- [AbstractCore](https://github.com/lpalbou/abstractcore) — providers and tools
- [AbstractRuntime](https://github.com/lpalbou/abstractruntime) — durable run execution
- [AbstractTUI](https://github.com/lpalbou/AbstractTUI) — the terminal rendering engine
- [AbstractUIC](https://github.com/lpalbou/abstractuic) — the shared browser UI components

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for development setup and the test
commands for each client. Security reports go through [`SECURITY.md`](SECURITY.md).

## License

MIT — see [`LICENSE`](LICENSE).
