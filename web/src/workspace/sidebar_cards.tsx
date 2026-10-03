// The sidebar cards (round 4, DESIGN §3): full drawer width, no leading icon, an 8/12 px rhythm.
//
// - Conversation: the title on one line (ellipsis), then `Oct 2 · 2 turns · 7 tools`. The tool
//   figure is the gateway's (`tool_calls` per turn, totalled over its sub-runs; catalog.ts adds the
//   listed turns); without it the card shows no tool figure rather than a guess.
// - Automation: the name on one line with the Active switch beside it, the "waiting for you" badge
//   while an approval is pending, then ONE line `every 24 h · last 3 h ago · next in 14 h` (kit
//   `automationTiming`, deterministic). The card body selects; the switch is a sibling control.
import React from "react";
import {
  AfSwitch,
  automationControls,
  automationTiming,
  controlHint,
  type AutomationSummary,
} from "@abstractframework/ui-kit";

import type { SessionSummary } from "./catalog";

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** "Oct 2 · 2 turns · 7 tools" (the date in the viewer's locale, month + day, never the year). */
export function conversationMetaLine(
  item: Pick<SessionSummary, "updatedAt" | "turnCount" | "toolCalls">,
  options: { locale?: string; timeZone?: string } = {},
): string {
  const date = item.updatedAt ? new Date(item.updatedAt) : null;
  const day =
    date && Number.isFinite(date.getTime())
      ? date.toLocaleDateString(options.locale, { month: "short", day: "numeric", ...(options.timeZone ? { timeZone: options.timeZone } : {}) })
      : "Saved conversation";
  const parts = [day, plural(item.turnCount, "turn", "turns")];
  // Only a non-zero figure: "0 tools" on every chat card is noise.
  if (typeof item.toolCalls === "number" && item.toolCalls > 0) parts.push(plural(item.toolCalls, "tool", "tools"));
  return parts.join(" · ");
}

export function conversationTitle(item: Pick<SessionSummary, "prompt" | "sessionId">): string {
  return item.prompt || `Conversation ${item.sessionId.slice(0, 8)}`;
}

export function ConversationCard(props: { item: SessionSummary; selected: boolean; onClick(): void }): React.ReactElement {
  const { item } = props;
  const title = conversationTitle(item);
  return (
    <button
      type="button"
      className={`code-session code-card${props.selected ? " is-selected" : ""}`}
      aria-current={props.selected ? "page" : undefined}
      data-session-id={item.sessionId}
      onClick={props.onClick}
      title={title}
    >
      <span>
        <strong className="code-card-title">{title}</strong>
        <small className="code-card-meta" data-field="meta">{conversationMetaLine(item)}</small>
      </span>
      {item.state === "running" || item.state === "waiting" ? (
        <span className={`code-status-dot ${item.state === "running" ? "is-working" : "is-waiting"}`} title={item.state} />
      ) : null}
    </button>
  );
}

/** An approval (any wait on a person) is pending on this automation. */
export function automationIsWaiting(s: Pick<AutomationSummary, "attention">): boolean {
  return s.attention.pending_waits > 0;
}

export function WaitingBadge(): React.ReactElement {
  return (
    <span className="code-waiting-badge" data-field="waiting">
      waiting for you
    </span>
  );
}

/** The Active switch of a card or the header: kit rule for availability, kit hint as tooltip. */
export function AutomationActiveSwitch(props: {
  summary: AutomationSummary;
  busy: boolean;
  onToggle(): void;
  className?: string;
  variant?: "inline" | "sm";
  /** Track and thumb only (the card): the name "Active" is the accessible name and the tooltip. */
  iconOnly?: boolean;
}): React.ReactElement {
  const c = automationControls(props.summary, [], props.busy).active;
  return (
    <AfSwitch
      className={`${props.className ?? ""}${props.iconOnly ? " code-switch-icon-only" : ""}`.trim() || undefined}
      variant={props.variant ?? "sm"}
      action="active"
      label={props.iconOnly ? <span className="code-visually-hidden">Active</span> : "Active"}
      ariaLabel="Active"
      checked={props.summary.status === "active"}
      unavailableReason={c.enabled || props.busy ? null : c.reason ?? "Not available now."}
      reasonVisible={false}
      busy={props.busy}
      hint={props.iconOnly ? `Active\n${controlHint("active", props.summary)}` : controlHint("active", props.summary)}
      onChange={() => props.onToggle()}
    />
  );
}

export function AutomationCard(props: {
  summary: AutomationSummary;
  selected: boolean;
  busy: boolean;
  nowMs: number;
  onSelect(): void;
  onToggleActive(): void;
}): React.ReactElement {
  const s = props.summary;
  const timing = automationTiming(s, props.nowMs);
  const waiting = automationIsWaiting(s);
  return (
    <div
      className={`code-card code-auto-card${props.selected ? " is-selected" : ""}`}
      data-automation-id={s.automation_id}
      data-status={s.status}
    >
      <button
        type="button"
        className="code-card-main"
        aria-current={props.selected ? "page" : undefined}
        title={`${s.title}\n${timing.line}`}
        onClick={props.onSelect}
      >
        <strong className="code-card-title">{s.title}</strong>
        {waiting ? <WaitingBadge /> : null}
        <small className="code-card-meta" data-field="timing">
          {[timing.cadence, timing.last, timing.next].filter(Boolean).map((part, i) => (
            <React.Fragment key={i}>{i ? " · " : ""}<span className="code-card-part">{part}</span></React.Fragment>
          ))}
        </small>
      </button>
      <AutomationActiveSwitch className="code-card-switch" iconOnly summary={s} busy={props.busy} onToggle={props.onToggleActive} />
    </div>
  );
}
