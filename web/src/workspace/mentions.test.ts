// @entity mentions (contract 2026-11-08): the roster, the order of the asks, what each refusal
// means for the rest of the send, and which turn a conversation restores its workflow from.
import { describe, expect, it } from "vitest";
import { GatewayHttpError } from "../lib/gateway_client";
import { conversationRestore, WORKFLOW_GONE } from "./conversation_restore";
import {
  MENTIONS_UNAVAILABLE,
  conversationWorkflowTurn,
  entityCandidates,
  filterMentionCandidates,
  isMentionTurn,
  mentionNoticeFor,
  mentionRefusal,
  mentionRequestBody,
  mentionsSummary,
  sendMentions,
} from "./mentions";
import type { WorkflowDefinition } from "./catalog";

const httpError = (status: number, body: unknown) =>
  new GatewayHttpError(typeof (body as { detail?: unknown })?.detail === "string" ? (body as { detail: string }).detail : `Gateway request failed (${status})`, {
    status,
    body_text: typeof body === "string" ? body : JSON.stringify(body),
  });
const castor = { id: "castor", name: "Castor", kind: "entity" as const, index: 0 };
const hypnos = { id: "hypnos", name: "Hypnos", kind: "entity" as const, index: 1 };
const ada = { id: "ada", name: "Ada", kind: "entity" as const, index: 2 };

describe("the @ list: GET /entities", () => {
  const body = {
    entities: [
      { slug: "hypnos", name: "Hypnos", identity: { avatar: { style: "watch-halo", color: "#7c5cff", watch_code: "h" } } },
      { slug: "castor", name: "Castor", identity: null },
      { slug: "broken", error: "unreadable manifest: x" },
      { slug: "castor", name: "Castor again" },
      { slug: "nerin", identity: { avatar: { style: null, color: "#2a9d8f" } } },
      { name: "no slug" },
    ],
    reads: { tile: {} },
  };
  it("offers each readable home once, by name, with its look; the slug is what a mention sends", () => {
    expect(entityCandidates(body)).toEqual([
      { id: "castor", name: "Castor", kind: "entity" },
      { id: "hypnos", name: "Hypnos", kind: "entity", avatar: { style: "watch-halo", color: "#7c5cff" } },
      { id: "nerin", name: "nerin", kind: "entity", avatar: { style: null, color: "#2a9d8f" } },
    ]);
    expect(entityCandidates({})).toEqual([]);
    expect(entityCandidates(null)).toEqual([]);
  });
  it("follows the letters typed after @: names starting with them first, then names containing them", () => {
    const all = entityCandidates({ entities: [{ slug: "castor", name: "Castor" }, { slug: "acast", name: "Acast" }, { slug: "hypnos", name: "Hypnos" }] });
    expect(filterMentionCandidates(all, "").map((c) => c.id)).toEqual(["acast", "castor", "hypnos"]);
    expect(filterMentionCandidates(all, "CAS").map((c) => c.id)).toEqual(["castor", "acast"]);
    expect(filterMentionCandidates(all, "zz")).toEqual([]);
  });
  it("leaves out an entity the gateway refused (404: this account cannot mention it)", () => {
    const all = entityCandidates({ entities: [{ slug: "castor", name: "Castor" }, { slug: "hypnos", name: "Hypnos" }] });
    expect(filterMentionCandidates(all, "", new Set(["castor"])).map((c) => c.id)).toEqual(["hypnos"]);
  });
  it("says on the first pick that the entity reads the conversation", () => {
    expect(mentionNoticeFor({ name: "Castor" })).toBe("Castor will read this conversation's history.");
  });
});

