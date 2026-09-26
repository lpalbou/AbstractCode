import type { StreamRepliesMode } from "@abstractframework/panel-chat";

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
 * The mode actually sent with the next run. A gateway that does not advertise
 * live replies gets nothing (`gateway_default` → `_runtime.stream` unset); the
 * settings panel shows why the choice is disabled.
 */
export function effectiveStreamReplies(
  mode: StreamRepliesMode,
  capability: StreamingCapability,
): StreamRepliesMode {
  return capability.status === "supported" ? mode : "gateway_default";
}

/** The label of the "Gateway default" option, with the gateway's value when known. */
export function gatewayDefaultStreamLabel(capability: StreamingCapability): string {
  if (capability.status === "supported" && capability.gatewayDefault !== null)
    return `Gateway default (${capability.gatewayDefault ? "on" : "off"})`;
  return "Gateway default";
}
