# 0030 — Clear web voice settings, shared Docs playback and sidebar width

- Status: planned
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
