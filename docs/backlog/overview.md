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
| Planned | 2 | 0001 |
| Proposed | 0 | |
| Completed | 0 | |
| Deprecated | 0 | |
| Recurrent | 0 | |

## Next Recommended Work

1. [0001](planned/0001_automations_wui_section_and_tui_commands.md) — Automations in the WUI and TUI. Phase after v1:
| 0002 | [TUI session fold defects](planned/0002_tui_session_fold_defects.md) | planned | child runs counted as turns; id-less runs; string timestamp ordering; first run by page order — align with the web fold |
   starts only after the gateway API (G), ui-kit panel and fixtures (U), and the Observer and Assistant integrations
   have shipped. Root parent: `abstractframework backlog 0928`.

## Planned

| ID | Item | Area | Depends on |
|---|---|---|---|
| 0001 | [Automations: WUI section and TUI `/automations` + `/schedule`](planned/0001_automations_wui_section_and_tui_commands.md) | web, tui | Surfaces built on `wave2/automations` (2026-09-28, unreleased); the session-kind filter/toggle on both session lists remains. |

## Proposed

None.

## Completed

None.

## Deprecated

None.

## Adding an item

Scan every lifecycle folder for the highest `NNNN_` prefix, take the next one, write the item, and update the counts
and tables here in the same commit. On completion, move the file to `completed/` and record the date and outcome here.
