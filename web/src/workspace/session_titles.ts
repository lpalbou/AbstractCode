// A conversation's stored name (backlog 1343). The gateway owns it:
//   - `PUT /sessions/{session_id}/title` {"title": "..."} stores it, normalised (one line, at most
//     120 characters); an empty title clears it. Answer: {session_id, title: <stored or null>, changed}.
//     Errors like the archive route (404 session_not_found, 401/403, 422, 500).
//   - `GET /runs` rows of a titled session carry `session_title` (catalog.ts reads it).
// An older gateway serves neither: this app then offers no rename and never sends the PUT.
import { gatewayApiPath } from "@abstractframework/ui-kit";
import type { SessionSummary } from "./catalog";
import { gatewayRequest } from "./transport";

/**
 * Whether the gateway stores conversation names, from `GET discovery/capabilities` as the catalog
 * holds it (`catalog.capabilities`, the unwrapped `capabilities.contracts`). Only a literal `true`
 * counts: a missing key is an older gateway.
 */
export function gatewaySupportsSessionTitles(contracts: unknown): boolean {
  // >>> The one place that names where the gateway advertises `session_titles: true`.
  // ASSUMED until the gateway's change lands: `capabilities.contracts.sessions.session_titles`.
  const sessions = (contracts as { sessions?: { session_titles?: unknown } } | null | undefined)?.sessions;
  return sessions?.session_titles === true;
  // <<<
}

export const sessionTitlePath = (sessionId: string) => `sessions/${encodeURIComponent(sessionId)}/title`;

/** The name the gateway stored (null: none, the conversation is named by its first message). */
export type StoredSessionTitle = { title: string | null; changed: boolean };

/** Store (or, with "", clear) a conversation's name; answers what the gateway stored. */
export async function renameSession(
  sessionId: string,
  title: string,
  request: typeof gatewayRequest = gatewayRequest,
): Promise<StoredSessionTitle> {
  const body = (await request(gatewayApiPath(sessionTitlePath(sessionId)), {
    method: "PUT",
    body: JSON.stringify({ title }),
  })) as { title?: unknown; changed?: unknown } | null;
  const stored = typeof body?.title === "string" && body.title.trim() ? body.title : null;
  return { title: stored, changed: body?.changed === true };
}

/**
 * The names this client set, laid over the listed conversations until a list read after the
 * gateway answered shows them (or another device's newer name). A pending rename shows its name at
 * once and wins over any list; a refused one goes back to what was shown before it. Each rename has
 * a turn number, so a slow answer never overwrites a newer rename of the same conversation.
 */
export class SessionTitleOverrides {
  private entries = new Map<string, { title: string | null; pending: boolean; listGen: number; turn: number }>();
  private turns = 0;

  /** Starts a rename shown at once; returns its turn for `stored` / `refused`. */
  begin(sessionId: string, title: string, listGen: number): number {
    const turn = ++this.turns;
    this.entries.set(sessionId, { title: title.trim() || null, pending: true, listGen, turn });
    return turn;
  }

  /** The gateway's answer: its normalised name stays shown until a list read after now lands. */
  stored(sessionId: string, turn: number, title: string | null, listGen: number): void {
    if (this.entries.get(sessionId)?.turn === turn) this.entries.set(sessionId, { title, pending: false, listGen, turn });
  }

  /** A refusal: the list's (or an earlier confirmed) name shows again, unless a newer rename runs. */
  refused(sessionId: string, turn: number): void {
    if (this.entries.get(sessionId)?.turn === turn) this.entries.delete(sessionId);
  }

  /** A list read started at `listGen` landed: settled names it already reflects are dropped. */
  landed(listGen: number): void {
    for (const [id, entry] of this.entries) if (!entry.pending && entry.listGen < listGen) this.entries.delete(id);
  }

  clear(): void {
    this.entries.clear();
  }

  apply<T extends Pick<SessionSummary, "sessionId" | "title">>(sessions: T[]): T[] {
    if (!this.entries.size) return sessions;
    return sessions.map((item) => {
      const entry = this.entries.get(item.sessionId);
      if (!entry) return item;
      const { title: _listed, ...rest } = item;
      return (entry.title ? { ...rest, title: entry.title } : rest) as T;
    });
  }
}
