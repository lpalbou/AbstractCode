import { SessionCard, AutomationCard as SharedAutomationCard, type SessionActivity } from "../vendor/abstractsessions/session_cards.js";
import "../vendor/abstractsessions/session_cards.css";
// The sidebar cards (round 4, DESIGN §3): full drawer width, no leading icon, an 8/12 px rhythm.
//
// - Active conversations use AbstractUIC SessionCard. Archived restore rows keep a date/count summary. The tool
//   figure is the gateway's (`tool_calls` per turn, totalled over its sub-runs; catalog.ts adds the
//   listed turns); without it the card shows no tool figure rather than a guess.
// - Automations use the same shared card shell, with Watch run/schedule/next facts.
import React, { useLayoutEffect, useRef, useState } from "react";
import {
  AfSwitch,
  AfModal,
  automationControls,
  controlHint,
  type AutomationSummary,
} from "@abstractframework/ui-kit";

import type { SessionSummary } from "./catalog";
import { LastNotificationLine } from "./last_notification";
import { toApiError } from "./automations";

// Retained for the archived-conversation restore list.
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

/** Shown when a conversation has no title and its first message is unknown. */
export const UNTITLED_CONVERSATION = "Untitled conversation";

/**
 * A conversation's name (ADR 0043: never a computer id): its first message, else a plain
 * fallback. The gateway serves no session title, so the opening message is the name.
 */
export function conversationTitle(item: Pick<SessionSummary, "prompt">): string {
  return item.prompt?.trim() || UNTITLED_CONVERSATION;
}

/**
 * The search box over the listed conversations: the query (any case) in the shown name or the
 * session id (support pastes ids). It searches the list it is given, so whatever the list leaves
 * out (AbstractNote's background work) is never found either.
 */
export function searchConversations<T extends Pick<SessionSummary, "prompt" | "sessionId">>(items: readonly T[], query: string): T[] {
  const q = query.toLowerCase();
  return items.filter((item) => `${item.prompt || ""} ${item.sessionId}`.toLowerCase().includes(q));
}

const WEEKDAYS: Record<string, string> = { mon: "Mon", tue: "Tue", wed: "Wed", thu: "Thu", fri: "Fri", sat: "Sat", sun: "Sun" };
const ordinal = (n: number) => `${n}${n % 100 >= 11 && n % 100 <= 13 ? "th" : ["th", "st", "nd", "rd"][n % 10] || "th"}`;

/** "30m" (a stored interval) → "Every 30 min"; "1h" → "Every hour"; "1d" → "Every day". */
function intervalWords(every: string): string {
  const match = /^([1-9][0-9]*)([smhd])$/.exec(every.trim());
  if (!match) return "";
  const n = Number(match[1]);
  const unit = match[2];
  if (n === 1) return { s: "Every second", m: "Every minute", h: "Every hour", d: "Every day" }[unit] || "";
  return `Every ${n} ${{ s: "s", m: "min", h: "h", d: "d" }[unit]}`;
}

/**
 * The automation's schedule in a few words for a sidebar row ("Daily 08:00", "Every 8 h",
 * "Weekly Mon, Thu 09:00", "Monthly 1st 09:00", "Every 2 min"), from the recorded rule's
 * STRUCTURE (its interval or calendar fields), never by reading the gateway's sentence. The time
 * is the rule's own wall time; its zone and bounds stay in the whole rule (the row's tooltip and
 * the automation's details). A state (Paused, …) or a trigger the row cannot word briefly gives
 * the kit's own word, or "" (the row then shows no schedule).
 */
