# smoke-ok

- Status: delivered
- Source: multi-agent coding workflow

## What was delivered

The smoke path in `scripts/zelda_headless_bench.py` runs the selected child
benchmark and uses the value after `exactly:` in `ZELDA_BENCH_SMOKE` as its
expected answer. It does not treat the prompt itself as a successful result.

When all selected child runs succeed and their captured final answers contain
the expected value, the driver exits 0 and writes only that value to stdout.
For the request `Reply with exactly: smoke-ok`, stdout is `smoke-ok` followed
by the normal line terminator. A failed child or a missing expected answer
produces a nonzero exit and no success text on stdout.

## Run

```sh
ZELDA_BENCH_SMOKE='Reply with exactly: smoke-ok' \
ZELDA_BENCH_TIMEOUT_S=120 ZELDA_BENCH_MAX_ITER=3 \
python3 scripts/zelda_headless_bench.py code-1
```

Lane arguments are `code-1`, `code-tui-1`, `code-2`, and `code-tui-2`. With no
lane argument, the script runs all four. TUI lanes use the release binary at
`target/release/abstractcode-tui` and therefore require `cargo build
--release` first.

## Output and artifacts

Smoke mode suppresses the benchmark's normal progress and assessment output,
but the child still runs and the script still writes logs, partial/final JSON
reports, summaries, and the assessment under `untracked/zelda-bench/`.

## Verification

The verified branch covered both outcomes:

- a successful child result containing `smoke-ok` exits 0 with stdout exactly
  `smoke-ok\n`;
- a successful child result without `smoke-ok` is rejected with a nonzero exit
  and empty stdout.
