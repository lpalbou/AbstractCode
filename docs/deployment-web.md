# AbstractCode Web deployment

The browser uses a same-origin application server, which authenticates to AbstractGateway and proxies its API. There are no models or tools in the web server. See [web features](web.md) and [architecture](architecture.md).

## Served by the gateway at `/apps/code/`

AbstractGateway (0.7.0 or later) can install, start and serve AbstractCode itself: the console's **Apps** page opens it at `http(s)://<gateway>/apps/code/`, one port and one address for the console, the API and every app. The gateway starts the web server on `127.0.0.1` and relays to it; **Open** signs the browser in to AbstractCode as the signed-in gateway user, so there is no second sign-in. This is the simplest deployment, and the only address you need to expose (or tunnel) for a remote machine.

Under the mount, AbstractCode follows the shared app-server contract: every response carries `X-AbstractFramework-App: code; mount=1`, the page is served with `<base href="/apps/code/">`, every asset and API call is relative to that base, the session cookies are scoped to `Path=/apps/code/`, and the browser's real address (forwarded by the gateway) is what the gateway sees for AbstractCode's calls.

## Run the web server yourself

Configure gateway user authentication and provision a user/token through your gateway's administration process. Then:

```bash
abstractgateway serve --host 127.0.0.1 --port 8080
```

In another terminal:

```bash
npx @abstractframework/code --gateway-url http://127.0.0.1:8080
```

Open `http://127.0.0.1:3002` and sign in with the gateway user and token. The server exchanges these for an app-scoped browser session. The raw token is not saved in local storage. API writes require the session's CSRF token.

| Flag | Default |
|---|---|
| `--gateway-url <url>` (also `--gateway`, `--url`) | the local gateway pointer `~/.abstractframework/gateway.json` written by `abstractgateway serve` and the installer, else `http://127.0.0.1:8080` |
| `--port <n>` | `3002` |
| `--host <addr>` | `127.0.0.1` (use `--host 0.0.0.0` to accept other machines directly) |
| `--help` | prints the flags |

`PORT`, `HOST` and `ABSTRACTCODE_GATEWAY_URL` / `ABSTRACTGATEWAY_URL` still work as legacy aliases, below the flags. Without a flag or environment variable, the server follows the gateway pointer, also when the gateway later moves to another port.

For a source checkout:

```bash
cd web
npm ci
npm run build
npm start -- --gateway-url http://127.0.0.1:8080
```

Development uses `npm run dev` with the same session flow.

## Reverse proxies and HTTPS

Put the packaged server behind your HTTPS reverse proxy. Forward the whole application, including `/api/connection/gateway` and `/api/gateway/*`; a static file host or direct gateway API proxy alone does not implement the session exchange. Allow long-lived streaming responses and disable buffering for ledger SSE.

Forwarded headers (`X-Forwarded-For`, `-Host`, `-Proto`, `-Prefix`) are believed only from a loopback peer, such as the gateway's `/apps/code/` relay or a reverse proxy on the same machine; from any other peer they are ignored. The server sets `X-Forwarded-For` on every gateway request to the browser's address (overwriting any browser-supplied value) and marks each gateway request with `X-AbstractFramework-App-Proxy: abstractcode`, so the gateway can tell whether a browser sits on its own machine before offering **Open folder**. A connection whose address cannot be determined is refused with HTTP 400. Pin the gateway with `--gateway-url` on the server. Browser-to-gateway CORS access is not needed.

Set `ABSTRACTCODE_TRUST_PROXY_HEADERS=1` when your own reverse proxy sits directly in front of AbstractCode (not in front of the gateway): browser-supplied gateway URL changes are then refused, and a sign-in may arrive under the proxy's public host name.

Browser-supplied destination changes are limited to browsers on this machine (a loopback address), and a sign-in that arrives over a loopback connection must name a loopback host (`127.0.0.1`, `localhost`), unless `ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1` explicitly enables remote configuration. With trusted proxy headers enabled, the server destination remains authoritative unless that opt-in is set. Do not enable remote reconfiguration on a publicly accessible app without your own access controls. Cross-origin mutation attempts are rejected before gateway egress.

## Gateway capabilities and storage

Discovery determines workflows, tools, skills, providers, workspace policy, and optional speech features. The gateway retains runs, history, files, and artifacts. The browser stores appearance and non-secret preferences. An offline PWA shell can display the application, but executing or restoring work requires a reachable authenticated gateway.

Speech recording requires HTTPS or localhost and microphone permission. See [iPhone notes](deployment-iphone.md) for platform-specific constraints.
