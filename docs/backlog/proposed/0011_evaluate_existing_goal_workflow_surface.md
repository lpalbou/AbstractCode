# 0011 — Evaluate /goal against the existing workflow-owned goal lane

- **Status:** proposed
- **Created:** 2026-10-30
- **Area:** tui, gateway workflows
- **Work item:** abstractcode-0011
- **ADR impact:** TBD only if a new durable goal contract is needed.

## Summary

Decide how a user-facing goal should relate to the framework's workflows before expanding /goal.
This is an evaluation request, not authorization to build a second goal controller.

## Current code reality

/goal, /goal <text> and /goal stop already route through ui/goal.rs. Available workflows are discovered by
abstractcode.goal.v1. The lane starts a Gateway run, keeps explicit tools and cycle limits, and remains
unavailable when Gateway publishes no compatible workflow. The UI must explain this existing capability.

## Scope and decision boundaries

Audit the workflow, discoverability, persisted lifecycle, budgets, stop/resume and verification semantics.
Compare using the existing workflow as-is with a small UX improvement or a revised workflow contract.
Keep progress, inference and verification Gateway-owned; preserve approval policy and one resource pool.
Clarify what separates a goal, an ordinary coding task, an automation and a /btw aside (0010).

## Expected outcome and validation

Record a recommendation with code/workflow evidence and operator decisions. If implementation is warranted,
create a standalone planned item with acceptance tests before changing behavior. Existing /goal tests remain
valid; absent workflow discovery must never launch a guessed or local replacement.