describe("what a refused mention means", () => {
  it("409 in another conversation: the contract's sentence", () => {
    expect(mentionRefusal(httpError(409, { detail: "Castor is in another conversation (session s2). One conversation at a time…", code: "in_another_conversation", session_id: "s2" }), "Castor")).toEqual({
      kind: "elsewhere",
      sentence: "Castor is in another conversation.",
    });
  });
  it("the visit door's own 409s pass through with their sentence, handled the same way", () => {
    expect(mentionRefusal(httpError(409, { detail: "Castor is asleep.", code: "entity_asleep" }), "Castor")).toEqual({ kind: "elsewhere", sentence: "Castor is asleep." });
  });
  it("404 with FastAPI's unmatched-route body is an older gateway; any other 404 is this entity", () => {
    expect(mentionRefusal(httpError(404, { detail: "Not Found" }), "Castor")).toEqual({ kind: "unavailable", sentence: MENTIONS_UNAVAILABLE });
    expect(mentionRefusal(httpError(404, { detail: "Entity 'castor' not found" }), "Castor")).toEqual({ kind: "not_offerable", sentence: "Castor cannot be mentioned here." });
    // A body with more than the route answer is the mention door's own.
    expect(mentionRefusal(httpError(404, { detail: "Not Found", code: "entity_missing" }), "Castor").kind).toBe("not_offerable");
  });
  it("503 entity_cannot_answer: the gateway's sentence", () => {
    expect(mentionRefusal(httpError(503, { detail: "Castor cannot answer: no mind resolved", code: "entity_cannot_answer" }), "Castor")).toEqual({
      kind: "cannot_answer",
      sentence: "Castor cannot answer: no mind resolved",
    });
  });
  it("anything else is a failure that names the entity", () => {
    expect(mentionRefusal(httpError(500, { detail: "boom" }), "Castor")).toEqual({ kind: "failed", sentence: "Castor could not be asked: boom." });
    expect(mentionRefusal(new TypeError("Failed to fetch"), "Castor")).toEqual({ kind: "failed", sentence: "Castor could not be asked: Failed to fetch." });
  });
});

describe("asking the mentions of one message", () => {
  it("one request per mention, in mention_index order, each after the previous returned", async () => {
    const calls: string[] = [];
    let release: (() => void) | null = null;
    const post = (m: typeof castor) => {
      calls.push(m.id);
      return new Promise((resolve) => {
        release = () => resolve({ run_id: `turn-${m.id}` });
      });
    };
    const done = sendMentions([hypnos, castor], post);
    await Promise.resolve();
    expect(calls).toEqual(["castor"]);
    release!();
    await new Promise((r) => setTimeout(r, 0));
    expect(calls).toEqual(["castor", "hypnos"]);
    release!();
    const outcome = await done;
    expect(outcome.answered.map((a) => [a.mention.id, a.runId])).toEqual([["castor", "turn-castor"], ["hypnos", "turn-hypnos"]]);
    expect(outcome.stopped).toBeNull();
  });
  it("the request body is the message as typed, the entity's id and its position", () => {
    expect(mentionRequestBody("  what do you think?  ", hypnos)).toEqual({ entity: "hypnos", text: "what do you think?", mention_index: 1 });
  });
  it("409 and an entity's 404 are said and the next one is asked; never queued", async () => {
    const calls: string[] = [];
    const outcome = await sendMentions([castor, hypnos, ada], async (m) => {
      calls.push(m.id);
      if (m.id === "castor") throw httpError(409, { detail: "x", code: "in_another_conversation" });
      if (m.id === "hypnos") throw httpError(404, { detail: "Entity 'hypnos' not found" });
      return { run_id: "turn-ada" };
    });
    expect(calls).toEqual(["castor", "hypnos", "ada"]);
    expect(outcome.skipped.map((s) => s.refusal.kind)).toEqual(["elsewhere", "not_offerable"]);
    expect(outcome.answered.map((a) => a.runId)).toEqual(["turn-ada"]);
    expect(mentionsSummary(outcome, { replyStillRunning: false })).toBe("Castor is in another conversation. Hypnos cannot be mentioned here.");
  });
  it("503 stops the rest (the message goes back to the composer)", async () => {
    const calls: string[] = [];
    const outcome = await sendMentions([castor, hypnos], async (m) => {
      calls.push(m.id);
      throw httpError(503, { detail: "Castor cannot answer: the turn produced no reply", code: "entity_cannot_answer" });
    });
    expect(calls).toEqual(["castor"]);
    expect(outcome.stopped?.refusal).toEqual({ kind: "cannot_answer", sentence: "Castor cannot answer: the turn produced no reply" });
  });
  it("an older gateway (no route) stops the rest and says so once", async () => {
    const calls: string[] = [];
    const outcome = await sendMentions([castor, hypnos], async (m) => {
      calls.push(m.id);
      throw httpError(404, { detail: "Not Found" });
    });
    expect(calls).toEqual(["castor"]);
    expect(outcome.stopped?.refusal.kind).toBe("unavailable");
    expect(mentionsSummary(outcome, { replyStillRunning: false })).toBe(MENTIONS_UNAVAILABLE);
  });
  it("Stop keeps the next ones from being asked; the one in flight is never cut", async () => {
    const abort = new AbortController();
    const calls: string[] = [];
    const outcome = await sendMentions([castor, hypnos], async (m) => {
      calls.push(m.id);
      abort.abort();
      return { run_id: `turn-${m.id}` };
    }, { signal: abort.signal });
    expect(calls).toEqual(["castor"]);
    expect(outcome.answered.map((a) => a.runId)).toEqual(["turn-castor"]);
    expect(outcome.aborted).toBe(true);
  });
  it("answers waiting for the current reply are said; nothing is said when there is nothing to say", async () => {
    const outcome = await sendMentions([castor, hypnos], async (m) => ({ run_id: `turn-${m.id}` }));
    expect(mentionsSummary(outcome, { replyStillRunning: true })).toBe("Castor and Hypnos answered; shown when the current reply ends.");
    expect(mentionsSummary(outcome, { replyStillRunning: false })).toBeNull();
  });
});

