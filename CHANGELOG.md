# Changelog

All notable changes to AbstractCode will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [terminal 0.5.1 / web 0.4.2] - 2026-09-23

Terminal client `abstractcode` 0.5.1 (tag `v0.5.1`) and browser client
`@abstractframework/code` 0.4.2 (tag `web-v0.4.2`).

### Added

- **Native-MTP request controls.** Browser Settings and the terminal's `/mtp`
  command (alias `/speculation`) and `--mtp` flag choose inherit, explicit Off,
  or a requested speculative-decoding depth. The pickers are driven by the
  gateway's advertised capabilities, show readiness and unavailable saved
  choices, and never load or download a model. Preferences and run requests
  keep an explicit Off distinct from inherit.
- **Web: drag-and-drop and paste files into the chat composer.** Files dropped
  on the conversation or pasted into the message field (screenshots included)
  attach like picked files. Each file appears as a chip in the composer:
  "Waiting" / "Uploading" (three uploads at a time), then name and size. A file
  over the gateway's `maxAttachmentBytes` is refused on its chip before any
  upload, with both sizes named; a gateway failure shows the gateway's reason
  with Retry. Chips are keyboard-focusable with labelled Remove buttons, and a
  live region announces the attachment count. The composer stays usable while
  files upload; starting a turn while a chip is still uploading, refused or
  failed is refused with the reason, and uploads survive a run starting or
  finishing mid-upload.
- **Web: the Activity tab names each step.** Started / waiting / completed
  records of one step collapse into a single row whose status advances, and
  `abstract.progress` / `abstract.status` records fold into the row of the step
  that emitted them instead of becoming rows of their own. Rows read like
  `llm · mlx · <model>` with tokens in/out, cache state, duration and tool-call
  count; `tools · web_search ×3` with an argument preview; `subflow · agent
  loop` with its task; `wait · delay 3 s`; `ask · your input`; and
  `run finished`. Expanding a row still shows the raw JSON of every record it
  collapsed.
- **Web: the run strip shows the model's phase.** When the provider reports it
  (AbstractCore `abstract.progress` records with `kind: "llm"`), the run strip
  reads `Prefill · 2,100 / 5,642 tokens (37%)` while the prompt is processed
  (or `Prefill · 5,642 tokens` without a measured position) and
  `Generating · 120 tokens · 157 tok/s` once tokens stream. Without phase
  records the strip renders exactly as before.
- **Web: message stat chips open a detail panel.** The tokens / tools / time
  chips under an assistant message open a structured panel on hover or focus:
  prompt-cache reuse per call, context growth, time-to-first-token and
  generation rates, speculative-decoding acceptance, tool time with the slowest
  batch and failure classes, and per-call tables for multi-call turns. Values
  the stack did not report read "not reported".
- Web: reusable AbstractUIC workflow chat for agent and registered Flow
  workflows (questions, event waits, status messages, structured final
  results), gateway skill selection, local next-turn queues with cancellation
  safety, optional gateway-backed dictation and read-aloud, and responsive
  layouts.
- Web: `npm run test:e2e` runs a Playwright end-to-end suite against a
  disposable local gateway fixture (`web/e2e/gateway_fixture.py`); browsers are
  installed separately with `npx playwright install`. `npm test` stays a
  browser-free unit suite.

### Changed

- Web: the browser opens a modular AbstractUIC-themed workspace with shared
  gateway sign-in, durable conversation history, workflow inputs,
  file/artifact inspection, and explicit approval controls. Generic Flow
  inputs are kept separate from agent-only settings.
- Web: Stop shows the ledger-derived stop state where the Stop button was:
  "Stopping…" → "Stopped", or the gateway's own report such as
  "Stop forced at 10 s: inference killed".
- Web: the shared components install from npm as
  `@abstractframework/panel-chat` `^0.1.16` and `@abstractframework/ui-kit`
  `^0.1.10`; building `web/` needs only the npm registry.
- Web: `npm run dev` uses the same gateway session proxy as the packaged
  server (`bin/server.js`), so development and production share one sign-in
  and CSRF flow. The development-only `/api` proxy to port 8081 is removed;
  set `ABSTRACTCODE_GATEWAY_URL` instead.

### Fixed

- Web: with a standing permission ("Permissions: all", or per-tool Allow) a
  covered tool batch no longer flashes "Approval needed" while it runs; the
  browser answers the gateway's parked approval automatically and shows the
  batch as running tools. Batches outside the permission still show the
  approval card, and after Revoke the next batch asks again.

