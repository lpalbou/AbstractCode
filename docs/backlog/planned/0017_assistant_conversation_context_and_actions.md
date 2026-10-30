# 0017 — Side assistant conversation context and compact actions

- **Status:** planned
- **Created:** 2026-10-30
- **Area:** Code TUI + AbstractTUI ChatPanel
- **Work item:** abstractcode-0017
- **ADR impact:** none; reuse Gateway grounded QA, keep engine presentation-only.

## Summary and current code reality

The operator's screenshot is in Docs scope. Docs receives documentation only;
This run receives a detailed export including tool arguments/results. Neither is
the requested default context: both should know the main session's visible user
requests and assistant replies, without tool payloads or hidden reasoning.
ChatPanel owns a Send row while the host adds a second New conversation row.

## Scope and validation

Capture bounded visible conversation text for both scopes at question send time,
including currently streamed assistant content. Docs additionally uses its cached
documentation, kept distinct from the fresh conversation reference. Label partial
history explicitly and bind side histories to source session. No tools, steering,
approval, workspace reads or full-ledger dump. Add one shared action row with
customizable labelled icon controls and hover/focus explanations. Test fresh
context, no tool/reasoning payloads, session isolation and same-row action geometry.
Exercise installed UI with a known main conversation. History paging/tool design
is evaluated separately rather than granting broad tools to grounded QA.
