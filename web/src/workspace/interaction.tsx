import React from "react";
import {
  JsonViewer,
  ToolActivityGroup,
  resolveWorkflowEventTarget,
  type ServerRecord,
  type WorkflowInteraction,
  type WorkflowRecord,
  type WorkflowWaitInteraction,
  type WorkflowSessionController,
} from "@abstractframework/panel-chat";

/** Converts runtime waits into controls. Subworkflow waits are never questions. */
export function presentInteraction(
  raw: WorkflowWaitInteraction | null,
  controller: WorkflowSessionController,
  records: WorkflowRecord[] = [],
  currentRun: ServerRecord | null = null,
  onPermissionsAll?: () => Promise<void>,
): WorkflowInteraction | null {
  if (!raw) return null;
  const wait = raw.wait as Record<string, any>;
  const id = `${raw.runId}:${wait.wait_key}:${raw.stepId || ""}`;
  const details = wait.details || {};
  if (
    details.mode === "approval_required" ||
    details.kind === "tool_approval"
  ) {
    const calls = Array.isArray(details.tool_calls) ? details.tool_calls : [];
    const target = { runId: raw.runId, waitKey: String(wait.wait_key || ""), stepId: raw.stepId };
    const names = [...new Set(calls.map((call: any) => call.name || "tool"))];
    return {
      id,
      kind: "tool-approval",
      title: `${calls.length || "Requested"} ${calls.length === 1 ? "action needs" : "actions need"} permission`,
      toolName: names.join(", ") || "the requested tools",
      description:
        "Review the targets below. Allow once approves this batch only.",
      detail: (
        <div className="code-approval-summary">
          <ToolActivityGroup
            tools={calls.map((call: any, index: number) => ({
              id: String(call.call_id || call.id || index),
              runId: raw.runId,
              name: call.name || "Tool",
              arguments: call.arguments,
              status: "waiting" as const,
            }))}
          />
          <details>
            <summary>Review full tool arguments (JSON)</summary>
            <JsonViewer value={calls.length ? calls : details} />
          </details>
        </div>
      ),
      approveLabel: "Allow once",
      denyLabel: "Deny",
      approveAllLabel: "Allow all enabled tools",
      approveAllDescription:
        "Sets permissions: all for this Gateway account, including future turns. Unchecked tools stay unavailable; explicit Ask overrides and Gateway restrictions remain enforced. Questions still need your answer.",
      onApproveAll: onPermissionsAll ? async () => {
        await controller.approveWithPolicyChange(target, () => { void onPermissionsAll(); });
      } : undefined,
      onApprove: async () => {
        await controller.approve(true, target);
      },
      onDeny: async () => {
        await controller.approve(false, target);
      },
    };
  }
  if (wait.reason === "user" || wait.reason === "ask_user")
    return {
      id,
      kind: "ask-user",
      title: "A question for you",
      prompt: String(
        wait.prompt || details.prompt || "How would you like to continue?",
      ),
      allowFreeText: wait.allow_free_text !== false,
      choices: (Array.isArray(wait.choices) ? wait.choices : []).map(
        (choice: any) =>
          typeof choice === "object" && choice !== null
            ? {
                id: String(choice.value ?? choice.id ?? choice.label),
                label: String(
                  choice.label ?? choice.text ?? choice.value ?? choice.id,
                ),
                description: choice.description,
              }
            : { id: String(choice), label: String(choice) },
      ),
      onSubmit: async (answer) => {
        await controller.resume(answer);
      },
    };
  const key = String(wait.wait_key || "");
  if (wait.reason === "event" || key.startsWith("evt:")) {
    const routing = resolveWorkflowEventTarget(raw, records, currentRun);
    const target = routing.target;
    const eventName = target?.name || "";
    return {
      id,
      kind: "event-wait",
      title: target ? "Waiting for an event" : "Event routing unavailable",
      eventName,
      prompt: String(
        wait.prompt ||
          `This workflow will continue when “${eventName || "its trigger"}” arrives.`,
      ),
      description: target
        ? "You can also send the expected event here."
        : `This event cannot be sent safely. ${routing.error}`,
      initialPayload: "{}",
      payloadLabel: "Event data (JSON)",
      sendLabel: "Send event",
      onSend: async (text) => {
        if (!target) throw new Error(routing.error);
        const payload = JSON.parse(text || "{}");
        const event: Record<string, unknown> = {
          name: target.name,
          scope: target.scope,
          payload,
        };
        if (target.scope === "session") event.session_id = target.scopeId;
        else if (target.scope === "workflow")
          event.workflow_id = target.scopeId;
        else if (target.scope === "run") event.run_id = target.scopeId;
        await controller.emitEvent(event);
      },
    };
  }
  return null;
}
