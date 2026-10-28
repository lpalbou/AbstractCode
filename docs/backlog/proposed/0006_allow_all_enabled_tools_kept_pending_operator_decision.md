# 0006 — "Allow all enabled tools" stays in AbstractCode web until the operator decides (AbstractUIC 0039)

- **Status:** proposed (OPERATOR DECISION GATE: keep until the operator says remove)
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

## Until then

No code change. Do not delete it in a 0039 adoption PR.
