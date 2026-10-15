import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import {
  CodeVoiceSettings,
  HOLD_HINT,
  TAP_MS,
  TRANSCRIBE_TIMEOUT_MS,
  VoiceTools,
  fetchVoiceDefaults,
  releaseAction,
  spokenMessage,
  transcriptionRunId,
} from "./voice_tools";
import { MEDIA_NEEDS_HTTPS } from "../lib/secure-context";

describe("gateway voice controls", () => {
  const base = {
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
  it("disables the microphone while the kit cannot record, and says how to use it when it can", () => {
    setNavigator({ mediaDevices: { getUserMedia: async () => ({}) } });
    const off = renderToStaticMarkup(
      <VoiceTools {...base} capability={{ tts: { available: true }, stt: { available: true } }} />,
    );
    expect(off).toContain("Hold to talk");
    expect(off.match(/disabled=""/g)?.length).toBe(1);
    const on = renderToStaticMarkup(
      <VoiceTools
        {...base}
        voice={{ ...base.voice, voice_ptt_supported: true }}
        capability={{ stt: { available: true } }}
      />,
    );
    expect(on).toContain("Hold to talk, release to send");
    expect(on).not.toContain('disabled=""');
  });
  it("has no composer speaker: replies carry their own (round 6); the mic stays", () => {
    setNavigator({ mediaDevices: { getUserMedia: async () => ({}) } });
    const markup = renderToStaticMarkup(
      <VoiceTools {...base} capability={{ tts: { available: true }, stt: { available: true } }} />,
    );
    expect(markup).toContain("Hold to talk");
    expect(markup).not.toContain("Read latest reply aloud");
    expect(markup).not.toContain("spoken reply");
  });
  it("says why dictation is off when the browser withholds the microphone", () => {
    setNavigator({});
    const markup = renderToStaticMarkup(
      <VoiceTools {...base} capability={{ stt: { available: true } }} />,
    );
    // The sentence itself (http: the kit's; secure context: browser lacks it): lib/media_sentence.test.ts.
    expect(markup).toContain(MEDIA_NEEDS_HTTPS);
    expect(markup).not.toContain("Hold to talk");
    expect(markup).toContain('disabled=""');
  });
  it("shows elapsed seconds and the route while transcribing", () => {
    setNavigator({ mediaDevices: { getUserMedia: async () => ({}) } });
    const markup = renderToStaticMarkup(
      <VoiceTools
        {...base}
        voice={{ ...base.voice, voice_ptt_supported: true, voice_ptt_busy: true, voice_ptt_since: Date.now() - 12_000 }}
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

describe("dictation never spins forever", () => {
  it("bounds a transcription with a timeout that fails as a sentence", () => {
    const src = readFileSync(new URL("./voice_tools.tsx", import.meta.url), "utf8");
    expect(TRANSCRIBE_TIMEOUT_MS).toBeGreaterThan(60_000);
    expect(src).toMatch(/Promise\.race\(\[\s*gateway\.audio_transcribe/);
    expect(src).toContain("the gateway did not answer within");
  });
});

// Press to talk (operator, 2026-10-10: "the mic button should also be a press
// button, like the spacebar for the TUI"): press records, release sends.
describe("press to talk", () => {
  it("a hold sends; a tap is discarded", () => {
    expect(releaseAction(TAP_MS + 1)).toBe("send");
    expect(releaseAction(2_000)).toBe("send");
    expect(releaseAction(TAP_MS - 1)).toBe("discard");
    expect(releaseAction(0)).toBe("discard");
    expect(HOLD_HINT).toContain("release to send");
  });
  it("sends what was typed, then what was said", () => {
    expect(spokenMessage("", "bonjour")).toBe("bonjour");
    expect(spokenMessage("compare these:  \n", "the two GPUs")).toBe("compare these:\nthe two GPUs");
  });
  it("transcribes on the conversation's run, or on the session's media run before the first one", async () => {
    expect(await transcriptionRunId("run-1", "acode-abc")).toBe("run-1");
    // A new conversation has no run yet: its first message can be spoken too.
    expect(await transcriptionRunId("", "acode-abc")).toBe("session_memory_acode-abc");
  });
  it("the composer sends the transcript; a disabled button's press is refused in code", () => {
    const app = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
    expect(app).toMatch(/onTranscript: \(text\) => void sendSpoken\(text\)/);
    const tools = readFileSync(new URL("./voice_tools.tsx", import.meta.url), "utf8");
    expect(tools).toMatch(/if \(!voice\.voice_ptt_supported \|\| voice\.voice_ptt_busy \|\| held\.current\) return;/);
    // Dictation is offered before the conversation's first run (the media run carries it).
    expect(tools).toMatch(/capability\.stt\?\.available === true && \(runId \|\| sessionId\)/);
  });
});
