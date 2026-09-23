# AbstractCode Web

AbstractCode Web is a gateway-authenticated workspace for coding agents and registered workflows. The browser observes durable runs; it does not execute models, workflow nodes, or tools locally.

Start with [getting started](getting-started.md). For hosting and authentication, see [web deployment](deployment-web.md).

## Working in the interface

- **Conversations** restores gateway sessions and history. Search by conversation text or session ID; load more to browse older runs.
- **Workflow** selects a private bundle entrypoint or an authorized shared catalog workflow. Agent interfaces and schema-driven workflows are both supported.
- **Inputs** presents the registered input schema. Use JSON input mode for nested or custom input objects. The gateway validates submitted values.
- **Settings** contains model, reasoning, MTP depth, tool policy, workspace requests, and gateway skills for agent workflows. Unset options preserve workflow defaults.
- **Files** browses the gateway's authorized shared files, not a local disk or the run's private working directory. Attach a shared file or upload from your device. Agents accept attachment references; generic workflows must declare an `attachments` array input.
- **Activity** shows durable workflow steps and tool arguments/results.
- **Artifacts** downloads files stored with the selected run.

Use the appearance control in the header to choose an AbstractUIC theme. On narrow screens, conversation navigation and the inspector open on demand.

MTP depth is independent of reasoning. Leave it on **Inherit** to follow workflow and
execution-host defaults, choose **Off** to send an explicit `false`, or request an advertised
depth. Choices come from provider/model execution discovery; missing support is unknown,
and saved unavailable choices stay visible. An explicit depth sets `require_acceleration`
so the host must honor it or report an error. The browser saves this preference locally and
sends it as `_runtime.speculation`; it does not load models, download heads, or change the
shared Core default. Fresh Core configurations use depth 2 only for compatible models.

## Running and supervising work

For an agent, type a task and press Enter; Shift+Enter inserts a newline. For a structured workflow, configure its required inputs and select **Run workflow**. Text, structured JSON results, and workflow messages appear in the transcript.

Answer questions in their dedicated cards. Tool approvals show requested arguments and offer **Allow once** and **Deny**. The browser never accepts an approval automatically; gateway and workflow policy determine which operations need a decision.

An event-driven workflow stays attached while waiting for its trigger. Its wait card can also submit an explicit JSON event through the gateway's durable command path. Messages and status updates emitted by workflow nodes are replayable. See the [UI event contract](ui_events.md).

Use **Pause**, **Resume**, **Conclude**, or **Stop** to supervise an active run. Commands are requests, not optimistic lifecycle changes: the display follows confirmed gateway state. Guidance is consumed at supported workflow boundaries. **Queue next turn** keeps a local queue for this conversation; a failure or cancellation pauses it for explicit review. Switching conversations or signing out clears it. Unsent drafts and queued turns do not survive reload.

## Workspaces and authorization

The gateway owns workspace roots, mount visibility, access modes, tool availability, and approval enforcement. Workspace controls are editable only when it permits client scope requests. Continuing an agent conversation restores its gateway-returned workspace instead of silently creating a different one.

Shared workflow restoration uses the gateway's verified public selection: registry scope, bundle ID, version, and flow ID. If that exact workflow is unavailable, restore it on the gateway or start a new conversation.

Credentials are exchanged through the app server for HttpOnly session cookies. Gateway requests stay same-origin and mutations include the app's CSRF token. Appearance and non-secret settings may be saved locally; transcripts and run state are loaded from the gateway.

## Optional voice

When the gateway advertises configured speech capabilities, a conversation with a run offers hold-to-dictate and read-aloud controls. Hold the microphone button (or Space/Enter while focused), then release to transcribe into the draft. Review the text before sending. Read-aloud supports pause, resume, and stop.

Recording and playback happen in the browser; transcription and synthesis use the gateway's durable media endpoints. Microphone access requires permission and a secure context (HTTPS or localhost). No speech model runs in the browser.

## Development and build

```bash
cd web
npm ci
ABSTRACTCODE_GATEWAY_URL=http://127.0.0.1:8080 npm run dev
```

Open `http://127.0.0.1:3002`. Vite and the packaged server provide the same connection and authenticated proxy routes.

```bash
npm run build
HOST=127.0.0.1 npm start
```

The build writes `web/dist/`. Serve it with the packaged server, not as a bare static site: connection/session middleware is part of the application.

## Shared chat integration

The app shell lives in `web/src/workspace/`. The reusable conversation view, wait controls, replay/stream controller, and React hook live in AbstractUIC's `@abstractframework/panel-chat` package. The controller accepts a transport; it does not own URLs, authentication, workspace policy, or browser storage. Other applications can reuse the chat inside a tab or observer without adopting Code's shell. See [architecture](architecture.md).

The terminal and browser share the gateway contract, not identical interfaces. Terminal slash commands, specialized memory/operator panels, and headless workflows remain terminal-specific. Voice depends on the gateway and browser. There is no offline execution mode.

## Current parity limits

The redesigned web client is not a complete port of every Rust control. It does not yet expose review/verifier rounds, gating or prompt-cache settings, dedicated goal/entity-memory views, or GPU/resource/cache administration. Registered workflows can still implement those behaviors, but their generic input and activity surfaces are not substitutes for the specialized terminal controls.

Project instructions are not automatically read from the browser's local filesystem. Agent workflows can receive additional instructions manually; any automatic workspace discovery must happen through authorized gateway execution. Transcript export is Markdown, not the terminal's detailed/SFT JSONL export. Attachments support upload, removal, and download, but not the terminal's local-path browser or rich preview. Tool permissions offer explicit per-tool choices rather than tier shortcuts. The prompt queue is not durable across reloads.
