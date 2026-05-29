# AbstractCode

Durable terminal TUI for agentic coding on the AbstractFramework stack (**AbstractAgent + AbstractRuntime + AbstractCore**).

Status: **pre-alpha** (APIs and UX may change).

Next: [`docs/getting-started.md`](docs/getting-started.md).

## AbstractFramework ecosystem

AbstractCode is part of **AbstractFramework**:
- [AbstractFramework](https://github.com/lpalbou/AbstractFramework)
- [AbstractCore](https://github.com/lpalbou/abstractcore) (providers + tools)
- [AbstractRuntime](https://github.com/lpalbou/abstractruntime) (durable runs)

## Features

- Interactive TUI (`abstractcode`) with **durable runs** (resume/pause/cancel), snapshots, and logs
- **Approval-gated tools** by default (with an allowlist you can configure)
- Built-in agents: `react`, `memact`, `codeact` (from `abstractagent`)
- Plan + Review modes (`--plan`, `--review`; `/plan`, `/review`)
- VisualFlow workflows:
  - run locally: `abstractcode flow ...` (optional extra)
  - run as an agent: `abstractcode --agent <flow_ref>`
- Remote tool execution via **MCP** (`/mcp`, `/executor`)
- Optional gateway-first Web UI in `web/`

## How it fits together (diagram)

```mermaid
flowchart LR
  U[User] --> AC[AbstractCode\n(TUI/CLI host)]
  AC --> AA[AbstractAgent\n(agent logic)]
  AC --> AR[AbstractRuntime\n(durable execution)]
  AR --> CORE[AbstractCore\n(LLM providers + tools)]

  AC -->|approve| TOOLS[Local tools]
  AC <--> MCP[MCP servers\n(remote tools)]

  WEB[web/\n(browser host)] <--> GW[AbstractGateway\n(/api/gateway/*)]
  AC -. optional .-> GW
```

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

## Prompt caching (best-effort)

AbstractCode defaults to `--prompt-cache auto`, which enables prompt caching when the runtime LLM client reports prompt-cache support via the AbstractCore capability contract (reduces repeated *prefill* work and can improve time-to-first-token).

Toggle:
- CLI: `--prompt-cache auto|on|off` (or `ABSTRACTCODE_PROMPT_CACHE=auto|on|off`)
- TUI: `/cache auto|on|off`

Provider notes:
- `mlx`: full in-process KV caching with a 3-compartment cache: `system | tools | history`.
- `huggingface` + GGUF (llama.cpp): same 3-compartment cache when AbstractCore can render the model's llama.cpp chat format exactly for caching (currently `chatml-function-calling` and `llama-3`); otherwise it falls back to stable keyed `LlamaRAMCache` reuse.
- Remote APIs: `prompt_cache_key` is forwarded where applicable (server-managed; semantics vary by provider).

Details: [`docs/architecture.md`](docs/architecture.md) and [`docs/faq.md`](docs/faq.md).

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

The web host lives in `web/` and connects to an **AbstractGateway** at `/api/gateway/*`.

Start here:
- [`docs/web.md`](docs/web.md)
- [`docs/deployment-web.md`](docs/deployment-web.md)

## Documentation

- Start here: [`docs/getting-started.md`](docs/getting-started.md)
- FAQ: [`docs/faq.md`](docs/faq.md)
- Docs index: [`docs/README.md`](docs/README.md)
- Agent-oriented docs: [`llms.txt`](llms.txt) and [`llms-full.txt`](llms-full.txt)
- [`docs/architecture.md`](docs/architecture.md)
- [`docs/cli.md`](docs/cli.md)
- [`docs/api.md`](docs/api.md)
- [`docs/workflows.md`](docs/workflows.md)
- [`docs/ui_events.md`](docs/ui_events.md)

## Development

```bash
pip install -e ".[dev]"
pytest -q
ruff check .
black .
```

## Project

- Changelog: [`CHANGELOG.md`](CHANGELOG.md)
- Contributing: [`CONTRIBUTING.md`](CONTRIBUTING.md)
- Security: [`SECURITY.md`](SECURITY.md)
- Acknowledgments: [`ACKNOWLEDGMENTS.md`](ACKNOWLEDGMENTS.md)

## License

MIT. See [`LICENSE`](LICENSE).
