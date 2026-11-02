# 0019 — Bounded read-only session history for the side assistant

- **Status:** proposed
- **Created:** 2026-10-30
- **Area:** Code TUI, Gateway session reference
- **Work item:** abstractcode-0019
- **ADR impact:** evaluate read authority and context budgets before implementation.

## Summary and code reality

Both side assistant scopes include recent visible requests/replies (0017), capped
at24000 message characters with honest omission markers. Code already loads older
turns through the Gateway and exposes history_loading/older_turns. Grounded QA is
a tool-free docs workflow; it cannot independently browse omitted session history.

## Design investigation

Compare an explicit Earlier messages control using existing authorized history
reads, a bounded reference-page provider, and a dedicated read-only history tool
for an appropriate side-assistant workflow. Prefer existing APIs. Scope every
read to the authenticated principal and captured session; enforce page/character/
request budgets, stable cursors and cancellation/source-session checks. Return
user/assistant messages and provenance only, not tool payloads, filesystem data,
hidden reasoning or arbitrary session access. Preserve the manual coding task's
state and approval boundaries. Do not bolt broad tools onto docs-qa just to add
history. Shared history projection belongs at a reusable Gateway boundary if
several clients need it; engine remains presentation-only.

## Validation and non-goals

Future tests: old question can be answered after bounded retrieval; a client
cannot read another principal's unauthorized session; large histories remain paginated;
stale replies do not cross sessions; missing pages are explicit; no Start/Steer/
Resume, tool execution or workspace read. No implementation of a new history
tool is included in0016; context remains honestly bounded until this is approved.
