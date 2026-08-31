import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import http from "node:http";
import { afterEach, describe, expect, it } from "vitest";

let child: ChildProcessWithoutNullStreams | undefined;

function free_port(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = http.createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : 0;
      server.close(() => resolve(port));
    });
  });
}

function request_json(
  port: number,
  method: string,
  path: string,
  payload: Record<string, unknown> | undefined,
  host_header: string,
  extra_headers: Record<string, string> = {},
): Promise<{ status: number; body: any }> {
  return new Promise((resolve, reject) => {
    const raw = payload ? Buffer.from(JSON.stringify(payload)) : Buffer.alloc(0);
    const req = http.request(
      {
        hostname: "127.0.0.1",
        port,
        method,
        path,
        headers: {
          Host: host_header,
          Accept: "application/json",
          "Content-Type": "application/json",
          "Content-Length": String(raw.length),
          ...extra_headers,
        },
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on("data", (chunk) => chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)));
        res.on("end", () => {
          const text = Buffer.concat(chunks).toString("utf8");
          resolve({ status: res.statusCode || 0, body: text ? JSON.parse(text) : {} });
        });
      },
    );
    req.on("error", reject);
    if (raw.length) req.write(raw);
    req.end();
  });
}

async function wait_ready(port: number): Promise<void> {
  const deadline = Date.now() + 5000;
  let last_error: unknown;
  while (Date.now() < deadline) {
    try {
      const response = await request_json(port, "GET", "/api/connection/gateway", undefined, "127.0.0.1");
      if (response.status === 200) return;
    } catch (error) {
      last_error = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw last_error || new Error("AbstractCode test server did not become ready");
}

async function start_server(port: number, env: Record<string, string> = {}): Promise<void> {
  child = spawn(process.execPath, ["bin/cli.js"], {
    cwd: new URL("../..", import.meta.url),
    env: {
      ...process.env,
      HOST: "127.0.0.1",
      PORT: String(port),
      ABSTRACTCODE_GATEWAY_URL: "http://127.0.0.1:65534",
      ...env,
    },
  });
  await wait_ready(port);
}

// A browser-supplied Gateway URL change, sent with a Host header that CLAIMS a
// remote hosted deployment. The TCP peer is always 127.0.0.1 here — that is
// the whole point: these tests pin which of the two the guard believes.
function post_gateway_url_change(port: number) {
  return request_json(
    port,
    "POST",
    "/api/connection/gateway",
    { gateway_url: "http://evil.example", gateway_user_id: "alice", gateway_token: "secret" },
    "code.abstractframework.ai",
    { "X-Forwarded-Host": "127.0.0.1" },
  );
}

const DENIAL = "Browser-supplied Gateway URL changes are disabled";

afterEach(() => {
  child?.kill("SIGTERM");
  child = undefined;
});

// The guard decides local-vs-remote from the unspoofable TCP peer
// (req.socket.remoteAddress), never from the client-controlled Host header.
// The old Host-based check was an SSRF hole: a LAN client sending
// `Host: localhost` at a 0.0.0.0 bind flipped it and unlocked the
// browser-supplied gateway URL, turning this server into a relay that
// forwarded its session cookies wherever the attacker named.
describe("hosted Gateway URL guard", () => {
  it("ignores a spoofed remote Host header when the peer is genuinely loopback", async () => {
    const port = await free_port();
    await start_server(port);

    const response = await post_gateway_url_change(port);

    // Not 403: the Host header no longer decides. A real loopback peer is
    // local no matter what it claims to be, so the request passes the config
    // gate and is stopped later by the ordinary auth requirement.
    expect(response.status).not.toBe(403);
    expect(String(response.body.detail || "")).not.toContain(DENIAL);
  });

  it("refuses config changes behind a trusted proxy, where the peer proves nothing", async () => {
    const port = await free_port();
    // Behind an explicit trusted proxy every peer is the proxy itself, so
    // loopback-by-socket carries no information and the guard must close.
    await start_server(port, { ABSTRACTCODE_TRUST_PROXY_HEADERS: "1" });

    const response = await post_gateway_url_change(port);

    expect(response.status).toBe(403);
    expect(String(response.body.detail || "")).toContain(DENIAL);
  });

  it("honors the explicit remote opt-in even behind a trusted proxy", async () => {
    const port = await free_port();
    await start_server(port, {
      ABSTRACTCODE_TRUST_PROXY_HEADERS: "1",
      ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG: "1",
    });

    const response = await post_gateway_url_change(port);

    // The operator has taken responsibility behind their own access control.
    expect(response.status).not.toBe(403);
    expect(String(response.body.detail || "")).not.toContain(DENIAL);
  });
});
