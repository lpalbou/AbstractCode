# 0006 — "Allow all enabled tools" removed: the approval card is Deny / Allow / Allow All for this session (AbstractUIC 0039)

- **Status:** completed 2026-11-07 (operator decided: remove; web 0.15.5)
- **Created:** 2026-10-28
- **Priority:** P2
- **Labels:** decision-gate, approvals, tools, session-tool-policy
- **Links:** AbstractUIC 0039 (`abstractuic/docs/backlog/planned/0039_hosts_move_approve_all_to_session_tool_policy.md`), framework root 1061 (session tool policy), this repo's 0004 (session tool choices on the gateway)

## Summary

OPERATOR DECISION GATE (never assignable). AbstractUIC 0039 asks every panel-chat host to move "allow all /
this kind" to the gateway's session tool policy and delete its local approve-all. AbstractCode web 0.12.0
adopted the session policy (per chat, capability-gated) but still wires the card's **"Allow all enabled
tools"** to a per-browser app tool layer: `web/src/workspace/app.tsx` ~l.486-490 (`onPermissionsAll` ->
`withAllEnabledAuto`, `approveAllScope: "in this browser"`), `web/src/workspace/tool_layers.ts` ~l.90
(`withAllEnabledAuto`), tests in `tool_layers.test.ts` ("Allow all enabled tools" and its Revoke). It makes
every enabled tool Auto for every later chat in this browser, which the per-chat session policy does not
replace. The operator has not decided whether it goes; it stays until they say remove.

## Open decision

- Remove it (Code then offers only the per-chat session policy, as 0039 intends), or keep it as the app-level
  layer (and document it as such in 0039 and in the Code docs).

## Until then (historical)

No code change. Do not delete it in a 0039 adoption PR.

## Decision (2026-11-07, operator, verbatim)

"Normally we said we have Deny, Allow, Allow All (all tools). Do not over complexify that." / "Deny, Allow
(this tool), Allow All (ALL tools). but the scope is ONLY FOR THIS SESSION. it doesn't change the default set
by users in the app settings" / "it NEVER changes the default of abstractcode, it only changes the
permissions for a session".

## Outcome (web 0.15.5, on AbstractUIC 0.10.7 / panel-chat 0.9.0)

- The card offers exactly Deny / Allow / Allow All. `app.tsx` passes `presentInteraction(..., { records,
  currentRun, sessionPolicy: true })`: Allow writes `{approve:{tools:[<this request's tools>]}}` and Allow
  All `{approve:{all:true}}` to the gateway's session tool policy (`POST sessions/{id}/tool-policy`, per
  session id) before the shown wait is answered. No `ask` list is sent: Allow All means every tool the
  gateway authorizes for the person at that moment (the gateway expands `all` when it is written; a tool
  enabled later in the session still asks; approvals never reach automation runs, untrusted-input runs or
  entity visits, as the gateway's `scope` says).
- Removed: `onPermissionsAll` -> `withAllEnabledAuto`, `approveAllScope: "in this browser"`,
  `withAllEnabledAuto` / `allEnabledAuto` / `withoutAutoChoices` (`tool_layers.ts`), the run strip's
  "Auto: every enabled tool · this browser" line and its Revoke (`run_status_bar.tsx`, `workspace.css`), and
  their tests. Nothing on the card writes the app's tool layer.
- Tests: `web/src/workspace/approval_card.test.tsx` (red on web 0.15.4, green now): exactly three buttons,
  no explanatory blocks, Allow / Allow All write only the session policy and then answer, Deny writes no
  policy, `localStorage.setItem` never called.
- Follow-up (not built, not asked): no in-app control shows or resets the session's grants once given;
  a new conversation starts without them.
