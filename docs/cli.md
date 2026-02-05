# CLI / TUI reference

This document covers **how to run AbstractCode** (CLI/TUI) and how its **durability + approvals** work.

Start here: [`docs/getting-started.md`](getting-started.md).

See also: [`docs/faq.md`](faq.md).

Related:
- Architecture: [`docs/architecture.md`](architecture.md)
- Workflows: [`docs/workflows.md`](workflows.md)
- Web app: [`docs/web.md`](web.md)

## Install

See the top-level [`README.md`](../README.md#install).

## Run the TUI (interactive)

Minimum:

```bash
abstractcode --provider ollama --model qwen3:1.7b-q4_K_M
```

Notes:
- Full CLI flags live in `abstractcode/cli.py` (see `abstractcode --help`).
- Inside the app, run `/help` for the authoritative command list (implemented in `abstractcode/react_shell.py::_show_help`).

## One-shot mode (`--prompt`)

Run a single task and exit (useful for scripting / quick checks):

```bash
abstractcode --provider ollama --model qwen3:1.7b-q4_K_M --prompt "Summarize @README.md"
```

Evidence:
- One-shot driver: `abstractcode/cli.py::_run_one_shot_prompt()`
- `@file` mention parsing: `abstractcode/file_mentions.py`

## Key commands (TUI)

Use `/help` for the full, authoritative list. Common commands:
- `/status` (current run status)
- `/logs runtime` and `/logs provider` (durable tracing and provider wire logs)
- `/tools`, `/mcp`, `/executor` (tool allowlist + MCP servers + where tools execute)
- `/snapshot save|load|list` (snapshots)
- `/flow ...` (run local workflows inside the REPL; requires `abstractcode[flow]`)
- `/gpu [status|on|off]` (optional gateway GPU meter)

## Tool approvals (default-on)

By default, tool calls are **paused at a durable boundary** and require approval:
- CLI flag: `--auto-approve` (unsafe; disables prompts)
- TUI command: `/auto-accept` (persists when state is enabled)

Evidence:
- Runtime tool executor is created as `PassthroughToolExecutor(mode="approval_required")` in `abstractcode/react_shell.py`.
- Local execution after approval uses `MappingToolExecutor.from_tools(...)` in `abstractcode/react_shell.py`.

## Persistence (durable runs)

Default state file:
- `~/.abstractcode/state.json` (override with `--state-file` or `ABSTRACTCODE_STATE_FILE`)

When state is enabled, AbstractCode writes:
- durable stores directory: `~/.abstractcode/state.d/` (derived from `state.json` → `state.d/`)
- persisted settings: `~/.abstractcode/state.config.json`

Disable persistence:

```bash
abstractcode --no-state
```

Evidence:
- Store dir + stores selection: `abstractcode/react_shell.py` (file-backed `JsonFileRunStore` + `JsonlLedgerStore` vs in-memory).
- Config file load/save: `abstractcode/react_shell.py::_load_config()` / `_save_config()`.

## Attachments (`@file`) and workspace roots

You can attach local/project files by mentioning them in a prompt:

```text
Explain @abstractcode/cli.py and @docs/architecture.md
```

Workspace resolution:
- Default workspace root is the current working directory.
- Optional named mounts via `ABSTRACTCODE_WORKSPACE_MOUNTS` (newline-separated `name=/abs/path`).

Size limit:
- Default max attachment size is **25 MB**.
- Override via `ABSTRACTCODE_MAX_ATTACHMENT_BYTES` (or `ABSTRACTGATEWAY_MAX_ATTACHMENT_BYTES`).

Evidence: `abstractcode/file_mentions.py` + `abstractcode/react_shell.py::_ingest_attachments()`.

## MCP (remote tool execution)

AbstractCode can discover tools from (and execute tools on) remote MCP servers.

Commands (see `/help` for full syntax):
- `/mcp add ...`, `/mcp sync`, `/mcp list`, `/mcp remove`
- `/executor use <server_id>` to make tools run remotely by default

Tool naming:
- After sync, MCP tools are exposed as `mcp::<server_id>::<tool_name>` (see `/help` and `abstractcode/react_shell.py`).

## Gateway integration (GPU meter)

The TUI can poll AbstractGateway for GPU utilization and display a small meter in the footer:
- command: `/gpu [status|on|off]`
- endpoint: `GET /api/gateway/host/metrics/gpu`

Enable:
- in-session: `/gpu on`
- env: `ABSTRACTCODE_GPU_MONITOR=1` (or `auto` to enable when a gateway URL/token is configured)

Evidence: `abstractcode/react_shell.py::_gpu_monitor_enabled_from_env()` and `abstractcode/react_shell.py::_fetch_gateway_gpu_utilization_pct()`.

## Environment variables (CLI + TUI)

Common:
- `ABSTRACTCODE_AGENT` (default agent selector; same as `--agent`)
- `ABSTRACTCODE_BASE_URL` (provider base URL; same as `--base-url`)
- `ABSTRACTCODE_STATE_FILE` (default state file)
- `ABSTRACTCODE_MAX_ITERATIONS`, `ABSTRACTCODE_MAX_TOKENS` (CLI defaults)

Workspace/attachments:
- `ABSTRACTCODE_WORKSPACE_MOUNTS`
- `ABSTRACTCODE_MAX_ATTACHMENT_BYTES`

Gateway (for `/gpu` and `abstractcode gateway|workflow` commands):
- `ABSTRACTCODE_GATEWAY_URL`, `ABSTRACTCODE_GATEWAY_TOKEN`

Themes:
- `ABSTRACTCODE_THEME`
- `ABSTRACTCODE_THEME_PRIMARY`, `ABSTRACTCODE_THEME_SECONDARY`, `ABSTRACTCODE_THEME_SURFACE`, `ABSTRACTCODE_THEME_MUTED`

Evidence: `abstractcode/cli.py`, `abstractcode/react_shell.py`, `abstractcode/file_mentions.py`, `abstractcode/theme.py`.
