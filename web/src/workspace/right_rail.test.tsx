// Round 4 right panel: rail drawer, activity groups, settings bound to the
// selection (an automation's definition saved as a new revision), voice.
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { activity_groups, default_open_group } from "../lib/activity_groups";
import type { ActivityRow } from "../lib/activity_rows";
import { automationRunPreferences, automationSettingsChanges, withAutomationRunPreferences } from "./automation_settings";
import { saveErrorText, saveStateText } from "./automation_settings_view";
import { ActivityGroups, AutomationActivity } from "./workspace_panels";
import { CodeRightRail } from "./right_rail";
import { DEFAULT_PREFERENCES } from "./settings_panel";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const voiceSource = readFileSync(new URL("./voice_tools.tsx", import.meta.url), "utf8");

function row(kind: ActivityRow["kind"], key: string, status = "completed", title = kind): ActivityRow {
  return { key, runId: "r1", cursor: 1, kind, title, detail: "", status, statusLabel: status, nodeId: "", stepIds: [], progress: "", progressFinal: false, entries: [], progressEvents: [], merged: {} };
}

describe("header: the usual gear, icon-only, tooltip Settings", () => {
  it("is a cog icon button named Settings with a Settings tooltip", () => {
    const at = appSource.indexOf('className={`code-panel-opener');
    const button = appSource.slice(at, appSource.indexOf("</button>", at));
    expect(button).toContain('aria-label="Settings" title="Settings"');
    expect(button).toMatch(/<Icon name="cog" size=\{18\} \/>\s*$/); // icon only: no text label after it
  });
  it("no sliders icon, no 'Workspace & settings' label anywhere in the app shell", () => {
    expect(appSource).not.toContain('<Icon name="settings"');
    expect(appSource).not.toContain("Workspace & settings");
    expect(voiceSource).not.toContain('<Icon name="settings"');
  });
  it("never the 'close the drawer and choose Edit' dead end", () => {
    expect(appSource).not.toContain("close the drawer and choose Edit");
    expect(appSource).not.toContain("These are conversation settings");
  });
});

describe("the rail drawer", () => {
  it("renders the kit rail with Activity, Files, Settings and no horizontal tab strip", () => {
    const html = renderToStaticMarkup(
      <CodeRightRail panel="files" onPanel={() => {}} activity={<p>A</p>} files={<p>FILES</p>} settings={<p>S</p>} activityBadge={2} />,
    );
    expect(html).toContain('role="tablist"');
    expect(html).toContain('aria-orientation="vertical"');
    for (const name of ["Activity", "Files", "Settings"]) expect(html).toContain(`aria-label="${name}"`);
    expect(html).toContain("FILES");
    expect(html).not.toContain("af-tabs");
    expect(html).toContain('class="af-rail__badge"');
  });
  it("is mounted inside the content area (beside the conversation), not as an overlay drawer", () => {
    expect(appSource).toMatch(/<\/main>\s*\)\}\s*\{railElement\}\s*<\/div>/);
  });
});

describe("activity groups (one per iteration, newest open)", () => {
  const rows = [row("run", "a"), row("llm", "b", "completed", "Model call"), row("tools", "c"), row("llm", "d", "running", "Model call 2"), row("wait", "e", "waiting")];
  it("starts a group at every model step; rows before the first form Start", () => {
    const groups = activity_groups(rows);
    expect(groups.map((g) => g.title)).toEqual(["Start", "Step 1", "Step 2"]);
    expect(groups[1].rows.map((r) => r.key)).toEqual(["b", "c"]);
    expect(groups[2].status).toBe("running");
    expect(default_open_group(groups)).toBe(groups[2].key);
  });
  it("renders foldable groups with only the newest open", () => {
    const html = renderToStaticMarkup(<ActivityGroups rows={rows} />);
    const opens = html.match(/<details class="code-activity-group[^"]*"[^>]*>/g) || [];
    expect(opens).toHaveLength(3);
    expect(opens.filter((tag) => tag.includes(" open=")).length).toBe(1);
    expect(opens[2]).toContain(" open=");
    expect(html).toContain('data-group="Step 2"');
  });
  it("an automation shows one group per occurrence, latest first and open", () => {
    const html = renderToStaticMarkup(
      <AutomationActivity nowMs={Date.parse("2026-10-03T12:00:00Z")} loadRecords={async () => []} occurrences={[
        { run_id: "o1", index: 1, status: "completed", finished_at: "2026-10-03T08:00:00Z" },
        { run_id: "o2", index: 2, status: "running", fired_at: "2026-10-03T11:30:00Z" },
      ]} />,
    );
    const groups = html.match(/<details class="code-activity-group[^"]*"[^>]*>/g) || [];
    expect(groups).toHaveLength(2);
    expect(groups[0]).toContain('data-group="Run #2"');
    expect(groups[0]).toContain(" open=");
    expect(groups[1]).not.toContain(" open=");
    expect(html).toContain("30 min ago");
  });
});

