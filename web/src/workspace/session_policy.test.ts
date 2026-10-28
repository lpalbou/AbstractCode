// "Allow for this chat" (backlog 0032, session tool policy contract v1): the kit client, bound to
// this conversation's session, only when the gateway serves capabilities.session_tool_policy.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { createSessionToolPolicyClient, sessionToolPolicyCapability } from "@abstractframework/ui-kit";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const catalogSource = readFileSync(new URL("./use_workspace_catalog.ts", import.meta.url), "utf8");
const settingsSource = readFileSync(new URL("./settings_panel.tsx", import.meta.url), "utf8");

describe("chat tool policy", () => {
  it("reads the capability the kit's way from GET discovery/capabilities", () => {
    expect(catalogSource).toContain("sessionToolPolicy: sessionToolPolicyCapability(value(4))");
    expect(sessionToolPolicyCapability({ capabilities: { session_tool_policy: { supported: true, version: 1 } } })?.supported).toBe(true);
    expect(sessionToolPolicyCapability({ capabilities: {} })).toBeNull();
  });

  it("binds the kit client to this conversation only when supported; a failed read is said", () => {
    expect(appSource).toMatch(/sessionPolicyCapability\?\.supported === true && session\.sessionId\s*\?\s*createSessionToolPolicyClient\(/);
    expect(appSource).toContain("controller.setSessionToolPolicyClient(null, { reason: catalog.capabilitiesError })");
    expect(appSource).toMatch(/sessionPolicy: \{\s*toolsetOf:/);
  });

  it("a client for an unsupported gateway never sends a request", async () => {
    const calls: string[] = [];
    const client = createSessionToolPolicyClient(async (path) => (calls.push(path), {}), "s1", null);
    await expect(client.get()).rejects.toThrow();
    expect(calls).toEqual([]);
  });

  it("never claims gateway storage for browser preferences", () => {
    expect(settingsSource).not.toContain("Saved for this Gateway account");
    expect(appSource).not.toContain("for this Gateway account");
  });
});
