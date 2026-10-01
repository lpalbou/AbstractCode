# AbstractCode Web

AbstractCode Web is a gateway-authenticated workspace for coding agents and registered workflows. The browser observes durable runs; it does not execute models, workflow nodes, or tools locally.

Start with [getting started](getting-started.md). For hosting and authentication, see [web deployment](deployment-web.md).

## Working in the interface

- **Conversations** restores gateway sessions and history. Search by conversation text or session ID. The list shows 25 conversations; **Load more conversations** adds the next 25.
- **Workflow** chooses what runs your next turn. The first entry, **Gateway default → name @version**, follows the default coding agent your gateway's operator set; if they change it, your next new turn uses the new one. Below it are the coding agents published on your gateway; switch on **Show all workflows** to list schema-driven workflows too. Your choice is remembered in this browser for your account. After you send, the toolbar shows which workflow the gateway actually started (for example "running Coder @0.1.0 (gateway default)"). A conversation whose last run came from the gateway default keeps following it on every turn, on any device (so a change on the gateway applies to its next turn); a conversation started with a specific workflow keeps that workflow. If the list says "gateway does not report a default agent workflow", the gateway does not provide a default: pick a workflow from the list, or update AbstractGateway. When the gateway's default does not declare the coding-agent interface, the list says so instead of using it. See [workflows](workflows.md#the-gateway-default).
- **Inputs** presents the registered input schema. Use JSON input mode for nested or custom input objects. The gateway validates submitted values.
- **Settings** contains model, reasoning, MTP depth, tool policy, workspace requests, and gateway skills for agent workflows. Unset options preserve workflow defaults. When the gateway offers no skills, the Skills tab shows the gateway's own explanation and where its skill shelf is.
- **Files** shows this conversation's workspace: the folder on the gateway where the agent reads and writes. The header gives its full path with a copy button; when your browser is not on the gateway's machine it adds "on the gateway host <name>", and when it is, **Open folder** opens the folder there. Browse folders with sizes and dates, refresh after a turn, and select a file to preview it: Markdown, JSON, images, other text, and HTML as source; other files offer a download. Text previews show at most the first 1 MiB and say so. Markdown previews show only images stored in the workspace; other images appear as links. A note says when the gateway listed only part of a large folder. **Attach to conversation** adds the selected file to your next message. Gateway admins also get **Shared workspace (admin)**, the operator's shared folder, where clicking a file attaches it. Agents accept attachment references; generic workflows must declare an `attachments` array input.
- **Activity** shows durable workflow steps and tool arguments/results.
- **Artifacts** downloads files stored with the selected run.

Use the appearance control in the header to choose an AbstractUIC theme. The **About** button next to it shows this app's version, the AbstractFramework links (website, source, documentation, issues, feedback), and the versions your gateway reports; when the gateway cannot answer, it shows "Gateway: unavailable" with the HTTP status. The layout adapts to the screen you use; see [Responsive layout](#responsive-layout).

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
| 1440 px and wider (laptops, external displays) | Three docked panes: conversations and automations on the left, the conversation, the workspace inspector on the right. |
| 1024 to 1439 px (smaller laptop windows, iPad landscape) | The left sidebar stays docked; the workspace inspector opens as an overlay from its header button. If you had it open at 1440 px or wider, it returns docked when the window is wide again. |
| Below 1024 px (tablets in portrait, narrow windows, phones) | The conversation uses the full width. The menu button opens conversations and automations as a drawer; the workspace button opens the inspector as a drawer. Close a drawer with Escape, a tap on the dimmed backdrop, or its close button. |

- **Phones use the whole screen.** Content reaches the screen edges with margins of 16 px or less, and nothing is drawn as a box inside a box. The automation detail is one flat page: each fact sits on one line with its label ("When  every 24 hours"), paths and identifiers take a full line as plain text, and the definition, each occurrence, its transcript and the folder are separated by thin lines. The approval card is part of the conversation, and a long tool argument such as a file path wraps. The navigation drawer takes the full width and scrolls as one list (Automations, then Conversations); tablets keep the narrower drawer, also as one list. On tablets the automation detail shows two columns only when each is at least about 360 px wide.
- **Phone toolbar and dialogs.** The toolbar fits one row (the model is chosen in **Settings**), dialogs such as sign-in, **Schedule a task**, appearance and **About** open as bottom sheets with their buttons always visible, and the composer shows its icons on one row and the destination, **Stop** and **Send** on the next.
- **Phone landscape.** The header and toolbar share one thin row and the status bar is hidden, so the conversation keeps most of the height.
- **On-screen keyboard.** While you type, the header, toolbar, run strip and status bar step aside and the message field is limited to about 30 % of the visible height; they return when the keyboard closes.
- **Touch.** Buttons and list rows are at least 44 px tall and text fields use 16 px text, so iOS does not zoom when you focus one. Keyboard shortcut hints are hidden on touch-only devices, and file actions show without hover.
- **Text size.** On touch screens, reading, helper and toolbar text is 14 px and body text 15 px; dates, counts and badges are 12 px. On desktops nothing is smaller than 12 px, and helper text and labels are 13 px. The font size setting in the appearance control scales everything.
- **Zoom and notches.** Pinch zoom is allowed, and the layout respects the safe areas of notched phones.
- **Escape.** One press closes the topmost layer only: with **Settings** or a dialog open over the navigation drawer, the drawer stays open.

### Collapsible sidebar sections

The **Automations** and **Conversations** sections of the left navigation (docked sidebar, overlay or phone drawer) fold and unfold when you click their header. The **+** and refresh buttons next to a header keep their own action. Open sections share the height and scroll independently; a folded section gives its space to the other. Both start open, and your choice is remembered in this browser (`localStorage`, key `abstractcode.sidebar.panels`); if the browser blocks storage, both sections simply start open. Each section sits on its own panel background with its header inside the panel.

The **Workspace** row at the bottom of the sidebar shows the conversation's folder name on one line, shortened with "…" when it is long; hover it to see the full path.

### Automation detail sections

In an automation's page, **Occurrences** and the folder ("Automation folder", or "Run #N folder") have a header you can click or tap to fold the list away. Both start open, and your choice is remembered in this browser (`localStorage`, key `abstractcode.automation.panels`).

### Switches

Every setting that is either on or off is a switch named after what it controls: **Show all workflows** (shown as "All" on phones), **Run queued turns**, each automation's **Active**, **Show archived** in the Automations section, and one switch per skill in **Settings**. A switch that is on is highlighted with a check mark and a bold label; off is plain; a switch that cannot change right now is dimmed and says why (for example "Connect to a gateway first." or "A run is in progress." — next to the switch on touch screens, in its tooltip with a mouse). A switch applies at once; there is nothing to save. One-shot actions such as **Pause**, **Resume**, **Conclude**, **Revoke** and read-aloud playback stay buttons.

## Stream replies

**Settings → Model & behavior → Stream replies** shows the assistant's reply while the model writes it, instead of only when it is complete. Choose:

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
the same ones the Assistant, the Observer and the terminal show — with their
state ("Active ▶", "Paused ⏸"), what runs now and the next run. **+** creates
one that runs the toolbar's workflow. Selecting one opens its page: controls,
runs as chat pairs, approvals and questions, and its folder. **Discuss** on a
run opens the fork as a conversation here. See [Automations](automations.md).

## Workspaces and authorization

The gateway owns workspace roots, mount visibility, access modes, tool availability, and approval enforcement. Workspace controls are editable only when it permits client scope requests. Continuing an agent conversation restores its gateway-returned workspace instead of silently creating a different one.

Shared workflow restoration uses the gateway's verified public selection: registry scope, bundle ID, version, and flow ID. If that exact workflow is unavailable, restore it on the gateway or start a new conversation.

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
