# Self-check

## Repair evidence

- Collapsed the repair history onto `main`, eliminating the five unrelated files from the mechanically delivered commit set.
- Verified `llms.txt`, `hello-v10.txt`, `LICENSE`, `Cargo.toml`, and `CODE_OF_CONDUCT.md` exactly match `main`.
- Verified that, excluding this evidence file, the sole delivered delta is the requested artifact.
- Verified the artifact contains exactly `overnight-cap-ok`, is exactly 16 bytes, and has no trailing newline.

## Artifact SHA-256

ARTIFACT-SHA256: untracked/overnight-bench/out/code-tui/multi-coder-1/hello.txt cbff1adba3d54781ca7b106d9b349f3bd62b2d3df8b1d9401d37abcaa7ef35d4
