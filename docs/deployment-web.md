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

## 2) Run AbstractCode Web (dev)

```bash
cd web
npm install
npm run dev
```

Open `http://127.0.0.1:3002/` and set:
- `Gateway URL`: `http://127.0.0.1:8081`
- `Auth token`: `dev-token` (if configured)

## 3) Build + host (static)

```bash
cd web
npm run build
```

Deploy `web/dist/` with any static file server.

If you serve the web app on the **same origin** as the gateway, you can set `Gateway URL` to empty and rely on same-origin `/api/gateway/*` routing.
