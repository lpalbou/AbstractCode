// Web 0.15.6 (operator 2026-11-07): Code's conversation looks like the web Assistant's — the
// shared panel-chat message, no Code overrides —, the working dots sit where the reply will
// appear while the run works, and the bottom status line is gone (connection and theme live in
// the top-right account widget, the workflow in Settings, the run id in Activity).
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { WorkflowChat } from "../vendor/abstractchat/conversation.js";

const css = readFileSync(new URL("./workspace.css", import.meta.url), "utf8");
const vendoredCss = readFileSync(new URL("../vendor/abstractchat/conversation.css", import.meta.url), "utf8");
const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");

describe("one message look: the shared panel-chat default", () => {
  it("Code's conversation does not restyle the message card, its header, avatar, name, time or body", () => {
    for (const cls of ["pc-chat-item", "pc-chat-item--user", "pc-chat-header", "pc-chat-avatar", "pc-chat-role", "pc-chat-time", "pc-md"])
      expect(css).not.toMatch(new RegExp(`\\.code-conversation\\s+\\.${cls}\\b`));
  });
  it("the shared default Code renders: no avatar tile, the time in the UI font, the compact card", () => {
    expect(vendoredCss).toMatch(/\.pc-chat-avatar \{\s*display: none;/);
    expect(vendoredCss).toMatch(/\.pc-chat-time \{[^}]*font-family: inherit;/);
    expect(vendoredCss).toMatch(/\.pc-chat-item \{[^}]*padding: 16px 18px;/);
  });
});

describe("working dots", () => {
  const base = { messages: [{ id: "u1", role: "user", content: "Hello" }], draft: "", onDraftChange() {}, onSend() {} };
  it("the shared chat shows three dots at the transcript's tail while working", () => {
    const html = renderToStaticMarkup(<WorkflowChat {...base} busy working workingLabel="Working" />);
    expect(html).toMatch(/Hello[\s\S]*<div class="pc-chat-working"><span class="pc-working-dots" role="img" aria-label="Working">(<span class="pc-working-dots__dot"><\/span>){3}<\/span><\/div>/);
    expect(renderToStaticMarkup(<WorkflowChat {...base} busy />)).not.toContain("pc-working-dots");
  });
  it("Code drives them from the run's working state, never from text", () => {
    expect(appSource).toContain("const progressTone = useMemo(() => workflowProgress(snapshot).tone, [snapshot]);");
    expect(appSource).toContain('const working = !paused && (sending || (active && progressTone === "working"));');
    expect(appSource).toMatch(/<WorkflowChat[\s\S]*?working=\{working\}[\s\S]*?workingLabel=\{sending \? "Starting" : "Working"\}/);
  });
});

describe("no bottom status line", () => {
  it("the app renders no status line and keeps no CSS for it", () => {
    expect(appSource).not.toContain("code-statusbar");
    expect(css).not.toContain("code-statusbar");
    expect(appSource).not.toContain("Ready to start");
    expect(appSource).not.toContain("No workflow selected");
  });
  it("phone landscape gives the freed row to the conversation", () => {
    expect(css).not.toContain('"foot foot foot"');
    expect(css).toMatch(/grid-template-rows: auto minmax\(0, 1fr\);\s*grid-template-areas:\s*"nav tool actions"\s*"content content content";/);
  });
});
