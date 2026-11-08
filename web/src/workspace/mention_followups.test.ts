// Mention follow-ups (gateway 0997ce4a / 06e9968c, contract "Additions"): `repeat_of` sent with each
// mention and the repeated prompt folded by it; 404 codes; 422 repeat_of_unknown; live reply frames
// that name their author, through the app's own stream reader and workflow transport.
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LlmDeltaEvent } from "@abstractframework/panel-chat";
import { GatewayClient } from "../lib/gateway_client";
import { GatewayHttpError } from "../lib/gateway_client";
import { createWorkflowTransport } from "./session_transport";
import { StreamGate } from "./stream_gate";
import {
  MENTIONS_UNAVAILABLE,
  foldRepeatedPrompts,
  foldRepeatedPromptsFrame,
  mentionRefusal,
  mentionRequestBody,
  withLiveAuthors,
} from "./mentions";

const httpError = (status: number, body: unknown) =>
  new GatewayHttpError(String((body as { detail?: unknown }).detail || status), { status, body_text: JSON.stringify(body) });
const castor = { kind: "entity" as const, name: "Castor", entity_id: "ent-castor", identity_ref: "entity:castor#3", avatar: { style: "watch-halo", color: "#39a5d0" } };
const agent = { kind: "agent" as const, name: "Basic agent", entity_id: null, identity_ref: null, avatar: null };

afterEach(() => vi.unstubAllGlobals());

describe("repeat_of: the person's words show once", () => {
  it("each mention names the person's own turn that carries the same words", () => {
    expect(mentionRequestBody("hi", { id: "castor", name: "Castor", kind: "entity", index: 0 }, "run-agent")).toEqual({ entity: "castor", text: "hi", mention_index: 0, repeat_of: "run-agent" });
    expect(mentionRequestBody("hi", { id: "castor", name: "Castor", kind: "entity", index: 0 }, "")).toEqual({ entity: "castor", text: "hi", mention_index: 0 });
  });
  const mentionView = {
    version: "replay_view_v1", root_run_id: "m1", session_id: "s1", status: "completed", cursor: { token: "m1:0~x", records: 0, runs: { m1: 0 } },
    messages: [
      { id: "m1:input", role: "user", kind: "prompt", text: "why does it fail?", run_id: "m1", repeat_of: "run-agent", author: { kind: "person", name: "Laurent", entity_id: null, identity_ref: null, avatar: null } },
      { id: "m1:final", role: "assistant", kind: "final", text: "Start with the test.", run_id: "m1", author: castor },
    ],
  };
  it("a mention turn's prompt that repeats a turn is folded; its answer stays; other views are untouched", () => {
    expect(foldRepeatedPrompts(mentionView).messages.map((m) => m.id)).toEqual(["m1:final"]);
    const plain = { ...mentionView, messages: [{ ...mentionView.messages[0], repeat_of: undefined }, mentionView.messages[1]] };
    expect(foldRepeatedPrompts(plain)).toBe(plain);
  });
  it("the same fold on the view stream's view and delta frames", () => {
    const view = foldRepeatedPromptsFrame({ event: "view", id: "m1:0~x", data: JSON.stringify(mentionView) });
    expect(JSON.parse(view.data).messages.map((m: { id: string }) => m.id)).toEqual(["m1:final"]);
    const delta = foldRepeatedPromptsFrame({ event: "delta", data: JSON.stringify({ messages: [mentionView.messages[0]] }) });
    expect(JSON.parse(delta.data).messages).toEqual([]);
    const done = { event: "done", data: "{}" };
    expect(foldRepeatedPromptsFrame(done)).toBe(done);
  });
  it("the app's workflow transport folds every view it reads (the loaded turn and the earlier ones)", async () => {
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify(mentionView), { status: 200, headers: { ETag: '"m1:0~x"' } })));
    const transport = createWorkflowTransport({ onDeltaError() {}, streams: new StreamGate() });
    const read = await transport.replayView!.getView("m1");
    expect(read.notModified).toBe(false);
    expect(read.notModified ? [] : read.view.messages.map((m) => m.id)).toEqual(["m1:final"]);
  });
  it("and every frame of the view stream", async () => {
    vi.stubGlobal("document", { cookie: "" });
    const wire = `id: m1:0~x\nevent: view\ndata: ${JSON.stringify(mentionView)}\n\nevent: done\ndata: {}\n\n`;
    vi.stubGlobal("fetch", vi.fn(async () => new Response(new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode(wire)); c.close(); } }), { status: 200, headers: { "Content-Type": "text/event-stream" } })));
    const transport = createWorkflowTransport({ onDeltaError() {}, streams: new StreamGate() });
    const frames: Array<{ event?: string; data: string }> = [];
    await transport.replayView!.streamView("m1", { signal: new AbortController().signal, onFrame: (f) => frames.push(f) });
    const view = frames.find((f) => f.event === "view");
    expect(JSON.parse(view!.data).messages.map((m: { id: string }) => m.id)).toEqual(["m1:final"]);
  });
});

