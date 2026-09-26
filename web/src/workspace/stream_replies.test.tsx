import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { buildWorkflowInput, type WorkflowDefinition } from "./catalog";
import { parsePreferences } from "./preferences";
import { DEFAULT_PREFERENCES, StreamRepliesField } from "./settings_panel";
import type { ChatMessage } from "@abstractframework/panel-chat";
import {
  addStreamNote,
  effectiveStreamReplies,
  malformedDeltaNote,
  mergeStreamNotes,
  normalizeStreamReplies,
  streamingUnsupportedNote,
  streamingCapability,
  type StreamingCapability,
} from "./stream_replies";

const agent: WorkflowDefinition = {
  id: "private:basic-agent@1:main",
  bundleId: "basic-agent",
  flowId: "main",
  name: "Basic agent",
  interfaces: ["abstractcode.agent.v1"],
} as unknown as WorkflowDefinition;

const baseInput = {
  workflow: agent,
  prompt: "hello",
  model: { provider: "lmstudio", model: "qwen" },
  speculation: undefined,
};

describe("Stream replies: preference", () => {
  it("defaults to the gateway default", () => {
    expect(DEFAULT_PREFERENCES.streamReplies).toBe("gateway_default");
    expect(parsePreferences(null).streamReplies).toBe("gateway_default");
    expect(parsePreferences("{}").streamReplies).toBe("gateway_default");
  });

  it("persists each choice through a save/restore round trip", () => {
    for (const mode of ["gateway_default", "on", "off"] as const) {
      const saved = JSON.stringify({ ...DEFAULT_PREFERENCES, streamReplies: mode });
      expect(parsePreferences(saved).streamReplies).toBe(mode);
    }
  });

  it("reads an unknown saved value as the default", () => {
    expect(parsePreferences(JSON.stringify({ streamReplies: true })).streamReplies).toBe("gateway_default");
    expect(parsePreferences(JSON.stringify({ streamReplies: "yes" })).streamReplies).toBe("gateway_default");
    expect(normalizeStreamReplies("on")).toBe("on");
  });
});

describe("Stream replies: run input", () => {
  const withoutStream = buildWorkflowInput(baseInput);

  it("on → _runtime.stream: true, and nothing else changes", () => {
    const input = buildWorkflowInput({ ...baseInput, streamReplies: "on" });
    expect((input._runtime as any).stream).toBe(true);
    const { stream, ...rest } = input._runtime as any;
    expect(stream).toBe(true);
    expect({ ...input, _runtime: rest }).toEqual(withoutStream);
  });

  it("off → _runtime.stream: false, and nothing else changes", () => {
    const input = buildWorkflowInput({ ...baseInput, streamReplies: "off" });
    const { stream, ...rest } = input._runtime as any;
    expect(stream).toBe(false);
    expect({ ...input, _runtime: rest }).toEqual(withoutStream);
  });

  it("gateway default → _runtime.stream unset (the gateway decides)", () => {
    const input = buildWorkflowInput({ ...baseInput, streamReplies: "gateway_default" });
    expect(input).toEqual(withoutStream);
    expect("stream" in (input._runtime as any)).toBe(false);
  });

  it("does not create a _runtime object on its own for the gateway default", () => {
    const bare = buildWorkflowInput({ workflow: agent, prompt: "x", streamReplies: "gateway_default" });
    expect(bare._runtime).toBeUndefined();
    const on = buildWorkflowInput({ workflow: agent, prompt: "x", streamReplies: "on" });
    expect(on._runtime).toEqual({ stream: true });
  });
});

describe("Stream replies: gateway capability", () => {
  it("is supported only for capabilities.streaming.deltas === true", () => {
    expect(streamingCapability({ capabilities: { streaming: { deltas: true, default: false } } })).toEqual({
      status: "supported",
      gatewayDefault: false,
    });
    expect(streamingCapability({ capabilities: { streaming: { deltas: true, default: true } } })).toEqual({
      status: "supported",
      gatewayDefault: true,
    });
    expect(streamingCapability({ capabilities: { streaming: { deltas: true } } })).toEqual({
      status: "supported",
      gatewayDefault: null,
    });
    for (const response of [
      { capabilities: {} },
      { capabilities: { streaming: true } },
      { capabilities: { streaming: { deltas: "yes" } } },
      { streaming: { deltas: true } },
      undefined,
    ])
      expect(streamingCapability(response).status).toBe("unsupported");
  });

  it("says why when the capabilities could not be read", () => {
    const cap = streamingCapability(undefined, "HTTP 502");
    expect(cap).toMatchObject({ status: "unsupported" });
    expect((cap as any).reason).toMatch(/HTTP 502/);
  });

  it("always sends Off; sends On only to a gateway that advertises live replies", () => {
    const unsupported = streamingCapability({ capabilities: {} });
    const failed = streamingCapability(undefined, "HTTP 502");
    const supported = streamingCapability({ capabilities: { streaming: { deltas: true, default: true } } });
    for (const cap of [unsupported, failed, { status: "loading" } as StreamingCapability, supported])
      expect(effectiveStreamReplies("off", cap)).toBe("off");
    expect(effectiveStreamReplies("on", unsupported)).toBe("gateway_default");
    expect(effectiveStreamReplies("on", failed)).toBe("gateway_default");
    expect(effectiveStreamReplies("on", { status: "loading" })).toBe("gateway_default");
    expect(effectiveStreamReplies("on", supported)).toBe("on");
    expect(effectiveStreamReplies("gateway_default", unsupported)).toBe("gateway_default");
    // Off reaches the run input even when the capabilities failed to load.
    const input = buildWorkflowInput({ ...baseInput, streamReplies: effectiveStreamReplies("off", failed) });
    expect((input._runtime as any).stream).toBe(false);
  });
});

