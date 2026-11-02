# 0013 — Responsive Gateway session listing and retained refresh state

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
- **Area:** tui
- **Work item:** abstractcode-0013
- **ADR impact:** none; Gateway summaries remain the source of truth.

## Summary and code reality

The operator reports a cached list briefly appearing, then a loading screen and a 60s timeout. The TUI asks
for 5,000 roots with include_metrics=true, which scans ledgers/subruns for every listed row. Systray uses
bounded metadata pages with metrics omitted and keeps its previous Gateway snapshot during refresh.
spawn_load_sessions replaces a Loaded index with Loading and then Failed, discarding visible cached data.

## Scope and validation

Remove mandatory ledger-derived metrics from the session discovery request; show a small metadata page
promptly before a larger completeness pass. Keep prior Gateway snapshots visible during refresh/error;
coalesce interactive refreshes. Counts must remain honest about partial pages and missing tool totals.
Test the exact wire query, cached refresh failure, incremental preview, and unchanged run/approval state.
Measure the metadata route against the local Gateway without triggering the expensive metrics scan.

## Completion report

Recent/archived lists request metadata without ledger metrics, preview 200 roots before the existing 5000 bound and retain loaded summaries on refresh/failure. Busy refreshes coalesce and known titles survive label errors.

Two loopback HTTP regression tests pin queries, previews, failure retention and no run mutation. Live metadata listing measured 602ms warm for 2658 roots; installed session picker first usable rows in 0.23s. Timings depend on host/cache. No expensive metrics scan was triggered.

Validation: AbstractTUI workspace/all-targets 2523 passed, 89 ignored; Code 1159 passed,
23 ignored; Gateway console 1013 passed, 76 ignored; Core console 244 passed, 2 ignored
(including doctests). Engine, Code and Gateway all-target Clippy passed with warnings denied.
Docs: engine API/README/agent skill, Code API/architecture/README, console README and
changelogs updated; LLM aggregates regenerated. Code and Gateway binaries installed locally.
No release, push, Gateway restart, live approval, coding run or user-preference mutation.
Source dependency floors are unreleased and use explicit local Cargo patch arguments.

Evidence: workspace `untracked/chat-20261030/` contains architecture self-review, PTY
captures and live smoke JSON receipts; suite logs are `untracked/chat-*-verified.log`.
Original path: `planned/0013_session_list_metadata_refresh.md`; final path: `completed/0013_session_list_metadata_refresh.md`.
ADR impact remains none: the existing UI-only engine and Gateway-owned execution boundaries
are preserved. Tool/session preferences remain client-local as tracked in proposed0004.
