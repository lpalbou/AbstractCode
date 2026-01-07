import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "./ui/app";
import "./ui/styles.css";

// Register minimal service worker (PWA shell cache).
if ("serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch(() => {
      // Best-effort. On iOS/Safari this can fail in some contexts.
    });
  });
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);


