# 0008 — Adopt panel-chat's `readAloudStep` export; delete the inlined copy

- **Status:** planned (blocked on the next `@abstractframework/panel-chat` release; nothing is released tonight)
- **Created:** 2026-10-29
- **Area:** web (`web/src/lib/read_aloud.ts` — the inlined copy; `web/src/workspace/app.tsx`,
  `web/src/workspace/read_aloud_input.ts`, `web/src/lib/read_aloud.test.ts` — the importers)
- **Work item:** `abstractcode-0008`
- **Related:** AbstractUIC backlog 0065 (the rule, panel-chat PR #26) and 0066 (the kit-side tracker of this debt);
  AbstractCode PR #24 (the inlining)

## Summary
AbstractCode PR #24 (merged 2026-10-29) fixed "read aloud speaks a session's existing messages on open / reload /
switch" by applying the kit's rule — AbstractUIC panel-chat `src/read_aloud.ts` (`READ_ALOUD_INITIAL`,
`readAloudStep`, `readAloudPhase`, types `ReadAloudInput`, `ReadAloudPhase`, `ReadAloudReply`, `ReadAloudState`),
exported from `@abstractframework/panel-chat` by AbstractUIC PR #26. That export is NOT in any published panel-chat
(0.6.4 is the newest; this web client pins `^0.6.4`), and no release happens tonight, so PR #24 shipped a VERBATIM
COPY of the kit file as `web/src/lib/read_aloud.ts`.

That copy breaks the operator's standing rule: shared UI behaviour is reused from AbstractUIC, never re-implemented
locally (AbstractUIC ADR 0001; the same rule that put the message footer spec in the kit, backlog 0007). The copy is
tolerated only until the kit ships the export. This item is the dated debt.

## What to do (when panel-chat with `readAloudStep` is published)
1. Bump `web/package.json` `@abstractframework/panel-chat` to the version that carries `read_aloud.ts` (AbstractUIC
   0066 names it), reinstall, relock.
2. `web/src/workspace/app.tsx`, `web/src/workspace/read_aloud_input.ts`, `web/src/lib/read_aloud.test.ts`: import
   `READ_ALOUD_INITIAL`, `readAloudStep`, `readAloudPhase` and the types from `@abstractframework/panel-chat` instead
   of `../lib/read_aloud`.
3. Delete `web/src/lib/read_aloud.ts`. Keep `read_aloud_input.ts` (Code's own adapter: scope = principal + session,
   phase from the run status / `sending`) — it is host wiring, not the rule.
4. `web/src/lib/read_aloud.test.ts` keeps passing unchanged (it exercises the rule through Code's adapter).

## Acceptance
- No file under `web/src` defines `readAloudStep`; `grep -rn "readAloudStep" web/src` shows imports from
  `@abstractframework/panel-chat` only.
- `npm test` (web) green; the four read-aloud cases still pass.
- CHANGELOG entry names the kit version adopted.
