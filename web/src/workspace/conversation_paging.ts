// Conversations in the sidebar are counted in CONVERSATIONS, not runs. The gateway lists runs
// (`GET /runs?root_only=true`: one root run per turn), so a page of N runs folds into fewer
// conversations — on a real store 100 runs were ~20 conversations. The sidebar shows
// CONVERSATIONS_PAGE conversations and "Load more" adds CONVERSATIONS_PAGE more; the run fetch
// grows (the same `limit` parameter, no cursor) until it holds one conversation more than shown,
// or the gateway has no more runs.
import { normalizeSessionSummaries, type SessionSummary } from "./catalog";

export const CONVERSATIONS_PAGE = 25;
/** First guess of runs per conversation; the fetch doubles while it holds too few conversations. */
export const RUNS_PER_CONVERSATION = 4;

export const conversationRunsPath = (runLimit: number) =>
  `runs?root_only=true&include_ledger_len=false&limit=${runLimit}`;

function runCount(body: any): number {
  const items = body?.items ?? body?.runs;
  return Array.isArray(items) ? items.length : 0;
}

/**
 * Fetches root runs until they fold into more than `want` conversations (so "Load more" is
 * known to have something to show) or the gateway reports no more runs.
 */
export async function fetchConversationRuns(
  fetchRuns: (runLimit: number) => Promise<any>,
  want: number,
): Promise<any> {
  let runLimit = Math.max(1, want) * RUNS_PER_CONVERSATION;
  for (;;) {
    const body = await fetchRuns(runLimit);
    if (
      body?.has_more !== true ||
      runCount(body) < runLimit ||
      normalizeSessionSummaries(body).length > want
    )
      return body;
    runLimit *= 2;
  }
}

/** The conversations the sidebar shows and whether "Load more" has more to show. */
export function conversationPage(
  sessions: SessionSummary[],
  want: number,
): { sessions: SessionSummary[]; hasMore: boolean } {
  return { sessions: sessions.slice(0, want), hasMore: sessions.length > want };
}
