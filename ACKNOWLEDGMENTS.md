# Acknowledgments

AbstractCode is built on (and inspired by) a lot of open-source work — thank you.

## Where dependencies are declared

- Terminal client: `tui/Cargo.toml` (resolved tree in `Cargo.lock`)
- Browser client: `web/package.json` (resolved tree in `web/package-lock.json`)

## Terminal client dependencies (Rust)

Declared in `tui/Cargo.toml`:

- [`abstracttui`](https://github.com/lpalbou/AbstractTUI) — the terminal rendering engine: layout, widgets, input, themes, and screen capture
- `ureq` — small blocking HTTP client, used for gateway calls and SSE streaming
- `serde_json` — JSON parsing for gateway payloads

## Server-side lineage

AbstractCode is a client. The agent it drives runs on
[AbstractGateway](https://github.com/lpalbou/abstractgateway), which builds on
[AbstractRuntime](https://github.com/lpalbou/abstractruntime) for durable runs
and [AbstractCore](https://github.com/lpalbou/abstractcore) for providers and
tools. Those projects carry their own acknowledgments.

## Browser client dependencies (TypeScript/React)

Declared in `web/package.json`:

- [`@abstractframework/ui-kit`, `panel-chat`, `monitor-flow`, `monitor-gpu`](https://github.com/lpalbou/abstractuic) — the shared AbstractUIC component packages

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

## Development & packaging

- Terminal client: `cargo` with `rustfmt` and `clippy`
- Browser client: `vite` and `typescript` for the build, `vitest` for tests

## Abstract* ecosystem

AbstractCode is part of the Abstract* stack and relies on:

- [AbstractFramework](https://github.com/lpalbou/AbstractFramework)
- [AbstractCore](https://github.com/lpalbou/abstractcore)
- [AbstractRuntime](https://github.com/lpalbou/abstractruntime)
- **AbstractAgent**
- (optional) **AbstractFlow** for local VisualFlow runs (`abstractcode[flow]`)
- **AbstractUIC** components in the Web UI (via `../../abstractuic/*`)

Thank you to all contributors and the broader open-source community.
