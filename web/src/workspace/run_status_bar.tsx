import React, { useEffect, useMemo, useState } from "react";
import { Icon } from "@abstractframework/ui-kit";
import {
  workflowProgress,
  type WorkflowSessionSnapshot,
} from "@abstractframework/panel-chat";

import { llm_phase_text } from "../lib/llm_phase";
import { view_progress_records } from "../lib/activity_rows";

export function RunStatusBar({
  active,
  paused,
  snapshot,
  onCommand,
  onActivity,
  onInteraction,
  publicView = false,
  stopping = false,
}: {
  active: boolean;
  paused: boolean;
  snapshot: WorkflowSessionSnapshot;
  onCommand: (command: string) => void;
  onActivity: () => void;
  onInteraction?: () => void;
  publicView?: boolean;
  /** A Stop was pressed while the run was starting: "Stopping…" until the run has stopped. */
  stopping?: boolean;
}) {
  const recorded = useMemo(() => workflowProgress(snapshot), [snapshot]);
  // The caller ends `stopping` when the run it stops is shown stopped; the run on screen until
  // then may be the previous one (its "Completed" is not this Stop's answer).
  const progress = stopping
    ? { ...recorded, label: "Stopping…", detail: "", tone: "working" as const, toolCalls: 0, totalTokens: undefined }
    : recorded;
  // Live model phase, when the provider reports one. "Generating the next
  // response" is true of a 2-second prefill and of a 40-second one; this
  // replaces it with what the call is actually doing right now. Empty when no
  // provider on this run emitted phase events, and the panel's own detail
  // stands unchanged — never a synthesized phase.
  const phase = useMemo(
    () =>
      progress.tone === "working" && !stopping
        ? llm_phase_text(snapshot.view ? view_progress_records(snapshot.view as never) : snapshot.records.map((entry) => entry.record))
        : "",
    [progress.tone, stopping, snapshot.records, snapshot.view],
  );
  const detail = publicView ? "" : phase || progress.detail;
  const label = publicView && progress.tone === "working" ? "Working" : progress.label;
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [active]);
  const start = Date.parse(progress.startedAt || "");
  const elapsed = Number.isFinite(start)
    ? Math.max(0, Math.floor((now - start) / 1000))
    : null;
  const time =
    elapsed === null
      ? ""
      : elapsed < 60
        ? `${elapsed}s`
        : `${Math.floor(elapsed / 60)}m ${elapsed % 60}s`;
  return (
    <div className={`code-run-strip code-run-strip--${progress.tone}`}>
      <div
        className="code-run-strip__activity"
        role="status"
        aria-live="polite"
      >
        <span className="code-run-strip__icon" aria-hidden="true">
          <Icon
            name={
              progress.tone === "working"
                ? "loader"
                : progress.tone === "success"
                  ? "check"
                  : progress.tone === "error"
                    ? "error"
                    : progress.tone === "waiting"
                      ? "history"
                      : "info"
            }
            size={15}
          />
        </span>
        {onInteraction && progress.tone === "waiting" ? (
          <button
            className="code-run-strip__attention"
            onClick={onInteraction}
            title="Go to the pending request"
          >
            <strong>{label}</strong>
            <Icon name="chevronRight" size={12} />
          </button>
        ) : (
          <strong>{label}</strong>
        )}
        {/* A live model phase stays visible on phones too (the static detail does not). */}
        <span className={`code-run-strip__detail${phase ? " code-run-strip__detail--live" : ""}`} title={detail}>
          {detail}
        </span>
      </div>
      <span
        className="code-run-strip__metrics"
        aria-label="Run activity metrics"
      >
        {active && time ? <span>{time}</span> : null}
        {!publicView && progress.toolCalls ? (
          <span>
            {progress.toolCalls} {progress.toolCalls === 1 ? "tool" : "tools"}
          </span>
        ) : null}
        {!publicView && progress.totalTokens !== undefined ? (
          <span>{progress.totalTokens.toLocaleString()} tokens</span>
        ) : null}
      </span>
      {active ? (
        <>
          <button onClick={() => onCommand(paused ? "resume" : "pause")}>
            {paused ? "Resume" : "Pause"} {/* state-toggle-lint: allow one-shot command on the running run (pause/resume the run tree), not a setting */}
          </button>
          <button onClick={() => onCommand("conclude")}>{publicView ? "Finish now" : "Conclude"}</button>
        </>
      ) : null}
      <button onClick={onActivity}>
        View activity <Icon name="chevronRight" size={12} />
      </button>
    </div>
  );
}
