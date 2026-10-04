// Workspace rail panel, round 9: the kit WorkspaceChooser over the gateway's
// workspace model (the admin allows, the account fine-tunes within it).
//
// - A conversation edits the signed-in ACCOUNT's folders
//   (GET/PUT api/gateway/workspace/policy/me): the shared workspace is always
//   on, each admin-allowed folder is a switch, "My folders" only when the
//   gateway allows any folder. Each change is one PUT; the gateway's refusal
//   sentence is shown with "Not saved.". Conversation turns send no folder
//   list: the gateway applies the account's effective folders.
// - An automation stores its chosen set in its definition
//   (input_data.workspace_allowed_paths, see automation_settings.ts), chosen
//   among the account's effective folders; null = follows the account.
//
// No policy logic here (round 9 X4): no path checks, no clamp, no filtering —
// the gateway decides and the kit only renders the folders it lists.
import React, { useCallback, useEffect, useState } from "react";
import {
  WorkspaceChooser,
  workspaceChooserClient,
  type WorkspaceAccountState,
  type WorkspaceRequest,
} from "@abstractframework/ui-kit";
import { formatError, gatewayRequest } from "./transport";

/** The app proxy's request for the kit client (JSON body; the gateway's `detail` sentence on 4xx). */
export const codeWorkspaceRequest: WorkspaceRequest = (path, init) =>
  gatewayRequest(path, { method: init.method, ...(init.body !== undefined ? { body: JSON.stringify(init.body) } : {}) });

/** The signed-in account's workspace state, loaded once per connection (and on `refreshKey`). */
export function useWorkspaceAccount(connected: boolean, refreshKey?: unknown, request: WorkspaceRequest = codeWorkspaceRequest) {
  const [state, setState] = useState<WorkspaceAccountState | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!connected) {
      setState(null);
      setError(null);
      return;
    }
    let live = true;
    setError(null);
    workspaceChooserClient(request)
      .load()
      .then((next) => live && setState(next))
      .catch((e) => live && setError(`Could not read your workspace folders: ${formatError(e)}`));
    return () => {
      live = false;
    };
  }, [connected, refreshKey, request]);
  const put = useCallback(
    async (body: Record<string, unknown>) => {
      setState(await workspaceChooserClient(request).put(body));
    },
    [request],
  );
  return { state, error, put };
}

export type CodeWorkspaceFoldersProps = {
  connected: boolean;
  /** Set for an automation: its stored set and the setter that saves a new revision. */
  automation?: { selection: string[] | null; onChange: (next: string[] | null) => void };
  /** Injected in tests; the app proxy otherwise. */
  request?: WorkspaceRequest;
};

export function CodeWorkspaceFolders({ connected, automation, request }: CodeWorkspaceFoldersProps) {
  const account = useWorkspaceAccount(connected, undefined, request);
  const unavailable = connected ? null : "Connect to your gateway to change workspace folders.";
  if (automation)
    return (
      <WorkspaceChooser
        mode="automation"
        idPrefix="code-workspace-automation"
        effective={account.state?.effective ?? null}
        selection={automation.selection}
        onSelectionChange={automation.onChange}
        loadError={connected ? account.error : unavailable}
        unavailableReason={unavailable}
      />
    );
  return (
    <WorkspaceChooser
      idPrefix="code-workspace-account"
      state={account.state}
      onPut={account.put}
      loadError={connected ? account.error : unavailable}
      unavailableReason={unavailable}
    />
  );
}
