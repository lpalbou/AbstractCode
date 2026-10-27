import { gatewayApiPath } from "@abstractframework/ui-kit";
import React, { useEffect, useRef, useState } from "react";
import {
  AfVoiceSection,
  Icon,
  elapsedSeconds,
  streamTtsJsonl,
  transcribingLine,
  useGatewayVoice,
  voiceSttRequest,
  voiceTtsRequest,
} from "@abstractframework/ui-kit";
import { gateway, gatewayRequest, newId, csrfHeaders } from "./transport";
import { MEDIA_NEEDS_HTTPS, mediaAvailable } from "../lib/secure-context";
import { session_memory_owner_run_id } from "../lib/session_memory";
import type { VoiceClientPreferences, VoiceCreation, VoiceDefaults } from "@abstractframework/ui-kit";

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

type CreatedVoice = { voice_id: string; name?: string; engine?: string };

/** `POST voice/design`: a voice from a written description, stored in the gateway's voice library.
 * A refusal (403 shared store, 501 engine missing) throws the gateway's own sentence. */
export function designVoice(request: { description: string; name?: string; language?: string }): Promise<CreatedVoice> {
  const body: Record<string, string> = { description: request.description };
  if (request.name) body.name = request.name;
  if (request.language) body.language = request.language;
  return gatewayRequest<CreatedVoice>(gatewayApiPath("voice/design"), { method: "POST", body: JSON.stringify(body) });
}

/** `POST voice/clones` (multipart): a voice from a recording, stored in the gateway's voice library. */
export function cloneVoice(
  file: Blob,
  request: { name?: string; language?: string; reference_text?: string },
): Promise<CreatedVoice> {
  const form = new FormData();
  form.set("file", file, (file as File).name || "recording.wav");
  if (request.name) form.set("name", request.name);
  if (request.language) form.set("language", request.language);
  if (request.reference_text) form.set("reference_text", request.reference_text);
  return gatewayRequest<CreatedVoice>(gatewayApiPath("voice/clones"), { method: "POST", body: form });
}

/** "Create a voice" in the kit's voice section, through this app's gateway proxy. */
export const codeVoiceCreation: VoiceCreation = { design: designVoice, clone: cloneVoice };

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

/** Code's Voice panel: the kit's shared section, fed by this gateway. */
export function CodeVoiceSettings({
  value,
  onChange,
  defaults,
  connected,
}: {
  value: VoiceClientPreferences;
  onChange: (next: VoiceClientPreferences) => void;
  defaults: { value: VoiceDefaults | null; failed: boolean };
  connected: boolean;
}) {
  return (
    <AfVoiceSection
      value={value}
      onChange={onChange}
      fetchCatalog={fetchVoiceCatalog}
      fetchDefaults={fetchVoiceDefaults}
      defaults={defaults.value ?? undefined}
      overrideOwner="this app"
      nested
      createVoice={codeVoiceCreation}
      unavailableReason={connected ? null : "Connect to a gateway to configure voice."}
    />
  );
}

/** The run a transcription rides: the conversation's run when it has one,
 * otherwise the SESSION's media run (`session_memory_<session id>`), which the
 * gateway creates on first use — so the very first message of a conversation
 * can be spoken (the terminal client and Observer do the same). */
export async function transcriptionRunId(runId: string, sessionId: string): Promise<string> {
  if (runId) return runId;
  return session_memory_owner_run_id(sessionId);
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
      capability.stt?.available === true && (runId || sessionId)
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
            const transcriptionRun = await transcriptionRunId(runId, sessionId);
            assertCurrent();
            const response: any = await Promise.race([
              gateway.audio_transcribe(transcriptionRun, {
                audio_artifact: attachment,
                request_id: newId(),
                ...voiceSttRequest(preferences),
              }),
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

/** A press shorter than this is a tap: it is discarded, nothing is sent. */
export const TAP_MS = 350;

/** What releasing the microphone button does (press to talk, like the
 * terminal client's Space key): a hold sends what was said; a tap — too short
 * to be speech — is discarded. Pure, so it is tested without a browser. */
export function releaseAction(heldMs: number): "send" | "discard" {
  return heldMs < TAP_MS ? "discard" : "send";
}

/** The message a press-to-talk sends: what was already typed, then what was said. */
export function spokenMessage(draft: string, transcript: string): string {
  const typed = draft.trimEnd();
  return typed ? `${typed}\n${transcript}` : transcript;
}

/** What the button teaches after a tap. */
export const HOLD_HINT = "Hold the microphone while you speak — release to send.";

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
  capability,
  route = "",
  onSettings,
}: {
  voice: ReturnType<typeof useWorkspaceVoice>;
  capability: Record<string, any>;
  /** The transcription route ("faster-whisper / large-v3": the override, else the gateway default). */
  route?: string;
  onSettings?: () => void;
}) {
  const held = useRef(false);
  const pressedAt = useRef(0);
  const mounted = useRef(true);
  const [tapHint, setTapHint] = useState(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      held.current = false;
    };
  }, []);
  useEffect(() => {
    if (!tapHint) return;
    const timer = setTimeout(() => setTapHint(false), 4000);
    return () => clearTimeout(timer);
  }, [tapHint]);
  // Press: record at once. A disabled button still receives pointer events
  // (only clicks are suppressed), so the press itself checks.
  const begin = () => {
    if (!voice.voice_ptt_supported || voice.voice_ptt_busy || held.current) return;
    held.current = true;
    pressedAt.current = Date.now();
    setTapHint(false);
    void voice.start_voice_ptt_recording().then(() => {
      // A permission prompt may outlive the press; never leave the mic open.
      if (!held.current) voice.cancel_voice_ptt_recording?.();
      if (!mounted.current) voice.cancel_voice_ptt_recording?.();
    });
  };
  // Release: a hold sends what was said; a tap is discarded.
  const release = () => {
    if (!held.current) return;
    held.current = false;
    if (releaseAction(Date.now() - pressedAt.current) === "discard") {
      voice.cancel_voice_ptt_recording?.();
      setTapHint(true);
      return;
    }
    voice.stop_voice_ptt_recording();
  };
  // Focus left the page mid-hold: nothing is sent.
  const abandon = () => {
    if (!held.current) return;
    held.current = false;
    voice.cancel_voice_ptt_recording?.();
  };
  useEffect(() => {
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", release);
    window.addEventListener("blur", abandon);
    return () => {
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", release);
      window.removeEventListener("blur", abandon);
    };
  }, [voice.stop_voice_ptt_recording, voice.cancel_voice_ptt_recording]);
  const since = voice.voice_ptt_since || 0;
  const now = useNow(Boolean(since));
  if (!capability.tts?.available && !capability.stt?.available) return null;
  // Over plain http from another machine the browser withholds the microphone:
  // say why on the control instead of a silently disabled button.
  const micBlocked = !mediaAvailable();
  const status = voice.voice_ptt_recording
    ? `Recording… ${since ? elapsedSeconds(since, now) : ""}`.trim()
    : voice.voice_ptt_busy
      ? transcribingLine(since || now, now, route)
      : tapHint
        ? HOLD_HINT
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
            voice.voice_ptt_recording ? "Recording — release to send" : "Hold to talk"
          }
          title="Hold to talk, release to send (or hold Space or Enter on the focused button)"
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
          onBlur={release}
        >
          <Icon name={voice.voice_ptt_busy ? "loader" : "mic"} size={15} className={voice.voice_ptt_busy ? "code-loading-spinner" : undefined} />
        </button>
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
