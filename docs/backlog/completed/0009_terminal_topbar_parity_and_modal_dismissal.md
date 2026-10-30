# 0009 — Terminal top-bar parity, grouped capabilities and ordinary modal dismissal

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
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

## Completion report

Completed 2026-10-30. Implementation commit **e4eedbe**, preceded by the existing clean implementation
checkpoint **7f22afd** and planning commit **0f94505**. Original path:
`docs/backlog/planned/0009_terminal_topbar_parity_and_modal_dismissal.md`; final path:
`docs/backlog/completed/0009_terminal_topbar_parity_and_modal_dismissal.md`.

The header has one Tools menu for Tools/Skills/MCP, spaced actions, Docs/Theme/About and one Gateway
identity/status menu. Narrow layouts use Menu. The footer retains instruments, workspace/session and Help.
Ordinary dialogs close on outside press without click-through; approval/question steps and disconnect
confirmation stay protected. A genuine human wait takes priority over Docs; auto-approved reads do not.
Focused entity chips keep their space when the header becomes narrow.

Docs is a native drawer using the existing authenticated Gateway corpus and docs-qa workflow, in its own
session. Closing it preserves both chats' drafts/state. Worker posts guard session and operation; temporary
polling errors follow the same run with backoff. Missing corpus does not start inference; failed runs surface
errors and an explicit Retry. Account identity uses /me and does not retain credentials from its response.
Disconnect clears the saved login only after confirmation, exits this client and leaves Gateway runs running.

### Validation receipts

- `cargo test --workspace --no-fail-fast`: **1,155 passed, 0 failed, 23 ignored** across 29 targets.
  Full-suite log: local `abstractframework/untracked/code-topbar-tests.log`.
- After the final input layout adjustment: `cargo test --test chrome_width_torture --test headless_ui`:
  **245 passed, 0 failed, 4 ignored**. After hiding a redundant scrollbar and adding the focused placeholder
  assertion: `cargo test --test chrome_width_torture`: **14 passed**.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed after final functional edits;
  `cargo fmt --all -- --check` and `git diff --check`: passed.
- Six `runner_docs_tests` cover the exact shared workflow request, coding-run isolation, missing corpus,
  failed inference, transient polling with one POST, stale-session rejection and identity credential omission.
- Real installed terminal against the local Gateway: Tools/Skills/MCP panels, About, Theme, account identity,
  consumed outside click, and one successful question through docs-qa. **1 Docs inference, 0 coding inferences**.
  The unsent coding draft and docs answer survived closing/reopening the drawer. No live approvals were answered.
- Final optimized local binary: SHA256
  `1dab725d379796d499a109d4a63a3c14eab84d2c63f24c88ebacc8a676a23e15`.
  A second smoke used that binary at 100 columns, opened compact Menu/Docs, and resized to 260 columns;
  **0 inference requests**. Settings were isolated copies. User processes were left running.
- Live receipts, text/SVG/PNG captures and scripts:
  local `abstractframework/untracked/code-topbar-20261030/{live-smoke.json,compact-smoke.json}`.
  Entry, menus, account, Docs waiting/completed/empty and compact layouts were inspected visually.
- README, command/key reference, architecture, Unreleased changelog and both LLM documentation indexes updated.
  Relative links in touched core pages resolve. Agent skill: none added for this interactive UI pass;
  headless CLI recipes are unchanged.

### Limits and follow-up triage

This is initial native parity: Docs displays completed answers, keeps its native chat during the process
lifetime and has no native history selector or attachments. Gateway docs sessions remain durable. These limits
are documented, not hidden behind full-parity claims. The shipped engine drawer spans the viewport height;
its existing inset proposal is AbstractTUI `proposed/field-agora/0905_drawer_vertical_insets_for_docked_chrome.md`.
No engine, Gateway, Runtime, dependency or provider architecture changed, so ADR impact remains none.

0010 preserves /btw design work; 0011 evaluates /goal using the existing discovered workflow interface. Neither
was implemented or promoted by this pass. No release or push. Agora was unreachable (connection refused), so
these local receipts are the closure evidence; no hub claim or receipt is fabricated.
