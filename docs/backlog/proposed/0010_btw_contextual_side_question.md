# 0010 — /btw <message>: a contextual side question alongside the coding run

- **Status:** proposed
- **Created:** 2026-10-30
- **Area:** tui, gateway workflows
- **Work item:** abstractcode-0010
- **ADR impact:** TBD before implementation if context ownership, tool authority or run isolation changes.

## Summary

Design and implement /btw <message> as a small, contextual aside while the main coding task continues.
The operator prefers the Codex-style interaction over Claude Code's; agree the precise context and persistence
semantics before implementation rather than treating the command name as a complete behavioral contract.

## Current code reality

There is no /btw command. The Assistant drawer now offers read-only questions over a labelled snapshot
of the loaded conversation/run, with separate history and no main-run steering (0014). A command shortcut
and the broader cancel/context/promotion semantics below remain proposed. The active task can be steered or queued; entity conversations have their own focus
and transcripts. Inference and durable histories belong to Gateway/Runtime workflows. /goal already has a
workflow-discovered lane (see 0011), and must not be repurposed as the side-question executor.

## Scope and decision boundaries

- A /btw answer is visibly separate and does not silently steer, interrupt or replace the main run.
- Specify the context snapshot, session/run relationship, history visibility and whether the aside can use tools.
  Prefer read-only context by default; any additional tool authority must be explicit and Gateway-enforced.
- Select an existing workflow or define the smallest reusable workflow contract; do not add a local agent loop.
- Expose progress, cancel/retry and answer recovery; cost and concurrent inference use must remain visible.
- Decide whether replies can be deliberately promoted into the main task; no automatic transcript injection.

## Validation

Main run and approval state remain unchanged by opening/cancelling an aside; fixtures prove which context and
permissions ride the request; interrupted clients recover the answer from Gateway; concurrent answers cannot
cross sessions. Test active, idle, failed, offline and approval-wait cases.

## Non-goals

No implementation in the top-bar/UI pass (0009), and no assumption of exact external-product behavior.
