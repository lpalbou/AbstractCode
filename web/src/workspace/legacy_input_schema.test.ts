import { beforeEach, describe, expect, it, vi } from "vitest";

import { buildWorkflowInput, type WorkflowDefinition } from "./catalog";
import {
  normalizeInputSchema,
  reconcileVisualFlowSchema,
  schemaDefaults,
  validateWorkflowInputs,
} from "./input_schema";
import { gatewayRequest } from "./transport";
import { fetchWorkflowSchema } from "./use_workspace_catalog";

vi.mock("./transport", () => ({
  gatewayRequest: vi.fn(),
  formatError: (error: Error) => error.message,
}));

const workflow: WorkflowDefinition = {
  id: "private:basic-agent@0.0.4:root",
  workflowId: "basic-agent@0.0.4:root",
  bundleId: "basic-agent",
  bundleVersion: "0.0.4",
  flowId: "root",
  name: "Basic agent",
  description: "",
  interfaces: ["abstractcode.agent.v1"],
  registryScope: "private",
};

type Pin = {
  id: string;
  type: string;
  required?: boolean;
  schema?: Record<string, unknown>;
};

function source(pins: Pin[] = [], defaults: Record<string, unknown> = {}) {
  return {
    id: "root",
    entryNode: "start",
    nodes: [
      {
        id: "start",
        type: "on_flow_start",
        data: {
          nodeType: "on_flow_start",
          outputs: [
            { id: "exec-out", type: "execution" },
            { id: "prompt", type: "string" },
            { id: "context", type: "object" },
            { id: "memory", type: "memory" },
            { id: "provider", type: "provider" },
            { id: "model", type: "model" },
            { id: "system", type: "string" },
            { id: "max_in_tokens", type: "number" },
            { id: "resp_schema", type: "object" },
            ...pins,
          ],
          pinDefaults: defaults,
        },
      },
    ],
    edges: [],
  };
}

/** The OLD Gateway inferred required solely from absent pinDefaults and
 * wrote that invented value into BOTH schema.required and inputs.required. */
function legacyDescriptor(flow = source()) {
  const data = flow.nodes[0].data;
  const defaults = data.pinDefaults;
  const inputs = data.outputs
    .filter((pin) => pin.type !== "execution")
    .map((pin) => {
      const hasDefault = Object.hasOwn(defaults, pin.id);
      const type = ["provider", "model"].includes(pin.type)
        ? "string"
        : pin.type === "memory"
          ? "object"
          : pin.type;
      const schema = {
        type,
        "x-abstract-type": pin.type,
        ...("schema" in pin ? pin.schema : {}),
        ...(hasDefault ? { default: defaults[pin.id] } : {}),
      };
      return {
        ...pin,
        required: !hasDefault,
        schema,
        ...(hasDefault ? { default: defaults[pin.id] } : {}),
      };
    });
  return {
    version: 1,
    bundle_id: "basic-agent",
    bundle_version: "0.0.4",
    flow_id: "root",
    inputs,
    defaults,
    input_data_schema: {
      type: "object",
      additionalProperties: true,
      properties: Object.fromEntries(inputs.map((pin) => [pin.id, pin.schema])),
      required: inputs.filter((pin) => pin.required).map((pin) => pin.id),
    },
  };
}

const rawResponse = (flow = source()) => ({
  bundle_id: "basic-agent",
  bundle_version: "0.0.4",
  flow_id: "root",
  flow,
});

