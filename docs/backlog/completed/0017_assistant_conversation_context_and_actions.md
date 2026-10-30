# 0017 — Side assistant conversation context and compact actions

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
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

## Completion report

Both Docs and This run receive a fresh 24,000-character recent conversation
reference with explicit omission markers, main user/guidance/reply text and
currently streaming visible content. Tool payloads and hidden reasoning are
excluded. Documentation cache remains separate from fresh context. Side histories
bind to the source session. ChatPanel accepts configurable send text/glyph and
host actions on the same row; Code uses labelled send/new/retry icons with
hover/focus tooltips.

Validation: 708 Code library tests, 16 chrome/context tests and 4 engine chat tests
passed; Code all-target Clippy passed with warnings denied. A live installed PTY
opened the operator's existing session read-only and Docs correctly identified
its first AI-news question. Send and new glyphs occupy one row. No coding
requests or user preference writes. Evidence: workspace
`untracked/chat-20261030/context-smoke.json` and context/actions test logs.
Docs/API/skills/LLM aggregates updated. No release or push. Original path
`planned/0017_assistant_conversation_context_and_actions.md`, final path
`completed/0017_assistant_conversation_context_and_actions.md`. Older-history
retrieval remains a separately scoped proposal; the snapshot does not claim
unloaded history. ADR impact none: grounded, tool-free QA and UI-only engine
ownership remain intact.
