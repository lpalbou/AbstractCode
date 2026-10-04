// Round 6 (DESIGN R6.2): archive a conversation from the card's "⋯" and the header's "⋯", one
// inline confirm, POST /sessions/{id}/archive (never DELETE), the active one hands over to the next.
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ConversationCard } from "./sidebar_cards";
import { ARCHIVE_CONFIRM_TEXT, ArchiveConfirm } from "./sidebar_panels";
import { archiveSession, archiveSessionPath, nextConversationAfterArchive } from "./archived_conversations";
import type { SessionSummary } from "./catalog";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");

const item = (sessionId: string, prompt = `Prompt ${sessionId}`) =>
  ({ sessionId, prompt, latestRunId: `run-${sessionId}`, firstRunId: `run-${sessionId}`, turnCount: 1, updatedAt: "2026-10-04T01:00:00Z", state: "completed" }) as unknown as SessionSummary;

describe("conversation card ⋯", () => {
  it("offers Archive in the kit menu, named after the conversation", () => {
    const html = renderToStaticMarkup(<ConversationCard item={item("s1", "Fix the parser")} selected={false} onClick={() => {}} onAskArchive={() => {}} />);
    expect(html).toContain('class="af-menu code-card-menu"');
    expect(html).toContain('aria-label="More actions for Fix the parser"');
    expect(html).toMatch(/role="menuitem"[^>]*data-menu-id="archive"[^>]*>Archive<\/button>/);
    // The card body still opens the conversation (its own button, the menu is a sibling).
    expect(html).toMatch(/<button type="button" class="code-session code-card"[^>]*data-session-id="s1"/);
  });
  it("shows the inline confirm under the card only while asked", () => {
    const confirm = <ArchiveConfirm busy={false} onConfirm={() => {}} onCancel={() => {}} />;
    const asked = renderToStaticMarkup(<ConversationCard item={item("s1")} selected onClick={() => {}} onAskArchive={() => {}} confirm={confirm} />);
    expect(asked).toContain("code-session-item is-confirming");
    expect(asked).toContain(ARCHIVE_CONFIRM_TEXT);
    const idle = renderToStaticMarkup(<ConversationCard item={item("s1")} selected onClick={() => {}} onAskArchive={() => {}} />);
    expect(idle).not.toContain(ARCHIVE_CONFIRM_TEXT);
  });
});

describe("the inline confirm", () => {
  it("says what archiving means, with Archive and Cancel", () => {
    expect(ARCHIVE_CONFIRM_TEXT).toBe("Archive this conversation? It stays searchable and auditable; it just leaves this list.");
    const html = renderToStaticMarkup(<ArchiveConfirm busy={false} onConfirm={() => {}} onCancel={() => {}} />);
    expect(html).toContain('data-action="confirm-archive"');
    expect(html).toContain("<span>Archive</span>");
    expect(html).toContain('data-action="cancel-archive"');
    expect(html).not.toMatch(/Archive\.\.\.|Archive…/);
  });
  it("shows the gateway's refusal as a sentence and disables both buttons while busy", () => {
    const html = renderToStaticMarkup(<ArchiveConfirm busy error="Not archived: session_not_found" onConfirm={() => {}} onCancel={() => {}} />);
    expect(html).toContain('role="alert">Not archived: session_not_found');
    expect(html.match(/disabled=""/g)?.length).toBe(2);
  });
});

describe("archive request", () => {
  it("POSTs /sessions/{id}/archive (never DELETE)", async () => {
    const calls: Array<{ path: string; init?: RequestInit }> = [];
    const request = (async (path: string, init?: RequestInit) => {
      calls.push({ path, init });
      return { archived: true };
    }) as any;
    await archiveSession("s/1", request);
    expect(archiveSessionPath("s/1")).toBe("sessions/s%2F1/archive");
    expect(calls).toHaveLength(1);
    expect(calls[0].path).toMatch(/sessions\/s%2F1\/archive$/);
    expect(calls[0].init?.method).toBe("POST");
  });
  it("lets a refusal through to the caller", async () => {
    const request = (async () => {
      throw new Error("404 session_not_found");
    }) as any;
    await expect(archiveSession("x", request)).rejects.toThrow("session_not_found");
  });
});

describe("the conversation shown after archiving", () => {
  const list = [item("a"), item("b"), item("c")];
  it("is the next one, else the previous one, else none", () => {
    expect(nextConversationAfterArchive(list, "a")?.sessionId).toBe("b");
    expect(nextConversationAfterArchive(list, "b")?.sessionId).toBe("c");
    expect(nextConversationAfterArchive(list, "c")?.sessionId).toBe("b");
    expect(nextConversationAfterArchive([item("a")], "a")).toBeNull();
    expect(nextConversationAfterArchive([], "a")).toBeNull();
    expect(nextConversationAfterArchive(list, "zz")?.sessionId).toBe("a");
  });
});

describe("app wiring", () => {
  it("the header ⋯ offers Archive for a saved conversation; the active one hands over", () => {
    expect(appSource).toMatch(/<AfMenu\s+className="code-conversation-menu"[\s\S]{0,300}id: "archive", label: "Archive"[\s\S]{0,120}askArchive\(currentSession\.sessionId, "header"\)/);
    expect(appSource).toContain('onAskArchive={connection.connected ? () => askArchive(item.sessionId, "card") : undefined}');
    expect(appSource).toMatch(/await archiveSession\(sessionId\)/);
    expect(appSource).toMatch(/nextConversationAfterArchive\(filteredSessions, sessionId\)/);
    expect(appSource).toMatch(/catalog\.forgetSession\(sessionId\)/);
  });
});
