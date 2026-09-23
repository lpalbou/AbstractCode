import { describe, expect, it } from "vitest";
import { buildWorkflowInput, type WorkflowDefinition } from "./catalog";
import {
  normalizeInputSchema,
  schemaDefaults,
  validateWorkflowInputs,
  modelInputGroups,
} from "./input_schema";

const workflow = (interfaces: string[]): WorkflowDefinition => ({
  id: "tenant_catalog:assistant@1:chat",
  workflowId: "assistant@1:chat",
  bundleId: "assistant",
  bundleVersion: "1",
  flowId: "chat",
  name: "Assistant",
  description: "",
  interfaces,
  registryScope: "tenant_catalog",
});

describe("Gateway workflow input contract regressions", () => {
  it("preserves semantic pin metadata, envelope defaults, and author-required fields", () => {
    const schema = normalizeInputSchema({
      inputs: [
        { id: "provider", type: "provider", label: "Provider" },
        { id: "model", type: "model", label: "Model" },
        { id: "thinking", type: "string", label: "Reasoning" },
      ],
      defaults: {
        provider: "endpoint:flow",
        model: "flow-model",
        thinking: "low",
      },
      input_data_schema: {
        type: "object",
        required: ["ticket"],
        properties: {
          ticket: { type: "string", minLength: 1 },
          provider: { type: "string" },
          model: { type: "string" },
          thinking: { type: "string" },
        },
      },
    });
    expect(schema?.required).toEqual(["ticket"]);
    expect(schema?.properties.provider["x-abstract-type"]).toBe("provider");
    expect(schemaDefaults(schema)).toEqual({
      provider: "endpoint:flow",
      model: "flow-model",
      thinking: "low",
    });
    expect(modelInputGroups(schema)).toEqual([
      {
        provider: "provider",
        model: "model",
        reasoning: "thinking",
        title: "Text model",
      },
    ]);
    expect(validateWorkflowInputs(schema, {})).toEqual(["ticket is required."]);
    expect(validateWorkflowInputs(schema, { ticket: "" })).toEqual([
      "ticket must contain at least 1 characters.",
    ]);
  });

  it("accepts falsy defaults and nullable values without inventing omitted fields", () => {
    const schema = normalizeInputSchema({
      defaults: { enabled: false, limit: 0, provider: "", resp_schema: null },
      input_data_schema: {
        type: "object",
        required: ["enabled", "limit", "provider", "resp_schema"],
        properties: {
          enabled: { type: "boolean" },
          limit: { type: "integer" },
          provider: { type: "string" },
          resp_schema: { type: ["object", "null"] },
          max_in_tokens: { type: "number" },
          primary_image_artifact: { type: "object" },
          image_provider: { type: "string" },
        },
      },
    });
    expect(schemaDefaults(schema)).toEqual({
      enabled: false,
      limit: 0,
      provider: "",
      resp_schema: null,
    });
    expect(validateWorkflowInputs(schema, {})).toEqual([]);
    expect(validateWorkflowInputs(schema, { resp_schema: {} })).toEqual([]);
    expect(validateWorkflowInputs(schema, { max_in_tokens: null })).toEqual([
      "max_in_tokens must be a number.",
    ]);
  });

  it("checks nullable enum constraints rather than treating null as universally valid", () => {
    const schema = {
      type: "object",
      properties: { mode: { type: ["string", "null"], enum: ["on", "off"] } },
    };
    expect(validateWorkflowInputs(schema, { mode: null })).toEqual([
      "mode must be one of the available choices.",
    ]);
    expect(
      validateWorkflowInputs(
        {
          ...schema,
          properties: {
            mode: { type: ["string", "null"], enum: ["on", null] },
          },
        },
        { mode: null },
      ),
    ).toEqual([]);
  });

  it("keeps independently cloned defaults so changing a draft cannot mutate the schema", () => {
    const schema = {
      properties: { context: { type: "object", default: { flags: [false] } } },
    };
    const draft = schemaDefaults(schema);
    (draft.context as { flags: boolean[] }).flags.push(true);
    expect(schemaDefaults(schema)).toEqual({ context: { flags: [false] } });
  });

  it.each(["abstractcode.agent.v1", "abstractassistant.agent.v1"])(
    "keeps workflow defaults, but no absent route pins, for %s",
    (agentInterface) => {
      const options = {
        workflow: workflow([agentInterface]),
        prompt: "new prompt",
        model: { provider: "", model: "" },
      };
      const inherited = buildWorkflowInput(options);
      expect(inherited).not.toHaveProperty("provider");
      expect(inherited).not.toHaveProperty("model");
      expect(inherited).not.toHaveProperty("_runtime");
      const defaults = {
        provider: "endpoint:flow",
        model: "flow-model",
        system: "Authored instructions",
        max_iterations: 24,
      };
      const authored = buildWorkflowInput({
        ...options,
        schemaInputs: defaults,
      });
      expect(authored).toMatchObject(defaults);
      expect(authored).not.toHaveProperty("_runtime");
      const overridden = buildWorkflowInput({
        ...options,
        schemaInputs: defaults,
        model: { provider: "endpoint:request", model: "request-model" },
      });
      expect(overridden).toMatchObject({
        provider: "endpoint:request",
        model: "request-model",
        _runtime: { provider: "endpoint:request", model: "request-model" },
      });
    },
  );

  it("rebuilds current assistant task and delegates history to Gateway after a restored turn", () => {
    const previous = {
      prompt: "yesterday",
      context: {
        task: "yesterday",
        messages: [{ role: "user", content: "old" }],
        project: "keep-project",
      },
      use_context: true,
    };
    const payload = buildWorkflowInput({
      workflow: workflow(["abstractassistant.agent.v1"]),
      schemaInputs: previous,
      prompt: "today",
    });
    expect(payload).toMatchObject({
      prompt: "today",
      context: { task: "today", project: "keep-project" },
      use_context: false,
      use_session_history: true,
    });
    expect(payload.context).not.toHaveProperty("messages");
    expect(previous.context.messages).toEqual([
      { role: "user", content: "old" },
    ]);
  });

  it("accepts intentionally supplied client context without reviving old messages", () => {
    const payload = buildWorkflowInput({
      workflow: workflow(["abstractassistant.agent.v1"]),
      prompt: "now",
      messages: [{ role: "assistant", content: "explicit" }],
    });
    expect(payload).toMatchObject({
      use_context: true,
      context: {
        task: "now",
        messages: [{ role: "assistant", content: "explicit" }],
      },
    });
  });

  it("does not inject agent controls into an arbitrary workflow", () => {
    const source = {
      topic: "release",
      context: { task: "owned-by-flow" },
      provider: "flow-provider",
      model: "flow-model",
    };
    const payload = buildWorkflowInput({
      workflow: workflow(["acme.report.v1"]),
      schemaInputs: source,
      prompt: "unused",
      model: { provider: "request", model: "request" },
      tools: [],
      reasoning: "high",
      useSessionHistory: true,
    });
    expect(payload).toEqual({ ...source, _runtime: { provider: "request", model: "request", allowed_tools: [], thinking: "high" } });
    expect(payload).not.toHaveProperty("use_session_history");
  });
});
