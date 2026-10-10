import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { CodeVoiceSettings, TRANSCRIBE_TIMEOUT_MS, VoiceTools, fetchVoiceDefaults, spokenLanguageLine, transcribeRequestBody } from "./voice_tools";
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

describe("dictation never spins forever", () => {
  it("bounds a transcription with a timeout that fails as a sentence", () => {
    const src = readFileSync(new URL("./voice_tools.tsx", import.meta.url), "utf8");
    expect(TRANSCRIBE_TIMEOUT_MS).toBeGreaterThan(60_000);
    expect(src).toMatch(/Promise\.race\(\[\s*gateway\.audio_transcribe/);
    expect(src).toContain("the gateway did not answer within");
  });
});

// Round 18: the spoken language is the ACCOUNT's (gateway-served block); Code keeps no copy.
describe("spoken language (round 18)", () => {
  const BLOCK = {
    value: "fr",
    label: "Spoken language",
    help: "The language spoken to the microphone. Auto lets the speech engine detect it; naming it skips detection, so short phrases and mixed-language speech transcribe reliably and a little faster.",
    choices: [
      { value: "auto", label: "Auto (detected)" },
      { value: "en", label: "English" },
      { value: "fr", label: "French" },
    ],
  };
  const voice = {
    tts_supported: false,
    tts_playback: { key: "", status: "idle" as const },
    toggle_tts: async () => {},
    stop_tts: () => {},
    voice_ptt_supported: true,
    voice_ptt_recording: false,
    voice_ptt_busy: false,
    start_voice_ptt_recording: async () => {},
    stop_voice_ptt_recording: () => {},
    cancel_voice_ptt_recording: () => {},
    voice_ptt_since: 0,
  };
  const realNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  afterEach(() => {
    if (realNavigator) Object.defineProperty(globalThis, "navigator", realNavigator);
  });
  it("the mic says the account's spoken language, by its served label", () => {
    Object.defineProperty(globalThis, "navigator", { value: { mediaDevices: { getUserMedia: async () => ({}) } }, configurable: true, writable: true });
    const markup = renderToStaticMarkup(<VoiceTools voice={voice} runId="run" capability={{ stt: { available: true } }} spokenLanguage={BLOCK} />);
    expect(markup).toContain('data-voice-language="true">Spoken language: French</span>');
    expect(markup).toContain('aria-describedby="code-voice-language"');
    expect(markup).toMatch(/title="Hold to dictate[^"]*\nSpoken language: French"/);
    const auto = renderToStaticMarkup(<VoiceTools voice={voice} runId="run" capability={{ stt: { available: true } }} spokenLanguage={{ ...BLOCK, value: "auto" }} />);
    expect(auto).toContain("Spoken language: Auto (detected)");
    const unknown = renderToStaticMarkup(<VoiceTools voice={voice} runId="run" capability={{ stt: { available: true } }} spokenLanguage={null} />);
    expect(unknown).not.toContain("Spoken language");
    expect(spokenLanguageLine(null)).toBe("");
  });
  it("a transcription request carries no language (the gateway applies the account's)", () => {
    const body = transcribeRequestBody({ artifact_id: "a" }, { stt_provider: "faster-whisper", stt_model: "large-v3", stt_language: "fr" } as never, "req-1");
    expect(body).toEqual({ audio_artifact: { artifact_id: "a" }, request_id: "req-1", provider: "faster-whisper", model: "large-v3" });
    expect("language" in body).toBe(false);
    const src = readFileSync(new URL("./voice_tools.tsx", import.meta.url), "utf8");
    expect(src).toContain("gateway.audio_transcribe(runId, transcribeRequestBody(attachment, preferences))");
    expect(src).not.toMatch(/stt_language|[{,]\s*language\s*:/);
  });
  it("Settings → Voice shows the served row; a pick reports the value; null says the seam sentence", () => {
    const markup = renderToStaticMarkup(
      <CodeVoiceSettings value={{}} onChange={() => {}} defaults={{ value: null, failed: false }} connected spokenLanguage={{ block: BLOCK, save: async () => undefined }} />,
    );
    expect(markup).toContain('data-setting="spoken-language"');
    expect(markup).toContain("French");
    expect(markup).toContain("naming it skips detection");
    const missing = renderToStaticMarkup(
      <CodeVoiceSettings value={{}} onChange={() => {}} defaults={{ value: null, failed: false }} connected spokenLanguage={{ block: null, save: async () => undefined }} />,
    );
    expect(missing).toContain("The gateway&#x27;s account preferences answer has no spoken_language block.");
    const none = renderToStaticMarkup(<CodeVoiceSettings value={{}} onChange={() => {}} defaults={{ value: null, failed: false }} connected />);
    expect(none).not.toContain("spoken-language");
  });
  it("app.tsx feeds the account block to the voice panel and the mic", () => {
    const app = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
    expect(app).toContain("save: accountWorkflow.saveSpokenLanguage,");
    expect(app).toContain('spokenLanguage={accountWorkflow.state.status === "ok" ? accountWorkflow.state.spokenLanguage : null}');
  });
});
