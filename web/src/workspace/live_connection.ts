// Why the composer is off while a live conversation has lost its streams (backlog 1163). The
// panel-chat controller keeps reconnecting while the run is live ("reconnecting", backing off to
// every 30 s); a message or a decision sent meanwhile could not reach the gateway, so the
// composer says why it waits instead of failing on Send.

const TERMINAL = new Set(["completed", "failed", "cancelled", "canceled"]);

export function liveConnectionNotice(input: { runId: string; status: string; connection: string | undefined }): string {
  if (!input.runId || TERMINAL.has(input.status)) return "";
  if (input.connection === "reconnecting") return "Reconnecting to this conversation. You can send again as soon as it is back.";
  if (input.connection === "disconnected") return "This conversation is not connected. Sign in again to continue.";
  return "";
}