### Security

- Web: browser login and gateway mutations validate request origin and Fetch
  Metadata before forwarding. Login requires JSON, and browser destination
  changes require a loopback peer and hostname unless explicitly enabled with
  `ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1`.

## [terminal 0.5.0 / web 0.4.0, 0.4.1] - 2026-08-31

### Security

- The browser client's bundled HTML sanitizer is updated to DOMPurify 3.4.14,
  which carries fixes for mutation-XSS via re-contextualization and for an
  `IN_PLACE` hook leaving a detached subtree executable. The dependency is no
  longer pinned to an exact version, so it can take future patches, and an
  override collapses the copy nested inside `monaco-editor` onto the same
  patched version.
- Build-toolchain dependencies are updated; `npm audit` reports no advisories
  at any severity. These never shipped to users — the published package
  contains only a bundle and a server on Node builtins — but they run on
  contributor machines and in CI.

### Changed

- The published browser package declares no runtime dependencies. It ships a
  self-contained bundle and a server that uses only Node builtins, so
  installing it no longer downloads React, Monaco and the rest of the build
  toolchain.
- Release binaries ship a `SHA256SUMS` file and build provenance, verifiable
  with `gh attestation verify`.

### Added

- Terminal client written in Rust, published to crates.io as `abstractcode` and
  installed with `cargo install abstractcode`. It lives in `tui/` and renders
  with [AbstractTUI](https://github.com/lpalbou/AbstractTUI): live reasoning
  cycles, tool cards that update in place, durable pause/cancel/resume,
  mid-run steering, file attachments, session history replay, and 26 themes.
- Prebuilt binaries for macOS (Apple silicon and Intel), Linux (x86-64 and
  ARM64), and Windows, attached to each release.

### Changed

- **AbstractCode is now a Rust terminal client plus a browser client.** The two
  ship independently: `v<version>` releases the terminal client, and
  `web-v<version>` releases `@abstractframework/code` to npm.
- The browser client consumes the shared AbstractUIC components as published
  npm packages rather than through path aliases into a sibling checkout, so
  `web/` builds from its own directory with no other repository present.
- Preferences moved to `~/.abstractcode/prefs.json`, beside the existing login
  store. `ABSTRACTCODE_PREFS_FILE` overrides the location.
- The browser client moves to 0.4.0, the first version released under the
  `web-v<version>` tag.

### Removed

- **The Python implementation of AbstractCode.** The terminal client replaces
  it. Version 0.3.8 remains installable from PyPI and tag `v0.3.8` marks its
  source; the implementation stays in this repository's history.
- `abstractcode flow`, `abstractcode gateway`, and `abstractcode workflow`
  subcommands. Workflow selection is now `--workflow <bundle[:flow]>`, and
  installing bundles is a gateway-side operation.

### Fixed

- The browser client's gateway-configuration guard decides local from the
  connection peer instead of the `Host` header, which a remote client could
  set to claim it was local.
- The shared component stylesheet is imported explicitly, restoring styling for
  the agent-cycles panel.

### Migration

- `pip install abstractcode` → `cargo install abstractcode`.
- Preferences from a pre-rename build are read once from
  `~/.abstractcode-tui/prefs.json` and saved forward automatically.
- Credentials in `~/.abstractcode/gateway.json` are unchanged.

## [0.3.9] - 2026-06-03

### Changed
- Raised AbstractFramework dependency floors to Core `>=2.13.32`, Runtime `>=0.4.27`, Agent `>=0.3.11`, and Flow `>=0.3.18`.
- Aligned the web package release version with the Python package.

## [0.3.8] - 2026-05-31

### Changed
- Raised AbstractFramework dependency floors to Core `>=2.13.31`, Runtime `>=0.4.26`, Agent `>=0.3.10`, and Flow `>=0.3.17`.
- Aligned the web package release version with the Python package.

### Fixed
- Hosted web mode now follows the Gateway URL/session policy used by Flow so remote browser clients cannot turn the app into a user-directed Gateway proxy.

## [0.3.7] - 2026-05-29

### Changed
- Raised AbstractFramework dependency floors to the current released Core, Runtime, Agent, and Flow versions.
- Aligned the web package release version with the Python package so one release tag publishes both surfaces.
- Updated the workflow agent contract wording for provider pins to match the provider-text/provider taxonomy.

### Fixed
- Added the dedicated web favicon asset referenced by the browser host.
- Expanded CI/release coverage so Python and web package gates run before publishing.

## [0.3.1] - 2026-02-04

### Added
- **Workflow-driven UI events (network-safe)**:
  - Workflows can emit `Emit Event(name="abstract.message")` to show a message/notification in AbstractCode.
  - Workflows can emit `Emit Event(name="abstract.tool_execution")` and `Emit Event(name="abstract.tool_result")` to render tool-call + tool-result UX blocks (without requiring actual tool execution).
  - `WAIT_EVENT` can carry a `prompt` so workflows can do durable ask+wait under `WaitReason.EVENT` (useful for thin clients); AbstractCode will prompt and resume.
  - `abstract.status` payload supports `duration` (seconds): default `-1` (sticky), `> 0` auto-clears unless superseded.
  - Tool event payloads can be a **single object or a list** (e.g., wire `LLM Call.tool_calls` / `Tool Calls.results` directly into an `Emit Event`).
  - Backward compatibility: `abstractcode.*` remains a deprecated alias accepted by existing hosts.
- **Documentation refresh for public release**: clearer user-facing docs (`docs/getting-started.md`, `docs/architecture.md`, `docs/cli.md`, `docs/api.md`, `docs/faq.md`) plus `SECURITY.md`, `CONTRIBUTING.md`, and `ACKNOWLEDGMENTS.md`.

### Fixed
- Align package version metadata and `abstractcode.__version__`.
- `/help` now shows the correct `/gpu [status|on|off]` usage.

## [0.3.0] - 2026-02-03

### Added
- **Workflow Agent Support** (`abstractcode/workflow_agent.py`): Run VisualFlow workflows as first-class agents via `abstractcode --agent <flow_id|flow_name|/path/to/flow.json>`
  - `abstractcode.agent.v1` interface contract requires host-configurable `provider`/`model`/`tools` start pins (in addition to `prompt`/`response`)
  - Workflows can emit `Emit Event(name="abstract.status")` to update TUI footer status text in real time
  - `On Flow End.meta` (and optional `scratchpad`/`success`) surfaced as assistant-message metadata (`workflow_meta`, `workflow_scratchpad`, `workflow_success`)
  - File-backed persistence support for durable workflow execution
  - Documented in README with usage examples
- **MCP (Model Context Protocol) Integration**: Connect to remote MCP servers for tool execution
  - `/mcp` command to configure and manage MCP server connections
  - `/executor` command to set default tool executor (local vs remote MCP server, session-persistent)
  - Automatic tool synchronization from MCP servers
  - Spinner feedback for remote MCP tool calls
  - Support for stdio-based MCP servers
  - MCP tools integrated into native tool allowlist
- **Enhanced History Commands**:
  - `/history copy` command to copy full conversation history to clipboard
- **Collapsible Thought/Tool Blocks**: Tool-using iterations now render **Thought** and **Tool Call** as **click-to-toggle** blocks (collapsed by default) with high-signal one-line summary always visible
- **Spinner Shimmer**: Status bar spinner text has subtle **reflect/shimmer** highlight traversing the entire text so "still working" is obvious without re-rendering scrollback
- **`/logs provider --no-tool-defs`**: Optionally replace provider request `tools` array (full tool definitions) with array of tool names for compact sharing/debugging
- **Terminal Markdown Module** (`abstractcode/terminal_markdown.py`): Dedicated module for rendering Markdown in terminal with newline unescaping
- **New Test Coverage**:
  - Workflow agent tests (`test_workflow_agent.py`)
  - MCP remote tool execution tests (`test_remote_mcp_tool_execution.py`, `test_remote_mcp_tool_execution_stdio.py`)
  - Repeat guardrail tests (`test_repeat_guardrail_write_file_content.py`)
  - Tool examples toggle tests (`test_tools_examples_toggle.py`)
  - History copy tests (`test_history_copy_full_to_clipboard.py`)
  - Executor command tests (`test_executor_command.py`, `test_executor_real_logic.py`)
  - Spinner shimmer tests (`test_fullscreen_ui_spinner_shimmer.py`)
  - Log provider tests (`test_log_provider_no_tool_defs.py`, `test_log_provider_tool_calls_anthropic.py`)
  - Answer markdown tests (`test_answer_markdown_newline_unescape.py`)

### Changed
- **`/clear`**: Now clears the screen (UI output) in addition to clearing in-memory conversation context
- **`/memorize`**: Renamed from memory-note command to **Memorize** (consistent UX term) to avoid ambiguity with span tagging
- **`/recall`**: Richer filtering and rehydration controls:
  - Added `--tags-mode all|any`, repeatable `--user NAME`, and repeatable `--location LOC`
  - Repeating `--tag k=v` now builds multi-value tags (e.g. `--tag person=alice --tag person=bob`)
  - `--into-context` now also rehydrates matching `memory_note` spans as synthetic system message (`[MEMORY NOTE] ...`)
- **Logging Commands**: Replaced legacy `/context` + `/llm` with `/logs runtime` + `/logs provider` (no backward compatibility)
  - `/logs provider` now reads from durable ledger and includes **all LLM provider calls in current session** (across runs) unless `--run` is used
  - `/logs provider` renders OpenAI/LMS-style "Received request … Generated prediction …" blocks (no truncation)
  - `/logs runtime ... copy` and `/logs provider ... copy` now accept `copy` as trailing token and copy without rendering
- **Verifier (Review) Mode**: Now enabled by default to prevent premature "stops" when model returns incomplete prose without tool calls
  - Added `--no-review` to disable (not recommended)
  - Default `--review-max-rounds` increased to 3
- **Tool Prompt Examples**: Now **off by default** to avoid large token overhead; use `/tools examples on` to enable
- **Output Versioning**: FullScreenUI now uses output versioning and caching for improved render performance
- **Scrolling Behavior**: Enhanced scrolling in FullScreenUI with better page up/down and smooth scroll support

### Fixed
- **Spinner Shimmer Sweep**: Status bar spinner shimmer now traverses **entire** spinner text (previously capped to first ~10 visible characters)
- **Tool Result Visibility**: Increased default tool observation preview to **1000 characters** (was 120) so small-but-critical outputs (e.g. exit codes, working directories) not silently truncated in UI
- **ANSWER Newline Rendering**: Unescape literal `\n` / `\r\n` sequences into real line breaks before terminal Markdown rendering, so multi-line answers display correctly
- **Web Search Reliability**: Added `ddgs>=9.10.0` as dependency so default `web_search` tool works without manual installs
- **Native Tools Prompt Accounting**: ReactShell token estimation now excludes full `Tools (session)` Active Memory catalog for **native-tool models**, matching prompt actually sent to OpenAI-compatible servers (e.g. LMStudio)
- **LLM-Call Payload Observability**: `/logs provider` shows verbatim provider request/response (`_provider_request` + `raw_response`), `/logs runtime` shows durable runtime step trace for LLM/tool calls
- **`/logs provider` Tool-Call Detection**: Best-effort tool-call summary now detects Anthropic `tool_use` blocks in addition to OpenAI-style `tool_calls`
- **Repeat Guardrail**: Reset duplicate-tool-call caches on **new runs** and **/cancel**, block `write_file` calls missing `content` to prevent repeated 0‑byte file writes
- **File Tool CWD Injection**: File tools (read/write/edit) no longer inject `cwd` into UI preview, preventing confusion when relative paths shown
- **Async Run Controls**: Improved async handling for pause/resume/cancel controls
- **Flow CLI Entry Validation**: Added required entry inputs validation in CLI flow commands

### Removed
- **`/new`, `/reset`**: Removed alias commands (identical to `/clear`). Use `/clear`
- **Legacy `/context`, `/llm`**: Removed in favor of `/logs runtime` and `/logs provider`

### Technical Details
- **44 commits**, **30 files changed**: 8,731 insertions, 756 deletions
- New modules: `workflow_agent.py` (721 lines), `terminal_markdown.py` (168 lines)
- 15 new test files covering workflow agents, MCP integration, repeat guardrails, and UI enhancements
- AbstractCore dependency updated to include `[tools]` extras for web search reliability
- Enhanced ReactShell with MCP client management, executor configuration, and improved token estimation

### Migration Notes
- Legacy `/context` and `/llm` commands removed; use `/logs runtime` and `/logs provider` instead
- Tool prompt examples now off by default; enable with `/tools examples on` if needed
- Verifier (review) mode now enabled by default; disable with `--no-review` if unwanted

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
