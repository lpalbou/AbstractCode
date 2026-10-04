// An automation's Workspaces (round 13, DESIGN R13.2 + R11.1 FINAL): the kit
// WorkspaceChooser at the RUN level, the same section and words as
// AbstractObserver's and the AbstractAssistant's automation forms.
//
// - New automation: the kit AfScheduleDialog's visible "Workspaces" section
//   (its `workspaces` slot) holds the chooser; the value is merged into the
//   create body's `target.input_data.workspace` (absent = "Use my default").
//   The gateway stores it and clamps it to the eligible workspaces at each run.
// - Existing automation: the rail's Workspace panel edits it (one revision
//   per change, automation_settings.ts); the header shows one line,
//   "Workspaces: <summary>", where the summary is the gateway's dry run
//   (POST api/gateway/workspace/effective/me {workspace}), verbatim.
// No policy logic here (no path checks, no clamp, no caps).
import React, { useEffect, useState } from "react";
import { WORKSPACE_CHOOSER_TEXT, workspaceDryRun, workspaceErrorSentence, type AutomationDefinition, type WorkspaceRequest } from "@abstractframework/ui-kit";

import type { RunWorkspace } from "./settings_panel";

/** The section title in all three apps (the kit's chooser title). */
export const AUTOMATION_WORKSPACES_TITLE = WORKSPACE_CHOOSER_TEXT.title;

/** "Workspaces: <summary>" — the automation's one line (the summary is the gateway's, verbatim). */
export function workspacesLine(summary: string): string {
  return `${AUTOMATION_WORKSPACES_TITLE}: ${summary}`;
}

function isPayload(v: unknown): v is RunWorkspace {
  return Boolean(v && typeof v === "object" && !Array.isArray(v) && Array.isArray((v as RunWorkspace).folders));
}

/** The automation's stored workspaces (`target.input_data.workspace`), or null ("Use my default"). */
export function automationWorkspace(definition: Pick<AutomationDefinition, "target"> | null | undefined): RunWorkspace | null {
  const v = (definition?.target?.input_data as Record<string, unknown> | undefined)?.workspace;
  return isPayload(v) ? v : null;
}

/**
 * A create body's `input_data` with the dialog's workspaces: the payload when
 * chosen (a stale R9 list or access mode is dropped: the gateway derives them
 * from the payload), nothing when "Use my default".
 */
export function withAutomationWorkspace(input: Record<string, any> | undefined, value: RunWorkspace | null): Record<string, any> {
  const out: Record<string, any> = { ...(input || {}) };
  if (value === null) {
    delete out.workspace;
    return out;
  }
  delete out.workspace_allowed_paths;
  delete out.workspace_access_mode;
  out.workspace = { posture: value.posture, default_mode: value.default_mode, folders: value.folders.map((f) => ({ path: f.path, mode: f.mode })) };
  return out;
}

const SUMMARY_TTL_MS = 30_000;
const summaries = new WeakMap<WorkspaceRequest, Map<string, { at: number; answer: Promise<string> }>>();

/** The gateway's effective summary for `value` (dry run), cached briefly per transport and value. */
export function workspaceSummary(request: WorkspaceRequest, value: RunWorkspace | null, now = Date.now()): Promise<string> {
  let cache = summaries.get(request);
  if (!cache) summaries.set(request, (cache = new Map()));
  const key = JSON.stringify(value);
  const hit = cache.get(key);
  if (hit && now - hit.at < SUMMARY_TTL_MS) return hit.answer;
  const answer = workspaceDryRun(request)(value).then((e) => e.summary);
  answer.catch(() => cache?.delete(key));
  cache.set(key, { at: now, answer });
  return answer;
}

/** "Workspaces: <summary>" for one automation; nothing while unknown, the gateway's sentence when it refuses. */
export function AutomationWorkspacesLine(props: { request: WorkspaceRequest; connected: boolean; value: RunWorkspace | null; refreshKey?: string | number }): React.ReactElement | null {
  const [text, setText] = useState<string | null>(null);
  const key = JSON.stringify(props.value);
  useEffect(() => {
    if (!props.connected) {
      setText(null);
      return;
    }
    let live = true;
    workspaceSummary(props.request, props.value)
      .then((s) => live && setText(workspacesLine(s)))
      .catch((e) => live && setText(workspacesLine(workspaceErrorSentence(e))));
    return () => {
      live = false;
    };
    // `value` is identified by `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.request, props.connected, key, props.refreshKey]);
  if (!text) return null;
  return (
    <span className="code-auto-header__workspaces" data-field="workspaces" title={text}>
      {text}
    </span>
  );
}
