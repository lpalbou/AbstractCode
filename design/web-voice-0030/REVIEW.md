# Code voice settings and shared Docs speech

Delivered locally 2026-10-31 for Code 0030.

- Background automations show **Limits**. Conversation settings retain **Limits &
  streaming** and the streaming control. Automations have no live conversation view.
- The private workspace is labelled **This automation**; its generated session id
  is no longer exposed as explanatory copy.
- Voice is app-local and no longer shows an automation revision/binding. Shared
  compact controls expand from route headers, place devices/ranges under their
  left-aligned labels, and disclose long browser/option guidance after 200ms.
  Real automation saving/error feedback remains; idle revision prose is now help.
- The shared Docs assistant shows the permanent message speaker whenever a speech
  route or app override is configured. New replies and history preserve their run
  provenance; synthesis and audio download use the host's authenticated transport.
  The shared player handles loading/play/pause/resume, aborted/stale requests,
  output device, volume and mute. Closing the drawer, changing conversation,
  disconnecting or unmounting stops playback. Assistant's kept-alive panel passes
  visibility and its app voice preferences. Existing default chat speakers remain.
- Sidebar headers/cards use the full drawer width, with text padding in one place.

## Source and validation

- AbstractUIC 8fd0154: compact settings, shared Docs voice transport/hook, checks,
  public integration guidance and consumer agent skill.
- Code 32898e4: settings, binding, sidebar and docs navigation; intake c84dd6b.
- Assistant b088c3b: app voice/visibility integration. Gateway a640b659: generated islands.
- Code: 502 tests, TypeScript/Vite build and relative URL check pass.
- Full ui-kit and panel-chat suites pass. Docs transport checks cover configured
  versus absent speech, overrides, encoded run/artifact paths, cancellation signals,
  backend/download errors and speaker rendering for 8 app identities. The Docs
  transport exposes the owning run; existing player/voice and history checks pass.
- 8 browser tests pass: dark/light and 320/390px layouts, actual Web Audio fixture
  playback/pause/resume, single synthesis/artifact reuse, stopping on close,
  full-width sidebar, automation labels/private folder/binding, revision writes and
  desktop/tablet/phone navigation/resize persistence. Runs use a disposable Gateway;
  audio is a local silent WAV fixture. No production model or synthesis run started.
- Assistant: 57 tests and build pass. Gateway islands: 4 checks pass.
- All 7 app frontends build. Live served bundle checks pass for Code, Observer,
  Continuum, Entity, Flow, Note and Gateway; Assistant's built bundle also contains
  the shared feature. Note served an installed npm copy, so its dist was updated
  with its built assets and a rollback retained under /tmp/note-0030-rollback/.
- Seven live browser checks open fixture past Docs replies against the real,
  configured Gateway speech defaults: Gateway, Code, Observer, Continuum, Entity,
  Flow and Note all show the speaker. No browser errors; the only writes are each
  test context's sign-in. Question/run-start requests are blocked in these checks.
  Screenshots use fixture replies rather than operator conversation content.
- Gateway applied its bundle through its own graceful host restart. There were
  zero executing runs before the restart; the new console returned in 22.9 seconds.

Directly inspected dark/light desktop Voice, narrow Voice and the playing Docs
reply. Source-owned screenshots/JSON/text receipts are in this folder. Initial
browser probes were adjusted for the custom select's button role and Voice's
app-level wrapper; the observed behaviors above pass on the final implementation.

## Documentation and release boundary

Browser navigation and Voice scope were aligned in Code's docs and llms corpus.
AbstractUIC's agent skill was created because its public component/hook APIs and
host-owned authenticated transports need multi-step integration guidance. Code's
existing CLI/browser skill was audited; parallel terminal changes were preserved.

This is a local deployment, without registry publication or a push. The new shared
APIs are staged from local tarballs at the existing package versions. Before a
registry release, publish ui-kit/panel-chat and raise consuming floors/locks,
including Code and Assistant, then verify clean registry-only installs. This is
separate release work; the currently served local apps contain the changes.
