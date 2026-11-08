import { gatewayApiPath } from "@abstractframework/ui-kit";
import { gatewayRequest, newId } from "./transport";

/**
 * A Stop pressed while a run was starting (reviews 2026-11-08 D3, R12, D6).
 *
 * In the page that pressed it, the stop is exact: the run id the start returns is cancelled.
 * The tab also keeps the stop in sessionStorage until that cancel is accepted, for one case only:
 * the page went away (a reload) before the start answered. The next page of the tab then looks
 * for the run that start created and NEVER guesses:
 * - exactly one unfinished run of that conversation started in the press's own window
 *   (1 s before … 60 s after the press): it is cancelled;
 * - none yet: the stop keeps waiting (the gateway may still be creating it), at most
 *   `WAIT_FOR_RUN_MS` after the press, then it is dropped;
 * - more than one: nothing is cancelled; the conversation shows the candidates and the person
 *   stops the one they meant (`ambiguous`).
 * A stop is dropped when the tab starts a new run in that conversation (the person moved on), and
 * when the start failed (the gateway refused it, or the network cut it in a page still here).
 */
export type PendingStop = {
  id: string;
  sessionId: string;
  at: number;
  runId?: string;
  /** Set when more than one run matches: the person chooses (never a guess). */
  candidates?: Array<{ runId: string; createdAt: string }>;
};

const KEY = "abstractcode.stop-while-starting.v1";
/** The press's window: a run this start created was created between these bounds. */
export const WINDOW_BEFORE_MS = 1_000;
export const WINDOW_AFTER_MS = 60_000;
/** How long a reloaded stop waits for its run to be listed before it is dropped. */
export const WAIT_FOR_RUN_MS = WINDOW_AFTER_MS + 30_000;
/** A stop with a known run id (or candidates) is kept at most this long while its cancel fails. */
const MAX_AGE_MS = 10 * 60_000;

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

/** The stop is done (the cancel was accepted), moot (nothing started) or decided by the person. */
export function forgetStopWhileStarting(id: string): void {
  if (!id) return;
  save(pendingStops().filter((p) => p.id !== id));
}

/** The tab started a new run in this conversation: an older stop there no longer applies. */
export function forgetStopsOfConversation(sessionId: string): void {
  save(pendingStops().filter((p) => p.sessionId !== sessionId));
}

/** Cancel a run by its id (the Stop button's command); a refused cancel is an error. */
export async function cancelStartedRun(runId: string): Promise<void> {
  const answer = await gatewayRequest<{ accepted?: boolean; detail?: unknown } | undefined>(gatewayApiPath("commands"), {
    method: "POST",
    body: JSON.stringify({ command_id: newId(), run_id: runId, type: "cancel", payload: {} }),
  });
  if (answer && answer.accepted === false) throw new Error(`the gateway did not accept the stop${answer.detail ? `: ${String(answer.detail)}` : ""}`);
}

const TERMINAL = new Set(["completed", "failed", "cancelled"]);

/** The unfinished runs of the conversation created inside the press's window. */
async function runsInWindow(p: PendingStop): Promise<Array<{ runId: string; createdAt: string }>> {
  const query = new URLSearchParams({ session_id: p.sessionId, root_only: "true", limit: "20", include_ledger_len: "false" });
  const answer = await gatewayRequest<{ items?: Array<Record<string, unknown>> }>(gatewayApiPath(`runs?${query}`));
  return (Array.isArray(answer?.items) ? answer.items : [])
    .filter((run) => {
      const created = Date.parse(String(run.created_at || ""));
      return Number.isFinite(created) && created >= p.at - WINDOW_BEFORE_MS && created <= p.at + WINDOW_AFTER_MS && !TERMINAL.has(String(run.status || ""));
    })
    .map((run) => ({ runId: String(run.run_id || ""), createdAt: String(run.created_at || "") }))
    .filter((run) => Boolean(run.runId));
}

/**
 * Finish what an earlier page of this tab asked to stop while a run was starting (see above).
 * Returns the stops still waiting for their run (the caller tries again later) and those that
 * need the person's choice.
 */
export async function finishPendingStops(now = () => Date.now()): Promise<{ waiting: number; choices: PendingStop[] }> {
  for (const p of pendingStops()) {
    if (p.candidates) continue; // the person decides
    try {
      if (p.runId) {
        await cancelStartedRun(p.runId);
        forgetStopWhileStarting(p.id);
        continue;
      }
      const runs = await runsInWindow(p);
      if (runs.length === 1) {
        await cancelStartedRun(runs[0].runId);
        forgetStopWhileStarting(p.id);
      } else if (runs.length > 1) {
        save(pendingStops().map((q) => (q.id === p.id ? { ...q, candidates: runs } : q)));
      } else if (now() - p.at > WAIT_FOR_RUN_MS) {
        forgetStopWhileStarting(p.id); // no run was created for it
      }
    } catch {
      /* kept: tried again */
    }
  }
  const left = pendingStops();
  return { waiting: left.filter((p) => !p.candidates).length, choices: left.filter((p) => Boolean(p.candidates)) };
}
