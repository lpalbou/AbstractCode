import * as http from "node:http";
import * as https from "node:https";
import { existsSync, readFileSync, statSync } from "node:fs";
import { isIP } from "node:net";
import { dirname, extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const BIN_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_DIST_DIR = resolve(BIN_DIR, "..", "dist");
const FALLBACK_GATEWAY_URL = "http://127.0.0.1:8080";
const URL_COOKIE = "abstractcode_gateway_url";
const SESSION_COOKIE = "abstractcode_gateway_session";
const CSRF_COOKIE = "abstractcode_gateway_csrf";
const TRUE_VALUES = new Set(["1", "true", "yes", "y", "on"]);
const HOP_HEADERS = new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "trailers",
  "transfer-encoding",
  "upgrade",
]);
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

function normalizeGatewayUrl(value) {
  return String(value || "")
    .trim()
    .replace(/\/+$/, "");
}

function serverConfig(options = {}) {
  const env = options.env || process.env;
  const defaultGatewayUrl =
    normalizeGatewayUrl(
      options.defaultGatewayUrl ||
        env.ABSTRACTCODE_GATEWAY_URL ||
        env.ABSTRACTGATEWAY_URL ||
        FALLBACK_GATEWAY_URL,
    ) || FALLBACK_GATEWAY_URL;
  return { env, defaultGatewayUrl };
}

function envBool(env, name) {
  const raw = env[name];
  return typeof raw === "string" && TRUE_VALUES.has(raw.trim().toLowerCase());
}

function requestHostname(req, config) {
  const trusted =
    envBool(config.env, "ABSTRACTCODE_TRUST_PROXY_HEADERS") ||
    envBool(config.env, "ABSTRACTGATEWAY_TRUST_PROXY_HEADERS");
  const value = trusted
    ? req.headers["x-forwarded-host"] || req.headers.host
    : req.headers.host;
  const raw = String(value || "")
    .split(",", 1)[0]
    .trim();
  if (!raw) return "";
  if (raw.startsWith("["))
    return raw.slice(1).split("]", 1)[0].trim().toLowerCase();
  if ((raw.match(/:/g) || []).length === 1)
    return raw.split(":")[0].trim().toLowerCase();
  return raw.toLowerCase();
}

function isLoopbackHostname(hostname) {
  const value = String(hostname || "")
    .trim()
    .toLowerCase();
  if (value === "localhost" || value.endsWith(".localhost") || value === "::1") return true;
  return isIP(value) === 4 && Number(value.split(".", 1)[0]) === 127;
}

function isLoopbackPeer(req) {
  let address = String(req.socket?.remoteAddress || "")
    .trim()
    .toLowerCase();
  if (address.startsWith("::ffff:")) address = address.slice("::ffff:".length);
  return (
    address === "::1" || address === "127.0.0.1" || address.startsWith("127.")
  );
}

function peerAddress(req) {
  let address = String(req.socket?.remoteAddress || "").trim().toLowerCase();
  if (address.startsWith("::ffff:")) address = address.slice("::ffff:".length);
  return address;
}

/** X-Forwarded-For for the gateway: the browser's socket address, appended to
 * an existing chain only when this server trusts its reverse proxy. */
export function forwardedForChain(req, config) {
  const trusted =
    envBool(config.env, "ABSTRACTCODE_TRUST_PROXY_HEADERS") ||
    envBool(config.env, "ABSTRACTGATEWAY_TRUST_PROXY_HEADERS");
  const prior = trusted
    ? String(req.headers["x-forwarded-for"] || "")
        .split(",")
        .map((part) => part.trim())
        .filter(Boolean)
    : [];
  const peer = peerAddress(req);
  return [...prior, ...(peer ? [peer] : [])].join(", ");
}

function connectionConfigAllowed(req, config) {
  if (envBool(config.env, "ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG"))
    return true;
  if (
    envBool(config.env, "ABSTRACTGATEWAY_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG")
  )
    return true;
  const trusted =
    envBool(config.env, "ABSTRACTCODE_TRUST_PROXY_HEADERS") ||
    envBool(config.env, "ABSTRACTGATEWAY_TRUST_PROXY_HEADERS");
  // A trusted proxy makes its own socket address meaningless. Hosted
  // deployments must explicitly opt in before a browser can choose an SSRF
  // destination; direct loopback development remains convenient.
  return (
    !trusted &&
    isLoopbackPeer(req) &&
    isLoopbackHostname(requestHostname(req, config))
  );
}

