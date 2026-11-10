// `@entity` mentions from Code's chat (contract 2026-11-08, untracked CONTRACT-mentions.md):
// the person picks a name from the kit's `@` list (fed by `GET /entities`); on send, the person's
// turn starts as always, then each picked entity is asked IN ORDER through
// `POST /sessions/{session_id}/mentions {entity, text, mention_index}`, the next after the previous
// returned. Each reply is a turn of the session the gateway records (workflow `entity-mention`,
// with its `author`), so the conversation shows it from the gateway's own history after a reload too.
//
// One answer per mention (AbstractGateway f0f23bb0, `mention_id`): each mention carries its own id,
// `<message>:<entity>:<mention_index>`, the message being the person's turn (`repeat_of`), or an id
// minted once for the message when it has none. A resend of the message (the same words after an
// entity could not answer, a timeout or a lost answer) names the same ids, so the gateway gives back
// the first answer instead of asking the entity again; a new message gets new ids.
//
// Only a message that started a turn has one to name (`mentionRepeatOf`): guidance, or an answer to
// the running turn's question, starts none, so its mentions name no `repeat_of`.
//
// Who the message is for (operator 2026-11-10): the `@` list also offers the conversation's own
// agent (`agentCandidate`, sent as the reserved slug `agent` in the start's `mentions`). A message
// whose chips name entities and not the agent does not reach the agent: the gateway records the
// person's turn and starts no agent run (`agent: "not_addressed"` with that turn's `turn_id`), and the
// entities answer through the mention door with `repeat_of` = that turn. With the agent's chip, or
// with no chip, the agent answers as before.
//
// DOM-free: app.tsx wires these rules; mentions.test.ts checks them. Nothing here reads `@` out
// of the message text (no-heuristics ruling): a mention is only ever a picked chip.
import { uniqueMentions, type ChatAuthor, type ChatMentionCandidate, type ChatMentionSent } from "../vendor/abstractchat/conversation.js";
import type { ChatMessage } from "@abstractframework/panel-chat";

/** The sentence an older gateway gets (no mentions route): shown, chips disabled, never silent. */
export const MENTIONS_UNAVAILABLE = "This gateway does not offer mentions yet.";

/** The workflow id of a turn the gateway recorded for an entity's reply to a mention. */
export const MENTION_WORKFLOW_ID = "entity-mention";

/** The workflow id of the person's turn the gateway recorded when the agent was not addressed. */
export const PERSON_TURN_WORKFLOW_ID = "person-turn";

/** The reserved slug naming the conversation's own agent in the start's `mentions` (no entity can
 * be called `agent`: the gateway refuses that slug). */
export const AGENT_MENTION_ID = "agent";

/** The `@` list's row for the conversation's agent: the workflow's display name (e.g. "Basic agent";
 * "Agent" when it has none), kind agent, id `agent`. */
export function agentCandidate(label: string | null | undefined): ChatMentionCandidate {
  return { id: AGENT_MENTION_ID, name: String(label ?? "").trim() || "Agent", kind: "agent" };
}

/** The `@` list: the conversation's agent first, then the entities (`entityCandidates`). */
export function mentionRoster(agentLabel: string | null | undefined, entities: readonly ChatMentionCandidate[]): ChatMentionCandidate[] {
  return [agentCandidate(agentLabel), ...entities.filter((c) => c.kind === "entity")];
}

/** The mentions the mention door asks: the entities only (the agent's chip rides the start). */
export function entityMentions<M extends Pick<ChatMentionSent, "kind">>(mentions: readonly M[]): M[] {
  return mentions.filter((m) => m.kind === "entity");
}

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
 * Ask each entity mention in order (`index` = `mention_index`), each after the previous returned, so
 * the second entity reads the first one's answer; an entity named twice is asked once (its first
 * index); the agent's chip is never asked here (it rides the start). A request already sent is never
 * cut (the entity answers anyway and the gateway records the turn): Stop only keeps the next ones
 * from being asked.
 */
