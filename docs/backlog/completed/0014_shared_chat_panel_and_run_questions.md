# 0014 — Shared chat panel, Docs layout and read-only run questions

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
- **Area:** AbstractTUI, Code TUI, Gateway console
- **Work item:** abstractcode-0014
- **ADR impact:** none; preserve the engine UI-only boundary and Gateway-owned inference.

## Summary and code reality

The operator requests messaging-style bubbles, sensible use of the full drawer height, run questions beside
Docs, a framework-independent customizable engine component, then adoption by the Gateway TUI console.
Code currently concatenates all messages into one MarkdownView; Gateway console has its own role renderer.
Both already use the docs-qa grounded reference workflow. Engine Feed provides measured custom blocks and
virtualized rendering. Gateway console consumes an older engine through its shared Core console dependency.

## Design boundaries

Compare app-only fixes, an engine presentation component and an engine-owned agent/transport. Choose the
presentation component: roles, bubbles, Markdown, scroll and composer are generic; inference, scope selection,
permissions, snapshots and durable conversation identity remain with each host. The engine imports no HTTP,
Gateway, Runtime or provider code. Run questions receive an explicitly labelled snapshot of the loaded
conversation at send time through grounded QA, with no tools or steering authority and separate history.

## Validation

Generic component tests and standalone example without framework packages; short/long/multiline/narrow
message layout, input preservation, disabled/busy handling, scroll and theme. Code tests prove isolation and
exact snapshot request. Inspect real installed terminal captures. Gateway adoption preserves its existing
history, stop and request behavior; test its console against one engine runtime shared with Core console.
No release or push.

## Completion report

Added framework-independent ChatMessage/ChatTranscript/ChatPanel in AbstractTUI using existing measured Feed/Markdown. Code adopts it with separate Docs/This run QA and immutable loaded-context snapshots. Gateway console adopts ChatTranscript while preserving its existing backend, history and Stop. Core console dependency aligned to the same engine runtime.

Generic layout/busy/draft/narrow tests and standalone no-network example; Code snapshot/authority isolation test; installed terminal returned actual Docs and Run answers without coding-draft changes. Gateway console live QA passed. One engine runtime verified with cargo tree. No inference/transport dependencies added to engine. Remaining slash /btw work stays proposed0010.

Validation: AbstractTUI workspace/all-targets 2523 passed, 89 ignored; Code 1159 passed,
23 ignored; Gateway console 1013 passed, 76 ignored; Core console 244 passed, 2 ignored
(including doctests). Engine, Code and Gateway all-target Clippy passed with warnings denied.
Docs: engine API/README/agent skill, Code API/architecture/README, console README and
changelogs updated; LLM aggregates regenerated. Code and Gateway binaries installed locally.
No release, push, Gateway restart, live approval, coding run or user-preference mutation.
Source dependency floors are unreleased and use explicit local Cargo patch arguments.

Evidence: workspace `untracked/chat-20261030/` contains architecture self-review, PTY
captures and live smoke JSON receipts; suite logs are `untracked/chat-*-verified.log`.
Original path: `planned/0014_shared_chat_panel_and_run_questions.md`; final path: `completed/0014_shared_chat_panel_and_run_questions.md`.
ADR impact remains none: the existing UI-only engine and Gateway-owned execution boundaries
are preserved. Tool/session preferences remain client-local as tracked in proposed0004.