function connectionConfigDenial(req, config) {
  const host = requestHostname(req, config) || "unknown host";
  return (
    `Browser-supplied Gateway URL changes are disabled for this non-local Code host (${host}). ` +
    "Use the server-configured Gateway URL, or set ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1 behind your own access control."
  );
}

function parseCookies(req) {
  const out = {};
  for (const part of String(req.headers.cookie || "").split(";")) {
    const index = part.indexOf("=");
    if (index < 0) continue;
    const key = part.slice(0, index).trim();
    const value = part.slice(index + 1).trim();
    if (!key) continue;
    try {
      out[key] = decodeURIComponent(value);
    } catch {
      out[key] = value;
    }
  }
  return out;
}

function cookieSecure(req, config) {
  if (req.socket?.encrypted) return "; Secure";
  const trusted =
    envBool(config.env, "ABSTRACTCODE_TRUST_PROXY_HEADERS") ||
    envBool(config.env, "ABSTRACTGATEWAY_TRUST_PROXY_HEADERS");
  return trusted && firstHeader(req.headers["x-forwarded-proto"]).toLowerCase() === "https"
    ? "; Secure"
    : "";
}

function setSessionCookies(
  res,
  req,
  gatewayUrl,
  sessionId,
  csrfToken,
  persist,
  config,
) {
  const secure = cookieSecure(req, config);
  const maxAge = persist ? "; Max-Age=2592000" : "";
  const privateAttrs = `; Path=/; HttpOnly; SameSite=Lax${secure}${maxAge}`;
  const csrfAttrs = `; Path=/; SameSite=Lax${secure}${maxAge}`;
  res.setHeader("Set-Cookie", [
    `${URL_COOKIE}=${encodeURIComponent(gatewayUrl)}${privateAttrs}`,
    `${SESSION_COOKIE}=${encodeURIComponent(sessionId)}${privateAttrs}`,
    `${CSRF_COOKIE}=${encodeURIComponent(csrfToken)}${csrfAttrs}`,
  ]);
}

function clearSessionCookies(res, req, config) {
  const secure = cookieSecure(req, config);
  res.setHeader("Set-Cookie", [
    `${URL_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure}`,
    `${SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure}`,
    `${CSRF_COOKIE}=; Path=/; SameSite=Lax; Max-Age=0${secure}`,
  ]);
}

function browserSession(req, config) {
  const cookies = parseCookies(req);
  const cookieUrl = normalizeGatewayUrl(cookies[URL_COOKIE]);
  const allowCookieUrl =
    envBool(config.env, "ABSTRACTCODE_ALLOW_BROWSER_GATEWAY_URL_COOKIE") ||
    envBool(config.env, "ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG") ||
    envBool(
      config.env,
      "ABSTRACTGATEWAY_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG",
    ) ||
    connectionConfigAllowed(req, config);
  return {
    gatewayUrl:
      cookieUrl && (allowCookieUrl || cookieUrl === config.defaultGatewayUrl)
        ? cookieUrl
        : config.defaultGatewayUrl,
    sessionId: String(cookies[SESSION_COOKIE] || "").trim(),
    csrfToken: String(cookies[CSRF_COOKIE] || "").trim(),
  };
}

function resolveBackend(gatewayUrl, config) {
  const url = new URL(
    normalizeGatewayUrl(gatewayUrl) || config.defaultGatewayUrl,
  );
  if (url.protocol !== "http:" && url.protocol !== "https:")
    throw new Error("Gateway URL must use http or https");
  if (!url.port) url.port = url.protocol === "https:" ? "443" : "80";
  return {
    url,
    origin: `${url.protocol}//${url.host}`,
    client: url.protocol === "https:" ? https : http,
  };
}

function mutatingMethod(method) {
  return ["POST", "PUT", "PATCH", "DELETE"].includes(
    String(method || "GET").toUpperCase(),
  );
}

function firstHeader(value) {
  return String(Array.isArray(value) ? value[0] : value || "")
    .split(",", 1)[0]
    .trim();
}

function requestOrigin(req, config) {
  const trusted =
    envBool(config.env, "ABSTRACTCODE_TRUST_PROXY_HEADERS") ||
    envBool(config.env, "ABSTRACTGATEWAY_TRUST_PROXY_HEADERS");
  const host = firstHeader(
    trusted
      ? req.headers["x-forwarded-host"] || req.headers.host
      : req.headers.host,
  );
  const protocol =
    firstHeader(trusted ? req.headers["x-forwarded-proto"] : "") ||
    (req.socket?.encrypted ? "https" : "http");
  if (!host || !["http", "https"].includes(protocol.toLowerCase())) return "";
  try {
    return new URL(`${protocol.toLowerCase()}://${host}`).origin;
  } catch {
    return "";
  }
}

