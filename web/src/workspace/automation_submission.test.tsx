import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { NewAutomationDialog } from "./automations_view";
import { buildWorkflowInput, type WorkflowDefinition } from "./catalog";
import type { AutomationsController } from "./automations";

const captured = vi.hoisted(() => ({ submit: undefined as undefined | ((body: any) => Promise<any>), props: undefined as any }));
vi.mock("@abstractframework/ui-kit", async (original) => ({
  ...await original<typeof import("@abstractframework/ui-kit")>(),
  AfScheduleDialog: (props: any) => { captured.submit = props.onSubmit; captured.props = props; return null; },
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

describe("R16.1: the schedule dialog's When is served by the gateway", () => {
  it("passes the controller's previewSchedule (POST schedule-preview) and the preferences opener", async () => {
    const previewSchedule = vi.fn(async (trigger: any) => ({ trigger, time_zone: "Europe/Paris", schedule_rule_text: "Every day at 08:00 (Europe/Paris)", schedule_text: "x", next_run_at: null, next_run_local: null, first_run_sentence: "Runs every day at 08:00 (Europe/Paris), first run Fri 9 Oct 08:00." }));
    const ctl = { state: { busy: false }, previewSchedule } as unknown as AutomationsController;
    const onOpenPreferences = vi.fn();
    renderToStaticMarkup(<NewAutomationDialog open target={null} workflowLabel="Agent" ctl={ctl} onCreated={() => undefined} onClose={() => undefined} onOpenPreferences={onOpenPreferences} />);
    expect(captured.props.previewSchedule).toBe(previewSchedule);
    expect(captured.props.onOpenPreferences).toBe(onOpenPreferences);
    const trigger = { source_id: "schedule", source_version: 2, config: { kind: "daily", at: "08:00" } };
    expect((await captured.props.previewSchedule(trigger)).first_run_sentence).toBe("Runs every day at 08:00 (Europe/Paris), first run Fri 9 Oct 08:00.");
    expect(previewSchedule).toHaveBeenCalledWith(trigger);
  });
});
