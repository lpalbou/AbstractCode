# 0015 — Voice preferences and review before sending

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
- **Area:** tui voice
- **Work item:** abstractcode-0015
- **ADR impact:** none; audio stays local and inference stays on Gateway.

## Summary and current code reality

The operator requests voice choice, reply read-aloud and a switch between direct
voice messages and editable transcription, comparable to Assistant/iPhone/watch.
Code already has `/voice`, Gateway catalog TTS choice, read-aloud, local device
controls and persisted per-client preferences. Ctrl+R is dictation into the draft;
hold Space is a voice message sent on release. The latter lacks a review option.
Saved TTS choices retain identifiers but not their human catalog labels.

## Scope

Expose an explicit Voice messages setting controlling held-Space delivery:
Send automatically or Review before sending. Preserve existing default (automatic)
and keep Ctrl+R dedicated to dictation into the draft. Cache the chosen voice's
human label in client preferences; keep it out of inference request payloads.
Read aloud remains the existing setting and narration queue. No service, new audio
queue, global Gateway settings or model ownership changes.

## Validation

Exercise actual fake-host recording/transcription: review mode produces editable
draft and no Start/Steer, then Enter sends it; direct mode still sends once. Verify
settings persistence and catalog labels without identifiers leaking into display or
label metadata leaking onto speech request wire. Check `/voice` in installed PTY.

## Completion report

Implemented review-before-send for held-Space voice messages in the existing
voice settings and `/voice auto-send on|off`. The setting is captured at recording
start and controls whether transcription is delivered to the editable composer
or the existing submit path. Review-mode hints say review rather than send.
Ctrl+R stays dictation-to-draft; the existing held-Space automatic default is
preserved. Existing read-aloud and speaker queues are reused without changes.
Catalog selections retain a client-only human voice label, including across
restarts; opaque legacy identifiers display Selected voice. The label and delivery
preference never enter TTS requests.

Validation: Code all-targets 1162 passed, 23 ignored; Clippy with warnings denied
passed. Voice suite: 28 passed, 6 ignored, including actual fake-host record,
upload and transcription in review mode, no Start/Steer/Resume before Enter,
editable correction sent on Enter, and existing direct-send paths. Native
installed PTY tested Voice messages, Read aloud and real Gateway voice catalog
selection using temporary settings: all persisted, no Gateway configuration
writes or inference requests. `untracked/chat-20261030/voice-smoke.json` records
the installed binary hash and checks; `untracked/voice-code-all-tests.log` and
`untracked/voice-code-clippy.log` hold suite evidence. Physical audio playback was
not retested in this settings pass; the existing synthesis/playback path is
unchanged and its fixture suite passes.

Docs: CLI reference, terminal README/changelog, package agent skill and both LLM
aggregates updated. No release/push. ADR impact remains none. Original path:
`planned/0015_voice_preferences_and_review.md`; final:
`completed/0015_voice_preferences_and_review.md`. Related avatar exploration is
proposed0016; no background inference or microphone was enabled.

## Speech interruption and meter controls — 2026-10-31

New submitted messages now stop current/preparing/paused speech and discard the
old narration queue before starting or guiding a run. Queue/goal starts and explicit
steering share the same cancellation path. Typing and navigation do not interrupt.
Late chunks/completions remain fenced by existing speech generations. The bottom
Pause/Resume button now sits directly beside the bounded histogram instead of at
the far edge, retaining stable pointer/focus state while levels update.

All1231 Code tests pass (23 existing ignored), including44 voice tests (6 ignored),
Clippy and formatting pass; captures inspected at40/220 columns and control
geometry verified at40/80/220. Installed locally without restarting user sessions.
[Review and receipt](../../../design/voice-interruption/REVIEW.md).
