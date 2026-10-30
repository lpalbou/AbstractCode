# Skills picker readability and mouse scrolling

Delivered locally 2026-10-30 for AbstractCode 0028.

The old 84×24 picker drew a cursor-windowed text slice with no pointer handler.
The new picker uses the shared engine List, up to 128 columns×42 rows, clamped to
leave a terminal margin. Names and attachment state occupy the list; the selected
skill's complete description and trust/blocked status occupy a separate scrollable
pane. Redundant implementation prose was removed. The information surface uses the
current theme with readable body text and a neutral selection highlight. Existing
checked-skill accent/bold state remains. Pointer Attach/Detach is explicit; selecting
or scrolling alone never changes attachments. Space still switches attachment and
Enter/Escape closes. No Gateway permission or skill activation contract changed.

Skill discovery also stopped discarding description lines after the first line.
The detail pane can now show all metadata returned by the Gateway.

## Validation and visible review

- Clean Code source c0d6b7d (picker f4a207f), engine 205b55541887441cffd42fb5b6cb1366978b5bf9.
- Full all-target suite: 1221 passed, 0 failed, 23 existing ignored. All-target Clippy
  with warnings denied and scoped formatting checks pass.
- New real input/render tests: larger dark/light layouts; no inline description
  clutter; wheel up/down at 80×24 without prior hover; last item reachable;
  scrollbar-thumb dragging; mouse select versus explicit Attach; blocked skill
  refusal; persisted attachments; PageDown/Home/End; stable selection after catalog
  insertion; separately scrollable long descriptions; Enter closes without toggling.
- Existing empty-shelf warnings, attachment-to-run propagation and checked/blocked
  style tests pass. Parser regression asserts complete multiline descriptions.
- Installed-binary native PTY smoke at 120×32 passes: wheel scrolling reaches skill 47,
  mouse selects it, its multiline description survives discovery, Attach persists
  only that skill in isolated fixture preferences, Enter closes, exit 0; 25 requests
  to a session-only mock Gateway; zero run starts or production application writes.
- Directly inspected the exported dark/light captures. Skill names and actions are
  separate from details, the selected item is clear, and list/detail scrollbars
  remain visible. Loading/empty/error are retained; blocked policy remains enforced.

The initial clean snapshot lacked the web source used by a cross-client wording
parity test. The snapshot was expanded to all committed Code resources and the
complete suite rerun green. An unrelated in-progress engine avatar change also
prevented a worktree Clippy resolution; final checks and install use only the
pinned clean snapshots above. Those unrelated changes were preserved.

## Installation

Installed atomically into ~/.cargo/bin/abstractcode; exact hash and rollback path
are in install.json. The installed native smoke result is in native-result.json.
No running process was restarted. Restart AbstractCode and reopen /skills to load
this binary. This is a local fix; no package publication or push.

Full descriptions remain behind selection; this change does not add a new search
interface or alter the Gateway's skill shelf. No new architecture decision or
follow-up is required for this presentation/input fix.
