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
- set `Auth token` if your gateway requires it
