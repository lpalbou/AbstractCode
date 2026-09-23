export type QueueIntent = {
  id: string;
  text: string;
  sessionId: string;
  sourceRunId: string;
  authEpoch: number;
};

type QueueContext = {
  sessionId: string;
  runId: string;
  snapshotRunId: string;
  snapshotSessionId: string;
  authEpoch: number;
};

function object(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

/** Stable non-secret identity for settings and async completion isolation. */
export function workspacePrincipalIdentity(status: unknown): string {
  const body = object(status);
  const gateway = object(body?.gateway);
  const principal = object(gateway?.principal);
  return JSON.stringify([
    text(body?.gateway_url ?? body?.gatewayUrl),
    text(principal?.tenant_id ?? principal?.tenantId),
    text(principal?.runtime_id ?? principal?.runtimeId),
    text(principal?.user_id ?? principal?.userId) || "session",
  ]);
}

export function queueIntentMatches(
  intent: QueueIntent,
  context: QueueContext,
): boolean {
  return (
    Boolean(intent.sourceRunId) &&
    intent.sessionId === context.sessionId &&
    intent.sessionId === context.snapshotSessionId &&
    intent.sourceRunId === context.runId &&
    intent.sourceRunId === context.snapshotRunId &&
    intent.authEpoch === context.authEpoch
  );
}

export type QueueTerminalDisposition = "advance" | "pause" | "wait";

/** Auto-advance only after success; failure/cancellation requires a person. */
export function queueTerminalDisposition(
  status: unknown,
): QueueTerminalDisposition {
  const value = typeof status === "string" ? status.trim().toLowerCase() : "";
  if (["completed", "complete", "done"].includes(value)) return "advance";
  if (["failed", "error", "cancelled", "canceled"].includes(value))
    return "pause";
  return "wait";
}

/** Remove the completed intent and bind the rest of its chain to the new run. */
export function advanceQueueIntents(
  items: QueueIntent[],
  completed: QueueIntent,
  nextRunId: string,
): QueueIntent[] {
  return items.flatMap((item) => {
    if (item.id === completed.id) return [];
    if (
      item.sessionId !== completed.sessionId ||
      item.authEpoch !== completed.authEpoch ||
      item.sourceRunId !== completed.sourceRunId
    )
      return [item];
    return [{ ...item, sourceRunId: nextRunId }];
  });
}

export function staleSendAbort(): DOMException {
  return new DOMException(
    "The conversation changed before the run was attached.",
    "AbortError",
  );
}
