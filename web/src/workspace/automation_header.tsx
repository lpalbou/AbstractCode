// The selected automation's header (round 4, DESIGN §4 "Automation header"). AbstractCode draws it
// itself and passes `hideHeader` to the kit panel below it:
//   title (wraps)
//   [Active] waiting for you   every 24 h · last 3 h ago · next in 14 h
//   Workspace  <short name> [open] [copy]          (never the full path on screen)
//   Run now · Stop · Edit · Archive                (Archive asks inline; no ellipsis)
// Every action shows a busy state, then an honest result line (the gateway's error when refused).
// Edit opens the Workflow panel on this automation (the host's `onEdit`).
import React, { useEffect, useRef, useState } from "react";
import {
  CONTROL_COMMANDS,
  Icon,
  apiErrorText,
  automationControls,
  automationTiming,
  controlHint,
  triggerSourceProblem,
  type ApiError,
  type AutomationSummary,
  type ControlId,
  type IconName,
  type OccurrenceRow,
  type TriggerSourceEntry,
} from "@abstractframework/ui-kit";

import { AutomationActiveSwitch, WaitingBadge, automationIsWaiting } from "./sidebar_cards";
import { workspaceLabel } from "./sidebar_panels";

/** What the result line says once the gateway accepted a command (the NEW state, not the verb). */
export const HEADER_NOTICES: Record<string, string> = {
  [CONTROL_COMMANDS.pause]: "Automation paused.",
  [CONTROL_COMMANDS.resume]: "Automation active.",
  [CONTROL_COMMANDS.run_now]: "Run requested.",
  [CONTROL_COMMANDS.stop_current]: "Stop requested.",
  [CONTROL_COMMANDS.archive]: "Automation archived.",
};

/** The header's buttons, in order, with their short labels (no ellipsis anywhere). */
export const HEADER_BUTTONS: Array<{ id: ControlId; label: string; icon: IconName; action: string }> = [
  { id: "run_now", label: "Run now", icon: "playCircle", action: "run_now" },
  { id: "stop_current", label: "Stop", icon: "stop", action: "stop_current" },
  { id: "revise", label: "Edit", icon: "edit", action: "edit" },
  { id: "archive", label: "Archive", icon: "archive", action: "archive" },
];

type Result = { tone: "busy" | "ok" | "error"; text: string; detail?: string };

function errorResult(e: unknown): Result {
  const err = e && typeof e === "object" && "code" in e ? (e as ApiError) : null;
  if (err) {
    const t = apiErrorText(err);
    return { tone: "error", text: t.title, detail: t.detail };
  }
  return { tone: "error", text: e instanceof Error ? e.message : String(e) };
}

