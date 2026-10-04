// Workspace rail panel, round 11 (DESIGN R11.1 FINAL / R11.6): the kit
// WorkspaceChooser at the level the selection needs.
//
// - A conversation = the SESSION level: this conversation's workspaces, stored
//   by the gateway on the session (GET/PUT api/gateway/sessions/{id}/workspaces),
//   so every app opening the conversation sees the same choice. "Use my
//   default" = {configured:false} (the account default applies). Each change is
//   one PUT; a refusal shows the gateway's sentence + "Not saved.".
// - "My default workspaces" opens the ACCOUNT level (PUT
//   api/gateway/workspace/policy/me) in a dialog: what every new conversation
//   of this account starts from.
// - An automation = the RUN level: its definition stores the payload
//   (input_data.workspace, see automation_settings.ts); the chooser shows the
//   gateway's dry run for it (POST api/gateway/workspace/effective/me).
//
// No policy logic here (no path checks, no clamp, no caps): the gateway
// decides and the kit renders its answers.
import React, { useCallback, useEffect, useState } from "react";
import {
  AfModal,
  WorkspaceChooser,
  workspaceChooserClient,
  workspaceDryRun,
  workspaceErrorSentence,
  type WorkspaceChooserState,
  type WorkspaceEffective,
  type WorkspacePayload,
  type WorkspaceRequest,
  type WorkspaceRunValue,
} from "@abstractframework/ui-kit";
import { formatError, gatewayRequest } from "./transport";

/** An automation's (or a one-off run's) workspaces: the start body's / definition's `workspace`. */
export type RunWorkspace = WorkspaceRunValue;

/** The app proxy's request for the kit client (JSON body; throws the gateway's sentence on 4xx). */
export const codeWorkspaceRequest: WorkspaceRequest = (path, init) =>
  gatewayRequest(path, { method: init.method, ...(init.body !== undefined ? { body: JSON.stringify(init.body) } : {}) });

const DISCONNECTED = "Connect to your gateway to change workspaces.";

