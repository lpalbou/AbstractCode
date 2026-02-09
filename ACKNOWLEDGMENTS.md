# Acknowledgments

AbstractCode is built on (and inspired by) a lot of open-source work — thank you.

## Where dependencies are declared

- Python: `pyproject.toml`
- Web UI: `web/package.json` (and the fully resolved tree in `web/package-lock.json`)
- Web UI local packages: `web/vite.config.ts` aliases (`../../abstractuic/*`)

## Runtime dependencies (Python)

Declared in `pyproject.toml`:

- `abstractagent` — built-in agents (`react`, `memact`, `codeact`)
- `abstractruntime` — durable runs, stores, snapshots, and workflow execution
- `abstractcore[tools,media]` — provider/model abstraction + tools + rich media handling
- `prompt_toolkit` — interactive terminal UI (TUI)
- `ddgs` — DuckDuckGo-backed search backend used by the default `web_search` tool

## Optional extras (Python)

- `abstractflow` — VisualFlow execution (`pip install "abstractcode[flow]"`, declared in `pyproject.toml`)
- `abstractmemory` — optional memory/knowledge graph integration (imported in `abstractcode/react_shell.py`, not installed by default)

## Web UI dependencies (TypeScript/React)

Declared in `web/package.json`:

- `react`, `react-dom` — UI framework
- `vite`, `typescript` — build toolchain
- `vitest` — tests
- `@monaco-editor/react` — editor (Monaco wrapper)
- `marked` — Markdown rendering
- `dompurify` — HTML sanitization for rendered Markdown

Local UI packages (via `web/vite.config.ts` aliases):

- `@abstractuic/ui-kit` — shared theme/tokens and UI primitives
- `@abstractuic/panel-chat` — chat message rendering
- `@abstractuic/monitor-flow` — run/trace viewer (Context inspector)
- `@abstractutils/monitor-gpu` — optional GPU widget integration

## Development & packaging

Declared in `pyproject.toml` (dev/build):

- `pytest`, `pytest-cov` — tests/coverage
- `ruff`, `black` — linting/formatting
- `setuptools`, `wheel` — packaging

## Abstract* ecosystem

AbstractCode is part of the Abstract* stack and relies on:

- [AbstractFramework](https://github.com/lpalbou/AbstractFramework)
- [AbstractCore](https://github.com/lpalbou/abstractcore)
- [AbstractRuntime](https://github.com/lpalbou/abstractruntime)
- **AbstractAgent**
- (optional) **AbstractFlow** for local VisualFlow runs (`abstractcode[flow]`)
- **AbstractUIC** components in the Web UI (via `../../abstractuic/*`)

Thank you to all contributors and the broader open-source community.