describe("automation settings = its definition", () => {
  const input = {
    prompt: "Summarize",
    provider: "lmstudio",
    model: "qwen",
    tools: ["read_file"],
    _runtime: { provider: "lmstudio", model: "qwen", thinking: "high", allowed_tools: ["read_file"], tool_policy: { require_approval_tools: ["read_file"] } },
    _limits: { max_iterations: 7 },
    workspace_access_mode: "workspace_only",
  };
  it("reads the definition with the same keys a conversation turn writes", () => {
    const p = automationRunPreferences(input);
    expect([p.provider, p.model, p.reasoning, p.maxIterations, p.workspaceMode]).toEqual(["lmstudio", "qwen", "high", "7", "workspace_only"]);
    expect(p.tools).toEqual({ mode: "custom", selected: ["read_file"], approval: { read_file: "ask" } });
    expect(p.toolsCustomized).toBe(true);
  });
  it("an empty definition reads as Gateway default everywhere", () => {
    const p = automationRunPreferences({ prompt: "x" });
    expect([p.provider, p.model, p.reasoning, p.maxIterations, p.maxTokens, p.system, p.workspaceMode]).toEqual(["", "", "", "", "", "", ""]);
    expect(p.toolsCustomized).toBe(false);
  });
  it("round-trips without a change (no revision for nothing)", () => {
    const def = { target: { bundle_ref: "b@1", flow_id: "f", workflow_id: "b:f", input_data: input } } as any;
    expect(automationSettingsChanges(def, automationRunPreferences(input))).toBeNull();
  });
  it("a reset REMOVES the override (the gateway default applies again); the task is kept", () => {
    const next = withAutomationRunPreferences(input, { ...automationRunPreferences(input), provider: "", model: "", reasoning: "", maxIterations: "" });
    expect(next.provider).toBeUndefined();
    expect(next._runtime.provider).toBeUndefined();
    expect(next._runtime.thinking).toBeUndefined();
    expect(next._limits).toBeUndefined();
    expect(next.prompt).toBe("Summarize");
  });
  it("a change becomes a PATCH target with the definition's bundle/flow and the new input", () => {
    const def = { target: { bundle_ref: "b@1", flow_id: "f", workflow_id: "b:f", input_data: input } } as any;
    const changes = automationSettingsChanges(def, { ...automationRunPreferences(input), model: "llama", reasoning: "low" })!;
    expect(changes.target).toMatchObject({ bundle_ref: "b@1", flow_id: "f" });
    expect((changes.target as any).input_data._runtime).toMatchObject({ model: "llama", thinking: "low" });
    expect((changes.target as any).input_data.model).toBe("llama");
  });
  it("tools back to the default drop the selection and the allowed-tools ceiling", () => {
    const next = withAutomationRunPreferences(input, { ...automationRunPreferences(input), toolsCustomized: false, tools: DEFAULT_PREFERENCES.tools });
    expect(next.tools).toBeUndefined();
    expect(next._runtime.allowed_tools).toBeUndefined();
    expect(next._runtime.tool_policy).toBeUndefined();
  });
  it("save outcomes say the revision, and a conflict is read from the typed code (no text guessing)", () => {
    expect(saveStateText({ status: "saved", revision: 5 })).toBe("Saved as revision 5; applies from the next run.");
    expect(saveErrorText({ status: 409, code: "revision_conflict", message: "x" })).toMatch(/changed elsewhere/);
    expect(saveErrorText({ status: 409, code: "automation_busy", message: "revision" })).not.toMatch(/changed elsewhere/);
  });
  it("app.tsx binds Settings to the selection: an automation edits its definition through the gateway", () => {
    expect(appSource).toContain("<AutomationSettingsPanel");
    expect(appSource).toMatch(/onRevise=\{\(changes, expected\) => automations\.revise\(automationDetail\.automationId, changes, expected\)\}/);
    expect(appSource).toContain("const openAutomationSettings = (automationId: string)");
  });
});

describe("voice", () => {
  it("TTS requests carry only the speech fields; transcription carries the STT override", () => {
    expect(voiceSource).toContain("...voiceTtsRequest(preferences)");
    expect(voiceSource).toContain("...voiceSttRequest(preferences)");
    expect(voiceSource).toContain('output_device_id: preferences.output_device || ""');
  });
  it("Settings → Voice is the kit's Assistant-layout section", () => {
    expect(appSource).toContain("<AfVoiceSection");
    expect(appSource).not.toContain("<VoiceSettings");
  });
});
