import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, it, expect } from "vitest";
import { VoiceTools } from "./voice_tools";
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
    expect(markup).toContain("Read latest reply aloud");
    expect(markup.match(/disabled=""/g)?.length).toBe(2);
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
});
