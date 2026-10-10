import { gatewayApiPath } from "@abstractframework/ui-kit";
import React, { useEffect, useRef, useState } from "react";
import {
  AfVoiceSection,
  Icon,
  elapsedSeconds,
  spokenLanguageLabel,
  streamTtsJsonl,
  transcribingLine,
  useGatewayVoice,
  voiceSttRequest,
  voiceTtsRequest,
} from "@abstractframework/ui-kit";
import { gateway, gatewayRequest, newId, csrfHeaders } from "./transport";
import { MEDIA_NEEDS_HTTPS, mediaAvailable } from "../lib/secure-context";
import type { SpokenLanguagePreference, VoiceClientPreferences, VoiceDefaults } from "@abstractframework/ui-kit";

/** The gateway's default voice routes (output.voice / input.voice): what "Gateway default" names. */
export function fetchVoiceDefaults(): Promise<VoiceDefaults> {
  return gatewayRequest<VoiceDefaults>(gatewayApiPath("voice/defaults"));
}

/** The voice catalog: engines, models and voices to pick an override from. */
export function fetchVoiceCatalog(provider?: string, model?: string) {
  const query = new URLSearchParams({ compact: "true" });
  if (provider) query.set("provider", provider);
  if (model) query.set("model", model);
  return gatewayRequest(gatewayApiPath(`voice/voices?${query.toString()}`));
}

/** Default voice routes, read once per connection (null until known; `failed` when the gateway could not answer). */
export function useVoiceDefaults(connected: boolean): { value: VoiceDefaults | null; failed: boolean } {
  const [state, setState] = useState<{ value: VoiceDefaults | null; failed: boolean }>({ value: null, failed: false });
  useEffect(() => {
    if (!connected) return;
    let alive = true;
    void fetchVoiceDefaults()
      .then((value) => alive && setState({ value: value || {}, failed: false }))
      .catch(() => alive && setState({ value: null, failed: true }));
    return () => {
      alive = false;
    };
  }, [connected]);
  return state;
}

/** The line next to the microphone (round 18): the ACCOUNT's spoken language, "" while unknown. */
export function spokenLanguageLine(block: SpokenLanguagePreference | null | undefined): string {
  const label = spokenLanguageLabel(block ?? null);
  return label ? `Spoken language: ${label}` : "";
}

/** What a transcription request carries: the uploaded audio, a request id and the STT route
 * override (`voiceSttRequest`: provider/model only). Never a language — the gateway applies the
 * account's spoken language (round 18). */
export function transcribeRequestBody<A>(audioArtifact: A, preferences: VoiceClientPreferences, requestId: string = newId()) {
  return { audio_artifact: audioArtifact, request_id: requestId, ...voiceSttRequest(preferences) };
}

/** Code's Voice panel: the kit's shared section, fed by this gateway. `spokenLanguage` = the
 * account's block (null when the gateway did not serve it) and its one-PUT save; absent = no
 * account (no row). The save note is kept here like AccountTimeZone's. */
export function CodeVoiceSettings({
  value,
  onChange,
  defaults,
  connected,
  spokenLanguage,
}: {
  value: VoiceClientPreferences;
  onChange: (next: VoiceClientPreferences) => void;
  defaults: { value: VoiceDefaults | null; failed: boolean };
  connected: boolean;
  spokenLanguage?: { block: SpokenLanguagePreference | null; save: (value: string) => Promise<unknown> };
}) {
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);
  const spoken = spokenLanguage
    ? {
        block: spokenLanguage.block,
        note,
        disabled: busy || !connected,
        onChange: async (next: string) => {
          setBusy(true);
          setNote(null);
          try {
            await spokenLanguage.save(next);
            setNote({ ok: true, text: "Saved." });
          } catch (reason) {
            setNote({ ok: false, text: `Not saved. ${reason instanceof Error ? reason.message : String(reason)}` });
          } finally {
            setBusy(false);
          }
        },
      }
    : undefined;
  return (
    <AfVoiceSection
      value={value}
      onChange={onChange}
      fetchCatalog={fetchVoiceCatalog}
      fetchDefaults={fetchVoiceDefaults}
      defaults={defaults.value ?? undefined}
      overrideOwner="this app"
      nested
      unavailableReason={connected ? null : "Connect to a gateway to configure voice."}
      spokenLanguage={spoken}
    />
  );
}

/** How long a transcription may take before it is reported as failed. */
export const TRANSCRIBE_TIMEOUT_MS = 180_000;

