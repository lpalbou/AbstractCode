# API and integration points

Start here: [`docs/getting-started.md`](getting-started.md).

AbstractCode is primarily a **CLI/TUI application**. This page documents the integration points that are most useful for external users:
- the CLI surface (stable entrypoint for scripting)
- minimal Python API (entrypoint function)
- workflow interface contracts (`abstractcode.agent.v1`)
- workflow-driven UI events (`abstract.*`)
- gateway interaction surface (what this repo’s clients call)

Status: **pre-alpha** — interfaces may evolve, but changes should be reflected in the docs and [`CHANGELOG.md`](../CHANGELOG.md).

## 1) CLI (primary interface)

The CLI entrypoint is the published script:

```bash
abstractcode --help
```

Note: workflow-related subcommands are dispatched by the first argument:
- `abstractcode flow --help`
- `abstractcode gateway --help`
- `abstractcode workflow --help`

Key modes:
- interactive TUI: `abstractcode ...`
- one-shot: `abstractcode --prompt "..." ...`
- local VisualFlow runs: `abstractcode flow ...` (requires `abstractcode[flow]`)
- gateway control-plane: `abstractcode gateway ...`
- gateway bundle management: `abstractcode workflow ...`

Evidence: `abstractcode/cli.py`.

## 2) Python API (minimal)

The supported Python entrypoint is:

```python
from abstractcode import main

raise SystemExit(main(["--help"]))
```

Evidence: `abstractcode/__init__.py` defines `main(argv=None)` and delegates to `abstractcode/cli.py`.

You can also run the module:

```bash
python -m abstractcode --help
```

Evidence: `abstractcode/__main__.py`.

## 2b) Prompt caching

AbstractCode exposes prompt caching via the CLI/TUI (`--prompt-cache` and `/cache`). Internally it is configured through run vars and injected into LLM call params:

- Run vars: `run.vars["_runtime"]["prompt_cache"]`
  - `false`: disable injection
  - `true`: enable injection (key is derived from session/run metadata)
  - `{"enabled": true, "key": "mykey"}`: enable and force a specific `prompt_cache_key`

- LLM params: `prompt_cache_key`
  - If a caller explicitly sets `prompt_cache_key` in params, that value wins and runtime injection is skipped.
  - Passing `prompt_cache_key=None`/empty disables caching for that call.

Local-control-plane note: when the selected backend reports prompt-cache module support, the runtime maintains a 3-compartment cache (`system | tools | history`) using AbstractCore’s prompt-cache control plane (`prepare_modules` / `fork` / `update`). For GGUF this depends on the current model's llama.cpp chat format having an exact cached renderer; otherwise the backend remains keyed-only.

## 3) Workflow agent contract: `abstractcode.agent.v1`

AbstractCode can run a VisualFlow workflow as an agent:

```bash
abstractcode --agent /path/to/workflow.json --provider ollama --model qwen3:1.7b-q4_K_M
```

Contract summary (required):
- `interfaces: ["abstractcode.agent.v1"]`
- **On Flow Start** outputs: `provider`, `model`, `prompt`, `tools`
- **On Flow End** inputs: `response`, `success`, `meta`

Details: [`docs/workflows.md`](workflows.md).

Evidence:
- contract validation/scaffold: `abstractcode/workflow_agent.py` (`_apply_abstractcode_agent_v1_scaffold`, `_validate_abstractcode_agent_v1`)
- run variable injection: `abstractcode/workflow_agent.py::WorkflowAgent.start()`

## 4) Workflow-driven UI events (`abstract.*`)

Workflows can emit durable UI hints via `emit_event` and have hosts render them:
- `abstract.status`
- `abstract.message`
- `abstract.tool_execution`
- `abstract.tool_result`

Details: [`docs/ui_events.md`](ui_events.md).

Evidence: ledger subscription + normalization in `abstractcode/workflow_agent.py::_subscribe_ui_events()`.

## 5) Gateway interaction surface (used by this repo)

This repo includes:
- a Python gateway client used by `abstractcode gateway ...` and `abstractcode workflow ...`: `abstractcode/gateway_cli.py`
- a browser gateway client used by the web app: `web/src/lib/gateway_client.ts`

They both call gateway endpoints under `/api/gateway/*`, but with different focus:
- Python client: runs, ledger polling, durable commands, bundles, KG (`/runs/*`, `/commands`, `/bundles/*`, `/kg/query`)
- Browser client: discovery + files + streaming views (`/discovery/*`, `/files/*`, `/runs/*`, etc.)

Note: AbstractGateway is a separate component; its full API surface is defined in its own project.
