// Operator round 6 (2026-11-11), Code web: (1) "that should not be in my prompt box… the 3 dancing
// dots in the chat itself, where the AI would answer, and with its @entity_name answering": each
// mentioned entity's answer on its way is a bubble in the thread (its orb and name, three dots), never
// a line in the composer; it is replaced by the answer, not duplicated. (2) "my message and then both
// of their messages": each entity's mention turn repeats the person's words (`repeat_of`); the words
// show once.
import { readFileSync } from "node:fs";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { type ChatMessage } from "@abstractframework/panel-chat";
import { WorkflowChat } from "../vendor/abstractchat/conversation.js";
import { foldRepeatedTurns, pendingMentionsFor, PENDING_MENTION_ID, sendMentions, withPendingMentions } from "./mentions";

const LOOK = { style: "watch-halo", color: "#336699" };
const castor = { id: "castor", name: "Castor", kind: "entity" as const, index: 0, start: 4 };
const nerin = { id: "nerin", name: "Nerin", kind: "entity" as const, index: 1, start: 12 };
const agent = { id: "agent", name: "Basic agent", kind: "agent" as const, index: 2, start: 20 };
const app = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");

/** The session as the views show it after "hey @Castor @Nerin": each mention turn (m1, m2) is the
 * person's words again (repeat_of the recorded turn p) and the entity's answer. */
const mirrored: ChatMessage[] = [
  { id: "u1", role: "user", kind: "prompt", runId: "m1", content: "hey @Castor @Nerin" },
  { id: "a1", role: "assistant", kind: "answer", runId: "m1", content: "Castor here.", author: { kind: "entity", name: "Castor", entity_id: null, identity_ref: null, avatar: LOOK } },
  { id: "u2", role: "user", kind: "prompt", runId: "m2", content: "hey @Castor @Nerin" },
  { id: "a2", role: "assistant", kind: "answer", runId: "m2", content: "Nerin here.", author: { kind: "entity", name: "Nerin", entity_id: null, identity_ref: null, avatar: null } },
];

describe("the person's words show once before both answers", () => {
  it("two mirrors of a turn not on screen: one user bubble, then both answers in order", () => {
    const out = foldRepeatedTurns(mirrored, new Map([["m1", "p"], ["m2", "p"]]));
    expect(out.map((m) => `${m.role}:${m.id}`)).toEqual(["user:u1", "assistant:a1", "assistant:a2"]);
  });

  it("the turn itself on screen: every mirror folds; two messages: each keeps its words once", () => {
    const own: ChatMessage = { id: "u0", role: "user", kind: "prompt", runId: "p", content: "hey @Castor @Nerin" };
    expect(foldRepeatedTurns([own, ...mirrored], new Map([["m1", "p"], ["m2", "p"]])).filter((m) => m.role === "user").map((m) => m.id)).toEqual(["u0"]);
    const later: ChatMessage[] = [
      { id: "u3", role: "user", kind: "prompt", runId: "m3", content: "and now?" },
      { id: "a3", role: "assistant", kind: "answer", runId: "m3", content: "Still here." },
    ];
    const out = foldRepeatedTurns([...mirrored, ...later], new Map([["m1", "p"], ["m2", "p"], ["m3", "q"]]));
    expect(out.filter((m) => m.role === "user").map((m) => m.id)).toEqual(["u1", "u3"]);
    expect(foldRepeatedTurns(mirrored, new Map())).toBe(mirrored);
  });
});

describe("an answer on its way is a bubble with its dots, not a line in the composer", () => {
  const pending = pendingMentionsFor("msg-1", [castor, nerin, castor, agent], (m) => (m.id === "castor" ? LOOK : null));

  it("one per entity (each once, the agent's chip rode the start), named as its answer will be", () => {
    expect(pending.map((p) => [p.key, p.author.name, p.author.avatar, p.runId])).toEqual([["msg-1:0", "Castor", LOOK, null], ["msg-1:1", "Nerin", null, null]]);
    const out = withPendingMentions([], pending);
    expect(out.map((m) => [m.id, m.role, m.author?.name, m.live?.label, m.content])).toEqual([
      [`${PENDING_MENTION_ID}msg-1:0`, "assistant", "Castor", "answering", ""],
      [`${PENDING_MENTION_ID}msg-1:1`, "assistant", "Nerin", "answering", ""],
    ]);
  });

  it("replaced, not duplicated, when its answer's turn shows a reply", () => {
    const answered = [{ ...pending[0], runId: "m1" }, pending[1]];
    const out = withPendingMentions(mirrored.slice(0, 2), answered);
    expect(out.map((m) => m.id)).toEqual(["u1", "a1", `${PENDING_MENTION_ID}msg-1:1`]);
    expect(withPendingMentions(mirrored, [{ ...pending[0], runId: "m1" }, { ...pending[1], runId: "m2" }])).toBe(mirrored);
  });

  it("the thread draws its author and its three dots", () => {
    const html = renderToStaticMarkup(
      <WorkflowChat draft="" onDraftChange={() => {}} onSend={() => {}} messages={withPendingMentions([], pending.slice(0, 1))}
        messageProps={{ renderFooter: () => null }} />,
    );
    expect(html).toContain("Castor");
    expect(html.match(/class="pc-working-dots"/g)?.length).toBe(1);
    expect(html).toContain("answering");
  });

  it("sendMentions names each answer's turn as it lands (not a refusal)", async () => {
    const landed: string[] = [];
    await sendMentions([castor, nerin], async (m) => {
      if (m.id === "nerin") throw Object.assign(new Error("busy"), { status: 409 });
      return { run_id: "m1" };
    }, { onAnswered: (m, runId) => landed.push(`${m.id}:${runId}`) });
    expect(landed).toEqual(["castor:m1"]);
  });

  it("a refusal that does not stop the run takes its bubble away at once, before the next entity is asked (review F3)", async () => {
    const events: string[] = [];
    await sendMentions([castor, nerin], async (m) => {
      events.push(`ask:${m.id}`);
      if (m.id === "castor") throw Object.assign(new Error("busy"), { status: 409 });
      return { run_id: "m2" };
    }, {
      onSkipped: (m, refusal) => events.push(`skip:${m.id}:${refusal.kind}`),
      onAnswered: (m) => events.push(`answer:${m.id}`),
    });
    expect(events).toEqual(["ask:castor", "skip:castor:elsewhere", "ask:nerin", "answer:nerin"]);
    expect(app).toMatch(/onSkipped: \(mention, refusal\) => \{\s*forThis\(\(entries\) => entries\.filter\(\(e\) => !\(e\.mention\.id === mention\.id && e\.mention\.index === mention\.index\)\)\);\s*if \(here\(\)\) setMentionStatus\(\{ sessionId, text: refusal\.sentence \}\);/);
  });

  it("the app: no 'is answering' line, the bubbles in the thread, their dots in the shared body, cleared on follow", () => {
    expect(app).not.toContain("is answering…");
    expect(app).toContain("setPendingMentions({ sessionId, entries: pendingMentionsFor(message, mentions, (m) => cards?.get(m.id)?.avatar) });");
    expect(app).toContain("forThis((entries) => entries.filter((e) => e.runId));");
    expect(app).toContain("pendingMentions?.sessionId === session.sessionId ? pendingMentions.entries : [],");
    expect(app).toContain("if (String(message.id || \"\").startsWith(PENDING_MENTION_ID)) return null;");
    expect(app).toMatch(/setFollowTurn\(null\);\s*\/\/[^\n]*\n\s*setPendingMentions\(\(now\) => \(now && now\.sessionId === followTurn\.sessionId \? null : now\)\);/);
  });
});
