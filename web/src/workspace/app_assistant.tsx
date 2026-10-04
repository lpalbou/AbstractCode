import React from "react";
import { DocsAssistantDrawer, type DocsAssistantSource, type GatewayFetch } from "@abstractframework/panel-chat";
import { useDrawerFocus } from "./layout";
import { csrfHeaders } from "./transport";

/**
 * The Docs assistant (round 8, R8.3): the kit's shared DocsAssistantDrawer —
 * the same chat the console and every app mount — grounded on THIS app's
 * llms.txt (the gateway reads it from this app's own build:
 * `GET api/gateway/docs/corpus?app=code`) through the gateway's docs-qa
 * workflow. Conversation, attachments, streaming and history are the kit's.
 */
export const CODE_DOCS_SOURCE: DocsAssistantSource = { app: "code", name: "AbstractCode" };

export const CODE_DOCS_SUGGESTIONS = ["How do I change the model?", "Where are my files and generated outputs?", "How do I schedule an automation?"];

/** Through this app's gateway proxy (relative `api/gateway/…`): the session
 * cookie, plus the proxy's CSRF header on writes (run start, uploads). */
export const docsGatewayFetch: GatewayFetch = (path, init = {}) => {
  const headers = new Headers(init.headers || {});
  const method = String(init.method || "GET").toUpperCase();
  if (method !== "GET" && method !== "HEAD") for (const [name, value] of Object.entries(csrfHeaders())) headers.set(name, value);
  return fetch(path, { ...init, headers, credentials: "same-origin" });
};

/** Keep mounted while closed; key by authenticated account in the host. */
export function AppAssistantDrawer({ open, onClose, connected, topOffset }: {
  open: boolean; onClose(): void; connected: boolean; topOffset: number;
}) {
  useDrawerFocus(open, true,
    () => document.querySelector(".code-app-assistant"),
    () => document.querySelector(".code-app-assistant .pc-composer textarea"));
  return <DocsAssistantDrawer open={open} onClose={onClose} source={CODE_DOCS_SOURCE} fetchGateway={docsGatewayFetch}
    connected={connected} topOffset={topOffset} className="code-app-assistant" placeholder="Ask about AbstractCode…"
    suggestions={CODE_DOCS_SUGGESTIONS} />;
}
