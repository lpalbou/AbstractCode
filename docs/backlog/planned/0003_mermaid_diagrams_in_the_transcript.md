# 0003 — Mermaid diagrams in the TUI transcript, through an AbstractTUI `Feed` fence hook

- **Status:** planned (not started; deferred by the operator 2026-10-10 — "we will do it later")
- **Created:** 2026-10-10
- **Area:** TUI (`tui/src/ui/transcript_view.rs`, `tui/Cargo.toml`) + the engine (`abstracttui` core `Feed`,
  `abstracttui-mermaid` extension) — the engine half is owned by the abstracttui seat
- **Work item:** `abstractcode-0003`; engine half to be filed in `abstracttui/docs/backlog/` when work starts

## Summary
Models answer with ```` ```mermaid ```` fences (operator screenshot 2026-10-10: a `flowchart LR` of "une loupe React
pour l'IA"), and the TUI shows them as code. AbstractTUI already renders the honest mermaid subset natively —
`abstracttui-mermaid` 0.5.0 (crates.io), built on the same `abstracttui` 0.6 this client pins — but only through
`MarkdownView`'s fence hook. The transcript is the engine's `Feed` widget, which has no such hook. The operator ruled
(2026-10-10) that this belongs in the ENGINE, not in an app-side workaround: `Feed` is the engine's chat/transcript
component and ADR-0005 (abstracttui) requires it to render markdown "through the same `BlockTypesetter`" as
`MarkdownView` — "one recipe, no drift". The missing hook is that drift.

## Current code reality (2026-10-10)
- `abstracttui/src/widgets/feed_typeset.rs:146` builds the feed's typesetter as
  `BlockTypesetter::new(&tokens).with_rule_style(self.rule)` — no fence claimant. `MarkdownView` passes one
  (`src/widgets/markdown_doc.rs:90-91`, `.with_fence_block(fence)`; public setter `MarkdownView::fence_block`,
  `src/widgets/markdown.rs:360`). The seam itself is public: `widgets::FenceBlock` (`measure` + `draw_row`,
  `src/widgets/markdown.rs:675`).
- `abstracttui-mermaid` `MermaidFence` (`extensions/mermaid/src/fence.rs:43`) caches ONE rendered diagram
  (`cache: RefCell<Option<Rendered>>`, keyed by source + width + theme). In a document that is fine; in a chat
  feed several diagrams are on screen at once and a single slot would re-render on every row of every frame.
- The same `feed_typeset.rs` spacing gives TWO blank rows when a markdown block follows a custom block inside one
  item (the Markdown arm pre-pushes a blank, then the first doc block adds its separator). AbstractCode works
  around it by giving the answer's markdown its own feed entry (`render_body`, keys `i{n}` / `i{n}~body` in
  `tui/src/ui/transcript_view.rs`). That workaround is part of the 2026-10-10 TUI round, uncommitted at the time
  of writing — check `git log` before relying on it.
- AbstractCode renders final answers, interim updates and open-cycle words as `FeedBlock::Markdown` in
  `tui/src/ui/transcript_view.rs`; live replies stream through a second `Feed` (`wire_live_feed`).
- Probe (scratch crate against the published `abstracttui 0.6.0` + `abstracttui-mermaid 0.5.0`, 2026-10-10):
  the operator's `flowchart LR` renders natively in 13 rows (~0.65 ms) — rounded node cards, braille edges with
  arrowheads, both cycles; a `sequenceDiagram` renders with lifelines and dashed replies; `pie` falls back to a
  one-line notice + a mermaid.live link + the source. At 100 columns the LR diagram (natural width ~135) is
  clipped on the right: a document row cannot pan.

## Scope
Engine half (abstracttui seat, released before the app half):
1. `Feed::fence_block(Rc<dyn FenceBlock>)` (or the equivalent on `FeedState`), threaded into the static typeset
   AND the streaming path, so a fence claimed in a live reply renders once it closes.
2. `MermaidFence`: a small multi-entry cache keyed by (source, width, theme) so many diagrams in one feed do not
   thrash.
3. Fix the custom-block → markdown double blank row in `feed_typeset.rs` (same file, same review).
4. Releases: `abstracttui` (feed hook + spacing), then `abstracttui-mermaid` against it.

App half (this repo):
1. Add `abstracttui-mermaid`; raise the `abstracttui` floor in `tui/Cargo.toml` to the release carrying the hook
   (document the floor in the dependency comment block, as for every earlier bump).
2. Turn the claimant on for the transcript feed and the live-reply feed.
3. Drop the `~body` split workaround if the engine fixed the spacing (or keep it with a note if not).
4. Read aloud: exclude fenced diagram source from what is spoken (`voice_view` narration and the message icons).

## Non-goals
- An app-side fence splitter (the "option 1" discussed 2026-10-10): rejected by the operator in favour of the
  engine fix.
- Re-laying wide diagrams top-to-bottom to fit the width: changes the author's diagram silently — an operator
  decision, not part of this item.
- Mermaid kinds outside the extension's subset (class, ER, gantt, pie, …): they keep the extension's honest
  fallback.

## Dependencies
- The operator lifting the read-only rule on `abstracttui` for this work, and an abstracttui release.
- `abstracttui-mermaid` release against that `abstracttui`.

## Validation
- Engine: a `Feed` test that a ```` ```mermaid ```` fence is claimed (static and streamed); two diagrams on screen
  render from cache (render count pinned); exactly one blank row between a custom block and the markdown after it.
- App: a headless test in `tui/tests/headless_ui.rs` — the operator's 2026-10-10 flowchart in a final answer shows
  its node labels (`Utilisateur`, `Service IA`, `Base de connaissances`) and no fence source; a `pie` answer shows
  the fallback notice; right-click copy on the answer still returns the full answer text, source included; read
  aloud of an answer with a diagram does not send the diagram source to `/voice/tts/stream`.

## Related
abstracttui ADR-0005 (content rendering responsibilities); abstracttui `docs/graphs-and-diagrams.md`; abstracttui
backlog `completed/extensions/0450`, `0452` (mermaid extension, document fences).
