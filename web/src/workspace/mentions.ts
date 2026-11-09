// `@entity` mentions from Code's chat (contract 2026-11-08, untracked CONTRACT-mentions.md):
// the person picks a name from the kit's `@` list (fed by `GET /entities`); on send, the person's
// turn starts as always, then each picked entity is asked IN ORDER through
// `POST /sessions/{session_id}/mentions {entity, text, mention_index}`, the next after the previous
// returned. Each reply is a turn of the session the gateway records (workflow `entity-mention`,
// with its `author`), so the conversation shows it from the gateway's own history after a reload too.
//
// DOM-free: app.tsx wires these rules; mentions.test.ts checks them. Nothing here reads `@` out
// of the message text (no-heuristics ruling): a mention is only ever a picked chip.
import { uniqueMentions, type ChatAuthor, type ChatMentionCandidate, type ChatMentionSent } from "../vendor/abstractchat/conversation.js";
import type { ChatMessage } from "@abstractframework/panel-chat";

/** The sentence an older gateway gets (no mentions route): shown, chips disabled, never silent. */
export const MENTIONS_UNAVAILABLE = "This gateway does not offer mentions yet.";

/** The workflow id of a turn the gateway recorded for an entity's reply to a mention. */
export const MENTION_WORKFLOW_ID = "entity-mention";

type Row = Record<string, unknown>;

function record(value: unknown): Row | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Row) : null;
}

function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

