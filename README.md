# AbstractCode

**A clean terminal CLI for multi-agent agentic coding**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.8+](https://img.shields.io/badge/python-3.8+-blue.svg)](https://www.python.org/downloads/)

---

## Status

AbstractCode is under active development. A minimal interactive shell exists to support manual testing of AbstractAgent workflows.

Note: the PyPI release may lag behind the monorepo. For the latest development version, install from source.

## What is AbstractCode?

AbstractCode is a clean terminal CLI for multi-agent agentic coding, similar to Claude Code, Codex, and Gemini CLI. It leverages the powerful Abstract Framework ecosystem to provide seamless AI-powered coding assistance directly in your terminal.

## The Abstract Framework

AbstractCode is built on top of the Abstract Framework, a comprehensive suite of tools for AI-powered development:

- **[AbstractCore](https://github.com/lpalbou/abstractcore)** - Unified interface for multiple LLM providers
- **[AbstractRuntime](https://github.com/lpalbou/abstractruntime)** - Runtime environment for AI agents
- **[AbstractAgent](https://github.com/lpalbou/abstractagent)** - Multi-agent orchestration and coordination

## Features (Coming Soon)

- 🤖 **Multi-Agent Coding** - Coordinate multiple AI agents for complex coding tasks
- 🔌 **Provider Agnostic** - Works with OpenAI, Anthropic, Ollama, and more
- 💻 **Terminal Native** - Clean CLI interface for seamless workflow integration
- 🎯 **Context Aware** - Understands your codebase and project structure
- 🔄 **Iterative Development** - Collaborative coding with AI assistance
- 🌐 **Offline Capable** - Works with local models via Ollama

## Installation

```bash
pip install abstractcode
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

# Limit agent iterations per task (default: 20)
abstractcode --max-iterations 25
```

Notes:
- Run resume state is stored next to the state file in `*.d/`.
- Conversation history is stored in the run state (`RunState.vars["context"]["messages"]`) inside `*.d/`, and AbstractCode keeps the state file pointing at the most recent run so restarts can reload context.
- In the interactive shell, commands are slash-prefixed (e.g. `/help`, `/status`, `/history`, `/task ...`).

## Development (Monorepo)

From the monorepo root:

```bash
pip install -e ./abstractcore -e ./abstractruntime -e ./abstractagent -e ./abstractcode
abstractcode --help
```

## Requirements

- Python 3.8 or higher
- AbstractCore
- AbstractRuntime
- AbstractAgent

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

AbstractCode provides a curated set of 8 tools for coding tasks:

| Tool | Description |
|------|-------------|
| `list_files` | Find and list files using glob patterns (case-insensitive) |
| `search_files` | Search for text patterns inside files using regex |
| `read_file` | Read file contents with optional line range |
| `write_file` | Write content to files, creating directories as needed |
| `edit_file` | Edit files by replacing text patterns (supports regex, line ranges, preview mode) |
| `execute_command` | Execute shell commands with security controls |
| `web_search` | Search the web via DuckDuckGo (no API key required) |
| `self_improve` | Log improvement suggestions for later review |

Additional tools are available via AbstractAgent for specialized use cases (execute_python, fetch_url).