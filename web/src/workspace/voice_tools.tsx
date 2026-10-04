import { gatewayApiPath } from "@abstractframework/ui-kit";
import React, { useEffect, useRef } from "react";
import { Icon, useGatewayVoice, streamTtsJsonl, voiceSttRequest, voiceTtsRequest } from "@abstractframework/ui-kit";
import { gateway, gatewayRequest, newId, csrfHeaders } from "./transport";
import { MEDIA_NEEDS_HTTPS, mediaAvailable } from "../lib/secure-context";
import type { VoiceClientPreferences } from "@abstractframework/ui-kit";

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
            const response = await gateway.audio_transcribe(runId, {
              audio_artifact: attachment,
              request_id: newId(),
              ...voiceSttRequest(preferences),
            });
            assertCurrent();
            return String(response.text || "");
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

export function VoiceTools({
  voice,
  runId,
  capability,
  onSettings,
}: {
  voice: ReturnType<typeof useWorkspaceVoice>;
  runId: string;
  capability: Record<string, any>;
  onSettings?: () => void;
}) {
  const held = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      held.current = false;
    };
  }, []);
  const begin = () => {
    held.current = true;
    void voice.start_voice_ptt_recording().then(() => {
      // A permission prompt may outlive the press; never leave the mic open.
      if (!held.current || !mounted.current) voice.stop_voice_ptt_recording();
    });
  };
  const stop = () => {
    held.current = false;
    voice.stop_voice_ptt_recording();
  };
  useEffect(() => {
    window.addEventListener("pointerup", stop);
    window.addEventListener("pointercancel", stop);
    window.addEventListener("blur", stop);
    return () => {
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      window.removeEventListener("blur", stop);
    };
  }, [voice.stop_voice_ptt_recording]);
  if (!capability.tts?.available && !capability.stt?.available) return null;
  // Over plain http from another machine the browser withholds the microphone:
  // say why on the control instead of a silently disabled button.
  const micBlocked = !mediaAvailable();
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
              ? "Recording — release to transcribe"
              : "Hold to dictate"
          }
          title={
            runId
              ? "Hold to dictate (Space or Enter on keyboard)"
              : "Start a conversation to enable dictation"
          }
          disabled={!voice.voice_ptt_supported || voice.voice_ptt_busy}
          aria-pressed={voice.voice_ptt_recording}
          onPointerDown={(event) => {
            if (event.button === 0) begin();
          }}
          onPointerUp={stop}
          onPointerCancel={stop}
          onKeyDown={(event) => {
            if ([" ", "Enter"].includes(event.key) && !event.repeat) {
              event.preventDefault();
              begin();
            }
          }}
          onKeyUp={(event) => {
            if ([" ", "Enter"].includes(event.key)) {
              event.preventDefault();
              stop();
            }
          }}
          onBlur={stop}
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
      {voice.voice_ptt_recording || voice.voice_ptt_busy ? (
        <span role="status">
          {voice.voice_ptt_recording ? "Recording…" : "Transcribing…"}
        </span>
      ) : null}
    </>
  );
}
