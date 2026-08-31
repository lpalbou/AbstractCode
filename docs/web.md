# AbstractCode Web (gateway-first)

The `web/` folder contains a **gateway-first** host UI:
- talks only to **AbstractGateway** under `/api/gateway/*`
- renders runs by replaying/streaming the **durable ledger**
- resumes waits by submitting durable commands

Start here: [`docs/getting-started.md`](getting-started.md).

Related:
- Deployment: [`docs/deployment-web.md`](deployment-web.md)
- iPhone notes: [`docs/deployment-iphone.md`](deployment-iphone.md)
- Architecture: [`docs/architecture.md`](architecture.md)

## Shared components

The interface is built from the shared AbstractUIC component packages —
`@abstractframework/ui-kit`, `panel-chat`, `monitor-flow` and `monitor-gpu` —
installed from npm like any other dependency and declared in
`web/package.json`. Building requires nothing but this directory.

## Voice (optional)

AbstractCode Web supports **push-to-talk transcription** and **TTS playback** when the connected AbstractGateway exposes the endpoints the web client calls.

Push-to-talk (record → upload → transcribe):
- Upload: `POST /api/gateway/attachments/upload`
- Transcribe: `POST /api/gateway/runs/{run_id}/audio/transcribe`

Text-to-speech:
- TTS: `POST /api/gateway/runs/{run_id}/voice/tts`


## Local development

```bash
cd web
npm ci
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
