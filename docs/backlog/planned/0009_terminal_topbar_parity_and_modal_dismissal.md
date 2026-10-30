# 0009 — Terminal top-bar parity, grouped capabilities and ordinary modal dismissal

- **Status:** planned
- **Created:** 2026-10-30
- **Area:** tui
- **Work item:** abstractcode-0009
- **ADR impact:** none; use the existing Gateway-owned inference, identity and workflow contracts.

## Summary

Give the terminal the WUI's recognizable top-right controls, one entry for current-session Tools/Skills/MCP,
more spacing between header actions, and outside-click dismissal for ordinary dialogs. Approval and user-question
steps remain protected. Build and install locally; no release.

## Current code reality

Baseline commit 7f22afd is committed and clean. The header and footer both show Skills/MCP counts. Top-right
contains only the Gateway label and light; About and theme exist elsewhere, while a native docs assistant and
account menu are absent. UiCtx::open_modal does not install the engine's existing outside-press hook. Approval
and ask-user prompts share this modal opener. AbstractUIC's AfTopBarActions orders Docs, Appearance, About and
one identity/status menu; its docs assistant uses Gateway docs/corpus?app=code and docs-qa@0.1.1:docsqa001.

## Scope

- One grouped Tools entry opens clear current-session Tools, Skills and MCP choices; remove duplicate counts
  from the footer. Preserve direct slash-command access and honest enabled/default/unknown distinctions.
- Header actions have more breathing room and degrade predictably at narrow widths.
- Docs drawer, theme, About and an account/status menu use labelled terminal-safe controls. Load identity from
  Gateway /me; reuse existing panels and the docs-qa workflow. Docs conversations are separate from coding runs.
- Ordinary modals close on outside press without click-through. Blocking approval/question steps do not close
  or send any decision. Returning focus must preserve the coding composer and active run.
- No new provider calls, client-side goal loop, arbitrary new agent or credential exposure.

## Validation

Headless click and keyboard tests; both outside-click dismissal and protected waits; resize/narrow-width cases;
Gateway request fixtures for identity and docs-qa, including missing corpus, failed run and late response guards;
real installed-terminal smoke with isolated settings. Existing workspace tests and Clippy stay green.

## References

- 0005 (semantic chrome controls)
- tui/src/ui/chrome.rs, ui/mod.rs, ui/modals.rs, ui/rail_view.rs
- AbstractUIC ui-kit/src/af_top_bar_actions.tsx, panel-chat/src/docs_assistant.tsx