export function compactSchedule(s: Pick<AutomationSummary, "status" | "trigger">): string {
  const state = { paused: "Paused", archived: "Archived", failed: "Failed", completed: "Ended" }[String(s.status)];
  if (state) return state;
  const trigger = s.trigger;
  if (trigger.source_id === "manual") return "Manual";
  if (trigger.source_id.startsWith("email")) return "On new email";
  if (trigger.source_id !== "schedule") return "";
  const c = (trigger.config || {}) as Record<string, unknown>;
  const at = typeof c.at === "string" ? c.at : "";
  const time = /^\d{2}:\d{2}/.test(at) ? at.slice(0, 5) : "";
  if (typeof c.every === "string") return intervalWords(c.every);
  if (c.kind === "daily") return time ? `Daily ${time}` : "Daily";
  if (c.kind === "weekly") {
    const days = (Array.isArray(c.days) ? c.days : []).map((d) => WEEKDAYS[String(d)] || "").filter(Boolean).join(", ");
    return ["Weekly", days, time].filter(Boolean).join(" ");
  }
  if (c.kind === "monthly") {
    const day = c.day === "last" ? "last day" : Number.isSafeInteger(Number(c.day)) ? ordinal(Number(c.day)) : "";
    return ["Monthly", day, time].filter(Boolean).join(" ");
  }
  if (c.kind === "once") return at ? `Once ${at.replace("T", " ").slice(0, 16)}` : "Once";
  if (c.start_at && !c.every) return "Once";
  return "";
}

/**
 * A sidebar row shows nothing cut (ADR 0053 §2). Its name keeps two lines; a longer one ends in
 * "…" and the whole name is the kit tooltip of the row's button (hover and keyboard focus). Its
 * details are one line: when they do not fit, whole parts leave the line, lowest value first
 * (`drop`: the schedule, then the tool count, then the next run), never an ellipsis inside a part;
 * what left is said in the same tooltip, and stays in the conversation or automation details.
 * `schedule` puts the compact schedule words on the automation's schedule part.
 */
export function useRevealClampedTitle(
  title: string,
  schedule = "",
  /** The gateway's counts when its list was cut (`has_more`): the line reads "4+ turns" (the kit's
   * accessible label keeps "At least 4 turns"). */
  counts?: { partial?: boolean; turns?: number; tools?: number },
): React.RefObject<HTMLDivElement> {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const root = ref.current;
    const name = root?.querySelector<HTMLElement>(".af-session-card__title");
    const button = root?.querySelector<HTMLElement>(".af-session-card__open");
    const meta = root?.querySelector<HTMLElement>(".af-session-card__meta");
    if (!root || !name || !button) return;
    name.removeAttribute("title");
    // Recomputed after every render (the kit re-renders the figures: a next run time changes).
    for (const el of Array.from(meta?.children || [])) if (el.hasAttribute("data-drop")) el.removeAttribute("data-drop");
    const cadence = meta?.querySelector<HTMLElement>('[data-field="cadence"]') || null;
    if (cadence) {
      if (schedule) cadence.setAttribute("data-short", schedule);
      else cadence.setAttribute("data-drop", "empty");
    }
    // Counts the gateway gave as a lower bound: "4+ turns", "2+ tool calls" on the line.
    for (const el of Array.from(meta?.children || []) as HTMLElement[]) {
      const label = el.getAttribute("aria-label") || "";
      if (!label || el.getAttribute("data-field") === "cadence") continue;
      const isTurns = label.endsWith(" turn") || label.endsWith(" turns");
      const isTools = label.endsWith(" tool call") || label.endsWith(" tool calls");
      const n = isTurns ? counts?.turns : isTools ? counts?.tools : undefined;
      const short = counts?.partial && typeof n === "number" ? `${n}+ ${isTurns ? (n === 1 ? "turn" : "turns") : n === 1 ? "tool call" : "tool calls"}` : "";
      if (short) { if (el.getAttribute("data-short") !== short) el.setAttribute("data-short", short); }
      else if (el.hasAttribute("data-short")) el.removeAttribute("data-short");
    }
    // While an automation needs you, "Needs you" says more than the next run time.
    const next = meta?.querySelector<HTMLElement>('[data-field="next"]') || null;
    if (next && meta?.querySelector(".af-session-card__attention")) next.setAttribute("data-drop", "attention");
    const tools = meta ? Array.from(meta.children).find((el) => /tool call/.test(el.getAttribute("aria-label") || "")) as HTMLElement | undefined : undefined;
    const droppable = [cadence, tools || null, next].filter((el): el is HTMLElement => Boolean(el) && el!.getAttribute("data-drop") !== "empty" && el!.getAttribute("data-drop") !== "attention");
    const update = () => {
      for (const el of droppable) el.removeAttribute("data-drop");
      const dropped: string[] = [];
      for (const el of droppable) {
        if (!meta || meta.scrollWidth <= meta.clientWidth + 1) break;
        el.setAttribute("data-drop", "fit");
        dropped.push(el.getAttribute("title") || el.getAttribute("aria-label") || el.textContent || "");
      }
      const tip = [name.scrollHeight > name.clientHeight + 1 ? title : "", ...dropped.filter(Boolean)].filter(Boolean).join(" · ");
      // Written only when it changes: the kit tooltip hides when its sentence is rewritten.
      if (tip && button.getAttribute("data-af-tip") !== tip) button.setAttribute("data-af-tip", tip);
      else if (!tip && button.hasAttribute("data-af-tip")) button.removeAttribute("data-af-tip");
    };
    update();
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(update) : null;
    observer?.observe(root);
    return () => observer?.disconnect();
  });
  return ref;
}

