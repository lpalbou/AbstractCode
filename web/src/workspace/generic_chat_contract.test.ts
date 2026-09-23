import { describe, it, expect } from "vitest";
import { buildWorkflowInput, type WorkflowDefinition } from "./catalog";
import { workflowPromptProperty, schemaDefaults, validateWorkflowInputs, restoreWorkflowFields } from "./input_schema";
const workflow: WorkflowDefinition = { id: "coding", workflowId: "coding", flowId: "coding", name: "Coding", description: "", interfaces: ["abstractcode.coding.v1"] };
const schema = { type: "object", properties: {
  request: { type: "string", default: "Authored request" }, workspace_root: { type: "string", default: "" },
  build_command: { type: "string", default: "" }, run_command: { type: "string", default: "" }, max_rounds: { type: "number", default: 3 },
  provider: { type: "string" }, model: { type: "string" },
} };
describe("schema-driven generic chat", () => {
  it("restores public fields without pinning next turns to prior host grants or routing", () => {
    const restored = restoreWorkflowFields(schema, { request: "Earlier", max_rounds: 5, provider: "custom", model: "custom", _runtime: { provider: "custom", model: "custom", allowed_tools: [], tool_policy: { auto_approve_tools: ["write_file"] } } });
    expect(restored).toEqual({ request: "Earlier", max_rounds: 5, provider: "custom", model: "custom" });
    expect(buildWorkflowInput({ workflow, schemaInputs: restored, tools: ["read_file"] })).toMatchObject({ _runtime: { allowed_tools: ["read_file"] } });
    expect(buildWorkflowInput({ workflow, schemaInputs: restored })).not.toHaveProperty("_runtime");
  });
  it("keeps the validated workspace envelope even without a public workspace pin", () => {
    expect(buildWorkflowInput({ workflow, inputSchema: { properties: {} }, workspace: { root: "/gateway/session" } })).toEqual({ workspace_root: "/gateway/session" });
  });
  it("maps coding request without fabricating commands, agent context or route defaults", () => {
    expect(workflowPromptProperty(schema)).toBe("request");
    const input = buildWorkflowInput({ workflow, inputSchema: schema, schemaInputs: schemaDefaults(schema), promptProperty: workflowPromptProperty(schema), prompt: "Build a game" });
    expect(input).toEqual({ request: "Build a game", workspace_root: "", build_command: "", run_command: "", max_rounds: 3 });
  });
  it("does not erase default/form request when Run workflow has no composer text", () => {
    const input = buildWorkflowInput({ workflow, inputSchema: schema, schemaInputs: schemaDefaults(schema), promptProperty: "request", prompt: "" });
    expect(input.request).toBe("Authored request");
  });
  it("binds host overrides only to declared pins and forwards reserved Runtime scope for every workflow", () => {
    const input = buildWorkflowInput({ workflow, inputSchema: schema, schemaInputs: schemaDefaults(schema), workspace: { root: "/authorized/session" }, model: { provider: "endpoint:test", model: "reasoner" }, tools: [], toolPolicy: { requireApprovalTools: ["write_file"] } });
    expect(input).toMatchObject({ workspace_root: "/authorized/session", provider: "endpoint:test", model: "reasoner", _runtime: { allowed_tools: [], provider: "endpoint:test", model: "reasoner", tool_policy: { require_approval_tools: ["write_file"] } } });
    expect(input).not.toHaveProperty("tools");
    expect(input).not.toHaveProperty("context");
  });
  it("honors text/nullable/semantic types and does not bypass genuine required inputs", () => {
    expect(workflowPromptProperty({ properties: { request: { type: ["string", "null"] } } })).toBe("request");
    expect(workflowPromptProperty({ properties: { request: { type: "object" } } })).toBeUndefined();
    expect(workflowPromptProperty({ properties: { ticket: { type: "string", "x-abstract-role": "prompt" } } })).toBe("ticket");
    expect(validateWorkflowInputs({ ...schema, required: ["ticket"] }, schemaDefaults(schema))).toEqual(["ticket is required."]);
  });
  it("intersects explicit selection with a workflow's authored Runtime ceiling", () => {
    expect(buildWorkflowInput({ workflow, schemaInputs: { _runtime: { allowed_tools: ["read_file"] } }, tools: ["read_file", "write_file"] })).toEqual({ _runtime: { allowed_tools: ["read_file"] } });
  });
});
