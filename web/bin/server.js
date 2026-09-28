/**
 * The AbstractCode web server: the built app (`dist/`) and the app-origin
 * gateway session proxy, on the shared app-server kit
 * (`@abstractframework/app-server`).
 *
 * - Serves under the gateway's `/apps/code/` as well as at its own port
 *   (`createMountedHandler`: base path from `X-Forwarded-Prefix`, the
 *   browser's address from a loopback peer's `X-Forwarded-For`, the identity
 *   header `X-AbstractFramework-App: code; mount=1` on every response). The
 *   shell gets `<base href>` and `base_path` (`injectShell`), so every asset
 *   and API call resolves under the base.
 * - `/api/connection/gateway` and `/api/gateway/*` go through the kit's
 *   session proxy (HttpOnly session cookie + CSRF twin at `Path=<base>/`,
 *   browser `Authorization` and cookies stripped, `X-Forwarded-For` = the
 *   browser, server-pinned gateway URL).
 * - What this app adds in front of the proxy: a browser mutation must come
 *   from this app's own origin, a sign-in must be JSON, and a sign-in over a
 *   loopback socket must name a loopback host (DNS-rebinding guard).
 */
import * as http from "node:http";
import { existsSync, readFileSync, statSync } from "node:fs";
import { isIP } from "node:net";
import { dirname, extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import {
  createGatewaySessionProxy,
  createMountedHandler,
  injectShell,
  requestContext,
  socketPeerAddress,
} from "@abstractframework/app-server";

export { socketPeerAddress };

const BIN_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_DIST_DIR = resolve(BIN_DIR, "..", "dist");
/** The gateway's catalog id: `/apps/code/` and the identity header. */
export const APP_ID = "code";
/** Cookie / CSRF-header prefix (`abstractcode_gateway_*`, `x-abstractcode-csrf`). */
export const COOKIE_APP_ID = "abstractcode";
const CONNECTION_PATH = "/api/connection/gateway";
const GATEWAY_PREFIX = "/api/gateway/";
const MIME_TYPES = {
  ".html": "text/html",
  ".js": "application/javascript",
  ".css": "text/css",
  ".json": "application/json",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
  ".webmanifest": "application/manifest+json",
};

function sendJson(res, status, payload) {
  if (res.headersSent) {
    res.destroy();
    return;
  }
  res.writeHead(status, { "Content-Type": "application/json; charset=utf-8" });
  res.end(JSON.stringify(payload));
}

function firstHeader(value) {
  return String(Array.isArray(value) ? value[0] : value || "")
    .split(",", 1)[0]
    .trim();
}

function mutatingMethod(method) {
  return ["POST", "PUT", "PATCH", "DELETE"].includes(String(method || "GET").toUpperCase());
}

function hostnameOf(host) {
  const raw = String(host || "").trim().toLowerCase();
  if (raw.startsWith("[")) return raw.slice(1).split("]", 1)[0];
  return (raw.match(/:/g) || []).length === 1 ? raw.split(":")[0] : raw;
}

/** `localhost`, `*.localhost`, `::1` or a 127.x IP LITERAL (never a DNS name starting "127."). */
function isLoopbackHostname(host) {
  const h = hostnameOf(host);
  if (h === "localhost" || h.endsWith(".localhost") || h === "::1") return true;
  return isIP(h) === 4 && h.startsWith("127.");
}

/** A browser mutation must come from the origin the browser sees this app at. */
function browserMutationAllowed(req, ctx) {
  const origin = firstHeader(req.headers.origin);
  if (origin) {
    try {
      return new URL(origin).origin === new URL(`${ctx.proto}://${ctx.host}`).origin;
    } catch {
      return false;
    }
  }
  return firstHeader(req.headers["sec-fetch-site"]).toLowerCase() !== "cross-site";
}

function jsonRequest(req) {
  return firstHeader(req.headers["content-type"]).split(";", 1)[0].trim().toLowerCase() === "application/json";
}

function isLoopbackPeerSocket(req) {
  const peer = socketPeerAddress(req);
  return peer === "::1" || peer.startsWith("127.");
}

/** The operator declared a reverse proxy in front of this app (legacy env switches, read by the kit too). */
function trustProxyDeclared() {
  return ["ABSTRACTCODE_TRUST_PROXY_HEADERS", "ABSTRACTGATEWAY_TRUST_PROXY_HEADERS"].some((name) =>
    ["1", "true", "yes", "y", "on"].includes(String(process.env[name] || "").trim().toLowerCase()),
  );
}

function pathnameOf(req) {
  return new URL(req.url || "/", "http://abstractcode.local").pathname;
}

/**
 * The gateway half: connection API + `/api/gateway/*`, with this app's
 * guards in front of the kit proxy. Connect-compatible (`next` for every
 * other path), so the Vite dev server mounts the SAME code.
 */
export function createGatewayMiddleware(options = {}) {
  const proxy = createGatewaySessionProxy({
    appId: COOKIE_APP_ID,
    ...(options.defaultGatewayUrl ? { defaultGatewayUrl: options.defaultGatewayUrl } : {}),
    connectionPath: CONNECTION_PATH,
    proxyPrefix: GATEWAY_PREFIX,
  });
  function middleware(req, res, next) {
    let pathname;
    try {
      pathname = pathnameOf(req);
    } catch {
      sendJson(res, 400, { detail: "Invalid request URL" });
      return;
    }
    const owned = pathname === CONNECTION_PATH || pathname.startsWith(GATEWAY_PREFIX);
    if (!owned) {
      if (typeof next === "function") next();
      else sendJson(res, 404, { detail: "Not found" });
      return;
    }
    let ctx;
    try {
      ctx = requestContext(req);
    } catch (error) {
      sendJson(res, Number(error?.status) || 400, { detail: String(error?.message || error) });
      return;
    }
    // DNS rebinding: a page at a hostile name resolving to 127.0.0.1 reaches
    // this loopback server with its own Host; as a same-origin script it can
    // also add X-Forwarded-* headers, which a loopback peer is believed for.
    // So a sign-in over a loopback socket must name a loopback host (the
    // gateway's /apps/code/ proxy and a local browser both do), unless the
    // operator declared a reverse proxy in front of this app.
    if (
      pathname === CONNECTION_PATH &&
      mutatingMethod(req.method) &&
      isLoopbackPeerSocket(req) &&
      !isLoopbackHostname(req.headers.host) &&
      !trustProxyDeclared()
    ) {
      sendJson(res, 403, {
        detail: `Sign-in refused: this local app was reached under the host name ${hostnameOf(req.headers.host) || "(none)"}, not a loopback address. Browser-supplied Gateway URL changes are disabled for it.`,
        reason_code: "host_not_allowed",
      });
      return;
    }
    if (mutatingMethod(req.method) && !browserMutationAllowed(req, ctx)) {
      sendJson(res, 403, { detail: "Cross-origin browser requests are not allowed", reason_code: "origin_required" });
      return;
    }
    if (pathname === CONNECTION_PATH && req.method === "POST" && !jsonRequest(req)) {
      sendJson(res, 415, { detail: "Gateway connection requests require Content-Type: application/json" });
      return;
    }
    proxy.handle(req, res, pathname);
  }
  middleware.proxy = proxy;
  return middleware;
}

function unsafeStaticPath(req) {
  const rawPath = String(req.url || "/").split("?", 1)[0];
  let decoded;
  try {
    decoded = decodeURIComponent(rawPath);
  } catch {
    return true;
  }
  return decoded.includes("\0") || decoded.split(/[\\/]+/).includes("..");
}

function isFile(filePath) {
  try {
    return existsSync(filePath) && statSync(filePath).isFile();
  } catch {
    return false;
  }
}

function serveFile(res, filePath) {
  if (!isFile(filePath)) return false;
  res.writeHead(200, {
    "Content-Type": MIME_TYPES[extname(filePath).toLowerCase()] || "application/octet-stream",
    "Cache-Control": "no-cache",
  });
  res.end(readFileSync(filePath));
  return true;
}

/** The app shell with `<base href="<basePath>/">` and `base_path`. */
function serveShell(res, distDir, basePath, gatewayUrl) {
  const index = join(distDir, "index.html");
  if (!isFile(index)) return false;
  const html = injectShell(readFileSync(index, "utf8"), { basePath, config: { gateway_url: gatewayUrl } });
  res.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-cache" });
  res.end(html);
  return true;
}

export function createCodeRequestHandler(options = {}) {
  const distDir = resolve(options.distDir || DEFAULT_DIST_DIR);
  const gatewayMiddleware = createGatewayMiddleware(options);
  return createMountedHandler({ appId: APP_ID }, (req, res, ctx) => {
    gatewayMiddleware(req, res, () => {
      let pathname;
      try {
        pathname = decodeURIComponent(pathnameOf(req));
      } catch {
        res.writeHead(400);
        res.end("Bad Request");
        return;
      }
      if (pathname === "/api" || pathname.startsWith("/api/")) {
        sendJson(res, 404, { detail: "Not found" });
        return;
      }
      if (unsafeStaticPath(req)) {
        res.writeHead(400);
        res.end("Bad Request");
        return;
      }
      const gatewayUrl = gatewayMiddleware.proxy.defaultGatewayUrl;
      if (pathname === "/" || pathname === "/index.html") {
        if (serveShell(res, distDir, ctx.basePath, gatewayUrl)) return;
      }
      const filePath = resolve(distDir, pathname.replace(/^\/+/, ""));
      if (filePath !== distDir && !filePath.startsWith(`${distDir}${sep}`)) {
        res.writeHead(400);
        res.end("Bad Request");
        return;
      }
      if (serveFile(res, filePath)) return;
      if (serveFile(res, `${filePath}.html`)) return;
      // Single-page app: any other path is the shell.
      if (serveShell(res, distDir, ctx.basePath, gatewayUrl)) return;
      res.writeHead(404);
      res.end("Not Found");
    });
  });
}

export function createCodeServer(options = {}) {
  return http.createServer(createCodeRequestHandler(options));
}
