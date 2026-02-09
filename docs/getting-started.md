# Getting started

AbstractCode is a **durable terminal TUI** for running agentic coding sessions on the AbstractFramework stack:
- **AbstractRuntime** provides durable runs/ledger/waits/artifacts.
- **AbstractAgent** provides the built-in agents (`react`, `memact`, `codeact`).
- **AbstractCore** provides provider/model abstraction and tool definitions.

Ecosystem links:
- [AbstractFramework](https://github.com/lpalbou/AbstractFramework)
- [AbstractCore](https://github.com/lpalbou/abstractcore)
- [AbstractRuntime](https://github.com/lpalbou/abstractruntime)

If you landed here directly, skim [`README.md`](../README.md) for an overview.

## 1) Install

Python **3.10+**:

```bash
pip install abstractcode
```

Optional (run VisualFlow locally via `abstractcode flow ...`):

```bash
pip install "abstractcode[flow]"
```

Evidence:
- Package deps and extras: `pyproject.toml`
- CLI entrypoints: `abstractcode/__init__.py` and `abstractcode/__main__.py` → `abstractcode/cli.py`

## 2) Start the interactive TUI

Ollama (default provider):

```bash
abstractcode --provider ollama --model qwen3:1.7b-q4_K_M
```

OpenAI-compatible server (example: LM Studio):

```bash
abstractcode --provider openai --base-url http://127.0.0.1:1234/v1 --model qwen/qwen3-next-80b
```

In the app:
- `/help` shows the authoritative command list (implemented in `abstractcode/react_shell.py::_show_help`)
- type a task (or use `/task ...`)

Evidence:
- Arg parsing: `abstractcode/cli.py`
- Interactive host: `abstractcode/react_shell.py`

## 2b) Plan / Review modes (optional)

AbstractCode supports two UX modes that affect how the agent responds:
- **Plan mode**: the agent outputs a TODO plan before acting (`--plan` or `/plan on`)
- **Review mode**: the agent self-checks before concluding (`--review` / `--no-review`, or `/review ...`)

Evidence:
- CLI flags: `abstractcode/cli.py`
- TUI commands: `abstractcode/react_shell.py::_show_help`

## 3) Approvals, tools, and files

### Tool approvals (default-on)

By default, tool calls pause at a durable boundary and require approval.

- CLI: `--auto-approve` (unsafe; disables prompts)
- TUI: `/auto-accept` (persists when state is enabled)

Evidence:
- Runtime is wired with `PassthroughToolExecutor(mode="approval_required")` in `abstractcode/react_shell.py`.
- After approval, tools run via `MappingToolExecutor.from_tools(...)` in `abstractcode/react_shell.py`.

### Attach files with `@path`

Mention files in your prompt to attach them:

```text
Explain @abstractcode/cli.py and @docs/architecture.md
```

Evidence:
- Mention parsing + workspace roots/mounts: `abstractcode/file_mentions.py`
- Attachment ingestion to ArtifactStore: `abstractcode/react_shell.py::_ingest_attachments()`

Workspace mounts (optional):
- `ABSTRACTCODE_WORKSPACE_MOUNTS` (preferred)
- `ABSTRACTGATEWAY_WORKSPACE_MOUNTS` (compat)

Format: newline-separated `name=/abs/path` entries.

## 4) Persistence (durable runs)

Defaults:
- state file: `~/.abstractcode/state.json`
- durable stores: `~/.abstractcode/state.d/`
- saved settings: `~/.abstractcode/state.config.json`

Disable persistence:

```bash
abstractcode --no-state
```

Evidence: file-backed vs in-memory stores are selected in `abstractcode/react_shell.py`.

## 5) Workflows and web UI (optional)

Workflows:
- Local VisualFlow runs: `abstractcode flow ...` (requires `abstractcode[flow]`)
- Workflow agent mode: `abstractcode --agent <flow_ref>`
- Gateway control-plane: `abstractcode gateway ...`
- Gateway bundle management: `abstractcode workflow ...`

Details: [`docs/workflows.md`](workflows.md).

Web UI:
- The gateway-first web host is in `web/` (separate Node/Vite build; not part of the pip wheel).

Details: [`docs/web.md`](web.md) and [`docs/deployment-web.md`](deployment-web.md).

## Next

- Docs index: [`docs/README.md`](README.md)
- FAQ (common issues): [`docs/faq.md`](faq.md)
- CLI/TUI reference (env vars, persistence details, MCP): [`docs/cli.md`](cli.md)
- Architecture (diagrams): [`docs/architecture.md`](architecture.md)
- API and integration points: [`docs/api.md`](api.md)
- Workflow UI events contract: [`docs/ui_events.md`](ui_events.md)
- Contributing: [`CONTRIBUTING.md`](../CONTRIBUTING.md)
- Security policy: [`SECURITY.md`](../SECURITY.md)
