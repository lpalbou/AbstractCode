import { gatewayApiPath } from "@abstractframework/ui-kit";
import React, { useEffect, useRef } from "react";
import {
  AfVoiceMicButton,
  AfVoiceSection,
  Icon,
  GATEWAY_VOICE_ROUTES,
  gatewayVoiceOptions,
  streamTtsJsonl,
  useGatewayVoice,
  voiceCaptureMode,
  voiceCloneDeployPath,
  voiceClonePath,
  voiceSttRequest,
  voiceTtsRequest,
  type VoiceLibrary,
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

/**
 * The gateway's voice library (backlog 0035; routes/voice_library.py, gateway 0.16.0): creation,
 * the person's voices (list / rename / remove) and, for an administrator only, Share with everyone.
 * A refusal is the gateway's own sentence.
 */
export function codeVoiceLibrary(isAdmin: boolean): VoiceLibrary {
  return {
    ...codeVoiceCreation,
    list: () => gatewayRequest(gatewayApiPath(GATEWAY_VOICE_ROUTES.clones)),
    rename: (voiceId, patch) => gatewayRequest(gatewayApiPath(voiceClonePath(voiceId)), { method: "PATCH", body: JSON.stringify(patch) }),
    remove: (voiceId) => gatewayRequest(gatewayApiPath(voiceClonePath(voiceId)), { method: "DELETE" }),
    ...(isAdmin ? { share: (voiceId: string) => gatewayRequest(gatewayApiPath(voiceCloneDeployPath(voiceId)), { method: "POST" }) } : {}),
  };
}

/** Code's microphone defaults (shown as the selected state while unset): hold to talk, send what was said. */
export const CODE_CAPTURE_DEFAULTS = { capture_mode: "hold" as const, auto_send: true };

/** Code's Voice panel: the kit's shared section, fed by this gateway. */
export function CodeVoiceSettings({
  value,
  onChange,
  defaults,
  connected,
  isAdmin = false,
  levelSource,
}: {
  value: VoiceClientPreferences;
  onChange: (next: VoiceClientPreferences) => void;
  defaults: { value: VoiceDefaults | null; failed: boolean };
  connected: boolean;
  /** An administrator may share a voice with everyone. */
  isAdmin?: boolean;
  /** The composer's voice (the live microphone level while recording). */
  levelSource?: React.ComponentProps<typeof AfVoiceSection>["levelSource"];
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
      voiceLibrary={codeVoiceLibrary(isAdmin)}
      // The composer microphone honours both: hold or tap, send at once or into the draft.
      rows={{ capture: true }}
      captureDefaults={CODE_CAPTURE_DEFAULTS}
      levelSource={levelSource ?? null}
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
  /** Where a transcript goes. Absent = no recording at all (a TTS-only surface never records). */
  onTranscript?: (text: string) => void;
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
    // Devices, volume, mute, input gain and the capture mode (hold / tap) the kit microphone uses.
    ...gatewayVoiceOptions({ ...CODE_CAPTURE_DEFAULTS, ...preferences }),
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
      capability.stt?.available === true && (runId || sessionId) && onTranscript
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
      if (mounted.current && activeScope.current === scope) onTranscript?.(text);
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

/** The message a spoken transcript makes: what was already typed, then what was said. */
export function spokenMessage(draft: string, transcript: string): string {
  const typed = draft.trimEnd();
  return typed ? `${typed}\n${transcript}` : transcript;
}

/**
 * The composer's voice controls: the kit microphone (AfVoiceMicButton: "Hold to talk" /
 * "Tap to talk", the live level, "Recording… 3 s" / "Transcribing… · route"), and Stop while a
 * reply is spoken. The transcript goes where the hook sends it (useWorkspaceVoice onTranscript).
 */
export function VoiceTools({
  voice,
  capability,
  preferences,
  route = "",
}: {
  voice: ReturnType<typeof useWorkspaceVoice>;
  capability: Record<string, any>;
  preferences: VoiceClientPreferences;
  /** The transcription route ("faster-whisper / large-v3": the override, else the gateway default). */
  route?: string;
}) {
  if (!capability.tts?.available && !capability.stt?.available) return null;
  return (
    <>
      {capability.stt?.available ? (
        <AfVoiceMicButton
          voice={voice}
          mode={voiceCaptureMode(preferences.capture_mode ?? CODE_CAPTURE_DEFAULTS.capture_mode)}
          routeText={route}
          // Over plain http from another machine the browser withholds the microphone: say why.
          unavailableReason={mediaAvailable() ? null : MEDIA_NEEDS_HTTPS}
          className="code-voice-mic"
        />
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
    </>
  );
}