describe("legacy VisualFlow requirements", () => {
  beforeEach(() => vi.mocked(gatewayRequest).mockReset());

  it("does not fabricate optional Basic Agent values to satisfy stale required metadata", () => {
    const flow = source();
    const stale = normalizeInputSchema(legacyDescriptor(flow))!;
    const payload = buildWorkflowInput({
      workflow,
      prompt: "Hello",
      schemaInputs: {},
    });
    expect(validateWorkflowInputs(stale, payload)).toEqual(
      expect.arrayContaining([
        "memory is required.",
        "provider is required.",
        "model is required.",
        "system is required.",
        "max_in_tokens is required.",
        "resp_schema is required.",
      ]),
    );
    const repaired = reconcileVisualFlowSchema(stale, flow);
    expect(validateWorkflowInputs(repaired, payload)).toEqual([]);
    for (const key of [
      "memory",
      "provider",
      "model",
      "system",
      "max_in_tokens",
      "resp_schema",
    ])
      expect(payload).not.toHaveProperty(key);
    expect(stale.required).toContain("provider");
  });

  it("retains genuinely authored custom requirements and field constraints", () => {
    const flow = source([
      {
        id: "ticket",
        type: "string",
        required: true,
        schema: { minLength: 1 },
      },
    ]);
    const schema = reconcileVisualFlowSchema(
      normalizeInputSchema(legacyDescriptor(flow))!,
      flow,
    );
    expect(validateWorkflowInputs(schema, {})).toEqual(["ticket is required."]);
    expect(validateWorkflowInputs(schema, { ticket: "" })).toEqual([
      "ticket must contain at least 1 characters.",
    ]);
    expect(validateWorkflowInputs(schema, { ticket: "ISSUE-42" })).toEqual([]);
  });

  it("preserves false, zero, empty, and null defaults from the original workflow", () => {
    const flow = source(
      [
        { id: "enabled", type: "boolean", required: true },
        { id: "budget", type: "number" },
        { id: "label", type: "string" },
        {
          id: "nullable",
          type: "object",
          schema: { type: ["object", "null"] },
        },
      ],
      { enabled: false, budget: 0, label: "", nullable: null },
    );
    const schema = reconcileVisualFlowSchema(
      normalizeInputSchema(legacyDescriptor(flow))!,
      flow,
    );
    expect(schemaDefaults(schema)).toEqual({
      enabled: false,
      budget: 0,
      label: "",
      nullable: null,
    });
    expect(schema.required).toContain("enabled");
    expect(validateWorkflowInputs(schema, {})).toEqual([]);
  });

  it("validates a real provider requirement against the final override-composed payload", () => {
    const flow = source();
    Object.assign(
      flow.nodes[0].data.outputs.find((pin) => pin.id === "provider")!,
      { required: true },
    );
    Object.assign(
      flow.nodes[0].data.outputs.find((pin) => pin.id === "model")!,
      { required: true },
    );
    const schema = reconcileVisualFlowSchema(
      normalizeInputSchema(legacyDescriptor(flow))!,
      flow,
    );
    expect(validateWorkflowInputs(schema, {})).toEqual([
      "provider is required.",
      "model is required.",
    ]);
    const payload = buildWorkflowInput({
      workflow,
      prompt: "Hello",
      model: { provider: "fixture", model: "fixture-model" },
    });
    expect(validateWorkflowInputs(schema, payload)).toEqual([]);
    expect(payload).toMatchObject({
      provider: "fixture",
      model: "fixture-model",
    });
  });

  it("fetches source from the selected private bundle version", async () => {
    vi.mocked(gatewayRequest)
      .mockResolvedValueOnce(legacyDescriptor())
      .mockResolvedValueOnce(rawResponse());
    const schema = await fetchWorkflowSchema(workflow);
    expect(validateWorkflowInputs(schema, {})).toEqual([]);
    expect(vi.mocked(gatewayRequest).mock.calls.map(([path]) => path)).toEqual([
      "/api/gateway/bundles/basic-agent/flows/root/input_schema?bundle_version=0.0.4",
      "/api/gateway/bundles/basic-agent/flows/root?bundle_version=0.0.4",
    ]);
  });

  it("never substitutes a private source for a same-named tenant catalog workflow", async () => {
    vi.mocked(gatewayRequest)
      .mockResolvedValueOnce({
        ...legacyDescriptor(),
        registry_scope: "tenant_catalog",
      })
      .mockResolvedValueOnce({
        ...rawResponse(),
        registry_scope: "tenant_catalog",
      });
    await fetchWorkflowSchema({ ...workflow, registryScope: "tenant_catalog" });
    expect(vi.mocked(gatewayRequest).mock.calls.map(([path]) => path)).toEqual([
      "/api/gateway/workflow-catalog/basic-agent/versions/0.0.4/flows/root/input_schema?scope=tenant",
      "/api/gateway/workflow-catalog/basic-agent/versions/0.0.4/flows/root?scope=tenant",
    ]);
  });

  it("leaves native-loop requirements authoritative without fetching a VisualFlow", async () => {
    vi.mocked(gatewayRequest).mockResolvedValueOnce({
      ...legacyDescriptor(),
      native_loop_factory: "react",
    });
    const schema = await fetchWorkflowSchema(workflow);
    expect(schema?.required).toContain("provider");
    expect(gatewayRequest).toHaveBeenCalledTimes(1);
  });

  it("leaves standalone fetched JSON schemas authoritative", async () => {
    vi.mocked(gatewayRequest).mockResolvedValueOnce({
      type: "object",
      properties: { ticket: { type: "string" } },
      required: ["ticket"],
    });
    const schema = await fetchWorkflowSchema(workflow);
    expect(validateWorkflowInputs(schema, {})).toEqual(["ticket is required."]);
    expect(gatewayRequest).toHaveBeenCalledTimes(1);
  });

  it("leaves inline JSON schemas authoritative and performs no lookup", async () => {
    const schema = await fetchWorkflowSchema({
      ...workflow,
      inputSchema: {
        type: "object",
        properties: { ticket: { type: "string" } },
        required: ["ticket"],
      },
    });
    expect(validateWorkflowInputs(schema, {})).toEqual(["ticket is required."]);
    expect(gatewayRequest).not.toHaveBeenCalled();
  });

  it("does not relax validation when source authorization fails", async () => {
    const denied = Object.assign(new Error("Forbidden"), { status: 403 });
    vi.mocked(gatewayRequest)
      .mockResolvedValueOnce(legacyDescriptor())
      .mockRejectedValueOnce(denied);
    await expect(fetchWorkflowSchema(workflow)).rejects.toThrow();
  });

  it.each([
    { bundle_version: "0.0.5" },
    { bundle_id: "other-agent" },
    { flow_id: "other-flow" },
  ])("rejects source identity drift: %j", async (mismatch) => {
    vi.mocked(gatewayRequest)
      .mockResolvedValueOnce(legacyDescriptor())
      .mockResolvedValueOnce({ ...rawResponse(), ...mismatch });
    await expect(fetchWorkflowSchema(workflow)).rejects.toThrow();
  });
});