export function AutomationHeaderBar(props: {
  summary: AutomationSummary;
  occurrences: OccurrenceRow[];
  /** `GET /trigger-sources`: a trigger this gateway cannot run is said under the timing line. */
  triggerSources: TriggerSourceEntry[];
  busy: boolean;
  nowMs?: number;
  /** Sends a full command type (`automation.run_now`…); rejects with the gateway's error. */
  onCommand(type: string): Promise<unknown>;
  onToggleActive(): Promise<unknown>;
  /** Opens the Workflow panel on this automation. */
  onEdit(): void;
  /** Shows the automation's folder (its files). */
  onOpenFolder(): void;
  /** Copies the folder's full path; resolves true when it reached the clipboard. */
  onCopyPath(path: string): Promise<boolean>;
}): React.ReactElement {
  const s = props.summary;
  const controls = automationControls(s, props.occurrences, props.busy);
  const timing = automationTiming(s, props.nowMs ?? Date.now());
  const [confirmingArchive, setConfirmingArchive] = useState(false);
  const [result, setResult] = useState<Result | null>(null);
  const confirmRef = useRef<HTMLButtonElement | null>(null);
  // Another automation opened: drop this one's confirmation and result.
  useEffect(() => {
    setConfirmingArchive(false);
    setResult(null);
  }, [s.automation_id]);
  useEffect(() => {
    if (confirmingArchive) confirmRef.current?.focus();
  }, [confirmingArchive]);

  const run = (label: string, task: () => Promise<unknown>, done: string) => {
    setResult({ tone: "busy", text: label });
    task().then(
      () => setResult({ tone: "ok", text: done }),
      (e) => setResult(errorResult(e)),
    );
  };
  const command = (type: string, busyText: string) => run(busyText, () => props.onCommand(type), HEADER_NOTICES[type] ?? "Done.");
  const onButton = (id: ControlId) => {
    if (id === "run_now") command(CONTROL_COMMANDS.run_now, "Starting a run…");
    else if (id === "stop_current") command(CONTROL_COMMANDS.stop_current, "Stopping…");
    else if (id === "revise") props.onEdit();
    else if (id === "archive") setConfirmingArchive(true);
  };
  const path = s.workspace_root || "";
  // Only once the sources are known (an empty list before the first read is not a problem).
  const problem = props.triggerSources.length ? triggerSourceProblem(s, props.triggerSources) : null;

  return (
    <header className="code-auto-header" data-automation-id={s.automation_id} aria-busy={props.busy || result?.tone === "busy"}>
      <h2 className="code-auto-header__title" tabIndex={-1}>{s.title}</h2>
      <div className="code-auto-header__state">
        <AutomationActiveSwitch
          variant="inline"
          summary={s}
          busy={props.busy}
          onToggle={() =>
            run(
              s.status === "active" ? "Pausing…" : "Activating…",
              props.onToggleActive,
              HEADER_NOTICES[s.status === "active" ? CONTROL_COMMANDS.pause : CONTROL_COMMANDS.resume],
            )
          }
        />
        {automationIsWaiting(s) ? <WaitingBadge /> : null}
        <span className="code-auto-header__timing" data-field="timing">{timing.line}</span>
      </div>
      {problem ? (
        <p className="code-auto-header__warn" role="note" data-field="trigger-problem">
          <Icon name="warning" size={13} /> <span>{problem}</span>
        </p>
      ) : null}
      {path ? (
        <div className="code-auto-header__workspace" data-field="workspace">
          <span className="code-auto-header__label">Workspace</span>
          <span className="code-auto-header__folder" title={path}>{workspaceLabel(path)}</span>
          <button type="button" className="code-icon-button" data-action="open-folder" aria-label="Open folder" title="Open folder" onClick={props.onOpenFolder}>
            <Icon name="folder" size={15} />
          </button>
          <button
            type="button"
            className="code-icon-button"
            data-action="copy-path"
            aria-label="Copy path"
            title="Copy path"
            onClick={() =>
              void props.onCopyPath(path).then(
                (ok) => setResult(ok ? { tone: "ok", text: "Path copied." } : { tone: "error", text: "The browser refused the clipboard." }),
                (e) => setResult(errorResult(e)),
              )
            }
          >
            <Icon name="copy" size={15} />
          </button>
        </div>
      ) : null}
      {confirmingArchive ? (
        <div className="code-auto-header__confirm" role="group" aria-label="Confirm archive">
          <p>Archive “{s.title}”? It will not run again; its history stays readable.</p>
          <div className="code-auto-header__actions">
            <button
              ref={confirmRef}
              type="button"
              className="code-subtle-button code-danger-button"
              data-action="archive-confirm"
              disabled={!controls.archive.enabled}
              onClick={() => {
                setConfirmingArchive(false);
                command(CONTROL_COMMANDS.archive, "Archiving…");
              }}
            >
              <Icon name="archive" size={14} /> <span>Archive</span>
            </button>
            <button type="button" className="code-subtle-button" data-action="archive-cancel" onClick={() => setConfirmingArchive(false)}>
              <span>Keep it</span>
            </button>
          </div>
        </div>
      ) : (
        <div className="code-auto-header__actions" role="toolbar" aria-label="Automation controls">
          {HEADER_BUTTONS.map((b) => {
            const st = controls[b.id];
            const hint = controlHint(b.id, s);
            return (
              <button
                key={b.id}
                type="button"
                className={`code-subtle-button${b.id === "archive" ? " code-danger-button" : ""}`}
                data-action={b.action}
                disabled={!st.enabled}
                title={st.enabled ? hint : `${st.reason ?? "Not available now."}\n${hint}`}
                onClick={() => onButton(b.id)}
              >
                <Icon name={b.icon} size={14} /> <span>{b.label}</span>
              </button>
            );
          })}
        </div>
      )}
      {result ? (
        <p className={`code-auto-header__result is-${result.tone}`} role={result.tone === "error" ? "alert" : "status"} data-field="result">
          <Icon name={result.tone === "busy" ? "loader" : result.tone === "ok" ? "check" : "error"} size={13} />
          <span>
            {result.text}
            {result.detail ? ` ${result.detail}` : ""}
          </span>
        </p>
      ) : null}
    </header>
  );
}
