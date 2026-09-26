import { afterEach, describe, expect, it, vi } from "vitest";
import type { LlmDeltaEvent } from "@abstractframework/panel-chat";
import { GatewayClient } from "./gateway_client";
import type { LedgerStreamEvent } from "./types";

// Fake gateway SSE for `GET /runs/{id}/ledger/stream`: ledger `step` frames
// carry `id:` (the cursor); `llm.delta` / `llm.delta_end` frames carry none.
const RUN = "run-1";
const delta = (seq: number, text: string, extra: Record<string, unknown> = {}) => ({
  kind: "llm.delta",
  run_id: RUN,
  root_run_id: RUN,
  parent_run_id: null,
  node_id: "agent",
  call_id: "step-9",
  seq,
  text,
  channel: "content",
  snapshot: false,
  ...extra,
});
const deltaEnd = (seq: number) => ({
  kind: "llm.delta_end",
  run_id: RUN,
  root_run_id: RUN,
  parent_run_id: null,
  node_id: "agent",
  call_id: "step-9",
  seq,
  reason: "completed",
});
const stepFrame = (cursor: number) =>
  `id: ${cursor}\nevent: step\ndata: ${JSON.stringify({
    cursor,
    record: { run_id: RUN, step_id: `s${cursor}`, status: "completed", effect: { type: "llm_call" } },
  })}\n\n`;
const deltaFrame = (event: string, payload: unknown) =>
  `event: ${event}\ndata: ${typeof payload === "string" ? payload : JSON.stringify(payload)}\n\n`;

function sseResponse(chunks: string[]): Response {
  const encoder = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
      controller.close();
    },
  });
  return new Response(body, { status: 200, headers: { "Content-Type": "text/event-stream" } });
}

/** Split the wire into small chunks so frames straddle reads. */
function chop(wire: string, size = 17): string[] {
  const out: string[] = [];
  for (let i = 0; i < wire.length; i += size) out.push(wire.slice(i, i + size));
  return out;
}

async function run(wire: string, withDelta = true) {
  const fetchMock = vi.fn(async () => sseResponse(chop(wire)));
  vi.stubGlobal("fetch", fetchMock);
  const steps: LedgerStreamEvent[] = [];
  const deltas: LlmDeltaEvent[] = [];
  const errors: Array<{ message: string; event: string }> = [];
  const client = new GatewayClient({ base_url: "" });
  await client.stream_ledger(RUN, {
    after: 3,
    on_step: (ev) => steps.push(ev),
    ...(withDelta
      ? {
          on_delta: (ev: LlmDeltaEvent) => deltas.push(ev),
          on_delta_error: (error: Error, frame: { event: string }) =>
            errors.push({ message: error.message, event: frame.event }),
        }
      : {}),
  });
  return { steps, deltas, errors, fetchMock };
}

afterEach(() => vi.unstubAllGlobals());

describe("ledger stream: live reply frames", () => {
  const wire =
    stepFrame(4) +
    deltaFrame("llm.delta", delta(0, "Hel", { snapshot: true })) +
    deltaFrame("llm.delta", delta(1, "lo")) +
    stepFrame(5) +
    deltaFrame("llm.delta_end", deltaEnd(2));

  it("sends llm.delta / llm.delta_end to on_delta and ledger steps to on_step", async () => {
    const { steps, deltas } = await run(wire);
    expect(steps.map((s) => s.cursor)).toEqual([4, 5]);
    expect(deltas).toHaveLength(3);
    expect(deltas[0]).toMatchObject({ call_id: "step-9", seq: 0, text: "Hel", snapshot: true, channel: "content" });
    expect(deltas[1]).toMatchObject({ seq: 1, text: "lo", snapshot: false });
    expect(deltas[2]).toMatchObject({ kind: "llm.delta_end", reason: "completed" });
  });

  it("never hands a delta to on_step, so the resume cursor is the last step's", async () => {
    const { steps } = await run(wire);
    // The only cursors the caller can resume from are the ledger's own.
    expect(steps.every((s) => typeof s.record?.step_id === "string" && s.record.step_id.startsWith("s"))).toBe(true);
    expect(Math.max(...steps.map((s) => s.cursor))).toBe(5);
  });

  it("requests the stream from the caller's cursor", async () => {
    const { fetchMock } = await run(wire);
    expect(String((fetchMock.mock.calls[0] as unknown[])[0])).toBe(
      `/api/gateway/runs/${RUN}/ledger/stream?after=3`,
    );
  });

  it("reports a malformed delta frame, skips it, and keeps the ledger stream open", async () => {
    const { steps, deltas, errors } = await run(
      stepFrame(4) +
        deltaFrame("llm.delta", "{not json") +
        deltaFrame("llm.delta", { ...delta(1, "x"), channel: "tools" }) +
        deltaFrame("llm.delta_end", { ...deltaEnd(1), reason: "done" }) +
        deltaFrame("llm.delta", delta(2, "ok")) +
        stepFrame(5),
    );
    expect(errors.map((e) => e.event)).toEqual(["llm.delta", "llm.delta", "llm.delta_end"]);
    expect(errors[0].message).toMatch(/llm\.delta/);
    expect(errors[1].message).toMatch(/channel/);
    expect(errors[2].message).toMatch(/reason/);
    // Frames after the bad ones still arrive: the optional lane never ends the tail.
    expect(deltas.map((d) => (d as any).text)).toEqual(["ok"]);
    expect(steps.map((s) => s.cursor)).toEqual([4, 5]);
  });

  it("refuses on_delta without an error reporter", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => sseResponse([])));
    const client = new GatewayClient({ base_url: "" });
    await expect(
      client.stream_ledger(RUN, { after: 0, on_step: () => {}, on_delta: () => {} }),
    ).rejects.toThrow(/on_delta_error/);
  });

  it("a caller that does not ask for deltas keeps the old behaviour", async () => {
    const { steps, deltas } = await run(wire + deltaFrame("llm.delta", "{not json"), false);
    expect(steps.map((s) => s.cursor)).toEqual([4, 5]);
    expect(deltas).toEqual([]);
  });
});
