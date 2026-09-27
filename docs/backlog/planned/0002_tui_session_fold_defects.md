# 0002 — TUI session fold: child runs counted as turns, id-less runs counted, string timestamp ordering, first run by page order

- **Status:** planned (not started)
- **Created:** 2026-09-27
- **Area:** TUI (`tui/src/runner.rs` `fold_session_rows` ~:715, `tui/src/gateway/mod.rs` `list_recent_runs` ~:865/:874)
- **Work item:** `abstractcode-0002`; related `abstractassistant 0853` (the Assistant's fold was compared against this one)

## Summary
While reviewing the Assistant's new gateway-first session list (abstractassistant 0853, review 37 in the framework repo:
`untracked/missions-2026-09-25/REVIEW/37-assistant-gateway-first-sessions.md`), the TUI fold was ported line by line and run on twelve
edge-case `/runs` pages next to the web fold (`web/src/workspace/catalog.ts:599`) and the Assistant's. The TUI is the odd one out:
1. it keeps child runs (rows with `parent_run_id`) and counts them as turns, although the pinned query asks for `root_only=true` (a
   defensive fold must still drop them: the gateway's index path and JSON path differ);
2. it counts runs with no `run_id`;
3. it orders by comparing timestamps as strings, so mixed time zones / missing `updated_at` sort wrong;
4. it picks the session's "first run" (the prompt/title source) by page order instead of the earliest `created_at`.
The web fold and the Assistant fold agree with each other on all four.

## Scope
Align `fold_session_rows` with the web fold: skip rows without `run_id` or `session_id` and rows with a `parent_run_id`; parse timestamps
(RFC 3339 → UTC) before comparing, fall back to `created_at`, tie-break by id; first run = earliest `created_at`; state = the liveliest
run (waiting > running > failed/cancelled > completed; missing = unknown). Keep the pinned query string test
(`the_session_listing_query_is_exactly_what_the_gateway_accepts`). Out of scope: `session_kind` (gateway contracts C11, later).

## Validation
A fixture-driven test over the same twelve edge-case pages (share the fixture shape with the Assistant's
`tests/basic/fixtures/gateway_runs/`), asserting identical rows to the web fold's expected output.

## Related
abstractassistant 0853; abstractframework backlog 0928 (contracts C11 `session_kind`); 0001.
