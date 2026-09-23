import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, it, expect } from "vitest";
import { VoiceTools } from "./voice_tools";

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
  it("keeps advertised speech disabled until a durable run exists", () => {
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
});
