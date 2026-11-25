// Backlog 1343: rename a conversation from its sidebar card. The kit card (vendored) offers Rename only when the
// host passes onRename, which the app does only on a gateway that advertises `session_titles`; the request is
// PUT /sessions/{id}/title (never on an older gateway); one function names a conversation everywhere.
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ConversationCard, UNTITLED_CONVERSATION, conversationHeaderTitle, conversationTitle, searchConversations } from "./sidebar_cards";
import { gatewaySupportsSessionTitles, renameSession, sessionTitlePath } from "./session_titles";
import type { SessionSummary } from "./catalog";


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
    expect(html).toMatch(/<p class="code-inline-error" id="(code-rename-error-[^"]+)" role="alert">Not renamed: session_not_found<\/p>/);
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
  it("a stored name without a visible character falls back to the next name", () => {
    for (const blank of ["\u200b\u200b", "\u2060", "\u0301", "\u3000", "\u202e"]) {
      expect(conversationTitle({ title: blank, prompt: "Write a snake game" })).toBe("Write a snake game");
      expect(conversationTitle({ title: blank })).toBe(UNTITLED_CONVERSATION);
    }
    expect(conversationTitle({ title: "\u0634\u0627\u06cc" })).toBe("\u0634\u0627\u06cc");
  });
  it("the header uses the listed name, else the name it was opened with (Archived), else the first message", () => {
    expect(conversationHeaderTitle({ listed: { title: "Snake", prompt: "Write a snake game" }, firstAsked: "Write a snake game", hasRun: true })).toBe("Snake");
    expect(conversationHeaderTitle({ listed: { prompt: "Write a snake game" }, firstAsked: "Other", hasRun: true })).toBe("Write a snake game");
    expect(conversationHeaderTitle({ opened: { title: "Q3 report", prompt: "Summarise" }, hasRun: true })).toBe("Q3 report");
    expect(conversationHeaderTitle({ opened: { prompt: "Summarise" }, hasRun: true })).toBe("Summarise");
    expect(conversationHeaderTitle({ firstAsked: "Typed first", hasRun: false })).toBe("Typed first");
    expect(conversationHeaderTitle({ hasRun: true })).toBe(UNTITLED_CONVERSATION);
    expect(conversationHeaderTitle({ hasRun: false })).toBe("New conversation");
  });
  it("search keeps the row being renamed, whatever it now matches", () => {
    const items = [item({ sessionId: "a", prompt: "Alpha plan" }), item({ sessionId: "b", prompt: "Beta notes", title: "Beta release plan" })];
    expect(searchConversations(items, "alpha").map((s) => s.sessionId)).toEqual(["a"]);
    expect(searchConversations(items, "alpha", new Set(["b"])).map((s) => s.sessionId)).toEqual(["a", "b"]);
    expect(searchConversations(items, "zzz", null).map((s) => s.sessionId)).toEqual([]);
  });
});

describe("the gateway capability", () => {
  it("only a literal capabilities.session_titles: true in GET discovery/capabilities turns rename on", () => {
    expect(gatewaySupportsSessionTitles({ capabilities: { session_titles: true, contracts: {} } })).toBe(true);
    for (const older of [{}, null, undefined, { capabilities: {} }, { capabilities: { session_titles: "true" } }, { capabilities: { session_titles: false } }, { session_titles: true }, { capabilities: { contracts: { session_titles: true } } }])
      expect(gatewaySupportsSessionTitles(older)).toBe(false);
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

describe("round 2: the request's own failures are plain refusals", () => {
  it("a gateway that does not answer in time is refused with a plain sentence", async () => {
    const request = ((_: string, init: RequestInit) => new Promise((_ok, no) => init.signal?.addEventListener("abort", () => no(init.signal!.reason)))) as any;
    await expect(renameSession("s1", "Name", request, 20)).rejects.toThrow("the gateway did not answer");
  });
  it("an answer that is not JSON is refused without its text", async () => {
    const request = (async () => { throw new SyntaxError("Unexpected token '<', \"<html>\" is not valid JSON"); }) as any;
    await expect(renameSession("s1", "Name", request)).rejects.toThrow("the gateway gave an answer that is not a name");
  });
  it("a blank stored title is no name; a long one is cut to 120 characters", async () => {
    expect(await renameSession("s1", "x", (async () => ({ title: "   ", changed: true })) as any)).toEqual({ title: null, changed: true });
    const long = await renameSession("s1", "x", (async () => ({ title: "\u{1F600}".repeat(300), changed: true })) as any);
    expect(Array.from(long.title!).length).toBe(120);
  });
  it("a refused request whose body is a page (a proxy's HTML) says only its status; a short sentence is kept", async () => {
    const { gatewayRequest } = await import("./transport");
    const original = globalThis.fetch;
    (globalThis as any).document ??= { cookie: "" };
    try {
      globalThis.fetch = (async () => new Response("<html><head><title>405 Not Allowed</title></head><body>nginx</body></html>", { status: 405 })) as any;
      await expect(gatewayRequest("api/gateway/sessions/s1/title", { method: "PUT", body: "{}" })).rejects.toThrow(/^Gateway request failed \(405\)$/);
      globalThis.fetch = (async () => new Response("Service unavailable", { status: 503 })) as any;
      await expect(gatewayRequest("api/gateway/sessions/s1/archive", { method: "POST", body: "{}" })).rejects.toThrow(/^Service unavailable$/);
      globalThis.fetch = (async () => new Response(JSON.stringify({ detail: { reason_code: "session_not_found", message: "Session s1 does not exist." } }), { status: 404 })) as any;
      await expect(gatewayRequest("api/gateway/sessions/s1/title", { method: "PUT", body: "{}" })).rejects.toThrow("Session s1 does not exist.");
    } finally {
      globalThis.fetch = original;
    }
  });
});