/** Shared compact session presentation; the host owns archive confirmation. */
export function ConversationCard(props: {
  item: SessionSummary;
  activity?: SessionActivity;
  unread?: boolean;
  needsAttention?: boolean;
  selected: boolean;
  onClick(): void;
  /** Archive icon activation asks the host to show its inline confirm. */
  onAskArchive?(): void;
  /** The inline confirm, while this card's archive is being asked. */
  confirm?: React.ReactNode;
}): React.ReactElement {
  const { item } = props;
  const title = conversationTitle(item);
  const reveal = useRevealClampedTitle(title, "", { partial: item.truncated, turns: item.turnCount, tools: item.toolCalls });
  return (
    <div ref={reveal} className={`code-session-item${props.confirm ? " is-confirming" : ""}`} data-item-id={item.sessionId}>
      <SessionCard sessionId={item.sessionId} title={title} selected={props.selected} unread={props.unread}
        needsAttention={props.needsAttention ?? props.activity?.needsAttention} working={props.activity?.working ?? item.state === "running"}
        turnCount={item.turnCount} toolCount={item.toolCalls} countsPartial={item.truncated} updatedAt={item.updatedAt} nowMs={Date.now()}
        onOpen={props.onClick} onArchive={props.onAskArchive} archiveDisabled={item.state === "running" || item.state === "waiting"} />
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

/** Automation transport and confirmation stay in the host; presentation is shared. */
export function AutomationCard(props: {
  summary: AutomationSummary;
  selected: boolean;
  busy: boolean;
  nowMs: number;
  onSelect(): void;
  onArchive?(): Promise<unknown>;
}): React.ReactElement {
  const s = props.summary;
  const [confirm, setConfirm] = useState(false);
  const [archiving, setArchiving] = useState(false);
  const [error, setError] = useState("");
  const archive = automationControls(s, [], props.busy || archiving).archive;
  const reveal = useRevealClampedTitle(s.title || "Automation", compactSchedule(s));
  return <div ref={reveal} className="code-automation-item" data-automation-id={s.automation_id} data-status={s.status}>
    <SharedAutomationCard summary={s} selected={props.selected} nowMs={props.nowMs} onOpen={props.onSelect}
      onArchive={props.onArchive ? () => { setError(""); setConfirm(true); } : undefined}
      archiveDisabled={!archive.enabled} archiveTitle={archive.reason || "Archive automation"} />
    <LastNotificationLine summary={s} nowMs={props.nowMs} className="code-card-notif" />
    <AfModal open={confirm} onClose={() => { if (!archiving) setConfirm(false); }} title="Archive automation?" size="narrow"
      footer={<>
        <button className="code-subtle-button" disabled={archiving} onClick={() => setConfirm(false)}>Cancel</button>
        <button className="code-subtle-button" disabled={!archive.enabled} onClick={async () => {
          if (archiving || !archive.enabled) return;
          setArchiving(true);
          try { await props.onArchive?.(); setConfirm(false); }
          catch (e) { setError(toApiError(e).message); }
          finally { setArchiving(false); }
        }}>{archiving ? "Archiving…" : "Archive"}</button>
      </>}>
      <p>Archive “{s.title}”? Scheduled runs will stop. Its history is kept and you can restore it from Archived.</p>
      {error && <p role="alert" className="code-inline-error">{error}</p>}
    </AfModal>
  </div>;
}
