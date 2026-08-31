#!/usr/bin/env node

/**
 * CLI entry point for @abstractframework/code
 * Serves the browser-based coding assistant on a configurable port
 */

import * as http from 'http';
import * as https from 'https';
import { readFileSync, existsSync, statSync } from 'fs';
import { join, extname, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const DIST_DIR = join(__dirname, '..', 'dist');
const PORT = process.env.PORT || 3002;
const HOST = process.env.HOST || '0.0.0.0';
const DEFAULT_GATEWAY_URL = String(process.env.ABSTRACTCODE_GATEWAY_URL || process.env.ABSTRACTGATEWAY_URL || 'http://127.0.0.1:8080').trim().replace(/\/+$/, '') || 'http://127.0.0.1:8080';
const GATEWAY_SESSION_URL_COOKIE = 'abstractcode_gateway_url';
const GATEWAY_SESSION_ID_COOKIE = 'abstractcode_gateway_session';
const GATEWAY_SESSION_CSRF_COOKIE = 'abstractcode_gateway_csrf';
const TRUE_VALUES = new Set(['1', 'true', 'yes', 'y', 'on']);

// MIME types for common file extensions
const MIME_TYPES = {
  '.html': 'text/html',
  '.js': 'application/javascript',
  '.css': 'text/css',
  '.json': 'application/json',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.webmanifest': 'application/manifest+json',
};

function getMimeType(filePath) {
  const ext = extname(filePath).toLowerCase();
  return MIME_TYPES[ext] || 'application/octet-stream';
}

function serveFile(res, filePath) {
  try {
    if (!existsSync(filePath) || !statSync(filePath).isFile()) {
      return false;
    }
    const content = readFileSync(filePath);
    const mimeType = getMimeType(filePath);
    res.writeHead(200, {
      'Content-Type': mimeType,
      'Cache-Control': 'no-cache',
    });
    res.end(content);
    return true;
  } catch (err) {
    return false;
  }
}

function sendJson(res, status, payload) {
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(JSON.stringify(payload));
}

function envBool(name) {
  const raw = process.env[name];
  if (typeof raw !== 'string' || !raw.trim()) return false;
  return TRUE_VALUES.has(raw.trim().toLowerCase());
}

function requestHostname(req) {
  const trustProxyHeaders = envBool('ABSTRACTCODE_TRUST_PROXY_HEADERS') || envBool('ABSTRACTGATEWAY_TRUST_PROXY_HEADERS');
  const headerValue = trustProxyHeaders ? (req?.headers?.['x-forwarded-host'] || req?.headers?.host) : req?.headers?.host;
  const raw = String(headerValue || '').split(',', 1)[0].trim();
  if (!raw) return '';
  if (raw.startsWith('[')) return raw.slice(1).split(']', 1)[0].trim().toLowerCase();
  if ((raw.match(/:/g) || []).length === 1) return raw.split(':')[0].trim().toLowerCase();
  return raw.toLowerCase();
}

function isLoopbackHostname(hostname) {
  const h = String(hostname || '').trim().toLowerCase();
  return h === 'localhost' || h === 'localhost.localdomain' || h === '::1' || h.startsWith('127.');
}

// The SECURITY gate must derive local-vs-remote from the unspoofable
// connection PEER, never from the client-controlled `Host` header (entity's
// HIGH SSRF finding, 2026-07-14: a LAN client sending `Host: localhost`
// against a 0.0.0.0 bind flipped the old Host-based check and unlocked the
// browser-supplied gateway-URL cookie → SSRF relay with session cookies).
// req.socket.remoteAddress is the real TCP peer; IPv4-mapped IPv6 loopback
// (::ffff:127.x) is normalized.
function isLoopbackPeer(req) {
  let addr = String(req?.socket?.remoteAddress || '').trim().toLowerCase();
  if (!addr) return false;
  if (addr.startsWith('::ffff:')) addr = addr.slice('::ffff:'.length);
  return addr === '::1' || addr === '127.0.0.1' || addr.startsWith('127.');
}

function browserGatewayConnectionConfigAllowed(req) {
  if (envBool('ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG')) return true;
  if (envBool('ABSTRACTGATEWAY_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG')) return true;
  // Behind an explicit trusted proxy the socket peer is always the proxy, so
  // loopback-by-socket is meaningless — such deployments MUST opt in via the
  // ALLOW_REMOTE env above. Without that opt-in, only a genuinely loopback
  // TCP peer may change the gateway config.
  const trustProxyHeaders =
    envBool('ABSTRACTCODE_TRUST_PROXY_HEADERS') || envBool('ABSTRACTGATEWAY_TRUST_PROXY_HEADERS');
  if (trustProxyHeaders) return false;
  return isLoopbackPeer(req);
}

function browserGatewayConnectionConfigDenial(req) {
  const host = requestHostname(req) || 'unknown host';
  return (
    `Browser-supplied Gateway URL changes are disabled for this non-local Code host (${host}). ` +
    'Use the server-configured Gateway URL, or set ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG=1 behind your own access control.'
  );
}

function parseCookies(req) {
  const out = {};
  for (const part of String(req?.headers?.cookie || '').split(';')) {
    const idx = part.indexOf('=');
    if (idx < 0) continue;
    const key = part.slice(0, idx).trim();
    const value = part.slice(idx + 1).trim();
    if (!key) continue;
    try {
      out[key] = decodeURIComponent(value);
    } catch {
      out[key] = value;
    }
  }
  return out;
}

function cookieSecure(req) {
  return String(req?.headers?.['x-forwarded-proto'] || '').trim().toLowerCase() === 'https' ? '; Secure' : '';
}

function setSessionCookies(res, req, gatewayUrl, sessionId, csrfToken, persist) {
  const secure = cookieSecure(req);
  const maxAge = persist ? '; Max-Age=2592000' : '';
  const attrs = `; Path=/; HttpOnly; SameSite=Lax${secure}${maxAge}`;
  const csrfAttrs = `; Path=/; SameSite=Lax${secure}${maxAge}`;
  res.setHeader('Set-Cookie', [
    `${GATEWAY_SESSION_URL_COOKIE}=${encodeURIComponent(gatewayUrl)}${attrs}`,
    `${GATEWAY_SESSION_ID_COOKIE}=${encodeURIComponent(sessionId)}${attrs}`,
    `${GATEWAY_SESSION_CSRF_COOKIE}=${encodeURIComponent(csrfToken)}${csrfAttrs}`,
  ]);
}

function clearSessionCookies(res, req) {
  const secure = cookieSecure(req);
  res.setHeader('Set-Cookie', [
    `${GATEWAY_SESSION_URL_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure}`,
    `${GATEWAY_SESSION_ID_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure}`,
    `${GATEWAY_SESSION_CSRF_COOKIE}=; Path=/; SameSite=Lax; Max-Age=0${secure}`,
  ]);
}

function browserSession(req) {
  const cookies = parseCookies(req);
  const cookieUrl = String(cookies[GATEWAY_SESSION_URL_COOKIE] || '').trim().replace(/\/+$/, '');
  const allowCookieUrl =
    envBool('ABSTRACTCODE_ALLOW_BROWSER_GATEWAY_URL_COOKIE') ||
    envBool('ABSTRACTCODE_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG') ||
    envBool('ABSTRACTGATEWAY_ALLOW_REMOTE_BROWSER_GATEWAY_CONFIG') ||
    browserGatewayConnectionConfigAllowed(req);
  return {
    gatewayUrl: cookieUrl && (allowCookieUrl || cookieUrl === DEFAULT_GATEWAY_URL) ? cookieUrl : DEFAULT_GATEWAY_URL,
    sessionId: String(cookies[GATEWAY_SESSION_ID_COOKIE] || '').trim(),
    csrfToken: String(cookies[GATEWAY_SESSION_CSRF_COOKIE] || '').trim(),
  };
}

function resolveBackend(gatewayUrl) {
  const backend = new URL(String(gatewayUrl || DEFAULT_GATEWAY_URL).trim());
  if (!backend.port) backend.port = backend.protocol === 'https:' ? '443' : '80';
  return {
    url: backend,
    origin: `${backend.protocol}//${backend.host}`,
    client: backend.protocol === 'https:' ? https : http,
  };
}

function mutatingMethod(method) {
  return ['POST', 'PUT', 'PATCH', 'DELETE'].includes(String(method || 'GET').toUpperCase());
}

function readRequestJson(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on('data', (chunk) => chunks.push(chunk));
    req.on('end', () => {
      try {
        const raw = Buffer.concat(chunks).toString('utf8');
        resolve(raw ? JSON.parse(raw) : {});
      } catch {
        resolve({});
      }
    });
    req.on('error', () => resolve({}));
  });
}

