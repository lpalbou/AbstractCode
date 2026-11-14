// Mentions routing (operator 2026-11-10): the `@` list offers the conversation's agent (`agent`), a
// message whose chips name entities and not the agent starts no agent run (`agent: "not_addressed"`
// with the recorded turn's `turn_id`, which its mentions repeat), no line above the composer says so
// (operator 2026-11-10 23:02: "do not add the text above… remove that"), and such a turn reads the
// same live and after a reload (`agent_addressed: false`).
import { readFileSync } from "node:fs";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkflowChat } from "../vendor/abstractchat/conversation.js";
import { conversationRestore } from "./conversation_restore";
import { createWorkflowTransport } from "./session_transport";
import { StreamGate } from "./stream_gate";
import {
  AGENT_MENTION_ID,
  agentCandidate,
  agentNotAddressed,
  conversationWorkflowTurn,
  entityMentions,
  filterMentionCandidates,
  isSideTurn,
  mentionRepeatOf,
  mentionRequestBody,
  mentionRoster,
  mentionsSummary,
  noteUnaddressed,
  noteUnaddressedFrame,
  sendMentions,
  startRunMentions,
  startedTurn,
} from "./mentions";
import type { WorkflowDefinition } from "./catalog";

afterEach(() => vi.unstubAllGlobals());

const castor = { id: "castor", name: "Castor", kind: "entity" as const, index: 0 };
const hypnos = { id: "hypnos", name: "Hypnos", kind: "entity" as const, index: 2 };
const agent = { id: AGENT_MENTION_ID, name: "Basic agent", kind: "agent" as const, index: 1 };

describe("the @ list offers the conversation's agent", () => {
  it("first, named as its workflow ('Agent' without a name), kind agent, id agent", () => {
    const roster = mentionRoster("Basic agent", [{ id: "castor", name: "Castor", kind: "entity" }]);
    expect(roster).toEqual([
      { id: "agent", name: "Basic agent", kind: "agent" },
      { id: "castor", name: "Castor", kind: "entity" },
    ]);
    expect(agentCandidate("  ").name).toBe("Agent");
    expect(AGENT_MENTION_ID).toBe("agent");
    // Its name or its id follows the letters typed after @.
    expect(filterMentionCandidates(roster, "bas").map((c) => c.id)).toEqual(["agent"]);
    expect(filterMentionCandidates(roster, "agent").map((c) => c.id)).toEqual(["agent"]);
  });
  it("no chip gets a first-pick sentence, the agent's or an entity's (operator 2026-11-10 23:02)", () => {
    const lib = readFileSync(new URL("./mentions.ts", import.meta.url), "utf8");
    expect(lib).not.toMatch(/will read this conversation|stays out of this message|export function (mentionNoticeFor|agentHint)/);
  });
});

describe("the start names every chip in pick order, the agent's as `agent`", () => {
  it("entities and the agent by pick order, each once; the mention door asks only the entities", async () => {
    expect(startRunMentions([hypnos, castor, agent])).toEqual(["castor", "agent", "hypnos"]);
    expect(startRunMentions([agent])).toEqual(["agent"]);
    expect(startRunMentions([castor])).toEqual(["castor"]);
    expect(entityMentions([castor, agent, hypnos])).toEqual([castor, hypnos]);
    const asked: string[] = [];
    const outcome = await sendMentions([castor, agent, hypnos], async (m) => {
      asked.push(m.id);
      return { run_id: `turn-${m.id}` };
    });
    expect(asked).toEqual(["castor", "hypnos"]);
    expect(outcome.answered.map((a) => a.mention.id)).toEqual(["castor", "hypnos"]);
  });
});

describe("no line above the composer (operator 2026-11-10 23:02: \"remove that\")", () => {
  const app = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
  it("the chat gets no first-pick sentence and no agent line; the last send's sentence alone", () => {
    expect(app).not.toMatch(/mentionNotice=|agentHint|composerAgentHint|useComposerChipKinds/);
    expect(app).toContain("mentionStatus={mentionStatus?.sessionId === session.sessionId ? mentionStatus.text : null}");
  });
  it("an entities-only message's chips show no row; a send's outcome still shows", () => {
    const quiet = renderToStaticMarkup(
      <WorkflowChat draft="@Castor hi" onDraftChange={() => {}} onSend={() => {}} messages={[]} mentionCandidates={async () => []} mentionStatus={null} />,
    );
    expect(quiet).not.toContain("pc-mention-notice");
    const outcome = renderToStaticMarkup(
      <WorkflowChat draft="" onDraftChange={() => {}} onSend={() => {}} messages={[]} mentionCandidates={async () => []} mentionStatus="Castor is in another conversation." />,
    );
    expect(outcome).toMatch(/pc-mention-notice pc-mention-notice--status[^>]*>.*Castor is in another conversation\./);
  });
});

