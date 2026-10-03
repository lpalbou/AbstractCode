import { describe, expect, it, vi } from "vitest";
import { docsQuestionInput, makeDocsAsk, extractDocsAnswer } from "./app_assistant";
import type { gatewayRequest } from "./transport";

describe("Code app assistant", () => {
  it("grounds questions in app docs, disables tools, and delegates history to the gateway", () => {
    const input = docsQuestionInput("Where are my files?");
    expect(input.prompt).toBe("Where are my files?");
    expect(input.system).toContain("AbstractCode");
    expect(input.system).toContain("Troubleshooting");
    expect(input.tools).toEqual([]);
    expect(input.use_session_history).toBe(true);
  });
  it("keeps a separate session and reads the final answer beyond the first ledger page", async () => {
    const request = vi.fn().mockResolvedValueOnce({ run_id: "docs-run" })
      .mockResolvedValueOnce({ status: "completed" })
      .mockResolvedValueOnce({ items: [{ result: {} }], next_after: 1 })
      .mockResolvedValueOnce({ items: [{ result: { response: { content: "Open Files." } } }], next_after: 2 })
      .mockResolvedValueOnce({ items: [], next_after: 2 });
    const ask = makeDocsAsk(() => "docs-session", request as typeof gatewayRequest);
    expect(await ask("Where?", { signal: new AbortController().signal, history: [] })).toBe("Open Files.");
    const body = JSON.parse(request.mock.calls[0][1].body);
    expect(body.session_id).toBe("docs-session");
    expect(body.bundle_id).toBe("basic-agent");
    expect(body.input_data.tools).toEqual([]);
    expect(request.mock.calls[3][0]).toContain("after=1");
  });
  it("stops polling when its caller cancels and forwards the cancellation signal", async () => {
    const controller = new AbortController();
    const request = vi.fn().mockResolvedValueOnce({ run_id: "cancelled" })
      .mockImplementationOnce(async () => { controller.abort(); return { status: "running" }; });
    await expect(makeDocsAsk(() => "session", request as typeof gatewayRequest)("Hi", {
      signal: controller.signal, history: [],
    })).rejects.toMatchObject({ name: "AbortError" });
    expect(request).toHaveBeenCalledTimes(2);
    expect(request.mock.calls[0][1].signal).toBe(controller.signal);
  });
  it("reports a gateway run waiting instead of polling forever", async () => {
    const request = vi.fn().mockResolvedValueOnce({ run_id: "waiting" }).mockResolvedValueOnce({ status: "waiting" });
    await expect(makeDocsAsk(() => "session", request as typeof gatewayRequest)("Hi", {
      signal: new AbortController().signal, history: [],
    })).rejects.toThrow("is waiting");
  });
  it("reports failed runs and missing answers without manufacturing a reply", async () => {
    const request = vi.fn().mockResolvedValueOnce({ run_id: "bad" }).mockResolvedValueOnce({ status: "failed", error: { message: "Provider unavailable" } });
    await expect(makeDocsAsk(() => "session", request as typeof gatewayRequest)("Hi", { signal: new AbortController().signal, history: [] })).rejects.toThrow("Provider unavailable");
    expect(extractDocsAnswer({ output: { response: "Answer" } }, [])).toBe("Answer");
    expect(extractDocsAnswer({}, [])).toBe("");
  });
});
