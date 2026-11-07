import { describe, expect, it, vi } from "vitest";
import { sendableRunOptions } from "./model_options_guard";
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
