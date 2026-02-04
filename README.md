# AbstractCode

Durable terminal TUI for agentic coding on the Abstract* stack (**AbstractAgent + AbstractRuntime + AbstractCore**).

Status: **pre-alpha** (APIs and UX may change).

Next: [`docs/getting-started.md`](docs/getting-started.md).

## Features

- Interactive TUI (`abstractcode`) with **durable runs** (resume/pause/cancel), snapshots, and logs
- **Approval-gated tools** by default (with an allowlist you can configure)
- Built-in agents: `react`, `memact`, `codeact`
- VisualFlow workflows:
  - run locally: `abstractcode flow ...` (optional extra)
  - run as an agent: `abstractcode --agent <flow_ref>`
- Remote tool execution via **MCP** (`/mcp`, `/executor`)
- Optional gateway-first Web UI in `web/`

## Install

Python: **3.10+**

```bash
pip install abstractcode
```

Optional (run VisualFlow locally via `abstractcode flow ...`):

```bash
pip install "abstractcode[flow]"
```

From source (development):

```bash
pip install -e ".[dev]"
```

## Quickstart (TUI)

Ollama (default provider):

```bash
abstractcode --provider ollama --model qwen3:1.7b-q4_K_M
```

OpenAI-compatible server (e.g. LM Studio):

```bash
abstractcode --provider openai --base-url http://127.0.0.1:1234/v1 --model qwen/qwen3-next-80b
```

Inside the app:
- `/help` shows the authoritative command list
- type a task (or use `/task ...`)
- tool approvals: `/auto-accept` (or start with `--auto-approve`)
- attach files with `@path/to/file` in your prompt

## Persistence (durable runs)

Default paths:
- state file: `~/.abstractcode/state.json`
- durable stores: `~/.abstractcode/state.d/`
- saved settings: `~/.abstractcode/state.config.json`

Disable persistence:

```bash
abstractcode --no-state
```

## Workflows

- Local runs: `abstractcode flow run <flow_id_or_path> ...` (requires `abstractcode[flow]`)
- Workflow agent: `abstractcode --agent /path/to/workflow.json ...`
- Remote control-plane: `abstractcode gateway --help`
- Bundle management on a gateway: `abstractcode workflow --help`

Details: [`docs/workflows.md`](docs/workflows.md).

## Web UI

The web host lives in `web/` and connects to an `abstractgateway` at `/api/gateway/*`.

Start here:
- [`docs/web.md`](docs/web.md)
- [`docs/deployment-web.md`](docs/deployment-web.md)

## Documentation

- Start here: [`docs/getting-started.md`](docs/getting-started.md)
- Docs index: [`docs/README.md`](docs/README.md)
- [`docs/architecture.md`](docs/architecture.md)
- [`docs/cli.md`](docs/cli.md)
- [`docs/workflows.md`](docs/workflows.md)
- [`docs/ui_events.md`](docs/ui_events.md)

## Development

```bash
pip install -e ".[dev]"
pytest -q
ruff check .
black .
```

## License

MIT. See [`LICENSE`](LICENSE).