function gatewayRequest(gatewayUrl, options, body) {
  return new Promise((resolve) => {
    let backend;
    try {
      backend = resolveBackend(gatewayUrl);
    } catch (err) {
      resolve({ ok: false, status: 0, payload: { detail: `Invalid gateway URL: ${String(err?.message || err)}` } });
      return;
    }
    const req = backend.client.request(
      {
        protocol: backend.url.protocol,
        hostname: backend.url.hostname,
        port: backend.url.port,
        timeout: options.timeout || 4000,
        ...options,
      },
      (resp) => {
        const chunks = [];
        resp.on('data', (chunk) => chunks.push(chunk));
        resp.on('end', () => {
          const text = Buffer.concat(chunks).toString('utf8');
          let payload = {};
          try {
            payload = text ? JSON.parse(text) : {};
          } catch {
            payload = { detail: text };
          }
          const status = resp.statusCode || 0;
          resolve({ ok: status >= 200 && status < 300, status, payload, headers: resp.headers, origin: backend.origin });
        });
      }
    );
    req.on('timeout', () => {
      req.destroy();
      resolve({ ok: false, status: 0, payload: { detail: 'Gateway request timed out' }, origin: backend?.origin });
    });
    req.on('error', (err) => resolve({ ok: false, status: 0, payload: { detail: String(err?.message || err) }, origin: backend?.origin }));
    if (body) req.write(body);
    req.end();
  });
}

