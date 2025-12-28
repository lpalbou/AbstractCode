# AbstractCode — Architecture (Current)

> Updated: 2025-12-27  
> Scope: this describes **what is implemented today** in this monorepo (no “future” design claims).

AbstractCode is a **host UX** (terminal app) for running AbstractFramework agents and (increasingly) workflows.
It owns terminal interaction, approvals, and persistence configuration; durable execution is owned by **AbstractRuntime**.

AbstractFlow workflow support is an **optional integration** (install via `abstractcode[flow]` in standalone packaging).

## Repository Layout

```
abstractcode/
  abstractcode/
    cli.py              # CLI entrypoint (argparse)
    react_shell.py      # Main interactive shell (ReAct/CodeAct)
    fullscreen_ui.py    # Full-screen prompt_toolkit UI (scrollback + status bar)
    recall.py           # /recall parsing + runtime ActiveContextPolicy bridge
    input_handler.py    # prompt_toolkit sessions (helper)
    flow_cli.py         # VisualFlow runner helpers (run/resume/pause/cancel)
```

## What AbstractCode Owns vs Uses

**AbstractCode owns**
- the terminal UX (`FullScreenUI`) and command routing (`ReactShell`)
- tool approval prompts (human-in-the-loop gate)
- filesystem layout for durable stores and snapshot persistence

**AbstractCode uses**
- **AbstractAgent**: `ReactAgent` / `CodeActAgent` workflows and the `BaseAgent` API surface
- **AbstractRuntime**: durable run state, waits, ledger, artifacts, memory recall/rehydration, snapshots
- **AbstractCore**: provider/model abstraction and tool schemas/call parsing (via runtime integration)

## Execution Model (Interactive Shell)

The current interactive app is `ReactShell` (`abstractcode/abstractcode/react_shell.py`).

### Agent selection
`abstractcode/abstractcode/cli.py` selects:
- `--agent react` → `abstractagent.agents.react.ReactAgent`
- `--agent codeact` → `abstractagent.agents.codeact.CodeActAgent`

### Durability (default-on)
If a state file is enabled (default: `~/.abstractcode/state.json`), `ReactShell` configures file-backed stores:
- `JsonFileRunStore` + `JsonlLedgerStore` in `STATEFILE_STEM.d/`
- `FileArtifactStore` in the same directory (large payloads + archived spans)
- snapshot store in `STATEFILE_STEM.d/snapshots/`

If `--no-state` is set, it uses in-memory stores only (cannot resume after quitting).

### Tool approvals (durable boundary)
AbstractCode gates tool execution by configuring the runtime with:
- `PassthroughToolExecutor(mode="approval_required")` as the runtime tool executor

This forces `EffectType.TOOL_CALLS` to pause with a durable `WAIT_EVENT` (details include `tool_calls`).
The CLI then:
1) prompts the user for approval (or auto-approves)
2) executes tools locally via `MappingToolExecutor.from_tools(...)`
3) resumes the run with the tool results payload

This keeps the durable state JSON-safe: tool *specs* can be persisted, tool *callables* never are.

### Waiting semantics
The run loop in `ReactShell._run_loop()` drives one step at a time (`agent.step()`), then handles waits:
- `WaitReason.USER` → prompt user and `agent.resume(...)`
- `WaitReason.EVENT` with `details.tool_calls` → approval + resume with tool results
- other waits → display wait info and return to the shell

## Memory UX (Runtime-owned)

AbstractCode’s memory commands are thin UX wrappers over runtime contracts:
- `/compact` triggers runtime compaction (archives spans in `ArtifactStore` and keeps provenance handles)
- `/spans`, `/expand`, `/recall` use `abstractruntime.memory.ActiveContextPolicy` via `abstractcode/abstractcode/recall.py`

This keeps “what memory means” consistent across hosts (CLI, web UI, etc.).

## Observability

AbstractCode reads durable execution artifacts from AbstractRuntime:
- `RunState.vars` for active context and runtime-owned metadata (`_runtime`)
- ledger entries via `LedgerStore` for step-by-step auditability
- artifacts via `ArtifactStore` for archived spans and large payloads

The full-screen UI (`abstractcode/abstractcode/fullscreen_ui.py`) is responsible only for rendering and input capture; it does not change execution semantics.

## Running AbstractFlow Workflows (Current)

AbstractCode also includes a small “host loop” for running `abstractflow` VisualFlow JSON outside the web UI:
- Entry (CLI): `abstractcode flow ...` in `abstractcode/abstractcode/cli.py`
- Entry (REPL): `/flow ...` in `abstractcode/abstractcode/react_shell.py`
- Driver: `abstractcode/abstractcode/flow_cli.py` (shared host loop)

Current behavior:
- Uses `abstractflow.visual.executor.create_visual_runner(...)` to compile and run the flow.
- Persists runs/ledger/artifacts to a **separate** state file (`~/.abstractcode/flow_state.json` by default) so it doesn’t interfere with agent session resume (`~/.abstractcode/state.json`).
- Handles `ASK_USER` via the host (CLI: `input()`, REPL: prompt_toolkit), renders `ANSWER_USER` output, and gates tool calls via runtime `PassthroughToolExecutor(mode="approval_required")`.
- REPL integration appends `ANSWER_USER` outputs into the active conversation context so users can keep iterating with the agent after a workflow step completes.
- For flow entry vars, the CLI accepts either JSON input (`--input-json`/`--input-json-file`) or ergonomic flags (`--query "..." --max_web_search 10`) which are coerced into a JSON-safe input dict.
- The CLI also includes lightweight run ops for portability: `flow runs`, `flow attach <run_id>`, and `flow emit ...` to inject custom events / resume event waits.
