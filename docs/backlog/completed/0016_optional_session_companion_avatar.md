# 0016 — Optional session companion avatar and change-driven advice

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
- **Area:** tui + AbstractTUI presentation
- **Work item:** abstractcode-0016
- **ADR impact:** evaluate if promoted; keep generic presentation in engine and execution in Gateway.

## Intake request and code reality (before implementation)

The operator asks whether a top-right pet/orb can reflect ready, thinking, working,
listening and speaking, offer occasional session advice, and have its own speaking
voice and conversational microphone destination. This item records feasibility and
a small staged design; no observer, inference timer or microphone is enabled.

AbstractTUI already exports cell shaders, DotCanvas, AnimatedImage (GIF/APNG),
capability-probed image protocols and software 3D. AnimatedImage supports a bound
playing signal. An attractive procedural orb is feasible without a new graphics
library; a watch-quality SwiftUI scene is not directly portable. Small terminals
have limited resolution: a cell-based orb with a status label is the portable
baseline, with pixel rendering only when terminal capabilities support it.
Watch examples are `abstractassistant/watch/apple/Sources/App/Shared/Orb.swift`,
OrbAvatars, OrbRobots and OrbScenes. Code already has Phase, ledger-derived Fold,
voice Dictation/Speaking state, generic ChatPanel, separate This run grounded QA,
and one host audio queue. The engine itself owns no inference or microphone.

## Smallest useful design

1. Opt-in visual component, configurable size/style and reduced motion. Generic
   engine input is an explicit pose plus optional amplitude, never Code's Store.
   Code maps observed run and voice state to ready/listening/speaking/working/
   thinking and exposes waiting/error distinctly. These are UI activity labels,
   not claims about hidden reasoning. No model call is needed for poses.
2. A click opens the existing assistant's This run scope. Manual questions work
   before enabling any unsolicited advice. Collapse to a labelled compact status
   control at narrow sizes. No occlusion of topbar controls or approval prompts.
3. Optional advice uses the existing Gateway-owned grounded question path. The
   client records the last meaningful session revision, a dirty flag, one active
   request, and a monotonic earliest-next-request time. Eligible only if visible,
   enabled, changed since the last attempted snapshot, not restoring/replaying,
   and at least 60 seconds since the previous attempt. Consume the captured
   revision on attempt (failures must not create rapid retries); changes during
   inference coalesce into the next eligible snapshot. Timers exist only for a
   dirty pending revision, never to poll an unchanged session. Suppress redundant
   comments and cap suggestion size/history. Late results from an old session or
   disabled companion are discarded using operation/session identity.
4. Use the same Gateway resource pool; no local model, provider instance, daemon,
   process, agent loop or tool execution. Treat observed logs as untrusted data.
   Reuse honest snapshot limits, reject/label missing context and expose request
   failures/cost. Default advice and unsolicited speech off. Decide the shared
   multi-client dedup requirement before adding any persistence or server policy.
5. A companion voice preset is a host preference passed to existing TTS, not a
   second audio pipeline. All speech shares one queue/stop owner to avoid overlap.
   Conversational listening is explicit push-to-talk routed to the side question
   scope; it must be visually and functionally distinct from dictate-to-composer.
   It stops playback before recording and never starts an always-on microphone.

## Alternatives

| Option | Benefit | Cost / decision |
|---|---|---|
| Status orb + manual side questions | Existing state, no extra inference | Recommended first increment |
| Opt-in change-driven advice | Context-aware suggestions | Add only after throttle/cost/UX proof |
| Separate resident pet agent or model | Independent persona loop | Reject: duplicate orchestration and resource ownership |
| Require 3D or a specific image protocol | Rich visuals | Reject as baseline: SSH/tmux and terminal portability |

## Validation before promotion

Headless pose/priority tests; resizing, themes and reduced-motion snapshots;
prove idle/hidden animation stops. Fake monotonic clock: unchanged/replayed
records cause zero requests, changes coalesce, no overlapping request and no
more than one attempt per 60 seconds even on errors or rapid session changes.
Manual keyboard/mouse parity, clear opt-in and dismiss controls. Verify no
coding Start/Steer/Resume or tool call from companion questions, no stale reply
crossing sessions, no separate model instantiation, no overlapping speech and
no microphone capture without an explicit action. Measure renderer CPU and
request/context cost before selecting animation frequency or enabling advice.

## Implementation authorization — 2026-10-30

Operator approved all six roadmap phases after0017, with worker agents. Deliver Halo and Hover styles, state-driven presentation, optional bounded change-driven advice, side conversation microphone/TTS, local install and tests. Existing Gateway entity voice/appearance ownership is preserved; cross-client identity cleanup is recorded separately. No release/push or new service/model pool.

## Completion report

Implemented all six local roadmap increments: watch-inspired Halo/Hover procedural
rendering; generic framework-independent Avatar API and standalone example;
observed Code activity mapping, resizing, modal/drawer priority and preferences;
optional meaningful-change-driven grounded comments; independent companion voice
selection and explicit side-destination microphone; integration/performance/audio
validation and documentation. All inference reuses existing Gateway QA/resource
ownership. No tools, approval or steering authority is granted to side comments.
The engine adds no network/audio/model dependency. Main and side speech share one
ordered queue. Source/session/reset/opt-out guards discard stale input and speech.

Five worker agents contributed renderer, pure policy, voice routing, independent
review and host integration tests. Parent integrated and evaluated their changes.
Independent review found/fixed modal scope lifetime, avatar click behavior, paused
motion, queued speech opt-out and live resize. Final producer-flood regression
prevents repeated equal state/audio writes starving the animation clock.

Validation: Code1192 passed/23 ignored; engine2529 passed/89 ignored. Both
all-target Clippy runs deny warnings. Gateway Docs11 regression tests passed.
The six ignored audio cases were explicitly executed and passed, including real
Gateway Supertonic streaming through the real Python playback bridge (two segments,
first audio1.267s in this run). Physical default speaker tone and999ms microphone
capture (15990 mono frames at16kHz) passed; recording discarded locally, no upload.
This proves device I/O, not subjective sound quality or live human speech recognition.

Installed TUI smoke passed both styles, stable gear/catalog selection, persisted
private settings, live wide/narrow resize and one icon-action row. Actual automatic
QA requests carried their captured source session and completed through Gateway;
a final-state change was coalesced into a later request. Exact quota is the client
monotonic-attempt limit, independently tested; server creation timestamps are not
that clock. A model-only test task explicitly sent tools=[] in an isolated session.
No operator run, approval or actual preferences were changed. No release or push.

Avatar-only benchmarks across16/256/truecolor show zero idle/hidden output/timers,
roughly12fps activity and bounded render work. Existing empty-session Code logo
animation remains independent; full-app output is not claimed zero. Evidence:
workspace `untracked/avatar-performance.md`, `untracked/companion-independent-review.md`,
`untracked/companion-20261030/` JSON receipts/captures and final suite logs. Final
installed binary hash matches the native live receipt. README/API/architecture,
package agent skills and LLM aggregates updated using coredoc.

Follow-ups remain proposed:0018 shared visual/voice identity extending existing
entity-home settings,0019 bounded authorized history retrieval,0020 cross-client
comment ownership/global dedup. No shared preference migration or backend rewrite
was performed. ADR impact none for this increment: existing engine presentation
and Gateway execution/identity boundaries are preserved. Original path:
`proposed/0016_optional_session_companion_avatar.md`, approved intermediate path:
`planned/0016_optional_session_companion_avatar.md`, final path:
`completed/0016_optional_session_companion_avatar.md`.
