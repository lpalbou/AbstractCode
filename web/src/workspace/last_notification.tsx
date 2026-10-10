// The automation's last "Email result" (round 16): the gateway serves `last_notification` on every
// automation summary — null | {channel: "email", status, at, sent_at, code, sentence, text}, where
// `text` is THE line ("Email result failed — <sentence>"). The app shows it only while the newest
// notice FAILED (a later successful send replaces it in the gateway, so the line clears by itself):
// the kit's error-toned pill carrying the served `text` verbatim, then the kit's compact time
// ("3 h ago"). Nothing about the outcome is computed here.
import React from "react";
import { AfChip, compactDuration, formatUtc, type AutomationSummary } from "@abstractframework/ui-kit";

export type FailedNotification = { text: string; at: string | null };

/** The served failed notice of this automation, or null (none, not failed, or not served). */
export function failedNotification(s: AutomationSummary): FailedNotification | null {
  const raw = (s as unknown as { last_notification?: unknown }).last_notification;
  if (!raw || typeof raw !== "object") return null;
  const n = raw as Record<string, unknown>;
  if (n.status !== "failed" || typeof n.text !== "string" || !n.text) return null;
  return { text: n.text, at: typeof n.at === "string" && n.at ? n.at : null };
}

/** "3 h ago" from the served `at` (kit wording); "" when the time is not served. */
export function notificationAgo(at: string | null, nowMs: number): string {
  const t = at ? Date.parse(at) : NaN;
  return Number.isNaN(t) ? "" : `${compactDuration(Math.max(0, nowMs - t))} ago`;
}

export function LastNotificationLine(props: { summary: AutomationSummary; nowMs: number; className?: string }): React.ReactElement | null {
  const n = failedNotification(props.summary);
  if (!n) return null;
  const ago = notificationAgo(n.at, props.nowMs);
  return (
    <small className={`code-notif-line ${props.className ?? ""}`.trim()} data-field="last-notification" data-status="failed">
      <AfChip tone="error" size="sm" className="code-notif-pill" title={n.at ? `${n.text}\n${formatUtc(n.at)}` : n.text}>
        {n.text}
      </AfChip>
      {ago ? <span className="code-notif-time">{ago}</span> : null}
    </small>
  );
}
