# 0022 — Floating companion placement and distinct activity poses

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
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

## Completion report

Replaced the in-flow sidebar with a bounded opaque floating surface in the
existing root UI tree, using AbstractTUI absolute positioning. Its centre comes
from the actual right header action plan; art, caption and combined Talk / ask
plus gear row align together. Transcript dimensions remain unchanged. Avatar
fills its existing Button absolutely instead of competing with Button's label
child; the label stays behind the bottom caption for visible keyboard focus.
No extra overlay focus owner or engine positioning API was introduced.

The detached dome stripe was also reproduced in terminal font rendering:
upper-half foreground with a brighter lower background exposed a small ascender
strip. Equivalent lower-half foreground encoding removes it. Hover visibly
leans with a raised hand while Listening; Waiting has a lower held posture,
dimmed downward gaze and three static dots; Working bobs with extended arms,
stronger plumes and six deterministic sparks. Existing animation cadence and
static/hidden/reduced-motion guarantees remain.

Validation: Code all-targets 1196 passed/23 ignored; engine 2533 passed/89 ignored;
both all-target Clippy and formatting passed. Twelve companion integration tests
include actual accessibility/action-bound geometry proving unchanged viewport,
full art/Button bounds, measured header centre, same-row actions and root
keyboard access after a cancelled art press. Independent activation/resize/draft
probes passed. Final installed PTY captures at 220×60 truecolor and 80×24 ANSI256,
with live 80×18 resizing, match SHA256
3fd38aa6657560751c4591015e2adf7577a93b1004311fb23a5f4beec1b7cd67.
Both exits 0, no user prefs, inference, audio or live task mutation. Independent
review Approved. Local install only, no release/push or Gateway restart.

Working isolated 32×17 drawing averaged about 491µs per Driver turn, 48×25 about
633µs, at 71 frames/six virtual seconds. This richer art costs more ANSI output;
Ready in a loaded-transcript fixture emits zero idle bytes. Whole-app empty
welcome captures still include the existing 150ms logo shimmer; do not attribute
that traffic to the avatar. The bounded opaque card deliberately obscures its
small rectangle of transcript; it does not reflow it.

Evidence: workspace untracked/avatar-overlay-{code-all,engine}-tests.log,
avatar-overlay-{code,engine}-clippy.log; avatar-poses-{32x17,48x25}.png;
avatar-pose-review.md; avatar-overlay-qa/validation-review.md and
independent-review.md with matching native captures. Engine commit b533328;
regression tests 63efdda. Docs, existing package agent skills and LLM aggregates
updated. ADR impact remains none.

Original path: planned/0022_companion_floating_placement_and_distinct_activity.md;
final path: completed/0022_companion_floating_placement_and_distinct_activity.md.
