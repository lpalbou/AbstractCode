# Workflows (VisualFlow / bundles / gateway)

Start here: [`docs/getting-started.md`](getting-started.md).

AbstractCode supports these workflow-related modes:

1) **Run VisualFlow locally**: `abstractcode flow ...` (requires `abstractflow`; install `abstractcode[flow]`)
2) **Run a workflow *as an agent***: `abstractcode --agent <flow_ref>` (compiles via `abstractruntime.visualflow_compiler`)
3) **Run/observe via AbstractGateway**: `abstractcode gateway ...`
4) **Manage gateway workflow bundles**: `abstractcode workflow ...` (upload/list/remove `.flow` bundles)
5) **Gateway-first web host UI**: `web/` (runs against `/api/gateway/*`)

Related:
- CLI: [`docs/cli.md`](cli.md)
- UI events contract: [`docs/ui_events.md`](ui_events.md)
- Web deployment: [`docs/deployment-web.md`](deployment-web.md)

## 1) Local workflows (`abstractcode flow ...`)

Install:

```bash
pip install "abstractcode[flow]"
```

Run a flow by id (from a flows dir) or by path to a VisualFlow `.json`:

```bash
abstractcode flow run <flow_id_or_path> --flows-dir /path/to/flows --param query="who are you?"
```

Other useful commands:
- `abstractcode flow runs` (list recent runs)
- `abstractcode flow attach <run_id>` (set the current flow ref)
- `abstractcode flow emit ...` (advanced: resume a wait_key / emit an event)

Durability:
- flow run reference file: `~/.abstractcode/flow_state.json` (override with `--flow-state-file` or `ABSTRACTCODE_FLOW_STATE_FILE`)
- durable stores directory: `~/.abstractcode/flow_state.d/`

Evidence:
- Defaults: `abstractcode/flow_cli.py::default_flow_state_file()` and `abstractcode/flow_cli.py::_flow_store_dir()`.

## 2) Workflow agent mode (`--agent <flow_ref>`)

Run a workflow as an interactive “agent” inside the TUI:

```bash
abstractcode --agent /path/to/workflow.json --provider ollama --model qwen3:1.7b-q4_K_M
```

Supported `flow_ref` forms (resolved in `abstractcode/workflow_agent.py::resolve_visual_flow()`):
- VisualFlow id (from `--flows-dir`/`ABSTRACTFLOW_FLOWS_DIR`)
- VisualFlow `name`
- path to `.json`
- path/ref to a bundled `.flow` (zip) file
- bundle ref `bundle_id[@version]` and optional `bundle_id[@version]:flow_id`

### `abstractcode.agent.v1` contract (implemented)

AbstractCode validates (and best-effort scaffolds) the interface:
- Flow must declare `interfaces: ["abstractcode.agent.v1"]`
- Required pins:
  - **On Flow Start** outputs: `provider`, `model`, `prompt`, `tools`
  - **On Flow End** inputs: `response`, `success`, `meta`

Evidence: `_apply_abstractcode_agent_v1_scaffold()` + `_validate_abstractcode_agent_v1()` in `abstractcode/workflow_agent.py`.

### Variables provided to the workflow

At run start, AbstractCode passes (at least):
- `vars.prompt`: the user task text
- `vars.provider`, `vars.model`: from the runtime config (`--provider/--model`)
- `vars.tools`: the session allowlist (empty list if not set)
- `vars.context.messages`: the conversation history (host-managed)
- `vars.context.attachments`: (optional) attachment refs from `@file` mentions
- `vars._limits`: max_iterations/max_tokens/etc (host limits)

Evidence: `abstractcode/workflow_agent.py::WorkflowAgent.start()`.

### Outputs surfaced back to the host

When the workflow completes, AbstractCode:
- extracts `response` text (best-effort) from `RunState.output`
- attaches `meta` to the assistant message metadata as `workflow_meta`
- attaches optional `scratchpad` as `workflow_scratchpad`
- attaches `success` as `workflow_success`

Evidence: `abstractcode/workflow_agent.py::WorkflowAgent.step()`.

## 3) AbstractGateway mode (`abstractcode gateway ...`)

The `gateway` subcommand is a thin HTTP control-plane client:

```bash
abstractcode gateway run <flow_id> --input-json '{"prompt":"hello"}'
abstractcode gateway attach <run_id>
abstractcode gateway kg --scope session
```

Evidence: argument parsing in `abstractcode/cli.py` and HTTP client in `abstractcode/gateway_cli.py`.

## Workflow bundles on a gateway (`abstractcode workflow ...`)

This subcommand manages **WorkflowBundle** `.flow` files on a running AbstractGateway.

Examples:

```bash
# Upload a bundle
abstractcode workflow install /path/to/bundle.flow --overwrite

# List entrypoints (optionally filter to workflows that implement the agent interface)
abstractcode workflow list --interface abstractcode.agent.v1

# Inspect and remove
abstractcode workflow info my-bundle@1.2.3
abstractcode workflow remove my-bundle@1.2.3

# (Optional) Deprecate / undeprecate bundles on the gateway (hide + block launch)
abstractcode workflow deprecate my-bundle --reason "superseded"
abstractcode workflow undeprecate my-bundle
```

Gateway config:
- flags: `--gateway-url`, `--gateway-token`
- env: `ABSTRACTCODE_GATEWAY_URL`, `ABSTRACTCODE_GATEWAY_TOKEN`

Evidence: CLI parser in `abstractcode/cli.py::build_workflow_parser()` and implementation in `abstractcode/workflow_cli.py`.

## Workflow-driven UI events

Workflows can emit reserved events that update AbstractCode’s UX (status/messages/tool blocks).

Contract: [`docs/ui_events.md`](ui_events.md).
