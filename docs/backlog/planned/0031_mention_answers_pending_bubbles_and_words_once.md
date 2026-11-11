# 0031 — Mention answers: a pending bubble with its dots in the thread; the person's words once

- Status: planned (built 2026-11-11, waiting for the operator's check)
- Created: 2026-11-11
- Area: Code web (AbstractUIC panel-chat 0.9.13; a kit gap for 0.9.14 below)
- Work-item id: abstractcode-0031
- ADR impact: none.

## Summary and current code reality

Operator, round 6 (2026-11-11 02:05), on :3003:
1. "ⓘ nerin is answering…" sat in the composer's status ("that should not be in my prompt box… you should
   [show] the 3 dancing dots in the chat itself, where the AI would answer, and with its @entity_name
   answering…"). `askMentions` (app.tsx) set it from `sendMentions`'s `onAsking`.
2. The thread repeated the person's message before each entity's answer ("You: hey @castor… / castor: … /
   You: hey @castor… / nerin: …"; "i would expect to see my message and then both of their messages"). Each
   mention is a session turn whose prompt repeats the person's words (`repeat_of`); `foldRepeatedTurns` folded
   such a prompt only while the repeated turn was on screen, and a message to entities only records a turn with
   no run in the conversation, so every mirror kept its words.

## Done (web)

- Each entity of the message gets a pending bubble at the thread's end from the moment the mentions are asked:
  the entity as author (name, look), the kit's live state ("answering"), the kit's `WorkingDots` in the card's
  footer; it goes when its answer's turn shows a reply (or the conversation opens on the newest answer); a
  refused, stopped or unasked mention's bubble goes at once. The composer's status keeps refusals and outcomes
  only.
- Of the prompts repeating a turn not on screen, the first keeps the words and the others fold.
- Tests `web/src/workspace/mention_round6.test.tsx`.

## Kit gap (panel-chat 0.9.14)

`ChatMessageCard` draws no body for a `live` message without text (only the header's "● answering"), and
`WorkingDots` is drawn only at the thread's tail (`WorkflowChat` `working`). Proposed: a `live` message without
text or media draws `WorkingDots` (label `live.label`) in its body; the conversation entry exports `WorkingDots`.
Then the app drops its footer dots.

## Open

Guidance sent with mentions names no `repeat_of`, so those mirrors still show the words each time.
