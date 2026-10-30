# 0029 — Shared watch-inspired avatar catalog and rendering

- **Status:** planned
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
