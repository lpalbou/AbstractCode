# Shared avatar delivery review — 0029

No unresolved blocking findings in the requested rendering/component scope.

- Execution mode: solo artifact review; the review skill was selected by the agent,
  not explicitly invoked by the user for this task. No independent review claim.
- Planned / actual reviewer set: correctness, architecture fit, user/operations,
  all inspected by the implementing agent. Subagents used: none for0029.
- Fallback reason / contract degradation: none; mandatory explicit-skill swarm
  contract did not apply.
- Final verdict: Approved for local installation and shared-component availability.

## Correctness evidence — Approved

See [validation.json](validation.json) for exact source revisions, binary hash and
counts. Code1224 and engine2706 tests pass, with23/89 existing ignored; core FFI6
passes. All-target Clippy with warnings denied and formatting pass. UI-kit npm test
passes. Native/Wasm54 fixture renders match exactly, including gaze. ABI checks
cover capacity16, dimensions, invalid inputs, palette completeness and destroyed
handles; the Wasm has no host imports. Pixel storage is bounded and reused.

Fake-clock tests cover actual two-second expiry and repeated cursor movement
without starving the animation clock. Red X samples distinguish diagonals from
filled circles. A reduced-motion regression checks that an existing gaze recenters
immediately. Code integration checks six stable saved IDs, real SGR mouse reports,
terminal failure state, transparent restoration and no task creation from pointer
activity. Full tests passed after the final production-source change.

## Architecture fit — Approved

AbstractTUI ADR0006 defines the source of truth: std-only `abstract-avatar` owns
geometry/catalog/material samples; adapters own palette, scheduling and disposal.
Rust TUI, Wasm Canvas/React and the Qt C-ABI example share that core. No Gateway,
voice, inference, preference or agent authority enters the geometry. Wasm source
and binary hashes are verified; normal npm builds require no Rust. Package dry
run includes the artifact/provenance and excludes the browser demo. Native core
packages successfully offline.

The strongest alternative is separate native/JS artwork: easier language-local
builds but ongoing geometric drift. The shared core adds a generated Wasm artifact
and therefore a regeneration check. That cost is explicit in the docs/script and
covered by native/Wasm parity tests. Shared identity and cross-client preferences
remain the separate0018 proposal; existing watch SwiftUI is a reference, not migrated.

## User and operations evidence — Approved

Inspected [browser Ready gallery](web-ready.png), [browser Error gallery](web-error.png)
and the [actual terminal-cell Error catalog](tui-error.png). New silhouettes differ;
Hover/Rover show red X eyes, Eye one central X. The same design/phase can be previewed
without audio or inference. Code saves only through Use this; Cancel/Escape preserve
settings. Actual installed native PTY launches show all six designs, enable1003
mouse movement and exit cleanly, with private preferences and zero run starts.

Headless Chrome exercises six styles/seven phases, actual pointer input,2s idle
return, reduced motion, context loss/recovery, theme changes and five mount/dispose
cycles with no console errors. Ready produces12 frames across six canvases in1.1s;
tracking30 in500ms, returning to12 in1.1s after idle. Reduced motion produces0 timer
frames. Qt42 frames and explicit gaze/reset/hide/disposal checks pass.

## Residual risk and scope

Terminal artwork remains cell-resolution; antialiased occupied cells replace
underlying glyphs. Physical Terminal.app mouse appearance was not re-recorded;
PTY input and actual terminal adapter output were verified. Rendering tests do not
claim microphone/TTS validation, Watch migration or completed WUI/systray rollout.
Browser bundlers must preserve the Wasm asset URL and CSP must permit Wasm.
Hosts must bind application-specific hidden state. These limits are documented in
AbstractUIC's avatars guide and the generic core README.

The local binary was installed without restarting user processes. Reopen the TUI
to load it. There was no publication, remote push or change to shared identity.

## 2026-10-31 refinement — live error visibility and applying a design

The prior renderer correctly drew red crosses for Error, but Code's observed pose
only recognized terminal run failure; recoverable failed tools did not enter it.
The host now observes its existing per-run tool-failure counter and terminal
failure flag, holds each new live indication for1s ahead of activity/speech, and
returns to observed activity. Further failures extend the hold. One cancellable
UI-thread timer is independent of animation and avatar remounts. Historical replay,
session/run switches, disable/hide and disposal avoid stale cues or queued clocks.
No run verdict or audio playback is changed by the cue.

The preview previously autofocus-selected its first button even when another design
was rendered. It now autofocuses the saved design and adds a persistent selected
marker. The explicit Apply avatar button saves then closes to show the artwork;
Cancel/Escape still discard changes.

All1228 Code tests pass (23 existing ignored), including22 companion checks.
Clippy with warnings denied and formatting pass. Fake-clock checks cover999ms
still Error/1000ms recovery and repeated failures extending the deadline. Native
installed PTY checks prove Hover initial selection, applying Vortex, saving to
private preferences, immediate modal closure and Vortex selection on reopening;
zero run creation and clean exit. Receipt: [error-hold-validation.json](error-hold-validation.json).
Native screens: [Hover](native-preview-hover.txt), [saved Vortex](native-preview-vortex.txt).
Verdict: Approved for local installation; no unresolved finding for this scope.

## Enter confirms a focused avatar design

Operator clarified that Enter on a highlighted design must confirm it. Design
rows now capture plain Enter ahead of Button's generic preview activation and
reuse the Apply avatar save/close path. Mouse clicks keep previewing; phase,
Cancel and motion controls retain their own Enter behavior. The hint names these
actions. All23 companion tests, all-target Clippy and formatting pass. Installed
native PTY validation clicks Vortex then presses Enter, confirms persistence and
closure, reopens on Vortex and confirms initial-focus Enter also applies. Receipt:
[enter-apply-validation.json](enter-apply-validation.json). Approved; no rendering
or protocol change. Unrelated concurrent web edits were preserved.
