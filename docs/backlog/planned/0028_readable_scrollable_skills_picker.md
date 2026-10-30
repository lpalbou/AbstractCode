# 0028 — Larger, readable Skills picker with mouse scrolling

- Status: planned
- Created: 2026-10-30
- Area: AbstractCode TUI
- Work-item id: abstractcode-0028
- ADR impact: none; use the existing engine List and skill attachment contract.

## Summary and current code reality

The operator requests a larger Skills modal with better readability and working
mouse-wheel scrolling. `open_skills` currently uses an84×24 dialog and `draw_rows`,
a custom cursor-windowed painter with no pointer handling. It concatenates names,
trust and descriptions into a single truncated line, with selected skills entirely
accent-coloured. The engine List already provides wheel scrolling, scrollbar drag,
keyboard navigation, mouse selection and stable selection keys.

## Scope and validation

Use the shared List, a larger viewport-clamped modal, clear skill names and a
separate scrollable description/trust detail pane. Keep Space attachment and
Enter/Escape closing, Gateway trust enforcement, persistence and run propagation.
Make mouse attachment explicit; browsing or scrolling must never attach a skill.
Verify long and changing catalogs, mouse wheel in both directions, scrollbar and
pointer selection, keyboard paging, dark/light rendering and narrow terminals.
Retain loading/empty/error feedback. Run affected and full terminal checks, build
and install locally without restarting a running user session.
