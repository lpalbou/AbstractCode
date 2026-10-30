# 0024 — Compact web settings and shared docs assistant layout

- Package: abstractcode (web), abstractuic (ui-kit)
- Type: improvement
- Status: planned
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
Code currently installs ui-kit0.9.8, while the current shared package is0.9.10.
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

- [ ] Implement shared compact controls and web composition.
- [ ] Verify state, behavior, visible layout and local deployment.
