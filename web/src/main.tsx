import React from "react";
import ReactDOM from "react-dom/client";

import "@abstractuic/ui-kit/theme.css";

import { App } from "./ui/app";
import "./ui/styles.css";

function applyViewportHeightVar(): void {
  try {
    const vv = window.visualViewport;
    const h = typeof vv?.height === "number" && Number.isFinite(vv.height) ? vv.height : window.innerHeight;
    // Use a `vh`-like px unit that matches the *current* usable viewport height.
    document.documentElement.style.setProperty("--vh", `${Math.max(1, h) * 0.01}px`);
  } catch {
    // ignore
  }
}

// Dev DX: avoid "hard refresh" loops caused by a previously-installed service worker caching assets.
if (import.meta.env.DEV && "serviceWorker" in navigator) {
  navigator.serviceWorker
    .getRegistrations()
    .then((regs) => Promise.all(regs.map((r) => r.unregister())))
    .catch(() => {
      // Best-effort.
    });
  if ("caches" in window) {
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k.startsWith("abstractcode-web-")).map((k) => caches.delete(k))))
      .catch(() => {
        // Best-effort.
    });
  }
}

// Avoid stacking listeners during dev/HMR.
const VH_LISTENER_KEY = "__abstractcode_web_vh_listener_v1";
if (!(globalThis as any)[VH_LISTENER_KEY]) {
  (globalThis as any)[VH_LISTENER_KEY] = true;
  applyViewportHeightVar();
  window.addEventListener("resize", applyViewportHeightVar);
  window.visualViewport?.addEventListener("resize", applyViewportHeightVar);
  window.visualViewport?.addEventListener("scroll", applyViewportHeightVar);
}

// Prod: register the PWA shell service worker.
if (import.meta.env.PROD && "serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("sw.js").catch(() => {
      // Best-effort.
    });
  });
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
