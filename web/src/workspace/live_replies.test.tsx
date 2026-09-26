import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  WorkflowChat,
  WorkflowSessionController,
  type ChatMessage,
} from "@abstractframework/panel-chat";
import { workflowTransport } from "./session_transport";

// End to end inside the page: the app's own workflow transport (fetch → SSE
// parser → onDelta) feeding panel-chat's controller, rendered by WorkflowChat.
// The gateway is faked at the fetch boundary; the stream stays open until the
// test pushes the next frames, like a real run.
const ROOT = "root-run";
const CALL = "step-llm-1";
const delta = (seq: number, text: string, extra: Record<string, unknown> = {}) => ({
  kind: "llm.delta",
  run_id: ROOT,
  root_run_id: ROOT,
  parent_run_id: null,
  node_id: "agent",
  call_id: CALL,
  seq,
  text,
  channel: "content",
  snapshot: false,
  ...extra,
});
const frame = (event: string, data: unknown, id?: number) =>
  `${id !== undefined ? `id: ${id}\n` : ""}event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
const step = (cursor: number, record: Record<string, unknown>) =>
  frame("step", { cursor, record: { run_id: ROOT, ...record } }, cursor);
const llmStarted = (cursor: number) =>
  step(cursor, { step_id: CALL, status: "started", effect: { type: "llm_call", payload: {} } });
const llmCompleted = (cursor: number, content: string) =>
  step(cursor, { step_id: CALL, status: "completed", effect: { type: "llm_call", payload: {} }, result: { content } });
const answer = (cursor: number, message: string) =>
  step(cursor, {
    step_id: `answer-${cursor}`,
    status: "completed",
    effect: { type: "answer_user", payload: { message } },
    result: { message },
  });

type Stream = { push: (text: string) => void; close: () => void; url: string };

function fakeGateway() {
  const streams: Stream[] = [];
  const ledgerAfters: number[] = [];
  const encoder = new TextEncoder();
  const json = (body: unknown) =>
    new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.includes("/ledger/stream")) {
      let controller!: ReadableStreamDefaultController<Uint8Array>;
      const body = new ReadableStream<Uint8Array>({ start: (c) => void (controller = c) });
      const stream: Stream = {
        url,
        push: (text) => controller.enqueue(encoder.encode(text)),
        close: () => controller.close(),
      };
      init?.signal?.addEventListener("abort", () => {
        try {
          controller.error(new DOMException("aborted", "AbortError"));
        } catch {
          /* already closed */
        }
      });
      streams.push(stream);
      return new Response(body, { status: 200, headers: { "Content-Type": "text/event-stream" } });
    }
    if (url.includes("/history_bundle"))
      return json({ run: { run_id: ROOT, status: "running" }, ledgers: { [ROOT]: { items: [] } } });
    if (url.includes("/ledger?")) {
      ledgerAfters.push(Number(new URL(url, "http://x").searchParams.get("after")));
      return json({ items: [], next_after: Number(new URL(url, "http://x").searchParams.get("after")) });
    }
    if (url.endsWith(`/runs/${ROOT}`)) return json({ run_id: ROOT, status: "running" });
    throw new Error(`unexpected request ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  return { streams, ledgerAfters };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 5));
async function settle() {
  for (let i = 0; i < 6; i += 1) await tick();
}
const live = (messages: ChatMessage[]) => messages.filter((m) => String(m.id).startsWith("live:"));
const render = (messages: ChatMessage[]) =>
  renderToStaticMarkup(
    <WorkflowChat
      messages={messages}
      draft=""
      onDraftChange={() => {}}
      onSend={async () => {}}
      streamReplies="on"
    />,
  );

beforeEach(() => {
  vi.stubGlobal("document", { cookie: "" });
});
afterEach(() => vi.unstubAllGlobals());

describe("live replies through the app's transport", () => {
  it("shows a live bubble from fake frames, then the final answer replaces it", async () => {
    const gw = fakeGateway();
    const controller = new WorkflowSessionController(workflowTransport, { clientId: "code-web-test" });
    await controller.load(ROOT);
    await settle();
    expect(gw.streams).toHaveLength(1);
    const stream = gw.streams[0];

    stream.push(llmStarted(1));
    stream.push(frame("llm.delta", delta(0, "Hello wor", { snapshot: true })));
    stream.push(frame("llm.delta", delta(1, "ld")));
    await settle();

    let messages = controller.getSnapshot().messages;
    expect(live(messages)).toHaveLength(1);
    expect(live(messages)[0]).toMatchObject({ id: `live:${ROOT}:${CALL}`, role: "assistant", content: "Hello world" });
    let html = render(messages);
    expect(html).toContain("Hello world");
    expect(html).toContain('data-stream-replies="on"');
    // Deltas are not ledger records.
    expect(controller.getSnapshot().records.map((r) => r.cursor)).toEqual([1]);

    stream.push(llmCompleted(2, "Hello world!"));
    stream.push(frame("llm.delta_end", { ...delta(2, ""), kind: "llm.delta_end", reason: "completed" }));
    stream.push(answer(3, "Hello world!"));
    await settle();

    messages = controller.getSnapshot().messages;
    expect(live(messages)).toHaveLength(0);
    const replies = messages.filter((m) => m.role === "assistant" && m.content === "Hello world!");
    expect(replies).toHaveLength(1);
    html = render(messages);
    expect(html).toContain("Hello world!");
    expect(controller.getSnapshot().records.map((r) => r.cursor)).toEqual([1, 2, 3]);
    controller.dispose();
  });

  it("deltas never move the cursor the controller resumes from", async () => {
    const gw = fakeGateway();
    const controller = new WorkflowSessionController(workflowTransport, { clientId: "code-web-test" });
    await controller.load(ROOT);
    await settle();
    const first = gw.streams[0];
    first.push(llmStarted(2));
    // Delta seq numbers far above the ledger cursor must not leak into it.
    first.push(frame("llm.delta", delta(40, "abc", { snapshot: true })));
    first.push(frame("llm.delta", delta(41, "def")));
    await settle();
    expect(live(controller.getSnapshot().messages)[0].content).toBe("abcdef");
    first.close(); // the stream drops: the controller catches up, then reconnects
    await settle();
    await new Promise((resolve) => setTimeout(resolve, 200));
    await settle();
    expect(gw.ledgerAfters.at(-1)).toBe(2);
    expect(gw.streams.length).toBeGreaterThanOrEqual(2);
    expect(gw.streams[1].url).toContain("after=2");
    // Reconnect drops the live bubble until the gateway's snapshot re-sends it.
    expect(live(controller.getSnapshot().messages)).toHaveLength(0);
    gw.streams[1].push(frame("llm.delta", delta(41, "abcdef", { snapshot: true })));
    await settle();
    expect(live(controller.getSnapshot().messages)[0].content).toBe("abcdef");
    controller.dispose();
  });

  it("a malformed delta frame surfaces an error instead of vanishing", async () => {
    const gw = fakeGateway();
    const controller = new WorkflowSessionController(workflowTransport, { clientId: "code-web-test" });
    await controller.load(ROOT);
    await settle();
    gw.streams[0].push("event: llm.delta\ndata: {not json\n\n");
    await settle();
    expect(controller.getSnapshot().error || "").toMatch(/llm\.delta/);
    controller.dispose();
  });
});
