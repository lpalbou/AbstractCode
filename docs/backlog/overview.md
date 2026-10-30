# AbstractCode Backlog Overview

AbstractCode is the framework's coding client: a Rust TUI (`tui/`) and a web workspace (`web/`), both thin clients of
AbstractGateway. This backlog tracks work owned by this repository. Cross-package waves and release sequencing live in
the root backlog (`abstractframework/docs/backlog/`); items here cite their root parent.

Conventions follow the root backlog: one four-digit ID per item (`NNNN_<slug>.md`, never reused, never dated),
lifecycle folders `planned/`, `proposed/`, `completed/`, `deprecated/`, and items shaped like the root's current items
(H1 `# NNNN — Title`, bullet metadata, Summary first). The cross-repo work-item id is `abstractcode-NNNN`.

The backlog was re-seeded on 2026-09-26; the one earlier item (a Python-era proposal) was removed in a955a02.

## Current Counts

| State | Items | IDs |
|---|---|---|
| Planned | 4 | 0001, 0002, 0003, 0008 |
| Proposed | 5 | 0004, 0005, 0006, 0010, 0011 |
| Completed | 5 | 0007, 0009, 0012, 0013, 0014 |
| Deprecated | 0 | |
| Recurrent | 0 | |

## Next Recommended Work



- [0009](completed/0009_terminal_topbar_parity_and_modal_dismissal.md) completed 2026-10-30: top-bar parity, grouped Tools/Skills/MCP and protected outside-click dismissal; installed locally, no release.
- [0010](proposed/0010_btw_contextual_side_question.md): contextual /btw side questions; [0011](proposed/0011_evaluate_existing_goal_workflow_surface.md): evaluate the existing workflow-owned /goal lane.

0. [0006](proposed/0006_allow_all_enabled_tools_kept_pending_operator_decision.md) — OPERATOR DECISION GATE: "Allow all enabled tools"
   (per-browser app layer) stays in Code web until the operator says remove (AbstractUIC 0039). Counts recounted 2026-10-28
   (0005 had been added without a count update).

1. [0001](planned/0001_automations_wui_section_and_tui_commands.md) — Automations in the WUI and TUI. Phase after v1:
   starts only after the gateway API (G), ui-kit panel and fixtures (U), and the Observer and Assistant integrations
   have shipped. Root parent: `abstractframework backlog 0928`.
2. [0003](planned/0003_mermaid_diagrams_in_the_transcript.md) — Mermaid diagrams in the TUI transcript. Deferred by
   the operator (2026-10-10); starts with the ENGINE half in abstracttui (a `Feed` fence hook), then the app wiring.
3. [0004](proposed/0004_session_tool_choices_on_the_gateway.md) — a session's tool choices are still stored per client
   (the TUI's last local session state). Waiting on an operator decision; no gateway change before an explicit go.
4. [0005](proposed/0005_tool_result_blocks_prompt_inset_clickable_chrome.md) — tool-result envelopes print as flat text
   (operator screenshot 2026-10-28); JSON blocks await AbstractTUI 0188; the input inset is implemented in0012. Core chrome controls
   and instrument controls are implemented; the 2026-10-30 local installation is terminal 0.12.0. Root 1055.

## Planned

| ID | Item | Area | Depends on |
|---|---|---|---|
| 0001 | [Automations: WUI section and TUI `/automations` + `/schedule`](planned/0001_automations_wui_section_and_tui_commands.md) | web, tui | Surfaces built on `wave2/automations` (2026-09-28, unreleased); the session-kind filter/toggle on both session lists remains. |
| 0002 | [TUI session fold defects](planned/0002_tui_session_fold_defects.md) | tui | child runs counted as turns; id-less runs; string timestamp ordering; first run by page order — align with the web fold |
| 0003 | [Mermaid diagrams in the TUI transcript](planned/0003_mermaid_diagrams_in_the_transcript.md) | tui + abstracttui | an abstracttui release adding a `Feed` fence hook (+ `abstracttui-mermaid` multi-diagram cache); operator to lift the read-only rule on abstracttui |

| 0008 | [Read aloud and export](planned/0008_adopt_panel_chat_read_aloud_export.md) | web | Shared panel-chat actions |

## Proposed

| ID | Item | Area | Depends on |
|---|---|---|---|
| 0004 | [A session's tool choices live on the gateway](proposed/0004_session_tool_choices_on_the_gateway.md) | gateway + tui + web | operator decision (deferred 2026-10-10): store-and-serve record vs. also applied at run start; whether a run may override its session; what a new session starts with |

| 0005 | [Rendering and chrome](proposed/0005_tool_result_blocks_prompt_inset_clickable_chrome.md) | tui | JSON remains proposed; input inset completed in0012 |
| 0006 | [Allow-all decision](proposed/0006_allow_all_enabled_tools_kept_pending_operator_decision.md) | web | Operator decision |
| 0010 | [/btw side question](proposed/0010_btw_contextual_side_question.md) | tui + workflows | Context/authority design |
| 0011 | [Evaluate /goal](proposed/0011_evaluate_existing_goal_workflow_surface.md) | tui + workflows | Existing abstractcode.goal.v1 lane |

## Completed

| ID | Item | Area |
|---|---|---|
| 0007 | [Shared message footer](completed/0007_kit_message_footer_spec.md) | web |
| 0009 | [Terminal top-bar parity](completed/0009_terminal_topbar_parity_and_modal_dismissal.md) — completed 2026-10-30, e4eedbe; full suite 1,155 passed, local Gateway/terminal checks passed | tui |

| 0012 | [tool modes scrolling and composer inset](completed/0012_tool_modes_scrolling_and_composer_inset.md) — completed 2026-10-30; tested and installed locally, no release | tui + engine/console |

| 0013 | [session list metadata refresh](completed/0013_session_list_metadata_refresh.md) — completed 2026-10-30; tested and installed locally, no release | tui + engine/console |

| 0014 | [shared chat panel and run questions](completed/0014_shared_chat_panel_and_run_questions.md) — completed 2026-10-30; tested and installed locally, no release | tui + engine/console |

## Deprecated

None.

## Adding an item

Scan every lifecycle folder for the highest `NNNN_` prefix, take the next one, write the item, and update the counts
and tables here in the same commit. On completion, move the file to `completed/` and record the date and outcome here.
