// Minimal service worker (PWA shell cache).
// Note: PWA on iOS requires HTTPS (except localhost) and has platform-specific limitations.

const CACHE_NAME = "abstractcode-thin-client-v0";

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      // Cache only the root shell. Vite will emit hashed assets in production builds.
      await cache.addAll(["/"]);
      self.skipWaiting();
    })()
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      const keys = await caches.keys();
      await Promise.all(keys.map((k) => (k === CACHE_NAME ? Promise.resolve() : caches.delete(k))));
      self.clients.claim();
    })()
  );
});

self.addEventListener("fetch", (event) => {
  const req = event.request;
  const url = new URL(req.url);

  // Never cache API calls.
  if (url.pathname.startsWith("/api/")) {
    return;
  }

  event.respondWith(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      const cached = await cache.match(req);
      if (cached) return cached;
      const resp = await fetch(req);
      // Best-effort cache for same-origin GET requests.
      if (req.method === "GET" && resp && resp.status === 200 && url.origin === self.location.origin) {
        cache.put(req, resp.clone());
      }
      return resp;
    })()
  );
});


