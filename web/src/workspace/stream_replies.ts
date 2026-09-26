import type { ChatMessage, StreamRepliesMode } from "@abstractframework/panel-chat";

export type { StreamRepliesMode };

export const STREAM_REPLIES_MODES: readonly StreamRepliesMode[] = [
  "gateway_default",
  "on",
  "off",
];

/**
 * What the gateway says about live replies, from `GET /discovery/capabilities`
 * → `capabilities.streaming: { deltas: true, default: bool }` (contract S-2 §6).
 * Only a literal `deltas: true` advertises live replies; anything else is
 * "not supported" with the reason shown next to the setting.
 */
export type StreamingCapability =
  | { status: "loading" }
  | { status: "supported"; gatewayDefault: boolean | null }
  | { status: "unsupported"; reason: string };

export const STREAMING_LOADING: StreamingCapability = { status: "loading" };

/** Read the streaming capability from the discovery response. `loadError` is
 * the failed request's message when the capabilities could not be read. */
export function streamingCapability(
  response: unknown,
  loadError?: string,
): StreamingCapability {
  if (loadError)
    return {
      status: "unsupported",
      reason: `the gateway's capabilities could not be read (${loadError})`,
    };
  const capabilities = (response as any)?.capabilities;
  const streaming = capabilities?.streaming;
  if (!streaming || typeof streaming !== "object" || streaming.deltas !== true)
    return {
      status: "unsupported",
      reason: "not supported by this gateway (it does not advertise live replies)",
    };
  return {
    status: "supported",
    gatewayDefault:
      typeof streaming.default === "boolean" ? streaming.default : null,
  };
}

/** A saved preference value; anything else is the default ("gateway_default"). */
export function normalizeStreamReplies(value: unknown): StreamRepliesMode {
  return STREAM_REPLIES_MODES.includes(value as StreamRepliesMode)
    ? (value as StreamRepliesMode)
    : "gateway_default";
}

/**
 * The mode actually sent with the next run. **Off is always sent**
 * (`_runtime.stream: false` is harmless on a gateway without live replies,
 * and it keeps a gateway default of "on" from overriding the user while the
 * capabilities load or after they failed). **On** is sent only to a gateway
 * that advertises live replies; otherwise nothing is sent, the settings panel
 * says why, and the transcript carries one note (`streamingUnsupportedNote`).
 */
export function effectiveStreamReplies(
  mode: StreamRepliesMode,
  capability: StreamingCapability,
): StreamRepliesMode {
  if (mode === "on" && capability.status !== "supported") return "gateway_default";
  return mode;
}

/** A host-side transcript note, placed after the message whose id is `after`
 * (at the end when that message is not in the transcript). */
export type StreamNote = { message: ChatMessage; after: string | null };

/**
 * One note per conversation when the saved choice is On but the gateway does
 * not advertise live replies, placed after the conversation's first message.
 */
export function streamingUnsupportedNote(
  mode: StreamRepliesMode,
  capability: StreamingCapability,
  messages: readonly ChatMessage[],
): StreamNote | null {
  if (mode !== "on" || capability.status !== "unsupported" || !messages.length) return null;
  const firstUser = messages.find((message) => message.role === "user") || messages[0];
  return {
    after: firstUser.id ? String(firstUser.id) : null,
    message: {
      id: "stream-replies:unsupported",
      role: "system",
      level: "info",
      title: "Stream replies",
      content: `Streaming is on in your settings but this gateway does not support live replies: ${capability.reason}. Replies appear when they are complete.`,
    },
  };
}

/** The `call_id` of a malformed delta frame when it can be read, for de-duplication. */
function frameCallId(data: string): string {
  try {
    const parsed = JSON.parse(data);
    return typeof parsed?.call_id === "string" && parsed.call_id ? parsed.call_id : "";
  } catch {
    return "";
  }
}

/**
 * The note for a malformed live-reply frame (the frame itself is skipped and
 * the run keeps streaming). Its id is per run and model call (or per error
 * when the frame names no call), so a repeated bad frame is reported once.
 */
export function malformedDeltaNote(
  report: { runId: string; error: Error; frame: { event: string; data: string } },
  after: string | null,
): StreamNote {
  const callId = frameCallId(report.frame.data);
  const key = callId ? `call:${callId}` : `error:${report.error.message}`;
  return {
    after,
    message: {
      id: `stream-replies:malformed:${report.runId}:${key}`,
      role: "system",
      level: "warn",
      title: "Live reply",
      runId: report.runId,
      content: `The gateway sent a malformed live reply update (${report.frame.event}): ${report.error.message}. It was skipped; the complete reply still arrives.`,
    },
  };
}

/** Add a note unless one with the same id is already there. */
export function addStreamNote(notes: readonly StreamNote[], note: StreamNote): StreamNote[] {
  return notes.some((item) => item.message.id === note.message.id) ? [...notes] : [...notes, note];
}

/** The transcript with the host's notes inserted after their anchor messages. */
export function mergeStreamNotes(
  messages: readonly ChatMessage[],
  notes: readonly (StreamNote | null)[],
): ChatMessage[] {
  const present = notes.filter((note): note is StreamNote => Boolean(note));
  if (!present.length) return [...messages];
  const out: ChatMessage[] = [];
  const placed = new Set<StreamNote>();
  for (const message of messages) {
    out.push(message);
    for (const note of present)
      if (!placed.has(note) && note.after !== null && String(message.id) === note.after) {
        out.push(note.message);
        placed.add(note);
      }
  }
  const top = present.filter((note) => !placed.has(note) && note.after === null);
  const rest = present.filter((note) => !placed.has(note) && note.after !== null);
  return [...top.map((note) => note.message), ...out, ...rest.map((note) => note.message)];
}

/** The label of the "Gateway default" option, with the gateway's value when known. */
export function gatewayDefaultStreamLabel(capability: StreamingCapability): string {
  if (capability.status === "supported" && capability.gatewayDefault !== null)
    return `Gateway default (${capability.gatewayDefault ? "on" : "off"})`;
  return "Gateway default";
}
