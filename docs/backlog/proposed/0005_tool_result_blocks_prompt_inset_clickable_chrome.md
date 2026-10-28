# 0005 — Tool result envelopes render as JSON blocks; prompt inset "› text"; clickable chrome (workflow, model, session id)

- **Status:** proposed
- **Created:** 2026-10-28
- **Area:** TUI (`tui/src/ui/transcript_view.rs`, `tui/src/ui/json_view.rs`, `tui/src/ui/chrome.rs`, the composer)
- **Work item:** `abstractcode-0005`
- **Root link:** AbstractFramework backlog 1055
- **Depends on (abstracttui, proposed):** 0188 JSON block (pretty-print + collapse over `CodeView`), 0186
  TextInput/TextArea content inset, 0187 semantic hit targets for drawn spans, 1260 status bar with named slots.

## Evidence
Operator screenshot, 2026-10-28 00:55: a sandbox tool-result envelope printed as flat, unindented text in the
transcript.

## Current code reality
- `transcript_view::shown_result` (~l.241) pretty-prints only when `json_view::is_json` holds: the WHOLE trimmed
  result must start with `{`/`[`, parse, and stay under `MAX_BYTES`. An envelope with a text line before the JSON
  (the sandbox sentence, a status prefix), JSON-in-a-string (`"{\"ok\":…}"`), or an oversize body falls through to
  `tool_body_rows` (`│ ` gutter, verbatim wrap) — the flat text in the screenshot.
- `json_view` is this app's own pretty-printer/tokenizer; abstracttui's `CodeView` already colours JSON
  (`JsonLexer`), so the app keeps a second JSON renderer.
- The composer draws its prompt without an inset inside the input; the header (`chrome.rs` ~l.120–150: workflow,
  provider/model, session id) is custom-drawn with no click targets.

## What we want
1. Tool results: detect a JSON payload inside the envelope (a leading sentence, then JSON; a JSON string holding
   JSON; JSONL) and render the JSON part with abstracttui 0188 (indent, key/value colours, collapse past depth N),
   the surrounding text kept as text; oversize bodies collapsed, not flattened. Retire `ui::json_view` for the
   kit's block once 0188 ships.
2. Composer: "› " then the text, through 0186's inset (mouse-to-caret and wrap correct), not a padded row.
3. Chrome: workflow, model and session id are clickable (pick workflow, pick model, copy/open session) through
   0187 action ids, inside a 1260 status bar or the 0560 header; no hard-coded cell coordinates.

## Validation
Headless renders of the screenshot's envelope (indented, coloured, collapsible), JSON-in-string and a prefixed
envelope; composer caret tests at the inset; chrome clicks reach the three actions after a resize. Red without each.