/** The names `GET /entities` offers this account (`{entities: [...]}`); unreadable rows are left out. */
export function entityCandidates(body: unknown): ChatMentionCandidate[] {
  const rows = Array.isArray(record(body)?.entities) ? (record(body)!.entities as unknown[]) : [];
  const out: ChatMentionCandidate[] = [];
  const seen = new Set<string>();
  for (const raw of rows) {
    const row = record(raw);
    if (!row || row.error !== undefined) continue;
    const id = text(row.slug);
    if (!id || seen.has(id)) continue;
    seen.add(id);
    const look = record(record(row.identity)?.avatar);
    const style = text(look?.style) || null;
    const color = text(look?.color) || null;
    out.push({ id, name: text(row.name) || id, kind: "entity", ...(style || color ? { avatar: { style, color } } : {}) });
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

/** The names for the query typed after `@`: names starting with it first, then names containing
 * it (case-insensitive, on the name or its id); the entities the gateway refused are not offered. */
export function filterMentionCandidates(
  all: readonly ChatMentionCandidate[],
  query: string,
  refused: ReadonlySet<string> = new Set(),
): ChatMentionCandidate[] {
  const q = String(query || "").trim().toLowerCase();
  const offered = all.filter((c) => !refused.has(c.id));
  if (!q) return offered;
  const starts = (c: ChatMentionCandidate) => c.name.toLowerCase().startsWith(q) || c.id.toLowerCase().startsWith(q);
  const contains = (c: ChatMentionCandidate) => c.name.toLowerCase().includes(q) || c.id.toLowerCase().includes(q);
  return [...offered.filter(starts), ...offered.filter((c) => !starts(c) && contains(c))];
}

/** The first-pick sentence under a chip (once per conversation). */
export function mentionNoticeFor(candidate: Pick<ChatMentionCandidate, "name">): string {
  return `${candidate.name} will read this conversation's history.`;
}

/** What a refused mention means for the rest of the send. */
export type MentionRefusal =
  /** 409: the entity is in another conversation (or its visit door refused): said, next mention. */
  | { kind: "elsewhere"; sentence: string }
  /** 404 for this entity (hidden = missing): said, not offered again, next mention. */
  | { kind: "not_offerable"; sentence: string }
  /** 404 (or 405) without a code: the route itself is missing (an older gateway). Mentions stop
   * and the composer says so. */
  | { kind: "unavailable"; sentence: string }
  /** 503 `entity_cannot_answer`: said, the message stays in the composer, mentions stop. */
  | { kind: "cannot_answer"; sentence: string }
  /** Anything else (a server error, the network): said, the message stays, mentions stop. */
  | { kind: "failed"; sentence: string };

function errorBody(error: unknown): { status: number; detail: string; code: string } {
  const e = record(error);
  const status = Number(e?.status);
  let raw: Row | null = null;
  const bodyText = typeof e?.body_text === "string" ? e.body_text : "";
  try {
    raw = record(JSON.parse(bodyText));
  } catch {
    raw = null;
  }
  const detail = text(raw?.detail) || (error instanceof Error ? error.message.trim() : "");
  return { status: Number.isFinite(status) ? status : 0, detail, code: text(raw?.code) };
}

/**
 * The refusal an error of `POST /sessions/{sid}/mentions` stands for: the gateway's codes decide
 * (`{detail, code}`). A 404 with a code (`entity_not_found`: a missing entity, or one this account
 * may not see) is this entity; a plain 404 or 405 without a code is the route itself missing (an
 * older gateway; every refusal of the door is coded). A 422 (`repeat_of_unknown`, …) is a failure
 * with the gateway's sentence.
 */
export function mentionRefusal(error: unknown, name: string): MentionRefusal {
  const { status, detail, code } = errorBody(error);
  if (status === 409) {
    if (code === "in_another_conversation" || !detail) return { kind: "elsewhere", sentence: `${name} is in another conversation.` };
    return { kind: "elsewhere", sentence: detail };
  }
  if ((status === 404 || status === 405) && !code) return { kind: "unavailable", sentence: MENTIONS_UNAVAILABLE };
  if (status === 404) return { kind: "not_offerable", sentence: `${name} cannot be mentioned here.` };
  if (status === 503) return { kind: "cannot_answer", sentence: detail || `${name} cannot answer right now.` };
  return { kind: "failed", sentence: `${name} could not be asked: ${detail || "the gateway did not answer"}.` };
}

/** A send's mentions once they were asked. */
export type MentionsOutcome = {
  /** The mentions answered, in order, with the session turn the gateway recorded for each reply. */
  answered: Array<{ mention: ChatMentionSent; runId: string }>;
  /** Refused, then the next one was asked (409, an entity's 404). */
  skipped: Array<{ mention: ChatMentionSent; refusal: MentionRefusal }>;
  /** The refusal that stopped the rest (503, the route missing, any other failure). */
  stopped: { mention: ChatMentionSent; refusal: MentionRefusal } | null;
  /** Not answered because of `stopped`: that mention and every one after it, with their index
   * (what a resend asks, with the same `repeat_of`). Empty when nothing stopped. */
  unsent: ChatMentionSent[];
  /** Stop was pressed: the mentions after the one in flight were not asked. */
  aborted: boolean;
  /** The mentions Stop kept from being asked. */
  unasked: ChatMentionSent[];
};

/** One `POST /sessions/{sid}/mentions`; resolves with the gateway's answer. */
export type MentionPost = (mention: ChatMentionSent) => Promise<unknown>;

/**
 * Ask each mention in order (`index` = `mention_index`), each after the previous returned, so the
 * second entity reads the first one's answer; an entity named twice is asked once (its first
 * index). A request already sent is never cut (the entity answers anyway and the gateway records
 * the turn): Stop only keeps the next ones from being asked.
 */
export async function sendMentions(
  mentions: readonly ChatMentionSent[],
  post: MentionPost,
  options: { signal?: AbortSignal; onAsking?: (mention: ChatMentionSent) => void } = {},
): Promise<MentionsOutcome> {
  const outcome: MentionsOutcome = { answered: [], skipped: [], stopped: null, unsent: [], aborted: false, unasked: [] };
  const ordered = uniqueMentions([...mentions].sort((a, b) => a.index - b.index));
  for (const [at, mention] of ordered.entries()) {
    if (options.signal?.aborted) {
      outcome.aborted = true;
      outcome.unasked = ordered.slice(at);
      break;
    }
    options.onAsking?.(mention);
    let answer: unknown;
    try {
      answer = await post(mention);
    } catch (error) {
      const refusal = mentionRefusal(error, mention.name);
      if (refusal.kind === "elsewhere" || refusal.kind === "not_offerable") {
        outcome.skipped.push({ mention, refusal });
        continue;
      }
      outcome.stopped = { mention, refusal };
      outcome.unsent = ordered.slice(at);
      break;
    }
    outcome.answered.push({ mention, runId: text(record(answer)?.run_id) });
  }
  return outcome;
}

/** The request body of one mention: the message as typed, the entity's id, its position, and
 * `repeat_of`, the person's own turn of this session that carries the same words (the gateway marks
 * the mention turn with it, and Code folds the repeated prompt by it). */
export function mentionRequestBody(
  text: string,
  mention: ChatMentionSent,
  repeatOf?: string,
): { entity: string; text: string; mention_index: number; repeat_of?: string } {
  const turn = String(repeatOf ?? "").trim();
  return { entity: mention.id, text: String(text ?? "").trim(), mention_index: mention.index, ...(turn ? { repeat_of: turn } : {}) };
}

/**
 * The turns whose prompt repeats another turn's words, learned from a view (or a view delta): a
 * mention turn's input message carries `repeat_of` (the person's own turn with the same words).
 * Noted as run id -> repeated turn into `into`; true when something new was noted.
 */
export function noteRepeats(view: unknown, into: Map<string, string>): boolean {
  const messages = record(view)?.messages;
  if (!Array.isArray(messages)) return false;
  let noted = false;
  for (const raw of messages) {
    const m = record(raw);
    const runId = text(m?.run_id);
    const of = text(m?.repeat_of);
    if (m?.role !== "user" || !runId || !of || of === runId || into.get(runId) === of) continue;
    into.set(runId, of);
    noted = true;
  }
  return noted;
}

/** The same on a `/view/stream` frame (`view` and `delta` carry messages; others teach nothing). */
export function noteRepeatsFrame(frame: { event?: string; data: string }, into: Map<string, string>): boolean {
  if ((frame.event !== "view" && frame.event !== "delta") || !frame.data.includes('"repeat_of"')) return false;
  try {
    return noteRepeats(JSON.parse(frame.data), into);
  } catch {
    return false; // the kit reports the malformed frame itself
  }
}

/**
 * The conversation without the prompts that repeat a turn SHOWN in it (`repeats`: run id -> the
 * turn it repeats): the person's words show once, on their own turn, and each entity's answer
 * follows. A repeat whose turn is not on screen keeps its words (else the question is gone).
 * The same array when nothing is folded.
 */
export function foldRepeatedTurns(messages: ChatMessage[], repeats: ReadonlyMap<string, string>): ChatMessage[] {
  if (!repeats.size) return messages;
  const asked = new Set(messages.filter((m) => m.role === "user" && m.runId && !repeats.has(m.runId)).map((m) => m.runId as string));
  const kept = messages.filter((m) => !(m.role === "user" && m.runId && asked.has(repeats.get(m.runId) ?? "")));
  return kept.length === messages.length ? messages : kept;
}

/** Live (streamed) replies labelled with the author their frames named, by run, until the saved
 * message (which carries its own `author`) replaces them. Unchanged list when nothing is labelled. */
export function withLiveAuthors(messages: ChatMessage[], authors: ReadonlyMap<string, ChatAuthor>): ChatMessage[] {
  if (!authors.size) return messages;
  let changed = false;
  const out = messages.map((m) => {
    if (!m.live || m.role !== "assistant" || m.author !== undefined || !m.runId) return m;
    const author = authors.get(m.runId);
    if (!author) return m;
    changed = true;
    return { ...m, author };
  });
  return changed ? out : messages;
}

/** "Castor", "Castor and Hypnos", "Castor, Hypnos and Ada". */
export function namesList(names: readonly string[]): string {
  if (names.length <= 1) return names[0] || "";
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/** The sentence above the composer once the mentions were asked: what was refused (409/404), who
 * Stop kept from being asked, or that the answers show when the current reply ends; null when there
 * is nothing to say. */
export function mentionsSummary(outcome: MentionsOutcome, options: { replyStillRunning: boolean }): string | null {
  const parts = outcome.skipped.map((s) => s.refusal.sentence);
  if (outcome.stopped?.refusal.kind === "unavailable") parts.push(outcome.stopped.refusal.sentence);
  if (outcome.aborted && outcome.unasked.length) parts.push(`Stopped before asking ${namesList(outcome.unasked.map((m) => m.name))}.`);
  const answered = outcome.answered.map((a) => a.mention.name);
  if (answered.length && options.replyStillRunning)
    parts.push(`${namesList(answered)} answered; shown when the current reply ends.`);
  return parts.length ? parts.join(" ") : null;
}

/** A turn the gateway recorded for an entity's reply (its `GET /runs/{id}/input_data` or run). */
export function isMentionTurn(value: unknown): boolean {
  return text(record(value)?.workflow_id) === MENTION_WORKFLOW_ID;
}

/**
 * The conversation's workflow turn when the loaded turn is a mention reply: the newest session turn
 * at or before `loadedRunId` that is not one (the runtime lists the turns oldest first, as the
 * kit's replay view reads them). Null when the conversation holds only mention replies.
 */
export function conversationWorkflowTurn(turns: unknown, loadedRunId: string): string | null {
  const rows = Array.isArray(turns) ? turns.map(record).filter((row): row is Row => row !== null) : [];
  const at = rows.findIndex((row) => text(row.run_id) === loadedRunId);
  const before = at >= 0 ? rows.slice(0, at + 1) : rows;
  for (let i = before.length - 1; i >= 0; i -= 1) {
    const id = text(before[i].run_id);
    if (id && !isMentionTurn(before[i])) return id;
  }
  return null;
}
