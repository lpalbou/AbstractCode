import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { normalizeSessionSummaries } from "./catalog";
import {
  CONVERSATIONS_PAGE,
  conversationPage,
  conversationRunsPath,
  fetchConversationRuns,
} from "./conversation_paging";

const hookSource = readFileSync(new URL("./use_workspace_catalog.ts", import.meta.url), "utf8");

/** `sessions` conversations of `turns` root runs each, newest first (the gateway's order). */
function store(sessions: number, turns: number) {
  const runs: Record<string, unknown>[] = [];
  for (let s = 0; s < sessions; s++)
    for (let t = 0; t < turns; t++) {
      const at = new Date(Date.UTC(2026, 8, 30, 12) - (s * turns + t) * 60_000).toISOString();
      runs.push({ run_id: `r-${s}-${t}`, session_id: `s-${s}`, parent_run_id: null, created_at: at, updated_at: at, input_data: { prompt: `c${s}` } });
    }
  const limits: number[] = [];
  const fetchRuns = async (limit: number) => {
    limits.push(limit);
    return { items: runs.slice(0, limit), has_more: runs.length > limit };
  };
  return { fetchRuns, limits };
}

describe("conversations page by 25 conversations, not runs", () => {
  it("one page is 25 conversations and the first fetch asks 100 runs", () => {
    expect(CONVERSATIONS_PAGE).toBe(25);
    expect(conversationRunsPath(100)).toBe("runs?root_only=true&include_ledger_len=false&limit=100");
  });

  it("grows the run fetch until it holds one conversation more than shown (5 turns each: 100 runs = 20 conversations)", async () => {
    const { fetchRuns, limits } = store(40, 5);
    const runs = await fetchConversationRuns(fetchRuns, 25);
    expect(limits).toEqual([100, 200]);
    const page = conversationPage(normalizeSessionSummaries(runs), 25);
    expect(page.sessions).toHaveLength(25);
    expect(page.sessions.map((s) => s.sessionId).slice(0, 2)).toEqual(["s-0", "s-1"]);
    expect(page.sessions[24].turnCount).toBe(5);
    expect(page.hasMore).toBe(true);
  });

  it("Load more (50) shows the 40 that exist and then has no more", async () => {
    const { fetchRuns, limits } = store(40, 5);
    const runs = await fetchConversationRuns(fetchRuns, 50);
    expect(limits).toEqual([200]);
    const page = conversationPage(normalizeSessionSummaries(runs), 50);
    expect(page.sessions).toHaveLength(40);
    expect(page.hasMore).toBe(false);
  });

  it("exactly 25 conversations: all shown, no Load more", async () => {
    const { fetchRuns } = store(25, 1);
    const page = conversationPage(normalizeSessionSummaries(await fetchConversationRuns(fetchRuns, 25)), 25);
    expect(page.sessions).toHaveLength(25);
    expect(page.hasMore).toBe(false);
  });

  it("stops when the gateway returns fewer runs than asked even if it says has_more", async () => {
    const limits: number[] = [];
    const body = await fetchConversationRuns(async (limit) => {
      limits.push(limit);
      return { items: [{ run_id: "a", session_id: "s" }], has_more: true };
    }, 25);
    expect(limits).toEqual([100]);
    expect(body.items).toHaveLength(1);
  });

  it("the catalog hook fetches the list through the conversation pager and Load more adds a page", () => {
    expect(hookSource).toContain("fetchConversationRuns(");
    expect(hookSource).toContain("conversationPage(");
    expect(hookSource).toMatch(/visible\.current \+= CONVERSATIONS_PAGE;/);
    expect(hookSource).not.toMatch(/limit=\$\{limit\}/);
  });
});
