# Self-check

## Repair evidence

- Smoke mode now runs the selected child benchmark instead of returning the prompt text directly.
- Verified a successful child result containing `smoke-ok` yields exit 0 and stdout exactly `smoke-ok\n`.
- Verified a successful child result without `smoke-ok` is rejected with nonzero exit and empty stdout.
- Verified `python3 -m py_compile scripts/zelda_headless_bench.py` and `git diff --check` exit 0.

## Artifact SHA-256

ARTIFACT-SHA256: scripts/zelda_headless_bench.py e66d48e9250411c290779c6510541e99b0c115b3589af8699d83e15ae9698d94
