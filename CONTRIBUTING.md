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
python3 scripts/check_portable_paths.py   # every tracked name checks out on Windows
```

CI also builds the terminal client on Windows. A tracked file name with `< > : " | ? *`,
a trailing dot or space, or a reserved device name (`CON`, `NUL`, `COM1`...) fails
`scripts/check_portable_paths.py`, which runs before every build and release.

For an unreleased AbstractTUI dependency, use a sibling checkout explicitly:

```sh
cargo test --workspace \
  --config 'patch.crates-io.abstracttui.path="../abstracttui"' \
  --config 'patch.crates-io.abstractavatar.path="../abstractavatar"'
cargo install --path tui \
  --config 'patch.crates-io.abstracttui.path="../abstracttui"' \
  --config 'patch.crates-io.abstractavatar.path="../abstractavatar"' --force
```

Resolve the patch path relative to your working directory. Registry-only builds
require the dependency version to have been published.
The Avatar patch is needed while that shared generator is also unreleased;
it is supplied per command, rather than through a global Cargo configuration.

All three run in CI and must pass. `cargo package --manifest-path tui/Cargo.toml --list`
shows exactly what a release would publish.

`cargo package` refuses to build from a dirty tree. If it names a file you did
not edit, it is a build artifact that should not be tracked — untrack it and
add it to `.gitignore` rather than committing it.

### Browser client (`web/`)

Requires Node.js **24**.

```bash
cd web
npm ci          # install from the lockfile, not the manifest
npm test
npm run build
```

`npm ci` rather than `npm install`, so you reproduce CI's dependency tree.

## Lockfile check

`npm run check:lock` (in `web/`) runs `scripts/check_lock.mjs`, and CI runs it before `npm ci`. It
fails when `package-lock.json` lags `package.json` (the lock's root entry records a different
dependency spec: `package.json` was edited without `npm install`), and when an
`@abstractframework/*` dependency (`ui-kit`, `panel-chat`, `app-server`, `monitor-*`) is missing
from the lock, resolves below the `package.json` floor or to another major.minor, comes from a
local `file:` tarball, or has a nested copy that differs from the top-level one. Fix it with
`npm install` (or `npm install @abstractframework/<name>@^<version>` to raise a floor) and commit
both files.

At release time, `npm run check:lock -- --latest` also fails when npm has a newer patch of an
`@abstractframework/*` dependency than the lock resolves (needs the network).

```bash
cd web
npm run check:lock
```

The conversation, the session cards and the Docs drawer are AbstractUIC snapshots under
`web/src/vendor/`. `npm run build` runs `scripts/check_vendor_sources.mjs`, which fails unless
every source file a snapshot's `provenance.json` records is byte-identical to the installed
package in `node_modules`. After raising a kit floor, re-sync them from an AbstractUIC checkout
at the published commit (`npm view @abstractframework/panel-chat@<version> gitHead`) with
`npm run sync:conversation`, `npm run sync:sessions` and `npm run sync:docs`, and commit them
with the lock.

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

The web app lives in `web/`. Its shared UI components install from npm as ordinary dependencies, so it builds from that directory alone.

See:
- [`docs/web.md`](docs/web.md)
- [`docs/deployment-web.md`](docs/deployment-web.md)
