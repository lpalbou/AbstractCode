import React, { useEffect, useMemo, useState } from "react";
import { Icon } from "@abstractframework/ui-kit";
import {
  workflowProgress,
  type WorkflowSessionSnapshot,
} from "@abstractframework/panel-chat";

import { llm_phase_text } from "../lib/llm_phase";

export function RunStatusBar({
  active,
  paused,
  snapshot,
  onRevokeApproval,
  onCommand,
  onActivity,
  onInteraction,
  permissionsAll,
}: {
  active: boolean;
  paused: boolean;
  snapshot: WorkflowSessionSnapshot;
  onRevokeApproval: () => void;
  onCommand: (command: string) => void;
  onActivity: () => void;
  onInteraction?: () => void;
  permissionsAll?: boolean;
}) {
  const progress = useMemo(() => workflowProgress(snapshot), [snapshot]);
  // Live model phase, when the provider reports one. "Generating the next
  // response" is true of a 2-second prefill and of a 40-second one; this
  // replaces it with what the call is actually doing right now. Empty when no
  // provider on this run emitted phase events, and the panel's own detail
  // stands unchanged — never a synthesized phase.
  const phase = useMemo(
    () => (progress.tone === "working" ? llm_phase_text(snapshot.records.map((entry) => entry.record)) : ""),
    [progress.tone, snapshot.records],
  );
  const detail = phase || progress.detail;
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
            <strong>{progress.label}</strong>
            <Icon name="chevronRight" size={12} />
          </button>
        ) : (
          <strong>{progress.label}</strong>
        )}
        <span className="code-run-strip__detail" title={detail}>
          {detail}
        </span>
      </div>
      <span
        className="code-run-strip__metrics"
        aria-label="Run activity metrics"
      >
        {active && time ? <span>{time}</span> : null}
        {progress.toolCalls ? (
          <span>
            {progress.toolCalls} {progress.toolCalls === 1 ? "tool" : "tools"}
          </span>
        ) : null}
        {progress.totalTokens !== undefined ? (
          <span>{progress.totalTokens.toLocaleString()} tokens</span>
        ) : null}
      </span>
      {active ? (
        <>
          <button onClick={() => onCommand(paused ? "resume" : "pause")}>
            {paused ? "Resume" : "Pause"}
          </button>
          <button onClick={() => onCommand("conclude")}>Conclude</button>
        </>
      ) : null}
      <button onClick={onActivity}>
        View activity <Icon name="chevronRight" size={12} />
      </button>
      {permissionsAll ? (
        <div className="code-run-strip__consent">
          <Icon name="check" size={13} />
          <span>Permissions: all · enabled tools only</span>
          <button onClick={onRevokeApproval}>Revoke</button>
        </div>
      ) : null}
    </div>
  );
}
