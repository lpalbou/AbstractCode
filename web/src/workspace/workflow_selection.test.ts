import { describe, expect, it } from "vitest";
import type { WorkflowDefinition } from "./catalog";
import { parsePreferences } from "./preferences";
import { DEFAULT_PREFERENCES } from "./settings_panel";
import {
  GATEWAY_DEFAULT,
  NO_DEFAULT_REPORTED,
  gatewayDefaultDefinition,
  gatewayDefaultFromEnvelope,
  gatewayDefaultOptionLabel,
  reconcileSelection,
  resolvedWorkflowNote,
  startRunBody,
  visibleWorkflowChoices,
} from "./workflow_selection";

const agent: WorkflowDefinition = {
  id: "private:coding-agent@0.1.0:coder",
  workflowId: "coding-agent@0.1.0:coder",
  bundleId: "coding-agent",
  bundleVersion: "0.1.0",
  flowId: "coder",
  name: "Coder",
  description: "",
  interfaces: ["abstractcode.agent.v1"],
  registryScope: "private",
};
const report: WorkflowDefinition = {
  ...agent,
  id: "private:reports@1.0.0:weekly",
  workflowId: "reports@1.0.0:weekly",
  bundleId: "reports",
  bundleVersion: "1.0.0",
  flowId: "weekly",
  name: "Weekly report",
  interfaces: [],
};
const envelope = {
  items: [],
  default_agent_workflows: {
    "abstractcode.agent.v1": {
      workflow_id: "coding-agent@0.1.0:coder",
      bundle_id: "coding-agent",
      bundle_version: "0.1.0",
      flow_id: "coder",
      registry_scope: "private",
      name: "Coder",
      source: "saved",
    },
  },
};

describe("gateway default agent workflow", () => {
  it("reads the default from the /bundles envelope and labels the first entry", () => {
    const state = gatewayDefaultFromEnvelope(envelope);
    expect(state).toMatchObject({
      status: "ok",
      workflow: { bundleId: "coding-agent", flowId: "coder", name: "Coder" },
    });
    expect(gatewayDefaultOptionLabel(state)).toBe(
      "Gateway default → Coder @0.1.0",
    );
  });

  it("says loudly when the gateway does not report a default", () => {
    const state = gatewayDefaultFromEnvelope({ items: [] });
    expect(state).toEqual({ status: "unavailable", reason: NO_DEFAULT_REPORTED });
    expect(gatewayDefaultOptionLabel(state)).toBe(
      "Gateway default — gateway does not report a default agent workflow",
    );
    expect(
      gatewayDefaultFromEnvelope({ default_agent_workflows: {} }),
    ).toMatchObject({ status: "unavailable" });
    expect(gatewayDefaultFromEnvelope(undefined)).toMatchObject({
      status: "unavailable",
    });
  });

  it("resolves the catalog row the default points at, or the gateway's description", () => {
    const state = gatewayDefaultFromEnvelope(envelope);
    expect(gatewayDefaultDefinition(state, [report, agent])).toBe(agent);
    const synthesized = gatewayDefaultDefinition(state, [report]);
    expect(synthesized).toMatchObject({
      bundleId: "coding-agent",
      bundleVersion: "0.1.0",
      flowId: "coder",
      interfaces: ["abstractcode.agent.v1"],
    });
    expect(
      gatewayDefaultDefinition({ status: "unavailable", reason: "x" }, [agent]),
    ).toBeNull();
  });
});

describe("workflow selection", () => {
  it("lists agent workflows unless every workflow is requested", () => {
    expect(visibleWorkflowChoices([agent, report], false)).toEqual([agent]);
    expect(visibleWorkflowChoices([agent, report], true)).toEqual([agent, report]);
  });

  it("keeps a valid choice, then the saved preference, then the gateway default", () => {
    const base = { visible: [agent], workflows: [agent, report], runId: "" };
    expect(reconcileSelection({ ...base, selection: GATEWAY_DEFAULT, preferred: agent.id })).toBe(GATEWAY_DEFAULT);
    expect(reconcileSelection({ ...base, selection: "gone", preferred: agent.id })).toBe(agent.id);
    expect(reconcileSelection({ ...base, selection: "gone", preferred: "also-gone" })).toBe(GATEWAY_DEFAULT);
    // A hidden (non-agent) workflow is not offered for a new conversation...
    expect(reconcileSelection({ ...base, selection: report.id, preferred: GATEWAY_DEFAULT })).toBe(GATEWAY_DEFAULT);
    // ...but a restored conversation keeps its run's workflow.
    expect(reconcileSelection({ ...base, runId: "run-1", selection: report.id, preferred: GATEWAY_DEFAULT })).toBe(report.id);
  });

  it("sends the sentinel with the interface and no bundle fields for the gateway default", () => {
    const body = startRunBody({
      selection: GATEWAY_DEFAULT,
      workflow: agent,
      sessionId: "s1",
      input: { prompt: "hi" },
    });
    expect(body).toEqual({
      flow_id: "@default",
      interface: "abstractcode.agent.v1",
      session_id: "s1",
      input_data: { prompt: "hi" },
    });
  });

  it("sends the exact bundle, version, scope and flow for an explicit choice", () => {
    expect(
      startRunBody({ selection: agent.id, workflow: agent, sessionId: "s1", input: {} }),
    ).toEqual({
      bundle_id: "coding-agent",
      bundle_version: "0.1.0",
      registry_scope: "private",
      flow_id: "coder",
      session_id: "s1",
      input_data: {},
    });
  });

  it("shows what the gateway resolved, and says when it did not report it", () => {
    expect(
      resolvedWorkflowNote({
        workflow_id: "coding-agent@0.1.0:coder",
        name: "Coder",
        bundle_version: "0.1.0",
        source: "gateway_default",
      }),
    ).toEqual({ text: "running Coder @0.1.0 (gateway default)", missing: false });
    expect(resolvedWorkflowNote({ name: "Coder", bundle_version: "0.1.0", source: "client" }).text).toBe(
      "running Coder @0.1.0",
    );
    expect(resolvedWorkflowNote(undefined)).toMatchObject({ missing: true });
  });
});

describe("workflow preference persistence", () => {
  it("defaults a fresh browser to the gateway default", () => {
    expect(DEFAULT_PREFERENCES.workflow).toBe(GATEWAY_DEFAULT);
    expect(parsePreferences(null).workflow).toBe(GATEWAY_DEFAULT);
    expect(parsePreferences("{not json").workflow).toBe(GATEWAY_DEFAULT);
    expect(parsePreferences(JSON.stringify({ model: "x" })).showAllWorkflows).toBe(false);
  });

  it("round-trips the sentinel and explicit ids verbatim", () => {
    const saved = { ...DEFAULT_PREFERENCES, workflow: GATEWAY_DEFAULT };
    expect(parsePreferences(JSON.stringify(saved)).workflow).toBe("@default");
    const explicit = { ...DEFAULT_PREFERENCES, workflow: agent.id, showAllWorkflows: true };
    expect(parsePreferences(JSON.stringify(explicit))).toMatchObject({
      workflow: agent.id,
      showAllWorkflows: true,
    });
  });

});
