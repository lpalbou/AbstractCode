# Contributing

Thanks for helping improve abstractcode. This is a small, focused
codebase; the fastest way to a merged change is to keep the layering rules
intact and ship the matching test.

## Build and test

```sh
cargo build
cargo test                 # unit + real-ledger replay + headless UI
cargo clippy --all-targets # must be warning-free
cargo fmt                  # rustfmt-clean
```

The whole `cargo test` suite runs offline — no gateway, no network, no pty.
Live verification (requires a running AbstractGateway + an LLM provider):

```sh
./target/debug/abstractcode doctor --gateway http://127.0.0.1:8080 --token <token>
ACODE_GATEWAY_TOKEN=<token> python3 scripts/pty_live_smoke.py
```

## Layering rules (load-bearing)

- **The UI thread owns every signal.** Worker threads never touch the store;
  they post closures through `WakeHandle::post` (see `src/runner.rs`).
  UI→worker communication goes through the `Cmd` mpsc channel only.
- **`src/protocol.rs` stays pure**: functions over `serde_json::Value`
  ledger records, no I/O, no state. New record shapes get unit tests with
  realistic fixtures.
- **`src/transcript.rs` (the fold) is UI-free** and must stay deterministic
  under replay: reconnects re-deliver records, so every new item type needs
  a dedup story (see the seen-sets at the top of `Fold`).
- **Gateway contract changes** follow the reference clients
  (`abstractcode/web/src/lib`, the AbstractAssistant gateway adapter) — this
  port deliberately mirrors their extraction semantics.

## Tests to write for a change

- Pure logic → unit test beside the code.
- Record-shape handling → extend `tests/fixtures/` (sanitize machine paths)
  and `tests/run_tree_replay.rs`.
- Anything the user sees → a headless UI test in `tests/headless_ui.rs`
  (CaptureTerm + Driver render the real interface; assert on
  `term.screen().to_text()`).

## Style

- rustfmt + clippy clean, no warnings.
- Comments explain intent and constraints, not mechanics.
- Truncated display content carries an explicit label (search for
  `#TRUNCATION` for the pattern).

## Releases

Bump `Cargo.toml`, add a dated `CHANGELOG.md` entry, run the full gate
(fmt, clippy, test, `cargo package`), then tag. The crate must always
install cleanly via `cargo install abstractcode`.
