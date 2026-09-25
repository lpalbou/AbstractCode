# AbstractCode Web deployment

The browser uses a same-origin application server, which authenticates to AbstractGateway and proxies its API. There are no models or tools in the web server. See [web features](web.md) and [architecture](architecture.md).

## Run the gateway and web server

Configure gateway user authentication and provision a user/token through your gateway's administration process. Then:

```bash
abstractgateway serve --host 127.0.0.1 --port 8080
```

In another terminal:

```bash
ABSTRACTCODE_GATEWAY_URL=http://127.0.0.1:8080 HOST=127.0.0.1 npx @abstractframework/code
```

Open `http://127.0.0.1:3002` and sign in with the gateway user and token. The server exchanges these for an app-scoped browser session. The raw token is not saved in local storage. API writes require the session's CSRF token.

For a source checkout:

```bash
cd web
npm ci
npm run build
ABSTRACTCODE_GATEWAY_URL=http://127.0.0.1:8080 HOST=127.0.0.1 npm start
```

`PORT` changes the web listener port. The CLI binds every interface by default; use `HOST=127.0.0.1` for local-only access. Development uses `npm run dev` with the same gateway setting and session flow.

## Reverse proxies and HTTPS

Put the packaged server behind your HTTPS reverse proxy. Forward the whole application, including `/api/connection/gateway` and `/api/gateway/*`; a static file host or direct gateway API proxy alone does not implement the session exchange. Allow long-lived streaming responses and disable buffering for ledger SSE.

Set `ABSTRACTCODE_TRUST_PROXY_HEADERS=1` only when the proxy strips untrusted forwarded headers and sets `X-Forwarded-Host` and `X-Forwarded-Proto` itself. These determine browser-origin validation and secure-cookie behavior. The server always sets `X-Forwarded-For` to the address of the connection it received (it never forwards a browser- or proxy-supplied value), so the gateway can tell whether a browser sits on its own machine before offering **Open folder**. Pin `ABSTRACTCODE_GATEWAY_URL` on the server. Browser-to-gateway CORS access is not needed.

Browser-supplied destination changes are limited to loopback peers using a loopback hostname, unless `ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1` explicitly enables remote configuration. With trusted proxy headers enabled, the server destination remains authoritative unless that opt-in is set. Do not enable remote reconfiguration on a publicly accessible app without your own access controls. Cross-origin mutation attempts are rejected before gateway egress.

## Gateway capabilities and storage

Discovery determines workflows, tools, skills, providers, workspace policy, and optional speech features. The gateway retains runs, history, files, and artifacts. The browser stores appearance and non-secret preferences. An offline PWA shell can display the application, but executing or restoring work requires a reachable authenticated gateway.

Speech recording requires HTTPS or localhost and microphone permission. See [iPhone notes](deployment-iphone.md) for platform-specific constraints.
