// This client's trouble reaching the gateway (operator 2026-11-08: "the one thing that is
// permitted on the client side is if it has issues connecting to the gateway; and then it
// should notify the user and propose retry or cancel. but that has nothing to do with if the
// gateway is connected"). The run's state and every wait come from the ledger view only; this
// is one page notice (the banner, with Retry now / Stop retrying) and the composer's note.

import type { WorkflowUnreachable } from "@abstractframework/panel-chat";

const TERMINAL = new Set(["completed", "failed", "cancelled", "canceled"]);

/** The banner's sentence while the page cannot reach the gateway. */
export function unreachableBannerText(unreachable: WorkflowUnreachable): string {
  const reason = unreachable.reason.trim().replace(/[.\s]+$/, "") || "no answer";
  return unreachable.retrying
    ? `Can't reach the gateway: ${reason}. Retrying…`
    : `Can't reach the gateway: ${reason}. Not retrying. Retry now to reconnect.`;
}

/** The composer's note while a live run's page cannot reach the gateway; "" otherwise. */
export function liveConnectionNotice(input: { runId: string; status: string; unreachable: WorkflowUnreachable | null | undefined }): string {
  if (!input.runId || TERMINAL.has(input.status) || !input.unreachable) return "";
  return "Can't reach the gateway; your message can be sent when it is back.";
}

/** "as of 12:21:34": when the page last had the gateway's word (local time). */
export function staleSince(unreachable: WorkflowUnreachable | null | undefined): string {
  const at = unreachable ? new Date(unreachable.since) : null;
  if (!at || Number.isNaN(at.getTime())) return "";
  return `as of ${at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false })}`;
}