function cookieValueFromSetCookie(rawHeaders, name) {
  const headers = Array.isArray(rawHeaders) ? rawHeaders : rawHeaders ? [rawHeaders] : [];
  for (const header of headers) {
    for (const candidate of String(header || '').split(/,(?=\s*[^;,=]+=)/)) {
      const first = candidate.split(';', 1)[0];
      const idx = first.indexOf('=');
      if (idx < 0) continue;
      if (first.slice(0, idx).trim() !== name) continue;
      const raw = first.slice(idx + 1).trim();
      try {
        return decodeURIComponent(raw);
      } catch {
        return raw;
      }
    }
  }
  return '';
}

async function handleConnectionApi(req, res) {
  if (req.method === 'GET') {
    const session = browserSession(req);
    if (!session.sessionId) {
      sendJson(res, 200, { ok: false, gateway_url: session.gatewayUrl, has_session: false, gateway: { ok: false, error: 'Gateway sign-in required' } });
      return;
    }
    const checked = await gatewayRequest(session.gatewayUrl, {
      method: 'GET',
      path: '/api/gateway/me',
      headers: { Accept: 'application/json', 'X-AbstractGateway-Session': session.sessionId },
    });
    sendJson(res, 200, { ok: checked.ok, gateway_url: session.gatewayUrl, has_session: Boolean(session.sessionId), gateway: checked.payload });
    return;
  }
  if (req.method === 'POST') {
    const payload = await readRequestJson(req);
    const gatewayUrl = String(payload.gateway_url || DEFAULT_GATEWAY_URL).trim().replace(/\/+$/, '') || DEFAULT_GATEWAY_URL;
    if (!browserGatewayConnectionConfigAllowed(req) && gatewayUrl !== DEFAULT_GATEWAY_URL) {
      sendJson(res, 403, { detail: browserGatewayConnectionConfigDenial(req) });
      return;
    }
    const body = Buffer.from(JSON.stringify({
      user_id: String(payload.gateway_user_id || '').trim(),
      token: String(payload.gateway_token || '').trim(),
      remember: payload.persist === true,
    }));
    const login = await gatewayRequest(gatewayUrl, {
      method: 'POST',
      path: '/api/gateway/session/login',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json', 'Content-Length': String(body.length) },
    }, body);
    const session = login.payload && typeof login.payload.session === 'object' ? login.payload.session : {};
    const setCookie = login.headers?.['set-cookie'];
    const sessionId = cookieValueFromSetCookie(setCookie, 'abstractgateway_session') || String(session.session_id || '').trim();
    const csrfToken = cookieValueFromSetCookie(setCookie, 'abstractgateway_csrf') || String(session.csrf_token || '').trim();
    if (!login.ok || !sessionId || !csrfToken) {
      sendJson(res, login.status || 401, { ok: false, detail: login.payload?.detail || 'Gateway browser session failed', gateway: login.payload });
      return;
    }
    setSessionCookies(res, req, gatewayUrl, sessionId, csrfToken, payload.persist === true);
    sendJson(res, 200, { ok: true, gateway_url: gatewayUrl, has_session: true, gateway: login.payload });
    return;
  }
  if (req.method === 'DELETE') {
    const session = browserSession(req);
    if (session.sessionId) {
      const body = Buffer.from('{}');
      await gatewayRequest(session.gatewayUrl, {
        method: 'POST',
        path: '/api/gateway/session/logout',
        headers: {
          Accept: 'application/json',
          'Content-Type': 'application/json',
          'Content-Length': String(body.length),
          'X-AbstractGateway-Session': session.sessionId,
          'X-AbstractGateway-CSRF': session.csrfToken,
        },
        timeout: 2000,
      }, body);
    }
    clearSessionCookies(res, req);
    sendJson(res, 200, { ok: true });
    return;
  }
  sendJson(res, 405, { detail: 'Method not allowed' });
}

