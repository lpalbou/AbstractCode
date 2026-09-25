import React, { useEffect, useState } from "react";
import { Icon } from "@abstractframework/ui-kit";
import { JsonViewer, Markdown } from "@abstractframework/panel-chat";
import { formatError, gatewayRequest } from "./transport";
import { copy_text } from "../lib/clipboard";

/** `GET /runs/{run_id}/workspace` (CONTRACTS §W). */
export type RunWorkspace = {
  workspace_root: string;
  kind?: string;
  session_id?: string;
  exists: boolean;
  host?: { hostname?: string; caller_is_this_machine?: boolean };
  open_supported?: boolean;
};

export type WorkspaceEntry = {
  name: string;
  path: string;
  type: "file" | "dir";
  size_bytes?: number;
  mtime?: string | number;
};

export type WorkspaceListing = {
  path: string;
  entries: WorkspaceEntry[];
  truncated: boolean;
};

export type PreviewKind = "markdown" | "json" | "image" | "html" | "text" | "binary";

/** Bytes fetched for a text preview; larger files show their first part. */
export const PREVIEW_TEXT_LIMIT = 512 * 1024;

const runPath = (runId: string) =>
  `/api/gateway/runs/${encodeURIComponent(runId)}/workspace`;

export function workspaceFilesUrl(runId: string, path: string): string {
  const params = new URLSearchParams({ path, recursive: "false" });
  return `${runPath(runId)}/files?${params}`;
}

export function workspaceContentUrl(runId: string, path: string): string {
  return `${runPath(runId)}/content?${new URLSearchParams({ path })}`;
}

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