function browserMutationAllowed(req, config) {
  const origin = firstHeader(req.headers.origin);
  if (origin) {
    let normalized;
    try {
      normalized = new URL(origin).origin;
    } catch {
      return false;
    }
    return normalized === requestOrigin(req, config);
  }
  return (
    firstHeader(req.headers["sec-fetch-site"]).toLowerCase() !== "cross-site"
  );
}

function jsonRequest(req) {
  return (
    firstHeader(req.headers["content-type"])
      .split(";", 1)[0]
      .trim()
      .toLowerCase() === "application/json"
  );
}

function sendJson(res, status, payload) {
  if (res.headersSent) {
    res.destroy();
    return;
  }
  res.writeHead(status, { "Content-Type": "application/json; charset=utf-8" });
  res.end(JSON.stringify(payload));
}

function readRequestJson(req) {
  return new Promise((resolveValue) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => {
      try {
        const raw = Buffer.concat(chunks).toString("utf8");
        resolveValue(raw ? JSON.parse(raw) : {});
      } catch {
        resolveValue({});
      }
    });
    req.on("error", () => resolveValue({}));
  });
}

function gatewayRequest(gatewayUrl, options, body, config) {
  return new Promise((resolveValue) => {
    let backend;
    try {
      backend = resolveBackend(gatewayUrl, config);
    } catch (error) {
      resolveValue({
        ok: false,
        status: 0,
        payload: {
          detail: `Invalid gateway URL: ${String(error?.message || error)}`,
        },
      });
      return;
    }
    const request = backend.client.request(
      {
        protocol: backend.url.protocol,
        hostname: backend.url.hostname,
        port: backend.url.port,
        timeout: options.timeout || 4000,
        ...options,
      },
      (response) => {
        const chunks = [];
        response.on("data", (chunk) => chunks.push(chunk));
        response.on("end", () => {
          const raw = Buffer.concat(chunks).toString("utf8");
          let payload;
          try {
            payload = raw ? JSON.parse(raw) : {};
          } catch {
            payload = { detail: raw };
          }
          const status = response.statusCode || 0;
          resolveValue({
            ok: status >= 200 && status < 300,
            status,
            payload,
            headers: response.headers,
            origin: backend.origin,
          });
        });
      },
    );
    request.on("timeout", () => {
      request.destroy();
      resolveValue({
        ok: false,
        status: 0,
        payload: { detail: "Gateway request timed out" },
        origin: backend.origin,
      });
    });
    request.on("error", (error) =>
      resolveValue({
        ok: false,
        status: 0,
        payload: { detail: String(error?.message || error) },
        origin: backend.origin,
      }),
    );
    if (body) request.write(body);
    request.end();
  });
}

function cookieValueFromSetCookie(rawHeaders, name) {
  const headers = Array.isArray(rawHeaders)
    ? rawHeaders
    : rawHeaders
      ? [rawHeaders]
      : [];
  for (const header of headers) {
    for (const candidate of String(header || "").split(/,(?=\s*[^;,=]+=)/)) {
      const first = candidate.split(";", 1)[0];
      const index = first.indexOf("=");
      if (index < 0 || first.slice(0, index).trim() !== name) continue;
      const raw = first.slice(index + 1).trim();
      try {
        return decodeURIComponent(raw);
      } catch {
        return raw;
      }
    }
  }
  return "";
}

