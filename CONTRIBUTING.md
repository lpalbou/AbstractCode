# Contributing to AbstractCode

Thanks for taking the time to contribute. This project is still **pre-alpha**, so feedback, bug reports, and small focused PRs are especially valuable.

AbstractCode is part of the **AbstractFramework** ecosystem:
- [AbstractFramework](https://github.com/lpalbou/AbstractFramework)
- [AbstractCore](https://github.com/lpalbou/abstractcore)
- [AbstractRuntime](https://github.com/lpalbou/abstractruntime)

## Quick links

- Getting started: [`docs/getting-started.md`](docs/getting-started.md)
- Architecture: [`docs/architecture.md`](docs/architecture.md)
- CLI reference: [`docs/cli.md`](docs/cli.md)
- Docs index: [`docs/README.md`](docs/README.md)

## Development setup

Prereqs:
- Python **3.10+**
- (Optional) Node.js if you work on the web app (`web/`)

Install in editable mode with dev tools:

```bash
pip install -e ".[dev]"
```

Run tests:

```bash
pytest -q
```

Format and lint:

```bash
ruff check .
black .
```

## What to include in a PR

- A clear description of the problem and the approach.
- Tests for behavior changes (or a short explanation if a test is not practical).
- Docs updates when you change CLI flags/commands or UX behavior (prefer evidence-backed notes pointing to the relevant files/functions).
- Changelog entry if the change is user-facing (see [`CHANGELOG.md`](CHANGELOG.md)).

## Reporting bugs / feature requests

If you found a bug or want a feature:
- Prefer a minimal reproducible example and include:
  - OS + Python version
  - `abstractcode --help` output (or version)
  - what you expected vs what happened

Security issues: please follow [`SECURITY.md`](SECURITY.md).

## Web app contributions

The web app lives in `web/`. Local development currently depends on shared UI packages via Vite path aliases.

See:
- [`docs/web.md`](docs/web.md)
- [`docs/deployment-web.md`](docs/deployment-web.md)
