# 0032 — Code TUI: mentions in the transcript (chips in the card, a pending bubble per entity, the words once)

- Status: planned (built 2026-11-11, waiting for the operator's check)
- Created: 2026-11-11
- Area: Code TUI (`tui/`); no AbstractTUI change (abstracttui 0.8.0)
- Work-item id: abstractcode-0032
- ADR impact: none.

## Summary and current code reality

Operator, 2026-11-11: "when you are done fixing code web, also create the same capabilities in code tui" (web:
[0031](0031_mention_answers_pending_bubbles_and_words_once.md) and the 0.9.13 chips in message bubbles).
Before: the person's card (`Item::User { text }`, `UserCard`) drew plain words; a send with mentions said "asking
Castor and Pollux" on the notice line and nothing in the transcript until each answer landed; a restored
conversation folded a mention's prompt only when the turn it repeats was restored, so the mentions of a message to
entities only each showed the words again.

## Done

- `Item::User` carries `mentions: Vec<SentMention>` (name, kind, byte offset); `mention::sent_mentions` places each
  chip from the raw draft's chip markers (never by reading the words for `@`), on the trimmed words the turn sends;
  `UserCard` draws them in the composer's chip style (accent on the raised ground, bold), a chip that wraps on both
  lines; an unplaceable chip keeps its words and one `[#SEAM]` line says so. No hover card in a terminal; the chip
  shows the entity's display name.
- `store.pending_mentions`: one per entity call from the send (the agent's chip rides the turn), drawn after the
  live replies as the entity's bubble ("● Castor · entity  answering •··", the dots on the run ticker); its outcome
  removes it (`gateway::mentions::apply`, the first matching), and a chain that ends removes the calls it never
  asks (`never_asked`). The "asking …" notices are gone.
- `rehydrate_turn_into`: the first mention of a turn not restored here keeps the words, the others fold.
- Tests: `mention` (placement, pending), `gateway::mentions` (`never_asked`), `transcript_view` (chip ranges, line
  starts, runs), `runner` (restore once), `tests/mentions_ui.rs` (the bubbles replaced, the words once, the chip
  style on the screen's cells).

## Open

The chip face is the composer's (`@Name`); the web's chip also draws the entity's orb.
