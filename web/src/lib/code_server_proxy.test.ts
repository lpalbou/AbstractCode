import http, { type IncomingHttpHeaders, type Server } from "node:http";
import { afterEach, describe, expect, it } from "vitest";

import { createCodeRequestHandler, createCodeServer, createGatewayMiddleware } from "../../bin/server.js";

type Response = { status: number; headers: IncomingHttpHeaders; body: any };
const servers: Server[] = [];

function listen(server: Server): Promise<number> {
  servers.push(server);
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      resolve(typeof address === "object" && address ? address.port : 0);
    });
  });
}

function request(port: number, method: string, path: string, options: {
  headers?: Record<string, string>;
  body?: Record<string, unknown>;
} = {}): Promise<Response> {
  return new Promise((resolve, reject) => {
    const raw = options.body ? Buffer.from(JSON.stringify(options.body)) : Buffer.alloc(0);
    const outgoing = http.request({
      hostname: "127.0.0.1", port, method, path,
      headers: {
        Accept: "application/json",
        ...(raw.length ? { "Content-Type": "application/json", "Content-Length": String(raw.length) } : {}),
        ...options.headers,
      },
    }, (incoming) => {
      const chunks: Buffer[] = [];
      incoming.on("data", (chunk) => chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)));
      incoming.on("end", () => {
        const text = Buffer.concat(chunks).toString("utf8");
        let body: any = text;
        try { body = text ? JSON.parse(text) : undefined; } catch { /* text response */ }
        resolve({ status: incoming.statusCode || 0, headers: incoming.headers, body });
      });
    });
    outgoing.on("error", reject);
    if (raw.length) outgoing.write(raw);
    outgoing.end();
  });
}

function cookieHeader(setCookie: string[] | undefined): string {
  return (setCookie || []).map((value) => value.split(";", 1)[0]).join("; ");
}

afterEach(async () => {
  await Promise.all(servers.splice(0).map((server) => new Promise<void>((resolve) => server.close(() => resolve()))));
});

