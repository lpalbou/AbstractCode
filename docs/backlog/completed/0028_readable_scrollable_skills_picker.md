# 0028 — Larger, readable Skills picker with mouse scrolling

- Status: completed
- Completed: 2026-10-30
- Created: 2026-10-30
- Area: AbstractCode TUI
- Work-item id: abstractcode-0028
- ADR impact: none; use the existing engine List and skill attachment contract.

## Summary and current code reality

The operator requests a larger Skills modal with better readability and working
mouse-wheel scrolling. `open_skills` currently uses an 84×24 dialog and `draw_rows`,
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

## Completion report

Implemented in f4a207f and c0d6b7d: a viewport-clamped 128×42 picker using the
engine List, separate scrollable details, explicit mouse Attach/Detach, ordinary
wheel/scrollbar/paging behavior and complete multiline descriptions from discovery.
Selection and scrolling never attach; Space, Enter/Escape, blocked trust checks,
preference persistence and run propagation retain their contracts.

The pinned clean-source full suite passes 1221 tests (23 existing ignored), Clippy
with warnings denied and formatting pass. Dark/light captures inspected; real
keyboard/mouse tests cover 80×24 through 180×60, scroll in both directions, drag to
the tail, independent detail scrolling, catalog insertion identity and blocked
attachment. Native installed-binary PTY check passed with isolated settings and
zero run starts. Atomically installed locally; no running session restart or release.

[Review, screenshots and receipts](../../../design/skills-picker-0028/REVIEW.md).
No additional follow-up or ADR change required.

## Operator refinement

2026-10-30: after approving the larger picker, the operator requested description
space at 40%. Implemented ffc12be with a proportional height, preserving list
scrolling. The 6 affected Skills tests pass; dark/light captures reviewed and
the binary rebuilt from clean sources and installed locally. Receipt and updated
screenshots are in the linked review folder.
