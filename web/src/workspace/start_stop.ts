import { gatewayApiPath } from "@abstractframework/ui-kit";
import { gatewayRequest, newId } from "./transport";

/**
 * A Stop pressed while a run was still starting (review 2026-11-08 D3, R12). The press is answered
 * at once; the run the gateway then returns is cancelled. Until that cancel is accepted the stop
 * is kept in this tab's sessionStorage, so a reload, or another conversation opened meanwhile,
 * never leaves a run going that nobody sees: the next page of this tab finishes it
 * (`finishPendingStops`). A record older than ten minutes is dropped.
 */
export type PendingStop = { id: string; sessionId: string; at: number; runId?: string };

const KEY = "abstractcode.stop-while-starting.v1";
const MAX_AGE_MS = 10 * 60_000;
/** A run the gateway created this long before the press still belongs to it (clock and queue slack). */
const BEFORE_PRESS_MS = 5_000;

function storage(): Storage | null {
  try {
    return typeof sessionStorage === "undefined" ? null : sessionStorage;
  } catch {
    return null;
  }
}

export function pendingStops(now = Date.now()): PendingStop[] {
  const s = storage();
  if (!s) return [];
  try {
    const list = JSON.parse(s.getItem(KEY) || "[]");
    return Array.isArray(list)
      ? list.filter((p) => p && typeof p.id === "string" && typeof p.sessionId === "string" && typeof p.at === "number" && now - p.at < MAX_AGE_MS)
      : [];
  } catch {
    return [];
  }
}

function save(list: PendingStop[]): void {
  const s = storage();
  if (!s) return;
  try {
    if (list.length) s.setItem(KEY, JSON.stringify(list));
    else s.removeItem(KEY);
  } catch {
    /* storage refused: the in-page cancel still runs */
  }
}

/** Remember a Stop pressed while the conversation `sessionId` was starting a run; returns its id. */
export function rememberStopWhileStarting(sessionId: string): string {
  const id = newId();
  save([...pendingStops(), { id, sessionId, at: Date.now() }]);
  return id;
}

/** The run that start created (cancelled by id from now on, also after a reload). */
export function noteStartedRun(id: string, runId: string): void {
  save(pendingStops().map((p) => (p.id === id ? { ...p, runId } : p)));
}

/** The stop is done (the cancel was accepted) or moot (nothing was started). */
export function forgetStopWhileStarting(id: string): void {
  if (!id) return;
  save(pendingStops().filter((p) => p.id !== id));
}

/** Cancel a run by its id: the same command the Stop button sends, without the run on screen. */
export async function cancelStartedRun(runId: string): Promise<void> {
  await gatewayRequest(gatewayApiPath("commands"), {
    method: "POST",
    body: JSON.stringify({ command_id: newId(), run_id: runId, type: "cancel", payload: {} }),
  });
}

const TERMINAL = new Set(["completed", "failed", "cancelled"]);

/** The runs a pending stop covers when the page that pressed it never saw the start's answer. */
async function runsStartedAround(p: PendingStop): Promise<string[]> {
  const query = new URLSearchParams({ session_id: p.sessionId, root_only: "true", limit: "20", include_ledger_len: "false" });
  const answer = await gatewayRequest<{ items?: Array<Record<string, unknown>> }>(gatewayApiPath(`runs?${query}`));
  return (Array.isArray(answer?.items) ? answer.items : [])
    .filter((run) => {
      const created = Date.parse(String(run.created_at || ""));
      return Number.isFinite(created) && created >= p.at - BEFORE_PRESS_MS && created <= p.at + MAX_AGE_MS && !TERMINAL.has(String(run.status || ""));
    })
    .map((run) => String(run.run_id || ""))
    .filter(Boolean);
}

/**
 * Finish what an earlier page of this tab asked to stop while a run was starting. A record stays
 * (until it ages out) while its cancel fails, or while the gateway does not list the run yet (it
 * may still be starting it): the caller tries again. Returns how many stops are still pending.
 */
export async function finishPendingStops(): Promise<number> {
  for (const p of pendingStops()) {
    try {
      const runs = p.runId ? [p.runId] : await runsStartedAround(p);
      if (!runs.length) continue;
      for (const runId of runs) await cancelStartedRun(runId);
      forgetStopWhileStarting(p.id);
    } catch {
      /* kept: tried again */
    }
  }
  return pendingStops().length;
}
