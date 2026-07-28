# Self-check

## Repair evidence

- Created the required `untracked/overnight-bench/out/code-tui/multi-coder-2/hello.txt`.
- Verified `llms.txt`, `hello-v10.txt`, `LICENSE`, `Cargo.toml`, and `CODE_OF_CONDUCT.md` match `main` and are not delivered changes.
- Removed the prior wrong-path artifact under `multi-coder-1` from the delivered changes.
- Verified that, excluding this evidence file, the sole delivered delta is the requested artifact.
- Verified the required artifact contains exactly `overnight-cap-ok`, is exactly 16 bytes, and has no trailing newline.

## Artifact SHA-256

ARTIFACT-SHA256: untracked/overnight-bench/out/code-tui/multi-coder-2/hello.txt cbff1adba3d54781ca7b106d9b349f3bd62b2d3df8b1d9401d37abcaa7ef35d4
