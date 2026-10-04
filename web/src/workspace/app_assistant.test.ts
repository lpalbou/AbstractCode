// Round 8 (R8.3): Code's Docs assistant is the kit's shared DocsAssistantDrawer
// grounded on THIS app's llms.txt (served from this app's build, read by the
// gateway at docs/corpus?app=code) through the docs-qa workflow.
import React from "react";
import http from "node:http";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DocsAssistantPanel, makeDocsQaAsk } from "@abstractframework/panel-chat";
import { AppAssistantDrawer, CODE_DOCS_SOURCE, docsGatewayFetch } from "./app_assistant";
// @ts-expect-error bin/server.js is plain ESM JavaScript without types
import { createCodeRequestHandler } from "../../bin/server.js";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const enc = new TextEncoder();

/** A fake gateway behind `fetch`: corpus, run start, live llm.delta frames, poll. */
function fakeGateway() {
  const calls: { url: string; method: string; csrf: string | null; body?: unknown }[] = [];
  let polls = 0;
  const json = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
  const sse = (frames: string[]) => new Response(new ReadableStream({ start(c) { for (const f of frames) c.enqueue(enc.encode(f)); c.close(); } }), { status: 200, headers: { "Content-Type": "text/event-stream" } });
  const delta = (seq: number, text: string) => `event: llm.delta\ndata: ${JSON.stringify({ kind: "llm.delta", run_id: "docs-run", call_id: "c1", seq, text, channel: "content", snapshot: false })}\n\n`;
  vi.stubGlobal("fetch", async (url: string, init: RequestInit = {}) => {
    const headers = new Headers(init.headers || {});
    calls.push({ url, method: String(init.method || "GET"), csrf: headers.get("X-AbstractCode-CSRF"), body: init.body });
    if (url === "api/gateway/docs/corpus?app=code") return json({ app: "AbstractCode", text: "# AbstractCode\n\n## Files\nOpen the Files panel." });
    if (url === "api/gateway/runs/start") return json({ run_id: "docs-run" });
    if (url === "api/gateway/runs/docs-run/ledger/stream?after=0") return sse([delta(0, "Open the "), delta(1, "**Files** panel.")]);
    if (url === "api/gateway/runs/docs-run") return json({ status: polls++ ? "completed" : "running", output: { response: "Open the **Files** panel.\n\n```json\n{\"panel\": \"files\"}\n```" } });
    return new Response(JSON.stringify({ detail: `unexpected ${url}` }), { status: 404 });
  });
  Object.defineProperty(globalThis, "document", { configurable: true, value: { cookie: "abstractcode_gateway_csrf=csrf%2Dtoken" } });
  return calls;
}

afterEach(() => {
  vi.unstubAllGlobals();
  delete (globalThis as { document?: unknown }).document;
});

describe("Code Docs assistant (kit DocsAssistantDrawer)", () => {
  it("asks docs-qa with Code's own llms.txt, streams the reply, CSRF on writes only", async () => {
    const calls = fakeGateway();
    const ask = makeDocsQaAsk({ fetchGateway: docsGatewayFetch, source: CODE_DOCS_SOURCE, pollMs: 1 });
    const live: string[] = [];
    const answer = await ask("Where are my files?", { signal: new AbortController().signal, sessionId: "code-docs-assistant:s1", onText: (t) => live.push(t) });
    expect(answer).toContain("Open the **Files** panel.");
    expect(live[live.length - 1]).toBe("Open the **Files** panel.");
    const start = calls.find((c) => c.url === "api/gateway/runs/start")!;
    const body = JSON.parse(String(start.body));
    expect(body).toMatchObject({ bundle_id: "docs-qa", flow_id: "docsqa001", session_id: "code-docs-assistant:s1" });
    expect(body.input_data).toMatchObject({ prompt: "Where are my files?", app: "AbstractCode", use_session_history: true });
    expect(body.input_data.docs).toContain("Open the Files panel.");
    expect(start.csrf).toBe("csrf-token");
    expect(calls.find((c) => c.url.startsWith("api/gateway/docs/corpus"))!.csrf).toBeNull();

    // The answer renders in the kit thread: question right, answer left, markdown + JSON, copy, footer.
    const html = renderToStaticMarkup(React.createElement(DocsAssistantPanel, {
      source: CODE_DOCS_SOURCE, draft: "", onDraftChange: () => {}, onSend: () => {},
      messages: [{ role: "user", content: "Where are my files?" }, { role: "assistant", title: "AbstractCode", content: answer }],
    }));
    expect(html).toContain("pc-chat-item--user");
    expect(html).toContain("pc-chat-item--assistant");
    expect(html).toContain("<strong>Files</strong>");
    expect(html).toContain("panel");
    expect(html.match(/aria-label="Copy message"/g)?.length).toBe(2);
    expect(html).toContain("Grounded on AbstractCode’s documentation (llms.txt) · docs-qa");
  });

  it("the drawer: compact header, icon-only New conversation, close, keep-alive", () => {
    const html = renderToStaticMarkup(React.createElement(AppAssistantDrawer, { open: true, onClose: () => {}, connected: true, topOffset: 48 }));
    expect(html).toContain("code-app-assistant");
    expect(html).toContain("Docs assistant");
    expect(html).toMatch(/aria-label="New conversation"[^>]*><svg/);
    expect(html).not.toMatch(/>New conversation</);
    expect(html).toContain('aria-label="Close panel"');
    expect(html).toContain('aria-label="Attach files"');
    const signedOut = renderToStaticMarkup(React.createElement(AppAssistantDrawer, { open: true, onClose: () => {}, connected: false, topOffset: 48 }));
    expect(signedOut).toContain("Connect to the gateway to use the docs assistant.");
  });

  it("the top bar opens it from the shared docs slot (book icon)", () => {
    expect(appSource).toContain('docs={{ open: assistantOpen,');
    expect(appSource).not.toContain('assistant={{ open: assistantOpen');
  });

  it("the app serves its llms.txt as text/plain (what the gateway reads)", async () => {
    const dist = mkdtempSync(join(tmpdir(), "code-dist-"));
    writeFileSync(join(dist, "index.html"), "<!doctype html><title>AbstractCode</title>");
    writeFileSync(join(dist, "llms.txt"), "# AbstractCode\n");
    const server = http.createServer(createCodeRequestHandler({ distDir: dist, env: {} }));
    await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
    try {
      const port = (server.address() as { port: number }).port;
      const res = await new Promise<{ status: number; type: string; body: string }>((resolve, reject) => {
        http.get(`http://127.0.0.1:${port}/llms.txt`, (r) => {
          let body = "";
          r.on("data", (d) => (body += d));
          r.on("end", () => resolve({ status: r.statusCode || 0, type: String(r.headers["content-type"] || ""), body }));
        }).on("error", reject);
      });
      expect(res.status).toBe(200);
      expect(res.type).toMatch(/^text\/plain/);
      expect(res.body).toBe("# AbstractCode\n");
    } finally {
      server.close();
    }
  });
});
