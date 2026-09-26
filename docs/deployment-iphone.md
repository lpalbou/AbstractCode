# AbstractCode Web — iPhone Notes (Safari / PWA)

AbstractCode Web is designed to run on iPhone as a **thin host UI** that connects to a **remote** AbstractGateway + AbstractRuntime deployment.

Related:
- Web app overview: [`docs/web.md`](web.md)
- Web deployment: [`docs/deployment-web.md`](deployment-web.md)

## Prerequisites

- The AbstractCode Web application server (`npx @abstractframework/code` or
  `npm start`) reachable over HTTPS, typically behind a reverse proxy with TLS.
  Serve the whole application, not only its static files: the session
  exchange runs in that server. See [web deployment](deployment-web.md).
- An AbstractGateway the application server can reach, with gateway user
  authentication and a user/token for you. The browser talks only to the
  application server, so the gateway needs no CORS entry for it.

## Steps

1) Open the AbstractCode Web URL in Safari.
2) Sign in with your `Gateway user` and that user's `Gateway token`. The
   Gateway URL is the one configured on the application server
   (`ABSTRACTCODE_GATEWAY_URL`).
3) (Optional) Add to Home Screen:
   - Safari → Share → Add to Home Screen

## Notes / constraints

- iOS aggressively suspends background tabs; long-running workflows should be designed to be resumable (ledger replay).
- File access is always remote (through the gateway); the phone does not run local tools. The **Files** tab shows the conversation's workspace on the gateway host.
- The Gateway token is exchanged for an app-scoped browser session and is not
  persisted in browser settings.
- On non-local hosted UI hostnames, the server-configured Gateway URL is
  authoritative. Browser-supplied Gateway URL changes are rejected unless
  `ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1` is enabled behind your
  own access control.
