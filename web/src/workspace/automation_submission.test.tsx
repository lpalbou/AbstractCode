import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { NewAutomationDialog } from "./automations_view";
import { buildWorkflowInput, type WorkflowDefinition } from "./catalog";
import type { AutomationsController } from "./automations";

const captured = vi.hoisted(() => ({ submit: undefined as undefined | ((body: any) => Promise<any>) }));
vi.mock("@abstractframework/ui-kit", async (original) => ({
  ...await original<typeof import("@abstractframework/ui-kit")>(),
  AfScheduleDialog: (props: any) => { captured.submit = props.onSubmit; return null; },
}));

const workflow: WorkflowDefinition = {
  id: "agent", workflowId: "agent@1:main", bundleId: "agent", bundleVersion: "1",
  flowId: "main", name: "Agent", description: "", interfaces: ["abstractcode.agent.v1"],
};

describe("automation submission uses the current workflow input builder", () => {
  it.each([{ tools: [] }, { tools: ["web_search"] }])("forwards an explicit tool selection $tools and changed model", async ({ tools }) => {
    const create = vi.fn(async (_body: any) => ({ automation_id: "new-automation" }));
    const onCreated = vi.fn(), onClose = vi.fn();
    const ctl = { state: { busy: false }, create } as unknown as AutomationsController;
    const body = {
      title: "Price tracker", context: { mode: "growing", growing: { max_tokens: 30000 } },
      policy: { tool_approval: "auto" },
      trigger: { source_id: "schedule", source_version: 1, config: { interval: "1d" } },
      target: { bundle_ref: "agent@1", flow_id: "main", input_data: { prompt: "Find current prices" } },
    };
    const restored = {
      provider: "old-provider", model: "old-model", tools: ["write_file"],
      context: { task: "Old conversation", messages: [{ role: "user", content: "old" }], attachments: [{ $artifact: "old" }] },
      _runtime: { provider: "old-provider", model: "old-model" },
    };
    let model = { provider: "old-provider", model: "old-model" };
    const buildInput = vi.fn((prompt: string) => buildWorkflowInput({ workflow, schemaInputs: restored, model, tools, prompt }));
    renderToStaticMarkup(<NewAutomationDialog open target={body.target} initialPrompt="Old conversation"
      workflowLabel="Agent" ctl={ctl} onCreated={onCreated} onClose={onClose} buildInput={buildInput} />);
    expect(buildInput).not.toHaveBeenCalled();
    model = { provider: "mlx", model: "selected-model" };
    await captured.submit!(body);
    expect(buildInput).toHaveBeenCalledWith("Find current prices", undefined);
    const sent = create.mock.calls[0][0];
    expect(sent.target.input_data).toMatchObject({
      prompt: "Find current prices", provider: "mlx", model: "selected-model", tools,
      context: { task: "Find current prices" },
      _runtime: { provider: "mlx", model: "selected-model", allowed_tools: tools },
    });
    expect(sent.target.input_data.context).not.toHaveProperty("messages");
    expect(sent.target.input_data.context).not.toHaveProperty("attachments");
    expect({ ...sent, target: body.target }).toEqual(body);
    expect(body.target.input_data).toEqual({ prompt: "Find current prices" });
    expect(restored.context.task).toBe("Old conversation");
    expect(onCreated).toHaveBeenCalledWith("new-automation");
    expect(onClose).toHaveBeenCalledOnce();
  });
});
