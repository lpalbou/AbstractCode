import { gatewayApiPath } from "@abstractframework/ui-kit";
import React, { useState } from "react";
import { Icon } from "@abstractframework/ui-kit";
import {
  WorkspaceBrowser,
  responseTotal,
  workspaceContentUrl,
  workspaceCanOpenFolder,
  type GatewayFetch,
  type RunWorkspace,
  type WorkspaceEntry,
} from "@abstractframework/panel-chat";
import { formatError, gatewayRequest } from "./transport";
import { copy_text } from "../lib/clipboard";
import { uploadRefusal, type PendingUpload } from "./attachment_uploads";

// Round 4: the folder listing AND the preview are the shared panel-chat
// `WorkspaceBrowser` (rows: name, size, generated date, download; a click
// previews the file in the kit's `FileViewer`; the root shows once as a short
// name with open-folder / copy-path icons). What stays AbstractCode's own:
// "Attach" (queue a workspace file for the next message) and the CSRF-aware
// "open folder" request. Preview helpers moved to panel-chat; re-exported here.
export {
  PREVIEW_TEXT_LIMIT,
  PREVIEW_LIMIT_LABEL,
  readBoundedText,
  responseTotal,
  partialPreviewNote,
  safeMarkdownImages,
  type BoundedText,
} from "@abstractframework/panel-chat";

const runPath = (runId: string) => gatewayApiPath(`runs/${encodeURIComponent(runId)}/workspace`);

/** The app proxy as the shared browser's gateway fetch (session cookie). */
export const proxyGatewayFetch: GatewayFetch = (path, init) => fetch(path, { ...init, credentials: "same-origin" });

/** "Open folder" acts on the gateway machine, so it is offered only to a
 * browser on that machine and only where the gateway can open folders. */
export function canOpenFolder(info: RunWorkspace): boolean {
  return workspaceCanOpenFolder({ ...info, exists: true });
}

/** `POST /runs/{id}/workspace/open` through the app proxy (CSRF header included). */
export async function openRunFolder(runId: string): Promise<void> {
  await gatewayRequest(`${runPath(runId)}/open`, { method: "POST" });
}

/** Why a workspace file cannot be attached, decided BEFORE downloading it
 * (the gateway's `maxAttachmentBytes`, the only size rule), or null. */
export function attachPrecheck(entry: WorkspaceEntry, maxBytes: number | undefined): string | null {
  if (typeof entry.size_bytes !== "number") return null;
  return uploadRefusal({ name: entry.name, size: entry.size_bytes }, maxBytes);
}

/** What the Files panel says after handing a file to the upload queue: a
 * refusal carries its reason; success is never claimed before the upload. */
export function attachOutcomeNotice(name: string, chip: PendingUpload | undefined): string {
  if (!chip) return `${name} was not queued for upload.`;
  if (chip.status === "refused") return `Not attached: ${chip.message || "refused by the gateway's attachment policy"}`;
  return `${name} is uploading; its chip in the message box shows when it is attached.`;
}

/** Read a workspace file and queue it as an upload (size rule applied before and after the request). */
export async function attachWorkspaceFile(
  runId: string,
  entry: WorkspaceEntry,
  maxAttachmentBytes: number | undefined,
  onAttachFiles: (files: File[]) => PendingUpload[],
): Promise<string> {
  const refusal = attachPrecheck(entry, maxAttachmentBytes);
  if (refusal) return `Not attached: ${refusal}`;
  const abort = new AbortController();
  const response = await fetch(workspaceContentUrl(runId, entry.path), { credentials: "same-origin", signal: abort.signal });
  if (!response.ok) throw new Error(`Could not read ${entry.name} (${response.status}): ${await response.text()}`);
  // No listed size: the response's own length is checked before reading.
  const late = attachPrecheck({ ...entry, size_bytes: responseTotal(response) }, maxAttachmentBytes);
  if (late) {
    abort.abort();
    return `Not attached: ${late}`;
  }
  const blob = await response.blob();
  const [chip] = onAttachFiles([new File([blob], entry.name, { type: blob.type })]);
  return attachOutcomeNotice(entry.name, chip);
}

/** The run's own workspace: the files the agent works on, previewed in place. */
export function SessionFiles({
  runId,
  enabled,
  refreshKey,
  maxAttachmentBytes,
  onAttachFiles,
  heading = "Conversation files",
}: {
  runId: string;
  enabled: boolean;
  heading?: string;
  refreshKey?: string;
  /** The gateway's attachment size limit, checked before any download. */
  maxAttachmentBytes?: number;
  /** Queue files for upload; returns their chips (refused ones included). */
  onAttachFiles: (files: File[]) => PendingUpload[];
}): React.ReactElement {
  const [notice, setNotice] = useState("");
  const [attaching, setAttaching] = useState("");
  if (!runId)
    return (
      <div className="code-pane-empty">
        <Icon name="folder" size={28} />
        <p>This conversation's files appear here.</p>
        <small>Start a conversation; the files its workflow creates and edits are listed and previewed here.</small>
      </div>
    );
  return (
    <>
      {notice ? <p className="code-field-help" role="status">{notice}</p> : null}
      {enabled ? (
        <WorkspaceBrowser
          key={runId}
          fetchGateway={proxyGatewayFetch}
          runId={runId}
          title={heading}
          refreshKey={refreshKey}
          className="code-workspace-browser"
          copyText={copy_text}
          onOpenFolder={() => openRunFolder(runId)}
          fileActions={(entry) => (
            <button
              type="button"
              className="af-file-viewer__icon-btn"
              data-action="attach-file"
              title="Attach to the next message"
              aria-label={`Attach ${entry.name} to the next message`}
              disabled={attaching === entry.path}
              onClick={() => {
                setAttaching(entry.path);
                setNotice("");
                void attachWorkspaceFile(runId, entry, maxAttachmentBytes, onAttachFiles)
                  .then(setNotice)
                  .catch((e) => setNotice(formatError(e)))
                  .finally(() => setAttaching(""));
              }}
            >
              <Icon name={attaching === entry.path ? "loader" : "paperclip"} size={15} />
            </button>
          )}
        />
      ) : null}
    </>
  );
}