describe("Stream replies: transcript notes", () => {
  const user = { id: "u1", role: "user", content: "hi" } as ChatMessage;
  const reply = { id: "a1", role: "assistant", content: "hello" } as ChatMessage;
  const unsupported = streamingCapability({ capabilities: {} });

  it("adds one note after the first message when On is saved but the gateway lacks live replies", () => {
    const note = streamingUnsupportedNote("on", unsupported, [user, reply]);
    expect(note).not.toBeNull();
    expect(note!.message.content).toContain("Streaming is on in your settings but this gateway does not support live replies");
    expect(note!.message.content).toContain("not supported by this gateway");
    const merged = mergeStreamNotes([user, reply], [note]);
    expect(merged.map((m) => m.id)).toEqual(["u1", "stream-replies:unsupported", "a1"]);
    expect(merged.filter((m) => m.id === "stream-replies:unsupported")).toHaveLength(1);
  });

  it("adds no note for other choices, a supported gateway, loading, or an empty transcript", () => {
    const supported = streamingCapability({ capabilities: { streaming: { deltas: true } } });
    expect(streamingUnsupportedNote("off", unsupported, [user])).toBeNull();
    expect(streamingUnsupportedNote("gateway_default", unsupported, [user])).toBeNull();
    expect(streamingUnsupportedNote("on", supported, [user])).toBeNull();
    expect(streamingUnsupportedNote("on", { status: "loading" }, [user])).toBeNull();
    expect(streamingUnsupportedNote("on", unsupported, [])).toBeNull();
  });

  it("reports a malformed frame once per model call", () => {
    const report = (data: string) => ({ runId: "r1", error: new Error("bad channel"), frame: { event: "llm.delta", data } });
    let notes = addStreamNote([], malformedDeltaNote(report('{"call_id":"c1","channel":"x"}'), "a1"));
    notes = addStreamNote(notes, malformedDeltaNote(report('{"call_id":"c1","channel":"y"}'), "a1"));
    expect(notes).toHaveLength(1);
    notes = addStreamNote(notes, malformedDeltaNote(report('{"call_id":"c2"}'), "a1"));
    expect(notes).toHaveLength(2);
    const merged = mergeStreamNotes([user, reply], notes);
    expect(merged.map((m) => m.id)).toEqual(["u1", "a1", notes[0].message.id, notes[1].message.id]);
    expect(merged[2]).toMatchObject({ role: "system", level: "warn" });
    expect(merged[2].content).toMatch(/malformed live reply update.*skipped/);
  });
});

describe("Stream replies: settings field", () => {
  const render = (streaming: StreamingCapability, disabled = false) =>
    renderToStaticMarkup(
      <StreamRepliesField value="on" onChange={() => {}} disabled={disabled} streaming={streaming} />,
    );

  it("is shown disabled with the reason when the gateway lacks streaming.deltas", () => {
    const html = render(streamingCapability({ capabilities: {} }));
    expect(html).toContain("Stream replies");
    expect(html).toMatch(/<select[^>]*disabled/);
    expect(html).toContain("not supported by this gateway");
  });

  it("is enabled and names the gateway default when supported", () => {
    const html = render(streamingCapability({ capabilities: { streaming: { deltas: true, default: true } } }));
    expect(html).not.toMatch(/<select[^>]*disabled/);
    expect(html).toContain("Gateway default (on)");
    expect(html).toContain(">On<");
    expect(html).toContain(">Off<");
  });

  it("follows the panel's disabled state even when supported", () => {
    const html = render(streamingCapability({ capabilities: { streaming: { deltas: true } } }), true);
    expect(html).toMatch(/<select[^>]*disabled/);
    expect(html).toContain("Gateway default<");
  });
});
