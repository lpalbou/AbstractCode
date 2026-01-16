# AbstractCode

Terminal TUI for the AbstractFramework:
- run **agents** (ReAct / CodeAct)
- run **workflows** authored in AbstractFlow (VisualFlow JSON)
- keep everything **durable** via AbstractRuntime (runs, ledger, artifacts)

## Install

```bash
pip install abstractcode
```

To run AbstractFlow workflows from AbstractCode:

```bash
pip install "abstractcode[flow]"
```

## Start the TUI (Agents)

```bash
abstractcode --provider lmstudio --model qwen/qwen3-next-80b
```

Interaction model:
- commands are slash-prefixed (`/help`)
- any non-command line starts a task (same as `/task ...`)

### Useful commands
- `/status` (run status)
- `/auto-accept on|off` (tool approvals)
- `/max-tokens N` (or `-1` auto-detect)
- `/compact [light|standard|heavy] [--preserve N] [focus...]` (durable compaction)
- `/spans`, `/expand <span> [--show] [--into-context]` (provenance recall)
- `/recall [--since ISO] [--until ISO] [--tag k=v] [--q text] [--into-context]`
- `/snapshot save|load|list`

### Persistence
By default, AbstractCode uses `~/.abstractcode/state.json` and stores the durable data in `~/.abstractcode/state.d/`.
- disable persistence: `abstractcode --no-state`
- override path: `ABSTRACTCODE_STATE_FILE=... abstractcode`

## One-shot mode (`--prompt`)
Run a single prompt and exit (useful for quick manual tests and scripting):

```bash
abstractcode --provider lmstudio --model qwen/qwen3-next-80b --prompt "What is this @abstractcode/abstractcode/cli.py about?"
```

Notes:
- `@path` mentions attach workspace-relative files (stored in the run `ArtifactStore`; passed to the LLM via artifact-backed media).
- Tool calls are approval-gated by default; use `--auto-approve` to run non-interactively.

## Workflow Agents (VisualFlow as `--agent`)

AbstractCode can run an AbstractFlow VisualFlow workflow *as an agent* (instead of using the built-in `react|codeact|memact` agents).

### Requirements (RunnableFlow (v1), `abstractcode.agent.v1`)
- The workflow JSON must declare: `interfaces: ["abstractcode.agent.v1"]`
- The workflow must expose these pins:
  - `On Flow Start`: output pins (required):
    - `provider` (type `provider`)
    - `model` (type `model`)
    - `prompt` (type `string`)
  - `On Flow End`: input pins (required):
    - `response` (type `string`)
    - `success` (type `boolean`)
    - `meta` (type `object`)

Recommended (optional) pins:
- `On Flow Start`: `use_context` (boolean), `context` (object), `system` (string), `tools` (tools), `max_iterations` (number), `max_in_tokens` (number), `temperature` (number), `seed` (number), `resp_schema` (object)
- `On Flow End`: `scratchpad` (object)

### Authoring in the AbstractFlow visual editor
- **Mark the interface**: click `📂 Load` → select the workflow → in the right preview panel find **Interfaces** → click the ✏️ icon → enable **RunnableFlow (v1)** → **Save Interfaces**
- **Pins are scaffolded automatically**: when the interface is enabled, AbstractFlow will ensure `On Flow Start` and `On Flow End` have the required pins (and may add common optional pins). You can still add/remove optional pins as needed.

Tip: an example workflow is shipped at `abstractflow/web/flows/acagent01.json` (implements the interface).

### What is `meta` and how do I use it?
`On Flow End.meta` is a **required JSON object** for host-facing metadata (usage, trace ids, warnings, raw provider info, etc.).

It is intentionally **not strictly validated** today (the host treats it as opaque JSON). To make workflows portable and predictable across hosts, we recommend using a small “envelope” shape:

```json
{
  "schema": "abstractcode.agent.v1.meta",
  "version": 1,
  "provider": "lmstudio",
  "model": "qwen/qwen3-next-80b",
  "usage": { "input_tokens": 123, "output_tokens": 456 },
  "trace": { "trace_id": "..." },
  "warnings": ["..."],
  "debug": {}
}
```

Hosts should treat unknown fields as allowed and ignore what they don’t understand (forward-compatible).

Typical ways to produce it inside a workflow:
- Wire `LLM Call.meta` (object) → `On Flow End.meta`
- Wire `Agent.meta` (object) → `On Flow End.meta`
- Or build your own object and wire it into `meta`

When present, AbstractCode attaches it to the assistant message metadata as `workflow_meta`.

### Workflow-driven status updates (footer / live UX)
Inside a workflow, you can update AbstractCode’s footer status text by emitting the reserved event:
- Add an **Emit Event** node
- Set **name** to `abstract.status` (preferred; `abstractcode.status` is a deprecated alias)
- Set **payload** to:
  - a string (e.g. `"Enrich Query..."`), or
  - an object like `{ "text": "Enrich Query...", "duration": -1 }`

