import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import {
  CODE_CAPTURE_DEFAULTS,
  CodeVoiceSettings,
  TRANSCRIBE_TIMEOUT_MS,
  VoiceTools,
  cloneVoice,
  codeVoiceCreation,
  codeVoicePreview,
  codeVoiceLibrary,
  designVoice,
  fetchVoiceDefaults,
  spokenMessage,
  transcriptionRunId,
} from "./voice_tools";
import { MEDIA_NEEDS_HTTPS } from "../lib/secure-context";

describe("gateway voice controls", () => {
  const base = {
    preferences: {},
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
  const mic = (markup: string) => /<button[^>]*data-action="voice-mic"[^>]*>/.exec(markup)?.[0] || "";
  it("is the kit microphone (AfVoiceMicButton): disabled with the reason while it cannot record", () => {
    setNavigator({ mediaDevices: { getUserMedia: async () => ({}) } });
    const off = renderToStaticMarkup(<VoiceTools {...base} capability={{ tts: { available: true }, stt: { available: true } }} />);
    expect(mic(off)).toContain('aria-label="Hold to talk"');
    expect(mic(off)).toContain('disabled=""');
    const on = renderToStaticMarkup(<VoiceTools {...base} voice={{ ...base.voice, voice_ptt_supported: true }} capability={{ stt: { available: true } }} />);
    expect(mic(on)).toContain('aria-label="Hold to talk"');
    expect(mic(on)).not.toContain('disabled=""');
  });
  it("Tap to talk when the person chose it (capture_mode)", () => {
    setNavigator({ mediaDevices: { getUserMedia: async () => ({}) } });
    const tap = renderToStaticMarkup(
      <VoiceTools {...base} preferences={{ capture_mode: "toggle" }} voice={{ ...base.voice, voice_ptt_supported: true }} capability={{ stt: { available: true } }} />,
    );
    expect(mic(tap)).toContain('aria-label="Tap to talk"');
    expect(CODE_CAPTURE_DEFAULTS).toEqual({ capture_mode: "hold", auto_send: true });
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
      <VoiceTools {...base} voice={{ ...base.voice, voice_ptt_supported: true }} capability={{ stt: { available: true } }} />,
    );
    // The sentence itself (http: the kit's; secure context: browser lacks it): lib/media_sentence.test.ts.
    expect(markup).toContain(MEDIA_NEEDS_HTTPS);
    expect(mic(markup)).toContain('disabled=""');
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

// Create a voice (AbstractGateway 0.16.0 voice library, ui-kit 0.8.9 `createVoice`):
// the kit's panel calls these through this app's authenticated gateway proxy.
describe("Code's Voice panel creates voices in the gateway's voice library", () => {
  const VOICE = { voice_id: "v-1", name: "Narrator", engine: "qwen3-tts" };
  const withFetch = async (response: () => Response, run: (mock: ReturnType<typeof vi.fn>) => Promise<void>) => {
    const fetchMock = vi.fn(async (_url: unknown, _init?: RequestInit) => response());
    const realFetch = globalThis.fetch;
    globalThis.fetch = fetchMock as unknown as typeof fetch;
    vi.stubGlobal("document", { cookie: "abstractcode_gateway_csrf=tok" });
    try {
      await run(fetchMock);
    } finally {
      globalThis.fetch = realFetch;
      vi.unstubAllGlobals();
    }
  };
  const ok = () => new Response(JSON.stringify(VOICE), { status: 200, headers: { "content-type": "application/json" } });
  const refusal = (status: number, detail: string) => () =>
    new Response(JSON.stringify({ detail }), { status, headers: { "content-type": "application/json" } });

  it("hands the kit the voice library: creation, list / rename / remove, and Share only for an administrator", async () => {
    expect(codeVoiceCreation.design).toBe(designVoice);
    expect(codeVoiceCreation.clone).toBe(cloneVoice);
    const user = codeVoiceLibrary(false);
    expect(user.design).toBe(designVoice);
    expect(typeof user.list).toBe("function");
    expect(user.share).toBeUndefined();
    expect(typeof codeVoiceLibrary(true).share).toBe("function");
    const src = readFileSync(new URL("./voice_tools.tsx", import.meta.url), "utf8");
    expect(src).toContain("voiceLibrary={codeVoiceLibrary(isAdmin)}");
    await withFetch(ok, async (fetchMock) => {
      await codeVoiceLibrary(true).rename!("v 1", { name: "N" });
      await codeVoiceLibrary(true).share!("v 1");
      const calls = fetchMock.mock.calls.map(([url, init]) => `${(init as RequestInit).method} ${url}`);
      expect(calls).toEqual(["PATCH api/gateway/voice/clones/v%201", "POST api/gateway/voice/clones/v%201/deploy"]);
    });
  });
  it("designs a voice: POST voice/design with the description, name and language", async () => {
    await withFetch(ok, async (fetchMock) => {
      const out = await designVoice({ description: "a calm, warm low female voice", name: "Narrator", language: "fr" });
      expect(out).toEqual(VOICE);
      const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
      expect(url).toBe("api/gateway/voice/design");
      expect(init.method).toBe("POST");
      expect(JSON.parse(String(init.body))).toEqual({ description: "a calm, warm low female voice", name: "Narrator", language: "fr" });
      const headers = new Headers(init.headers);
      expect(headers.get("Content-Type")).toBe("application/json");
      expect(headers.get("X-AbstractCode-CSRF")).toBe("tok");
    });
  });
  it("clones a voice: POST voice/clones as multipart with the recording and its fields", async () => {
    await withFetch(ok, async (fetchMock) => {
      const recording = new File([new Uint8Array([1, 2, 3])], "me.wav", { type: "audio/wav" });
      const out = await cloneVoice(recording, { name: "Me", language: "en", reference_text: "Hello there." });
      expect(out.voice_id).toBe("v-1");
      const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
      expect(url).toBe("api/gateway/voice/clones");
      expect(init.method).toBe("POST");
      expect(init.body).toBeInstanceOf(FormData);
      const form = init.body as FormData;
      expect((form.get("file") as File).name).toBe("me.wav");
      expect(form.get("name")).toBe("Me");
      expect(form.get("language")).toBe("en");
      expect(form.get("reference_text")).toBe("Hello there.");
      // The browser sets the multipart boundary: no JSON content type.
      expect(new Headers(init.headers).get("Content-Type")).toBeNull();
    });
  });
  it("a gateway refusal (403, 501) surfaces as the gateway's own sentence", async () => {
    const shared = "On this gateway every account shares one voice store, so creating, changing or deleting a voice is an administrator's act.";
    await withFetch(refusal(403, shared), async () => {
      await expect(designVoice({ description: "x" })).rejects.toThrow(shared);
    });
    const missing = "Voice design needs abstractvoice[qwen3-tts] where the voice capability runs.";
    await withFetch(refusal(501, missing), async () => {
      const err = await cloneVoice(new Blob([new Uint8Array([1])]), {}).catch((e) => e);
      expect(err.message).toBe(missing);
      expect(err.status).toBe(501);
    });
  });
});

describe("choosing a voice plays it (previewVoice)", () => {
  it("synthesizes on the conversation's run (or the session's media run) and returns the audio bytes", async () => {
    const calls: string[] = [];
    const realFetch = globalThis.fetch;
    globalThis.fetch = (async (url: unknown, init?: RequestInit) => {
      calls.push(`${init?.method || "GET"} ${String(url)}`);
      if (String(url).endsWith("/voice/tts")) {
        const body = JSON.parse(String(init?.body));
        expect(body.text).toBeTruthy();
        expect(body.provider).toBe("qwen3-tts");
        expect(body.voice).toBe("Vivian");
        return new Response(JSON.stringify({ ok: true, run_id: "r", request_id: "q", audio_artifact: { $artifact: "a 1" } }), { status: 200, headers: { "content-type": "application/json" } });
      }
      return new Response(new Uint8Array([1, 2, 3]), { status: 200 });
    }) as typeof fetch;
    vi.stubGlobal("document", { cookie: "abstractcode_gateway_csrf=tok" });
    try {
      const bytes = await codeVoicePreview("", "acode-abc")({ provider: "qwen3-tts", voice: "Vivian" });
      expect(new Uint8Array(bytes)).toEqual(new Uint8Array([1, 2, 3]));
      expect(calls).toEqual([
        "POST api/gateway/runs/session_memory_acode-abc/voice/tts",
        "GET api/gateway/runs/session_memory_acode-abc/artifacts/a%201/content",
      ]);
      await expect(codeVoicePreview("", "")({})).rejects.toThrow(/Open a conversation/);
    } finally {
      globalThis.fetch = realFetch;
      vi.unstubAllGlobals();
    }
  });
  it("the Voice panel hands it to the kit while connected", () => {
    const src = readFileSync(new URL("./voice_tools.tsx", import.meta.url), "utf8");
    expect(src).toContain("previewVoice={connected && sampleRun ? codeVoicePreview(sampleRun.runId, sampleRun.sessionId) : undefined}");
    const app = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
    expect(app).toContain('sampleRun={{ runId: identity ? session.runId : "", sessionId: session.sessionId }}');
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
  it("sends what was typed, then what was said", () => {
    expect(spokenMessage("", "bonjour")).toBe("bonjour");
    expect(spokenMessage("compare these:  \n", "the two GPUs")).toBe("compare these:\nthe two GPUs");
  });
  it("transcribes on the conversation's run, or on the session's media run before the first one", async () => {
    expect(await transcriptionRunId("run-1", "acode-abc")).toBe("run-1");
    // A new conversation has no run yet: its first message can be spoken too.
    expect(await transcriptionRunId("", "acode-abc")).toBe("session_memory_acode-abc");
  });
  it("the composer sends the transcript, or puts it in the draft when auto-send is off; the automation view never records", () => {
    const app = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
    expect(app).toContain("setDraft((d) => spokenMessage(d, text))");
    expect(app).toContain("else void sendSpoken(text);");
    const tools = readFileSync(new URL("./voice_tools.tsx", import.meta.url), "utf8");
    // Dictation is offered before the conversation's first run (the media run carries it); no recording without a destination.
    expect(tools).toMatch(/capability\.stt\?\.available === true && \(runId \|\| sessionId\) && onTranscript/);
    const automations = readFileSync(new URL("./automations_view.tsx", import.meta.url), "utf8");
    expect(automations).not.toContain("onTranscript: () => {}");
  });
});
