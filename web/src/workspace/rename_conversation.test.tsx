// Backlog 1343: rename a conversation from its sidebar card. The kit card (vendored) offers Rename only when the
// host passes onRename, which the app does only on a gateway that advertises `session_titles`; the request is
// PUT /sessions/{id}/title (never on an older gateway); one function names a conversation everywhere.
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ConversationCard, UNTITLED_CONVERSATION, conversationTitle } from "./sidebar_cards";
import { gatewaySupportsSessionTitles, renameSession, sessionTitlePath } from "./session_titles";
import type { SessionSummary } from "./catalog";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const archivedSource = readFileSync(new URL("./archived_conversations.tsx", import.meta.url), "utf8");

const item = (over: Partial<SessionSummary> = {}) =>
  ({ sessionId: "s1", prompt: "Fix the parser", latestRunId: "r1", firstRunId: "r1", runIds: ["r1"], turnCount: 1, updatedAt: "2026-10-04T01:00:00Z", state: "done", truncated: false, ...over }) as SessionSummary;
const card = (props: Partial<React.ComponentProps<typeof ConversationCard>> = {}) =>
  renderToStaticMarkup(<ConversationCard item={item()} selected={false} onClick={() => {}} onAskArchive={() => {}} {...props} />);

describe("the card's Rename action", () => {
  it("is offered only with onRename, as a sibling of the open and archive buttons", () => {
    const plain = card();
    expect(plain).not.toContain("af-session-card__rename");
    expect((plain.match(/<button/g) || []).length).toBe(2);
    const renamable = card({ onRename: async () => {} });
    expect(renamable).toContain('class="af-session-card__rename" aria-label="Rename Fix the parser" title="Rename conversation" aria-keyshortcuts="F2"');
    expect((renamable.match(/<button/g) || []).length).toBe(3);
    expect(renamable).not.toMatch(/<button[^>]*>(?:(?!<\/button>)[\s\S])*<button/);
  });
  it("names the card by its stored title, and the selected (open) conversation can be renamed too", () => {
    const html = card({ item: item({ title: "Parser work" }), selected: true, onRename: async () => {} });
    expect(html).toContain('aria-current="page"');
    expect(html).toContain(">Parser work</span>");
    expect(html).toContain('aria-label="Rename Parser work"');
  });
  it("says a refusal under the card, the way an archive refusal is said", () => {
    const html = card({ onRename: async () => {}, renameError: "Not renamed: session_not_found" });
    expect(html).toContain('<p class="code-inline-error" role="alert">Not renamed: session_not_found</p>');
    expect(card({ onRename: async () => {} })).not.toContain("role=\"alert\"");
  });
});

describe("one name everywhere", () => {
  it("stored title, else the first message, else Untitled conversation", () => {
    expect(conversationTitle({ title: "Snake", prompt: "Write a snake game" })).toBe("Snake");
    expect(conversationTitle({ title: "  ", prompt: "Write a snake game" })).toBe("Write a snake game");
    expect(conversationTitle({ prompt: "Write a snake game" })).toBe("Write a snake game");
    expect(conversationTitle({})).toBe(UNTITLED_CONVERSATION);
  });
  it("the sidebar, the chat header and the archived list all use it", () => {
    expect(appSource).toMatch(/conversationTitle\(\{ title: currentSession\?\.title, prompt: firstAsked \}\)/);
    expect(appSource).toMatch(/<strong title=\{automationView \? automationTitle : title\}>/);
    expect(archivedSource).toContain("title={conversationTitle(item)}");
  });
});

describe("the gateway capability", () => {
  it("only a literal session_titles: true at the advertised place turns rename on", () => {
    expect(gatewaySupportsSessionTitles({ sessions: { session_titles: true } })).toBe(true);
    for (const older of [{}, null, undefined, { sessions: {} }, { sessions: { session_titles: "true" } }, { sessions: { session_titles: false } }, { session_titles: true }])
      expect(gatewaySupportsSessionTitles(older)).toBe(false);
  });
  it("the app passes onRename only when the gateway advertises it (an older one is never asked)", () => {
    expect(appSource).toContain("const renameSupported = connection.connected && gatewaySupportsSessionTitles(catalog.capabilities);");
    expect(appSource).toContain("onRename={renameSupported ? (name) => renameConversation(item.sessionId, name) : undefined}");
    expect(appSource.match(/renameConversation\(/g)?.length).toBe(1);
    expect(appSource).toMatch(/await catalog\.renameSession\(sessionId, name\)/);
    expect(appSource).toMatch(/setRenameError\(\{ sessionId, message: `Not renamed: \$\{formatError\(e\)\}` \}\);\s*throw e;/);
  });
});

describe("the rename request", () => {
  it("PUTs {title} to /sessions/{id}/title and answers the stored name", async () => {
    const calls: Array<{ path: string; init?: RequestInit }> = [];
    const request = (async (path: string, init?: RequestInit) => {
      calls.push({ path, init });
      return { session_id: "s/1", title: "Snake game", changed: true };
    }) as any;
    expect(await renameSession("s/1", "  Snake game ", request)).toEqual({ title: "Snake game", changed: true });
    expect(sessionTitlePath("s/1")).toBe("sessions/s%2F1/title");
    expect(calls).toHaveLength(1);
    expect(calls[0].path).toBe("api/gateway/sessions/s%2F1/title");
    expect(calls[0].init?.method).toBe("PUT");
    expect(JSON.parse(String(calls[0].init?.body))).toEqual({ title: "  Snake game " });
  });
  it("a cleared name answers null", async () => {
    const request = (async () => ({ session_id: "s1", title: null, changed: false })) as any;
    expect(await renameSession("s1", "", request)).toEqual({ title: null, changed: false });
  });
  it.each([
    ["404", "session_not_found"],
    ["403", "Forbidden"],
    ["500", "session titles unreadable"],
    ["network", "Failed to fetch"],
  ])("lets a %s refusal through to the caller", async (_kind, message) => {
    const request = (async () => {
      throw new Error(message);
    }) as any;
    await expect(renameSession("x", "Name", request)).rejects.toThrow(message);
  });
});
