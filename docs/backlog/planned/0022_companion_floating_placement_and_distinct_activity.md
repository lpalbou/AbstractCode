# 0022 — Floating companion placement and distinct activity poses

- **Status:** planned
- **Created:** 2026-10-30
- **Area:** Code companion layout, AbstractTUI Avatar
- **Work item:** abstractcode-0022
- **ADR impact:** none; reuse existing absolute layout and generic presentation component.

## Summary and current code reality

The operator reports a strip below the header, a tiny right-shifted companion,
and indistinct Listening/Ready/Waiting/Working appearances. The host adds Avatar
beside the internal label child of an empty Button, so flex layout squeezes the
art to half width and paints the blank label as a strip. Its ordinary sidebar
also shrinks the transcript and aligns controls separately from the picture.

## Scope

Use the existing root-tree absolute positioning for a bounded floating companion
centred beneath the right header action group. Keep the normal transcript width,
centre caption and one combined action row, and fill the avatar button without
an extra in-flow label column. Respect resize, compact presentation, modal/drawer
priority, focus and cleanup. Main artwork should match the preview at about 70%
of its size. Listening visibly leans toward the user, Waiting differs from Ready,
and Working hovers with bounded procedural particles. Retain the existing frame
clock and static/hidden/reduced-motion guarantees.

## Validation and non-goals

Actual terminal geometry/captures and keyboard/mouse tests must show no strip,
no transcript narrowing, centred art/actions, correct click bounds, composer and
header accessibility, resize and restoration after hiding. Inspect pose previews
at 32×17 and 48×25. Reuse engine overlays/layout instead of adding another focus
owner, backend, dependency, graphics service or model. No user-state mutations by
tests, release or Gateway restart. Follow-up to completed 0021.
