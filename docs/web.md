# AbstractCode Web

AbstractCode Web is a gateway-authenticated workspace for coding agents and registered workflows. The browser observes durable runs; it does not execute models, workflow nodes, or tools locally.

Start with [getting started](getting-started.md). For hosting and authentication, see [web deployment](deployment-web.md).

## Working in the interface

- **Conversations** restores gateway sessions and history. Search by conversation text or session ID. The list shows 25 conversations; **Load more conversations** adds the next 25.
- **The right panel** is a vertical rail at the right edge of the window with one icon per subject — **Activity**, **Files**, **Model**, **Workflow**, **Workspace**, **Tools**, **Skills** and **Voice**. Clicking an icon opens its panel beside the rail; clicking it again (or the panel's collapse arrow) folds the panel back to the icons. The rail always stays. The gear button in the header (tooltip **Settings**) opens the **Model** panel, or folds any open settings panel.

| Panel | What it shows |
|---|---|
| Activity | The run as the transcript shows it — model steps, tool calls, approvals — in foldable groups, one per agent iteration, the newest open. For an automation: one group per run (newest first, the latest open); opening a group reads that run's steps from the gateway. |
| Files | The workspace folder: name, size, generated date (relative; the exact time on hover) and a download icon per file. Click a file to preview it. Generated outputs and attachments are listed below, with the same rows. |
| Model | The shared model picker (route, reasoning, MTP depth), then **Behavior**: iteration limit, context token limit, additional instructions, **Stream replies**. |
| Workflow | Which workflow runs (**Gateway default** until you pick one), then that workflow's **Inputs** and **Run workflow** / **Back to chat**. For an automation: its definition — workflow, title, task, schedule, context, tool approval, email result (one **Save**). |
| Workspace | The current workspace (short name; the full path as tooltip), access mode and mounts. |
| Tools | Permissions and the shared tool policy (which tools, and when each asks you). |
| Skills | The gateway's skills, one switch each. |
| Voice | Engines, output device, read aloud and voice latency. |

The six settings panels are bound to what is selected in the sidebar. Each starts with a line saying what it edits — **Conversation** and its title, or **Automation**, its title and its revision. Everything reads **Gateway default** until you override it; an override shows only while it is set, and choosing the default again removes it.

With an **automation** selected, Model, Workflow, Workspace, Tools and Skills edit its saved definition in place. Typed fields save when you pause, pickers at once, the Workflow form with its **Save**; every change is saved through the gateway as a new revision, the revision shown on every panel updates, and changes apply from the next run. The automation header's **Edit** opens its **Workflow** panel. If someone else changed the automation meanwhile, the save is refused and the latest revision is shown.

The **Workflow** picker lists workflows authorized by the gateway for `abstractcode.agent.v1`. **Gateway default** follows the default the operator configured; selecting a named workflow pins the conversation to that workflow. The choice is remembered for your account. Unset settings preserve workflow defaults. When skills are unavailable, **Skills** shows the gateway's explanation. See [workflows](workflows.md#the-gateway-default).

**Files** shows the folder on the gateway where the agent reads and writes, once, as its short name — the full path is its tooltip — with **Open folder** (only on the gateway's own machine) and **Copy path** icons. Click a file to preview it with the shared viewer: Markdown rendered, code and JSON in the shared code viewer (highlighted; JSON pretty-printed), images and PDFs shown, audio in the shared waveform player (play, pause, seek), other text as text; HTML and SVG show as source. Text previews read at most the first 1 MiB and say so; Markdown images load only from the workspace. The preview header has the file's size and date, **Attach** (adds it to your next message), **Download** and close. Gateway admins also have **Shared workspace (admin)**.

**Voice** follows the Assistant's layout: **Engines** (*Text → speech* and *Speech → text*, each "Gateway default" with **Change**), **Output** (*Output device*, where the browser can choose a speaker), **Replies** (*Read aloud* switch: speak each new reply; *Voice latency*: Balanced, Faster or Higher quality, when the gateway's voice engine offers it). These are this browser's choices for your account; the gateway's defaults are never changed from here.

The panel width is resizable on wide screens (drag the panel's left edge, or focus it and use the arrow keys) and remembered with the open panel. The upper-right **About** widget is the shared AbstractFramework About: the app name and version, the AbstractFramework and AbstractGateway versions the connected gateway reports (or why one is missing), links (website, source, docs, issues, feedback, contact) and the author/licence line — no package list. See [Responsive layout](#responsive-layout).

The standard upper-right controls remain available: **Code assistant**, **Appearance**, **About**, **Settings** (gear), and gateway connection. The assistant answers questions using the bundled app documentation, in its own gateway session with no tools; it does not inherit the conversation’s settings or attachments. Closing its drawer preserves its conversation.

MTP depth is independent of reasoning. Leave it on **Inherit** to follow workflow and
execution-host defaults, choose **Off** to send an explicit `false`, or request an advertised
depth. Choices come from provider/model execution discovery; missing support is unknown,
and saved unavailable choices stay visible. An explicit depth sets `require_acceleration`
so the host must honor it or report an error. The browser saves this preference locally and
sends it as `_runtime.speculation`; it does not load models, download heads, or change the
shared Core default. Fresh Core configurations use depth 2 only for compatible models.

## Responsive layout

AbstractCode Web works on phones, tablets, laptop windows and wide screens, and re-flows as you resize the window.

| Width | Layout |
|---|---|
| 1024 px and wider | Conversations and automations stay in a docked left sidebar. The right rail's panel opens beside the conversation (the conversation narrows); its width is resizable and remembered. |
| 768–1023 px | Conversation navigation opens from the menu button. The rail stays at the right edge; its panel floats over the conversation with a dimmed backdrop. |
| Below 768 px | Same as tablets; the floating panel takes the width left of the rail. |

- **Touch controls.** The rail icons, the gear and every panel button are at least 44 px; the resize edge widens for touch.
- **Keyboard.** Up/Down, Home and End move between rail icons; Enter or Space opens one. On a floating panel, Escape folds it and returns focus to its icon. A dialog or dropdown above it handles Escape first. The resize edge takes Left/Right (Shift for bigger steps), Home and End.
- **On-screen keyboard.** The panel content scrolls within the remaining height.
- **Appearance.** The upper-right appearance widget contains theme and text-size controls. Rows and paths wrap; nothing scrolls sideways.
- **Conversation navigation.** The sidebar menu closes when you open a rail panel, keeping one navigation surface in front of the conversation.

### Navigation panels

The sidebar holds two stacking drawers, **Automations** above **Conversations**, both full width. Each header has a fold control, **+** and refresh; the Conversations **+** starts a new conversation (also ⇧⌘N). Both headers always stay visible:

| Automations | Conversations | Layout |
|---|---|---|
| closed | closed | The two header rows at the top. |
| open | closed | Automations fills the space above the Conversations header, which sits mid-height. |
| closed | open | Conversations fills everything below its header. |
| open | open | An even split; the Conversations header stays mid-height. |

Each list scrolls inside its own drawer. Fold choices are remembered in this browser (`abstractcode.sidebar.panels`). On a short landscape phone screen the whole drawer scrolls instead. The current workspace is shown in the **Workspace** panel and in **Files**.

At the end of each list a quiet line **Archived · N** appears when the gateway reports archived items (N is the gateway's count: `archived_automations` of `GET /automations`, `archived_sessions` of `GET /runs?root_only=true`; no line at 0). Click it to show the archived automations or conversations inline, each with **Unarchive** (an automation comes back paused, with its history; a conversation comes back to the list). Clicking an archived item's name opens it. Whether each line is open is remembered in this browser (`abstractcode.sidebar.archived`). Archived items never appear among the live rows.

A conversation card shows its title on one line, then `Oct 2 · 2 turns · 7 tools`: the date of the latest turn, the number of turns and, when there were any, the tool calls across them. The tool figure comes from the gateway (`GET /api/gateway/runs?include_metrics=true`, each turn's total including its sub-runs); with a gateway that does not report it, the card shows no tool figure.

An automation card shows its name on its own line, **waiting for you** while an approval or question is pending, then two quiet lines: `↻ every 24 h · last 3 h ago`, and `next in 20 h` with the **Active** switch right-aligned on that line (relative times rounded down, no year or seconds; `last never` before the first run, `running now` while a run executes, `waiting since 5 min` while a run waits for you; the second line holds only the switch when nothing is scheduled).

### Automation detail sections

In an automation's page, **Occurrences** and the folder ("Automation folder", or "Run #N folder") have a header you can click or tap to fold the list away. Both start open, and your choice is remembered in this browser (`localStorage`, key `abstractcode.automation.panels`).

### Switches

Every setting that is either on or off is a switch named after what it controls: **Run queued turns**, each automation's **Active**, and one switch per skill in **Skills**. A switch that is on is highlighted with a check mark and a bold label; off is plain; a switch that cannot change right now is dimmed and says why (for example "Connect to a gateway first." or "A run is in progress." — next to the switch on touch screens, in its tooltip with a mouse). A switch applies at once; there is nothing to save. One-shot actions such as **Pause**, **Resume**, **Conclude**, **Revoke** and read-aloud playback stay buttons.

## Stream replies

**Model → Stream replies** shows the assistant's reply while the model writes it, instead of only when it is complete. Choose:

- **Gateway default** (the default): the gateway operator's setting decides. When the gateway reports it, the option reads **Gateway default (on)** or **Gateway default (off)**.
- **On** or **Off**: your choice for every new turn, whatever the gateway default.

The choice is remembered in this browser for your account and applies to the next turn. It is sent as `_runtime.stream` (`true` for On, `false` for Off, nothing for Gateway default); no other run setting changes. Off is always sent, so a gateway whose default is on never streams your replies against your choice.

While a reply streams, a live assistant bubble grows as text arrives (redrawn a few times per second); a sub-agent's live reply is labelled with its step. Images in assistant messages load only from this app's own address, such as files in the conversation's workspace; other images appear as links. When the model call ends, the complete message replaces the live text. If the call fails or is cancelled, the bubble becomes a short note saying so. If the connection drops, the live text disappears and comes back from the gateway when the page reconnects; nothing is shown twice. When the gateway runs a call without streaming (for example a call that must return structured output), the transcript shows a one-line note saying why, and the reply appears when it is complete.

The setting is available only when the gateway advertises live replies. Otherwise it stays visible but disabled, with the reason ("not supported by this gateway", or why the gateway's capabilities could not be read), and replies appear when they are complete. If you had chosen On, the conversation also shows one note: "Streaming is on in your settings but this gateway does not support live replies".

If the gateway sends a live update the app cannot read, the transcript shows one note for that model call, the update is skipped, and the run keeps going: the complete reply still arrives.

## Running and supervising work

For an agent, type a task and press Enter; Shift+Enter inserts a newline. For a structured workflow, configure its required inputs and select **Run workflow**. Text, structured JSON results, and workflow messages appear in the transcript.

Answer questions in their dedicated cards. Tool approvals show requested arguments and offer **Allow once** and **Deny**. The browser never accepts an approval automatically; gateway and workflow policy determine which operations need a decision.

An event-driven workflow stays attached while waiting for its trigger. Its wait card can also submit an explicit JSON event through the gateway's durable command path. Messages and status updates emitted by workflow nodes are replayable. See the [UI event contract](ui_events.md).

Use **Pause**, **Resume**, **Conclude**, or **Stop** to supervise an active run. Commands are requests, not optimistic lifecycle changes: the display follows confirmed gateway state. Guidance is consumed at supported workflow boundaries. **Queue next turn** keeps a local queue for this conversation. With **Run queued turns** on, each queued turn starts when the current run ends; off, the turns wait (**Run next** starts one). A failure or cancellation switches the queue off for explicit review. Switching conversations or signing out clears it. Unsent drafts and queued turns do not survive reload.

## Automations

The **Automations** section of the sidebar lists the gateway's automations —
the same ones the Assistant, the Observer and the terminal show — as cards
(see [Navigation panels](#navigation-panels)). **+** creates
one with its own workflow picker, initially set to the conversation's workflow. Selecting one opens its page. Its header shows
the title, the **Active** switch, the timing line, **waiting for you** when
something is pending, the workspace folder by its short name with **Open folder**
and **Copy path** icons, and the buttons **Run now**, **Stop**, **Edit** and
**Archive** (Archive asks for confirmation in place). Every action shows that it is
working, then its result or the gateway's reason for refusing. Below the header:
the definition, runs as chat pairs, approvals and questions, and its folder. **Discuss** on a
run opens the fork as a conversation here. See [Automations](automations.md).

## Workspaces and authorization

The gateway owns workspace roots, mount visibility, access modes, tool availability, and approval enforcement. Workspace controls are editable only when it permits client scope requests. Continuing an agent conversation restores its gateway-returned workspace instead of silently creating a different one.

Shared workflow restoration uses the gateway's verified public selection: registry scope, bundle ID, version, and flow ID. If that exact workflow is unavailable, restore it on the gateway or start a new conversation.

A turn sends exactly what you attached to it. Reopening a conversation restores its workflow fields (its request text, tool choices, limits) into the form, never a run's context, attachments or media; switching to another conversation or starting a new one resets the form to the workflow's defaults, so nothing from one conversation rides into the first turn of another.

Credentials are exchanged through the app server for HttpOnly session cookies. Gateway requests stay same-origin and mutations include the app's CSRF token. Appearance and non-secret settings may be saved locally; transcripts and run state are loaded from the gateway.

## Optional voice

When the gateway advertises configured speech capabilities, a conversation with a run offers hold-to-dictate and read-aloud controls. Hold the microphone button (or Space/Enter while focused), then release to transcribe into the draft. Review the text before sending. Read-aloud supports pause, resume, and stop.

Recording and playback happen in the browser; transcription and synthesis use the gateway's durable media endpoints. Microphone access requires permission and a secure context (HTTPS or localhost). No speech model runs in the browser.

## Opening the app from another computer over http

You can open AbstractCode from another computer at the gateway's plain http address, for example `http://<host>:8080/apps/code/` on your LAN or over Tailscale. Conversations, automations, files and copy buttons work there: when the browser withholds the clipboard, **Copy** uses the browser's copy command and says "Copied" or "Copy failed — select and copy". Browsers offer the microphone and camera only on https or on the computer itself, so over plain http the dictation button stays off and says: "This page is loaded over http, so voice and camera is unavailable — open it over https (for example through tailscale serve; the gateway console's Network page explains how) or on the gateway's own computer." To use voice from another computer, open the app through an https address such as `tailscale serve` or your own HTTPS reverse proxy (see [Web deployment](deployment-web.md#reverse-proxies-and-https)).

## Development and build

```bash
cd web
npm ci
npm run dev   # set the Gateway URL in the interface, or ABSTRACTCODE_GATEWAY_URL
```

Open `http://127.0.0.1:3002`. Vite and the packaged server provide the same connection and authenticated proxy routes.

```bash
npm run build
npm start -- --gateway-url http://127.0.0.1:8080
```

The build writes `web/dist/`. Serve it with the packaged server, not as a bare static site: connection/session middleware is part of the application.

## Shared chat integration

The app shell lives in `web/src/workspace/`. The reusable conversation view, wait controls, replay/stream controller, and React hook live in AbstractUIC's `@abstractframework/panel-chat` package. The controller accepts a transport; it does not own URLs, authentication, workspace policy, or browser storage. Other applications can reuse the chat inside a tab or observer without adopting Code's shell. See [architecture](architecture.md).

The terminal and browser share the gateway contract, not identical interfaces. Terminal slash commands, specialized memory/operator panels, and headless workflows remain terminal-specific. Voice depends on the gateway and browser. There is no offline execution mode.

## Differences from the terminal client

The browser client does not offer every terminal control. It does not expose review/verifier rounds, gating or prompt-cache settings, dedicated goal/entity-memory views, or GPU/resource/cache administration. Registered workflows can still implement those behaviors, but their generic input and activity surfaces are not substitutes for the specialized terminal controls.

Project instructions are not automatically read from the browser's local filesystem. Agent workflows can receive additional instructions manually; any automatic workspace discovery must happen through authorized gateway execution. Transcript export is Markdown, not the terminal's detailed/SFT JSONL export. Attachments support upload, removal, and download, but not the terminal's local-path browser. Tool permissions offer explicit per-tool choices rather than tier shortcuts. The prompt queue is not durable across reloads.
