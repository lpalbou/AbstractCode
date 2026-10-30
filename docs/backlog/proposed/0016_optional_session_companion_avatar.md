# 0016 — Optional session companion avatar and change-driven advice

- **Status:** proposed
- **Created:** 2026-10-30
- **Area:** tui + AbstractTUI presentation
- **Work item:** abstractcode-0016
- **ADR impact:** evaluate if promoted; keep generic presentation in engine and execution in Gateway.

## Operator request and audited code reality

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
