# Compact Code settings and readable terminal tools

Operator requests: Code 0024 (WUI) and 0027 (terminal). Local delivery 2026-10-30.

The Model panel now shows provider/model first, with reasoning, MTP and limits folded.
Agent combines workflow, category/individual tool permissions, skills and instructions.
Categories start folded; unavailable categories have a separate disclosure. Workspace
uses the shared compact chooser, showing inherited access once and retaining editable
folder policy. Informational help uses the themed shared tooltip after 200ms and on focus.
The workflow timezone control is removed; existing account/Gateway inheritance applies.
Code uses the same DocsAssistantDrawer as the other apps, with the composer at the bottom.
Docs opens independently of the workspace panel. Shared drawer resizing now retains the
selected docked panel when the browser reloads.

The terminal picker now starts each visit with folded, sorted categories, indented
children and distinct Off/Ask/Auto ink. Long descriptions and sandbox constraints live
in the selected detail pane. Category and individual controls retain Gateway limits.
Keyboard and mouse expansion remain available. The modal is bounded to 116columns/34rows.

## Sources

- Code web: 6b4d62d; intake 83569d0.
- AbstractUIC: 1caceae and 98e94b1.
- Code terminal: e339c62 and 9addf83; clean engine 0853ea6b522d9111d84a938af8ea52602bc66f6d.
- Unrelated uncommitted avatar/runner/transcript work was excluded from the installed terminal.

## Verification

- All 502 Code web tests pass, TypeScript/Vite build and relative-URL checks pass.
- Full ui-kit suite passes, including 186 drawer checks, 88 tool-mode checks,
  103 workspace checks and 55 delayed-tooltip checks; route-picker checks pass.
- Four rail/automation browser tests pass: desktop/tablet/phone navigation, resized
  panel reload, automation settings saved as a new revision.
- Four policy/layout browser tests pass, including actual runs against a disposable
  Gateway: Off excludes a tool; Auto executes; Ask waits. No production model calls.
- Two final dark/light layout tests pass, including 200ms help, folded defaults,
  category/individual modes, Docs preserving the underlying panel and 320/390px layouts.
- Live app 3003: 14 checks per theme pass (28 total), no browser errors or setting/run
  writes; only this test context's two sign-ins. The JSON report contains no credential.
- Terminal clean-source all-targets: 1208 tests pass, 23 existing ignored; Clippy with
  warnings denied and formatting checks pass. Native installed-binary PTY smoke:
  initially folded categories, expansion, individual mode changes, clean exit, zero runs.

The PNGs in this folder use disposable fixture data, not operator conversation content.
Reviewed dark/light, expanded tool rows, compact default state and phone layout.

## Local deployment and release follow-up

The running Code app 3003 serves this checkout's built web/dist through the deployment
symlink. No Gateway or active session restart was needed. Served JS: index-YDf66H7_.js.
Reload the browser for this build.

The terminal was installed atomically in ~/.cargo/bin/abstractcode (version 0.12.0),
SHA256 33e9c72c4a8edca11879c8f7ed42cdb96ad99dfdac4f68c8b2b8781f7c3a5d02.
Existing processes were left running; restart the terminal client to load the new binary.
Rollback is retained under /tmp/code-tools-0027-rollback/ for this session.

No package publication or push was performed. The compact shared APIs are unreleased:
this local web build stages an npm pack of AbstractUIC 98e94b1 over registry ui-kit 0.9.10.
The committed Code lock records the published base 0.9.10, not the locally staged tarball.
A fresh registry-only npm ci does not yet include these APIs. Before a registry Code
release, publish the shared changes, raise Code's ui-kit floor/lock and rerun a clean
registry-only build. This is a release follow-up, not a claim of published availability.
Local reproduction: build ui-kit, npm pack --workspace ui-kit, install that tarball into
Code/web with --no-save --package-lock=false, then npm run build --prefix web.
