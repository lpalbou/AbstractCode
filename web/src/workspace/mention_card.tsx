// The card on a mention chip in a message (panel-chat 0.9.13 `renderMentionCard`, operator
// 2026-11-10): resting on "@Castor" shows the entity's orb, its name and one sentence of its state,
// read from the `GET /entities` row the `@` list loaded (`entityMentionCards`). The agent's chip,
// or a name the loaded list does not hold, has no card (null).
import React from "react";
import { ChatAuthorMark, type ChatMessageMention } from "../vendor/abstractchat/conversation.js";
import type { EntityMentionCard } from "./mentions";
import "./mention_card.css";

export function MentionCard({ card }: { card: EntityMentionCard }): React.ReactElement {
  return (
    <div className="code-mention-card">
      <ChatAuthorMark mark="entity" look={card.avatar} initial={card.name} size={28} />
      <div className="code-mention-card__text">
        <span className="code-mention-card__name">{card.name}</span>
        {card.sentence ? <span className="code-mention-card__state">{card.sentence}</span> : null}
      </div>
    </div>
  );
}

/** `renderMentionCard`: the card of an entity the loaded list holds, else null. */
export function mentionCard(mention: ChatMessageMention, cards: ReadonlyMap<string, EntityMentionCard> | null | undefined): React.ReactNode | null {
  if (mention.kind !== "entity") return null;
  const card = cards?.get(mention.id);
  return card ? <MentionCard card={card} /> : null;
}
