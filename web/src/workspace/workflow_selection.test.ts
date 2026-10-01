import { describe, expect, it } from "vitest";
import type { WorkflowDefinition } from "./catalog";
import { parsePreferences } from "./preferences";
import { DEFAULT_PREFERENCES } from "./settings_panel";
import {
  GATEWAY_DEFAULT,
  NO_DEFAULT_REPORTED,
  conversationSelection,
  defaultInterfaceMismatch,
  runSelectionSource,
  selectionSourceNote,
  gatewayDefaultDefinition,
  gatewayDefaultFromEnvelope,
  gatewayDefaultOptionLabel,
  reconcileSelection,
  resolvedWorkflowNote,
  startRunBody,
  executableChoices,
  pickerValue,
  selectionFromPicker,
} from "./workflow_selection";
import { parseExecutableWorkflows, WORKFLOW_PICKER_DEFAULT } from "@abstractframework/ui-kit";

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
    expect(defaultInterfaceMismatch(gatewayDefaultDefinition(state, [report, agent]))).toBe("");
    // A listed row that does not declare the interface is shown as a mismatch, not repaired.
    const undeclared = { ...agent, interfaces: ["abstractassistant.agent.v1"] };
    const listed = gatewayDefaultDefinition(state, [undeclared]);
    expect(listed).toBe(undeclared);
    expect(defaultInterfaceMismatch(listed)).toBe(
      "The gateway default Coder does not declare abstractcode.agent.v1 (it declares abstractassistant.agent.v1); it will not run as a coding agent here.",
    );
    // A default without a registry scope is not given one by the client.
    const noScope = JSON.parse(JSON.stringify(envelope));
    delete noScope.default_agent_workflows["abstractcode.agent.v1"].registry_scope;
    expect(gatewayDefaultFromEnvelope(noScope)).toMatchObject({ status: "unavailable" });
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
  // The header lists exactly GET /bundles?executable_for=abstractcode.agent.v1:
  // a bundle that does not declare the interface is not in that answer, and
  // there is no client switch that would bring it back.
  const executable = {
    executable_for: "abstractcode.agent.v1",
    items: [
      { bundle_id: "coding-agent", bundle_version: "0.1.0", registry_scope: "private", owner: { kind: "gateway", user_id: null }, shipped: true,
        entrypoints: [{ flow_id: "coder", name: "Coder", interfaces: ["abstractcode.agent.v1"], workflow_id: "coding-agent@0.1.0:coder" }] },
      { bundle_id: "mine", bundle_version: "0.0.1", registry_scope: "private", owner: { kind: "user", user_id: "u1" }, shipped: false,
        entrypoints: [{ flow_id: "main", name: "My agent", interfaces: ["abstractcode.agent.v1"], workflow_id: "mine@0.0.1:main" }] },
    ],
    default_agent_workflows: {},
  };

  it("lists exactly the gateway's executable_for answer (no client-side widening)", () => {
    const choices = executableChoices(parseExecutableWorkflows(executable, "abstractcode.agent.v1"));
    expect(choices.map((c) => c.id)).toEqual(["private:coding-agent@0.1.0:coder", "private:mine@0.0.1:main"]);
    expect(choices[0]).toMatchObject({ bundleId: "coding-agent", bundleVersion: "0.1.0", flowId: "coder", registryScope: "private", name: "Coder" });
    expect(choices.some((c) => c.id === report.id)).toBe(false);
    expect(executableChoices(null)).toEqual([]);
  });

  it("refuses a gateway answer that carries a workflow without the interface", () => {
    const leaky = { ...executable, items: [{ ...executable.items[0], entrypoints: [{ flow_id: "r", name: "Report", interfaces: ["abstractflow.prompt.v1"] }] }] };
    expect(() => parseExecutableWorkflows(leaky, "abstractcode.agent.v1")).toThrow(/does not declare abstractcode.agent.v1/);
    expect(() => parseExecutableWorkflows({ ...executable, executable_for: undefined }, "abstractcode.agent.v1")).toThrow(/does not filter workflows per app/);
  });

  it("maps picker values to selections and back", () => {
    const [entry] = parseExecutableWorkflows(executable, "abstractcode.agent.v1").entries;
    expect(selectionFromPicker(entry.value, entry)).toBe("private:coding-agent@0.1.0:coder");
    expect(selectionFromPicker(WORKFLOW_PICKER_DEFAULT, null)).toBe(GATEWAY_DEFAULT);
    expect(pickerValue(GATEWAY_DEFAULT, null)).toBe(WORKFLOW_PICKER_DEFAULT);
    expect(pickerValue(agent.id, agent)).toBe("coding-agent@0.1.0:coder");
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
    // The removed "Show all workflows" preference is dropped from old saves.
    expect(parsePreferences(JSON.stringify({ model: "x", showAllWorkflows: true }))).not.toHaveProperty("showAllWorkflows");
  });

  it("round-trips the sentinel and explicit ids verbatim", () => {
    const saved = { ...DEFAULT_PREFERENCES, workflow: GATEWAY_DEFAULT };
    expect(parsePreferences(JSON.stringify(saved)).workflow).toBe("@default");
    const explicit = { ...DEFAULT_PREFERENCES, workflow: agent.id };
    expect(parsePreferences(JSON.stringify(explicit))).toMatchObject({ workflow: agent.id });
  });

});

describe("gateway default per conversation (CONTRACTS A-4)", () => {
  it("uses the gateway's reason when it has no default for the interface", () => {
    expect(
      gatewayDefaultFromEnvelope({
        default_agent_workflows: {},
        default_agent_workflows_unavailable: {
          "abstractcode.agent.v1": { source: "default", value: null, reason: "basic-agent is not installed" },
        },
      }),
    ).toEqual({
      status: "unavailable",
      reason: "no default workflow for abstractcode.agent.v1: basic-agent is not installed",
    });
  });

  const runInputs = (selection?: Record<string, unknown>) => ({
    run_id: "r1",
    input_data: { prompt: "hi", ...(selection ? { workflow_selection: selection } : {}) },
  });

  it("keeps @default when the gateway says the last run came from its default, on any browser", () => {
    // A second browser has no local state at all: the run itself decides.
    const fromDefault = runInputs({ source: "gateway_default", interface: "abstractcode.agent.v1", workflow_id: agent.workflowId });
    expect(runSelectionSource(fromDefault)).toBe("gateway_default");
    expect(conversationSelection({ restoredInputs: fromDefault, restored: agent })).toBe(GATEWAY_DEFAULT);
    expect(conversationSelection({ restoredInputs: fromDefault, restored: undefined })).toBe(GATEWAY_DEFAULT);
    expect(selectionSourceNote(fromDefault)).toBe("");
  });

  it("keeps the exact workflow when the run was a client choice", () => {
    const fromClient = runInputs({ source: "client", workflow_id: report.workflowId });
    expect(conversationSelection({ restoredInputs: fromClient, restored: report })).toBe(report.id);
    expect(conversationSelection({ restoredInputs: fromClient, restored: undefined })).toBeUndefined();
    expect(selectionSourceNote(fromClient)).toBe("");
  });

  it("says so, visibly, when an older gateway does not record how the workflow was chosen", () => {
    const old = runInputs();
    expect(runSelectionSource(old)).toBeUndefined();
    expect(conversationSelection({ restoredInputs: old, restored: agent })).toBe(agent.id);
    expect(selectionSourceNote(old)).toContain("does not record whether this conversation used the gateway default");
    expect(selectionSourceNote(null)).toBe("");
  });
});
