# 0024 — Compact web settings and shared docs assistant layout

- Package: abstractcode (web), abstractuic (ui-kit)
- Type: improvement
- Status: completed
- Completed: 2026-10-30
- Created: 2026-10-30
- Priority: high
- Work-item id: abstractcode-0024

## Summary

Operator requests a full simplification of Code WUI settings: compact Model;
one Agent tab for workflow/tools/skills; category and individual Off/Ask/Auto
with categories folded initially; quieter Workspace; short inline labels with
200ms themed information tooltips; shared docs assistant matching other apps.
Use the shared components and preserve their permission/storage semantics.

## Current code reality

Code uses ProviderModelPicker, ToolModeEditor, WorkspaceChooser and
DocsAssistantPanelContent already. Model renders every option and long help;
workflow/tools/skills occupy separate rail panels; categories stay expanded;
Workspace repeats inherited policy at several levels. Account time zone already
inherits the Gateway and need not appear in workflow settings. The docs panel
misses the shared fill layout, placing its composer above unused space.
Code currently installs ui-kit 0.9.8, while the current shared package is0.9.10.
Unrelated terminal/avatar changes in this checkout belong to another task.

## Scope and validation

Compose compact shared controls and progressive disclosure, group Agent panels,
preserve conversation/automation binding, reset/inheritance, tool restrictions,
skill selection and workspace writes. Keep diagnostic/refusal feedback usable.
The docs panel must fill its viewport and retain history/new-chat/attachments.
Check desktop/mobile, theme, keyboard and delayed help; run appropriate shared
component tests, Code tests/build and browser behavior checks. Commit only this
scope, locally deploy and retain screenshots and verification evidence. Package
publication is separate from local deployment; no registry release is claimed.

- [x] Implement shared compact controls and web composition.
- [x] Verify state, behavior, visible layout and local deployment.

## Completion report

Implemented in Code 6b4d62d and AbstractUIC 1caceae/98e94b1. Model is compact;
Agent combines workflow, folded category/individual tool permissions, skills and
instructions; Workspace removes repeated policy prose. Shared 200ms information
help replaces upfront explanations. Account/Gateway timezone inheritance remains.
Docs now uses the shared standalone DocsAssistantDrawer with bottom composer.
Drawer resize persistence was repaired with a regression check.

All 502 web tests, the full ui-kit suite and build pass. Four navigation/automation
checks, four behavior/layout checks and two final dark/light checks pass. Actual
Off/Ask/Auto runs used the disposable Gateway. Live local app validation passes
28 layout checks with no application setting/run writes. Screenshots and receipts:
[review](../../../design/compact-settings-0024/REVIEW.md).

Locally deployed through the existing Code 3003 deployment symlink. No public
release. Shared compact APIs are staged from local AbstractUIC source; before a
registry Code release publish ui-kit, raise the dependency floor/lock, and verify a
fresh registry-only install. The evidence report states the exact reproduction.
