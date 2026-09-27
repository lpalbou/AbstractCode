# 0001 — Automations in AbstractCode: a WUI "Automations" section and TUI `/automations` + `/schedule`

- **Status:** planned (phase after v1: sequenced after the Observer and Assistant integrations)
- **Created:** 2026-09-26
- **Area:** web workspace (`web/src/workspace/`), TUI (`tui/src/commands.rs`, `runner.rs`, `store.rs`, `ui/`)
- **Mission:** C (Code) of the Automations plan
- **Work item:** `abstractcode-0001`; parent `abstractframework backlog 0928`

Design: untracked/design/automations-PLAN.md (2026-09-26)

## Summary
Give AbstractCode an Automations surface on both clients once the gateway Automations API (mission G) and the
ui-kit `AutomationPanel` + client module (mission U) exist, and once the Observer and Assistant integrations have
shipped. The WUI gets an "Automations" sidebar section that lists `AutomationSummary` rows and opens one in ui-kit's
`AutomationPanel`. The TUI stays minimal: `/automations` (list, open, pause, resume, run-now, archive, discuss) and
`/schedule <when> | <prompt>`, which schedules the current workflow with a `schedule@1` trigger. Both clients fold
session lists by the gateway's `session_kind` field and hide automation and occurrence sessions by default behind a
toggle. Today both clients render a legacy scheduled run as a one-turn conversation and never show its results.

## Why
Operator constraints (PLAN §1 and the 2026-09-26 rulings, verbatim):

- "**Visible and manageable.** Users can inspect results and steps, edit definitions, pause/resume, run manually,
  stop current work and archive."
- "**Isolated discussion.** Discuss creates a separate session seeded at a selected occurrence. It cannot write into
  automation context or resources." Amended by ruling 4: "**Discuss is NOT read-only and NOT tool-restricted.** A
  discussion is a new durable runtime session, forked/seeded from the automation's conversation through the chosen
  occurrence, replayable like any session, with the target's normal tools. Isolation means only that it never writes
  back into the automation's session/context (a fork)."
- "**Minimal scope.** Reuse commands, stores, effects, bundles and tool ceilings."
- PLAN §2: "Grouping | Automations view groups everything by `automation_id`, never session prefixes".
- Ruling 6: "App breadth for v1: **Observer and Assistant only.** Code WUI/TUI and the console inventory move to the
  phase after v1."
- Ruling 7: "Automations v1 is the next minor wave; every package writes its PLANNED backlog items first, in its own
  backlog structure."

The operator's original request named the apps that must show automations as Observer, Assistant "and possibly
abstractcode". This item is that "possibly": planned, not v1.

## Scope

### In scope
- **WUI**: an "Automations" section in the workspace sidebar (`code-sidebar`), listed from `GET /automations`, each
  row opening ui-kit's `AutomationPanel` in the main pane through ui-kit's automation client module. The panel's
  callbacks map to the F routes: `onCommand` → `POST /automations/{id}/commands`, `onRevise` → `PATCH
  /automations/{id}`, `onDiscuss` → `POST /automations/{id}/discuss` then open the returned `session_id` as an
  ordinary conversation, `onSeen` → `POST /automations/{id}/seen`, `onOpenRun` → the existing run view,
  `onAnswerWait` → the existing wait-answer path.
- **WUI**: a "New automation" entry from the current conversation (What = the current workflow, When = trigger
  editor from `GET /trigger-sources`, context Independent by default / Growing).
- **TUI** `/automations` with subcommands `list` (default), `open <id|n>`, `pause`, `resume`, `run-now`, `archive`,
  `discuss <index> <prompt>`. `open` shows the summary and the occurrence page (`GET /automations/{id}/occurrences`);
  command subcommands send `POST /automations/{id}/commands` with a client-minted `command_id` and report the
  `CommandReceipt` (acceptance), then the ledger outcome on the next read (application). `discuss` switches to the
  returned discussion session.
