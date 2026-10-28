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
| Proposed | 3 | 0004, 0005, 0006 |
| Completed | 0 | |
| Deprecated | 0 | |
| Recurrent | 0 | |

## Next Recommended Work

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
   (operator screenshot 2026-10-28); JSON blocks, a "› " prompt inset and clickable chrome, on abstracttui 0186/0187/0188/1260. Root 1055.

## Planned

| ID | Item | Area | Depends on |
|---|---|---|---|
| 0001 | [Automations: WUI section and TUI `/automations` + `/schedule`](planned/0001_automations_wui_section_and_tui_commands.md) | web, tui | Surfaces built on `wave2/automations` (2026-09-28, unreleased); the session-kind filter/toggle on both session lists remains. |
| 0002 | [TUI session fold defects](planned/0002_tui_session_fold_defects.md) | tui | child runs counted as turns; id-less runs; string timestamp ordering; first run by page order — align with the web fold |
| 0003 | [Mermaid diagrams in the TUI transcript](planned/0003_mermaid_diagrams_in_the_transcript.md) | tui + abstracttui | an abstracttui release adding a `Feed` fence hook (+ `abstracttui-mermaid` multi-diagram cache); operator to lift the read-only rule on abstracttui |

## Proposed

| ID | Item | Area | Depends on |
|---|---|---|---|
| 0004 | [A session's tool choices live on the gateway](proposed/0004_session_tool_choices_on_the_gateway.md) | gateway + tui + web | operator decision (deferred 2026-10-10): store-and-serve record vs. also applied at run start; whether a run may override its session; what a new session starts with |

## Completed

None.

## Deprecated

None.

## Adding an item

Scan every lifecycle folder for the highest `NNNN_` prefix, take the next one, write the item, and update the counts
and tables here in the same commit. On completion, move the file to `completed/` and record the date and outcome here.
