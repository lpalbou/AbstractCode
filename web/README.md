# AbstractCode Web (Gateway-First)

This is the **web version of AbstractCode** (the Python CLI), designed as a **gateway-first** host UI:
- talks only to `abstractgateway` (`/api/gateway/*`)
- renders by replaying/streaming the ledger
- resumes waits by submitting durable commands

Important: the existing workflow-run testbed remains at `abstractcode/web/thin_client/` and is **not** modified by this app.

## Local dev
```bash
cd abstractcode/web
npm install
npm run dev
```

In the UI:
- set `Gateway URL` (e.g. `http://127.0.0.1:8080`)
- set `Auth token` if your gateway requires it

