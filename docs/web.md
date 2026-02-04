# AbstractCode Web (gateway-first)

The `web/` folder contains a **gateway-first** host UI:
- talks only to `abstractgateway` under `/api/gateway/*`
- renders runs by replaying/streaming the **durable ledger**
- resumes waits by submitting durable commands

Start here: [`docs/getting-started.md`](getting-started.md).

Related:
- Deployment: [`docs/deployment-web.md`](deployment-web.md)
- iPhone notes: [`docs/deployment-iphone.md`](deployment-iphone.md)
- Architecture: [`docs/architecture.md`](architecture.md)

## Status (important for external users)

The web app currently consumes shared UI components via Vite path aliases to a sibling `abstractuic/` repo:
- configured in `web/vite.config.ts`
- imports used in `web/src/ui/app.tsx` and `web/src/main.tsx`

This repo includes a prebuilt `web/dist/` for convenience. If you need to modify/rebuild the web app, see the next section.

## Local development (requires the sibling UI repo)

```bash
cd web
npm install
npm run dev
```

Dev server:
- runs on `http://127.0.0.1:3002/` (see `web/package.json`)
- proxies `/api/*` to `http://localhost:8081` by default (see `web/vite.config.ts`)

## Build

```bash
cd web
npm run build
```

Output: `web/dist/` (static files).
