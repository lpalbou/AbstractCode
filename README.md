# AbstractCode

**A clean terminal host for AbstractFramework agents and workflows**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.10+](https://img.shields.io/badge/python-3.10+-blue.svg)](https://www.python.org/downloads/)

---

## Status

AbstractCode is under active development. A minimal interactive shell exists to support manual testing of AbstractAgent workflows.

Note: the PyPI release may lag behind the monorepo. For the latest development version, install from source.

## What is AbstractCode?

AbstractCode is a terminal host for:
- **Agents** (ReAct / CodeAct) built on AbstractAgent + AbstractRuntime
- **Workflows** authored in AbstractFlow (VisualFlow JSON) and executed durably by AbstractRuntime

## The Abstract Framework

AbstractCode is built on top of the Abstract Framework, a comprehensive suite of tools for AI-powered development:

- **[AbstractCore](https://github.com/lpalbou/abstractcore)** - Unified interface for multiple LLM providers
- **[AbstractRuntime](https://github.com/lpalbou/abstractruntime)** - Runtime environment for AI agents
- **[AbstractAgent](https://github.com/lpalbou/abstractagent)** - Multi-agent orchestration and coordination
- **[AbstractFlow](https://github.com/lpalbou/AbstractFlow)** - Visual workflow authoring (VisualFlow JSON)

## Installation

```bash
pip install abstractcode
```

To run AbstractFlow workflows from AbstractCode:

```bash
pip install "abstractcode[flow]"
```

## Quick Start

```bash
# Show options
abstractcode --help

# Durable resume is enabled by default (state file: ~/.abstractcode/state.json)
# Override with:
ABSTRACTCODE_STATE_FILE=.abstractcode.state.json abstractcode

# Or disable persistence (in-memory only; cannot resume after quitting)
abstractcode --no-state

# Auto-approve tool calls (unsafe; bypasses interactive approvals)
abstractcode --auto-approve

# Limit agent iterations per task (default: 25)
abstractcode --max-iterations 25

# Run CodeAct instead of ReAct
abstractcode --agent codeact
```

Notes:
- Run resume state is stored next to the state file in `*.d/`.
- Conversation history is stored in the run state (`RunState.vars["context"]["messages"]`) inside `*.d/`, and AbstractCode keeps the state file pointing at the most recent run so restarts can reload context.
- In the interactive shell, commands are slash-prefixed (e.g. `/help`, `/status`, `/history`, `/task ...`).

## Run AbstractFlow Workflows (CLI)

Visual workflows authored in AbstractFlow are portable `VisualFlow` JSON files. AbstractCode can run them via:

```bash
abstractcode flow run <flow_id_or_path> [inputs...]
```

### Passing inputs (no JSON typing required)

Any unknown flags are treated as input variables, with basic type coercion:
- `true`/`false` → booleans
- numbers → ints/floats
- `{...}` / `[...]` → parsed JSON

Examples:

```bash
# Run by path, pass inputs as flags
abstractcode flow run abstractflow/web/flows/4e2f2329.json --query "who are you?"

# Deep research example
abstractcode flow run abstractflow/web/flows/b3a9d7c1.json \\
  --query "who are you?" \\
  --max_web_search 15 \\
  --max_fetch_url 50 \\
  --follow_up_questions true
```

Other input options:

```bash
# Provide inputs from a JSON file
abstractcode flow run deep-research-pro --input-json-file params.json

# Or repeatable key=value
abstractcode flow run deep-research-pro --param query="who are you?" --param max_web_search=15
```

### Tool approvals

By default, tool calls are approval-gated:
- choose `a` to approve all remaining tool calls for that run
- use `--accept-tools` (alias: `--auto-approve`) to auto-execute tools without prompts (unsafe)

### Resume / pause / cancel

```bash
abstractcode flow resume
abstractcode flow pause
abstractcode flow resume-run
abstractcode flow cancel
```

### State locations

- Agent shell state: `~/.abstractcode/state.json` (stores in `~/.abstractcode/state.d/`)
- Flow runner state: `~/.abstractcode/flow_state.json` (stores in `~/.abstractcode/flow_state.d/`)

Environment variables:
- `ABSTRACTCODE_STATE_FILE`
- `ABSTRACTCODE_FLOW_STATE_FILE`
- `ABSTRACTFLOW_FLOWS_DIR`

## Run Workflows Inside the REPL

In the interactive shell you can run flows without leaving the session:

```text
/flow run deep-research-pro --query "..." --max_web_search 10 --follow_up_questions true
```

`ANSWER_USER` outputs from the workflow are appended to the current conversation’s active context (durably when a run is loaded), so you can continue the dialogue naturally.

## Development (Monorepo)

From the monorepo root:

```bash
pip install -e ./abstractcore -e ./abstractruntime -e ./abstractagent -e ./abstractcode
abstractcode --help
```

## Requirements

- Python 3.10 or higher
- AbstractCore
- AbstractRuntime
- AbstractAgent
  - (Optional) AbstractFlow for `abstractcode flow ...`

## Documentation

Full documentation will be available at [abstractcore.ai](https://abstractcore.ai)

## Development Status

This project is in early development. Stay tuned for updates!

## Contributing

Contributions are welcome! Please check back soon for contribution guidelines.

## Contact

**Maintainer:** Laurent-Philippe Albou  
📧 Email: contact@abstractcore.ai  
🌐 Website: [abstractcore.ai](https://abstractcore.ai)

## License

MIT License - see LICENSE file for details.

---

**AbstractCode** - Multi-agent agentic coding in your terminal, powered by the Abstract Framework.

## Default Tools

AbstractCode provides a curated set of 9 tools for coding tasks (ReAct agent):

| Tool | Description |
|------|-------------|
| `list_files` | Find and list files using glob patterns (case-insensitive) |
| `search_files` | Search for text patterns inside files using regex |
| `read_file` | Read file contents with optional line range |
| `write_file` | Write content to files, creating directories as needed |
| `edit_file` | Edit files by replacing text patterns (supports regex, line ranges, preview mode) |
| `execute_command` | Execute shell commands with security controls |
| `web_search` | Search the web via DuckDuckGo (no API key required) |
| `fetch_url` | Fetch a URL and return text/metadata (best-effort parsing) |
| `self_improve` | Log improvement suggestions for later review |

When running `--agent codeact`, AbstractCode exposes `execute_python` instead of the ReAct toolset.