`duration` is seconds:
- default: `-1` (sticky)
- if `> 0`: auto-clears after the timeout unless superseded by a newer status

Example workflow: `abstractflow/web/flows/acagent_status_demo.json` (3 status updates, each separated by a 2s Delay).

### Workflow-driven UI events (messages + tool UX)
Workflows can also emit additional reserved events for host UX:
- `abstract.message`: show a message/notification (payload is a string or `{text, level?, title?}`)
- `abstract.tool_execution`: render a tool-call block (payload is a tool call object or a list)
  - recommended shape: `{name, arguments, call_id?}`
- `abstract.tool_result`: render a tool-result block (payload is a tool result object or a list)
  - recommended shape: `{name, call_id?, success?, output?, error?}`

Full contract (recommended for integrators): `docs/ui_events.md`

Example workflows:
- `abstractflow/web/flows/acagent_message_demo.json`
- `abstractflow/web/flows/acagent_tool_events_demo.json`

### Durable ask+wait (prompt the user and resume)
For workflows that need human input, you can:
- use the **Ask User** node (WAIT_USER), or
- use **Wait Event** with a `prompt` field (WAIT_EVENT), which is also durable and network-friendly.

Example workflow: `abstractflow/web/flows/acagent_ask_demo.json`

### Run
Use `--agent` with a workflow id/name (from the flows directory) or a direct JSON path:

```bash
abstractcode --agent acagent01
abstractcode --agent abstractflow/web/flows/acagent01.json

# If your flows are stored elsewhere:
ABSTRACTFLOW_FLOWS_DIR=/path/to/flows abstractcode --agent my_flow_id
```

## Run Workflows (AbstractFlow VisualFlow)

### From the CLI

```bash
abstractcode flow run <flow_id_or_path> [inputs...]
```

Inputs can be passed as flags (no JSON typing required):

```bash
abstractcode flow run abstractflow/web/flows/4e2f2329.json --query "who are you?"

abstractcode flow run abstractflow/web/flows/b3a9d7c1.json \
  --query "who are you?" \
  --max_web_search 15 \
  --max_fetch_url 50 \
  --follow_up_questions true
```

Other input options:

```bash
abstractcode flow run deep-research-pro --input-json-file params.json
abstractcode flow run deep-research-pro --param max_web_search=15 --param follow_up_questions=true
```

Tool approvals:
- approval-gated by default (type `a` once to approve all remaining calls for that run)
- auto-approve (unsafe): `--accept-tools` (alias `--auto-approve`)

Controls:

```bash
abstractcode flow resume
abstractcode flow pause
abstractcode flow resume-run
abstractcode flow cancel
```

Run discovery and event injection:

```bash
abstractcode flow runs
abstractcode flow attach <run_id>
abstractcode flow emit --name my_event --scope session --payload-json '{"k":"v"}'
```

Flow state:
- default: `~/.abstractcode/flow_state.json` (stores in `~/.abstractcode/flow_state.d/`)
- override with `ABSTRACTCODE_FLOW_STATE_FILE=...`
- flow discovery default dir: `ABSTRACTFLOW_FLOWS_DIR=...`

### From inside the TUI (keeps outputs in context)

```text
/flow run deep-research-pro --query "..." --max_web_search 10 --follow_up_questions true
```

`ANSWER_USER` outputs from the workflow are appended into the current conversation’s active context so you can continue the dialogue naturally.

## Development (Monorepo)

```bash
pip install -e ./abstractcore -e ./abstractruntime -e ./abstractagent -e ./abstractflow -e ./abstractcode
abstractcode --help
```

## Environment variables
- `ABSTRACTCODE_STATE_FILE`
- `ABSTRACTCODE_FLOW_STATE_FILE`
- `ABSTRACTFLOW_FLOWS_DIR`

## Default Tools

AbstractCode provides a curated set of 10 tools for coding tasks (ReAct agent):

| Tool | Description |
|------|-------------|
| `list_files` | Find and list files using glob patterns (case-insensitive) |
| `search_files` | Search for text patterns inside files using regex |
| `analyze_code` | Outline a Python/JS file (imports/classes/functions + line ranges) |
| `read_file` | Read file contents with optional line range |
| `write_file` | Write content to files, creating directories as needed |
| `edit_file` | Edit files by replacing text patterns (supports regex, line ranges, preview mode) |
| `execute_command` | Execute shell commands with security controls |
| `web_search` | Search the web via DuckDuckGo (no API key required) |
| `fetch_url` | Fetch a URL and return text/metadata (best-effort parsing) |
| `self_improve` | Log improvement suggestions for later review |

When running `--agent codeact`, AbstractCode exposes `execute_python` instead of the ReAct toolset.
