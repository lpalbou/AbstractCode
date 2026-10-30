# 0025 — Shape-only companion overlay

- **Status:** planned
- **Created:** 2026-10-30
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