describe("a conversation whose newest turn is a mention reply", () => {
  const turns = [
    { run_id: "a1", workflow_id: "code-agent@1.0.0:main" },
    { run_id: "m1", workflow_id: "entity-mention" },
    { run_id: "a2", workflow_id: "code-agent@1.0.0:main" },
    { run_id: "m2", workflow_id: "entity-mention" },
    { run_id: "m3", workflow_id: "entity-mention" },
  ];
  it("restores its workflow from the newest agent turn at or before the loaded one", () => {
    expect(isMentionTurn({ workflow_id: "entity-mention" })).toBe(true);
    expect(isMentionTurn({ workflow_id: "code-agent@1.0.0:main" })).toBe(false);
    expect(conversationWorkflowTurn(turns, "m3")).toBe("a2");
    expect(conversationWorkflowTurn(turns, "m1")).toBe("a1");
    expect(conversationWorkflowTurn([{ run_id: "m1", workflow_id: "entity-mention" }], "m1")).toBeNull();
    expect(conversationWorkflowTurn(undefined, "m1")).toBeNull();
  });

  const workflow: WorkflowDefinition = { id: "private:code-agent@1.0.0:main", workflowId: "code-agent@1.0.0:main", bundleId: "code-agent", bundleVersion: "1.0.0", flowId: "main", name: "Code agent", registryScope: "private" } as WorkflowDefinition;
  const base = { sessionId: "s1", runId: "m3", workflows: [workflow], conversationSessions: [{ sessionId: "s1" }], notesSessions: [], catalogLoading: false };
  it("continues with the agent turn's workflow, not the mention reply's", () => {
    const restore = conversationRestore({ ...base, restoredInputs: { run_id: "a2", workflow_id: "code-agent@1.0.0:main", input_data: {} }, run: { workflow_id: "entity-mention" } });
    expect(restore.workflow?.id).toBe(workflow.id);
    expect(restore.error).toBe("");
  });
  it("a conversation of mention replies only restores no workflow and says nothing is gone", () => {
    const restore = conversationRestore({ ...base, restoredInputs: { run_id: "m3", workflow_id: "entity-mention", input_data: { prompt: "hi" } }, run: { workflow_id: "entity-mention" } });
    expect(restore).toEqual({ notesSession: false, readOnlyNotice: "", versionNote: "", error: "" });
  });
  it("an agent turn whose workflow is gone still says so", () => {
    const restore = conversationRestore({ ...base, workflows: [], restoredInputs: { run_id: "a2", workflow_id: "code-agent@1.0.0:main", input_data: {} }, run: { workflow_id: "entity-mention" } });
    expect(restore.error).toBe(WORKFLOW_GONE);
  });
});
