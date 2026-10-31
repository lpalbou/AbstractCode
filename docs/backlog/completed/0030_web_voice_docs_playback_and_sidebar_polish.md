# 0030 — Clear web voice settings, shared Docs playback and sidebar width

- Status: completed
- Completed: 2026-10-31
- Created: 2026-10-31
- Area: Code web and AbstractUIC
- Work-item id: abstractcode-0030
- ADR impact: none; retain shared settings, chat speaker and Gateway voice authority.

## Summary and current code reality

Operator requests accurate automation Model headings (streaming is intentionally
hidden for background work), a human workspace label instead of a generated
session directory id, a cleaner Voice panel, read-aloud speakers in every app's
shared Docs assistant whenever speech is configured, and full sidebar width.
Voice is app-local but Code currently binds its header to the selected automation.
The shared Voice rows place long route summaries and repeated help beside controls.
DocsAssistantPanel passes only Copy to ChatThread and has no voice transport.
Sidebar headers/list/card padding cumulatively consume useful title width.

## Scope and validation

Fix Code labels/binding and sidebar gutters, add optional compact shared voice
presentation with delayed information help, and wire speech into the common Docs
assistant using the Gateway run's TTS route and existing playback lifecycle. Retain
history provenance, manual speech/pause/stop behavior, authenticated transport,
voice overrides, disconnected/unsupported behavior and settings persistence.
Test automation versus conversation headings, voice settings/reset, speaker
visibility and real transport, panel width at desktop/mobile and light/dark
rendering. Build and locally deploy affected apps without changing live user
settings or starting production model runs. Public publication remains separate.

## Completion report

Code 32898e4, AbstractUIC 8fd0154, Assistant b088c3b and Gateway a640b659 implement
accurate automation labels, a human private-folder label, app-scoped compact Voice,
full-width sidebar cards and per-reply speech in the shared Docs assistant. The
voice transport retains each answer’s run ownership, uses the authenticated host
fetch and delegates playback to the shared voice hook. App overrides and defaults,
errors, pause/resume and stop-on-close/identity lifecycle are retained.

All 502 Code tests, the full shared suites, 57 Assistant tests, 4 islands tests,
8 browser layout/playback/automation tests and 7 live browser speaker checks pass.
All consuming frontends were built locally; served bundles verified. Note’s
installed static copy was updated; Gateway used its graceful restart control with
zero executing runs. No production question or TTS generation was started.

[Review and evidence](../../../design/web-voice-0030/REVIEW.md). Local packages
remain unreleased; publish shared APIs and raise dependency floors/locks before a
registry release. No further implementation or ADR work is required for this UI task.
