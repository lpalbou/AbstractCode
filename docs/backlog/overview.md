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
| Planned | 3 | 0001, 0002, 0003 |
| Proposed | 7 | 0004, 0005, 0010, 0011, 0018, 0019, 0020 |
| Completed | 20 | 0006, 0007, 0008, 0009, 0012, 0013, 0014, 0015, 0016, 0017, 0021, 0022, 0023, 0024, 0025, 0026, 0027, 0028, 0029, 0030 |
| Deprecated | 0 |  |
| Recurrent | 0 |  |

## Next Recommended Work

- [0030](completed/0030_web_voice_docs_playback_and_sidebar_polish.md) completed 2026-10-31: compact app-scoped Voice, accurate automation labels, wider sidebar and shared Docs speaker; locally deployed across the apps. Registry release follow-up remains separate.

- [0029](completed/0029_shared_avatar_catalog_and_rendering.md) completed 2026-10-30: six shared watch-inspired avatars, native/Wasm/React/Qt adapters, red X Error faces and two-second Ready cursor tracking; validated and installed locally. Shared identity migration stays in0018.

- [0028](completed/0028_readable_scrollable_skills_picker.md) completed 2026-10-30: larger Skills picker on the shared List; readable details, mouse wheel/scrollbar and explicit pointer attachment; 1221 tests passed and installed native smoke passed.

- [0024](completed/0024_web_settings_simplification_and_shared_docs_layout.md) and [0027](completed/0027_tui_tool_picker_readability.md) completed 2026-10-30: compact shared web settings and Docs drawer; folded, coloured terminal tool list; both locally deployed. Shared ui-kit registry release remains a release follow-up.

- [0025](completed/0025_avatar_shape_overlay.md) and [0026](completed/0026_canonical_final_answer_visibility.md) completed 2026-10-30: shape-only avatar compositing and canonical final-answer recovery; full checks, real-run replay, adversarial review and native installed validation passed.

- [0023](completed/0023_companion_placement_motion_and_separate_documentation.md) completed 2026-10-30: saved top/bottom placement, expressive animation, separate Documentation/Companion entries and current-audio controls; full checks and installed native validation passed.

- [0022](completed/0022_companion_floating_placement_and_distinct_activity.md) completed 2026-10-30: centred floating companion, full-size artwork and distinct listening/waiting/working poses; independently reviewed and installed locally.

- [0021](completed/0021_companion_visual_quality_and_phase_preview.md) completed 2026-10-30: shaded companion graphics and a watch-style phase preview; installed locally with independent visual review.

- [0009](completed/0009_terminal_topbar_parity_and_modal_dismissal.md) completed 2026-10-30: top-bar parity, grouped Tools/Skills/MCP and protected outside-click dismissal; installed locally, no release.
- [0010](proposed/0010_btw_contextual_side_question.md): contextual /btw side questions; [0011](proposed/0011_evaluate_existing_goal_workflow_surface.md): evaluate the existing workflow-owned /goal lane.

0. [0006](completed/0006_allow_all_enabled_tools_kept_pending_operator_decision.md) completed 2026-11-07 in web 0.15.5: the operator
   decided — the approval card is Deny / Allow / Allow All, for this session only; the per-browser "Allow all enabled tools"
   layer and its Revoke line are removed (AbstractUIC 0039).

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
| 1163 | [The page reconnects its live streams after the app server restarts and re-fetches the view](planned/1163_the_page_reconnects_its_live_streams_after_the_app_server_restarts.md) | web + panel-chat | 2026-11-08: a tool approval raised while the Code app server restarted under the page was shown in the tray, never in Code; reconnect without give-up, re-fetch the view, visible Reconnecting state; deploy-under-a-live-run rule |



## Proposed

| ID | Item | Area | Depends on |
|---|---|---|---|
| 0004 | [A session's tool choices live on the gateway](proposed/0004_session_tool_choices_on_the_gateway.md) | gateway + tui + web | operator decision (deferred 2026-10-10): store-and-serve record vs. also applied at run start; whether a run may override its session; what a new session starts with |
| 0005 | [Rendering and chrome](proposed/0005_tool_result_blocks_prompt_inset_clickable_chrome.md) | tui | JSON remains proposed; input inset completed in0012 |
| 0010 | [/btw side question](proposed/0010_btw_contextual_side_question.md) | tui + workflows | Context/authority design |
| 0011 | [Evaluate /goal](proposed/0011_evaluate_existing_goal_workflow_surface.md) | tui + workflows | Existing abstractcode.goal.v1 lane |
| 0018 | [Shared visual and voice identity](proposed/0018_shared_agent_visual_and_voice_identity.md) | cross-client | Existing entity settings |
| 0019 | [Bounded side-assistant history access](proposed/0019_bounded_side_assistant_history_access.md) | tui + Gateway | Existing authorized session history |
| 0020 | [Shared comment ownership](proposed/0020_shared_companion_comment_ownership.md) | cross-client | Existing Gateway command/session ownership |

## Completed

| ID | Item | Area |
|---|---|---|
| 0006 | [Approval card: Deny / Allow / Allow All for this session](completed/0006_allow_all_enabled_tools_kept_pending_operator_decision.md) — completed 2026-11-07 in web 0.15.5; per-browser allow-all removed | web |
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
| 0024 | [Compact web settings and shared Docs layout](completed/0024_web_settings_simplification_and_shared_docs_layout.md) — 502 web tests, shared suite and browser checks passed; locally deployed, shared package release pending | web + ui-kit |
| 0027 | [Folded, readable terminal tools](completed/0027_tui_tool_picker_readability.md) — 1208 tests passed; native installed binary verified | tui |
| 0025 | [Shape-only companion overlay](completed/0025_avatar_shape_overlay.md) — completed 2026-10-30; shape compositing, restoration, native install and independent review | tui + engine |
| 0026 | [Canonical final answer visibility](completed/0026_canonical_final_answer_visibility.md) — completed 2026-10-30; canonical output recovery, real-run replay and adversarial ordering tests | tui |
| 0028 | [Readable, scrollable Skills picker](completed/0028_readable_scrollable_skills_picker.md) — 1221 tests passed; locally installed and native mouse scrolling verified | tui |
| 0029 | [Shared avatar catalog and rendering](completed/0029_shared_avatar_catalog_and_rendering.md) — six shared models, red X Error and Ready cursor tracking; full suites and native installation verified | tui + ui-kit + engine |
| 0030 | [Web voice and shared Docs playback](completed/0030_web_voice_docs_playback_and_sidebar_polish.md) — 502 Code tests, shared/Assistant/islands checks, 8 browser tests and 7 live app checks pass; locally deployed | web + shared UI |
| 0008 | [Read aloud from panel-chat](completed/0008_adopt_panel_chat_read_aloud_export.md) — completed 2026-11-07 in web 0.15.1; inlined copy deleted, 503 web tests pass | web |

## Deprecated

None.

## Adding an item

Scan every lifecycle folder for the highest `NNNN_` prefix, take the next one, write the item, and update the counts
and tables here in the same commit. On completion, move the file to `completed/` and record the date and outcome here.

