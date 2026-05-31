# AbstractCode Web (Gateway-First)

This is the **web version of AbstractCode** (the Python CLI), designed as a **gateway-first** host UI:
- talks only to **AbstractGateway** (`/api/gateway/*`)
- renders by replaying/streaming the ledger
- resumes waits by submitting durable commands

Status:
- This app currently consumes shared UI components via Vite path aliases to a sibling `abstractuic/` repo.
- This repo includes a prebuilt `web/dist/` for convenience.

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
local development only. When the web UI is served from a non-local hostname,
the server-configured Gateway URL is authoritative; browser-supplied Gateway URL
changes are rejected unless `ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1`
is set behind your own access control. If a reverse proxy rewrites `Host`, set
`ABSTRACTCODE_TRUST_PROXY_HEADERS=1` only when the proxy strips client-supplied
forwarded headers.