export function formatBytes(value: number | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "";
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size < 10 ? size.toFixed(1) : Math.round(size)} ${units[unit]}`;
}

export function formatMtime(value: string | number | undefined): string {
  if (value === undefined || value === null || value === "") return "";
  const date = new Date(typeof value === "number" && value < 1e12 ? value * 1000 : value);
  return Number.isFinite(date.getTime())
    ? date.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })
    : String(value);
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

export function WorkspaceFileList({
  listing,
  selected,
  onOpenDir,
  onSelectFile,
}: {
  listing: WorkspaceListing;
  selected?: string;
  onOpenDir: (path: string) => void;
  onSelectFile: (entry: WorkspaceEntry) => void;
}): React.ReactElement {
  const entries = [...listing.entries].sort(
    (a, b) =>
      Number(b.type === "dir") - Number(a.type === "dir") || a.name.localeCompare(b.name),
  );
  return (
    <>
      <ul className="code-file-list code-session-files">
        {entries.map((entry) => (
          <li key={entry.path}>
            <button
              className={selected === entry.path ? "is-selected" : undefined}
              title={entry.type === "dir" ? `Open ${entry.path}` : `Preview ${entry.path}`}
              onClick={() =>
                entry.type === "dir" ? onOpenDir(entry.path) : onSelectFile(entry)
              }
            >
              <Icon name={entry.type === "dir" ? "chevronRight" : "edit"} size={14} />
              <span>{entry.type === "dir" ? `${entry.name}/` : entry.name}</span>
              <small className="code-file-meta">
                {entry.type === "file" ? formatBytes(entry.size_bytes) : ""}
                {entry.mtime !== undefined ? ` · ${formatMtime(entry.mtime)}` : ""}
              </small>
            </button>
          </li>
        ))}
      </ul>
      {listing.truncated ? (
        <p className="code-field-help" role="status">
          The gateway listed only part of this folder ({listing.entries.length} entries shown).
        </p>
      ) : null}
      {!listing.entries.length ? (
        <p className="code-pane-empty">This folder is empty.</p>
      ) : null}
    </>
  );
}

type PreviewState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; kind: PreviewKind; text?: string; partial: boolean };

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
            <p className="code-field-help">
              Showing the first {formatBytes(PREVIEW_TEXT_LIMIT)} of {formatBytes(entry.size_bytes)}. Download for the whole file.
            </p>
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
  onAttachFiles,
}: {
  runId: string;
  enabled: boolean;
  refreshKey?: string;
  onAttachFiles: (files: File[]) => void;
}): React.ReactElement {
  const [info, setInfo] = useState<RunWorkspace | null>(null);
  const [infoError, setInfoError] = useState("");
  const [directory, setDirectory] = useState("");
  const [listing, setListing] = useState<WorkspaceListing | null>(null);
  const [listError, setListError] = useState("");
  const [loading, setLoading] = useState(false);
  const [revision, setRevision] = useState(0);
  const [notice, setNotice] = useState("");
  const [opening, setOpening] = useState(false);
  const [selected, setSelected] = useState<WorkspaceEntry | null>(null);
  const [preview, setPreview] = useState<PreviewState>({ status: "idle" });
  const [attaching, setAttaching] = useState(false);

  useEffect(() => {
    setDirectory("");
    setSelected(null);
  }, [runId]);

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
    setListing(null);
    setListError("");
    if (!enabled || !runId || !info?.exists) return;
    const abort = new AbortController();
    setLoading(true);
    void gatewayRequest<WorkspaceListing>(workspaceFilesUrl(runId, directory), {
      signal: abort.signal,
    })
      .then((data) =>
        setListing({
          path: String(data?.path ?? directory),
          entries: Array.isArray(data?.entries) ? data.entries : [],
          truncated: data?.truncated === true,
        }),
      )
      .catch((e) => {
        if (!abort.signal.aborted) setListError(formatError(e));
      })
      .finally(() => {
        if (!abort.signal.aborted) setLoading(false);
      });
    return () => abort.abort();
  }, [enabled, runId, info, directory, refreshKey]);

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
    const partial = (selected.size_bytes ?? 0) > PREVIEW_TEXT_LIMIT;
    void fetch(workspaceContentUrl(runId, selected.path), {
      credentials: "same-origin",
      signal: abort.signal,
      headers: partial ? { Range: `bytes=0-${PREVIEW_TEXT_LIMIT - 1}` } : {},
    })
      .then(async (response) => {
        if (!response.ok)
          throw new Error(
            `Preview failed (${response.status}): ${(await response.text()) || response.statusText}`,
          );
        const kind = previewKind(selected.name, response.headers.get("content-type") || "");
        if (kind === "binary" || kind === "image") {
          setPreview({ status: "ready", kind, partial: false });
          return;
        }
        setPreview({
          status: "ready",
          kind,
          text: await response.text(),
          partial: partial && response.status === 206,
        });
      })
      .catch((e) => {
        if (!abort.signal.aborted) setPreview({ status: "error", message: formatError(e) });
      });
    return () => abort.abort();
  }, [runId, selected]);

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
    setAttaching(true);
    setNotice("");
    try {
      const response = await fetch(workspaceContentUrl(runId, selected.path), {
        credentials: "same-origin",
      });
      if (!response.ok)
        throw new Error(`Could not read ${selected.name} (${response.status}): ${await response.text()}`);
      const blob = await response.blob();
      onAttachFiles([new File([blob], selected.name, { type: blob.type })]);
      setNotice(`${selected.name} added to the message.`);
    } catch (e) {
      setNotice(formatError(e));
    } finally {
      setAttaching(false);
    }
  };

  return (
    <>
      <div className="code-pane-intro">
        <Icon name="terminal" size={17} />
        <div>
          <strong>Conversation workspace</strong>
          <span>{directory ? `/${directory}` : "Top folder"}</span>
        </div>
        <button
          className="code-icon-button"
          aria-label="Refresh files"
          onClick={() => setRevision((n) => n + 1)}
          disabled={!enabled}
        >
          <Icon name="refresh" size={14} />
        </button>
      </div>
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
      {directory ? (
        <button
          className="code-back"
          onClick={() => {
            setDirectory(directory.split("/").slice(0, -1).join("/"));
            setSelected(null);
          }}
        >
          ← Parent folder
        </button>
      ) : null}
      {loading ? <p className="code-muted" role="status">Loading files…</p> : null}
      {listError ? (
        <div className="code-inline-error" role="alert">
          {listError}
          <button onClick={() => setRevision((n) => n + 1)}>Retry</button>
        </div>
      ) : null}
      {listing ? (
        <WorkspaceFileList
          listing={listing}
          selected={selected?.path}
          onOpenDir={(path) => {
            setDirectory(path);
            setSelected(null);
          }}
          onSelectFile={setSelected}
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
