# Changelog

All notable changes to AbstractCode will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- **Tool result visibility**: increased the default tool observation preview to **1000 characters** (was 120) so small-but-critical outputs (e.g., exit codes, working directories) are not silently truncated in the UI.
- **Web search reliability**: add `ddgs` as a dependency so the default `web_search` tool works without requiring manual installs.

### Changed
- **`/clear`**: now clears the screen (UI output) in addition to clearing in-memory conversation context.
- **`/memorize`**: the memory-note command is now **Memorize** (consistent UX term) to avoid ambiguity with span tagging.
- **`/recall`**: richer filtering and rehydration controls:
  - Added `--tags-mode all|any`, repeatable `--user NAME`, and repeatable `--location LOC`.
  - Repeating `--tag k=v` now builds multi-value tags (e.g. `--tag person=alice --tag person=bob`).
  - `--into-context` now also rehydrates matching `memory_note` spans as a synthetic system message (`[MEMORY NOTE] ...`).

### Removed
- **`/new`, `/reset`**: removed alias commands (they were identical to `/clear`). Use `/clear`.

## [0.2.0] - 2025-12-17

### Initial Release

AbstractCode is an interactive terminal CLI for multi-agent agentic coding, providing a clean and powerful interface for AI-assisted development workflows.

#### Core Features

**Interactive Terminal Interface**
- Full-screen terminal UI built with prompt_toolkit featuring scrollable output, ANSI color support, and mouse interaction
- Clean command-line interface with slash-prefixed commands (`/help`, `/status`, `/task`, etc.)
- Real-time status bar showing provider, model, and context token usage
- Animated spinner with visual feedback during agent reasoning
- Multi-line input support with command history and autocomplete

**Multi-Agent Support**
- React agent with thought-action-observation reasoning loops
- CodeAct agent with Python code execution capabilities
- Configurable iteration limits (default: 25) and context tokens (default: 32768)
- Multiple LLM provider support (Ollama, OpenAI, and more via AbstractCore)
- Dynamic model selection with per-provider configuration

**Built-in Tool Suite**
- `list_files` - Find and list files using glob patterns
- `search_files` - Search file contents with regex patterns
- `read_file` - Read files with optional line range selection
- `write_file` - Write to files with automatic directory creation
- `edit_file` - Edit files using regex or line-based replacements
- `execute_command` - Execute shell commands with security gating
- `web_search` - Search the web via DuckDuckGo (no API key required)
- `fetch_url` - Fetch and process web content

**State Management & Persistence**
- Durable file-backed state with JSON storage (`~/.abstractcode/state.json`)
- Directory-based stores for run, ledger, and snapshot persistence
- Session resumption with conversation history restoration
- Named snapshots for saving and loading specific run states
- Optional in-memory mode for ephemeral sessions

**Security & Safety**
- Interactive tool approval with detailed argument preview
- Per-tool approval flow with yes/no/all/edit/quit options
- Argument editing in JSON format before execution
- Double-confirmation required for shell command execution
- Session-based "approve all" mode with persistence
- Optional auto-approve mode for non-interactive use

**Context & Memory Management**
- Conversation history tracking with `/history` command
- Memory usage breakdown by component (`/memory` command)
- Intelligent conversation compaction with three modes:
  - Light compression (minimal reduction)
  - Standard compression (balanced approach)
  - Heavy compression (aggressive reduction)
- Configurable message preservation for recent context
- Focus-based summarization to maintain topic coherence

**Configuration & Customization**
- Persistent configuration file (`*.config.json`) for saved settings
- Environment variables for default agent type, state file location, and limits
- CLI arguments for provider, model, iterations, tokens, and behavior
- Runtime commands for adjusting max tokens, max messages, and auto-approve
- Color output with `NO_COLOR` environment variable support

**Interactive Commands**

Task Management:
- `/task <description>` - Start a new task
- `/resume` - Resume the last saved or waiting run
- `/clear` (aliases: `/reset`, `/new`) - Clear memory and start fresh

Information & Status:
- `/help` - Display all available commands
- `/tools` - List available tools with descriptions
- `/status` - Show current run ID, workflow, status, and waiting reason
- `/history [N]` - Display recent conversation history
- `/memory` - Show token usage breakdown

Configuration:
- `/auto-accept [on|off]` - Toggle auto-approve for tool execution
- `/max-tokens [N]` - Show or set maximum context tokens (-1 for auto-detection)
- `/max-messages [N]` - Show or set maximum history messages
- `/compact [mode] [--preserve N]` - Compress conversation with configurable preservation

Snapshots:
- `/snapshot save <name>` - Save current run state as named snapshot
- `/snapshot load <name>` - Load a saved snapshot by name
- `/snapshot list` - List all available snapshots

**Keyboard & Mouse Controls**
- Enter - Submit input
- Up/Down arrows - Navigate command history or completion menu
- Page Up/Page Down - Scroll output area
- Home/End - Jump to top or bottom of output
- Ctrl+Up/Ctrl+Down - Smooth scroll output
- Ctrl+L - Clear output area
- Ctrl+C/Ctrl+D - Exit application
- Mouse wheel - Scroll output area
- Mouse click - Position cursor in input

**Technical Architecture**
- Thread-safe multi-threaded design with worker, spinner, and render threads
- Atomic ANSI parsing with cached snapshots to prevent race conditions
- Integration with AbstractCore for LLM capabilities
- Integration with AbstractRuntime for workflow orchestration
- Integration with AbstractAgent for agent implementations
- Efficient lazy imports for fast `--help` response time
- Graceful error handling with state preservation on interruption

#### Dependencies

**Required:**
- `prompt_toolkit>=3.0.0` - Terminal UI framework

**Implicit (from AbstractCore/AbstractRuntime/AbstractAgent):**
- AbstractCore for LLM provider abstraction
- AbstractRuntime for workflow and state management
- AbstractAgent for React and CodeAct agent implementations

#### Installation

```bash
pip install abstractcode
```

#### Quick Start

```bash
# Start with default settings (Ollama + qwen3:1.7b)
abstractcode

# Use a specific provider and model
abstractcode --provider openai --model gpt-4o-mini

# Use CodeAct agent with auto-approve
abstractcode --agent codeact --auto-approve

# Disable state persistence
abstractcode --no-state

# Set custom iteration and token limits
abstractcode --max-iterations 50 --max-tokens 64000
```

#### Example Session

```bash
$ abstractcode
AbstractCode v0.2.0 | Provider: ollama | Model: qwen3:1.7b

> Create a Python script that analyzes a CSV file

🤖 Thinking: I'll create a CSV analysis script using pandas...

🔧 Tool: write_file
   File: analyze_csv.py
   [Approve] (y/n/all/edit/quit): y

✓ File written successfully

🤖 The script has been created. Would you like me to test it?

> yes, test it with a sample CSV

[Agent continues working...]

Commands: /help | /status | /tools | /history | /clear
```

---

## Versioning Notes

- **0.2.0**: Initial public release with full feature set
- **0.1.0**: Internal development version

---

[0.2.0]: https://github.com/lpalbou/abstractcode/releases/tag/v0.2.0
