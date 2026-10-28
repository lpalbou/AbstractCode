# 0005 — Tool result envelopes render as JSON blocks; prompt inset "› text"; clickable chrome (workflow, model, session id)

- **Status:** proposed — item 3 (clickable chrome) completed in terminal 0.12.0; items 1–2 wait for abstracttui 0188 / 0186
- **Created:** 2026-10-28
- **Area:** TUI (`tui/src/ui/transcript_view.rs`, `tui/src/ui/json_view.rs`, `tui/src/ui/chrome.rs`, the composer)
- **Work item:** `abstractcode-0005`
- **Root link:** AbstractFramework backlog 1055
- **Depends on (abstracttui):** 0188 JSON block (pretty-print + collapse over `CodeView`, proposed), 0186
  TextInput/TextArea content inset (proposed); 0187 semantic hit targets for drawn spans and 1260 status bar
  with named slots (shipped in abstracttui 0.7.0).

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

## Progress

### Item 3 — clickable chrome: done (terminal 0.12.0, 2026-10-28)
Operator layout (screenshot 2026-10-28 09:23), built on abstracttui 0.7.0:
- Header (`chrome::header`): the workflow name and the provider/model are `print_action` spans
  (`chrome::ACT_WORKFLOW`, `ACT_MODEL`); the gateway + connection light is ONE `mark_action` rect
  (`ACT_GATEWAY`) with its own focus ring. A stable `Element::on_action` host (focusable) wraps the
  per-tick line, so focus survives its rebuilds.
- Status bar (`chrome::status_bar`, `status_bar_model`): an abstracttui `StatusBar` — LEFT the
  instruments, CENTER the workspace path (`ACT_WORKSPACE`, `BarFit::TruncateStart`) and the session id
  (`ACT_SESSION`), RIGHT the markers and the theme (`ACT_THEME`); priorities in `chrome::bar_priority`.
- Every id maps in `ui::chrome_action` to the modal its slash command opens: `/workflow` (catalog
  refresh + picker), `/model`, `/status`, `/workspace` (rail Workspace panel), `/sessions`, the theme
  dropdown; the attention chip (`ACT_WAITING`) opens the waiting automation.
- Removed: the attention chip's stored `(x0, x1)` click span (the last hard-coded cell coordinates in the
  chrome), the bar's own right-cluster planner (`right_plan`/`status_right`/`footer_session`), the
  focusable theme trigger view. Lost: the wheel over the CLOSED theme name (the engine does not expose
  which span a wheel lands on; an `EventCtx` accessor would restore it).

Evidence (`cargo test --workspace`, clippy `-D warnings`):
- `tests/chrome_width_torture.rs::chrome_degrades_whole_item_at_every_width` — 60/80/100/120/200/271
  cols: header right widget intact, workspace + session + theme kept in order at every width,
  instruments drop whole in priority order, every action id is a drawn hit target.
- `every_chrome_widget_opens_its_modal_on_click` (80 and 140 cols) and
  `every_chrome_widget_opens_its_modal_from_the_keyboard` — the click lands on `UiTree::action_rect`
  (no cell constant in the test either); red when a tag is removed (checked: without the
  `ACT_GATEWAY` tag all three fail).
- `r17w1_screens::a_click_on_the_chip_opens_the_same_automation` — the chip through its action id.
- Adversarial review (Opus, read-only) fixes, each pinned: a click left the main tree's focus on the
  host so the next Enter/Space re-ran the widget → every activation hands focus back to the composer
  (`a_click_hands_the_keyboard_back_to_the_prompt`, red without it); a focus flag could outlive its torn-down
  host and swallow Space → cleared on the chrome build scope's teardown; an uncapped gateway-down reason
  cleared the bar → cut to 40 columns, priority below the workspace path
  (`a_gateway_down_reason_never_clears_the_status_bar`); the workflow test now requires the picker to open
  (catalog seeded), not just a catalog request; `/help` describes the widgets.
