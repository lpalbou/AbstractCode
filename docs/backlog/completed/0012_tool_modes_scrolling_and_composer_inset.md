# 0012 — Tool modes, usable list navigation and composer inset

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
- **Area:** tui, AbstractTUI
- **Work item:** abstractcode-0012
- **ADR impact:** none; retain existing Gateway tool allowlist and approval-policy contracts.

## Summary

Add a real cell between the composer stroke and typed text. Enlarge the Tools modal, use the engine List
for wheel/keyboard navigation and show explicit Off/Ask/Auto choices per tool and category.

## Current code reality

Baseline bc44085 is clean. Tool descriptions come from discovery/tools (first description line), with Code
adding gate/status labels. Tools uses custom draw_rows and fixed 84x26 sizing, so it bypasses List's built-in
wheel handling. Selection is disabled_tools plus auto/ask pins; request construction already sends allowlists
and _runtime.tool_policy. TextArea and TextInput have no inset knob in checked-out AbstractTUI 0.7.0; engine
0186 remains proposed. Implement that small widget contract and validate locally against it; no publishing.

## Scope and validation

Use one mode helper over existing policy state, preserve server-disabled clamps, apply categories only to
eligible members and do not alter the active run implicitly. Show categories and full selected-tool details
without squeezing essential controls into a clipped title. Test modes through request serialization,
category mixed/gated states, persistence, real mouse-wheel behavior, narrow/large resizing and input geometry
(wrap, caret and completion anchor). Build/install locally with the sibling engine, no release or push.

## Completion report

Implemented TextInput/TextArea inner inset and applied it to Code composer. Replaced custom tool rows with engine List, a viewport-sized modal, explicit Off/Ask/Auto choices and category edits over current eligible tools. Existing Gateway policy/request contract remains authoritative.

Tool mode request-wire, category/gated/mixed-state and persistence tests; actual terminal mouse wheel and composer capture. Tool descriptions remain Gateway discovery first lines, with Code availability/sandbox labels. Engine 0186 retains proposed style/token and mouse-caret extensions.

Validation: AbstractTUI workspace/all-targets 2523 passed, 89 ignored; Code 1159 passed,
23 ignored; Gateway console 1013 passed, 76 ignored; Core console 244 passed, 2 ignored
(including doctests). Engine, Code and Gateway all-target Clippy passed with warnings denied.
Docs: engine API/README/agent skill, Code API/architecture/README, console README and
changelogs updated; LLM aggregates regenerated. Code and Gateway binaries installed locally.
No release, push, Gateway restart, live approval, coding run or user-preference mutation.
Source dependency floors are unreleased and use explicit local Cargo patch arguments.

Evidence: workspace `untracked/chat-20261030/` contains architecture self-review, PTY
captures and live smoke JSON receipts; suite logs are `untracked/chat-*-verified.log`.
Original path: `planned/0012_tool_modes_scrolling_and_composer_inset.md`; final path: `completed/0012_tool_modes_scrolling_and_composer_inset.md`.
ADR impact remains none: the existing UI-only engine and Gateway-owned execution boundaries
are preserved. Tool/session preferences remain client-local as tracked in proposed0004.
