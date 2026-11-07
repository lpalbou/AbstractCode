# AbstractCode Web

AbstractCode Web is a gateway-authenticated workspace for coding agents and registered workflows. The browser observes durable runs; it does not execute models, workflow nodes, or tools locally.

Start with [getting started](getting-started.md). For hosting and authentication, see [web deployment](deployment-web.md).

Archiving a conversation, the **Archived · N** lines and the gateway's default voice routes in **Voice** need AbstractGateway 0.13.0 or later.

## Working in the interface

- **Conversations** restores gateway sessions and history. Search by conversation text or session ID. The list shows 25 conversations; **Load more conversations** adds the next 25. A conversation is named by its first message (**Untitled conversation** when that is unknown), never by its session ID. AbstractNote's own work (capturing, filing and processing your notes, any session whose first turn runs an `abstractnote` workflow) is not a conversation and is not listed or found by search; opened from a link, such a session can be read but not continued, and the app says so. Whether a session is Notes' work is decided by its first turn, so a conversation stays listed whatever a later turn runs; for the same reason the workflow picker does not offer AbstractNote's flows ("Ask your notes").
- **The right panel** is a vertical rail at the right edge of the window with one icon per subject — **Activity**, **Files**, **Model**, **Agent**, **Workspace** and **Voice**. Clicking an icon opens its panel beside the rail; clicking it again (or the panel's collapse arrow) folds the panel back to the icons. The rail always stays; it is the only way into the panels (the header has no settings button).

| Panel | What it shows |
|---|---|
| Activity | The run as the transcript shows it — model steps, tool calls, approvals — in foldable groups, one per agent iteration, the newest open. Each command call shows, first in its detail, the sandbox it ran under, from the run ledger: `Sandbox: macOS sandbox-exec · 3 workspaces enforced` (the kind the gateway recorded and the number of workspaces it enforced, listed below with their mode: this run's folder, each allowed workspace Read-only or Read & write, each refused one, and the count of built-in protected folders), or `Sandbox: none — refused` when the host had no sandbox and the command did not run. For an automation: one group per run (newest first, the latest open); opening a group reads that run's steps from the gateway. |
| Files | The conversation's private workspace: name, size, generated date (relative; the exact time on hover) and a download icon per file. Click a file to preview it. Generated outputs and attachments are listed below, with the same rows. |
| Model | Compact Provider and Model controls. **Reasoning & speed** keeps reasoning and MTP options on demand; **Limits & streaming** contains context limits and live replies for conversations; background automations show **Limits**. |
| Agent | The conversation's workflow, folded tool categories with category and individual **Off / Ask / Auto**, skills, and optional instructions/run limits. Unavailable tools have their own disclosure. The default workflow for new chats is separate and stored on the account. For an automation, the same tab edits its definition and settings. |
| Workspace | The run folder and a concise shared folder chooser. **Use my default** inherits account workspaces; allowed folders appear once, blocked folders are folded, and permission details are available from the information button. **My default workspaces** opens the account settings. |
| Voice | Engines, output device and volume, microphone, read aloud and voice latency. |

Model, Agent and Workspace are bound to what is selected in the sidebar. Each starts with a line saying what it edits — **Conversation** and its title, or **Automation**, its title and its revision. Voice preferences apply to this app for your account, independently of the selected conversation or automation. Model and voice selectors read **Gateway default** until you override it; an override shows only while it is set, and choosing the default again removes it.

With an **automation** selected, Model, Agent and Workspace edit its saved definition in place. Typed fields save when you pause, pickers at once, the workflow form in Agent with its **Save**; every change is saved through the gateway as a new revision, the revision shown on every panel updates, and changes apply from the next run. The automation header's **Edit** opens its **Agent** panel. If someone else changed the automation meanwhile, the save is refused and the latest revision is shown.

The **Workflow** picker lists workflows authorized by the gateway for `abstractcode.agent.v1`. **Gateway default** follows the default the operator configured; selecting a named workflow pins the conversation to that workflow. Below it, **Default for new conversations** is the workflow every new conversation starts with: **Gateway default (name)** first (the gateway's words, the same as the Assistant and the console), then the workflows you may run. It is kept by the gateway for your account (`GET`/`PUT /api/gateway/accounts/me/preferences`, gateway 0.13.1 and later), so the Assistant, the console's Accounts → Preferences and every browser share it; a change applies at once ("Saved.", or "Not saved." and the gateway's sentence). Scheduling uses the account's effective time zone, inherited from the Gateway when unset; time zone is not a routine workflow control. A workflow this browser remembered before is moved to your account once (when your account has none yet) and then forgotten by the browser. With a gateway older than 0.13.1 the row is not shown and the conversation picker's choice is remembered in this browser, as before. Unset settings preserve workflow defaults. When skills are unavailable, **Skills** shows the gateway's explanation. See [workflows](workflows.md#the-gateway-default).

**Files** shows the conversation's private workspace on the gateway, where the agent reads and writes, once, as its short name — the full path is its tooltip — with **Open folder** (only on the gateway's own machine) and **Copy path** icons. Click a file to preview it with the shared viewer: Markdown rendered, code and JSON in the shared code viewer (highlighted; JSON pretty-printed), images and PDFs shown, audio in the shared waveform player (play, pause, seek), other text as text; HTML and SVG show as source. Text previews read at most the first 1 MiB and say so; Markdown images load only from the workspace. The preview header has the file's size and date, **Attach** (adds it to your next message), **Download** and close. Gateway admins also have **Gateway files (admin)**: the gateway's own files, browsed and attached the same way.

**Voice** uses compact shared controls. Click a **Text → speech** or **Speech → text** route to choose its engine, model and voice; **Use gateway default** clears an override. **Output** offers the speaker device, a **Test** chime and reply volume. **Microphone** offers an input device and a three-second recording/playback test, input level, spoken language and capture mode. **Replies** controls automatic reading of new replies and voice latency when supported. Information buttons explain browser limits and optional controls. These choices belong to this app in your browser for your account; they do not edit an automation or the Gateway defaults.

The panel width is resizable on wide screens (drag the panel's left edge, or focus it and use the arrow keys) and remembered with the open panel. The upper-right **About** widget is the shared AbstractFramework About: the app name and version, the AbstractFramework and AbstractGateway versions the connected gateway reports (or why one is missing), links (website, source, docs, issues, feedback, contact) and the author/licence line — no package list. See [Responsive layout](#responsive-layout).

The standard upper-right controls remain available: **Docs assistant** (book icon), **Appearance**, **About** and gateway connection. Next to the conversation's title, **⋯** holds **Archive** (see [Archiving a conversation](#archiving-a-conversation)). The Docs assistant is the same chat as the gateway console and the other apps: it answers from AbstractCode's llms.txt (this app serves it at `/llms.txt`; the gateway reads it at `GET /api/gateway/docs/corpus?app=code`) through the gateway's docs-qa workflow, in its own gateway session; it does not inherit the conversation’s settings or attachments. You can attach files to a question, the answer streams when the gateway's **Streamed replies** setting is on, each message has **Copy**, and the icon at the top starts a new conversation. Closing its drawer preserves its conversation. It uses the same shared drawer as the Gateway and other apps, with the composer pinned to the bottom.

MTP depth is independent of reasoning. Leave it on **Gateway default** to follow workflow and
execution-host defaults, choose **Off** to send an explicit `false`, or request an advertised
depth. **Off** and the depths appear only when the gateway says the selected model supports
MTP; otherwise the control is disabled and says why. Changing the provider or the model puts
reasoning and MTP back on **Gateway default**, and a saved level or depth the current model
cannot use is never sent. An explicit depth sets `require_acceleration`
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

- **Touch controls.** The rail icons, the **⋯** menus and every panel button are at least 44 px; the resize edge widens for touch.
- **Keyboard.** Up/Down, Home and End move between rail icons; Enter or Space opens one. On a floating panel, Escape folds it and returns focus to its icon. A dialog or dropdown above it handles Escape first. The resize edge takes Left/Right (Shift for bigger steps), Home and End.
- **On-screen keyboard.** The panel content scrolls within the remaining height.
- **Appearance.** The upper-right appearance widget contains theme and text-size controls. Rows and paths wrap; nothing scrolls sideways.
- **Conversation navigation.** The sidebar menu closes when you open a rail panel, keeping one navigation surface in front of the conversation.

### Navigation panels

The sidebar holds two stacking drawers, **Conversations** above **Automations**, both full width. Cards have 8 px gaps and 12 px side insets. Conversation unread marks use the Gateway's account-wide read ledger: consuming loaded content clears the same revision in other clients, while a later answer remains unread. Failed acknowledgments retain the mark. Each header has a fold control, **+** and refresh; the Conversations **+** starts a new conversation (also ⇧⌘N). Both headers always stay visible:

| Automations | Conversations | Layout |
|---|---|---|
| closed | closed | The two header rows at the top. |
| open | closed | Automations fills the remaining space below the collapsed Conversations header. |
| closed | open | Conversations fills the space above the Automations header, which sits mid-height. |
| open | open | An even split; the Automations header stays mid-height. |

The Automations header's badge counts what waits for you. On a gateway with your event stream
(`/api/gateway/me/events`, read through this app's proxy) it is the approvals and questions waiting
for you plus the automation results you have not seen (its tooltip says "1 waiting for you · 2 new");
the stream's live tail stays open while the tab is visible and resumes with `Last-Event-ID`, a hidden
tab reads it once a minute, and the automation list is re-read after each automation event instead
of every 30 seconds. On a gateway without the stream the badge is the list's `pending_waits +
unseen_count` sum, read every 30 seconds while the tab is visible.

Each list scrolls inside its own drawer. Fold choices are remembered in this browser (`abstractcode.sidebar.panels`). On a short landscape phone screen the whole drawer scrolls instead. The current workspace is shown in the **Workspace** panel and in **Files**.

At the end of each list a quiet line **Archived · N** appears when the gateway reports archived items (N is the gateway's count: `archived_automations` of `GET /automations`, `archived_sessions` of `GET /runs?root_only=true`; no line at 0). Click it to show the archived automations or conversations inline, each with **Unarchive** (an automation comes back paused, with its history; a conversation comes back to the list). Clicking an archived item's name opens it. Whether each line is open is remembered in this browser (`abstractcode.sidebar.archived`). Archived items never appear among the live rows.

#### Archiving a conversation

Every conversation card has a **⋯** (shown on hover or focus with a mouse, always on touch; always on the open conversation), and so does the conversation's title in the header. Choose **Archive** and confirm in place: "Archive this conversation? It stays searchable and auditable; it just leaves this list." The app calls `POST /api/gateway/sessions/{session_id}/archive`; nothing is deleted (runs, ledger and files stay on the gateway, and AbstractObserver keeps showing the runs, marked **Archived**). The conversation leaves the list and appears under **Archived · N**, where **Unarchive** brings it back. Archiving the open conversation opens the next one in the list (or the previous one; a new conversation when it was the last). A refusal from the gateway is shown under the confirmation as a sentence.

A conversation card shows its title on one line, then `Oct 2 · 2 turns · 7 tools`: the date of the latest turn, the number of turns and, when there were any, the tool calls across them. The tool figure comes from the gateway (`GET /api/gateway/runs?include_metrics=true`, each turn's total including its sub-runs); with a gateway that does not report it, the card shows no tool figure.

An automation card shows its name on its own line, **waiting for you** while an approval or question is pending, then two quiet lines: `↻ every 24 h · last 3 h ago`, and `next in 20 h` with the **Active** switch right-aligned on that line (relative times rounded down, no year or seconds; `last never` before the first run, `running now` while a run executes, `waiting since 5 min` while a run waits for you; the second line holds only the switch when nothing is scheduled).

### Automation detail sections

In an automation's page, **Occurrences** and the folder ("Automation folder", or "Run #N folder") have a header you can click or tap to fold the list away. Both start open, and your choice is remembered in this browser (`localStorage`, key `abstractcode.automation.panels`).

### Switches

Every setting that is either on or off is a switch named after what it controls: **Run queued turns**, each automation's **Active**, and one switch per skill in **Skills**. A switch that is on is highlighted with a check mark and a bold label; off is plain; a switch that cannot change right now is dimmed and says why (for example "Connect to a gateway first." or "A run is in progress." — next to the switch on touch screens, in its tooltip with a mouse). A switch applies at once; there is nothing to save. One-shot actions such as **Pause**, **Resume**, **Conclude**, **Revoke** and read-aloud playback stay buttons.

## Stream replies

For a conversation, **Model → Limits & streaming → Stream replies** shows the assistant's reply while the model writes it, instead of only when it is complete. Choose:

- **Gateway default** (the default): the gateway operator's setting decides. When the gateway reports it, the option reads **Gateway default (on)** or **Gateway default (off)**.
- **On** or **Off**: your choice for every new turn, whatever the gateway default.

The choice is remembered in this browser for your account and applies to the next turn. It is sent as `_runtime.stream` (`true` for On, `false` for Off, nothing for Gateway default); no other run setting changes. Off is always sent, so a gateway whose default is on never streams your replies against your choice.

While a reply streams, a live assistant bubble grows as text arrives (redrawn a few times per second); a sub-agent's live reply is labelled with its step. Images in assistant messages load only from this app's own address, such as files in the conversation's workspace; other images appear as links. When the model call ends, the complete message replaces the live text. If the call fails or is cancelled, the bubble becomes a short note saying so. If the connection drops, the live text disappears and comes back from the gateway when the page reconnects; nothing is shown twice. When the gateway runs a call without streaming (for example a call that must return structured output), the transcript shows a one-line note saying why, and the reply appears when it is complete.

The setting is available only when the gateway advertises live replies. Otherwise it stays visible but disabled, with the reason ("not supported by this gateway", or why the gateway's capabilities could not be read), and replies appear when they are complete. If you had chosen On, the conversation also shows one note: "Streaming is on in your settings but this gateway does not support live replies".

If the gateway sends a live update the app cannot read, the transcript shows one note for that model call, the update is skipped, and the run keeps going: the complete reply still arrives.

## Running and supervising work

For an agent, type a task and press Enter; Shift+Enter inserts a newline. For a structured workflow, configure its required inputs and select **Run workflow**. Text, structured JSON results, and workflow messages appear in the transcript.

Answer questions in their dedicated cards. Tool approvals show requested arguments and offer **Allow once** and **Deny**. On a gateway that serves the chat tool policy (AbstractGateway 0.16.0, `capabilities.session_tool_policy`), **Allow for this chat** and **Allow all <toolset> tools for this chat** store the grant on the gateway for this conversation, so later calls run without a gate from every client; on an older gateway the button says what it does instead (**Allow for this run**, this browser only). **Allow all enabled tools** sets every enabled tool to Auto in Agent → Tools (this browser). Gateway and workflow policy determine which operations need a decision.

An event-driven workflow stays attached while waiting for its trigger. Its wait card can also submit an explicit JSON event through the gateway's durable command path. Messages and status updates emitted by workflow nodes are replayable. See the [UI event contract](ui_events.md).

Use **Pause**, **Resume**, **Conclude**, or **Stop** to supervise an active run. Commands are requests, not optimistic lifecycle changes: the display follows confirmed gateway state. Guidance is consumed at supported workflow boundaries. **Queue next turn** keeps a local queue for this conversation. With **Run queued turns** on, each queued turn starts when the current run ends; off, the turns wait (**Run next** starts one). A failure or cancellation switches the queue off for explicit review. Switching conversations or signing out clears it. Unsent drafts and queued turns do not survive reload.

## Automations

The **Automations** section of the sidebar lists the gateway's automations —
the same ones the Assistant, the Observer and the terminal show — as cards,
except AbstractNote's own automations ("Process my notes", "Reflect on my notes",
"Consolidate my notes": those whose workflow is in the `abstractnote` bundle), which
are Notes' background work and are left out of the list and of its `Paused · N` and
`Archived · N` counts (the Conversations `Archived · N` leaves Notes' sessions out the same way)
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

The gateway owns the workspaces, tool availability and approval enforcement, on three levels (the kit `WorkspaceChooser` shows each with the same words as the console, Flow, Observer and the AbstractAssistant):

- **The gateway** (the admin): the *eligible* workspaces — a posture, **Deny everything, allow listed workspaces** or **Allow everything, refuse listed workspaces** (everything else at one default mode), and workspaces each capped at **Read & write**, **Read-only** or **Refused**. Nobody below can reach outside it or raise a cap.
- **Your default** (**My default workspaces**, under the panel): your own subset among the eligible workspaces, with the same two postures and modes; **Follow the gateway policy** = exactly what the gateway allows. Each change is one `PUT /api/gateway/workspace/policy/me`.
- **This conversation** (the **Workspace** panel): the workspaces this conversation uses, starting from your default. Each change is one `PUT /api/gateway/sessions/{session_id}/workspaces`; the gateway stores the choice on the session, so every app opening the conversation sees it. **Use my default** goes back to your default.

The panel shows, top to bottom: the gateway's line ("Gateway: Allow everything, refuse listed workspaces (rw) · /archive (ro)"), **Use my default**, the posture, each workspace with Read & write / Read-only / Refused and a remove button (a mode above the gateway's cap is disabled, with the tooltip "The gateway allows this workspace read-only"), **Add a workspace path**, and the effective line, verbatim from the gateway (for example "Deny everything, allow listed workspaces · /Users/me/Pictures (rw) · /Users/me/Documents (ro)"). There is no Save button. A change the gateway refuses (a path outside the eligible workspaces, a mode above a cap) shows the gateway's sentence with "Not saved." and nothing changes.

Each conversation also has its private workspace (**Current workspace session-…** at the top of the panel): created by the gateway, always Read & write, where relative paths are written.

An automation keeps its own workspaces in its definition (`input_data.workspace`, the same `{posture, default_mode, folders}` a one-off run sends, saved as a new revision); what they mean is the gateway's answer for them (`POST /api/gateway/workspace/effective/me`). Until you change them the automation uses your default (**Use my default** returns to that). Conversation turns send no workspace list: the gateway applies this conversation's workspaces (else your default, else the gateway's) at every run start and refuses anything outside the eligible workspaces. Continuing an agent conversation restores its gateway-returned workspace instead of silently creating a different one.

Shared workflow restoration uses the gateway's verified public selection: registry scope, bundle ID, version, and flow ID. When the version a conversation recorded is no longer available but the same workflow is loaded in another version, the conversation continues with the loaded version (from the other registry, the shared catalog or the private one, when only that copy is left) and a line above it says so ("This conversation started with a version of … that is no longer available. It continues with the version available now, so its next answers may differ."). Only when the workflow itself is gone is the next turn refused: "The workflow this conversation used is no longer available, so it cannot be continued here. Start a new conversation to keep working."

A turn sends exactly what you attached to it. Reopening a conversation restores its workflow fields (its request text, tool choices, limits) into the form, never a run's context, attachments or media; switching to another conversation or starting a new one resets the form to the workflow's defaults, so nothing from one conversation rides into the first turn of another.

Credentials are exchanged through the app server for HttpOnly session cookies. Gateway requests stay same-origin and mutations include the app's CSRF token. Appearance and non-secret settings may be saved locally; transcripts and run state are loaded from the gateway.

## Optional voice

When the gateway advertises configured speech capabilities, the composer offers dictation, from the first message of a new conversation on, and each reply has a speaker button. The microphone is press to talk, like Space in the terminal client: hold it (or Space/Enter while it is focused) while you speak, and release to send what you said, after anything already typed. A quick tap records nothing and reminds you to hold. When the message cannot be sent (a run is starting, or the send fails), the words stay in the draft. While it works the composer shows "Recording… 3 s", then "Transcribing… 12 s · faster-whisper / large-v3" (the route: your override, else the gateway default). A recording that is too short, silent or fails says so in a sentence. Transcription speed is the route's: faster-whisper large-v3 runs on the CPU of a Mac (about 25 s for a 4 s sentence, 9 s with the spoken language named); a smaller model such as `small` takes 1–2 s. A reply's speaker reads it aloud, with pause and resume; while a reply plays, the composer shows **Stop spoken reply**. The composer has no speaker button of its own.

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

Settings keep explanations behind themed information buttons: hover for 0.2 seconds or focus the button with the keyboard. Errors and save refusals remain visible.

The **Docs assistant** shows a speaker on assistant replies when speech is configured. Click it to read the reply aloud, pause or resume. Closing the drawer stops playback.


## Avatar conversation view

Choose **Avatar** in the conversation header for a second view over the same work. Code always
opens on **Conversation**; Avatar lasts for the current visit.
The avatar shows activity and, under it, one live line: the latest thing the assistant
reports while it works, replaced as soon as something new arrives (nothing cycles). Click
it to read it in full, then **Reply to this** to answer it or **Latest** to follow again.
The findings it reports (facts or evidence it established) accumulate in a
**Findings** panel beside the avatar, or a sheet you open with **Findings** on a phone; open
one to see the results it came from. The answer appears once, as a message, with Markdown,
links, files and structured results like the conversation.
**Conversation** restores the ordinary chat and technical details. Switching views
preserves the current draft and attachments.

While work continues, type or speak naturally to guide it. **Reply to this** on the live line pins a
specific public update as context. Sending a correction makes it available to the
assistant; the assistant's later response shows whether it adopted the correction.
Questions use their displayed answer controls. Permission requests require an explicit
choice; an ordinary message or silence grants no permission.

Essential questions keep waiting. A clarification may state a two-to-five minute
answer deadline and what will happen without an answer. **Conversation cues** controls
visual and audio alerts independently for questions, permission requests and completion.
Audio alerts default off. Voice narration uses the existing voice preference;
reading history or editing a reply silences automatic narration. Talk with the composer's
microphone (talking stops a reply that is being read aloud); play or pause a reply (and the live
line) with its own play button, and stop it with the X in the composer, without stopping the task. The avatar shows the speaking pose only while the reply is audible.

Public updates require a run started in Avatar mode and a backend supporting the public
conversation protocol. Switching an already active legacy run changes its presentation;
it does not manufacture missing updates. Tool details remain available in the ordinary
view. Public updates describe the assistant's communicated conclusions, not private
reasoning, and source references establish provenance rather than proof of correctness.
