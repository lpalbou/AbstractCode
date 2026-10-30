# 0025 — Shape-only companion overlay

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
- **Area:** AbstractCode TUI / AbstractTUI
- **Work item:** abstractcode-0025
- **ADR impact:** none expected; preserve generic presentation and canonical Gateway ledger ownership.

## Summary and current code reality

The companion currently clears a bounded rectangular surface in Code and the Avatar sampler returns opaque theme background outside its procedural art. This hides transcript text that lies outside the visible avatar silhouette.

## Scope

Add opt-in generic Avatar background transparency using existing mosaic/rendering capabilities; skip empty cells, preserve antialiased materials, remove host rectangle fills, retain opaque preview defaults and existing keyboard/control behavior. Avoid new terminal protocols, dependencies or global input infrastructure.

## Validation

Actual transcript text/styles survive in empty avatar cells; art remains legible in both styles/themes; moving/hidden art restores underlying text without trails; top/bottom positioning, resize, reduced motion and controls remain correct.

Two adversarial workers investigate these related user reports independently.
Local installation only after review and checks; preserve live sessions and audio.

## Completion report

Added opt-in Avatar::transparent_background(true) and matching Button support in
AbstractTUI. Procedural samples outside the art are transparent; fully empty mosaic
cells skip writes so underlying glyphs, colors and attributes survive. Antialiased
occupied edges keep their alpha and upper-only half-blocks keep visible foreground
ink. Existing opaque previews remain the default. Button idle/hover painting no
longer clears a blank label row; focus/press retains a selection cue. The caption
uses only its own padded text bounds for legibility, never a full-width strip.

Code removes the enclosing rectangle fill and opts into both generic APIs. Main
transcript dimensions, existing positioning and accessible input ownership remain
unchanged. Occupied terminal cells replace a whole glyph, including glow/edges;
this is shape-only cell compositing, not a new pixel graphics protocol. The picture
button retains its existing rectangular hit bounds; no input subsystem was added.

Validation: 28 style/pose/light cases preserve exact corner text/colors/BOLD;
Driver tests verify animation damage, old-silhouette restoration, theme changes,
hidden cleanup and reduced-motion quietness. The real Code root test checks both
styles and both positions against underlying transcript cells, then verifies exact
restoration when hidden. Actual dense-transcript SVG/PNG inspected, including the
tight caption. Native installed captures pass at220×60 truecolor and80×24 ANSI256,
with saved position changes, drawers, input and live resize. Engine205b555.

Final integration: Code all-targets1217 passed/23 ignored; AbstractTUI2538 passed/
89 ignored; both all-target Clippy, formatting and diff checks passed. Two explicit
adversarial workers implemented separate faults and independently reviewed each
other; root inspected real artifacts and ran broad/native checks. Final verdict:
Approved. Installed SHA25694d0f5f9badcc961a074428cb76ba8948fec02c20b4f85566bc8238755d65ad9.
Local installation only; no release/push or live user-process/Gateway restart.
Private preferences and synthetic audio fixtures preserved user settings, runs and
physical playback. No inference was started during native validation.

Documentation, existing package skills and LLM aggregates updated. Evidence:
workspace untracked/shape-answer-review.md, shape-overlay-qa/ captures/reports,
shape-overlay-engine-all.log, shape-answer-code-all.log, matching Clippy/build/
install logs, and final-answer-visibility/report.md, sanitized-comparison.json and
actual-replay.log. Original path planned/0025_avatar_shape_overlay.md;
final path completed/0025_avatar_shape_overlay.md. ADR impact remains none.
