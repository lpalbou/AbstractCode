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

Shared UI components come from the published `@abstractframework/*` packages
(see `package.json`), so this app builds from its own directory with no other
checkout present.

Docs:
- Web overview: [`../docs/web.md`](../docs/web.md)
- Deployment: [`../docs/deployment-web.md`](../docs/deployment-web.md)
  - Voice features (optional): push-to-talk transcription + TTS (see `../docs/web.md`)

## Local dev
```bash
cd web
npm install
npm run dev
```

In the UI:
- set `Gateway URL` (blank = same origin / dev proxy; e.g. `http://127.0.0.1:8081`)
- in hosted user-auth mode, set `Gateway user` and that user's `Gateway token`

When a Gateway user is provided, the web server exchanges the token for a
Gateway browser session and stores only app-scoped session cookies. The raw
token is not persisted in browser settings. Direct bearer-token mode is kept for
local development only.

Whether a browser may change the Gateway URL is decided by the **connection
peer**, not by any request header: only a request arriving from loopback may
reconfigure it, and the server-configured Gateway URL is authoritative for
everyone else. Set `ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1` to allow
it from anywhere, behind your own access control. Behind a reverse proxy every
peer is the proxy, so loopback carries no meaning there — set
`ABSTRACTCODE_TRUST_PROXY_HEADERS=1` to refuse browser-supplied changes
regardless of peer, and add the env var above if you still want to permit them.
