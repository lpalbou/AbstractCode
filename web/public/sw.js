// Minimal service worker (PWA shell cache).
// Note: PWA on iOS requires HTTPS (except localhost) and has platform-specific limitations.

const CACHE_NAME = "abstractcode-web-v1";

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      await cache.addAll(["./"]);
      self.skipWaiting();
    })()
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      const keys = await caches.keys();
      // Only this app's own older caches: other apps share the gateway's origin.
      await Promise.all(keys.map((k) => (k.startsWith("abstractcode-web-") && k !== CACHE_NAME ? caches.delete(k) : Promise.resolve())));
      self.clients.claim();
    })()
  );
});

self.addEventListener("fetch", (event) => {
  const req = event.request;
  const url = new URL(req.url);

  // Never cache API calls (under this app's own base: the gateway serves it at /apps/code/).
  if (url.pathname.startsWith(new URL("api/", self.registration.scope).pathname)) return;

  event.respondWith(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      // Network-first to avoid stale bundles after deploy; cache as a fallback for offline.
      try {
        const resp = await fetch(req);
        if (req.method === "GET" && resp && resp.status === 200 && url.origin === self.location.origin) {
          cache.put(req, resp.clone());
        }
        return resp;
      } catch {
        const cached = await cache.match(req);
        if (cached) return cached;
        throw new Error("offline and not cached");
      }
    })()
  );
});
