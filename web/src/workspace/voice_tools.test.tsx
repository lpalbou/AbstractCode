import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, it, expect, vi } from "vitest";
import { CodeVoiceSettings, VoiceTools, fetchVoiceDefaults } from "./voice_tools";
import { MEDIA_NEEDS_HTTPS } from "../lib/secure-context";

describe("gateway voice controls", () => {
  const base = {
    runId: "",
    sessionId: "session",
    onTranscript: () => {},
    onError: () => {},
    voice: {
      tts_supported: false,
      tts_playback: { key: "", status: "idle" as const },
      toggle_tts: async () => {},
      stop_tts: () => {},
      voice_ptt_supported: false,
      voice_ptt_recording: false,
      voice_ptt_busy: false,
      start_voice_ptt_recording: async () => {},
      stop_voice_ptt_recording: () => {},
      cancel_voice_ptt_recording: () => {},
      voice_ptt_since: 0,
    },
  };
  it("does not advertise speech capabilities the gateway has not enabled", () => {
    expect(renderToStaticMarkup(<VoiceTools {...base} capability={{}} />)).toBe(
      "",
    );
    expect(
      renderToStaticMarkup(
        <VoiceTools
          {...base}
          capability={{ tts: { available: false }, stt: { available: false } }}
        />,
      ),
    ).toBe("");
  });
  const realNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const setNavigator = (value: unknown) =>
    Object.defineProperty(globalThis, "navigator", { value, configurable: true, writable: true });
  afterEach(() => {
    if (realNavigator) Object.defineProperty(globalThis, "navigator", realNavigator);
  });
  it("keeps advertised speech disabled until a durable run exists", () => {
    setNavigator({ mediaDevices: { getUserMedia: async () => ({}) } });
    const markup = renderToStaticMarkup(
      <VoiceTools
        {...base}
        capability={{ tts: { available: true }, stt: { available: true } }}
      />,
    );
    expect(markup).toContain("Hold to dictate");
    expect(markup.match(/disabled=""/g)?.length).toBe(1);
  });
  it("has no composer speaker: replies carry their own (round 6); the mic stays", () => {
    setNavigator({ mediaDevices: { getUserMedia: async () => ({}) } });
    const markup = renderToStaticMarkup(
      <VoiceTools {...base} runId="run" capability={{ tts: { available: true }, stt: { available: true } }} />,
    );
    expect(markup).toContain("Hold to dictate");
    expect(markup).not.toContain("Read latest reply aloud");
    expect(markup).not.toContain("spoken reply");
  });
  it("says why dictation is off when the browser withholds the microphone", () => {
    setNavigator({});
    const markup = renderToStaticMarkup(
      <VoiceTools {...base} runId="run" capability={{ stt: { available: true } }} />,
    );
    // The sentence itself (http: the kit's; secure context: browser lacks it): lib/media_sentence.test.ts.
    expect(markup).toContain(MEDIA_NEEDS_HTTPS);
    expect(markup).not.toContain("Hold to dictate");
    expect(markup).toContain('disabled=""');
  });
  it("shows elapsed seconds and the route while transcribing", () => {
    setNavigator({ mediaDevices: { getUserMedia: async () => ({}) } });
    const markup = renderToStaticMarkup(
      <VoiceTools
        {...base}
        voice={{ ...base.voice, voice_ptt_supported: true, voice_ptt_busy: true, voice_ptt_since: Date.now() - 12_000 }}
        runId="run"
        route="faster-whisper / large-v3"
        capability={{ stt: { available: true } }}
      />,
    );
    expect(markup).toMatch(/Transcribing… 1[1-3] s · faster-whisper \/ large-v3/);
  });
});

// Round 6 R6.1: Code showed "Gateway default · openai" for both engines on a
// gateway whose output.voice is supertonic and input.voice faster-whisper.
describe("Code's Voice panel names the gateway's routes", () => {
  const ROUTES = {
    tts: { route: "output.voice", configured: true, provider: "supertonic", model: "supertonic-3", voice: "M3" },
    stt: { route: "input.voice", configured: true, provider: "faster-whisper", model: "large-v3" },
  };
  it("renders the output.voice / input.voice routes and never openai", () => {
    const markup = renderToStaticMarkup(
      <CodeVoiceSettings value={{}} onChange={() => {}} defaults={{ value: ROUTES, failed: false }} connected />,
    );
    expect(markup).toContain("Gateway default · supertonic / supertonic-3");
    expect(markup).toContain("Gateway default · faster-whisper / large-v3");
    expect(markup).not.toMatch(/openai/i);
    // Devices + tests live in the shared section.
    expect(markup).toContain('aria-label="Output device"');
    expect(markup).toContain('aria-label="Input device"');
    expect(markup).toContain('data-action="test-speaker"');
    expect(markup).toContain('data-action="test-microphone"');
  });
  it("reads the defaults from the gateway's one voice-defaults API", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify(ROUTES), { status: 200, headers: { "content-type": "application/json" } }));
    const realFetch = globalThis.fetch;
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    vi.stubGlobal("document", { cookie: "" });
    try {
      const body = await fetchVoiceDefaults();
      expect(String((fetchMock.mock.calls[0] as unknown[])[0])).toMatch(/api\/gateway\/voice\/defaults$/);
      expect(body.tts?.provider).toBe("supertonic");
    } finally {
      globalThis.fetch = realFetch;
      vi.unstubAllGlobals();
    }
  });
});
