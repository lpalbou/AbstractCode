# 0029 — Shared watch-inspired avatar catalog and rendering

- **Status:** completed
- **Created:** 2026-10-30
- **Area:** AbstractTUI, AbstractUIC, Code companion
- **Work item:** abstractcode-0029
- **ADR impact:** AbstractTUI0006; rendering only, no identity/preference authority change.

## Summary and current code reality

Code currently exposes Halo/Hover. Their geometry lives in the terminal widget,
while watch SwiftUI has Vortex, Eclipse, Eye and Rover among its larger catalog.
The user requests more models and reusable visuals for TUI, WUI and systray apps.

## Scope

Extract one generic geometry/catalog core; add those four watch-inspired styles.
Keep terminal/Canvas/React adapters thin, expose optional native C use, and ship a
local browser gallery to prove reuse. Preserve existing keys, transparency, all
seven readable phases, bounded animation, reduced motion and hidden cleanup.
Expose every style in Code's existing silent preview and saved preference. No
new inference/service, automated listening, global preference migration or full
application rollout. Existing identity cleanup remains tracked separately0018.

## Validation

Native/Wasm pixel parity with equal palette/input; malformed ABI bounds/handle
cleanup; all styles/phases at main/preview sizes; visible shape/motion distinctions;
terminal transparency, resize and existing workflows; browser lifecycle/reduced
motion/context loss, real gallery screenshots; targeted/full checks and local
install. Commit completed work per repository, no releases or pushes.

## Completion report

Added four watch-inspired models (Vortex, Eclipse, Eye, Rover) alongside Halo/Hover.
One independent Rust geometry/catalog core supplies AbstractTUI plus bounded optional
native C/Wasm adapters. AbstractUIC now exports themed Canvas/React hosts and a local
six-model browser gallery. A reusable Qt example proves native reuse. Existing
watch scenes, client audio/inference and shared identity ownership stay separate.

Code lists all six designs in its silent preview, saves stable keys and renders
the selected design. Hover/Rover Error uses red X eyes; Eye uses one central X.
Ready eyes follow actual pointer motion, center after2s idle, and stop tracking in
other phases/reduced motion/hidden views. Previews support Ready tracking too.
Terminal failure preserves Error through the final summary. No pointer event
starts a run or records audio.

Code1224 tests and engine2706 tests pass (23/89 existing ignored), core FFI6 tests,
all-target Clippy and formatting pass. Native/Wasm54 frame fixtures match exactly;
real Chrome gallery, reduced motion,2s expiry, context recovery and repeated disposal
pass; Qt42 frames and lifecycle pass. Installed native PTY smoke exercises all six
styles, actual1003 motion enablement and clean exits with isolated settings and
zero execution requests. No running session restart, release or push.

[Review, screenshots and machine-readable receipt](../../../design/shared-avatar-0029/REVIEW.md).
Shared appearance/voice authority and broad client migration remain in0018.
