# AbstractCode Web — iPhone Notes (Safari / PWA)

AbstractCode Web is designed to run on iPhone as a **thin host UI** that connects to a **remote** AbstractGateway + AbstractRuntime deployment.

## Prerequisites

- A reachable HTTPS endpoint for AbstractGateway (recommended: reverse proxy + TLS).
- The AbstractCode Web static site hosted over HTTPS.
- Gateway configured with:
  - `ABSTRACTGATEWAY_AUTH_TOKEN` (recommended)
  - `ABSTRACTGATEWAY_ALLOWED_ORIGINS` including your web host origin (exact host recommended for prod).

## Steps

1) Open the AbstractCode Web URL in Safari.
2) Go to `Settings`:
   - set `Gateway URL` to your gateway URL (e.g. `https://gateway.example.com`)
   - set `Auth token` if required
3) (Optional) Add to Home Screen:
   - Safari → Share → Add to Home Screen

## Notes / constraints

- iOS aggressively suspends background tabs; long-running workflows should be designed to be resumable (ledger replay).
- File access is always remote (via gateway); the phone does not run local tools in v1.

