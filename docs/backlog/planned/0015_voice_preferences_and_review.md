# 0015 — Voice preferences and review before sending

- **Status:** planned
- **Created:** 2026-10-30
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
