# FAQ

Start here: [`docs/getting-started.md`](getting-started.md).

This page answers common questions from first-time users. For the authoritative CLI/TUI command list, run `/help` inside the app (implemented in `abstractcode/react_shell.py::_show_help`).

## What is AbstractCode?

AbstractCode is a **durable terminal TUI** for running agentic coding sessions on the AbstractFramework stack:
- **AbstractRuntime**: durable execution (runs, ledger, waits, artifacts)
- **AbstractAgent**: built-in agents (`react`, `memact`, `codeact`)
- **AbstractCore**: provider/model abstraction + tool definitions

Evidence: runtime wiring and agent selection in `abstractcode/react_shell.py` and `abstractcode/cli.py`.

## Is AbstractCode the runtime or the gateway?

No. AbstractCode is a **host UI**:
- The TUI runs a local runtime (`create_local_runtime(...)`) by default.
- The web app (`web/`) is a thin client that talks to **AbstractGateway** under `/api/gateway/*`.

Evidence:
- Local runtime creation: `abstractcode/react_shell.py`
- Gateway HTTP client: `abstractcode/gateway_cli.py`
- Web gateway client: `web/src/lib/gateway_client.ts`

## How do I install it?

```bash
pip install abstractcode
```

Optional (to run VisualFlow locally via `abstractcode flow ...`):

```bash
pip install "abstractcode[flow]"
```

Evidence: extras in `pyproject.toml` and flow implementation in `abstractcode/flow_cli.py`.

## How do I choose provider/model (Ollama, OpenAI-compatible, …)?

Use `--provider` and `--model` (and `--base-url` when needed):

```bash
abstractcode --provider ollama --model qwen3:1.7b-q4_K_M
abstractcode --provider openai --base-url http://127.0.0.1:1234/v1 --model qwen/qwen3-next-80b
```

Evidence: CLI args in `abstractcode/cli.py`.

## Where does it store state? Can I disable persistence?

By default AbstractCode persists to:
- state file: `~/.abstractcode/state.json`
- durable stores: `~/.abstractcode/state.d/`
- saved settings: `~/.abstractcode/state.config.json`

Disable persistence:

```bash
abstractcode --no-state
```

Evidence: store selection + config persistence in `abstractcode/react_shell.py`.

## Why does it keep asking to approve tool calls?

Tool execution is **approval-gated by default**. This is a durability + safety boundary:
- runtime emits a durable wait for tool calls
- the host prompts you
- tools run locally (or via MCP) and results are fed back into the run

Skip prompts (unsafe):
- CLI: `--auto-approve`
- TUI: `/auto-accept`

Evidence:
- Runtime tool executor: `PassthroughToolExecutor(mode=\"approval_required\")` in `abstractcode/react_shell.py`
- Tool execution: `MappingToolExecutor.from_tools(...)` in `abstractcode/react_shell.py`

## What are Plan and Review modes?

- **Plan mode**: the agent produces a short TODO list before acting (`--plan` or `/plan on`).
- **Review mode**: the agent runs a self-check pass before concluding (`--review` / `--no-review`, or `/review ...`).

Evidence:
- CLI flags: `abstractcode/cli.py`
- TUI commands: `/help` in `abstractcode/react_shell.py::_show_help`

## How do I restrict which tools the agent may use?

Use `/tools` to manage the allowlist:
- `/tools only <name...>`
- `/tools enable <name...>`
- `/tools disable <name...>`
- `/tools reset`

Evidence: implementation in `abstractcode/react_shell.py::_handle_tools()`.

## How do I attach files to a prompt?

Mention files as `@path` in your prompt:

```text
Explain @abstractcode/cli.py and @docs/architecture.md
```

Notes:
- Workspace-relative paths are resolved against a workspace root (see below).
- Absolute paths are also accepted (best-effort), but `@…` tokens stop at whitespace.

Evidence:
- Mention parsing: `abstractcode/file_mentions.py::extract_at_file_mentions()`
- Attachment resolution: `abstractcode/react_shell.py::_resolve_attachment_file()`

### What is the workspace root? Can I mount other directories?

Workspace root:
- Always the current working directory at launch.

Optional mounts:
- `ABSTRACTCODE_WORKSPACE_MOUNTS` (preferred) or `ABSTRACTGATEWAY_WORKSPACE_MOUNTS` (compat)
  - newline-separated `name=/abs/path`

Session-only mounts:
- `/whitelist <dir...>` or `/whitelist name=/abs/dir ...`
- `/blacklist <path...>` and `/blacklist reset`

Evidence: `abstractcode/file_mentions.py::default_workspace_root()` and `abstractcode/file_mentions.py::default_workspace_mounts()`.

## What’s the difference between `/logs runtime` and `/logs provider`?

- `/logs runtime`: durable step trace for LLM/tool calls (runtime perspective)
- `/logs provider`: provider wire request/response (what was sent/received)

Evidence: commands are exposed in `abstractcode/react_shell.py::_show_help` and implemented in `abstractcode/react_shell.py`.

## How do workflows work here?

There are several workflow-related modes:

- Local VisualFlow runs: `abstractcode flow ...` (requires `abstractcode[flow]`)
- Workflow as an agent: `abstractcode --agent <flow_ref>` (implements `abstractcode.agent.v1`)
- Gateway control-plane: `abstractcode gateway ...`
- Gateway bundle management: `abstractcode workflow ...`

Details: [`docs/workflows.md`](workflows.md).

Evidence:
- Local flows: `abstractcode/flow_cli.py`
- Workflow agent: `abstractcode/workflow_agent.py`
- Gateway + bundles: `abstractcode/gateway_cli.py` and `abstractcode/workflow_cli.py`

## Why doesn’t the web app build when I clone only this repo?

The web app currently imports shared UI components via Vite path aliases pointing at a sibling `abstractuic/` repo.

Options:
- deploy the prebuilt static output from `web/dist/` (if it matches your needs), or
- clone/build with the sibling UI repo present.

Evidence:
- Vite aliases: `web/vite.config.ts`
- Imports: `web/src/ui/app.tsx` and `web/src/main.tsx`

## How do I enable the GPU meter in the footer?

The TUI can poll AbstractGateway:
- endpoint: `GET /api/gateway/host/metrics/gpu`
- command: `/gpu [status|on|off]`

Enable:
- in-session: `/gpu on`
- env: `ABSTRACTCODE_GPU_MONITOR=1` (or `auto`)

Evidence: `abstractcode/react_shell.py::_handle_gpu()` and `abstractcode/react_shell.py::_fetch_gateway_gpu_utilization_pct()`.

## I’m on MemAct: why do I have `/memory` but not on ReAct/CodeAct?

`/memory` is MemAct-specific (it shows MemAct “Active Memory”).

Evidence: command list in `abstractcode/react_shell.py::_show_help` and agent wiring in `abstractcode/react_shell.py`.
