# AbstractCode Web — Deployment (gateway-first)

AbstractCode Web (`web/`) is a browser/PWA host that talks only to **AbstractGateway** (`/api/gateway/*`).

Related:
- Web app overview + local dev caveats: [`docs/web.md`](web.md)
- Architecture: [`docs/architecture.md`](architecture.md)

## 1) Run AbstractGateway

Run a gateway that serves `/api/gateway/*` and (for browsers) allows your web app origin (CORS).

Example (token + allowed origins):

```bash
export ABSTRACTGATEWAY_AUTH_TOKEN="dev-token"
export ABSTRACTGATEWAY_ALLOWED_ORIGINS="http://localhost:*,http://127.0.0.1:*"

abstractgateway serve --host 127.0.0.1 --port 8081
```

Notes:
- Web UI uses gateway discovery endpoints for dropdowns: providers/models/tools.
- Web UI uses gateway file endpoints for `@file` mentions: `/api/gateway/files/search` and `/api/gateway/files/read`.
- Web UI uses gateway attachment endpoints for uploads: `/api/gateway/attachments/upload`.
- (Optional) Voice features use: `/api/gateway/runs/{run_id}/audio/transcribe` and `/api/gateway/runs/{run_id}/voice/tts`.

## 2) Run AbstractCode Web (dev)

```bash
cd web
npm install
npm run dev
```

Open `http://127.0.0.1:3002/` and set:
- `Gateway URL`: `http://127.0.0.1:8081`
- `Gateway user` and that user's `Gateway token` when Gateway user auth is
  enabled

## 3) Build + host (static)

```bash
cd web
npm run build
```

Deploy `web/dist/` behind the packaged web server or a reverse proxy that
routes same-origin `/api/...` to Gateway.

In hosted user-auth mode, AbstractCode Web exchanges the Gateway user token for
an app-scoped browser session and strips bearer tokens from saved browser
settings. Direct bearer-token mode is retained for local development when no
Gateway user is configured. When the web UI is served from a non-local
hostname, the server-configured Gateway URL is authoritative; browser-supplied
Gateway URL changes are rejected unless
`ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1` is enabled behind your own
access control. If a reverse proxy rewrites `Host`, set
`ABSTRACTCODE_TRUST_PROXY_HEADERS=1` only when the proxy strips
client-supplied forwarded headers.
