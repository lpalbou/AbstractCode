import React from "react";
import { DocsAssistantDrawer, docsAssistantPanel, type DocsAssistantSource, type GatewayFetch } from "@abstractframework/panel-chat";
import type { AfDrawerPanel } from "@abstractframework/ui-kit";
import { csrfHeaders } from "./transport";

/**
 * The shared Docs assistant drawer, grounded on THIS app's llms.txt (the gateway
 * reads it from this app's own build: `GET api/gateway/docs/corpus?app=code`) through the
 * gateway's docs-qa workflow. Conversation, attachments, streaming, history and focus are the kit's.
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

/** The right side's "docs" panel (kept alive; key the drawer system by account in the host). */
export function codeDocsPanel(connected: boolean): AfDrawerPanel {
  return { ...docsAssistantPanel(CODE_DOCS_SOURCE, docsGatewayFetch, { connected, suggestions: CODE_DOCS_SUGGESTIONS, placeholder: "Ask about AbstractCode…" }), fill: true };
}

/** Use the same drawer as Gateway, Observer, Flow, Entity and Note. */
export function CodeDocsAssistant(props: { connected: boolean; open: boolean; onClose: () => void }): React.ReactElement {
  return <DocsAssistantDrawer source={CODE_DOCS_SOURCE} fetchGateway={docsGatewayFetch} connected={props.connected} open={props.open} onClose={props.onClose} topOffset="auto" suggestions={CODE_DOCS_SUGGESTIONS} placeholder="Ask about AbstractCode…" />;
}
