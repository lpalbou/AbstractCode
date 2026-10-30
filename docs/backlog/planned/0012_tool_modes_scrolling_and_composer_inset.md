# 0012 — Tool modes, usable list navigation and composer inset

- **Status:** planned
- **Created:** 2026-10-30
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