const HOP_BY_HOP_HEADERS = new Set(['connection', 'keep-alive', 'proxy-authenticate', 'proxy-authorization', 'te', 'trailer', 'trailers', 'transfer-encoding', 'upgrade']);

function proxyHeaders(headers) {
  const out = {};
  for (const [key, value] of Object.entries(headers || {})) {
    const k = String(key).toLowerCase();
    if (HOP_BY_HOP_HEADERS.has(k) || k === 'content-length') continue;
    out[key] = value;
  }
  return out;
}

function proxyApiRequest(req, res) {
  const session = browserSession(req);
  if (!session.sessionId) {
    sendJson(res, 401, { detail: 'Gateway sign-in required' });
    return;
  }
  if (mutatingMethod(req.method)) {
    const presented = String(req.headers['x-abstractcode-csrf'] || '').trim();
    if (!session.csrfToken || presented !== session.csrfToken) {
      sendJson(res, 403, { detail: 'Gateway browser session CSRF token missing or invalid', reason_code: 'csrf_required' });
      return;
    }
  }
  let backend;
  try {
    backend = resolveBackend(session.gatewayUrl);
  } catch (err) {
    sendJson(res, 500, { detail: `Invalid gateway URL: ${String(err?.message || err)}` });
    return;
  }
  const headers = { ...req.headers, host: backend.url.host };
  delete headers.cookie;
  delete headers.authorization;
  delete headers['x-forwarded-for'];
  delete headers['x-forwarded-host'];
  delete headers['x-forwarded-proto'];
  headers['x-abstractgateway-session'] = session.sessionId;
  if (mutatingMethod(req.method)) headers['x-abstractgateway-csrf'] = session.csrfToken;
  const proxyReq = backend.client.request(
    { protocol: backend.url.protocol, hostname: backend.url.hostname, port: backend.url.port, method: req.method, path: req.url, headers },
    (proxyRes) => {
      res.writeHead(proxyRes.statusCode || 502, proxyHeaders(proxyRes.headers));
      proxyRes.pipe(res);
    }
  );
  proxyReq.on('error', (err) => sendJson(res, 502, { detail: `Backend not reachable at ${backend.origin} (${String(err?.message || err)})` }));
  req.pipe(proxyReq);
}

const server = http.createServer((req, res) => {
  // Remove query strings and normalize path
  const url = new URL(req.url, `http://${req.headers.host}`);
  let pathname = url.pathname;

  if (pathname === '/api/connection/gateway') {
    void handleConnectionApi(req, res);
    return;
  }

  if (pathname.startsWith('/api/')) {
    proxyApiRequest(req, res);
    return;
  }
  
  // Security: prevent directory traversal
  if (pathname.includes('..')) {
    res.writeHead(400);
    res.end('Bad Request');
    return;
  }

  // Try to serve the requested file
  let filePath = join(DIST_DIR, pathname);
  
  if (serveFile(res, filePath)) {
    return;
  }

  // Try with .html extension
  if (serveFile(res, filePath + '.html')) {
    return;
  }

  // Try index.html in directory
  if (serveFile(res, join(filePath, 'index.html'))) {
    return;
  }

  // SPA fallback: serve index.html for all other routes
  const indexPath = join(DIST_DIR, 'index.html');
  if (serveFile(res, indexPath)) {
    return;
  }

  // If nothing works, return 404
  res.writeHead(404);
  res.end('Not Found');
});

server.listen(PORT, HOST, () => {
  console.log(`
╔════════════════════════════════════════════════════╗
║      AbstractCode Web UI is running!               ║
╚════════════════════════════════════════════════════╝

  🌐 Local:   http://localhost:${PORT}
  🌐 Network: http://${HOST}:${PORT}

  💻 Browser-based coding assistant
  🔗 Connect to AbstractGateway in settings
  🚀 Start coding with durable agent sessions

  Press Ctrl+C to stop
`);
});

process.on('SIGINT', () => {
  console.log('\n\n👋 Shutting down AbstractCode Web...\n');
  process.exit(0);
});

process.on('SIGTERM', () => {
  console.log('\n\n👋 Shutting down AbstractCode Web...\n');
  process.exit(0);
});