describe("shared Code Gateway middleware", () => {
  it("rejects cross-origin connection changes before gateway egress", async () => {
    let gatewayRequests = 0;
    const gatewayPort = await listen(http.createServer((req, res) => {
      gatewayRequests += 1;
      req.resume();
      res.writeHead(500, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ detail: "must not be reached" }));
    }));
    const codePort = await listen(createCodeServer({ defaultGatewayUrl: `http://127.0.0.1:${gatewayPort}` }));

    const foreignOrigin = await request(codePort, "POST", "/api/connection/gateway", {
      headers: { Origin: "https://evil.example" },
      body: { gateway_url: `http://127.0.0.1:${gatewayPort}` },
    });
    expect(foreignOrigin).toMatchObject({ status: 403, body: { reason_code: "origin_required" } });

    const fetchMetadata = await request(codePort, "POST", "/api/connection/gateway", {
      headers: { "Sec-Fetch-Site": "cross-site" },
      body: { gateway_url: `http://127.0.0.1:${gatewayPort}` },
    });
    expect(fetchMetadata).toMatchObject({ status: 403, body: { reason_code: "origin_required" } });

    const nonJson = await request(codePort, "POST", "/api/connection/gateway", {
      headers: { Origin: `http://127.0.0.1:${codePort}`, "Content-Type": "text/plain" },
      body: { gateway_url: `http://127.0.0.1:${gatewayPort}` },
    });
    expect(nonJson.status).toBe(415);
    expect(gatewayRequests).toBe(0);
  });

  it("rejects deceptive 127-prefixed hostnames before gateway egress", async () => {
    let gatewayRequests = 0;
    const gatewayPort = await listen(http.createServer((req, res) => {
      gatewayRequests += 1;
      req.resume(); res.writeHead(500); res.end();
    }));
    const codePort = await listen(createCodeServer({ defaultGatewayUrl: "http://127.0.0.1:65534" }));

    const response = await request(codePort, "POST", "/api/connection/gateway", {
      headers: { Host: "127.attacker.example" },
      body: { gateway_url: `http://127.0.0.1:${gatewayPort}`, gateway_user_id: "alice", gateway_token: "secret" },
    });

    expect(response.status).toBe(403);
    expect(gatewayRequests).toBe(0);
  });

  it("uses trusted reverse-proxy origin headers without rejecting a same-origin login", async () => {
    const gatewayPort = await listen(http.createServer((req, res) => {
      req.resume();
      res.writeHead(200, {
        "Content-Type": "application/json",
        "Set-Cookie": [
          "abstractgateway_session=gateway-session; Path=/; HttpOnly",
          "abstractgateway_csrf=gateway-csrf; Path=/",
        ],
      });
      res.end(JSON.stringify({ session: {} }));
    }));
    const codePort = await listen(createCodeServer({
      defaultGatewayUrl: `http://127.0.0.1:${gatewayPort}`,
      env: { ABSTRACTCODE_TRUST_PROXY_HEADERS: "1" },
    }));

    const login = await request(codePort, "POST", "/api/connection/gateway", {
      headers: {
        Host: "internal-proxy:3002",
        Origin: "https://code.example",
        "X-Forwarded-Host": "code.example",
        "X-Forwarded-Proto": "https",
      },
      body: { gateway_user_id: "alice", gateway_token: "secret" },
    });
    expect(login.status).toBe(200);
    expect(login.headers["set-cookie"]).toHaveLength(3);
    expect(login.headers["set-cookie"]?.every((cookie) => cookie.includes("; Secure"))).toBe(true);
  });

  it("exchanges the app session, strips browser authority, and enforces mutation CSRF", async () => {
    const received: IncomingHttpHeaders[] = [];
    const gatewayPort = await listen(http.createServer((req, res) => {
      if (req.url === "/api/gateway/session/login") {
        req.resume();
        res.writeHead(200, {
          "Content-Type": "application/json",
          "Set-Cookie": [
            "abstractgateway_session=gateway-session; Path=/; HttpOnly",
            "abstractgateway_csrf=gateway-csrf; Path=/",
          ],
        });
        res.end(JSON.stringify({ session: {} }));
        return;
      }
      received.push(req.headers);
      req.resume();
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ ok: true }));
    }));
    const codePort = await listen(createCodeServer({ defaultGatewayUrl: `http://127.0.0.1:${gatewayPort}` }));

    const login = await request(codePort, "POST", "/api/connection/gateway", {
      headers: { "X-Forwarded-Proto": "https" },
      body: { gateway_user_id: "alice", gateway_token: "secret" },
    });
    expect(login.status).toBe(200);
    expect(login.headers["set-cookie"]).toHaveLength(3);
    expect(login.headers["set-cookie"]?.every((cookie) => !cookie.includes("; Secure"))).toBe(true);
    const cookies = cookieHeader(login.headers["set-cookie"]);
    expect(cookies).toContain("abstractcode_gateway_session=gateway-session");
    expect(cookies).toContain("abstractcode_gateway_csrf=gateway-csrf");

    const read = await request(codePort, "GET", "/api/gateway/echo?value=1", {
      headers: { Cookie: cookies, Authorization: "Bearer browser-token" },
    });
    expect(read.status).toBe(200);
    expect(received[0]["x-abstractgateway-session"]).toBe("gateway-session");
    expect(received[0].authorization).toBeUndefined();
    expect(received[0].cookie).toBeUndefined();

    const denied = await request(codePort, "POST", "/api/gateway/echo", { headers: { Cookie: cookies } });
    expect(denied).toMatchObject({ status: 403, body: { reason_code: "csrf_required" } });
    expect(received).toHaveLength(1);

    const mutation = await request(codePort, "POST", "/api/gateway/echo", {
      headers: { Cookie: cookies, "X-AbstractCode-CSRF": "gateway-csrf", Authorization: "Bearer browser-token" },
      body: { value: 1 },
    });
    expect(mutation.status).toBe(200);
    expect(received[1]["x-abstractgateway-session"]).toBe("gateway-session");
    expect(received[1]["x-abstractgateway-csrf"]).toBe("gateway-csrf");
    expect(received[1]["x-abstractcode-csrf"]).toBeUndefined();
    expect(received[1].authorization).toBeUndefined();

    expect((await request(codePort, "GET", "/api/unrelated")).status).toBe(404);
    expect(received).toHaveLength(2);
    expect((await request(codePort, "GET", "/%2e%2e/secret")).status).toBe(400);
  });

  it("sets X-Forwarded-For to the socket peer on every gateway-bound path (contract A-2)", async () => {
    // What the stub gateway saw, per path.
    const saw: Record<string, IncomingHttpHeaders> = {};
    const gatewayPort = await listen(http.createServer((req, res) => {
      req.resume();
      const path = String(req.url || "").split("?", 1)[0];
      saw[path] = { ...req.headers, "x-forwarded-for-count": String(req.rawHeaders.filter((h, i) => i % 2 === 0 && h.toLowerCase() === "x-forwarded-for").length) };
      if (path === "/api/gateway/session/login") {
        res.writeHead(200, {
          "Content-Type": "application/json",
          "Set-Cookie": ["abstractgateway_session=gs; Path=/; HttpOnly", "abstractgateway_csrf=gc; Path=/"],
        });
        res.end(JSON.stringify({ session: {} }));
        return;
      }
      if (path.endsWith("/ledger/stream")) {
        res.writeHead(200, { "Content-Type": "text/event-stream" });
        res.end("event: ping\ndata: {}\n\n");
        return;
      }
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end("{}");
    }));
    const handler = createCodeRequestHandler({ defaultGatewayUrl: `http://127.0.0.1:${gatewayPort}` });
    // The app binds loopback, so `x-test-peer` stands in for the transport
    // peer a LAN browser would have (req.socket.remoteAddress).
    const codePort = await listen(http.createServer((req, res) => {
      // Keep-alive reuses sockets: restore the real peer when not spoofing.
      const socket = req.socket as any;
      socket.__realPeer ??= socket.remoteAddress;
      const spoof = req.headers["x-test-peer"];
      Object.defineProperty(socket, "remoteAddress", { value: spoof ? String(spoof) : socket.__realPeer, configurable: true });
      handler(req, res);
    }));

    const login = await request(codePort, "POST", "/api/connection/gateway", {
      headers: { "X-Forwarded-For": "127.0.0.1", "x-test-peer": "192.168.1.51" },
      body: { gateway_user_id: "alice", gateway_token: "secret" },
    });
    expect(login.status).toBe(200);
    expect(saw["/api/gateway/session/login"]["x-forwarded-for"]).toBe("192.168.1.51");
    const cookies = cookieHeader(login.headers["set-cookie"]);

    const probe = await request(codePort, "GET", "/api/connection/gateway", {
      headers: { Cookie: cookies, "X-Forwarded-For": "203.0.113.9" },
    });
    expect(probe.status).toBe(200);
    expect(saw["/api/gateway/me"]["x-forwarded-for"]).toBe("127.0.0.1");

    await request(codePort, "GET", "/api/gateway/runs/r1/workspace", {
      headers: {
        Cookie: cookies,
        "X-Forwarded-For": "203.0.113.9, 198.51.100.4",
        Forwarded: "for=203.0.113.9",
        "X-Real-IP": "203.0.113.9",
      },
    });
    const proxied = saw["/api/gateway/runs/r1/workspace"];
    expect(proxied["x-forwarded-for"]).toBe("127.0.0.1");
    expect(proxied["x-forwarded-for-count"]).toBe("1");
    expect(proxied.forwarded).toBeUndefined();
    expect(proxied["x-real-ip"]).toBeUndefined();

    await request(codePort, "GET", "/api/gateway/runs/r1/ledger/stream", {
      headers: { Cookie: cookies, Accept: "text/event-stream", "X-Forwarded-For": "127.0.0.1", "x-test-peer": "192.168.1.50" },
    });
    expect(saw["/api/gateway/runs/r1/ledger/stream"]["x-forwarded-for"]).toBe("192.168.1.50");

    await request(codePort, "GET", "/api/gateway/v4mapped", { headers: { Cookie: cookies, "x-test-peer": "::ffff:10.0.0.7" } });
    expect(saw["/api/gateway/v4mapped"]["x-forwarded-for"]).toBe("10.0.0.7");
    await request(codePort, "GET", "/api/gateway/v6", { headers: { Cookie: cookies, "x-test-peer": "fe80::1", "X-Forwarded-For": "::1" } });
    expect(saw["/api/gateway/v6"]["x-forwarded-for"]).toBe("fe80::1");

    const logout = await request(codePort, "DELETE", "/api/connection/gateway", {
      headers: { Cookie: cookies, "X-Forwarded-For": "127.0.0.1", "x-test-peer": "192.168.1.52" },
    });
    expect(logout.status).toBe(200);
    expect(saw["/api/gateway/session/logout"]["x-forwarded-for"]).toBe("192.168.1.52");
  });

  it("refuses (400) a request whose socket peer is unknown, on the proxy and the connection API", async () => {
    const middleware = createGatewayMiddleware({ defaultGatewayUrl: "http://127.0.0.1:65534" });
    const run = (req: any) => new Promise<number>((resolve) => {
      const res: any = { headersSent: false, statusCode: 0, writeHead(code: number) { this.statusCode = code; return this; }, setHeader() {}, getHeader() {}, end() { resolve(this.statusCode); }, on() {} };
      middleware(req, res, () => resolve(-1));
    });
    const cookie = "abstractcode_gateway_session=gs; abstractcode_gateway_csrf=gc";
    expect(await run({ method: "GET", url: "/api/gateway/echo", headers: { cookie, "x-forwarded-for": "127.0.0.1" }, socket: {} })).toBe(400);
    expect(await run({ method: "GET", url: "/api/connection/gateway", headers: { cookie }, socket: {} })).toBe(400);
  });

  it("falls through for non-owned paths when mounted in Vite", async () => {
    const middleware = createGatewayMiddleware({ defaultGatewayUrl: "http://127.0.0.1:65534" });
    const port = await listen(http.createServer((req, res) => {
      middleware(req, res, () => {
        res.writeHead(418, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ next: true }));
      });
    }));

    expect(await request(port, "GET", "/api/unrelated")).toMatchObject({ status: 418, body: { next: true } });
    expect((await request(port, "GET", "/api/gateway/me")).status).toBe(401);
  });
});