/** Optional media stays in the gateway; the browser only records and plays audio. */
export function useWorkspaceVoice({
  runId,
  sessionId,
  capability,
  scope,
  preferences,
  onTranscript,
  onError,
}: {
  runId: string;
  sessionId: string;
  capability: Record<string, any>;
  scope: string;
  preferences: VoiceClientPreferences;
  onTranscript: (text: string) => void;
  onError: (text: string) => void;
}) {
  const mounted = useRef(true);
  const activeScope = useRef(scope);
  activeScope.current = scope;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const assertCurrent = () => {
    if (!mounted.current || activeScope.current !== scope)
      throw new Error("Voice session changed.");
  };
  const voice = useGatewayVoice({
    output_device_id: preferences.output_device || "",
    input_device_id: preferences.input_device || "",
    input_gain: preferences.input_gain,
    volume: preferences.reply_volume,
    // Tap to start / tap to stop, or hold: VoiceTools stops the recording itself.
    stop_on_pointerup: false,
    tts_stream:
      capability.tts?.available === true && runId
        ? async function* (text, signal) {
            assertCurrent();
            yield* streamTtsJsonl({
              path: gatewayApiPath(`runs/${encodeURIComponent(runId)}/voice/tts/stream`),
              headers: csrfHeaders(),
              signal,
              body: {
                text,
                request_id: newId(),
                // Only the speech fields: read-aloud, speaker and the
                // transcription route are this app's, not the request's.
                ...voiceTtsRequest(preferences),
              },
            });
          }
        : undefined,
    transcribe:
      capability.stt?.available === true && runId
        ? async (blob, mime) => {
            assertCurrent();
            const limit = Number(capability.stt?.max_upload_bytes || 0);
            if (limit > 0 && blob.size > limit)
              throw new Error(
                "Recording exceeds the gateway's upload limit. Try a shorter recording.",
              );
            const extension = mime.includes("mp4")
              ? "m4a"
              : mime.includes("ogg")
                ? "ogg"
                : mime.includes("wav")
                  ? "wav"
                  : "webm";
            const file = new File([blob], `recording.${extension}`, {
              type: mime || "audio/webm",
            });
            const attachment = await gateway.attachments_upload(
              sessionId,
              file,
            );
            assertCurrent();
            // A transcription that never answers ends as a sentence, never a spinner forever.
            let timer: ReturnType<typeof setTimeout> | undefined;
            const response: any = await Promise.race([
              gateway.audio_transcribe(runId, transcribeRequestBody(attachment, preferences)),
              new Promise((_, reject) => {
                timer = setTimeout(
                  () => reject(new Error(`the gateway did not answer within ${TRANSCRIBE_TIMEOUT_MS / 1000} s`)),
                  TRANSCRIBE_TIMEOUT_MS,
                );
              }),
            ]).finally(() => clearTimeout(timer));
            assertCurrent();
            return {
              text: String(response.text || ""),
              provider: response.provider ?? null,
              model: response.model ?? null,
            };
          }
        : undefined,
    on_transcript: (text) => {
      if (mounted.current && activeScope.current === scope) onTranscript(text);
    },
    on_error: (text) => {
      if (mounted.current && activeScope.current === scope) onError(text);
    },
  });
  useEffect(() => {
    voice.stop_tts();
    voice.cancel_voice_ptt_recording?.();
    return () => {
      voice.stop_tts();
      voice.cancel_voice_ptt_recording?.();
    };
  }, [scope, voice.stop_tts, voice.cancel_voice_ptt_recording]);
  return voice;
}

/** A press shorter than this is a tap: recording keeps going until the next tap. */
export const TAP_MS = 350;

/** Ticks once a second while `active` (the elapsed seconds of "Recording…" / "Transcribing…"). */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [active]);
  return now;
}

