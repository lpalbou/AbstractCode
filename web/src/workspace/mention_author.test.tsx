// "Who answered" on Code's left bubble and the older-gateway sentence, through the vendored
// panel-chat WorkflowChat the app mounts (contract 2026-11-08): `author` from the gateway's turn
// surfaces draws the name, the kind tag and the mark; `author` null or absent is today's card.
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { WorkflowChat } from "../vendor/abstractchat/conversation.js";
import { MENTIONS_UNAVAILABLE } from "./mentions";

const base = { draft: "", onDraftChange() {}, onSend() {} };
const person = { kind: "person", name: "Laurent", entity_id: null, identity_ref: null, avatar: null };
const entity = { kind: "entity", name: "Castor", entity_id: "ent-castor", identity_ref: "entity:castor#3", avatar: { style: null, color: "#2a9d8f" } };
const agent = { kind: "agent", name: "Basic agent", entity_id: null, identity_ref: null, avatar: null };
const chat = (messages: unknown[], extra: Record<string, unknown> = {}) =>
  renderToStaticMarkup(<WorkflowChat {...base} {...extra} messages={messages as any} />);

describe("the author header on the left bubble", () => {
  it("an entity's reply shows its name, the entity tag and its mark", () => {
    const html = chat([
      { id: "u1", role: "user", content: "What do you think?", author: person },
      { id: "a1", role: "assistant", content: "I think so.", runId: "m1", author: entity },
    ]);
    expect(html).toContain('data-author="entity"');
    expect(html).toMatch(/<span class="pc-chat-role">Castor<\/span>/);
    expect(html).toMatch(/pc-chat-author-kind"[^>]*><span class="af-chip__label">entity</);
    // The person's own bubble keeps its header (authors label the left side only).
    expect(html.match(/data-author=/g)).toHaveLength(1);
  });
  it("an agent's reply shows the workflow name the gateway serves, with the agent tag", () => {
    const html = chat([{ id: "a1", role: "assistant", content: "Done.", runId: "r1", author: agent }]);
    expect(html).toContain('data-author="agent"');
    expect(html).toMatch(/<span class="pc-chat-role">Basic agent<\/span>/);
    expect(html).toMatch(/pc-chat-author-kind"[^>]*><span class="af-chip__label">agent</);
  });
  it("author null (an older gateway) is exactly today's card: 'Agent', no mark, no tag", () => {
    const today = chat([{ id: "a1", role: "assistant", content: "Done.", runId: "r1" }]);
    const nulled = chat([{ id: "a1", role: "assistant", content: "Done.", runId: "r1", author: null }]);
    expect(nulled).toBe(today);
    expect(today).toMatch(/<span class="pc-chat-role">Agent<\/span>/);
    expect(today).not.toContain("data-author");
    expect(today).not.toContain("pc-chat-author-kind");
  });
  it("an unknown author kind is refused, never guessed: today's card", () => {
    const today = chat([{ id: "a1", role: "assistant", content: "Done.", runId: "r1" }]);
    expect(chat([{ id: "a1", role: "assistant", content: "Done.", runId: "r1", author: { ...agent, kind: "robot" } }])).toBe(today);
  });
});

describe("the composer's mention sentences", () => {
  const candidates = async () => [];
  it("an older gateway: the sentence shows above the composer", () => {
    const html = chat([], { mentionCandidates: candidates, mentionStatus: MENTIONS_UNAVAILABLE });
    expect(html).toContain(MENTIONS_UNAVAILABLE);
    expect(html).toContain("pc-mention-notice--status");
  });
  it("a refusal sentence shows the same way, and nothing shows without one", () => {
    expect(chat([], { mentionCandidates: candidates, mentionStatus: "Castor is in another conversation." })).toContain("Castor is in another conversation.");
    expect(chat([], { mentionCandidates: candidates })).not.toContain("pc-mention-notice");
  });
});