export async function sendMentions(
  mentions: readonly ChatMentionSent[],
  post: MentionPost,
  options: { signal?: AbortSignal; onAsking?: (mention: ChatMentionSent) => void } = {},
): Promise<MentionsOutcome> {
  const outcome: MentionsOutcome = { answered: [], skipped: [], stopped: null, unsent: [], aborted: false, unasked: [] };
  const ordered = uniqueMentions(entityMentions(mentions).sort((a, b) => a.index - b.index));
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

/**
 * `mentions` of `POST /runs/start` (AbstractGateway c26d38a2): every chip of the message, by the id
 * the mention door is sent (`entity`, the chip's slug; `agent` for the agent's chip), in the order
 * they were picked (`index`), each once. Entities without the agent: the gateway starts no agent
 * run (`agent: "not_addressed"`). The agent with entities: the gateway tells the agent the mentioned
 * entities answer for themselves, so it answers only the part addressed to it, or says nothing (an
 * empty reply, which the gateway serves as no answer, so the kit shows no bubble for it). Empty for
 * a message without chips: the start then carries no `mentions` key (`startRunBody`).
 */
export function startRunMentions(mentions: readonly ChatMentionSent[]): string[] {
  return uniqueMentions([...mentions].sort((a, b) => a.index - b.index)).map((m) => (m.kind === "agent" ? AGENT_MENTION_ID : m.id));
}

/** What `POST /runs/start` answered about the message's turn. */
export type StartedTurn = {
  /** The agent's run, which the conversation follows; "" when the agent was not addressed and the
   * gateway names no run for the recorded turn. */
  runId: string;
  /** The turn the message's words are: what its mentions name as `repeat_of` (`turn_id` when the
   * agent was not addressed, else the agent's run). */
  turn: string;
  /** False for `agent: "not_addressed"`: no agent run started (no spinner, no answer to wait for). */
  agentAddressed: boolean;
};

/** Read a start's answer. A started run needs its `run_id`; a message the agent was not addressed in
 * needs the recorded turn's `turn_id`. Either missing throws: never a guess. */
export function startedTurn(answer: unknown): StartedTurn {
  const row = record(answer);
  const runId = text(row?.run_id);
  if (text(row?.agent) === "not_addressed") {
    const turn = text(row?.turn_id);
    if (!turn) throw new Error("The gateway did not name the turn it recorded for this message.");
    return { runId, turn, agentAddressed: false };
  }
  if (!runId) throw new Error("The gateway did not return a run identifier.");
  return { runId, turn: runId, agentAddressed: true };
}

/** A turn the agent was not asked to answer: its row says `agent_addressed: false` (`GET /runs/{id}`,
 * a session turn, a view message); absent or true is an ordinary turn. */
export function agentNotAddressed(value: unknown): boolean {
  return record(value)?.agent_addressed === false;
}

/**
 * The turns the agent was not addressed in, learned from a view (or a view delta): the person's
 * message of such a turn carries `agent_addressed: false`. Noted by run id into `into`; true when
 * something new was noted.
 */
export function noteUnaddressed(view: unknown, into: Set<string>): boolean {
  const messages = record(view)?.messages;
  if (!Array.isArray(messages)) return false;
  let noted = false;
  for (const raw of messages) {
    const m = record(raw);
    const runId = text(m?.run_id);
    if (m?.role !== "user" || !runId || !agentNotAddressed(m) || into.has(runId)) continue;
    into.add(runId);
    noted = true;
  }
  return noted;
}

/** The same on a `/view/stream` frame (`view` and `delta` carry messages; others teach nothing). */
export function noteUnaddressedFrame(frame: { event?: string; data: string }, into: Set<string>): boolean {
  if ((frame.event !== "view" && frame.event !== "delta") || !frame.data.includes('"agent_addressed"')) return false;
  try {
    return noteUnaddressed(JSON.parse(frame.data), into);
  } catch {
    return false; // the kit reports the malformed frame itself
  }
}

/** A fresh message id: "msg-" and 24 random hex digits (crypto.getRandomValues: any page). */
export function mintMessageId(): string {
  const bytes = globalThis.crypto.getRandomValues(new Uint8Array(12));
  return `msg-${Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("")}`;
}

/** The message part of its mentions' ids: the person's turn (`repeat_of`) when there is one, else an
 * id minted now. Taken once per message and held with it; a resend passes the one first used. */
export function mentionMessageKey(repeatOf: string | null | undefined, mint: () => string = mintMessageId): string {
  return String(repeatOf ?? "").trim() || mint();
}

/**
 * The turn a message's mentions name as `repeat_of` ("" = none). The gateway takes it only for the
 * caller's own turn of this session whose prompt is the mention's text (AbstractGateway
 * entity_mentions `_check_repeat_of`: both trimmed, then compared exactly), else 422
 * `repeat_of_unknown`. A message that STARTED a turn (sent, or queued and then started) names it:
 * that turn's prompt is the typed words, as the mention's text is. A message that STEERED the
 * running turn (guidance through `inject_guidance`, or an answer to its question) names none: the
 * running turn's prompt is an earlier message and the guidance travels in the steer's own field, so
 * no turn carries these words. The mention turn then keeps them as its own prompt and its
 * `mention_id`s take a key minted for the message (`mentionMessageKey("")`).
 */
export function mentionRepeatOf(sent: { started: string } | { steered: string }): string {
  return "started" in sent ? String(sent.started ?? "").trim() : "";
}

/** One mention's `mention_id`: `<message>:<entity>:<mention_index>`. An empty message key throws. */
export function mentionId(message: string, mention: Pick<ChatMentionSent, "id" | "index">): string {
  const key = String(message ?? "").trim();
  if (!key) throw new Error("A mention needs its message key (mention_id).");
  return `${key}:${mention.id}:${mention.index}`;
}

/** The request body of one mention: the message as typed, the entity's id, its position,
 * `repeat_of`, the person's own turn of this session that carries the same words (the gateway marks
 * the mention turn with it, and Code folds the repeated prompt by it), and its `mention_id` from the
 * message's key (`message`). */
export function mentionRequestBody(
  text: string,
  mention: ChatMentionSent,
  repeatOf: string | undefined,
  message: string,
): { entity: string; text: string; mention_index: number; repeat_of?: string; mention_id: string } {
  const turn = String(repeatOf ?? "").trim();
  return { entity: mention.id, text: String(text ?? "").trim(), mention_index: mention.index, ...(turn ? { repeat_of: turn } : {}), mention_id: mentionId(message, mention) };
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

/** A turn of the conversation that is not its workflow's: an entity's reply to a mention, or the
 * person's turn the gateway recorded when the agent was not addressed (`agent_addressed: false`,
 * workflow `person-turn`). Its inputs restore no workflow. */
export function isSideTurn(value: unknown): boolean {
  return isMentionTurn(value) || agentNotAddressed(value) || text(record(value)?.workflow_id) === PERSON_TURN_WORKFLOW_ID;
}

/**
 * The conversation's workflow turn when the loaded turn is a side turn (`isSideTurn`: a mention
 * reply, or a person's turn the agent was not addressed in): the newest session turn at or before
 * `loadedRunId` that is not one (the runtime lists the turns oldest first, as the kit's replay view
 * reads them). Null when the conversation holds only side turns.
 */
export function conversationWorkflowTurn(turns: unknown, loadedRunId: string): string | null {
  const rows = Array.isArray(turns) ? turns.map(record).filter((row): row is Row => row !== null) : [];
  const at = rows.findIndex((row) => text(row.run_id) === loadedRunId);
  const before = at >= 0 ? rows.slice(0, at + 1) : rows;
  for (let i = before.length - 1; i >= 0; i -= 1) {
    const id = text(before[i].run_id);
    if (id && !isSideTurn(before[i])) return id;
  }
  return null;
}