- **TUI** `/schedule <when> | <prompt>`: What = the focused conversation's workflow (`target.workflow_id`,
  `bundle_ref`, `flow_id`, `input_data` with the prompt); When = presets mapped to `schedule@1` config
  (`in <N><s|m|h|d>` → `start_at`; `at <HH:MM>` → next local occurrence as UTC `start_at`; `every <N><s|m|h|d>` →
  `every`; optional `until <time>` and `x<count>`); `--growing` selects Growing, default Independent. It POSTs
  `/automations` with a client-minted `request_id` and prints the returned `automation_id` and `next_fire_at`.
- **Both**: session lists fold by the gateway's `session_kind`. `chat` and `discussion` show by default;
  `automation` and `occurrence` are hidden behind a toggle (WUI sidebar switch, TUI sessions-modal key). Grouping
  never parses the `scheduled:` prefix.
- **Both**: a gateway without the Automation API capability (§I, "Apps check Automation API capability") shows an
  explicit "this gateway has no automations API" line; nothing is silently empty.
- Tests below, the vendored fixtures, and the coredoc pass for the changed docs (`docs/web.md`, `docs/api.md`,
  README command table, llms files).

### Out of scope
- Anything in v1 (Observer and Assistant ship first; this item does not start before they do).
- A TUI definition editor, trigger-source editor beyond `schedule@1`, or `stop_current`/`revise` in the TUI (the WUI
  panel exposes every command type; the TUI covers the list in the mission letter).
- `/task`: already taken by the entity task desk (`commands.rs:233`, "leave a task on an entity's desk"); no alias,
  no reuse.