async function handleConnectionApi(req, res, config) {
  if (req.method === "GET") {
    const session = browserSession(req, config);
    if (!session.sessionId) {
      sendJson(res, 200, {
        ok: false,
        gateway_url: session.gatewayUrl,
        has_session: false,
        gateway: { ok: false, error: "Gateway sign-in required" },
      });
      return;
    }
    const checked = await gatewayRequest(
      session.gatewayUrl,
      {
        method: "GET",
        path: "/api/gateway/me",
        headers: {
          Accept: "application/json",
          "X-AbstractGateway-Session": session.sessionId,
        },
      },
      undefined,
      config,
    );
    sendJson(res, 200, {
      ok: checked.ok,
      gateway_url: session.gatewayUrl,
      has_session: true,
      gateway: checked.payload,
    });
    return;
  }
  if (req.method === "POST") {
    if (!jsonRequest(req)) {
      sendJson(res, 415, {
        detail:
          "Gateway connection requests require Content-Type: application/json",
      });
      return;
    }
    const payload = await readRequestJson(req);
    const gatewayUrl =
      normalizeGatewayUrl(payload.gateway_url || config.defaultGatewayUrl) ||
      config.defaultGatewayUrl;
    if (
      !connectionConfigAllowed(req, config) &&
      gatewayUrl !== config.defaultGatewayUrl
    ) {
      sendJson(res, 403, { detail: connectionConfigDenial(req, config) });
      return;
    }
    const body = Buffer.from(
      JSON.stringify({
        user_id: String(payload.gateway_user_id || "").trim(),
        token: String(payload.gateway_token || "").trim(),
        remember: payload.persist === true,
      }),
    );
    const login = await gatewayRequest(
      gatewayUrl,
      {
        method: "POST",
        path: "/api/gateway/session/login",
        headers: {
          Accept: "application/json",
          "Content-Type": "application/json",
          "Content-Length": String(body.length),
        },
      },
      body,
      config,
    );
    const session =
      login.payload && typeof login.payload.session === "object"
        ? login.payload.session
        : {};
    const setCookie = login.headers?.["set-cookie"];
    const sessionId =
      cookieValueFromSetCookie(setCookie, "abstractgateway_session") ||
      String(session.session_id || "").trim();
    const csrfToken =
      cookieValueFromSetCookie(setCookie, "abstractgateway_csrf") ||
      String(session.csrf_token || "").trim();
    if (!login.ok || !sessionId || !csrfToken) {
      sendJson(res, login.status || 401, {
        ok: false,
        detail: login.payload?.detail || "Gateway browser session failed",
        gateway: login.payload,
      });
      return;
    }
    setSessionCookies(
      res,
      req,
      gatewayUrl,
      sessionId,
      csrfToken,
      payload.persist === true,
      config,
    );
    sendJson(res, 200, {
      ok: true,
      gateway_url: gatewayUrl,
      has_session: true,
      gateway: login.payload,
    });
    return;
  }
  if (req.method === "DELETE") {
    const session = browserSession(req, config);
    if (session.sessionId) {
      const body = Buffer.from("{}");
      await gatewayRequest(
        session.gatewayUrl,
        {
          method: "POST",
          path: "/api/gateway/session/logout",
          timeout: 2000,
          headers: {
            Accept: "application/json",
            "Content-Type": "application/json",
            "Content-Length": String(body.length),
            "X-AbstractGateway-Session": session.sessionId,
            "X-AbstractGateway-CSRF": session.csrfToken,
          },
        },
        body,
        config,
      );
    }
    clearSessionCookies(res, req, config);
    sendJson(res, 200, { ok: true });
    return;
  }
  sendJson(res, 405, { detail: "Method not allowed" });
}

function proxyHeaders(headers) {
  const out = {};
  for (const [key, value] of Object.entries(headers || {})) {
    const normalized = String(key).toLowerCase();
    if (HOP_HEADERS.has(normalized) || normalized === "content-length")
      continue;
    out[key] = value;
  }
  return out;
}

function proxyGatewayRequest(req, res, config) {
  const session = browserSession(req, config);
  if (!session.sessionId) {
    sendJson(res, 401, { detail: "Gateway sign-in required" });
    return;
  }
  if (mutatingMethod(req.method)) {
    const presented = String(req.headers["x-abstractcode-csrf"] || "").trim();
    if (!session.csrfToken || presented !== session.csrfToken) {
      sendJson(res, 403, {
        detail: "Gateway browser session CSRF token missing or invalid",
        reason_code: "csrf_required",
      });
      return;
    }
  }
  let backend;
  try {
    backend = resolveBackend(session.gatewayUrl, config);
  } catch (error) {
    sendJson(res, 500, {
      detail: `Invalid gateway URL: ${String(error?.message || error)}`,
    });
    return;
  }
  const headers = { ...req.headers, host: backend.url.host };
  delete headers.cookie;
  delete headers.authorization;
  // The gateway decides whether the BROWSER sits on its machine (workspace
  // "Open folder", same-machine defaults). Seen from the gateway, this proxy
  // is the peer, so the browser's address travels as X-Forwarded-For. A
  // chain from the browser side is kept (appended to) only behind a trusted
  // reverse proxy; otherwise it is client-supplied and replaced.
  const forwardedFor = forwardedForChain(req, config);
  delete headers["x-forwarded-for"];
  if (forwardedFor) headers["x-forwarded-for"] = forwardedFor;
  delete headers["x-forwarded-host"];
  delete headers["x-forwarded-proto"];
  delete headers["x-abstractcode-csrf"];
  headers["x-abstractgateway-session"] = session.sessionId;
  if (mutatingMethod(req.method))
    headers["x-abstractgateway-csrf"] = session.csrfToken;
  const proxyRequest = backend.client.request(
    {
      protocol: backend.url.protocol,
      hostname: backend.url.hostname,
      port: backend.url.port,
      method: req.method,
      path: req.url,
      headers,
    },
    (proxyResponse) => {
      res.writeHead(
        proxyResponse.statusCode || 502,
        proxyHeaders(proxyResponse.headers),
      );
      proxyResponse.pipe(res);
    },
  );
  proxyRequest.on("error", (error) =>
    sendJson(res, 502, {
      detail: `Backend not reachable at ${backend.origin} (${String(error?.message || error)})`,
    }),
  );
  req.pipe(proxyRequest);
}

