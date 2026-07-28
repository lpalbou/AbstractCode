# ensure-cap-file

- Status: planned
- Source: multi-agent coding workflow

## Goal
Ensure the requested hello.txt exists at the exact specified path with the 16-byte content `overnight-cap-ok` and no trailing newline, without writing to any other location; current findings indicate it already satisfies these requirements.

## Steps
- Verify that `/Users/albou/tmp/abstractframework/abstractcode-tui/untracked/overnight-bench/out/code-tui/multi-coder-1/hello.txt` still exists and contains exactly the 16 bytes representing `overnight-cap-ok`.
- If verification succeeds, make no changes.
- Only if the file no longer matches, replace its entire content in place with exactly `overnight-cap-ok`, without a trailing newline.
- Perform a final read-only verification of the target file's exact content and byte length; do not access or modify any other file.

## Files
- (none)

## Risks
- A text-writing method could append a trailing newline, producing 17 bytes instead of the required 16.
- The file could change between verification and completion; if so, only the specified target may be corrected.
- Any diagnostic output redirected to disk would violate the requirement not to write anywhere else.
