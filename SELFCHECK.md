# Self-check

## Repair evidence

- Created the required `untracked/overnight-bench/out/code-tui/multi-coder-2/hello.txt`.
- Restored the five baseline files named by the failure so they are not branch-delivered changes: `llms.txt`, `hello-v10.txt`, `LICENSE`, `Cargo.toml`, and `CODE_OF_CONDUCT.md`.
- Verified `cargo check --manifest-path Cargo.toml` succeeds.
- Verified the required artifact contains exactly `overnight-cap-ok`, is exactly 16 bytes, and has no trailing newline.

## Artifact SHA-256

ARTIFACT-SHA256: untracked/overnight-bench/out/code-tui/multi-coder-2/hello.txt cbff1adba3d54781ca7b106d9b349f3bd62b2d3df8b1d9401d37abcaa7ef35d4
