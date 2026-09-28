import React, { useCallback, useEffect, useState } from "react";
import { Icon } from "@abstractframework/ui-kit";
import {
  JsonViewer,
  Markdown,
  WorkspaceBrowser,
  formatBytes,
  workspaceContentUrl,
  type GatewayFetch,
  type RunWorkspace,
  type WorkspaceEntry,
} from "@abstractframework/panel-chat";
import { formatError, gatewayRequest } from "./transport";
import { copy_text } from "../lib/clipboard";
import { uploadRefusal, type PendingUpload } from "./attachment_uploads";

// The folder listing, its types, URL builders and validation are the shared
// panel-chat `WorkspaceBrowser` (one browser for every client); this module
// keeps what is AbstractCode's own: the path header (copy / open folder), the
// preview and "attach to conversation".

export type PreviewKind = "markdown" | "json" | "image" | "html" | "text" | "binary";

/** Bytes read for a text preview; larger files show their first part. */
export const PREVIEW_TEXT_LIMIT = 1024 * 1024;
export const PREVIEW_LIMIT_LABEL = "1 MiB";

const runPath = (runId: string) =>
  `/api/gateway/runs/${encodeURIComponent(runId)}/workspace`;

/** The app proxy as the shared browser's gateway fetch (session cookie). */
export const proxyGatewayFetch: GatewayFetch = (path, init) =>
  fetch(path, { ...init, credentials: "same-origin" });

const TEXT_EXTENSIONS = new Set(
  (
    "txt log csv tsv py js mjs cjs ts tsx jsx rs go java kt c h cc cpp hpp cs rb php swift sh bash zsh " +
    "fish ps1 toml yaml yml ini cfg conf env xml css scss less sql graphql proto gradle make mk " +
    "dockerfile gitignore lock r jl lua pl ex exs erl hs ml vue svelte tex rst adoc org diff patch"
  ).split(" "),
);
const IMAGE_EXTENSIONS = new Set("png jpg jpeg gif webp bmp ico avif svg".split(" "));

function extension(name: string): string {
  const base = name.split("/").pop() || name;
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : base.toLowerCase();
}

/** Choose how a file is previewed, from its name and (once fetched) the
 * Content-Type the gateway reported. Unknown types are offered as a download. */
export function previewKind(name: string, contentType = ""): PreviewKind {
  const ext = extension(name);
  const type = contentType.split(";", 1)[0].trim().toLowerCase();
  if (ext === "md" || ext === "markdown" || type === "text/markdown") return "markdown";
  if (ext === "json" || ext === "jsonl" || type === "application/json" || type.endsWith("+json"))
    return ext === "jsonl" ? "text" : "json";
  if (IMAGE_EXTENSIONS.has(ext) || type.startsWith("image/")) return "image";
  if (ext === "html" || ext === "htm" || type === "text/html") return "html";
  if (TEXT_EXTENSIONS.has(ext) || type.startsWith("text/")) return "text";
  if (["application/javascript", "application/xml", "application/x-yaml", "application/toml"].includes(type))
    return "text";
  return "binary";
}

/** "Open folder" acts on the gateway machine, so it is offered only to a
 * browser on that machine and only where the gateway can open folders. */
export function canOpenFolder(info: RunWorkspace): boolean {
  return info.open_supported === true && info.host?.caller_is_this_machine === true;
}

export function WorkspaceHeader({
  info,
  onCopy,
  onOpen,
  opening,
}: {
  info: RunWorkspace;
  onCopy: () => void;
  onOpen: () => void;
  opening?: boolean;
}): React.ReactElement {
  const remote = info.host?.caller_is_this_machine !== true;
  return (
    <div className="code-session-workspace">
      <div className="code-session-workspace-path">
        <code title={info.workspace_root}>{info.workspace_root}</code>
        <button
          className="code-icon-button"
          aria-label="Copy workspace path"
          title="Copy path"
          onClick={onCopy}
        >
          <Icon name="copy" size={13} />
        </button>
      </div>
      {remote ? (
        <p className="code-field-help">
          on the gateway host {info.host?.hostname || "(hostname not reported)"}
        </p>
      ) : null}
      {!info.exists ? (
        <p className="code-field-help">
          This folder does not exist yet. It appears when the workflow writes its first file.
        </p>
      ) : null}
      {canOpenFolder(info) ? (
        <button className="code-subtle-button" onClick={onOpen} disabled={opening}>
          <Icon name="terminal" size={13} />
          <span>{opening ? "Opening…" : "Open folder"}</span>
        </button>
      ) : null}
    </div>
  );
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
  if (chip.status === "refused")
    return `Not attached: ${chip.message || "refused by the gateway's attachment policy"}`;
  return `${name} is uploading; its chip in the message box shows when it is attached.`;
}

