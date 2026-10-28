# Completed: the conversation follows the kit's message footer spec (web 0.13.3)

## Metadata
- Created: 2026-10-28
- Status: Completed (web 0.13.3)
- Area: web (`WorkflowChat` from panel-chat)

## Summary
Operator QA 2026-10-28, item 2: "we wanted both the speaker and copy icons permanent; where is the speaker? Show the
date when the message is not from today." The message footer is a shared primitive (AbstractUIC ADR 0001): the spec
is written once in panel-chat 0.6.4 (`message_footer.ts`, AbstractUIC backlog 0064) and this app adopts it by pin
bump, with no local twin of the card.

## What changes here
- `@abstractframework/panel-chat` ^0.6.4 (lockfile refreshed): a message not from today is stamped with its date
  ("Yesterday · 10:52 PM", "Oct 21 · 10:52 PM"), the speaker stays on every assistant message while the voice is
  available (disabled with its reason while a reply streams), and the files-changed chip opens the structured
  detail panel.

## Evidence
- The kit check `panel-chat/scripts/check_message_footer.mjs` (56 checks, mutation-checked) pins the behaviour.
- This app: tests and build green on the bumped pin (see the PR).
- Screenshots (AbstractCode web on a scratch gateway, browser clock +7 days, before "12:31 PM", after
  "Oct 28 · 12:31 PM"): `untracked/release-closing/design/footer/code-footer-*`, `code-tokens-tooltip-*` (workspace).
