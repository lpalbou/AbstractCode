// Code's tools are the kit's layers (backlog 0033; decision 2026-10-27, option (a)): the one-time,
// labeled migration of the Permissions tier, the run's tool_policy from the layer, the client
// approver for Ask only.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { normalizeToolCatalog, planRunTools, toWorkflowApprovalPolicy, withRunTools } from "@abstractframework/ui-kit";
import {
  TOOLS_MIGRATION_NOTICE,
  allEnabledAuto,
  inputToolLayer,
  legacyToolPreferences,
  migrateLegacyTools,
  toolsMigrationKey,
  withAllEnabledAuto,
  withToolLayer,
  withoutAutoChoices,
} from "./tool_layers";
import { parsePreferences } from "./preferences";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const settingsSource = readFileSync(new URL("./settings_panel.tsx", import.meta.url), "utf8");

// A served discovery/tools answer: rank 1 read, rank 2 write, rank 3 exec, an outreach tool whose
// destination the model chooses, a tool the gateway auto-approves, and one it does not offer.
const catalog = normalizeToolCatalog({
  items: [
    { name: "read_file", toolset: "files", enabled: true, approval_default: "ask", risk_rank: 1 },
    { name: "write_file", toolset: "files", enabled: true, approval_default: "ask", risk_rank: 2 },
    { name: "execute_command", toolset: "shell", enabled: true, approval_default: "ask", risk_rank: 3 },
    { name: "fetch_url", toolset: "web", enabled: true, approval_default: "ask", risk_rank: 1, model_controlled_destination: true },
    { name: "list_files", toolset: "files", enabled: true, approval_default: "auto", risk_rank: 1 },
    { name: "send_email", toolset: "mail", enabled: false, approval_default: "ask", risk_rank: 4, why_disabled: "No mailbox" },
  ],
});