export type BoundedText = { text: string; received: number; total?: number; partial: boolean };

/** The size a content response reports: the total of `Content-Range`
 * (a 206), else `Content-Length` of a full 200 answer. */
export function responseTotal(response: Response): number | undefined {
  const range = response.headers.get("content-range");
  const total = range ? Number(range.split("/")[1]) : NaN;
  if (Number.isFinite(total)) return total;
  const length = Number(response.headers.get("content-length"));
  return response.status === 200 && Number.isFinite(length) && response.headers.has("content-length")
    ? length
    : undefined;
}

/** Read at most `limit` bytes of a response body as text, then stop reading
 * (never the whole of a large file). */
export async function readBoundedText(
  response: Response,
  limit: number,
  knownSize?: number,
): Promise<BoundedText> {
  const decoder = new TextDecoder();
  let text = "";
  let received = 0;
  const reader = response.body?.getReader();
  if (reader) {
    while (received < limit) {
      const { done, value } = await reader.read();
      if (done) break;
      const chunk = value.subarray(0, limit - received);
      received += chunk.length;
      text += decoder.decode(chunk, { stream: true });
    }
    await reader.cancel().catch(() => {});
  }
  const total = responseTotal(response) ?? knownSize;
  const partial = total !== undefined ? total > received : received >= limit;
  return { text, received, total, partial };
}

export function partialPreviewNote(total: number | undefined): string {
  return `Showing the first ${PREVIEW_LIMIT_LABEL} of ${total !== undefined ? formatBytes(total) : "a file of unknown size"}; download for the rest.`;
}

/** Markdown previews never load images from elsewhere: an image renders only
 * when its URL is this conversation's workspace content route (a relative
 * path is resolved against the Markdown file's folder); any other image
 * becomes a plain link. Fenced code is left untouched. */
