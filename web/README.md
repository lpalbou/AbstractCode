# AbstractCode Web (Gateway-First)

This is the **web version of AbstractCode** (the Python CLI), designed as a **gateway-first** host UI:
- talks only to `abstractgateway` (`/api/gateway/*`)
- renders by replaying/streaming the ledger
- resumes waits by submitting durable commands

Important: the gateway-first **observability UI** moved to `abstractobserver/` (published separately). `abstractcode/web/` is now reserved for the AbstractCode web app.

## Local dev
```bash
cd abstractcode/web
npm install
npm run dev
```

In the UI:
- set `Gateway URL` (blank = same origin / dev proxy; e.g. `http://127.0.0.1:8081`)
- set `Auth token` if your gateway requires it
