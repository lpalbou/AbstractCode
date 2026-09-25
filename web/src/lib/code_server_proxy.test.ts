import http, { type IncomingHttpHeaders, type Server } from "node:http";
import { afterEach, describe, expect, it } from "vitest";

import { createCodeServer, createGatewayMiddleware } from "../../bin/server.js";

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

  it("overwrites X-Forwarded-For with the browser's socket address, never passing a client value", async () => {
    const received: IncomingHttpHeaders[] = [];
    const gatewayPort = await listen(http.createServer((req, res) => {
      req.resume();
      if (req.url === "/api/gateway/session/login") {
        res.writeHead(200, {
          "Content-Type": "application/json",
          "Set-Cookie": ["abstractgateway_session=gs; Path=/; HttpOnly", "abstractgateway_csrf=gc; Path=/"],
        });
        res.end(JSON.stringify({ session: {} }));
        return;
      }
      received.push(req.headers);
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end("{}");
    }));
    const login = async (codePort: number) => cookieHeader((await request(codePort, "POST", "/api/connection/gateway", {
      body: { gateway_user_id: "alice", gateway_token: "secret" },
    })).headers["set-cookie"]);

    const direct = await listen(createCodeServer({ defaultGatewayUrl: `http://127.0.0.1:${gatewayPort}` }));
    const cookies = await login(direct);
    await request(direct, "GET", "/api/gateway/runs/r1/workspace", {
      headers: { Cookie: cookies, "X-Forwarded-For": "203.0.113.9" },
    });
    // Not behind a trusted proxy: the spoofable header is replaced by the peer.
    expect(received[0]["x-forwarded-for"]).toBe("127.0.0.1");

    const proxied = await listen(createCodeServer({
      defaultGatewayUrl: `http://127.0.0.1:${gatewayPort}`,
      env: { ABSTRACTCODE_TRUST_PROXY_HEADERS: "1" },
    }));
    const proxiedCookies = await login(proxied);
    await request(proxied, "GET", "/api/gateway/runs/r1/workspace", {
      headers: { Cookie: proxiedCookies, "X-Forwarded-For": "198.51.100.7" },
    });
    // Even behind a trusted reverse proxy: overwritten, never appended to.
    expect(received[1]["x-forwarded-for"]).toBe("127.0.0.1");
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
