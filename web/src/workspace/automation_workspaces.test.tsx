// Round 13 (DESIGN R13.2): New automation = the kit AfScheduleDialog with a
// visible Workspaces section (the WorkspaceChooser at the run level) whose
// value rides the create body as target.input_data.workspace; the header
// shows the automation's one line "Workspaces: <the gateway's summary>" and
// an icon that opens its Workspace panel (a change there saves a revision).
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { WORKSPACE_CHOOSER_TEXT as T } from "@abstractframework/ui-kit";

import { NewAutomationDialog } from "./automations_view";
import { AutomationHeaderBar } from "./automation_header";
import { CodeWorkspaceFolders } from "./workspace_folders";
import { automationSettingsChanges, automationRunPreferences } from "./automation_settings";
import { AUTOMATION_WORKSPACES_TITLE, automationWorkspace, withAutomationWorkspace, workspaceSummary, workspacesLine } from "./automation_workspaces";
import type { AutomationsController } from "./automations";

const captured = vi.hoisted(() => ({ props: undefined as any }));
vi.mock("@abstractframework/ui-kit", async (original) => {
  const real = await original<typeof import("@abstractframework/ui-kit")>();
  return { ...real, AfScheduleDialog: (props: any) => { captured.props = props; return null; } };
});

const PICS = "/Users/alice/Pictures";
const VALUE = { posture: "allowed_only" as const, default_mode: "rw" as const, folders: [{ path: PICS, mode: "rw" as const }] };
const LINE = "Deny everything, allow listed workspaces · /Users/alice/Pictures (rw)";
const view = readFileSync(new URL("./automations_view.tsx", import.meta.url), "utf8");
const kitDialog = readFileSync(new URL("../../node_modules/@abstractframework/ui-kit/dist/automations/AfScheduleDialog.js", import.meta.url), "utf8");
const kitPkg = JSON.parse(readFileSync(new URL("../../node_modules/@abstractframework/ui-kit/package.json", import.meta.url), "utf8"));

describe("New automation: the kit dialog's Workspaces section (R13.2)", () => {
  it("passes the run-level chooser in the dialog's workspaces slot, starting at Use my default", () => {
    const ctl = { state: { busy: false, emailStatus: null }, create: vi.fn(), loadEmailStatus: vi.fn() } as unknown as AutomationsController;
    renderToStaticMarkup(<NewAutomationDialog open target={null} initialPrompt="" workflowLabel="Agent" ctl={ctl} onCreated={() => {}} onClose={() => {}} connected />);
    const slot = captured.props.workspaces;
    expect(slot.type).toBe(CodeWorkspaceFolders);
    expect(slot.props.automation.value).toBeNull();
    expect(slot.props.connected).toBe(true);
    expect(slot.props.idPrefix).toBe("code-new-automation-workspace");
  });

  it("the create body carries the chosen workspaces as target.input_data.workspace (a stale R9 list dropped); Use my default sends none", () => {
    expect(withAutomationWorkspace({ prompt: "x", workspace_allowed_paths: ["/old"], workspace_access_mode: "workspace_or_allowed" }, VALUE)).toEqual({ prompt: "x", workspace: VALUE });
    expect(withAutomationWorkspace({ prompt: "x", workspace: VALUE }, null)).toEqual({ prompt: "x" });
    expect(withAutomationWorkspace(undefined, null)).toEqual({});
    expect(view).toMatch(/const input = withAutomationWorkspace\(tooled \|\| body\.target\.input_data, workspace\);/);
  });

  it("uses ui-kit 0.8.4: the dialog has the Workspaces slot and no Advanced disclosure", () => {
    expect(kitPkg.version).toBe("0.8.4");
    expect(kitDialog).toContain("Title and limits");
    expect(kitDialog).toContain('"data-field": "workspaces"');
    expect(kitDialog).not.toMatch(/af-schedule__advanced|"summary"/);
  });
});

describe("An existing automation's workspaces", () => {
  const definition = { revision: 2, target: { bundle_ref: "agent@1", flow_id: "main", input_data: { prompt: "x", workspace: VALUE, workspace_allowed_paths: [PICS] } } } as any;

  it("reads the stored payload (absent = Use my default)", () => {
    expect(automationWorkspace(definition)).toEqual(VALUE);
    expect(automationWorkspace({ target: { bundle_ref: "a", flow_id: "b", input_data: {} } } as any)).toBeNull();
  });

  it("the Workspace panel's change is one revision of target.input_data.workspace (Use my default removes it)", () => {
    const prefs = automationRunPreferences(definition.target.input_data);
    expect(prefs.workspace).toEqual(VALUE);
    const mine = automationSettingsChanges(definition, { ...prefs, workspace: null });
    expect(mine?.target?.input_data).not.toHaveProperty("workspace");
    const ro = automationSettingsChanges(definition, { ...prefs, workspace: { ...VALUE, folders: [{ path: PICS, mode: "ro" }] } });
    expect((ro?.target?.input_data as any).workspace.folders).toEqual([{ path: PICS, mode: "ro" }]);
    expect(ro?.target?.input_data).not.toHaveProperty("workspace_allowed_paths");
  });

  it("the header shows the one line and a Change workspaces icon that opens the Workspace panel", () => {
    const onEditWorkspaces = vi.fn();
    const html = renderToStaticMarkup(
      <AutomationHeaderBar
        summary={{ automation_id: "a1", title: "Sort photos", status: "active", revision: 2, workspace_root: "", trigger: { source_id: "schedule", source_version: 1, config: { every: "24h" } }, attention: { items: [], waits: [] }, occurrence_count: 0 } as any}
        occurrences={[]}
        triggerSources={[]}
        busy={false}
        onCommand={async () => {}}
        onToggleActive={async () => {}}
        onEdit={() => {}}
        onOpenFolder={() => {}}
        onCopyPath={async () => true}
        workspaces={<span data-field="workspaces">{workspacesLine(LINE)}</span>}
        onEditWorkspaces={onEditWorkspaces}
      />,
    );
    expect(html).toContain(`Workspaces: ${LINE}`);
    expect(html).toMatch(/data-action="edit-workspaces" aria-label="Change workspaces"/);
  });

  it("the line is the kit title + the gateway's dry-run summary verbatim, cached per value", async () => {
    expect(AUTOMATION_WORKSPACES_TITLE).toBe(T.title);
    const request = vi.fn(async (_p: string, init: { body?: unknown }) => ({ posture: "allowed_only", default_mode: "rw", folders: [], gateway_summary: "g", summary: (init.body as any)?.workspace ? LINE : "Mine" }));
    expect(await workspaceSummary(request as any, VALUE)).toBe(LINE);
    expect(await workspaceSummary(request as any, VALUE)).toBe(LINE);
    expect(await workspaceSummary(request as any, null)).toBe("Mine");
    expect(request).toHaveBeenCalledTimes(2);
    expect(request.mock.calls[0][0]).toBe("api/gateway/workspace/effective/me");
  });
});
