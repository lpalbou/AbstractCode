# Speech interruption and current-audio controls

Root causes: plain submission/start/steer paths did not call the existing shared
speaker stop operation; the meter used all spare row width and placed its control
at the far right. The existing generation cancellation and audio owner were sound
but were never invoked at this ownership boundary.

New messages invoke the shared stop before dispatch/buffering. It clears queued
narration before publishing Idle and invalidates old callbacks. Queue/goal starts
and explicit steering use the same stop. No new audio transport or cancellation
mechanism was introduced. Duplicate calls are idempotent and cannot spawn a bridge.
Typing or navigation leave audio alone. Microphone/side-conversation policies stay
in their existing owner.

The speaking meter has a bounded preferred width, followed by the existing shared
Pause/Resume button and a flexible hint. Recording keeps its existing meter/hint.
Only the spectrum subscribes to level updates, so a press survives changing audio.
Narrow layouts shrink the histogram while preserving the button.

Observed validation: all1231 Code tests pass (23 existing ignored);44 voice checks
pass (6 ignored). New checks submit during Idle/Starting/Running and while
Playing/Paused/Preparing, verify the host Stop, queue clearing, no late segments
and preserved new speech identity. Typing does not interrupt. Pause/resume retains
the selected generation and does not synthesize a new reply. Controls remain1–4
cells after the last bar at40/80/220 columns, survive meter changes between down/up,
and disappear when idle/preparing/recording. All-target Clippy with warnings denied,
formatting and diff checks pass. [Receipt](validation.json).

Inspected actual composed terminal frames: [40 columns](speech-meter-40.png) and
[220 columns](speech-meter-220.png). The virtual terminal capture's fixed backing
size retained stale cells after resize; resize assertions use the composed frame,
while the existing80-column input checks still exercise emitted bytes and host
operations. The images use the capture's built-in font; real glyphs depend on the
terminal font. Physical audio was not replayed: this change reuses the existing
stop/pause bridge, exercised through the fake host and streaming gateway fixtures.

Solo review across correctness, ownership and user/operations. No independent
review claim or unresolved finding in this scope. Verdict: Approved for local
installation. No release, push or running session restart; concurrent web changes
were preserved.

## Session-boundary follow-up

The shared session reset omitted the existing speaker stop, so `/new`/`/clear`
and session switching left current speech alive. The shared reset now stops the
host and clears queued narration before clearing the old session state. Existing
speech generations reject late chunks/completions. Nine regression cases cover
all three entry paths while Preparing/Playing/Paused, immediate idle/identity
reset, host Stop and no later playback or queued TTS request.45 voice checks and6
session-screen checks pass;6 existing audio/live tests remain ignored. Clippy
with warnings denied and formatting pass. Rebuilt and installed locally; no user
process restarted. [Receipt](new-session-validation.json). Approved for this scope.
