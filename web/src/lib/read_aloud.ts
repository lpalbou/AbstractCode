/**
 * A verbatim copy of AbstractUIC panel-chat `src/read_aloud.ts` (backlog 0065, AbstractUIC PR #26): the
 * kit is not released in this wave, so the rule is inlined here; replace with the import from
 * `@abstractframework/panel-chat` on the next kit release (AbstractCode backlog).
 *
 * Read aloud: which reply a host speaks by itself (operator, 2026-10-29: "when read aloud is on,
 * opening or re-opening a session immediately starts speaking the existing messages").
 *
 * The rule, in one place for every host: a reply is spoken automatically only when it was
 * generated while this conversation was open — a run of it was sent from here or seen in
 * progress here (`live`), then it completed with a reply that was not there when the run
 * started. Opening, reloading or switching to a conversation never speaks: a run that is already
 * over when the conversation is shown was never live here. A run that fails or is stopped speaks
 * nothing and disarms (a later load of other messages is never taken for its reply). A person
 * can always press a message's speaker; that path does not go through this gate.
 *
 * Pure (no React): `readAloudStep` takes the previous state and what the host shows now, and
 * says what to speak, if anything. Hosts keep the state in a ref and call it on each render.
 */

export type ReadAloudReply = { id: string; text: string };

/** Where the conversation's newest run stands, as the host sees it now. */
export type ReadAloudPhase =
  /** Sent from here, starting, running or waiting: a reply may come. */
  | "live"
  /** The run completed (its reply may land a render later). */
  | "completed"
  /** The run failed or was stopped: no reply is coming. */
  | "ended"
  /** Nothing known (idle, loading, a status this rule does not know). */
  | "none";

export type ReadAloudState = {
  /** The conversation the state belongs to (a new scope starts disarmed). */
  scope: string;
  /** A run was sent or seen in progress in this scope and has not given its reply yet. */
  armed: boolean;
  /** The newest reply when the run was first seen live (not spoken: it was already there). */
  baselineId: string;
};

export const READ_ALOUD_INITIAL: ReadAloudState = { scope: "", armed: false, baselineId: "" };

export type ReadAloudInput = {
  /** The conversation on screen (e.g. `${principal}:${sessionId}`). */
  scope: string;
  phase: ReadAloudPhase;
  /** The newest finished reply on screen (null when there is none). */
  reply: ReadAloudReply | null;
};

export function readAloudStep(prev: ReadAloudState, input: ReadAloudInput): { state: ReadAloudState; speak: ReadAloudReply | null } {
  const replyId = input.reply?.id || "";
  // A new conversation (opened, switched to, reloaded): nothing seen live yet.
  const base: ReadAloudState = prev.scope === input.scope ? prev : { scope: input.scope, armed: false, baselineId: "" };
  if (input.phase === "live") {
    // Armed on the first live sight; the reply already there is the baseline.
    return { state: base.armed ? base : { ...base, armed: true, baselineId: replyId }, speak: null };
  }
  if (!base.armed) return { state: base, speak: null };
  if (input.phase === "ended") return { state: { ...base, armed: false }, speak: null };
  if (input.phase === "completed" && replyId && replyId !== base.baselineId && input.reply && input.reply.text.trim()) {
    return { state: { ...base, armed: false, baselineId: replyId }, speak: input.reply };
  }
  return { state: base, speak: null };
}

const LIVE = new Set(["sending", "starting", "queued", "pending", "admitted", "running", "waiting", "backoff", "paused", "stopping"]);
const ENDED = new Set(["failed", "cancelled", "canceled", "error", "stopped"]);

/** The phase of a run from the controller's `snapshot.status` (unknown statuses are "none"). */
export function readAloudPhase(status: string | null | undefined, sending = false): ReadAloudPhase {
  if (sending) return "live";
  const s = String(status || "").trim().toLowerCase();
  if (LIVE.has(s)) return "live";
  if (s === "completed") return "completed";
  if (ENDED.has(s)) return "ended";
  return "none";
}
