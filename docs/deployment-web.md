# AbstractCode Web — Deployment (Gateway-First)

AbstractCode Web (`abstractcode/web/`) is a browser/PWA host that talks only to **AbstractGateway** (`/api/gateway/*`).

## 1) Run AbstractGateway

Example (bundle mode):

```bash
export ABSTRACTGATEWAY_DATA_DIR="./runtime"
export ABSTRACTGATEWAY_FLOWS_DIR="./flows/bundles"
export ABSTRACTGATEWAY_AUTH_TOKEN="dev-token"
export ABSTRACTGATEWAY_ALLOWED_ORIGINS="http://localhost:*,http://127.0.0.1:*"

# Optional: workspace root used by @file mentions
export ABSTRACTGATEWAY_WORKSPACE_DIR="."

abstractgateway serve --host 127.0.0.1 --port 8080
```

Notes:
- Web UI uses gateway discovery endpoints for dropdowns: providers/models/tools.
- Web UI uses gateway file endpoints for `@file` mentions: `/api/gateway/files/search` and `/api/gateway/files/read`.

## 2) Run AbstractCode Web (dev)

```bash
cd abstractcode/web
npm install
npm run dev
```

Open `http://127.0.0.1:3002/` and set:
- `Gateway URL`: `http://127.0.0.1:8080`
- `Auth token`: `dev-token` (if configured)

## 3) Build + host (static)

```bash
cd abstractcode/web
npm run build
```

Deploy `abstractcode/web/dist/` with any static file server.

If you serve the web app on the **same origin** as the gateway, you can set `Gateway URL` to empty and rely on same-origin `/api/gateway/*` routing.

