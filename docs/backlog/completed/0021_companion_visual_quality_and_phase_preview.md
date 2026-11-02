# 0021 — Companion visual quality and watch-style phase preview

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
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

## Completion report

Delivered watch-inspired shaded Halo/Hover artwork with theme-cached material
ramps, antialiased silhouettes, soft corona, red robot eyes and thrusters. Generic
Avatar defaults to 28×15; Code uses 28×15 or 32×17 when space allows. Raster buffers
are reused and bounded at 64×32 cells; Code's preview is capped at 48×25.

Appearance opens a silent local preview with both styles, all seven phases,
3.5-second cycling, manual hold, pause and keyboard controls. The caption separates
In use from Not applied; only Use this saves appearance. Cancel/Escape preserves
it. The engine clamps the 96×42 request consistently at open and resize, keeping
controls reachable down to 80×18. Panel and artwork share an explicitly opaque
composited theme ground. Preview has no recorder, player, inference or task command.

Validation: broad Code suites 1194 passed / 23 ignored, engine 2531 passed /
89 ignored. Final host changes passed the ten companion integration tests and
all-target Clippy with warnings denied; engine all-target Clippy/fmt passed too.
Four independent probes cover keyboard, short layout, resize, commit/cancel and
clock ownership. Real installed PTY full-flow checks cover phases/hold/cycle,
persistence, pause and resize across 220×60, 80×24 and 80×18. Dark truecolor,
256-colour and light/monochrome profiles were inspected. Final ground-only
wide/narrow visual captures match installed binary SHA256
3e9319fc108aebcab2813388ec7053907a5888b87ff34cdfa262764c9dd08f8e.
Independent review Approved, with no remaining blocker.

Performance: isolated 32×17 truecolor active rendering averaged 221–457µs per
Driver turn at roughly 12fps; Ready/hidden/reduced cases emitted zero frames,
bytes or pending timers after setup. Rich foreground art increases ANSI traffic;
this is not watch pixel/vector fidelity. Earlier full-flow capped-preview captures
measured zero output while paused; their metrics and hashes are kept distinct
from the final background-only build. An existing underlying Docs-gear tooltip
can remain below the narrow preview without obstructing controls; no tooltip
lifecycle redesign is included here.

Evidence: workspace untracked/avatar-preview-{code,engine}-all-tests.log,
avatar-preview-{code,engine}-clippy.log; avatar-quality-review.md;
avatar-widehost32-benchmark.json; avatar-preview-qa/validation-review.md and
independent-review.md, matching ground-*/ground-capture.json and PNGs. Test
settings were private copies; no user run, approval, saved preference, inference
or audio was changed. Docs and existing package agent skills updated using
coredoc; LLM aggregates regenerated. Installed locally, no release/push or Gateway
restart. Engine commit 823e706. ADR impact remains none.

Original path: planned/0021_companion_visual_quality_and_phase_preview.md;
final path: completed/0021_companion_visual_quality_and_phase_preview.md.
