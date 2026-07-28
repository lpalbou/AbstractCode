# create-cap-marker

- Status: planned
- Source: multi-agent coding workflow

## Goal
Create the requested hello.txt artifact at the existing destination with byte-for-byte content equal to overnight-cap-ok, exactly 16 bytes with no trailing newline, while making no other filesystem changes.

## Steps
- Write the 16-byte string overnight-cap-ok directly to /Users/albou/tmp/abstractframework/abstractcode-tui/untracked/overnight-bench/out/code-tui/multi-coder-2/hello.txt without appending a newline.
- Verify that the target file contains exactly overnight-cap-ok, is exactly 16 bytes long, and that no temporary, backup, or other files were created.

## Files
- /Users/albou/tmp/abstractframework/abstractcode-tui/untracked/overnight-bench/out/code-tui/multi-coder-2/hello.txt

## Risks
- A text-writing method may append a trailing newline, causing the required byte-for-byte check to fail.
- An editor or atomic-write mechanism may create temporary or backup files, violating the requirement not to write anywhere else.
- Using an incorrect or relative path could modify a location other than the specified target.
