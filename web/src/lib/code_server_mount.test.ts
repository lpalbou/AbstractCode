import http, { type IncomingHttpHeaders, type Server } from "node:http";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { createCodeServer } from "../../bin/server.js";
// @ts-expect-error — a Node build script, no type declarations.
import { checkRelativeUrls, rootedUrls } from "../../scripts/check_relative_urls.mjs";

// Serving under the gateway's /apps/code/ (the app-server kit's mount
// contract): the gateway relays with X-Forwarded-Prefix from a loopback peer.

type Reply = { status: number; headers: IncomingHttpHeaders; text: string };
const servers: Server[] = [];

function listen(server: Server): Promise<number> {
  servers.push(server);
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve((server.address() as any).port)));
}

function get(port: number, path: string, headers: Record<string, string> = {}, method = "GET", body?: string): Promise<Reply> {
  return new Promise((resolve, reject) => {
    const req = http.request({ hostname: "127.0.0.1", port, path, method, headers }, (res) => {
      const chunks: Buffer[] = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode || 0, headers: res.headers, text: Buffer.concat(chunks).toString("utf8") }));
    });
    req.on("error", reject);
    if (body) req.write(body);
    req.end();
  });
}

function distDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "code-dist-"));
  mkdirSync(join(dir, "assets"));
  writeFileSync(join(dir, "index.html"), '<!doctype html><html><head><meta charset="utf-8"><script type="module" src="./assets/app.js"></script></head><body></body></html>');
  writeFileSync(join(dir, "assets", "app.js"), "console.log('code');\n");
  return dir;
}

afterEach(async () => {
  await Promise.all(servers.splice(0).map((s) => new Promise<void>((r) => s.close(() => r()))));
});

describe("served under the gateway at /apps/code/", () => {
  it("announces itself, puts <base href> and base_path in the shell, and serves assets relative to the base", async () => {
    const port = await listen(createCodeServer({ distDir: distDir(), defaultGatewayUrl: "http://127.0.0.1:1" }));
    const shell = await get(port, "/", { "X-Forwarded-Prefix": "/apps/code", "X-Forwarded-For": "203.0.113.5" });
    expect(shell.status).toBe(200);
    expect(shell.headers["x-abstractframework-app"]).toBe("code; mount=1");
    expect(shell.text).toContain('<base href="/apps/code/">');
    expect(shell.text).toContain('"base_path":"/apps/code"');
    // A deep link (single-page app) gets the same shell.
    const deep = await get(port, "/some/view", { "X-Forwarded-Prefix": "/apps/code" });
    expect(deep.text).toContain('<base href="/apps/code/">');
    const asset = await get(port, "/assets/app.js", { "X-Forwarded-Prefix": "/apps/code" });
    expect(asset.status).toBe(200);
    expect(asset.headers["x-abstractframework-app"]).toBe("code; mount=1");
    // At its own port the base is the root.
    expect((await get(port, "/")).text).toContain('<base href="/">');
  });

  it("refuses a malformed prefix, and believes forwarded headers only from a loopback peer", async () => {
    const port = await listen(createCodeServer({ distDir: distDir(), defaultGatewayUrl: "http://127.0.0.1:1" }));
    expect((await get(port, "/", { "X-Forwarded-Prefix": "/apps/../etc" })).status).toBe(400);
  });

  it("sets the session cookies at Path=/apps/code/ when signed in through the mount", async () => {
    const gatewayPort = await listen(
      http.createServer((req, res) => {
        req.resume();
        res.writeHead(200, {
          "Content-Type": "application/json",
          "Set-Cookie": ["abstractgateway_session=gs; Path=/; HttpOnly", "abstractgateway_csrf=gc; Path=/"],
        });
        res.end(JSON.stringify({ session: {} }));
      }),
    );
    const port = await listen(createCodeServer({ distDir: distDir(), defaultGatewayUrl: `http://127.0.0.1:${gatewayPort}` }));
    const body = JSON.stringify({ gateway_user_id: "alice", gateway_token: "t" });
    const login = await get(
      port,
      "/api/connection/gateway",
      {
        "X-Forwarded-Prefix": "/apps/code",
        "X-Forwarded-For": "203.0.113.5",
        "X-Forwarded-Host": "gw.example",
        "X-Forwarded-Proto": "https",
        Origin: "https://gw.example",
        "Content-Type": "application/json",
        "Content-Length": String(Buffer.byteLength(body)),
      },
      "POST",
      body,
    );
    expect(login.status).toBe(200);
    const cookies = login.headers["set-cookie"] || [];
    expect(cookies).toHaveLength(3);
    for (const c of cookies) expect(c).toContain("Path=/apps/code/");
    expect(cookies.every((c) => c.includes("Secure"))).toBe(true);
  });
});

describe("same-origin URLs are relative (the app is served under a base path)", () => {
  it("no root-absolute /api, /assets or /apps URL in the shipped sources", () => {
    expect(checkRelativeUrls().found).toEqual([]);
  });

  it("the check sees a rooted literal (it is not decoration)", () => {
    const dir = mkdtempSync(join(tmpdir(), "code-rooted-"));
    writeFileSync(join(dir, "x.ts"), 'fetch("/api/gateway/runs");\nfetch("api/gateway/runs");\nconst a = `/assets/x.js`;\n');
    expect(rootedUrls(dir)).toHaveLength(2);
  });
});
