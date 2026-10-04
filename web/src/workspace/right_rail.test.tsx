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
import { CodeRightRail, RAIL_ITEMS, RAIL_PANEL_KEY, RAIL_PANEL_KEY_V1, readRailPanel, type RailPanel } from "./right_rail";
import { DEFAULT_PREFERENCES } from "./settings_panel";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const voiceSource = readFileSync(new URL("./voice_tools.tsx", import.meta.url), "utf8");

function row(kind: ActivityRow["kind"], key: string, status = "completed", title = kind): ActivityRow {
  return { key, runId: "r1", cursor: 1, kind, title, detail: "", status, statusLabel: status, nodeId: "", stepIds: [], progress: "", progressFinal: false, entries: [], progressEvents: [], merged: {} };
}

describe("header: no gear (round 6 — the rail icons open the panels)", () => {
  it("the top bar has no Settings/gear button and no extra actions", () => {
    expect(appSource).not.toContain("code-panel-opener");
    expect(appSource).not.toContain('aria-label="Settings"');
    expect(appSource).not.toContain('<Icon name="cog"');
    expect(appSource).not.toContain("extraActions=");
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
  const content = Object.fromEntries(RAIL_ITEMS.map((item) => [item.id, <p key={item.id}>{item.id.toUpperCase()}</p>])) as Record<RailPanel, React.ReactNode>;
  it("renders the kit rail with one icon per subject, in order, and no horizontal tab strip", () => {
    const html = renderToStaticMarkup(<CodeRightRail panel="files" onPanel={() => {}} content={content} activityBadge={2} />);
    expect(html).toContain('role="tablist"');
    expect(html).toContain('aria-orientation="vertical"');
    const names = [...html.matchAll(/role="tab"[^>]*aria-label="([^"]+)"|aria-label="([^"]+)"[^>]*role="tab"/g)].map((m) => m[1] || m[2]);
    expect(names).toEqual(["Activity", "Files", "Model", "Workflow", "Workspace", "Tools", "Skills", "Voice"]);
    expect(html).toContain("FILES");
    expect(html).not.toContain("af-tabs");
    expect(html).toContain('class="af-rail__badge"');
  });
  it("has no 'Settings' panel and no 'Model & behavior' anywhere in the app shell", () => {
    expect(RAIL_ITEMS.map((item) => item.label)).not.toContain("Settings");
    expect(appSource).not.toContain("Model & behavior");
    expect(appSource).not.toContain("Tools & skills");
    expect(appSource).not.toMatch(/setRailPanel\("settings"\)/);
  });
  it("app.tsx gives every rail panel its own content", () => {
    for (const id of ["activity", "files", "model", "workflow", "workspace", "tools", "skills", "voice"]) expect(appSource).toMatch(new RegExp(`\\n\\s+${id}: `));
  });
  it("restores the remembered panel; a round-4 'settings' opens Model", () => {
    const store = (values: Record<string, string>) => ({ getItem: (k: string) => (k in values ? values[k] : null) });
    expect(readRailPanel(store({ [RAIL_PANEL_KEY]: "skills" }))).toBe("skills");
    expect(readRailPanel(store({ [RAIL_PANEL_KEY]: "" }))).toBeNull();
    expect(readRailPanel(store({ [RAIL_PANEL_KEY_V1]: "settings" }))).toBe("model");
    expect(readRailPanel(store({ [RAIL_PANEL_KEY_V1]: "files" }))).toBe("files");
    expect(readRailPanel(store({ [RAIL_PANEL_KEY]: "bogus" }))).toBeNull();
    expect(readRailPanel(null)).toBeNull();
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
  it("app.tsx binds every settings panel to the selection: an automation edits its definition through the gateway", () => {
    expect(appSource).toContain("useAutomationSettings(");
    expect(appSource).toMatch(/\(automationId, changes, expected\) => automations\.revise\(automationId, changes, expected\)/);
    expect(appSource).toContain("<AutomationDefinitionForm");
    expect(appSource).toMatch(/onRevise=\{\(changes, expected\) => automations\.revise\(automationDetail\.automationId, changes, expected\)\}/);
    // Edit on the automation header opens its Workflow panel (task, schedule, workflow).
    expect(appSource).toMatch(/const openAutomationSettings = \(automationId: string\) => \{[\s\S]*?setRailPanel\("workflow"\);/);
  });
});

describe("voice", () => {
  it("TTS requests carry only the speech fields; transcription carries the STT override", () => {
    expect(voiceSource).toContain("...voiceTtsRequest(preferences)");
    expect(voiceSource).toContain("...voiceSttRequest(preferences)");
    expect(voiceSource).toContain('output_device_id: preferences.output_device || ""');
  });
  it("Settings → Voice is the kit's Assistant-layout section", () => {
    // Round 6: the kit section is mounted through CodeVoiceSettings (voice_tools.tsx) with the gateway's voice/defaults.
    expect(appSource).toContain("<CodeVoiceSettings");
    expect(voiceSource).toContain("<AfVoiceSection");
    expect(voiceSource).toContain("fetchDefaults={fetchVoiceDefaults}");
    expect(appSource).not.toContain("<VoiceSettings");
  });
});

describe("adversary pass W4 fixes", () => {
  it("F4: the workspace section names access modes in plain words", async () => {
    const { SettingsContent, workspaceModeLabel } = await import("./settings_panel");
    expect(workspaceModeLabel("workspace_or_allowed")).toBe("Workspace and allowed paths");
    const html = renderToStaticMarkup(
      <SettingsContent tab="workspace" value={DEFAULT_PREFERENCES} onChange={() => {}} tools={[]} disabled={false}
        policy={{ allowedAccessModes: ["workspace_only", "workspace_or_allowed"], mounts: [], clientWorkspaceScopeOverrides: true } as any} />,
    );
    expect(html).toContain("This workspace only · Workspace and allowed paths");
    expect(html).not.toMatch(/>[^<]*workspace_only[^<]*</);
    expect(html).not.toMatch(/>[^<]*workspace or allowed[^<]*</);
  });
  it("F3: an artifact without a filename reads as its type, never its id", async () => {
    const { artifactTypeLabel } = await import("./workspace_panels");
    expect(artifactTypeLabel("image/png")).toBe("PNG image");
    expect(artifactTypeLabel("application/pdf")).toBe("PDF document");
    expect(artifactTypeLabel("text/plain; charset=utf-8")).toBe("Text file");
    expect(artifactTypeLabel("")).toBe("File");
  });
});

describe("R5 Files: audio in the kit waveform player, JSON/code in the kit code viewer", () => {
  it("previews audio artifacts through an object URL (the kit's AfAudioPlayer), JSON/code as text (AfCodeBlock)", async () => {
    const { artifactPreviewMode } = await import("./workspace_panels");
    const { fileViewerKind } = await import("@abstractframework/ui-kit");
    expect(artifactPreviewMode(fileViewerKind("reply.wav", "audio/wav"))).toBe("url");
    expect(artifactPreviewMode(fileViewerKind("speech", "audio/mpeg"))).toBe("url");
    expect(artifactPreviewMode(fileViewerKind("data.json", "application/json"))).toBe("text");
    expect(artifactPreviewMode(fileViewerKind("main.py", "text/x-python"))).toBe("text");
    expect(artifactPreviewMode(fileViewerKind("blob.bin", "application/octet-stream"))).toBe("none");
  });
  it("the kit viewer renders audio with the waveform player and JSON with the code block (no local viewer)", async () => {
    const { FileViewer } = await import("@abstractframework/panel-chat");
    const audio = renderToStaticMarkup(<FileViewer name="reply.wav" contentType="audio/wav" status="ready" url="blob:x" nowMs={0} onClose={() => {}} onDownload={() => {}} />);
    expect(audio).toMatch(/af-audio/);
    const json = renderToStaticMarkup(<FileViewer name="data.json" contentType="application/json" status="ready" text='{"a":1}' nowMs={0} onClose={() => {}} onDownload={() => {}} />);
    expect(json).toMatch(/af-code/);
    const panels = readFileSync(new URL("./workspace_panels.tsx", import.meta.url), "utf8");
    expect(panels).not.toMatch(/<audio\b|<pre\b/);
  });
});