export function safeMarkdownImages(text: string, runId: string, markdownPath: string): string {
  const contentPrefix = `/api/gateway/runs/${encodeURIComponent(runId)}/workspace/content?`;
  const folder = markdownPath.split("/").slice(0, -1);
  const resolveRelative = (src: string): string | null => {
    if (/^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith("/") || src.startsWith("#")) return null;
    const parts = [...folder];
    for (const part of src.split(/[?#]/, 1)[0].split("/")) {
      if (!part || part === ".") continue;
      if (part === "..") {
        if (!parts.length) return null;
        parts.pop();
      } else parts.push(decodeURIComponentSafe(part));
    }
    return parts.length ? workspaceContentUrl(runId, parts.join("/")) : null;
  };
  const rewriteLine = (line: string): string => {
    let out = "";
    let i = 0;
    while (i < line.length) {
      if (line[i] === "!" && line[i + 1] === "[") {
        const labelEnd = line.indexOf("]", i + 2);
        if (labelEnd !== -1 && line[labelEnd + 1] === "(") {
          const hrefEnd = line.indexOf(")", labelEnd + 2);
          if (hrefEnd !== -1) {
            const alt = line.slice(i + 2, labelEnd);
            const src = line.slice(labelEnd + 2, hrefEnd).trim();
            const local = src.startsWith(contentPrefix) ? src : resolveRelative(src);
            out += local ? `![${alt}](${local})` : `[image: ${alt || src}](${src})`;
            i = hrefEnd + 1;
            continue;
          }
        }
      }
      out += line[i];
      i += 1;
    }
    return out;
  };
  let fenced = false;
  return text
    .split("\n")
    .map((line) => {
      if (/^\s*(```|~~~)/.test(line)) {
        fenced = !fenced;
        return line;
      }
      return fenced ? line : rewriteLine(line);
    })
    .join("\n");
}

function decodeURIComponentSafe(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

type PreviewState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; kind: PreviewKind; text?: string; partial: boolean; total?: number };

export function FilePreview({
  entry,
  url,
  state,
  onAttach,
  attaching,
}: {
  entry: WorkspaceEntry;
  url: string;
  state: PreviewState;
  onAttach: () => void;
  attaching: boolean;
}): React.ReactElement {
  return (
    <div className="code-file-preview" aria-label={`Preview of ${entry.name}`}>
      <div className="code-file-preview-head">
        <strong title={entry.path}>{entry.name}</strong>
        <a className="code-text-button" href={url} download={entry.name}>
          Download
        </a>
        <button className="code-text-button" onClick={onAttach} disabled={attaching}>
          {attaching ? "Attaching…" : "Attach to conversation"}
        </button>
      </div>
      {state.status === "loading" ? (
        <p className="code-muted" role="status">Loading preview…</p>
      ) : state.status === "error" ? (
        <p className="code-error-text" role="alert">{state.message}</p>
      ) : state.status === "ready" ? (
        <>
          {state.partial ? (
            <p className="code-field-help" role="status">{partialPreviewNote(state.total)}</p>
          ) : null}
          {state.kind === "image" ? (
            <img className="code-file-preview-image" src={url} alt={entry.name} />
          ) : state.kind === "markdown" ? (
            <Markdown text={state.text || ""} />
          ) : state.kind === "json" ? (
            <JsonPreview text={state.text || ""} />
          ) : state.kind === "binary" ? (
            <p className="code-field-help">
              No preview for this file type. Use Download to open it.
            </p>
          ) : (
            <pre className="code-file-preview-text">{state.text}</pre>
          )}
        </>
      ) : null}
    </div>
  );
}

function JsonPreview({ text }: { text: string }) {
  try {
    return <JsonViewer value={JSON.parse(text)} collapseAfterDepth={2} />;
  } catch (e) {
    return (
      <>
        <p className="code-field-help">Not valid JSON ({formatError(e)}); shown as text.</p>
        <pre className="code-file-preview-text">{text}</pre>
      </>
    );
  }
}

/** The run's own workspace: the files the agent works on, with previews. */
export function SessionFiles({
  runId,
  enabled,
  refreshKey,
  maxAttachmentBytes,
  onAttachFiles,
  heading = "Conversation workspace",
}: {
  runId: string;
  enabled: boolean;
  /** The pane's title (the run's own workspace, or an automation's folder). */
  heading?: string;
  refreshKey?: string;
  /** The gateway's attachment size limit, checked before any download. */
  maxAttachmentBytes?: number;
  /** Queue files for upload; returns their chips (refused ones included). */
  onAttachFiles: (files: File[]) => PendingUpload[];
}): React.ReactElement {
  const [info, setInfo] = useState<RunWorkspace | null>(null);
  const [infoError, setInfoError] = useState("");
  const [revision, setRevision] = useState(0);
  const [notice, setNotice] = useState("");
  const [opening, setOpening] = useState(false);
  const [selected, setSelected] = useState<WorkspaceEntry | null>(null);
  const [preview, setPreview] = useState<PreviewState>({ status: "idle" });
  const [attaching, setAttaching] = useState(false);
  const onSelectFile = useCallback((entry: WorkspaceEntry) => setSelected(entry), []);

  useEffect(() => {
    setSelected(null);
  }, [runId]);

  // The path header's facts (copy, open folder on this machine).
  useEffect(() => {
    setInfo(null);
    setInfoError("");
    if (!enabled || !runId) return;
    const abort = new AbortController();
    void gatewayRequest<RunWorkspace>(runPath(runId), { signal: abort.signal })
      .then((data) => setInfo(data))
      .catch((e) => {
        if (!abort.signal.aborted) setInfoError(formatError(e));
      });
    return () => abort.abort();
  }, [enabled, runId, revision]);

  useEffect(() => {
    if (!selected || !runId) {
      setPreview({ status: "idle" });
      return;
    }
    const byName = previewKind(selected.name);
    if (byName === "image") {
      setPreview({ status: "ready", kind: "image", partial: false });
      return;
    }
    const abort = new AbortController();
    setPreview({ status: "loading" });
    // Always bounded: the content route supports Range, and the body is read
    // only up to the limit even when a server ignores Range.
    void fetch(workspaceContentUrl(runId, selected.path), {
      credentials: "same-origin",
      signal: abort.signal,
      headers: { Range: `bytes=0-${PREVIEW_TEXT_LIMIT - 1}` },
    })
      .then(async (response) => {
        if (!response.ok)
          throw new Error(
            `Preview failed (${response.status}): ${(await response.text()) || response.statusText}`,
          );
        const kind = previewKind(selected.name, response.headers.get("content-type") || "");
        if (kind === "binary" || kind === "image") {
          await response.body?.cancel().catch(() => {});
          setPreview({ status: "ready", kind, partial: false });
          return;
        }
        const read = await readBoundedText(response, PREVIEW_TEXT_LIMIT, selected.size_bytes);
        setPreview({
          status: "ready",
          kind,
          text: kind === "markdown" ? safeMarkdownImages(read.text, runId, selected.path) : read.text,
          partial: read.partial,
          total: read.total,
        });
      })
      .catch((e) => {
        if (!abort.signal.aborted) setPreview({ status: "error", message: formatError(e) });
      });
    return () => abort.abort();
  }, [runId, selected, refreshKey]);

  if (!runId)
    return (
      <div className="code-pane-empty">
        <Icon name="terminal" size={28} />
        <p>This conversation's files appear here.</p>
        <small>Start a conversation; the files its workflow creates and edits are listed and previewed here.</small>
      </div>
    );

  const attachSelected = async () => {
    if (!selected) return;
    setNotice("");
    // The size rule is applied before anything is downloaded.
    const refusal = attachPrecheck(selected, maxAttachmentBytes);
    if (refusal) {
      setNotice(`Not attached: ${refusal}`);
      return;
    }
    setAttaching(true);
    try {
      const abort = new AbortController();
      const response = await fetch(workspaceContentUrl(runId, selected.path), {
        credentials: "same-origin",
        signal: abort.signal,
      });
      if (!response.ok)
        throw new Error(`Could not read ${selected.name} (${response.status}): ${await response.text()}`);
      // No listed size: the response's own length is checked before reading.
      const late = attachPrecheck({ ...selected, size_bytes: responseTotal(response) }, maxAttachmentBytes);
      if (late) {
        abort.abort();
        setNotice(`Not attached: ${late}`);
        return;
      }
      const blob = await response.blob();
      const [chip] = onAttachFiles([new File([blob], selected.name, { type: blob.type })]);
      setNotice(attachOutcomeNotice(selected.name, chip));
    } catch (e) {
      setNotice(formatError(e));
    } finally {
      setAttaching(false);
    }
  };

  return (
    <>
      {infoError ? (
        <div className="code-inline-error" role="alert">
          Workspace unavailable: {infoError}
          <button onClick={() => setRevision((n) => n + 1)}>Retry</button>
        </div>
      ) : null}
      {info ? (
        <WorkspaceHeader
          info={info}
          opening={opening}
          onCopy={() => {
            void copy_text(info.workspace_root).then((ok) =>
              setNotice(ok ? "Path copied." : "Could not copy; select the path instead."),
            );
          }}
          onOpen={() => {
            setOpening(true);
            setNotice("");
            void gatewayRequest(`${runPath(runId)}/open`, { method: "POST" })
              .then(() => setNotice("Folder opened on this machine."))
              .catch((e) => setNotice(`Open folder failed: ${formatError(e)}`))
              .finally(() => setOpening(false));
          }}
        />
      ) : null}
      {notice ? <p className="code-field-help" role="status">{notice}</p> : null}
      {enabled ? (
        <WorkspaceBrowser
          key={runId}
          fetchGateway={proxyGatewayFetch}
          runId={runId}
          title={heading}
          onSelectFile={onSelectFile}
          selectedPath={selected?.path}
          refreshKey={`${refreshKey ?? ""}:${revision}`}
          className="code-workspace-browser"
        />
      ) : null}
      {selected ? (
        <FilePreview
          entry={selected}
          url={workspaceContentUrl(runId, selected.path)}
          state={preview}
          attaching={attaching}
          onAttach={() => void attachSelected()}
        />
      ) : null}
    </>
  );
}