/** One stored level (session or account): load on connect / id change, one PUT per change. */
function useStoredWorkspaces(
  connected: boolean,
  target: { level: "account" } | { level: "session"; session: string } | null,
  request: WorkspaceRequest,
  refreshKey?: unknown,
) {
  const [state, setState] = useState<WorkspaceChooserState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const key = target ? (target.level === "session" ? `session:${target.session}` : "account") : "";
  useEffect(() => {
    setState(null);
    setError(null);
    if (!connected || !target) return;
    let live = true;
    workspaceChooserClient(request, target)
      .load()
      .then((next) => live && setState(next))
      .catch((e) => live && setError(`Could not read the workspaces: ${formatError(e)}`));
    return () => {
      live = false;
    };
    // `target` is identified by `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connected, key, request, refreshKey]);
  const save = useCallback(
    async (payload: WorkspacePayload) => {
      if (!target) throw new Error(DISCONNECTED);
      const next = await workspaceChooserClient(request, target).save(payload);
      setState(next);
      return next;
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key, request],
  );
  return { state, error, save };
}

export type CodeWorkspaceFoldersProps = {
  connected: boolean;
  /** The open conversation's session id (session level). */
  sessionId?: string;
  /** Set for an automation: its stored payload and the setter that saves a new revision (run level). */
  automation?: { value: RunWorkspace | null; onChange: (next: RunWorkspace | null) => void };
  /** Opens "My default workspaces" (the account level). */
  onOpenDefaults?: () => void;
  /** Bumped when the account default changed (the session's "Use my default" view follows it). */
  refreshKey?: unknown;
  /** Injected in tests; the app proxy otherwise. */
  request?: WorkspaceRequest;
};

export function CodeWorkspaceFolders(props: CodeWorkspaceFoldersProps) {
  if (props.automation) return <CodeRunWorkspaces {...props} automation={props.automation} />;
  return <CodeSessionWorkspaces {...props} />;
}

/** The conversation's workspaces (session level) + the link to the account default. */
export function CodeSessionWorkspaces({ connected, sessionId, onOpenDefaults, refreshKey, request = codeWorkspaceRequest }: CodeWorkspaceFoldersProps) {
  const session = useStoredWorkspaces(connected, sessionId ? { level: "session", session: sessionId } : null, request, refreshKey);
  const unavailable = !connected ? DISCONNECTED : !sessionId ? "Open a conversation to choose its workspaces." : null;
  // The link sits outside the chooser so it stays reachable while the session loads or fails.
  return (
    <>
      <WorkspaceChooser
        level="session"
        idPrefix="code-workspace-session"
        state={session.state}
        save={session.save}
        loadError={unavailable ?? session.error}
        unavailableReason={unavailable}
      />
      {onOpenDefaults ? (
        <p className="code-default-workspaces">
          <button type="button" className="code-text-button" data-action="open-default-workspaces" onClick={onOpenDefaults} disabled={!connected}>
            My default workspaces
          </button>
        </p>
      ) : null}
    </>
  );
}

/** "My default workspaces": the account level (what this account's conversations start from). */
export function CodeDefaultWorkspacesDialog({ open, onClose, connected, onChanged, request = codeWorkspaceRequest }: {
  open: boolean;
  onClose: () => void;
  connected: boolean;
  /** Called after each saved change (the session panel reloads its default). */
  onChanged?: () => void;
  request?: WorkspaceRequest;
}) {
  const account = useStoredWorkspaces(connected && open, open ? { level: "account" } : null, request);
  const unavailable = connected ? null : DISCONNECTED;
  const save = useCallback(
    async (payload: WorkspacePayload) => {
      const next = await account.save(payload);
      onChanged?.();
      return next;
    },
    [account, onChanged],
  );
  return (
    <AfModal open={open} onClose={onClose} title="My default workspaces" size="narrow">
      <WorkspaceChooser
        level="account"
        idPrefix="code-workspace-account"
        state={account.state}
        save={save}
        loadError={unavailable ?? account.error}
        unavailableReason={unavailable}
      />
    </AfModal>
  );
}

/**
 * The run level's host: the value lives with its owner (an automation
 * definition, a run window), the effective workspaces come from the gateway's
 * dry run. A change is dry-run first; a refusal rejects with the gateway's
 * sentence (the kit shows it + "Not saved.") and the value stays.
 */
export function useRunWorkspaceLevel(
  connected: boolean,
  value: RunWorkspace | null,
  setValue: (next: RunWorkspace | null) => void,
  request: WorkspaceRequest,
) {
  const [effective, setEffective] = useState<WorkspaceEffective | null>(null);
  const [error, setError] = useState<string | null>(null);
  const valueKey = JSON.stringify(value);
  useEffect(() => {
    setError(null);
    if (!connected) {
      setEffective(null);
      return;
    }
    let live = true;
    workspaceDryRun(request)(value)
      .then((next) => live && setEffective(next))
      .catch((e) => live && setError(`Could not read the workspaces: ${workspaceErrorSentence(e) || formatError(e)}`));
    return () => {
      live = false;
    };
    // `value` is identified by `valueKey`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connected, valueKey, request]);
  const onChange = useCallback(
    async (next: RunWorkspace | null) => {
      const answer = await workspaceDryRun(request)(next);
      setEffective(answer);
      setValue(next);
    },
    [request, setValue],
  );
  return { effective, error, onChange };
}

function CodeRunWorkspaces({ connected, automation, request = codeWorkspaceRequest }: CodeWorkspaceFoldersProps & { automation: NonNullable<CodeWorkspaceFoldersProps["automation"]> }) {
  const run = useRunWorkspaceLevel(connected, automation.value, automation.onChange, request);
  const unavailable = connected ? null : DISCONNECTED;
  return (
    <WorkspaceChooser
      level="run"
      idPrefix="code-workspace-automation"
      value={automation.value}
      effective={run.effective}
      onChange={run.onChange}
      loadError={unavailable ?? run.error}
      unavailableReason={unavailable}
    />
  );
}
