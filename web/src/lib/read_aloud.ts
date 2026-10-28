/**
 * A verbatim copy of AbstractUIC panel-chat `src/read_aloud.ts` (backlog 0065, AbstractUIC PR #26): the
 * kit is not released in this wave, so the rule is inlined here; replace with the import from
 * `@abstractframework/panel-chat` on the next kit release (AbstractCode backlog).
 *
 * Read aloud: which reply a host speaks by itself (operator, 2026-10-29: "when read aloud is on,
 * opening or re-opening a session immediately starts speaking the existing messages").
 *
 * The rule, in one place for every host: a reply is spoken automatically only when it was
 * generated while this conversation was open — its run was seen in progress here (`live`), then
 * a reply that was not there when the run started is on screen and the run is no longer live.
 * Opening, reloading or switching to a conversation never speaks: a run that is already over
 * when the conversation is shown was never seen live. A person can always press a message's
 * speaker; that path does not go through this gate.
 *
 * Pure (no React): `readAloudStep` takes the previous state and what the host shows now, and
 * says what to speak, if anything. Hosts keep the state in a ref and call it on each render.
 */

export type ReadAloudReply = { id: string; text: string };

export type ReadAloudState = {
  /** The conversation the state belongs to (a new scope starts disarmed). */
  scope: string;
  /** A run was seen in progress in this scope since the last spoken reply. */
  armed: boolean;
  /** The newest reply when the run was first seen in progress (not spoken: it was already there). */
  baselineId: string;
};

export const READ_ALOUD_INITIAL: ReadAloudState = { scope: "", armed: false, baselineId: "" };

export type ReadAloudInput = {
  /** The conversation on screen (e.g. `${principal}:${sessionId}`). */
  scope: string;
  /** A run of this conversation is in progress now (running, waiting, …; not loading, not over). */
  live: boolean;
  /** The newest reply on screen (finished text; null when there is none). */
  reply: ReadAloudReply | null;
};

export function readAloudStep(prev: ReadAloudState, input: ReadAloudInput): { state: ReadAloudState; speak: ReadAloudReply | null } {
  const replyId = input.reply?.id || "";
  // A new conversation (opened, switched to, reloaded): nothing seen live yet.
  const base: ReadAloudState = prev.scope === input.scope ? prev : { scope: input.scope, armed: false, baselineId: "" };
  if (input.live) {
    // Armed on the first live sight; the reply already there is the baseline.
    return { state: base.armed ? base : { ...base, armed: true, baselineId: replyId }, speak: null };
  }
  if (base.armed && replyId && replyId !== base.baselineId && input.reply && input.reply.text.trim()) {
    return { state: { ...base, armed: false, baselineId: replyId }, speak: input.reply };
  }
  return { state: base, speak: null };
}

/** The statuses of a run that is in progress (the controller's `snapshot.status`). */
export function runIsLive(status: string | null | undefined): boolean {
  const s = String(status || "").trim().toLowerCase();
  return Boolean(s) && !["idle", "loading", "completed", "failed", "cancelled", "canceled", "error"].includes(s);
}
