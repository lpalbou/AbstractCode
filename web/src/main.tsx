import React from "react";
import ReactDOM from "react-dom/client";

import "@abstractframework/ui-kit/theme.css";
import { installViewportVars } from "@abstractframework/ui-kit";

import "@abstractframework/panel-chat/panel_chat.css";
import { CodeWorkspace, WorkspaceErrorBoundary } from "./workspace/app";
import "./workspace/workspace.css";

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

// The visible-viewport height (--vv-height, shrinks under the iOS keyboard) and
// --keyboard-inset, from the kit (idempotent across HMR). The shell sizes to
// var(--vv-height, var(--vh-full)) — see workspace.css.
installViewportVars();

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
    <WorkspaceErrorBoundary><CodeWorkspace /></WorkspaceErrorBoundary>
  </React.StrictMode>
);
