# Self-check

## Repair evidence

- Exact smoke executable: `ZELDA_BENCH_SMOKE='Reply with exactly: smoke-ok' python3 scripts/zelda_headless_bench.py`
- Verified exit 0, stdout exactly `smoke-ok\n`, and empty stderr.
- Verified an invalid smoke prompt without `exactly:` exits nonzero.
- Verified `python3 -m py_compile scripts/zelda_headless_bench.py` and `git diff --check` exit 0.

## Artifact SHA-256

ARTIFACT-SHA256: scripts/zelda_headless_bench.py 343a80f0af8f2e5e02183fdbb1014bf53077ab76fb34d1446bf3f1cf952e1e70