function requestPathname(req) {
  return new URL(req.url || "/", "http://abstractcode.local").pathname;
}

/** Connect-compatible middleware shared by Vite and the packaged server. */
export function createGatewayMiddleware(options = {}) {
  const config = serverConfig(options);
  return function codeGatewayMiddleware(req, res, next) {
    let pathname;
    try {
      pathname = requestPathname(req);
    } catch {
      sendJson(res, 400, { detail: "Invalid request URL" });
      return;
    }
    if (pathname === "/api/connection/gateway") {
      if (mutatingMethod(req.method) && !browserMutationAllowed(req, config)) {
        sendJson(res, 403, {
          detail: "Cross-origin browser requests are not allowed",
          reason_code: "origin_required",
        });
        return;
      }
      void handleConnectionApi(req, res, config).catch((error) => {
        sendJson(res, 500, {
          detail: `Gateway connection request failed (${String(error?.message || error)})`,
        });
      });
      return;
    }
    if (pathname === "/api/gateway" || pathname.startsWith("/api/gateway/")) {
      if (mutatingMethod(req.method) && !browserMutationAllowed(req, config)) {
        sendJson(res, 403, {
          detail: "Cross-origin browser requests are not allowed",
          reason_code: "origin_required",
        });
        return;
      }
      proxyGatewayRequest(req, res, config);
      return;
    }
    if (typeof next === "function") next();
    else sendJson(res, 404, { detail: "Not found" });
  };
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

function serveFile(res, filePath) {
  try {
    if (!existsSync(filePath) || !statSync(filePath).isFile()) return false;
    res.writeHead(200, {
      "Content-Type":
        MIME_TYPES[extname(filePath).toLowerCase()] ||
        "application/octet-stream",
      "Cache-Control": "no-cache",
    });
    res.end(readFileSync(filePath));
    return true;
  } catch {
    return false;
  }
}

export function createCodeRequestHandler(options = {}) {
  const distDir = resolve(options.distDir || DEFAULT_DIST_DIR);
  const gatewayMiddleware = createGatewayMiddleware(options);
  return function codeRequestHandler(req, res) {
    gatewayMiddleware(req, res, () => {
      let pathname;
      try {
        pathname = decodeURIComponent(requestPathname(req));
      } catch {
        res.writeHead(400);
        res.end("Bad Request");
        return;
      }
      if (
        pathname === "/api" ||
        pathname.startsWith("/api/") ||
        unsafeStaticPath(req)
      ) {
        if (pathname === "/api" || pathname.startsWith("/api/"))
          sendJson(res, 404, { detail: "Not found" });
        else {
          res.writeHead(400);
          res.end("Bad Request");
        }
        return;
      }
      const filePath = resolve(distDir, pathname.replace(/^\/+/, ""));
      if (filePath !== distDir && !filePath.startsWith(`${distDir}${sep}`)) {
        res.writeHead(400);
        res.end("Bad Request");
        return;
      }
      if (serveFile(res, filePath)) return;
      if (serveFile(res, `${filePath}.html`)) return;
      if (serveFile(res, join(filePath, "index.html"))) return;
      if (serveFile(res, join(distDir, "index.html"))) return;
      res.writeHead(404);
      res.end("Not Found");
    });
  };
}

export function createCodeServer(options = {}) {
  return http.createServer(createCodeRequestHandler(options));
}
