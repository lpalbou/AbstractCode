// Mentions in the conversation (panel-chat 0.9.13, operator 2026-11-10): a message this page sent
// shows the chips its send carried, placed by `start` (the words are never read for "@"), with the
// `@` list's looks; resting on a chip shows a small card (orb, name, state sentence) from the
// `GET /entities` rows the `@` list loaded. A reloaded turn carries no mentions: no chip.
import { readFileSync } from "node:fs";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ChatMessage } from "@abstractframework/panel-chat";
import { WorkflowChat } from "../vendor/abstractchat/conversation.js";
import { mentionCard } from "./mention_card";
import { entityMentionCards, entityStateSentence, sentMessageMentions, withSentMentions } from "./mentions";

const LOOK = { style: "watch-halo", color: "#336699" };
const BODY = {
  entities: [
    { slug: "castor", name: "Castor", identity: { avatar: LOOK }, state: { state: "awake", mode: "visiting" } },
    { slug: "pollux", name: "Pollux", state: { state: "asleep" } },
    { slug: "vega", name: "Vega" },
    { slug: "ghost", error: "unreadable" },
  ],
};
const app = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");

describe("a sent message carries its chips", () => {
  it("each chip, its start on the trimmed words, the look from the @ list; the agent's chip too", () => {
    const text = "  hi @Castor and @Basic agent";
    const sent = [
      { id: "castor", name: "Castor", kind: "entity" as const, index: 0, start: 5 },
      { id: "agent", name: "Basic agent", kind: "agent" as const, index: 1, start: 17 },
    ];
    const cards = entityMentionCards(BODY);
    const shown = sentMessageMentions(text, sent, (m) => (m.kind === "entity" ? cards.get(m.id)?.avatar : null));
    for (const m of shown) expect(text.trim().slice(m.start!, m.start! + m.name.length + 1)).toBe(`@${m.name}`);
    expect(shown.map((m) => [m.id, m.avatar ?? null])).toEqual([["castor", LOOK], ["agent", null]]);
  });

  it("the run's prompt gets them (by run, not by words); other messages, a reload: untouched", () => {
    const mentions = [{ id: "castor", name: "Castor", kind: "entity" as const, index: 0, start: 3 }];
    const view: ChatMessage[] = [
      { id: "p1", role: "user", kind: "prompt", runId: "r1", content: "hi @Castor" },
      { id: "a1", role: "assistant", kind: "answer", runId: "r1", content: "@Castor said hi" },
      { id: "p2", role: "user", kind: "prompt", runId: "r2", content: "hi @Castor" },
    ];
    const out = withSentMentions(view, new Map([["r1", mentions]]));
    expect(out.map((m) => m.mentions ?? null)).toEqual([mentions, null, null]);
    expect(withSentMentions(view, new Map())).toBe(view);
  });

  it("the start sets them on the optimistic message and remembers them by run; reset with the identity", () => {
    expect(app).toContain("const shown = sentMessageMentions(text, mentions, (m) => (m.kind === \"entity\" ? cards?.get(m.id)?.avatar : null));");
    expect(app.match(/if \(shown\.length\) sentMentions\.current\.set\(started\.runId, shown\);/g)?.length).toBe(2);
    expect(app.match(/\.\.\.mentionsField/g)?.length).toBe(2);
    expect(app).toContain("withSentMentions(foldRepeatedTurns(baseMessages, repeatTurns.current), sentMentions.current)");
    expect(app).toMatch(/entityCards\.current = null;\s*sentMentions\.current = new Map\(\);/);
  });

  it("the bubble draws the kit's chip; without mentions, none", () => {
    const mentions = [{ id: "castor", name: "Castor", kind: "entity" as const, index: 0, start: 3, avatar: LOOK }];
    const chat = (messages: ChatMessage[]) =>
      renderToStaticMarkup(<WorkflowChat draft="" onDraftChange={() => {}} onSend={() => {}} messages={messages} messageProps={{ renderMentionCard: () => null }} />);
    expect(chat([{ id: "m1", role: "user", content: "hi @Castor", mentions }])).toContain("pc-mention-chip--message");
    const plain = chat([{ id: "m1", role: "user", content: "hi @Castor" }]);
    expect(plain).not.toContain("pc-mention-chip");
    expect(plain).toContain("@Castor");
  });
});

describe("the card on a chip", () => {
  it("the state sentence from the row (no pronoun)", () => {
    expect(entityStateSentence("Castor", { state: "awake", mode: "visiting" })).toBe("Castor is in a conversation.");
    expect(entityStateSentence("Pollux", { state: "asleep" })).toBe("Pollux is asleep.");
    expect(entityStateSentence("Vega", undefined)).toBe("");
  });

  it("an entity the loaded list holds: orb, name, sentence; the agent or a stranger: no card", () => {
    const cards = entityMentionCards(BODY);
    expect([...cards.keys()]).toEqual(["castor", "pollux", "vega"]);
    const out = renderToStaticMarkup(<>{mentionCard({ id: "castor", name: "Castor", kind: "entity", index: 0, start: 0 }, cards)}</>);
    expect(out).toContain('class="code-mention-card"');
    expect(out).toContain("Castor is in a conversation.");
    expect(out).toContain("pc-author-mark");
    expect(mentionCard({ id: "agent", name: "Basic agent", kind: "agent", index: 0, start: 0 }, cards)).toBeNull();
    expect(mentionCard({ id: "nobody", name: "Nobody", kind: "entity", index: 0, start: 0 }, cards)).toBeNull();
    expect(mentionCard({ id: "castor", name: "Castor", kind: "entity", index: 0, start: 0 }, null)).toBeNull();
    expect(app).toContain("renderMentionCard: (mention) => mentionCard(mention, entityCards.current?.identity === identity ? entityCards.current.cards : null),");
  });
});
