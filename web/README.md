# AbstractCode Web (Gateway-First)

The browser client for [AbstractCode](../README.md), published to npm as
`@abstractframework/code`. Like the terminal client, it is a **gateway-first**
host UI:
- talks only to **AbstractGateway** (`/api/gateway/*`)
- renders by replaying/streaming the ledger
- resumes waits by submitting durable commands

Run it without installing anything:

```bash
npx @abstractframework/code      # serves on http://127.0.0.1:3002
```

The interface uses AbstractUIC themes, authentication, settings, and reusable
workflow chat. It builds without a sibling checkout: the shared
`@abstractframework/*` components install from npm like any other dependency,
at the ranges declared in `package.json`.

Docs:
- Web overview: [`../docs/web.md`](../docs/web.md)
- Deployment: [`../docs/deployment-web.md`](../docs/deployment-web.md)
  - Voice features (optional): push-to-talk transcription + TTS (see `../docs/web.md`)

## Local dev
```bash
cd web
npm ci
npm run dev
```

In the UI:
- set `Gateway URL` (e.g. `http://127.0.0.1:8080`), or configure it on the
  server with `ABSTRACTCODE_GATEWAY_URL`
- set `Gateway user` and that user's `Gateway token`

When a Gateway user is provided, the web server exchanges the token for a
Gateway browser session and stores only app-scoped session cookies. The raw
token is not persisted in browser settings. Vite development and the packaged
server use the same session proxy; the browser keeps API requests same-origin.

Choose a workflow in the toolbar: **Gateway default** (the coding agent your
gateway's operator set, resolved by the gateway when the turn starts), a
published coding agent, or, with **Show all workflows**, any registered
workflow. The **Files** tab shows the conversation's workspace on the gateway
with previews, **Settings → Stream replies** shows replies as the model writes
them (on gateways that support it), and **About** lists the app's and the
gateway's versions. Agent tasks use the composer; structured workflows use
**Inputs**. Questions, tool approvals, and
event waits appear in the conversation. Runs and history remain on the gateway
when you close the browser. Unsent drafts and queued turns do not survive reload.

Whether a browser may change the Gateway URL is decided by the **connection
peer**, not by any request header: only a request arriving from loopback may
reconfigure it, and the server-configured Gateway URL is authoritative for
everyone else. Set `ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1` to allow
it from anywhere, behind your own access control. Behind a reverse proxy every
peer is the proxy, so loopback carries no meaning there — set
`ABSTRACTCODE_TRUST_PROXY_HEADERS=1` to refuse browser-supplied changes
regardless of peer, and add the env var above if you still want to permit them.
