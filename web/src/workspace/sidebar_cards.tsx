// The sidebar cards (round 4, DESIGN §3): full drawer width, no leading icon, an 8/12 px rhythm.
//
// - Conversation: the title on one line (ellipsis), then `Oct 2 · 2 turns · 7 tools`. The tool
//   figure is the gateway's (`tool_calls` per turn, totalled over its sub-runs; catalog.ts adds the
//   listed turns); without it the card shows no tool figure rather than a guess.
// - Automation (round 5): the name on its own row (+ the "waiting for you" badge while an approval
//   is pending), then two quiet lines from the kit's `automationTiming` (deterministic):
//   `↻ every 24 h · last 3 h ago`, then `next in 20 h` with the Active switch right-aligned on that
//   same line. The card body selects; the switch is a sibling control.
import React from "react";
import {
  AfMenu,
  AfSwitch,
  Icon,
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

/**
 * A conversation card (round 6): the card body opens the conversation; a sibling "⋯" (the kit's
 * AfMenu) offers Archive, which asks the host to show the inline confirm (`confirm`, rendered
 * under the card while it is this card's turn). Nothing is ever deleted: archive hides.
 */
export function ConversationCard(props: {
  item: SessionSummary;
  selected: boolean;
  onClick(): void;
  /** Archive was chosen in the card's "⋯": the host shows its inline confirm. */
  onAskArchive?(): void;
  /** The inline confirm, while this card's archive is being asked. */
  confirm?: React.ReactNode;
}): React.ReactElement {
  const { item } = props;
  const title = conversationTitle(item);
  return (
    <div className={`code-session-item${props.confirm ? " is-confirming" : ""}`} data-item-id={item.sessionId}>
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
      {props.onAskArchive ? (
        <AfMenu
          className="code-card-menu"
          label={`More actions for ${title}`}
          title="More actions"
          items={[{ id: "archive", label: "Archive", danger: true, onSelect: props.onAskArchive }]}
        />
      ) : null}
      {props.confirm}
    </div>
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

/** The card's two timing lines: `every 24 h · last 3 h ago` and `next in 20 h` ("" when nothing is scheduled). */
export function automationCardLines(s: AutomationSummary, nowMs: number): { first: string; second: string; scheduled: boolean } {
  const t = automationTiming(s, nowMs);
  return { first: [t.cadence, t.last].filter(Boolean).join(" · "), second: t.next || "", scheduled: s.trigger.source_id === "schedule" };
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
  const lines = automationCardLines(s, props.nowMs);
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
        title={`${s.title}\n${[lines.first, lines.second].filter(Boolean).join(" · ")}`}
        onClick={props.onSelect}
      >
        <strong className="code-card-title">{s.title}</strong>
        {waiting ? <WaitingBadge /> : null}
        <small className="code-card-meta" data-field="timing">
          {lines.scheduled ? <Icon name="refresh" size={12} className="code-card-cadence-icon" /> : null}
          <span className="code-card-part">{lines.first}</span>
        </small>
      </button>
      <div className="code-card-line2">
        <small className="code-card-meta" data-field="next">{lines.second}</small>
        <AutomationActiveSwitch className="code-card-switch" iconOnly summary={s} busy={props.busy} onToggle={props.onToggleActive} />
      </div>
    </div>
  );
}
