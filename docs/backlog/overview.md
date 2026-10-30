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
| Planned | 7 | 0001, 0002, 0003, 0008, 0024, 0025, 0026 |
| Proposed | 8 | 0004, 0005, 0006, 0010, 0011, 0018, 0019, 0020 |
| Completed | 11 | 0007, 0009, 0012, 0013, 0014, 0015, 0016, 0017, 0021, 0022, 0023 |
| Deprecated | 0 | |
| Recurrent | 0 | |

## Next Recommended Work

- [0025](planned/0025_avatar_shape_overlay.md) and [0026](planned/0026_canonical_final_answer_visibility.md): shape-only avatar compositing and missing visible final-answer investigation (operator screenshots, 2026-10-30).

- [0023](completed/0023_companion_placement_motion_and_separate_documentation.md) completed 2026-10-30: saved top/bottom placement, expressive animation, separate Documentation/Companion entries and current-audio controls; full checks and installed native validation passed.

- [0022](completed/0022_companion_floating_placement_and_distinct_activity.md) completed 2026-10-30: centred floating companion, full-size artwork and distinct listening/waiting/working poses; independently reviewed and installed locally.

- [0021](completed/0021_companion_visual_quality_and_phase_preview.md) completed 2026-10-30: shaded companion graphics and a watch-style phase preview; installed locally with independent visual review.

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

Voice preferences completed: [0015](completed/0015_voice_preferences_and_review.md).

| ID | Item | Area | Depends on |
|---|---|---|---|
| 0001 | [Automations: WUI section and TUI `/automations` + `/schedule`](planned/0001_automations_wui_section_and_tui_commands.md) | web, tui | Surfaces built on `wave2/automations` (2026-09-28, unreleased); the session-kind filter/toggle on both session lists remains. |
| 0002 | [TUI session fold defects](planned/0002_tui_session_fold_defects.md) | tui | child runs counted as turns; id-less runs; string timestamp ordering; first run by page order — align with the web fold |
| 0003 | [Mermaid diagrams in the TUI transcript](planned/0003_mermaid_diagrams_in_the_transcript.md) | tui + abstracttui | an abstracttui release adding a `Feed` fence hook (+ `abstracttui-mermaid` multi-diagram cache); operator to lift the read-only rule on abstracttui |
| 0008 | [Read aloud and export](planned/0008_adopt_panel_chat_read_aloud_export.md) | web | Shared panel-chat actions |

| 0024 | [Compact web settings and shared docs layout](planned/0024_web_settings_simplification_and_shared_docs_layout.md) | web + ui-kit | Existing shared settings and drawer components |
| 0025 | [Shape-only companion overlay](planned/0025_avatar_shape_overlay.md) | tui + engine | Existing Avatar and mosaic rendering |
| 0026 | [Canonical final answer visibility](planned/0026_canonical_final_answer_visibility.md) | tui | Canonical ledger and terminal reconciliation |

## Proposed

| ID | Item | Area | Depends on |
|---|---|---|---|
| 0004 | [A session's tool choices live on the gateway](proposed/0004_session_tool_choices_on_the_gateway.md) | gateway + tui + web | operator decision (deferred 2026-10-10): store-and-serve record vs. also applied at run start; whether a run may override its session; what a new session starts with |
| 0005 | [Rendering and chrome](proposed/0005_tool_result_blocks_prompt_inset_clickable_chrome.md) | tui | JSON remains proposed; input inset completed in0012 |
| 0006 | [Allow-all decision](proposed/0006_allow_all_enabled_tools_kept_pending_operator_decision.md) | web | Operator decision |
| 0010 | [/btw side question](proposed/0010_btw_contextual_side_question.md) | tui + workflows | Context/authority design |
| 0011 | [Evaluate /goal](proposed/0011_evaluate_existing_goal_workflow_surface.md) | tui + workflows | Existing abstractcode.goal.v1 lane |
| 0018 | [Shared visual and voice identity](proposed/0018_shared_agent_visual_and_voice_identity.md) | cross-client | Existing entity settings |
| 0019 | [Bounded side-assistant history access](proposed/0019_bounded_side_assistant_history_access.md) | tui + Gateway | Existing authorized session history |
| 0020 | [Shared comment ownership](proposed/0020_shared_companion_comment_ownership.md) | cross-client | Existing Gateway command/session ownership |

## Completed

| ID | Item | Area |
|---|---|---|
| 0007 | [Shared message footer](completed/0007_kit_message_footer_spec.md) | web |
| 0009 | [Terminal top-bar parity](completed/0009_terminal_topbar_parity_and_modal_dismissal.md) — completed 2026-10-30, e4eedbe; full suite 1,155 passed, local Gateway/terminal checks passed | tui |
| 0012 | [tool modes scrolling and composer inset](completed/0012_tool_modes_scrolling_and_composer_inset.md) — completed 2026-10-30; tested and installed locally, no release | tui + engine/console |
| 0013 | [session list metadata refresh](completed/0013_session_list_metadata_refresh.md) — completed 2026-10-30; tested and installed locally, no release | tui + engine/console |
| 0014 | [shared chat panel and run questions](completed/0014_shared_chat_panel_and_run_questions.md) — completed 2026-10-30; tested and installed locally, no release | tui + engine/console |
| 0015 | [Voice preferences and review](completed/0015_voice_preferences_and_review.md) — completed 2026-10-30; 1162 tests passed and installed terminal settings verified | tui |
| 0017 | [Side assistant context and compact actions](completed/0017_assistant_conversation_context_and_actions.md) — completed 2026-10-30; targeted suites and live installed check passed | tui + engine |
| 0016 | [Session companion](completed/0016_optional_session_companion_avatar.md) — completed 2026-10-30; five workers, full suites, real audio/device and installed UI checks | tui + engine |
| 0021 | [Companion graphics and phase preview](completed/0021_companion_visual_quality_and_phase_preview.md) — completed 2026-10-30; shaded art, explicit phase preview, independent review and local install | tui + engine |
| 0022 | [Floating companion and distinct poses](completed/0022_companion_floating_placement_and_distinct_activity.md) — completed 2026-10-30; full-width transcript, centred art/actions, distinct poses, local install and independent review | tui + engine |
| 0023 | [Companion placement, motion and separate docs](completed/0023_companion_placement_motion_and_separate_documentation.md) — completed 2026-10-30; current-audio controls, full suites and installed native validation | tui + engine |

## Deprecated

None.

## Adding an item

Scan every lifecycle folder for the highest `NNNN_` prefix, take the next one, write the item, and update the counts
and tables here in the same commit. On completion, move the file to `completed/` and record the date and outcome here.
