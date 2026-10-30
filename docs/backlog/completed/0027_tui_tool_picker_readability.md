# 0027 — Folded, readable terminal tool categories

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
- **Area:** AbstractCode TUI tool picker
- **Work item:** abstractcode-0027
- **ADR impact:** none; retain the existing engine List and tool policy authority.

## Summary and current code reality

The operator's screenshot shows a dense, full-width list with every description
visible, category chevrons that do not fold, and a green selection row obscuring
mode distinctions. Inventory ordering can also create duplicate category headers.

## Scope and boundaries

Start every visit with collapsed categories. Expand/collapse with Enter/Space,
left/right arrows or the category's mouse accessory. Keep category and individual
Off/Ask/Auto controls and Gateway availability constraints. Give modes distinct
semantic colours while leaving tool names neutral; indent tools below categories,
put descriptions in the selected detail pane, and bound width on large terminals.
Do not change the Gateway policy, tool execution, avatar, or browser client.

## Validation

Real engine keyboard/render tests for initially collapsed and re-opened state,
interleaved inventory grouping, category and individual mode changes, server gates,
selection on collapse, description disclosure, dark/light colours, and existing
run-start allowlist propagation. Verify formatting, Clippy and a readable capture.

## Completion report

The picker now groups interleaved inventories into unique sorted categories and
starts every visit collapsed. Enter/Space or the category mouse accessory toggles
children; left/right arrows fold categories and preserve parent selection. Stable
engine List keys preserve the selected tool when inventory changes. Category and
individual Off/Ask/Auto controls still use the unchanged tool-mode authority and
cannot grant a Gateway-disabled tool.

Tool rows use a visible tree guide with indented status/name, semantic Off/Ask/Auto
ink, neutral tool names and neutral selection background. Full descriptions,
sandbox labels and Gateway enable gates remain in the selected detail pane. The
modal is bounded to116columns/34rows; compact keyboard help fits80columns and uses
readable foreground instead of decoration ink. No new engine API or dependency.

Validation: all231 enabled existing headless UI tests plus4 new keyboard/mouse/
render tests passed (4 existing tests ignored), with dark/light semantic-colour
and neutral-name assertions,80/120column mouse expansion, grouping, collapsed
reopening, inventory insertion selection, category/individual policy preservation,
Gateway gates, long-list scrolling and actual run-start allowlist/pin propagation.
Clippy with warnings denied, scoped formatting and diff checks passed. Reviewed
actual exported dark/light SVG-to-PNG captures. Evidence is in
`/tmp/code-tools-0027-tests.log` and `/tmp/code-tools-0027-captures/` for this session.
Local binary installation is coordinated by the parent task to avoid overwriting
another agent's current build; no release, push or running-session restart.

Residuals: descriptions remain explicitly scrollable in the existing detail pane;
no separate search feature or permission-policy redesign was introduced. No new
follow-up or durable architecture decision is warranted by this presentation fix.
The parent task owns the concurrent overview/ledger update.
