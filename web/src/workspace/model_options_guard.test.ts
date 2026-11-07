import { describe, expect, it, vi } from "vitest";
import { CAPABILITY_WAIT_MS, guardedRunInput, sendableRunOptions } from "./model_options_guard";
import { buildWorkflowInput } from "./catalog";

// Operator rule 2026-11-07: a saved MTP or reasoning value the current model cannot use is never sent.
const DEPTH2 = { mode: "native_mtp" as const, num_draft_tokens: 2, require_acceleration: true as const };
const NO_MTP = { capabilities: { reasoning_levels: ["low", "high"] }, execution: { speculation: { supported: false, reason: "native_mtp_backend_unavailable", supported_depths: [] } } };
const MTP = { capabilities: {}, execution: { speculation: { supported: true, supported_depths: [2, 3] } } };

describe("send-time model options", () => {
  it("drops a saved MTP depth on a model whose card has no MTP (the gpt-5.6-terra incident)", async () => {
    const fetchCaps = vi.fn().mockResolvedValue(NO_MTP);
    const options = await sendableRunOptions({ provider: "airelays", model: "gpt-5.6-terra", reasoning: "high", speculation: DEPTH2 }, undefined, fetchCaps);
    expect(fetchCaps).toHaveBeenCalledWith("gpt-5.6-terra", "airelays");
    expect(options).toEqual({ reasoning: "high" });
    const runtime = buildWorkflowInput({ model: { provider: "airelays", model: "gpt-5.6-terra" }, ...options })._runtime as Record<string, unknown>;
    expect(runtime.speculation).toBeUndefined();
    expect(runtime.thinking).toBe("high");
  });
  it("drops a reasoning level the model does not advertise", async () => {
    expect(await sendableRunOptions({ provider: "p", model: "m", reasoning: "ultra" }, undefined, async () => NO_MTP)).toEqual({});
  });
  it("keeps what the model supports, read on the gateway default's model when none is chosen", async () => {
    const fetchCaps = vi.fn().mockResolvedValue(MTP);
    expect(await sendableRunOptions({ provider: "", model: "", speculation: DEPTH2 }, { provider: "mlx", model: "qwen" }, fetchCaps)).toEqual({ speculation: DEPTH2 });
    expect(fetchCaps).toHaveBeenCalledWith("qwen", "mlx");
  });
  it("costs no request when nothing is chosen", async () => {
    const fetchCaps = vi.fn();
    expect(await sendableRunOptions({ provider: "p", model: "m", reasoning: "" }, undefined, fetchCaps)).toEqual({});
    expect(fetchCaps).not.toHaveBeenCalled();
  });
  it("never sends MTP when the model's card cannot be read", async () => {
    expect(await sendableRunOptions({ provider: "p", model: "m", speculation: DEPTH2 }, undefined, async () => { throw new Error("down"); })).toEqual({});
    expect(await sendableRunOptions({ provider: "", model: "", speculation: DEPTH2 }, undefined, vi.fn())).toEqual({});
  });
});

describe("send-time model options: bounded wait", () => {
  it("does not stall a send on a capability probe that never answers: after the wait, no MTP depth, Off and reasoning kept", async () => {
    vi.useFakeTimers();
    try {
      const never = () => new Promise<unknown>(() => {});
      const depth = sendableRunOptions({ provider: "p", model: "m", reasoning: "high", speculation: DEPTH2 }, undefined, never);
      const off = sendableRunOptions({ provider: "p", model: "m", reasoning: "high", speculation: false }, undefined, never);
      await vi.advanceTimersByTimeAsync(CAPABILITY_WAIT_MS);
      expect(await depth).toEqual({ reasoning: "high" });
      expect(await off).toEqual({ reasoning: "high", speculation: false });
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("send-time model options: the model the run will use", () => {
  const MTP_MODEL = { provider: "mlx", model: "qwen-mtp" };
  const caps = vi.fn(async (model: string) => (model === "qwen-mtp" ? MTP : NO_MTP));
  // A workflow that pins its own model (public provider/model inputs).
  const pinnedWorkflow = { bundleId: "b", bundleVersion: "1", flowId: "pinned", name: "Pinned", interfaces: [] } as never;
  const pinnedSchema = { type: "object", properties: { prompt: { type: "string" }, provider: { type: "string" }, model: { type: "string" } } };

  it("an automation targeting a workflow pinned to a model without MTP never carries the conversation's depth", async () => {
    // The conversation runs on an MTP model with depth 2 saved; no model chosen in Settings.
    const prefs = { provider: "", model: "", speculation: DEPTH2 };
    const input = await guardedRunInput(
      (options) => buildWorkflowInput({
        workflow: pinnedWorkflow,
        inputSchema: pinnedSchema,
        schemaInputs: { provider: "airelays", model: "gpt-5.6-terra" },
        prompt: "nightly",
        model: { provider: prefs.provider, model: prefs.model },
        ...options,
      }),
      prefs,
      MTP_MODEL,
      caps,
    );
    expect(caps).toHaveBeenLastCalledWith("gpt-5.6-terra", "airelays");
    expect((input._runtime as Record<string, unknown> | undefined)?.speculation).toBeUndefined();
  });

  it("an unpinned workflow is checked on the gateway default's model and keeps a depth it supports", async () => {
    const prefs = { provider: "", model: "", speculation: DEPTH2 };
    const input = await guardedRunInput(
      (options) => buildWorkflowInput({ workflow: pinnedWorkflow, inputSchema: pinnedSchema, schemaInputs: {}, prompt: "x", model: prefs, ...options }),
      prefs,
      MTP_MODEL,
      caps,
    );
    expect(caps).toHaveBeenLastCalledWith("qwen-mtp", "mlx");
    expect((input._runtime as Record<string, unknown>).speculation).toEqual(DEPTH2);
  });
});