describe("refusals with the gateway's codes", () => {
  it("404 entity_not_found is this entity; a plain 404 without a code is the route missing", () => {
    expect(mentionRefusal(httpError(404, { detail: "Entity 'castor' not found", code: "entity_not_found" }), "Castor").kind).toBe("not_offerable");
    expect(mentionRefusal(httpError(404, { detail: "Not Found" }), "Castor")).toEqual({ kind: "unavailable", sentence: MENTIONS_UNAVAILABLE });
    expect(mentionRefusal(httpError(404, { detail: "Entity 'castor' not found" }), "Castor").kind).toBe("unavailable");
  });
  it("422 repeat_of_unknown says the gateway's sentence and keeps the message", () => {
    expect(mentionRefusal(httpError(422, { detail: "repeat_of names no turn of this session", code: "repeat_of_unknown" }), "Castor")).toEqual({
      kind: "failed",
      sentence: "Castor could not be asked: repeat_of names no turn of this session.",
    });
  });
});

describe("live replies name their author", () => {
  const frame = (payload: Record<string, unknown>) => `event: llm.delta\ndata: ${JSON.stringify(payload)}\n\n`;
  const base = { kind: "llm.delta", run_id: "r1", root_run_id: "r1", parent_run_id: null, call_id: "c1", seq: 0, text: "Hel", channel: "content", snapshot: true };
  it("the stream reader hands the frame's author beside the delta (null when the frame has none)", async () => {
    const wire = frame({ ...base, author: castor }) + frame({ ...base, seq: 1, text: "lo", snapshot: false });
    vi.stubGlobal("fetch", vi.fn(async () => new Response(new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode(wire)); c.close(); } }), { status: 200 })));
    const seen: Array<[LlmDeltaEvent, unknown]> = [];
    await new GatewayClient({ base_url: "" }).stream_ledger("r1", { after: 0, on_step() {}, on_delta: (d, f) => seen.push([d, f?.author]), on_delta_error() {} });
    expect(seen.map(([d, a]) => [d.run_id, a])).toEqual([["r1", castor], ["r1", null]]);
  });
  it("the transport tells the app the author before the kit sees the frame", async () => {
    vi.stubGlobal("document", { cookie: "" });
    const wire = frame({ ...base, author: castor });
    vi.stubGlobal("fetch", vi.fn(async () => new Response(new ReadableStream({ start(c) { c.enqueue(new TextEncoder().encode(wire)); c.close(); } }), { status: 200 })));
    const order: string[] = [];
    const transport = createWorkflowTransport({ onDeltaError() {}, onDeltaAuthor: (runId, author) => order.push(`author ${runId} ${author.name}`), streams: new StreamGate() });
    await transport.streamLedger("r1", 0, () => {}, new AbortController().signal, undefined, (d) => order.push(`delta ${d.run_id}`));
    expect(order).toEqual(["author r1 Castor", "delta r1"]);
  });
  it("a live bubble without an author takes its run's; saved messages and other runs keep theirs", () => {
    const authors = new Map([["r1", castor]]);
    const messages = [
      { id: "live:r1:c1", role: "assistant" as const, content: "Hel", runId: "r1", live: { callId: "c1" } },
      { id: "live:r2:c1", role: "assistant" as const, content: "x", runId: "r2", live: { callId: "c1" } },
      { id: "r1:final", role: "assistant" as const, content: "Hello", runId: "r1", author: agent },
      { id: "r1:input", role: "user" as const, content: "hi", runId: "r1" },
      // A saved message without an author (an older gateway) stays today's card.
      { id: "r1:answer", role: "assistant" as const, content: "Hello", runId: "r1" },
    ];
    const out = withLiveAuthors(messages as never, authors);
    expect(out.map((m) => m.author?.name ?? null)).toEqual(["Castor", null, "Basic agent", null, null]);
    expect(withLiveAuthors(messages as never, new Map())).toBe(messages);
  });
});
