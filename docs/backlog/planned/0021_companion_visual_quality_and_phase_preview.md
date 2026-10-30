# 0021 — Companion visual quality and watch-style phase preview

- **Status:** planned
- **Created:** 2026-10-30
- **Area:** AbstractTUI Avatar, Code companion settings
- **Work item:** abstractcode-0021
- **ADR impact:** none; generic rendering remains in AbstractTUI and local preferences remain in Code.

## Summary and current code reality

The operator rejects the tiny, coarse avatar in the installed Code terminal.
The host allocates only 18×9 cells and the engine limits its sparse token-coloured
raster. Appearance currently toggles immediately without a visual preview.
The watch's OrbStylePreview already demonstrates the desired interaction: a
large preview, automatic phases, manual phase hold, and explicit Use this.

## Scope

Improve continuous shaded curves, glow and recognisable watch-inspired Halo and
Hover geometry through the existing portable terminal renderer. Enlarge the
host avatar appropriately. Appearance opens a large, silent preview with both
styles and all seven labelled phases. Automatic cycling and manual hold work
without inference or microphone access. Previewing and cancelling do not save
appearance; Use this saves it. Reduced motion and modal close stop preview work.

## Validation and boundaries

Inspect actual terminal cell captures at normal and narrow sizes, both styles,
active and static poses, dark/light themes, keyboard and mouse. Check explicit
commit versus cancel, no inference or main-run mutation, and timer cleanup.
Retain the engine's producer-flood and hidden/reduced-motion guarantees. Run
appropriate suites and Clippy, update docs, install locally. No image assets,
graphics dependency, protocol service, release or user preference mutation by
test harnesses. Earlier completion 0016 is preserved; this item records its
visual and discoverability follow-up.
