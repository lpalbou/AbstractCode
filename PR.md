# PR: input focus, engine 0.3.3, /review + project context, docs pass

Branch: verify-cap-file → main (fast-forward; main is an ancestor)

## Goal

Make every kind of input reach the prompt, adopt the engine release that
fixes copy-during-a-run, and bring the documentation set back in step with
the code.

## What changed

**Input returns to the composer.** The transcript is focusable, so a click
in the scrollback or a `Tab` parked the keyboard there — and the transcript
answers only navigation keys, so anything typed after that was dropped. A
root capture handler now hands focus back and keeps what arrived:
characters (including `/`, which opens the command dropdown), pasted text
with newlines normalized, and dropped files staged as attachment chips.
Navigation keys, `Ctrl`/`Alt` chords and modal keys are untouched.

**Engine `abstracttui` 0.3.1 → 0.3.3.** A live screen selection freezes
follow-tail scrollers, so a drag over a streaming transcript copies the
cells it highlighted. The composer inherits Codex editor chords (word and
line motion, word delete); `Ctrl+E` is move-to-line-end there, so
conversation focus moved to `Alt+E` — the only collision. 0.3.2 was tagged
without reaching crates.io, so 0.3.3 is the resolvable floor; a
`[patch.crates-io]` builds it from the sibling checkout until it publishes,
and must be removed before this crate is published.

**Also in this batch** (previously uncommitted): `/review`
verifier-before-conclude, project-context (`AGENTS.md`) injection shared
with `exec`, `--no-prompt-cache` on the interactive path, discovery,
protocol and run-input work, benchmark and scoring scripts, rtype fixtures.

**Docs (coredoc pass):** CHANGELOG entries for the above; keys table
carrying the type-to-focus rule and the new editor chords; troubleshooting
repaired for typing and for the clipboard (both entries described behavior
that no longer matches the code); `docs/orchestration-cards.md` indexed in
`docs/README.md` and `llms.txt`; `llms-full.txt` regenerated from the
corpus.

## Verification

- `cargo test`: 315 unit + 156 headless + 44 across the remaining
  integration targets — 0 failures.
- `cargo clippy --all-targets`: clean for every file in this change.
- `cargo fmt --check`: clean for every file in this change.
- The six focus/paste/drop tests each fail with the handler disabled.

## Known limits

- `src/exec.rs` and `src/runner.rs` carry pre-existing `cargo fmt` drift,
  and clippy reports `items after a test module` at `src/exec.rs:1018`.
  Untouched here — that work is in progress.
- No remote is configured for this repository, so this is a local
  fast-forward merge rather than a hosted pull request.