- External/event triggers, rolling summaries, delivery routing (PLAN §5 v2/v3).
- Migrating legacy `POST /runs/schedule` schedules (PLAN §2 "Legacy": unchanged, `legacy:true`, "Recreate as
  automation" is the panel's job).
- Any client-side scheduling, queueing or execution authority: the gateway and runtime own all of it.

## Contract F shapes consumed (verbatim from the PLAN)

```text
AutomationSummary={
 automation_id,title,status,trigger:TriggerBinding,context_mode,next_fire_at?,
 occurrence_count,last_occurrence?:{
  run_id,index,status,fired_at,finished_at?,excerpt,notify},
 attention:{pending_waits:int,unread:bool,cursor:string},
 legacy:bool,revision:int|null,updated_at,capabilities:string[],
 session_kind:"automation"
}
OccurrenceRow={
 run_id,index,fired_at,finished_at?,status,
 trigger:{source_id,summary},user_turn,answer,notify,
 artifacts:[{artifact_id,name,mime_type,url}],
 waits:[{run_id,wait_key,reason,prompt?,choices?}],
 ledger_url,workspace_url?
}
CommandReceipt={command_id,accepted,duplicate,seq}
```

| Route | Request → response |
|---|---|
| `POST /automations` | `{request_id,title,target,trigger,context?,policy?}` → `{automation_id,revision,summary}` |
| `GET /automations` | `status,changed_since,cursor,limit` → `Page<AutomationSummary>` |
| `GET /automations/{id}` | → `{definition,active_revision,summary}` |
| `PATCH /automations/{id}` | `{command_id,expected_revision?,changes:{title?,target?,trigger?,context?}}` → receipt |
| `POST /automations/{id}/commands` | `{command_id,type,payload?}` → receipt |
| `GET /automations/{id}/occurrences` | `cursor,limit` → `Page<OccurrenceRow>` |
| `POST /automations/{id}/discuss` | `{request_id,occurrence_index,prompt}` → `{session_id,run_id,session_kind:"discussion"}` |
| `POST /automations/{id}/seen` | `{attention_cursor}` → `{attention_cursor}` |
| `GET /trigger-sources` | → `{items:[TriggerSource+{available,unavailable_reason?}]}` |

Command types: `automation.revise|automation.pause|automation.resume|automation.run_now|automation.stop_current|automation.archive`.
Status: `active|paused|completed|archived|failed`. Errors `{error:{code,message,field?,command_id?}}` with 404
`automation_not_found|occurrence_not_found`, 409 `revision_conflict|automation_busy|invalid_state|identity_conflict|cursor_expired`,
422 `invalid_definition|unsupported_feature|unknown_trigger_source`. The TUI prints `code` and `message` verbatim.

Contract B ruling 4 applies to `discuss`: the discussion is an ordinary durable session with the target's normal
tools; `DISCUSSION_READERS_V1` is dropped. The discussion's workspace (own, or shared read-write) is an open operator
question; this item renders whatever `workspace_url` the gateway returns and assumes neither.

### Fixtures
Vendor abstractuic's canonical `fixtures/automations/{list,occurrences,trigger-sources,commands,errors}.json`
(PLAN §3 G: "checksum-shared with Qt/Rust") into `web/src/workspace/fixtures/automations/` and
`tui/tests/fixtures/automations/`, each with a recorded SHA-256; both test suites fail when a vendored file's
checksum differs from the recorded one. The TUI already keeps recorded gateway documents under
`tui/tests/fixtures/` (run trees, history bundles) and pins its listing query literally
(`gateway/mod.rs:868-874` doc, test `the_session_listing_query_is_exactly_what_the_gateway_accepts` at :1490); the
automation queries get the same pin.

## Current code reality (verified at abstractcode a636806, 2026-09-26)

TUI:
- `tui/src/config.rs:939` `mint_session_id()` mints `acode-<12 hex>`: a client-local id; nothing ties a session to
  an automation.
- `tui/src/gateway/mod.rs:865` `list_recent_runs` → `:874` `session_listing_path`, query literal at `:886`
  `/runs?limit={}&root_only=true&include_ledger_len=false`. A legacy scheduled parent is a root run, so it shows as a
  one-turn conversation; its result children are not root runs and are filtered out by the gateway. The gateway
  refuses unknown query parameters with a 400 (doc at `:869-873`), so any new `session_kind` filter must use G's
  exact spelling.
- `tui/src/runner.rs:715` `fold_session_rows` folds `/runs` rows by `session_id` and status only; it never reads
  `workflow_id` or any kind field.
- `tui/src/store.rs:212` `SessionRow {id,state,last_at,turns,first_run,prompt}`: no kind, no automation id.
- `tui/src/ui/modals.rs:3060` `open_sessions` (sessions modal, one `Cmd::LoadSessions` at the gesture, never
  polled); rows merged at `:2945` `merge_session_rows`.
- `tui/src/ui/mod.rs:1743` `switch_session`: the path `discuss` and "open occurrence session" reuse.
- `tui/src/commands.rs:4` `enum Command`; `:143` `parse` (match arms through `:251`, `/task` at `:233`); `:260`
  `COMPLETIONS`; `:382` `HELP_LINES`.
- `tui/src/ui/mod.rs:1056` `dispatch_command` (the brief said :1055; the fn line is 1056).
- `tui/src/store.rs:1330` `notify()` pushes in-app notices only; there is no OS/tray notification path, so TUI
  attention is limited to the list's unread/pending-wait markers.
- `tui/src/store.rs:781`: the queue's doc already records that `POST /runs/schedule` is time-based only.

WUI:
- `web/src/workspace/app.tsx:112-123` `route()`/`writeRoute()`: session and run ids live in the URL hash
  (`#session=…&run=…`); an automation selection needs its own hash key.
- `web/src/workspace/use_workspace_catalog.ts:87-95` fetches the same
  `runs?root_only=true&include_ledger_len=false&limit=…` plus `discovery/capabilities` (the capability check source).
- `web/src/workspace/catalog.ts:599` `normalizeSessionSummaries` groups by `session_id` and drops any run with a
  `parent_run_id` (`:609`), so occurrence children never appear; no kind field is read.
- `web/src/workspace/app.tsx:949-1044` `<aside className="code-sidebar" aria-label="Conversations">`, session list
  at `:1006-1030`; `AfTopBarActions` at `:1093`.
- `web/src/workspace/workspace_panels.tsx:15` `InspectorTab = "files" | "activity" | "artifacts"`.
- `web/package.json:45` `@abstractframework/ui-kit ^0.1.12`: no `AutomationPanel` yet; the floor rises to the ui-kit
  minor that ships it (PLAN §I).

Gateway (read only, for the `scheduled:` rule): legacy schedules mint `scheduled:<uuid>` as the wrapper workflow id
and use it as the occurrence `session_prefix` when `share_context` is false (abstractgateway
`routes/gateway.py:8357-8358`). That prefix is exactly what this item must not group by.

## Seams
- **C reads G**: the F routes, error codes, `AutomationSummary.capabilities`, the Automation API capability in
  `/discovery/capabilities`, and the `session_kind` field on `/runs` rows (and its filter spelling, if any). Read in
  the live gateway before writing; no fallback when a field is absent — the contract test fails and a `blocked` ask
  goes to the gateway seat.
- **C reads U**: the automation client module, `AutomationPanelProps`, and the fixtures. The WUI imports them; it
  does not re-implement the panel.
- **C reads R indirectly**: session kinds `chat|discussion|automation|occurrence` come from runtime attribution via
  G; the client never derives a kind from ids or prefixes.

## Tests
- `web/src/workspace/automations.test.ts`: list rendering from `list.json`; open → panel receives
  `occurrences.json`; each command type posts the exact body; error envelopes from `errors.json` render code and
  message; sidebar hides `automation`/`occurrence` sessions by default and shows them with the toggle; a gateway
  without the capability shows the explicit line; fixture checksums.
- `tui/tests/automation_contracts.rs`: request paths and bodies pinned literally (as the listing query is);
  `/automations` and `/schedule` parse cases (including `/task` unchanged); `/schedule` preset → `schedule@1`
  config table; folding by `session_kind` with the hidden-by-default rule; error rendering; fixture checksums.
- Each new check deleted once and seen RED (a check whose absent-input case passes is not a check).
- Commands: `cd web && npm test`, `cargo test --manifest-path tui/Cargo.toml --test automation_contracts`, plus the
  full `cargo test --manifest-path tui/Cargo.toml` and `npm run build`.

## Definition of Done
- [ ] WUI Automations section lists, opens and manages automations through ui-kit's `AutomationPanel`, against a live
      gateway running the H acceptance setup.
- [ ] TUI `/automations list|open|pause|resume|run-now|archive|discuss` and `/schedule <when> | <prompt>` work
      against the same gateway; `/help` and completions list them; `/task` unchanged.
- [ ] Both session lists fold by `session_kind`, hide automation/occurrence sessions by default, and show them with
      the toggle; no code parses `scheduled:`.
- [ ] Discuss opens the returned discussion session as a normal conversation in both clients.
- [ ] Fixtures vendored with checksums; both test files green; each check seen RED once.
- [ ] Capability-absent gateways show the explicit line in both clients.
- [ ] ui-kit floor raised to the release with `AutomationPanel`; coredoc pass on changed docs; changelog entries.

## Dependencies
- G (gateway Automations API, `session_kind` on runs, capability flag) released.
- U (ui-kit `AutomationPanel`, client module, fixtures) released.
- The Observer (O) and Assistant (A) integrations shipped first (ruling 6).

## Related
- `abstractframework backlog 0928` (root Automations item; release sequence).
- PLAN §4 mission C; §3 contracts F and G; rulings 4, 6, 7.

## Contracts pass (2026-09-27)

Final contracts: untracked/design/automations-CONTRACTS.md (root repo; rev 2 with Astra turn-6 amendments 1–11). They supersede the contract text copied above; earlier text is kept as history. Concrete changes for this item:

- Same client rules as the Assistant: no `changed_since` (full paginated polling), attention via `GET …/attention`, `session_kind` filter spelled `session_kind=chat,discussion` on `/runs`, errors from `detail.reason_code`, fixed-interval cadence labels, Discuss = read-only workspace.
- Fixture set adds `attention.json`; vendored copies are byte-identical and belong in the root sync script's groups.