export function VoiceTools({
  voice,
  runId,
  capability,
  route = "",
  spokenLanguage = null,
  onSettings,
}: {
  voice: ReturnType<typeof useWorkspaceVoice>;
  runId: string;
  capability: Record<string, any>;
  /** The transcription route ("faster-whisper / large-v3": the override, else the gateway default). */
  route?: string;
  /** The account's spoken-language block (round 18): "Spoken language: <label>" by the mic. */
  spokenLanguage?: SpokenLanguagePreference | null;
  onSettings?: () => void;
}) {
  const held = useRef(false);
  const latched = useRef(false);
  const pressedAt = useRef(0);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      held.current = false;
      latched.current = false;
    };
  }, []);
  // Press: start (or, while a tapped recording runs, stop it).
  const begin = () => {
    if (latched.current) {
      latched.current = false;
      voice.stop_voice_ptt_recording();
      return;
    }
    held.current = true;
    pressedAt.current = Date.now();
    void voice.start_voice_ptt_recording().then(() => {
      // A permission prompt may outlive the press; never leave the mic open.
      if (!held.current && !latched.current) voice.stop_voice_ptt_recording();
      if (!mounted.current) voice.cancel_voice_ptt_recording?.();
    });
  };
  // Release: a hold ends the recording; a tap keeps it going until the next tap.
  const release = () => {
    if (!held.current) return;
    held.current = false;
    if (Date.now() - pressedAt.current < TAP_MS) {
      latched.current = true;
      return;
    }
    voice.stop_voice_ptt_recording();
  };
  // Focus left the page: end any recording.
  const stopAll = () => {
    held.current = false;
    latched.current = false;
    voice.stop_voice_ptt_recording();
  };
  useEffect(() => {
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", release);
    window.addEventListener("blur", stopAll);
    return () => {
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", release);
      window.removeEventListener("blur", stopAll);
    };
  }, [voice.stop_voice_ptt_recording]);
  useEffect(() => {
    if (!voice.voice_ptt_recording) latched.current = false;
  }, [voice.voice_ptt_recording]);
  const since = voice.voice_ptt_since || 0;
  const now = useNow(Boolean(since));
  if (!capability.tts?.available && !capability.stt?.available) return null;
  // Over plain http from another machine the browser withholds the microphone:
  // say why on the control instead of a silently disabled button.
  const micBlocked = !mediaAvailable();
  const language = spokenLanguageLine(spokenLanguage);
  const status = voice.voice_ptt_recording
    ? `Recording… ${since ? elapsedSeconds(since, now) : ""}`.trim()
    : voice.voice_ptt_busy
      ? transcribingLine(since || now, now, route)
      : "";
  return (
    <>
      {capability.stt?.available && micBlocked ? (
        <>
          <button
            className="code-icon-button"
            aria-label="Dictation unavailable"
            aria-describedby="code-voice-https-note"
            title={MEDIA_NEEDS_HTTPS}
            disabled
          >
            <Icon name="mic" size={15} />
          </button>
          <span id="code-voice-https-note" className="code-voice-note" role="note">
            {MEDIA_NEEDS_HTTPS}
          </span>
        </>
      ) : null}
      {capability.stt?.available && !micBlocked ? (
        <button
          className="code-icon-button"
          aria-label={
            voice.voice_ptt_recording
              ? "Recording — tap or release to transcribe"
              : "Hold to dictate"
          }
          title={[
            runId
              ? "Hold to dictate, or tap to start and tap again to stop (Space or Enter on keyboard)"
              : "Start a conversation to enable dictation",
            language,
          ]
            .filter(Boolean)
            .join("\n")}
          aria-describedby={language ? "code-voice-language" : undefined}
          disabled={!voice.voice_ptt_supported || voice.voice_ptt_busy}
          aria-pressed={voice.voice_ptt_recording}
          onPointerDown={(event) => {
            if (event.button === 0) begin();
          }}
          onPointerUp={release}
          onPointerCancel={release}
          onKeyDown={(event) => {
            if ([" ", "Enter"].includes(event.key) && !event.repeat) {
              event.preventDefault();
              begin();
            }
          }}
          onKeyUp={(event) => {
            if ([" ", "Enter"].includes(event.key)) {
              event.preventDefault();
              release();
            }
          }}
          onBlur={() => {
            if (!latched.current) release();
          }}
        >
          <Icon name={voice.voice_ptt_busy ? "loader" : "mic"} size={15} className={voice.voice_ptt_busy ? "code-loading-spinner" : undefined} />
        </button>
      ) : null}
      {capability.stt?.available && !micBlocked && language ? (
        <span id="code-voice-language" className="code-voice-language" data-voice-language>
          {language}
        </span>
      ) : null}
      {/* No composer speaker (round 6): each reply has its own speaker button; Stop stays while one plays. */}
      {voice.tts_playback.status !== "idle" ? (
        <button
          className="code-icon-button"
          aria-label="Stop spoken reply"
          onClick={voice.stop_tts}
        >
          <Icon name="x" size={14} />
        </button>
      ) : null}
      {onSettings && capability.tts?.available ? (
        <button
          className="code-icon-button"
          aria-label="Voice settings"
          title="Voice settings"
          onClick={onSettings}
        >
          <Icon name="cog" size={14} />
        </button>
      ) : null}
      {status ? (
        <span role="status" className="code-voice-status" data-voice-status>
          {status}
        </span>
      ) : null}
    </>
  );
}
