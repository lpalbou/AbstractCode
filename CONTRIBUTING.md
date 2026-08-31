# Contributing to AbstractCode

Thanks for taking the time to contribute. This project is still **pre-alpha**, so feedback, bug reports, and small focused PRs are especially valuable.

AbstractCode is part of the **AbstractFramework** ecosystem:
- [AbstractFramework](https://github.com/lpalbou/AbstractFramework)
- [AbstractCore](https://github.com/lpalbou/abstractcore)
- [AbstractRuntime](https://github.com/lpalbou/abstractruntime)

## Quick links

- Getting started: [`docs/getting-started.md`](docs/getting-started.md)
- Architecture: [`docs/architecture.md`](docs/architecture.md)
- API and CLI surface: [`docs/api.md`](docs/api.md)
- Docs index: [`docs/README.md`](docs/README.md)

## Development setup

This repository holds two clients, each with its own toolchain. You only need
the one you are working on.

### Terminal client (`tui/`)

Requires Rust **1.87+** (the crate's declared minimum; CI builds against it).

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

All three run in CI and must pass. `cargo package --manifest-path tui/Cargo.toml --list`
shows exactly what a release would publish.

### Browser client (`web/`)

Requires Node.js **24**.

```bash
cd web
npm ci          # install from the lockfile, not the manifest
npm test
npm run build
```

`npm ci` rather than `npm install`, so you reproduce CI's dependency tree.

## Releases

The two clients version independently, each under its own tag prefix:

| Tag | Publishes |
|---|---|
| `v<version>` | the crate `abstractcode` to crates.io, plus binaries on the GitHub release |
| `web-v<version>` | `@abstractframework/code` to npm |

Each publish job asserts the tag matches its own manifest version, so bump
`tui/Cargo.toml` or `web/package.json` before tagging.

## What to include in a PR

- A clear description of the problem and the approach.
- Tests for behavior changes (or a short explanation if a test is not practical).
- Docs updates when you change CLI flags/commands or UX behavior (prefer evidence-backed notes pointing to the relevant files/functions).
- Changelog entry if the change is user-facing (see [`CHANGELOG.md`](CHANGELOG.md)).

## Reporting bugs / feature requests

If you found a bug or want a feature:
- Prefer a minimal reproducible example and include:
  - OS, and the client and version you used
  - `abstractcode --help` output (or version)
  - what you expected vs what happened

Security issues: please follow [`SECURITY.md`](SECURITY.md).

## Web app contributions

The web app lives in `web/`. Local development currently depends on shared UI packages via Vite path aliases.

See:
- [`docs/web.md`](docs/web.md)
- [`docs/deployment-web.md`](docs/deployment-web.md)
