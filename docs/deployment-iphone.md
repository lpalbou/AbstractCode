# AbstractCode Web — iPhone Notes (Safari / PWA)

AbstractCode Web is designed to run on iPhone as a **thin host UI** that connects to a **remote** AbstractGateway + AbstractRuntime deployment.

Related:
- Web app overview: [`docs/web.md`](web.md)
- Web deployment: [`docs/deployment-web.md`](deployment-web.md)

## Prerequisites

- A reachable HTTPS endpoint for AbstractGateway (recommended: reverse proxy + TLS).
- The AbstractCode Web static site hosted over HTTPS.
- Gateway configured with:
  - Gateway user auth for hosted access
  - `ABSTRACTGATEWAY_ALLOWED_ORIGINS` including your web host origin (exact host recommended for prod).

## Steps

1) Open the AbstractCode Web URL in Safari.
2) Go to `Settings`:
   - set `Gateway URL` to your gateway URL (e.g. `https://gateway.example.com`)
   - set `Gateway user` and that user's `Gateway token`
3) (Optional) Add to Home Screen:
   - Safari → Share → Add to Home Screen

## Notes / constraints

- iOS aggressively suspends background tabs; long-running workflows should be designed to be resumable (ledger replay).
- File access is always remote (via gateway); the phone does not run local tools in v1.
- The Gateway token is exchanged for an app-scoped browser session and is not
  persisted in browser settings.
- On non-local hosted UI hostnames, the server-configured Gateway URL is
  authoritative. Browser-supplied Gateway URL changes are rejected unless
  `ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1` is enabled behind your
  own access control.