describe("a start the agent was not addressed in", () => {
  it("`agent: not_addressed` names the recorded turn (`turn_id`): the mentions repeat it, never the run", () => {
    const skipped = startedTurn({ run_id: "run-9", turn_id: "turn-1", agent: "not_addressed" });
    expect(skipped).toEqual({ runId: "run-9", turn: "turn-1", agentAddressed: false });
    expect(mentionRepeatOf({ started: skipped.turn })).toBe("turn-1");
    expect(mentionRequestBody("@Castor hi", castor, mentionRepeatOf({ started: skipped.turn }), "turn-1")).toEqual({
      entity: "castor",
      text: "@Castor hi",
      mention_index: 0,
      repeat_of: "turn-1",
      mention_id: "turn-1:castor:0",
    });
    expect(startedTurn({ turn_id: "turn-1", agent: "not_addressed" })).toEqual({ runId: "", turn: "turn-1", agentAddressed: false });
  });
  it("a started run is the turn, as before; a missing id throws, never a guess", () => {
    expect(startedTurn({ run_id: "r1", agent: "started" })).toEqual({ runId: "r1", turn: "r1", agentAddressed: true });
    expect(startedTurn({ run_id: "r1" })).toEqual({ runId: "r1", turn: "r1", agentAddressed: true });
    expect(() => startedTurn({ run_id: "r1", agent: "not_addressed" })).toThrow("did not name the turn");
    expect(() => startedTurn({ agent: "started" })).toThrow("did not return a run identifier");
  });
  it("no reply is said to be running when the answers arrive", () => {
    const outcome = { answered: [{ mention: castor, runId: "m1" }], skipped: [], stopped: null, unsent: [], aborted: false, unasked: [] };
    expect(mentionsSummary(outcome, { replyStillRunning: true })).toBe("Castor answered; shown when the current reply ends.");
    expect(mentionsSummary(outcome, { replyStillRunning: false })).toBeNull();
  });
});

describe("the turn reads the same after a reload (`agent_addressed: false`, as served)", () => {
  // AbstractGateway replay_view.py served_json: the flag sits at the TOP of the turn's own view (its
  // root), absent otherwise, never inside `messages`; GET /runs and the session turns carry it on the row.
  const personView = {
    version: "replay_view_v1", root_run_id: "p1", session_id: "s1", status: "completed", agent_addressed: false, cursor: { token: "p1:0~x", records: 0, runs: { p1: 0 } },
    messages: [{ id: "p1:input", role: "user", kind: "prompt", text: "@Castor is it ready?", run_id: "p1" }],
  };
  const mentionView = {
    version: "replay_view_v1", root_run_id: "m1", session_id: "s1", status: "completed", cursor: { token: "m1:0~x", records: 0, runs: { m1: 0 } },
    messages: [
      { id: "m1:input", role: "user", kind: "prompt", text: "@Castor is it ready?", run_id: "m1" },
      { id: "m1:final", role: "assistant", kind: "final", text: "It is.", run_id: "m1" },
    ],
  };
  it("the marker on a run or a session turn's row; on a view, at its top level by its root; never on a message", () => {
    expect(agentNotAddressed({ run_id: "p1", agent_addressed: false })).toBe(true);
    expect(agentNotAddressed({ run_id: "a1", agent_addressed: true })).toBe(false);
    expect(agentNotAddressed({ run_id: "a1" })).toBe(false);
    expect(agentNotAddressed(null)).toBe(false);
    const into = new Set<string>();
    expect(noteUnaddressed(personView, into)).toBe(true);
    expect([...into]).toEqual(["p1"]);
    expect(noteUnaddressed(personView, into)).toBe(false);
    expect(noteUnaddressed(mentionView, into)).toBe(false);
    // A flag on a message is not what the gateway serves: never read.
    const onMessage = { ...mentionView, messages: [{ ...mentionView.messages[0], run_id: "p9", agent_addressed: false }] };
    expect(noteUnaddressed(onMessage, new Set())).toBe(false);
    const fromFrame = new Set<string>();
    expect(noteUnaddressedFrame({ event: "view", data: JSON.stringify(personView) }, fromFrame)).toBe(true);
    expect([...fromFrame]).toEqual(["p1"]);
    expect(noteUnaddressedFrame({ event: "done", data: "{}" }, new Set())).toBe(false);
  });
  it("after a reload the app's workflow transport notes it from the turn's own view", async () => {
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      const view = String(url).includes("/p1/") ? personView : mentionView;
      return new Response(JSON.stringify(view), { status: 200, headers: { ETag: `"${view.cursor.token}"` } });
    }));
    const unaddressed = new Set<string>();
    const transport = createWorkflowTransport({ onDeltaError() {}, streams: new StreamGate(), unaddressed });
    await transport.replayView!.getView("m1");
    expect([...unaddressed]).toEqual([]);
    await transport.replayView!.getView("p1");
    expect([...unaddressed]).toEqual(["p1"]);
  });
  it("such a turn is no workflow turn: the conversation continues with its agent turn's workflow", () => {
    const turns = [
      { run_id: "a1", workflow_id: "code-agent@1.0.0:main" },
      { run_id: "p1", workflow_id: "person-turn", agent_addressed: false },
      { run_id: "m1", workflow_id: "entity-mention" },
    ];
    expect(isSideTurn({ workflow_id: "person-turn" })).toBe(true);
    expect(isSideTurn({ workflow_id: "code-agent@1.0.0:main", agent_addressed: false })).toBe(true);
    expect(isSideTurn({ workflow_id: "code-agent@1.0.0:main" })).toBe(false);
    expect(conversationWorkflowTurn(turns, "m1")).toBe("a1");
    expect(conversationWorkflowTurn(turns, "p1")).toBe("a1");
    expect(conversationWorkflowTurn(turns.slice(1), "m1")).toBeNull();
    const workflow = { id: "private:code-agent@1.0.0:main", workflowId: "code-agent@1.0.0:main", bundleId: "code-agent", bundleVersion: "1.0.0", flowId: "main", name: "Code agent", registryScope: "private" } as WorkflowDefinition;
    const restore = conversationRestore({
      sessionId: "s1",
      runId: "p1",
      restoredInputs: { run_id: "p1", workflow_id: "person-turn", input_data: { prompt: "@Castor is it ready?" } },
      run: { workflow_id: "person-turn" },
      workflows: [workflow],
      conversationSessions: [{ sessionId: "s1" }],
      notesSessions: [],
      catalogLoading: false,
    });
    expect(restore).toEqual({ notesSession: false, readOnlyNotice: "", versionNote: "", error: "" });
  });
});
