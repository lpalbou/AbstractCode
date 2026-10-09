# Changelog

All notable changes to AbstractCode will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [terminal 0.9.2 / web 0.11.2] - 2026-10-09

Requires AbstractGateway with `schedule@2`, the served schedule fields and `POST /api/gateway/automations/schedule-preview` (round 16) and `@abstractframework/ui-kit` 0.8.7.

### Added

- Web: calendar schedules. The schedule dialog's **When** offers **Repeat** (fixed UTC interval, as before), **Daily**, **Weekly** (day chips that show their state), **Monthly** (day 1–31 or the last day) at a time of day, and **Once at**; every schedule is written as `schedule@2`. For every schedule (Repeat with its bounds too) the line under **When** is the gateway's own sentence; for Daily, Weekly, Monthly and Once the time zone reads "in Europe/Paris (your account's time zone)" with **Change in preferences** (opens Settings → Workflow). An automation's Workflow settings edit its calendar rule, keeping its time zone and limits.
- Web: Settings → Workflow → **Time zone**, your account's time zone kept by the gateway (`PUT /api/gateway/accounts/me/preferences` `{time_zone}`): the kit picker over the gateway's IANA list, **Gateway default (<zone>)** first, one PUT at once ("Saved." / "Not saved." + the gateway's sentence).
- tui: the same calendar schedules in `/schedule` and in an automation's Edit panel — see [tui/CHANGELOG.md](tui/CHANGELOG.md).
- tui: the status line shows **Automations · N waiting** while an automation waits for you — N is the Code web's Automations header badge (`attention.pending_waits + attention.unseen_count` summed over `GET /api/gateway/automations`). `Enter` on an empty prompt, or a click on it, opens the automation that waits (a pending approval or question first, else unseen results). It is read at start and when the gateway comes back, then every 15 s only while something waits or an active automation asks before each tool call; otherwise nothing polls.
- tui: Workflow → **Default for new conversations**, your account's default workflow kept by the gateway (`GET`/`PUT /api/gateway/accounts/me/preferences`), the same row as the web: **Gateway default (name)** first, verbatim from the gateway, then the workflows you may run; one PUT per change ("Saved." / "Not saved." + the gateway's sentence; the previous choice stays shown on a refusal). A fresh conversation, `/new` and `abstractcode exec` without `--workflow` start on it; `/workflow` then changes only this conversation. The workflow this computer remembered is uploaded once when the account has none, then removed here. With a gateway older than 0.13.1 the panel says so and `/workflow` keeps saving the choice on this computer, as before.
- tui: `/sessions` **Search conversations** — `/` then typing filters the cards on the title and the id, like the web; `Esc` clears the search before closing.
- tui: `Ctrl+P` in an automation (`/automations <id>`) reads the selected run's reply aloud through the automation's run, like the web; again = stop.
- tui: `/schedule` is the Code web's dialog in seven steps with its words — **What** (a workflow picker on `GET /bundles?executable_for=abstractcode.agent.v1`, **Gateway default** first, and the task), **When** (Repeat, Daily, Weekly, Monthly, Once at… — each worded by the gateway — or **When an email arrives** with the kit's filters, check interval and batch size), **Context** (+ **Max growing context (tokens)**), **Tools** (the `/tools` rows, starting from the conversation's tools), **Workspaces**, **Mailbox** (**Email result**, **Recipients**) and **Title and limits**. The email choices are offered only when `GET /api/gateway/me/email` says the mailbox is usable ("Connect a mailbox first — open My email" otherwise).
- tui: an automation's **Edit** (Workflow panel) changes **Stop after this many runs** / **Stop at (UTC)**, an email automation's check interval, **Email result** and its **Recipients** — one revision each.

### Changed

- tui: **behaviour change** — `/automation` now creates an automation; it used to open the list, which is `/automations` (or `/autos`); `/schedule` is kept as an alias for this release.
- tui: an automation's **Edit** panel offers only its definition's sections (**Sections (1–5)**: Task and schedule, Model and limits, Workspaces, Tools, Skills — named, highlighted, clickable), scrolls with the keys and the wheel so Mailbox and Title and limits are always reachable, and changes **Max growing context (tokens)** (one revision); `o` **Open as chat** (or `Enter` on a run) reads an automation's runs as a read-only conversation in the transcript view. `Esc` in an automation's Edit goes back to the automation (it used to close everything).
- tui: `/automation [task]` opens **New automation** (`/schedule`, its old name, stays a silent alias for this release; `/automation` no longer opens the list — `/automations` does). Its **Task** is a multiline editor (the composer's widget: `Enter`/`Ctrl+J` newline, a visible **Continue** button); its **Tools** step starts with every tool deselected and **Use workflow default tools** off, with **Select all** / **Unselect all**, a state box per toolset (`[x]` all · `[ ]` none · `[~]` some) that toggles its tools, wheel and page-key scrolling, click-to-toggle, and the focus staying on the line you toggle. The web dialog's Tools section is unchanged (it starts from the conversation's tools) — see [tui/CHANGELOG.md](tui/CHANGELOG.md).
- Web: automation rows, the header and Run now's hint show the gateway's served next run (`next_run_local`, cut to "2026-10-09 07:30 Europe/Paris", plus the relative time to `next_run_at`) and every schedule's `schedule_rule_text` (cards read "Every 30 minutes (UTC) · last 3 h ago"); the app no longer reads `next_fire_at` or computes a next run.
- tui: an automation created in the terminal carries the workflow's real inputs, built like the web's: the workflow's input schema defaults, then the conversation's provider/model, reasoning, stream setting, iteration and token limits, tools and skills (never its workspace or approval policy), checked against the schema before the POST (the check's sentence otherwise). Before, only the task was sent.

## [terminal 0.9.1 / web 0.11.1] - 2026-10-08

### Added

- Web: Workflow → **Default for new conversations**, your account's default workflow kept by the gateway (AbstractGateway 0.13.1+, `GET`/`PUT /api/gateway/accounts/me/preferences`), shared with the Assistant, the console's Accounts → Preferences and every browser. **Gateway default (name)** first, verbatim from the gateway, then the workflows you may run; a change is one PUT at once ("Saved." / "Not saved." + the gateway's sentence); a choice that no longer runs shows the gateway's reason. The conversation picker now changes only this conversation. A workflow this browser remembered is uploaded once when the account has none, then removed from this browser's preferences. With a gateway older than 0.13.1 the row is hidden and the picker's choice stays in this browser, as before.
- tui: **Workspaces** — `/workspace` shows this conversation's workspaces with the kit chooser's words (Gateway line, **Use my default**, posture, Read-only / Read & write / Refused per workspace, **Add a workspace path**, the effective line, **My default workspaces**); each change is one `PUT /api/gateway/sessions/{id}/workspaces` (or `/workspace/policy/me`), a refusal reads "<the gateway's sentence> Not saved.".
- tui: `/schedule` has visible **Workspaces** (stored as `input_data.workspace`) and **Title and limits** steps; an automation's Workspace panel uses the same chooser (one revision per change).
- tui: the command sandbox — tool cards show the gateway's state (**Sandboxed to this run's workspaces**), and each command in the transcript, `/details` and Activity shows `Sandbox: <kind> · N workspaces enforced` / `Sandbox: none — refused` from the ledger.

### Changed

- Web: CI: `npm run check:lock` (also a CI step, before `npm ci`) fails when `package-lock.json` lags `package.json` or resolves an `@abstractframework/*` dependency below its floor, in another major.minor or from a local tarball; `--latest` also catches a published patch the lock has not taken. The lock now resolves `@abstractframework/app-server` 0.1.12 (floor `^0.1.12`). See [CONTRIBUTING](CONTRIBUTING.md#lockfile-check).
- tui: interactive runs no longer send `workspace_access_mode` / `workspace_allowed_paths`; the access-mode modal is replaced by the Workspace panel. Headless `exec` is unchanged.

## [terminal 0.9.0 / web 0.11.0] - 2026-10-05

Requires the AbstractGateway round-11 workspace model (`/api/gateway/sessions/{id}/workspaces`, `/api/gateway/workspace/policy/me`, `POST /api/gateway/workspace/effective/me`) and `@abstractframework/ui-kit` 0.8.3.

Requires AbstractGateway 0.13.0 or later for archiving conversations, the
**Archived · N** lists and the gateway's default voice routes. The web client
builds against `@abstractframework/ui-kit` ^0.8.0 and
`@abstractframework/panel-chat` ^0.4.0.

### Added

- web: the command sandbox (AbstractGateway round 12). **Tools**: the process-spawning tools show the gateway's state on their card (`sandboxed` / `sandbox` of `GET /api/gateway/discovery/tools`, verbatim: **Sandboxed to this run's workspaces**, or the refused / unsandboxed state), with the gateway's `command_sandbox.sentence` in the kit tooltip; no state is ever derived in the app. **Activity** (and the transcript's tool cards): each command call shows one line first in its detail — `Sandbox: macOS sandbox-exec · 4 workspaces enforced` or `Sandbox: none — refused` — and the enforced paths, from the ledger's `output.sandbox`. Needs `@abstractframework/ui-kit` 0.8.5 (tool state badge) and `@abstractframework/panel-chat` 0.4.1 (the sandbox line).
- web: the **Docs assistant** (book icon in the top bar) is the kit's shared `DocsAssistantDrawer`, the same chat as the gateway console and the other apps: your question on the right, the answer on the left with Markdown, code, JSON and links, copy, attachments, live streaming, an icon-only New conversation and close. It answers from AbstractCode's llms.txt (served by this app at `/llms.txt`, read by the gateway at `docs/corpus?app=code`) through the gateway's docs-qa workflow, replacing the basic-agent run over bundled guide pages.
- web: **Archive** a conversation from its card's **⋯** or from the **⋯** next to its title in the header, with an inline confirmation ("Archive this conversation? It stays searchable and auditable; it just leaves this list."). It calls `POST /api/gateway/sessions/{id}/archive` (nothing is deleted); the conversation moves under **Archived · N** with **Unarchive**, and archiving the open conversation opens the next one.
- web: a quiet **Archived · N** line at the end of each sidebar list (automations and conversations) shows the archived items inline, each with **Unarchive**; N is the gateway's count (`archived_automations`, `archived_sessions`), the line is absent at 0 and its open state is remembered.
- web: the right panel is a vertical rail at the right edge with **Activity**, **Files**, **Model**, **Workflow**, **Workspace**, **Tools**, **Skills** and **Voice**; a click opens the panel beside the rail (docked and resizable from 1024 px, the width remembered; floating below), the open icon folds it back.
- web: the settings panels follow the selection. A conversation shows its run settings; an automation shows its definition in the same panels, and each change is saved through the gateway as a new revision (`PATCH /automations/{id}` with `expected_revision`), the revision shown on every panel. Everything reads **Gateway default** until overridden. The automation header's **Edit** opens its **Workflow** panel.
- web: **Voice** panel — Engines ("Gateway default · supertonic / supertonic-3", read from `GET /api/gateway/voice/defaults`, with **Change**), output device with **Test**, reply volume, microphone with a live level meter and **Test** (3 s recorded and played back), spoken language, input level, **Read aloud** and voice latency.
- web: the automation page has its own header: title, **Active** switch, timing line, **waiting for you** badge, the workspace as a short name with Open folder / Copy path icons, and **Run now**, **Stop**, **Edit**, **Archive** (inline confirmation).
- tui: **settings panels** — `/settings [panel]` opens Activity, Files, Model, Workflow, Workspace, Tools, Skills and Voice, bound to the conversation or to an automation's definition (each change saved as a new revision, "Revision N"); `/activity` groups the work per model step (per run for an automation).
- tui: **Archive** a conversation from the `/sessions` board or with `/archive` (inline confirmation, `POST /sessions/{id}/archive`); **Archived · N** with **Unarchive** for conversations and automations.
- tui: **voice** — `Ctrl+P` / `/speak` reads the latest reply aloud through the gateway's streaming voice route, sentence by sentence (`Esc` stops); `Ctrl+R` / `/dictate` dictates into the composer with the gateway's default speech-to-text route; **Read aloud** speaks each new reply; `/voice` shows the gateway's default routes, output and input devices with Tests and a level meter, volume, language and latency. Audio plays and records on this computer through AbstractVoice (`--voice-python <PATH>`); see `tui/docs/api.md`.

### Changed

- Automation settings: the gateway's `workspace: {"configured": false}` (an automation that follows your default workspaces at each run) shows as **Use my default** in the Workspace panel; before, it was read as a stored choice.
- **New automation**: the kit dialog (ui-kit 0.8.5) has a visible **Workspaces** section after Tools. It holds the run-level chooser, and its value is stored in the definition as `target.input_data.workspace` (absent = **Use my default**). The dialog has no **Advanced** disclosure: **Title and limits** is a visible section. The automation header shows **Workspaces: <summary>** (the gateway's dry-run summary) and a **Change workspaces** icon that opens the rail's Workspace panel on that automation.
- Fixed: on an automation, choosing **Use my default** in the Workspace panel left the list the gateway had derived from the old choice in the definition. That list narrowed the account default, and the gateway refused the revision. The list now goes with the payload; a list stored without a payload (round 9) stays.
- web: the **Workspace** panel shows THIS conversation's workspaces (the kit `WorkspaceChooser`, session level, the same words as the gateway console, Flow, Observer and the AbstractAssistant): "Gateway: <the admin's eligible workspaces>" on top, **Use my default**, the posture ("Deny everything, allow listed workspaces" / "Allow everything, refuse listed workspaces"), each workspace with Read-only / Read & write / Refused (a mode above the gateway's cap disabled with its tooltip), **Add a workspace path**, and the effective line verbatim. Each change is one `PUT /api/gateway/sessions/{id}/workspaces`: the gateway stores the choice on the session, so every app opening the conversation sees it; **Use my default** sends `{configured: false}`. A refusal shows the gateway's sentence with "Not saved.". **Current workspace session-…** (the private workspace) stays on top.
- web: **My default workspaces** (under the panel) opens your account's default (`PUT /api/gateway/workspace/policy/me`), what every conversation starts from; **Follow the gateway policy** returns to the gateway's.
- web: an automation's **Workspace** panel stores its workspaces in its definition as `input_data.workspace` (`{posture, default_mode, folders}`, a new revision; replaces a stored `workspace_allowed_paths` list when you change it); what they mean is the gateway's dry run (`POST /api/gateway/workspace/effective/me`). **Use my default** removes it.
- web: a gateway refusal answered as `{"detail": {"reason", "message", "path"}}` is shown as its `message` sentence, not as JSON.
- web: the admin-only Files source is called **Gateway files (admin)** (there is no shared workspace any more).
- web: the sidebar's Automations and Conversations sections are two stacking full-width drawers. Both headers stay visible; with Automations open the Conversations header sits mid-height, and each list scrolls inside its own drawer. Fold state is remembered.
- web: conversation cards read the title, then `Oct 2 · 2 turns · 7 tools` (the tool figure only when above zero; the gateway's per-turn total from `GET /runs?include_metrics=true`).
- web: automation cards show the name on its own line, **waiting for you** while an approval is pending, then two quiet lines: `↻ every 24 h · last 3 h ago` and `next in 20 h` with the **Active** switch right-aligned.
- web: **Activity** shows model steps, tool calls and approvals in foldable groups, one per iteration (one per run for an automation), the newest open. A finished timer wait no longer reads "Waiting for you"; a group says "Waiting for you" only when a question or an approval waits on you.
- web: **Files** rows show name, size, generated date and a download icon; a click previews the file in the shared viewer (Markdown, code and JSON, images, PDF, audio in the shared waveform player, text). The workspace path shows once, as a short name with Open folder / Copy path icons.
- web: composer dictation starts on a tap and stops on the next tap (or records while held), uses the microphone chosen in **Voice**, shows "Recording… 3 s" then "Transcribing… 12 s · faster-whisper / large-v3", and explains in a sentence when nothing was heard, the recording was too short or transcription failed (also after 180 s without an answer).
- web: **About** is the shared AbstractFramework About: app name and version, AbstractFramework and AbstractGateway versions (from `GET /api/gateway/about`), links and the licence line.
- web: workspace access modes read in plain words ("This workspace only", "Workspace and allowed paths"); a generated output without a filename reads as its type ("PNG image").
- web: requires `@abstractframework/ui-kit` ^0.8.0 and `@abstractframework/panel-chat` ^0.3.1.
- tui: `/sessions` cards read `Oct 2 · 2 turns · 7 tools`; `/automations` cards read `↻ every 24 h · last 3 h ago` / `next in 20 h` with the **Active** switch, and offer Run now / Stop / Edit / Archive (inline confirmation) like the web client.
- tui: `tui/assets/automation_controls.json` matches the kit's (Unarchive label and hint, result-email wording).

### Removed

- web: the access-mode select, the workspace-root field, the "additional allowed paths" text area and the client-scope notice (the gateway no longer has access modes or client scope overrides). Turns no longer send `workspace_access_mode` or `workspace_allowed_paths`; saving an automation's settings removes a stored `workspace_access_mode`.
- web: the unmounted legacy UI (`src/ui/app.tsx`, never imported by the app) and the old access-mode / allowed-paths / ignored-paths scope fields in `lib/storage.ts`, `lib/gateway_client.ts` and the workflow-input builder.
- web: the horizontal **Workspace & settings** drawer (replaced by the rail), the sidebar's **Show archived** switch (replaced by **Archived · N**), and the header's gear button (the rail icons open the panels).
- web: the composer's speaker button; each reply keeps its own speaker, and **Stop spoken reply** stays while a reply plays.

### Fixed

- web: the Voice panel named the engines "Gateway default · openai" whatever the gateway routed to; it shows the gateway's actual default routes.

## [web 0.10.3] - 2026-10-03

- Conversations and automation answers share narration controls, with an animated loading spinner, incremental audio playback and immediate cancellation. Automation answers use the central panel width.

- User and automation-trigger cards use 75% of the available width, align to the right, and keep normal left-aligned text.

- Web navigation uses one **Workspace & settings** drawer with six shared tabs: Activity, Files, Model & behavior, Tools & skills, Workspace, and Voice. The standard upper-right appearance, About and connection controls remain available, alongside a docs-grounded Code assistant. Workflow inputs, generated outputs, voice controls remain available within those categories. The drawer supports desktop, tablet and phone layouts, touch controls, independent content scrolling and keyboard navigation.

- Automation creation and editing include a workflow picker. The scheduling form uses a compact responsive layout with visible email-recipient controls and persistent action buttons. Automation views identify their own workflow and hide conversation-only controls.

- Automation creation and editing include the shared searchable tool dropdown, with explicit empty selections and workflow defaults. Sidebar refreshes preserve rows and show loading text only before the initial load.

- Browser automation creation preserves the selected model, tool selection and workflow settings. Automation workspaces and approval policy remain separate from the conversation.

## [web 0.10.2] - 2026-10-02

- Configure the growing-context token budget at automation creation and editing (default 50,000); the field is shown only for Growing context.
- Email result delivers every completed result to the selected Recipients without changing email-tool permissions.
- Conversations and automations show accurate loading messages with spinners.

## [web 0.10.1] - 2026-10-02

### Changed

- **Web sidebar: New conversation is a "+" in the Conversations header.** The large "+ New conversation" button
  at the top of the sidebar is removed; the "+" sits beside the Conversations refresh button. ⇧⌘N and the
  "Search conversations" row are unchanged; the Automations header keeps its "+" and refresh.
- **Web run header: no duplicated text.** The "AGENT · running <workflow> (gateway default)" text beside the
  workflow picker is removed (the picker already names the workflow); a workflow that could not be resolved is
  still reported there. The "Gateway default ⌄" model control is now a settings (gear) icon that opens the same
  Run settings; its tooltip names the model.

### Fixed

- **Web: live model progress is shown at phone width.** The run strip's "Prefill · 2,560 / 17,110 tokens (15%)"
  / "Generating · 46 tokens · 38 tok/s" line was hidden on narrow screens with the rest of the strip's detail; a
  live model phase now stays visible there (the static detail is still hidden). Desktop behaviour is unchanged:
  the phase line comes from the gateway's `abstract.progress` records, including those of a subflow's run.
- **Web Activity panel: a subflow finishing no longer reads as the run finishing.** The terminal row of a child
  run is titled "subflow finished"; "run finished" is kept for the conversation's own run.
- Test: the run strip and the Activity llm row are rendered through the real session controller from a fake
  child-run ledger stream (prefill tokens done / total with %, decode tokens and tok/s), red when the phase line
  is removed.

## [web 0.10.0] - 2026-10-01


### Changed

- **Release:** a `web-v*` tag now also gets a GitHub release page carrying its CHANGELOG section
  (job `github-release-web`), like the terminal client's tag and the framework's other packages.
  The page for `web-v0.9.0` was created by hand.
- **Web sidebar: Automations and Conversations headers are panel headers, and no list is cut off.** Each header is
  a full-width 44 px row with the same background as the "New conversation" button: the arrow and the name on
  the left, the section's buttons (+ for automations, refresh) on the right. Click, Enter or Space folds the
  section; the choice is still remembered in this browser. The items sit below on the plain sidebar background.
  Both sections now grow to their full length and the sidebar scrolls them as one list (docked, overlay and phone
  drawer), with the Workspace and Settings rows kept at the bottom; in 0.9.0 the automations list was capped
  and its fourth row was hidden under the Conversations header.
- **Web: the toolbar's workflow picker lists only what this app can run for you.** "Show all workflows" is
  removed. The picker is the kit's `WorkflowPicker` (ui-kit, unreleased): "Gateway default", then **Shared**
  (workflows your admin made available) and **Mine** (your own), each with its version in small text. The list
  is the gateway's answer to `GET /api/gateway/bundles?executable_for=abstractcode.agent.v1`; a gateway that does
  not filter per app is reported next to the picker. An old saved "show all" preference is ignored.

### Fixed

- **Web: a new conversation never carries another conversation's attachments.** Opening a
  conversation restored its last run's workflow fields into the form — including the agent's
  `context`, which held that turn's attachments and media — and the form kept them when
  "New conversation" started a fresh session, so the first turn of the new conversation sent the
  old screenshot along with its question and the model answered about it (2026-10-01). Now a
  conversation switch resets the form to the workflow's defaults, a run's `context`,
  `messages`, `attachments` and `media` are never restored into the form, and a turn's run input
  carries exactly the attachments added to that turn (`restoreWorkflowFields`, `buildWorkflowInput`;
  tests red on removal).

## [terminal 0.8.0 / web 0.9.0] - 2026-10-01

### Changed
- **Web: requires `@abstractframework/ui-kit` 0.4.0.** The switches, the http fallbacks and the phone layout build on
  the kit's `AfSwitch`, `randomId()` and `insecureContextReason()`.
- **Terminal: on/off rows use one marker.** `/tools` and `/skills` show `[x] name` in the accent colour and bold
  when on, `[ ] name` in plain text when off, and `[-] name — reason` dimmed when the row cannot change (a tool
  disabled on this gateway, a skill the gateway blocks). Space switches the selected row; the key hints say
  "space switch". The workspace access mode, a choice of one, shows `(•)` / `( )`.
- **Terminal: automations have an "Active" switch instead of pause/resume.** In `/automations` each row starts
  with its Active marker, and one automation shows `[x] Active — runs on its schedule` or
  `[ ] Active — paused: scheduled runs are skipped (Run now still works)`. Space switches it (`p` still works);
  the switch sends `automation.pause` or `automation.resume`. When it cannot change it says why: legacy
  schedule, archived, ended, a command in flight, or the change not permitted. The confirmation describes the new
  state: "Active is off: scheduled runs are skipped." / "Active is on: it runs on its schedule."
- **TUI assets:** `tui/assets/automation_controls.json` is again byte-identical to the kit's (ui-kit 0.4.0: the
  `active` label and hint, and "Connect a mailbox first — open My email").
- **Web sidebar: the Workspace row, the two lists as panels, 25 conversations per page.**
  - The Workspace row at the bottom of the sidebar keeps the folder name on one line, shortened with "…", and
    never runs under the arrow; the full path shows on hover.
  - Automations and Conversations each sit on their own panel background (light and dark), with the heading
    inside the panel; collapsing a panel and the remembered state work as before.
  - The conversation list shows 25 conversations and "Load more conversations" adds 25 (counted in
    conversations, not turns); it reloads only the list.
- **Web: phones and tablets use the whole screen.** On a phone the automation detail, the approval card, the
  workspace panel and the conversation list now reach the edges of the screen (16 px margins or less) instead of
  sitting in boxes inside boxes:
  - The automation detail is one flat page: each fact sits on one line with its label ("When  every 24 hours"),
    paths and identifiers take a full line as plain text, and the definition, each occurrence, its transcript and
    the folder are separated by thin lines rather than drawn as cards.
  - The approval card is part of the conversation (no frame around it); a long tool argument such as a file path
    wraps instead of scrolling inside its own box.
  - The workspace panel shows the conversation's folder path once, at reading size.
  - The navigation drawer takes the full width on phones and scrolls as one list (Automations, then
    Conversations), rather than one small scrolling area per list. Tablets keep the narrower drawer, also as one
    list.
  - On tablets the automation detail shows two columns only when both are at least about 360 px wide; otherwise it
    uses the phone layout.
- **Web: larger text.** On touch screens (phones and tablets) reading, helper and toolbar text is 14 px and body
  text 15 px; dates, counts and badges stay at 12 px. On desktops nothing is smaller than 12 px, and helper text
  and labels are 13 px. Your font size setting still scales everything.
- **Web: on/off settings are switches labelled by what they control.** Every setting that is either on or
  off uses the kit's switch (ui-kit 0.4.0): highlighted with a check mark and bold label when on, plain when
  off, and dimmed with the reason when it cannot change right now. It applies at once; there is nothing to
  save.
  - Toolbar: "Show all workflows" (shown as "All" on phones). While no gateway is connected or a run is in
    progress, the reason ("Connect to a gateway first." / "A run is in progress.") shows next to the switch on
    touch screens; with a mouse it is the switch's tooltip.
  - Composer queue: "Run queued turns" replaces the "Pause queue" / "Resume queue" button.
  - Automations: each automation's "Active" switch replaces the Pause / Resume buttons (on = runs on its
    schedule, off = paused). Once the automation ended, is archived or is a legacy schedule, the switch shows
    why it cannot change. The sidebar's "Show archived" filter is a switch too.
  - Run settings: each skill is a switch named after the skill. A skill the gateway blocks stays listed, with
    the gateway's reason. While Run settings are locked, the skill switches point at the panel's notice
    instead of repeating it under every row. Tools and "Email me the result" in Schedule a task follow the kit's switches.
  - One-shot actions stay buttons: pausing or resuming the running run, Conclude, Revoke, and the spoken-reply
    playback control.
- **Tests:** `src/ui/state_toggles.test.ts` runs the kit's `findVerbToggleLabels` check over every source
  file, and `src/workspace/state_switches.test.tsx` checks each switch in both states.
  `e2e/state_toggles.shots.mjs` captures the switch surfaces at desktop, tablet and phone widths, in light
  and dark, and records any label above 15 px or heavier than 600.

### Added
- **Web: folding lists in the automation detail.** "Occurrences" and the folder ("Automation folder", or "Run #N
  folder") have a header you can click or tap to fold the list away and give the rest of the detail the room.
  Both start open and your choice is remembered in this browser (`localStorage` key
  `abstractcode.automation.panels`).

### Fixed
- **Web:** the backdrop behind the workspace panel on narrow windows is no longer a second "Close workspace
  inspector" button for screen readers; the panel's own close button is the one control with that name.
- **Web: works over plain http from another machine.** Opening AbstractCode at `http://<host>:8080/apps/code/` from a
  laptop (LAN or Tailscale) stopped at "crypto.randomUUID is not a function": browsers offer that function only on
  https or localhost. Ids come from the kit's `randomId()`. Copy buttons fall back to the browser's copy command
  and say "Copied" or "Copy failed — select and copy". Over plain http the dictation button stays off and says:
  "This page is loaded over http, so voice and camera is unavailable — open it over https (for example through
  tailscale serve; the gateway console's Network page explains how) or on the gateway's own computer." On https or
  localhost in a browser without the microphone API it says "Voice and camera are not supported in this browser
  (getUserMedia unavailable)." The web
  manifest is requested with the app's session cookie
  (`crossorigin="use-credentials"`), so the gateway no longer answers it with 401.

## [web 0.8.0] - 2026-09-30

### Added
- **Web: responsive layout.** AbstractCode Web adapts to phones (portrait and landscape), tablets, laptop
  windows of any width and very wide screens, and re-flows as you resize the window. Windows 1440 px and wider
  keep three docked panes (conversations, conversation, workspace) and the familiar desktop look.
  - Below 1024 px the conversation sidebar and the workspace inspector open as drawers from the header
    (close them with Escape, the backdrop or their close button). From 1024 to 1439 px the sidebar stays docked
    and the workspace inspector opens as an overlay; back at 1440 px and wider it returns docked if you left it
    open. One Escape closes one layer.
  - The toolbar wraps on narrow windows and becomes a single row on phones; icon-only buttons keep their
    names for assistive technologies.
  - Dialogs (sign-in, Schedule a task, appearance, About) become bottom sheets on phones and in phone
    landscape, with their actions always visible.
  - Phone landscape uses one thin row of chrome and a compact composer so the conversation keeps the height.
  - The composer adapts to its width: on phones its icons sit on one row and the destination, Stop and Send on
    the next. While the on-screen keyboard is up, the header, toolbar, run strip and status bar step aside and
    the message field is limited to 30 % of the visible height.
  - Touch devices get 44 px targets and 16 px text fields (iOS does not zoom when you focus a field); keyboard
    shortcut hints are hidden and file actions show without hover.
  - The layout fits the visible viewport and the safe areas of notched phones, and pinch zoom is allowed.
  - The automation detail switches between one and two columns according to the room it has.
- **Web: collapsible sidebar panels.** "Automations" and "Conversations" in the left navigation (docked sidebar,
  tablet overlay and phone drawer) are collapsible. Click a section header to fold or unfold it; its "+" and
  refresh buttons keep their own action. Open panels share the height and scroll independently; a folded panel
  gives its space to the other. Both start open, and your choice is remembered in this browser
  (`localStorage` key `abstractcode.sidebar.panels`).
- **e2e: `e2e/responsive.screens.mjs`**, a screens module that drives sign-in, conversation, approval,
  automations, the Schedule a task dialog, automation detail, workspace, settings, About and the collapsed
  sidebar against the fixture gateway, for screenshot and layout checks at several screen sizes.

### Changed
- **Web: requires `@abstractframework/ui-kit` 0.3.2 and `@abstractframework/panel-chat` 0.2.1**, which provide
  the responsive tokens, sheets, drawers and touch sizes the app builds on.

### Fixed
- **Web:** the sign-in screen makes no voice-catalog request before you are signed in.

## [web 0.7.0] - 2026-09-30

### Added
- **Web: email automations** (framework backlog 0992 WP6). The New automation dialog (the kit's
  `AfScheduleDialog`, `@abstractframework/ui-kit` 0.2.0) offers **When an email arrives** (typed filters:
  from these addresses / domains, sent to these addresses, subject contains, attachments; the check interval,
  hourly by default for a model, never under 60 s, with the rule shown; at most N emails per run), **Email me
  the result** (`notify.channels`) and **May send email without asking to: Only me / Me and these addresses**
  (`policy.email_allowed_recipients`). The app reads `GET /api/gateway/me/email` with every list refresh and
  when the dialog opens; without a usable account the options are off and the dialog says "Email isn't set up
  — open My email", which opens the gateway console's Users tab (`<gateway>/console#users`) in a new tab. The
  automation panel's Edit form and Definition card show the same fields.
- **e2e: `e2e/email_automations.spec.ts`** against the fixture gateway run with the email branches: the
  not-set-up state, and an email-triggered automation created from the dialog whose stored definition carries
  the filters and allowed recipients (a fixture account stored with `test: false`, pointing at a refused
  loopback port; `example.test` addresses only). Its third test (Email me the result) needs AbstractGateway 0.8.0,
  which accepts `notify` in `POST /api/gateway/automations`.

### Changed
- **Web: requires `@abstractframework/ui-kit` 0.2.0 and `@abstractframework/panel-chat` 0.1.21.** The email
  options need AbstractGateway 0.8.0 or later (per-user email).
- **TUI assets:** `tui/assets/automation_controls.json` is again byte-identical to the kit's (it gained the
  `email` wording section; the terminal client does not use it yet).

## [web 0.6.2] - 2026-09-29

### Fixed
- **Web: the approval gate is the same in every client.** A conversation whose run was parked on the
  agent loop's tool approval (the root run waits on `subworkflow:<child>`, the child asks) showed the gate
  only in the client that started the turn; a browser opening the same conversation afterwards showed
  "Running a tool write_file", a Steer composer and no Allow/Deny while the run waited. Requires
  `@abstractframework/panel-chat` 0.1.20, which rebuilds the gate from durable state (the child's waiting
  record and its `GET /runs/{child}` wait) and never lets the root's delegation wait displace it; the
  transcript's tool row for a parked batch now reads "Approval needed" instead of "Running" (the ledger's
  `$slim` pointer on the waiting record).
- **e2e: two devices, one gate.** `e2e/approval_sync.spec.ts` starts the fixture's new "Delegated tool
  approval" flow (root → subflow → write_file) in one browser context and opens the conversation in a second
  one: both show the card, "Approval needed" and a waiting composer; a Deny in the second settles the run in
  the first.

## [web 0.6.1] - 2026-09-28

### Changed
- **Web: requires `@abstractframework/ui-kit` 0.1.16 or later.** The automation panel then shows AbstractUIC's
  shared control hints: every control carries a tooltip and an accessible description, and Run now's says it
  runs once now, that the next scheduled run keeps its time, that it does not count toward a run limit and that
  it works while paused, with the next scheduled time.

## [terminal 0.7.1] - 2026-09-28

### Added
- **Terminal: Run now says what it does.** `/automations` (the list and one automation) shows "g run now:
  Run it once now, without waiting for the schedule; the next scheduled run keeps its time." under the key
  hints, and `/help` says the same. The text is AbstractUIC's shared control hint (vendored as
  `tui/assets/automation_controls.json`, a byte-identical copy of the kit's file), the same as the Observer,
  the Assistant and the browser panel.

### Fixed
- **Terminal: a refused credential is reported as "not signed in".** At launch
  the client asks the gateway first. On a 401 or 403 it exits (code 1) before
  opening the screen and prints the gateway, the HTTP status, whether a token
  was sent, and how to sign in: on the gateway's computer
  `abstractgateway apps tui-command code` (a one-use line that opens the client
  signed in, with no token to handle), or `abstractcode login --token <value>`.
  A gateway that does not answer within 3 seconds does not hold up the launch.
  While the app runs, a refused credential (for example after a gateway restart
  ends a sign-in handed over by the gateway) shows "not signed in" in the
  header, the status strip and the status card; the client stops reloading the
  workflow catalog and the history, and reloads them once the gateway accepts
  it again.
- **Terminal: the login store is private from creation.** `abstractcode login`
  writes `~/.abstractcode/gateway.json` as a new 0600 file in the same folder,
  fsyncs it and renames it over the store: it is never readable by others, and
  a symlink at that path is replaced, never written through.

## [terminal 0.7.0 / web 0.6.0] - 2026-09-28

### Added
- **Web: served by the gateway at `/apps/code/`** (AbstractGateway 0.7.0 or later). The web server follows the
  shared app-server contract (`@abstractframework/app-server`): the identity
  header `X-AbstractFramework-App: code; mount=1`, `<base href>` and the base
  path in the page, every asset and API call relative to it, session cookies
  at `Path=/apps/code/`, and the browser's real address for the gateway. The
  gateway's console **Apps** page opens it with one sign-in. See
  [docs/deployment-web.md](docs/deployment-web.md).
- **Web: launch flags.** `--gateway-url` (aliases `--gateway`, `--url`),
  `--port`, `--host`, `--help`. Without a gateway URL the server follows the
  local gateway pointer `~/.abstractframework/gateway.json`. `PORT`, `HOST`
  and `ABSTRACTCODE_GATEWAY_URL` remain as legacy aliases.
- **Terminal: the local gateway pointer.** Without `--gateway-url`, the
  environment or a saved login, the terminal client connects to the gateway
  named by `~/.abstractframework/gateway.json`, written by the installer and
  by `abstractgateway serve` from AbstractGateway 0.7.0 (a loopback URL,
  schema 1, a regular file owned by you that no other user can write, at
  most 64 KiB, checked on the opened file; anything else is ignored with one
  notice). A saved `http://127.0.0.1:8080` gives way to the pointer.

- **Automations in both clients.** Create, manage and answer gateway
  automations (AbstractGateway 0.6.0 and later) from AbstractCode, with the
  same behaviour as the Assistant and the Observer. See
  [docs/automations.md](docs/automations.md).
  - **Terminal:** `/automations [id]` lists them (state as text + icon —
    "Active ▶", "Paused ⏸" —, what runs now, the next run, attention; archived
    ones hidden until `h`) and opens one: its folder (`w` browses it), the waits
    that need you (`y`/`n` for tool approvals, Enter for questions and events),
    its runs as chat pairs, pause/resume (`p`), run now (`g`), stop current
    (`x`), revise (`e`), archive (`a`, twice) and Discuss (`d`), which switches
    the session to the new discussion. `/schedule [task]` creates one from the
    current workflow in four steps (task, when, context, tools).
  - **Web:** an **Automations** section in the sidebar (state, now, next,
    attention, **Show archived**), **+** to create one with the toolbar's
    workflow (the shared schedule dialog), and a page per automation with the
    shared automation panel and its **Automation folder**. **Discuss** opens the
    fork as a conversation in the app.
  - What runs now comes only from the gateway's `current_occurrence` and the
    next run only from `next_fire_at`; archiving hides and stops an automation
    and keeps its history.

### Changed

- **Terminal: `--gateway-url` is the documented flag** for the gateway address,
  as in every AbstractFramework app; `--gateway` keeps working as an alias.
  `--help` and the docs label the environment variables as legacy aliases of
  `--gateway-url` and `--token`.
- **Terminal: the conversation history comes from the gateway only.** The
  client no longer sends its own copy (`context.messages`), which was capped
  at 40 messages / 24,000 characters and dropped the oldest turns without
  saying so. With AbstractGateway 0.7.0 the gateway replays the newest whole
  turns up to 50,000 tokens and records it in the run (ADR-0026);
  AbstractGateway 0.6.0 replays with its own older limits.
- **Terminal: `abstractcode login` saves a gateway URL only when you give
  one** (`--gateway-url`, its legacy environment alias, or the login already
  saved), so a login no longer stops the client following the local gateway
  pointer to a new port.
- **Web: the conversation history comes from the gateway only.** The legacy
  REPL view no longer sends the local transcript as `context.messages` (it
  sent the last 200 messages when "Use context" was on); it asks the gateway
  to replay the session (`use_session_history`), as the workspace view and
  the terminal client do. The "Use context" setting is removed (the gateway's
  replay is the one history control) and dropped from saved settings. A
  follow-up in a Discuss session no longer gets HTTP 400 there. The
  workspace view's input builder no longer accepts a client history at all.
- **Web: the web server listens on `127.0.0.1` by default** (it was every
  interface). Use `--host 0.0.0.0` to accept other machines directly (the
  server then prints a warning that it is exposed beyond this machine), or let
  the gateway serve it at `/apps/code/`.
- **Web: shared components.** Automation state labels (word + icon), the
  automation folder and the conversation's Files list (the shared workspace
  browser; files open in a new tab as text or download, never as active
  content), tool approvals and questions (the shared interaction cards), and
  automation ledger/artifact links (opened through the app's own proxy) come
  from the shared kit.
- **Web:** requires `@abstractframework/ui-kit` 0.1.14 and
  `@abstractframework/panel-chat` 0.1.19, and adds one runtime dependency,
  `@abstractframework/app-server` 0.1.11.

### Security

- **Web:** with `@abstractframework/app-server` 0.1.11, the sign-in proxy
  accepts a browser-supplied Gateway URL only from a browser on this machine:
  a loopback address **and** a loopback host name, so a page from another site
  whose name resolves to `127.0.0.1` (DNS rebinding) is refused.

## [terminal 0.6.0 / web 0.5.0] - 2026-09-26

### Added

- **Web: Stream replies.** Settings → Model & behavior → **Stream replies**
  (Gateway default / On / Off; default Gateway default) shows the reply as
  the model writes it: a live bubble grows and is replaced by the complete
  message when the model call ends. It sets `_runtime.stream` on the next
  run (nothing for Gateway default, so the gateway's own setting decides).
  Off is always sent, so a gateway default of on never overrides it.
  When the gateway does not advertise live replies, the setting is shown
  disabled with the reason, and a saved On adds one note to the
  conversation. A call the gateway could not stream gets a one-line note.
  A malformed live update is reported once per model call and skipped; the
  run keeps streaming and the complete reply still arrives. Images in
  assistant messages load only from the app's own address (workspace
  files); others are links. Requires `@abstractframework/panel-chat` 0.1.17
  and a gateway that sends live reply events.
- **Web: "Gateway default" workflow.** The toolbar's workflow list starts
  with **Gateway default → name @version**, the coding agent your gateway's
  operator set. Choosing it is remembered as "the gateway default", so a
  change made on the gateway applies to your next new turn; the gateway
  resolves it when the turn starts, and the toolbar then shows what it
  started ("running Coder @0.1.0 (gateway default)"). A fresh browser uses
  it. The list shows coding agents; **Show all workflows** lists the rest.
  Your choice is remembered per account. A conversation whose last run the
  gateway started from its default keeps following it on every turn, on
  any device; one started with a specific workflow keeps that workflow.
  When the gateway's default does not declare the coding-agent interface,
  the toolbar says so.
- **Web: the Files tab shows the conversation's workspace.** Full path with a
  copy button, "on the gateway host <name>" when the browser is elsewhere,
  **Open folder** when the browser is on the gateway's machine, folders with
  sizes and dates, refresh, an explicit note for a partial listing, and a
  preview pane (Markdown, JSON, images, HTML as source, text; download for
  other files). Text previews show at most the first 1 MiB and say so;
  Markdown previews show images only from the conversation's workspace
  (other images become links). Any file can be attached to the next
  message; the gateway's size limit is checked before the file is read.
  Admins keep the operator's shared folder as **Shared workspace (admin)**.
- **Web: About dialog.** The header's About button shows AbstractCode's
  version, the AbstractFramework website, author and links (website,
  source, documentation, report an issue, give feedback), plus the
  AbstractFramework and package versions the connected gateway reports, or
  "Gateway: unavailable (HTTP <status>)" when it cannot. Requires
  `@abstractframework/ui-kit` 0.1.12.
- **Web: an empty Skills tab explains itself.** It shows the gateway's
  reasons in full, the skill shelf location and its source, instead of
  "This gateway has no skills available."
- **Terminal: streamed replies.** `/stream` (Gateway default / On / Off),
  `--stream on|off|default`, and `exec --stream on`, which prints the reply
  live on lines starting `✎` and does not print it again at the end. Off is
  always sent; On is sent only to a gateway that advertises live replies.
- **Terminal: `/files`** browses and previews the run's workspace on the
  gateway host, with its absolute path and host; `c` copies a path, `r`
  refreshes, and `o` shows the workspace folder in your file manager when the
  gateway is on this machine and allows it.
- **Terminal: `/about`** (also `/version`), `/skills` explanations for an
  empty shelf, and MTP as the last step of `/model`.

See [`tui/CHANGELOG.md`](tui/CHANGELOG.md) for the terminal client's full
entries.

### Changed

- Terminal: `/model` asks about MTP only for a model that can use it; otherwise the picker ends after the reasoning step and the transcript says why (`/mtp` still sets it).
- **Web app server** sets `X-Forwarded-For` to the address of the
  connection it received on every call it makes to the gateway (API,
  live streams, status check, sign-in, sign-out), overwriting any incoming
  value and dropping `Forwarded` / `X-Real-IP`; a connection without a
  known address is refused. Each of those requests also carries
  `X-AbstractFramework-App-Proxy: code` (a browser-supplied value is
  dropped).
- **Web: dependency floors** `@abstractframework/ui-kit` ^0.1.12 and
  `@abstractframework/panel-chat` ^0.1.17.
- **Terminal: the default workflow is the gateway's.** `/workflow` starts
  with **Gateway default → name @version** and `--workflow default` selects
  it; the client no longer falls back to a workflow of its own. When the
  gateway has no default and you have picked none, the client asks you to
  pick. `exec` with a saved workflow that is gone exits 2.
- **Terminal: a remote gateway is not sent your local folder** as the
  workspace. `/workspace send auto|always|never` (saved as
  `send_local_workspace`) overrides this, for example for a shared mount.

## [terminal 0.5.1 / web 0.4.2] - 2026-09-23

Terminal client `abstractcode` 0.5.1 (tag `v0.5.1`) and browser client
`@abstractframework/code` 0.4.2 (tag `web-v0.4.2`).

### Added

- **Native-MTP request controls.** Browser Settings and the terminal's `/mtp`
  command (alias `/speculation`) and `--mtp` flag choose inherit, explicit Off,
  or a requested speculative-decoding depth. The pickers are driven by the
  gateway's advertised capabilities, show readiness and unavailable saved
  choices, and never load or download a model. Preferences and run requests
  keep an explicit Off distinct from inherit.
- **Web: drag-and-drop and paste files into the chat composer.** Files dropped
  on the conversation or pasted into the message field (screenshots included)
  attach like picked files. Each file appears as a chip in the composer:
  "Waiting" / "Uploading" (three uploads at a time), then name and size. A file
  over the gateway's `maxAttachmentBytes` is refused on its chip before any
  upload, with both sizes named; a gateway failure shows the gateway's reason
  with Retry. Chips are keyboard-focusable with labelled Remove buttons, and a
  live region announces the attachment count. The composer stays usable while
  files upload; starting a turn while a chip is still uploading, refused or
  failed is refused with the reason, and uploads survive a run starting or
  finishing mid-upload.
- **Web: the Activity tab names each step.** Started / waiting / completed
  records of one step collapse into a single row whose status advances, and
  `abstract.progress` / `abstract.status` records fold into the row of the step
  that emitted them instead of becoming rows of their own. Rows read like
  `llm · mlx · <model>` with tokens in/out, cache state, duration and tool-call
  count; `tools · web_search ×3` with an argument preview; `subflow · agent
  loop` with its task; `wait · delay 3 s`; `ask · your input`; and
  `run finished`. Expanding a row still shows the raw JSON of every record it
  collapsed.
- **Web: the run strip shows the model's phase.** When the provider reports it
  (AbstractCore `abstract.progress` records with `kind: "llm"`), the run strip
  reads `Prefill · 2,100 / 5,642 tokens (37%)` while the prompt is processed
  (or `Prefill · 5,642 tokens` without a measured position) and
  `Generating · 120 tokens · 157 tok/s` once tokens stream. Without phase
  records the strip renders exactly as before.
- **Web: message stat chips open a detail panel.** The tokens / tools / time
  chips under an assistant message open a structured panel on hover or focus:
  prompt-cache reuse per call, context growth, time-to-first-token and
  generation rates, speculative-decoding acceptance, tool time with the slowest
  batch and failure classes, and per-call tables for multi-call turns. Values
  the stack did not report read "not reported".
- Web: reusable AbstractUIC workflow chat for agent and registered Flow
  workflows (questions, event waits, status messages, structured final
  results), gateway skill selection, local next-turn queues with cancellation
  safety, optional gateway-backed dictation and read-aloud, and responsive
  layouts.
- Web: `npm run test:e2e` runs a Playwright end-to-end suite against a
  disposable local gateway fixture (`web/e2e/gateway_fixture.py`); browsers are
  installed separately with `npx playwright install`. `npm test` stays a
  browser-free unit suite.

### Changed

- Web: the browser opens a modular AbstractUIC-themed workspace with shared
  gateway sign-in, durable conversation history, workflow inputs,
  file/artifact inspection, and explicit approval controls. Generic Flow
  inputs are kept separate from agent-only settings.
- Web: Stop shows the ledger-derived stop state where the Stop button was:
  "Stopping…" → "Stopped", or the gateway's own report such as
  "Stop forced at 10 s: inference killed".
- Web: the shared components install from npm as
  `@abstractframework/panel-chat` `^0.1.16` and `@abstractframework/ui-kit`
  `^0.1.10`; building `web/` needs only the npm registry.
- Web: `npm run dev` uses the same gateway session proxy as the packaged
  server (`bin/server.js`), so development and production share one sign-in
  and CSRF flow. The development-only `/api` proxy to port 8081 is removed;
  set `ABSTRACTCODE_GATEWAY_URL` instead.

### Fixed

- Web: with a standing permission ("Permissions: all", or per-tool Allow) a
  covered tool batch no longer flashes "Approval needed" while it runs; the
  browser answers the gateway's parked approval automatically and shows the
  batch as running tools. Batches outside the permission still show the
  approval card, and after Revoke the next batch asks again.

### Security

- Web: browser login and gateway mutations validate request origin and Fetch
  Metadata before forwarding. Login requires JSON, and browser destination
  changes require a loopback peer and hostname unless explicitly enabled with
  `ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1`.

## [terminal 0.5.0 / web 0.4.0, 0.4.1] - 2026-08-31

### Security

- The browser client's bundled HTML sanitizer is updated to DOMPurify 3.4.14,
  which carries fixes for mutation-XSS via re-contextualization and for an
  `IN_PLACE` hook leaving a detached subtree executable. The dependency is no
  longer pinned to an exact version, so it can take future patches, and an
  override collapses the copy nested inside `monaco-editor` onto the same
  patched version.
- Build-toolchain dependencies are updated; `npm audit` reports no advisories
  at any severity. These never shipped to users — the published package
  contains only a bundle and a server on Node builtins — but they run on
  contributor machines and in CI.

### Changed

- The published browser package declares no runtime dependencies. It ships a
  self-contained bundle and a server that uses only Node builtins, so
  installing it no longer downloads React, Monaco and the rest of the build
  toolchain.
- Release binaries ship a `SHA256SUMS` file and build provenance, verifiable
  with `gh attestation verify`.

### Added

- Terminal client written in Rust, published to crates.io as `abstractcode` and
  installed with `cargo install abstractcode`. It lives in `tui/` and renders
  with [AbstractTUI](https://github.com/lpalbou/AbstractTUI): live reasoning
  cycles, tool cards that update in place, durable pause/cancel/resume,
  mid-run steering, file attachments, session history replay, and 26 themes.
- Prebuilt binaries for macOS (Apple silicon and Intel), Linux (x86-64 and
  ARM64), and Windows, attached to each release.

### Changed

- **AbstractCode is now a Rust terminal client plus a browser client.** The two
  ship independently: `v<version>` releases the terminal client, and
  `web-v<version>` releases `@abstractframework/code` to npm.
- The browser client consumes the shared AbstractUIC components as published
  npm packages rather than through path aliases into a sibling checkout, so
  `web/` builds from its own directory with no other repository present.
- Preferences moved to `~/.abstractcode/prefs.json`, beside the existing login
  store. `ABSTRACTCODE_PREFS_FILE` overrides the location.
- The browser client moves to 0.4.0, the first version released under the
  `web-v<version>` tag.

### Removed

- **The Python implementation of AbstractCode.** The terminal client replaces
  it. Version 0.3.8 remains installable from PyPI and tag `v0.3.8` marks its
  source; the implementation stays in this repository's history.
- `abstractcode flow`, `abstractcode gateway`, and `abstractcode workflow`
  subcommands. Workflow selection is now `--workflow <bundle[:flow]>`, and
  installing bundles is a gateway-side operation.

### Fixed

- The browser client's gateway-configuration guard decides local from the
  connection peer instead of the `Host` header, which a remote client could
  set to claim it was local.
- The shared component stylesheet is imported explicitly, restoring styling for
  the agent-cycles panel.

### Migration

- `pip install abstractcode` → `cargo install abstractcode`.
- Preferences from a pre-rename build are read once from
  `~/.abstractcode-tui/prefs.json` and saved forward automatically.
- Credentials in `~/.abstractcode/gateway.json` are unchanged.

## [0.3.9] - 2026-06-03

### Changed
- Raised AbstractFramework dependency floors to Core `>=2.13.32`, Runtime `>=0.4.27`, Agent `>=0.3.11`, and Flow `>=0.3.18`.
- Aligned the web package release version with the Python package.

## [0.3.8] - 2026-05-31

### Changed
- Raised AbstractFramework dependency floors to Core `>=2.13.31`, Runtime `>=0.4.26`, Agent `>=0.3.10`, and Flow `>=0.3.17`.
- Aligned the web package release version with the Python package.

### Fixed
- Hosted web mode now follows the Gateway URL/session policy used by Flow so remote browser clients cannot turn the app into a user-directed Gateway proxy.

## [0.3.7] - 2026-05-29

### Changed
- Raised AbstractFramework dependency floors to the current released Core, Runtime, Agent, and Flow versions.
- Aligned the web package release version with the Python package so one release tag publishes both surfaces.
- Updated the workflow agent contract wording for provider pins to match the provider-text/provider taxonomy.

### Fixed
- Added the dedicated web favicon asset referenced by the browser host.
- Expanded CI/release coverage so Python and web package gates run before publishing.

## [0.3.1] - 2026-02-04

### Added
- **Workflow-driven UI events (network-safe)**:
  - Workflows can emit `Emit Event(name="abstract.message")` to show a message/notification in AbstractCode.
  - Workflows can emit `Emit Event(name="abstract.tool_execution")` and `Emit Event(name="abstract.tool_result")` to render tool-call + tool-result UX blocks (without requiring actual tool execution).
  - `WAIT_EVENT` can carry a `prompt` so workflows can do durable ask+wait under `WaitReason.EVENT` (useful for thin clients); AbstractCode will prompt and resume.
  - `abstract.status` payload supports `duration` (seconds): default `-1` (sticky), `> 0` auto-clears unless superseded.
  - Tool event payloads can be a **single object or a list** (e.g., wire `LLM Call.tool_calls` / `Tool Calls.results` directly into an `Emit Event`).
  - Backward compatibility: `abstractcode.*` remains a deprecated alias accepted by existing hosts.
- **Documentation refresh for public release**: clearer user-facing docs (`docs/getting-started.md`, `docs/architecture.md`, `docs/cli.md`, `docs/api.md`, `docs/faq.md`) plus `SECURITY.md`, `CONTRIBUTING.md`, and `ACKNOWLEDGMENTS.md`.

### Fixed
- Align package version metadata and `abstractcode.__version__`.
- `/help` now shows the correct `/gpu [status|on|off]` usage.

## [0.3.0] - 2026-02-03

### Added
- **Workflow Agent Support** (`abstractcode/workflow_agent.py`): Run VisualFlow workflows as first-class agents via `abstractcode --agent <flow_id|flow_name|/path/to/flow.json>`
  - `abstractcode.agent.v1` interface contract requires host-configurable `provider`/`model`/`tools` start pins (in addition to `prompt`/`response`)
  - Workflows can emit `Emit Event(name="abstract.status")` to update TUI footer status text in real time
  - `On Flow End.meta` (and optional `scratchpad`/`success`) surfaced as assistant-message metadata (`workflow_meta`, `workflow_scratchpad`, `workflow_success`)
  - File-backed persistence support for durable workflow execution
  - Documented in README with usage examples
- **MCP (Model Context Protocol) Integration**: Connect to remote MCP servers for tool execution
  - `/mcp` command to configure and manage MCP server connections
  - `/executor` command to set default tool executor (local vs remote MCP server, session-persistent)
  - Automatic tool synchronization from MCP servers
  - Spinner feedback for remote MCP tool calls
  - Support for stdio-based MCP servers
  - MCP tools integrated into native tool allowlist
- **Enhanced History Commands**:
  - `/history copy` command to copy full conversation history to clipboard
- **Collapsible Thought/Tool Blocks**: Tool-using iterations now render **Thought** and **Tool Call** as **click-to-toggle** blocks (collapsed by default) with high-signal one-line summary always visible
- **Spinner Shimmer**: Status bar spinner text has subtle **reflect/shimmer** highlight traversing the entire text so "still working" is obvious without re-rendering scrollback
- **`/logs provider --no-tool-defs`**: Optionally replace provider request `tools` array (full tool definitions) with array of tool names for compact sharing/debugging
- **Terminal Markdown Module** (`abstractcode/terminal_markdown.py`): Dedicated module for rendering Markdown in terminal with newline unescaping
- **New Test Coverage**:
  - Workflow agent tests (`test_workflow_agent.py`)
  - MCP remote tool execution tests (`test_remote_mcp_tool_execution.py`, `test_remote_mcp_tool_execution_stdio.py`)
  - Repeat guardrail tests (`test_repeat_guardrail_write_file_content.py`)
  - Tool examples toggle tests (`test_tools_examples_toggle.py`)
  - History copy tests (`test_history_copy_full_to_clipboard.py`)
  - Executor command tests (`test_executor_command.py`, `test_executor_real_logic.py`)
  - Spinner shimmer tests (`test_fullscreen_ui_spinner_shimmer.py`)
  - Log provider tests (`test_log_provider_no_tool_defs.py`, `test_log_provider_tool_calls_anthropic.py`)
  - Answer markdown tests (`test_answer_markdown_newline_unescape.py`)

### Changed
- **`/clear`**: Now clears the screen (UI output) in addition to clearing in-memory conversation context
- **`/memorize`**: Renamed from memory-note command to **Memorize** (consistent UX term) to avoid ambiguity with span tagging
- **`/recall`**: Richer filtering and rehydration controls:
  - Added `--tags-mode all|any`, repeatable `--user NAME`, and repeatable `--location LOC`
  - Repeating `--tag k=v` now builds multi-value tags (e.g. `--tag person=alice --tag person=bob`)
  - `--into-context` now also rehydrates matching `memory_note` spans as synthetic system message (`[MEMORY NOTE] ...`)
- **Logging Commands**: Replaced legacy `/context` + `/llm` with `/logs runtime` + `/logs provider` (no backward compatibility)
  - `/logs provider` now reads from durable ledger and includes **all LLM provider calls in current session** (across runs) unless `--run` is used
  - `/logs provider` renders OpenAI/LMS-style "Received request … Generated prediction …" blocks (no truncation)
  - `/logs runtime ... copy` and `/logs provider ... copy` now accept `copy` as trailing token and copy without rendering
- **Verifier (Review) Mode**: Now enabled by default to prevent premature "stops" when model returns incomplete prose without tool calls
  - Added `--no-review` to disable (not recommended)
  - Default `--review-max-rounds` increased to 3
- **Tool Prompt Examples**: Now **off by default** to avoid large token overhead; use `/tools examples on` to enable
- **Output Versioning**: FullScreenUI now uses output versioning and caching for improved render performance
- **Scrolling Behavior**: Enhanced scrolling in FullScreenUI with better page up/down and smooth scroll support

### Fixed
- **Spinner Shimmer Sweep**: Status bar spinner shimmer now traverses **entire** spinner text (previously capped to first ~10 visible characters)
- **Tool Result Visibility**: Increased default tool observation preview to **1000 characters** (was 120) so small-but-critical outputs (e.g. exit codes, working directories) not silently truncated in UI
- **ANSWER Newline Rendering**: Unescape literal `\n` / `\r\n` sequences into real line breaks before terminal Markdown rendering, so multi-line answers display correctly
- **Web Search Reliability**: Added `ddgs>=9.10.0` as dependency so default `web_search` tool works without manual installs
- **Native Tools Prompt Accounting**: ReactShell token estimation now excludes full `Tools (session)` Active Memory catalog for **native-tool models**, matching prompt actually sent to OpenAI-compatible servers (e.g. LMStudio)
- **LLM-Call Payload Observability**: `/logs provider` shows verbatim provider request/response (`_provider_request` + `raw_response`), `/logs runtime` shows durable runtime step trace for LLM/tool calls
- **`/logs provider` Tool-Call Detection**: Best-effort tool-call summary now detects Anthropic `tool_use` blocks in addition to OpenAI-style `tool_calls`
- **Repeat Guardrail**: Reset duplicate-tool-call caches on **new runs** and **/cancel**, block `write_file` calls missing `content` to prevent repeated 0‑byte file writes
- **File Tool CWD Injection**: File tools (read/write/edit) no longer inject `cwd` into UI preview, preventing confusion when relative paths shown
- **Async Run Controls**: Improved async handling for pause/resume/cancel controls
- **Flow CLI Entry Validation**: Added required entry inputs validation in CLI flow commands

### Removed
- **`/new`, `/reset`**: Removed alias commands (identical to `/clear`). Use `/clear`
- **Legacy `/context`, `/llm`**: Removed in favor of `/logs runtime` and `/logs provider`

### Technical Details
- **44 commits**, **30 files changed**: 8,731 insertions, 756 deletions
- New modules: `workflow_agent.py` (721 lines), `terminal_markdown.py` (168 lines)
- 15 new test files covering workflow agents, MCP integration, repeat guardrails, and UI enhancements
- AbstractCore dependency updated to include `[tools]` extras for web search reliability
- Enhanced ReactShell with MCP client management, executor configuration, and improved token estimation

### Migration Notes
- Legacy `/context` and `/llm` commands removed; use `/logs runtime` and `/logs provider` instead
- Tool prompt examples now off by default; enable with `/tools examples on` if needed
- Verifier (review) mode now enabled by default; disable with `--no-review` if unwanted

## [0.2.0] - 2025-12-17

### Initial Release

AbstractCode is an interactive terminal CLI for multi-agent agentic coding, providing a clean and powerful interface for AI-assisted development workflows.

#### Core Features

**Interactive Terminal Interface**
- Full-screen terminal UI built with prompt_toolkit featuring scrollable output, ANSI color support, and mouse interaction
- Clean command-line interface with slash-prefixed commands (`/help`, `/status`, `/task`, etc.)
- Real-time status bar showing provider, model, and context token usage
- Animated spinner with visual feedback during agent reasoning
- Multi-line input support with command history and autocomplete

**Multi-Agent Support**
- React agent with thought-action-observation reasoning loops
- CodeAct agent with Python code execution capabilities
- Configurable iteration limits (default: 25) and context tokens (default: 32768)
- Multiple LLM provider support (Ollama, OpenAI, and more via AbstractCore)
- Dynamic model selection with per-provider configuration

**Built-in Tool Suite**
- `list_files` - Find and list files using glob patterns
- `search_files` - Search file contents with regex patterns
- `read_file` - Read files with optional line range selection
- `write_file` - Write to files with automatic directory creation
- `edit_file` - Edit files using regex or line-based replacements
- `execute_command` - Execute shell commands with security gating
- `web_search` - Search the web via DuckDuckGo (no API key required)
- `fetch_url` - Fetch and process web content

**State Management & Persistence**
- Durable file-backed state with JSON storage (`~/.abstractcode/state.json`)
- Directory-based stores for run, ledger, and snapshot persistence
- Session resumption with conversation history restoration
- Named snapshots for saving and loading specific run states
- Optional in-memory mode for ephemeral sessions

**Security & Safety**
- Interactive tool approval with detailed argument preview
- Per-tool approval flow with yes/no/all/edit/quit options
- Argument editing in JSON format before execution
- Double-confirmation required for shell command execution
- Session-based "approve all" mode with persistence
- Optional auto-approve mode for non-interactive use

**Context & Memory Management**
- Conversation history tracking with `/history` command
- Memory usage breakdown by component (`/memory` command)
- Intelligent conversation compaction with three modes:
  - Light compression (minimal reduction)
  - Standard compression (balanced approach)
  - Heavy compression (aggressive reduction)
- Configurable message preservation for recent context
- Focus-based summarization to maintain topic coherence

**Configuration & Customization**
- Persistent configuration file (`*.config.json`) for saved settings
- Environment variables for default agent type, state file location, and limits
- CLI arguments for provider, model, iterations, tokens, and behavior
- Runtime commands for adjusting max tokens, max messages, and auto-approve
- Color output with `NO_COLOR` environment variable support

**Interactive Commands**

Task Management:
- `/task <description>` - Start a new task
- `/resume` - Resume the last saved or waiting run
- `/clear` (aliases: `/reset`, `/new`) - Clear memory and start fresh

Information & Status:
- `/help` - Display all available commands
- `/tools` - List available tools with descriptions
- `/status` - Show current run ID, workflow, status, and waiting reason
- `/history [N]` - Display recent conversation history
- `/memory` - Show token usage breakdown

Configuration:
- `/auto-accept [on|off]` - Toggle auto-approve for tool execution
- `/max-tokens [N]` - Show or set maximum context tokens (-1 for auto-detection)
- `/max-messages [N]` - Show or set maximum history messages
- `/compact [mode] [--preserve N]` - Compress conversation with configurable preservation

Snapshots:
- `/snapshot save <name>` - Save current run state as named snapshot
- `/snapshot load <name>` - Load a saved snapshot by name
- `/snapshot list` - List all available snapshots

**Keyboard & Mouse Controls**
- Enter - Submit input
- Up/Down arrows - Navigate command history or completion menu
- Page Up/Page Down - Scroll output area
- Home/End - Jump to top or bottom of output
- Ctrl+Up/Ctrl+Down - Smooth scroll output
- Ctrl+L - Clear output area
- Ctrl+C/Ctrl+D - Exit application
- Mouse wheel - Scroll output area
- Mouse click - Position cursor in input

**Technical Architecture**
- Thread-safe multi-threaded design with worker, spinner, and render threads
- Atomic ANSI parsing with cached snapshots to prevent race conditions
- Integration with AbstractCore for LLM capabilities
- Integration with AbstractRuntime for workflow orchestration
- Integration with AbstractAgent for agent implementations
- Efficient lazy imports for fast `--help` response time
- Graceful error handling with state preservation on interruption

#### Dependencies

**Required:**
- `prompt_toolkit>=3.0.0` - Terminal UI framework

**Implicit (from AbstractCore/AbstractRuntime/AbstractAgent):**
- AbstractCore for LLM provider abstraction
- AbstractRuntime for workflow and state management
- AbstractAgent for React and CodeAct agent implementations

#### Installation

```bash
pip install abstractcode
```

#### Quick Start

```bash
# Start with default settings (Ollama + qwen3:1.7b)
abstractcode

# Use a specific provider and model
abstractcode --provider openai --model gpt-4o-mini

# Use CodeAct agent with auto-approve
abstractcode --agent codeact --auto-approve

# Disable state persistence
abstractcode --no-state

# Set custom iteration and token limits
abstractcode --max-iterations 50 --max-tokens 64000
```

#### Example Session

```bash
$ abstractcode
AbstractCode v0.2.0 | Provider: ollama | Model: qwen3:1.7b

> Create a Python script that analyzes a CSV file

🤖 Thinking: I'll create a CSV analysis script using pandas...

🔧 Tool: write_file
   File: analyze_csv.py
   [Approve] (y/n/all/edit/quit): y

✓ File written successfully

🤖 The script has been created. Would you like me to test it?

> yes, test it with a sample CSV

[Agent continues working...]

Commands: /help | /status | /tools | /history | /clear
```

---

## Versioning Notes

- **0.2.0**: Initial public release with full feature set
- **0.1.0**: Internal development version

---

[0.2.0]: https://github.com/lpalbou/abstractcode/releases/tag/v0.2.0