describe("one-time migration of the Permissions tier into this app's tool layer", () => {
  const legacy = (permissions: string, extra: Record<string, unknown> = {}) => legacyToolPreferences({ permissions, ...extra })!;

  it("nothing to migrate for untouched settings", () => {
    expect(legacyToolPreferences({ permissions: "default", toolsCustomized: false })).toBeNull();
    expect(legacyToolPreferences({})).toBeNull();
  });
  it('"all": every enabled tool the kit bulk rule reaches → Auto (risk ≤ 2, never a model-chosen destination)', () => {
    expect(migrateLegacyTools(legacy("all"), catalog)).toEqual({ read_file: "auto", write_file: "auto" });
  });
  it('"write": rank ≤ 2 → Auto; "read": rank 1 → Auto; never exec, never a model-chosen destination', () => {
    expect(migrateLegacyTools(legacy("write"), catalog)).toEqual({ read_file: "auto", write_file: "auto" });
    expect(migrateLegacyTools(legacy("read"), catalog)).toEqual({ read_file: "auto" });
  });
  it("explicit pins map exactly; a custom selection turns the others Off; a tool the gateway does not offer is never written", () => {
    const layer = migrateLegacyTools(
      legacy("default", { toolsCustomized: true, tools: { mode: "custom", selected: ["read_file", "execute_command", "send_email"], approval: { execute_command: "approve", read_file: "ask" } } }),
      catalog,
    );
    expect(layer).toEqual({ execute_command: "auto", write_file: "off", fetch_url: "off", list_files: "off" });
  });
  it("a pin wins over the tier", () => {
    expect(migrateLegacyTools(legacy("all", { toolsCustomized: true, tools: { mode: "all", selected: [], approval: { write_file: "ask" } } }), catalog)).toEqual({ read_file: "auto" });
  });
  it("the stored preferences keep the legacy fields aside until migrated, and never store the layer", () => {
    const parsed = parsePreferences(JSON.stringify({ permissions: "all", tools: { mode: "all", selected: [], approval: {} }, toolsCustomized: false, model: "m" }));
    expect(parsed.legacyTools).toEqual({ tools: { mode: "all", selected: [], approval: {} }, toolsCustomized: false, permissions: "all" });
    expect(parsed.toolLayer).toEqual({});
    expect(parsePreferences(JSON.stringify({ model: "m" })).legacyTools).toBeUndefined();
  });
  it("is labeled, once: the notice is pending until dismissed, in the Tools panel", () => {
    expect(TOOLS_MIGRATION_NOTICE).toBe("Your Permissions setting became per-tool defaults on this device — review them");
    expect(toolsMigrationKey("acct")).toBe("abstractcode.tools.migrated.v1:acct");
    expect(appSource).toContain('localStorage.setItem(toolsMigrationKey(identity), "pending")');
    expect(appSource).toContain('localStorage.setItem(toolsMigrationKey(identity), "dismissed")');
    // Only when this app has no stored layer yet: a second load never migrates again.
    expect(appSource).toMatch(/if \(!Object\.keys\(toolStore\.readApp\(CODE_AGENT_INTERFACE\)\)\.length\) \{\s*const migrated = migrateLegacyTools/);
    expect(appSource).toContain("setPreferences(({ legacyTools: _done, ...rest }) => rest)");
    expect(settingsSource).toContain("toolsNotice ? (");
    expect(appSource).toContain("toolsNotice={!automation && toolsNoticePending ? { text: TOOLS_MIGRATION_NOTICE, onDismiss: dismissToolsNotice } : null}");
  });
});

describe("a run gets the layer through the kit plan", () => {
  const layer = { read_file: "auto", execute_command: "ask", fetch_url: "off" } as const;
  it("start body: auto_approve_tools = the Auto tools, require_approval_tools = the Ask tools only; Off outside allowed_tools", () => {
    const input = withRunTools({ prompt: "x" }, planRunTools(catalog, { app: layer }));
    expect((input as any)._runtime.tool_policy).toEqual({ auto_approve_tools: ["list_files", "read_file"], require_approval_tools: ["execute_command", "write_file"] });
    expect((input as any)._runtime.allowed_tools).not.toContain("fetch_url");
    expect((input as any)._runtime.allowed_tools).not.toContain("send_email");
    expect(appSource).toMatch(/if \(hasToolChoices\(catalog\.toolCatalog, \{ app: appToolLayer \}\)\)\s*input = withRunTools\(input, planRunTools\(catalog\.toolCatalog, \{ app: appToolLayer \}\)\)/);
  });
  it("no choice: nothing is sent, the gateway's grant applies", () => {
    expect(withToolLayer({ prompt: "x", tools: ["a"], _runtime: { allowed_tools: ["a"], tool_policy: {} } }, {}, catalog)).toEqual({ prompt: "x" });
  });
  it("the client approver answers Auto tools only (Ask waits for the person)", () => {
    expect(toWorkflowApprovalPolicy(planRunTools(catalog, { app: layer }))).toEqual({
      enabledTools: ["execute_command", "list_files", "read_file", "write_file"],
      autoApproveTools: ["list_files", "read_file"],
    });
    expect(appSource).toContain("toWorkflowApprovalPolicy(planRunTools(catalog.toolCatalog, { app: appToolLayer }, { ceiling:");
  });
  it("an automation's definition reads back as the same layer (no revision for nothing)", () => {
    const input = withRunTools({ prompt: "x" }, planRunTools(catalog, { app: layer }));
    // execute_command's Ask is its gateway default: the layer's effective choices come back.
    expect(inputToolLayer(input, catalog)).toEqual({ read_file: "auto", fetch_url: "off" });
  });
});

describe('"Allow all enabled tools" and its Revoke', () => {
  it("Allow all sets Auto on what the bulk rule reaches; the strip says so; Revoke removes the Auto choices only", () => {
    const all = withAllEnabledAuto(catalog, { execute_command: "off" });
    expect(all).toEqual({ execute_command: "off", read_file: "auto", write_file: "auto" });
    expect(allEnabledAuto(catalog, all)).toBe(true);
    expect(allEnabledAuto(catalog, {})).toBe(false);
    expect(withoutAutoChoices(catalog, all)).toEqual({ execute_command: "off" });
    expect(appSource).toContain('approveAllScope: "in this browser"');
  });
});
