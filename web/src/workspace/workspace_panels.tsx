import { gatewayApiPath, formatExactTime, formatRelativeTime, fileViewerKind, fileViewerNeedsText, formatFileSize } from "@abstractframework/ui-kit";
import React, { useEffect, useMemo, useState } from "react";
import { Icon, type IconName } from "@abstractframework/ui-kit";
import {
  FileViewer,
  JsonViewer,
  ToolActivityGroup,
  PREVIEW_TEXT_LIMIT,
  partialPreviewNote,
  workflowEvidence,
  type WorkflowRecord,
} from "@abstractframework/panel-chat";
import { downloadArtifact, formatError, gateway, gatewayRequest } from "./transport";
import type { WorkspacePolicy } from "./catalog";
import { SessionFiles } from "./session_files";
import type { PendingUpload } from "./attachment_uploads";
import {
  activity_rows,
  type ActivityKind,
  type ActivityRow,
} from "../lib/activity_rows";
import { activity_groups, default_open_group, type ActivityGroup } from "../lib/activity_groups";

type FileRow = {
  path: string;
  name?: string;
  kind?: string;
  is_dir?: boolean;
  type?: string;
  size_bytes?: number;
};

/** File browsing and generated output for the rail's Files panel. */
export type WorkspaceFilesContentProps = {
  policy: WorkspacePolicy | null;
  runId: string;
  enabled: boolean;
  isAdmin: boolean;
  refreshKey?: string;
  onAttach: (path: string) => Promise<void>;
  onAttachFiles: (files: File[]) => PendingUpload[];
  includeArtifacts?: boolean;
};

export function WorkspaceFilesContent({
  policy, runId, enabled, isAdmin, refreshKey, onAttach, onAttachFiles,
  includeArtifacts = true,
}: WorkspaceFilesContentProps) {
  const [filesMode, setFilesMode] = useState<"session" | "shared">("session");
  return <>
    {isAdmin ? (
      <div className="code-files-mode" role="group" aria-label="Files source">
        <button aria-pressed={filesMode === "session"} onClick={() => setFilesMode("session")}>
          This conversation
        </button>
        <button aria-pressed={filesMode === "shared"} onClick={() => setFilesMode("shared")}>
          Shared workspace (admin)
        </button>
      </div>
    ) : null}
    {isAdmin ? <div hidden={filesMode !== "shared"}>
      <SharedWorkspaceBrowser
        policy={policy}
        enabled={enabled && filesMode === "shared"}
        runId={runId}
        onAttach={onAttach}
      />
    </div> : null}
    <div hidden={isAdmin && filesMode !== "session"}>
      <SessionFiles
        runId={runId}
        enabled={enabled && (!isAdmin || filesMode === "session")}
        refreshKey={refreshKey}
        maxAttachmentBytes={policy?.maxAttachmentBytes}
        onAttachFiles={onAttachFiles}
      />
    </div>
    {includeArtifacts ? <section className="code-rail-section" aria-labelledby="code-artifacts-title">
      <h3 id="code-artifacts-title" className="code-rail-section-title">Generated outputs & attachments</h3>
      <Artifacts runId={runId} enabled={enabled} refreshKey={refreshKey} />
    </section> : null}
  </>;
}

/** The conversation's activity, one foldable group per iteration (newest open). */
export function WorkspaceActivityContent({ records, runId }: {
  records: WorkflowRecord[];
  runId: string;
}) {
  // Rows, not records: the three records of one step are one row, and
  // progress/status records are progress UI rather than activity.
  const rows = useMemo(() => activity_rows(records, runId), [records, runId]);
  if (!rows.length)
    return (
      <div className="code-pane-empty">
        <Icon name="activity" size={28} />
        <p>Follow the work as it happens.</p>
        <small>Model steps, tool calls and approvals appear here, one group per step.</small>
      </div>
    );
  return <ActivityGroups rows={rows} />;
}

/** The rail badge: rows still running or waiting for an answer. */
export function activityAttention(records: WorkflowRecord[], runId: string): { count: number; hint: string } {
  const rows = activity_rows(records, runId);
  const waiting = rows.filter((row) => row.status === "waiting").length;
  const running = rows.filter((row) => row.status === "running").length;
  const parts = [waiting ? `${waiting} waiting` : "", running ? `${running} running` : ""].filter(Boolean);
  return { count: waiting + running, hint: parts.join(" · ") };
}

export function ActivityGroups({ rows }: { rows: ActivityRow[] }) {
  const groups = useMemo(() => activity_groups(rows), [rows]);
  const newest = default_open_group(groups);
  // Folding is the reader's: a group they opened or closed stays that way;
  // the newest group is open until they close it.
  const [toggled, setToggled] = useState<Record<string, boolean>>({});
  return (
    <div className="code-activity-groups">
      {groups.map((group) => {
        const open = toggled[group.key] ?? group.key === newest;
        return (
          <details
            key={group.key}
            className={`code-activity-group is-${group.status}`}
            data-group={group.title}
            open={open}
            onToggle={(event) => {
              const next = (event.currentTarget as HTMLDetailsElement).open;
              if (next !== open) setToggled((prev) => ({ ...prev, [group.key]: next }));
            }}
          >
            <summary>
              <Icon name="chevronRight" size={14} className="code-activity-group-chevron" />
              <strong>{group.title}</strong>
              {group.detail ? <span className="code-activity-group-detail" title={group.detail}>{group.detail}</span> : <span className="code-activity-group-detail" />}
              <span className={`code-step-dot is-${group.status}`} aria-hidden="true" />
              <small>{group.statusLabel}</small>
            </summary>
            {open ? <ActivityGroupBody group={group} /> : null}
          </details>
        );
      })}
    </div>
  );
}

/** A group's content, rendered like the transcript: tool calls as the chat's tool cards, other steps as rows. */
function ActivityGroupBody({ group }: { group: ActivityGroup }) {
  const records = useMemo(
    () => group.rows.flatMap((row) => row.entries.map((entry) => ({ cursor: entry.cursor, record: entry.record, runId: entry.runId }) as WorkflowRecord)),
    [group],
  );
  const tools = useMemo(() => workflowEvidence(records).tools, [records]);
  const others = group.rows.filter((row) => row.kind !== "tools");
  return (
    <div className="code-activity-group-body">
      {others.map((row) => <ActivityRowView key={row.key} row={row} />)}
      {tools.length ? <ToolActivityGroup tools={tools} /> : null}
    </div>
  );
}

const ACTIVITY_ICONS: Record<ActivityKind, IconName> = {
  llm: "sparkle",
  tools: "terminal",
  subflow: "agent",
  event: "info",
  wait: "history",
  ask: "chat",
  message: "send",
  run: "playCircle",
  progress: "loader",
  step: "list",
};

function ActivityRowView({ row }: { row: ActivityRow }) {
  const merged = row.merged as Record<string, any>;
  const effect = (merged.effect || {}) as Record<string, any>;
  return (
    <details className={`code-activity-row code-activity-row--${row.kind}`}>
      <summary>
        <span className="code-activity-icon" aria-hidden="true">
          <Icon name={ACTIVITY_ICONS[row.kind]} size={14} />
        </span>
        <span className="code-activity-text">
          <span className="code-activity-title">{row.title}</span>
          {row.detail ? <span className="code-activity-sub">{row.detail}</span> : null}
          {row.progress ? <span className="code-activity-progress">{row.progress}</span> : null}
        </span>
        <span className={`code-step-dot is-${row.status}`} />
        <small>{row.statusLabel}</small>
      </summary>
      <div className="code-activity-detail">
        <div className="code-field-help">
          {row.nodeId ? `${row.nodeId} · ` : ""}run {row.runId.slice(0, 8)} · {row.entries.length}{" "}
          {row.entries.length === 1 ? "record" : "records"}
          {row.progressEvents.length ? ` · ${row.progressEvents.length} progress` : ""}
        </div>
        <JsonViewer
          value={{
            input: effect.payload,
            result: merged.result,
            error: merged.error,
            records: row.entries.map((entry) => entry.record),
            ...(row.progressEvents.length ? { progress: row.progressEvents } : {}),
          }}
          collapseAfterDepth={2}
        />
      </div>
    </details>
  );
}

export type OccurrenceSummary = {
  run_id: string;
  index: number;
  status: string;
  fired_at?: string;
  finished_at?: string;
  failure?: { message: string };
};

/** Read one run's ledger (all pages, bounded) as workflow records. */
export async function loadRunRecords(runId: string, signal?: AbortSignal, maxItems = 2000): Promise<WorkflowRecord[]> {
  const out: WorkflowRecord[] = [];
  let after = 0;
  while (out.length < maxItems) {
    if (signal?.aborted) throw new DOMException("aborted", "AbortError");
    const page = await gateway.get_ledger(runId, { after, limit: 500 });
    const items = Array.isArray(page.items) ? page.items : [];
    for (const item of items) {
      const cursor = Number(item?.cursor) || after + out.length + 1;
      out.push({ cursor, record: item?.record ?? item, runId: String(item?.record?.run_id || runId) });
    }
    if (!items.length || !page.next_after || page.next_after <= after) break;
    after = page.next_after;
  }
  return out;
}

/** An automation's activity: one group per occurrence (run), the latest open; a group loads its run's ledger when opened. */
export function AutomationActivity({ occurrences, nowMs, loadRecords = loadRunRecords, onOpenRun }: {
  occurrences: OccurrenceSummary[];
  nowMs: number;
  loadRecords?: (runId: string, signal?: AbortSignal) => Promise<WorkflowRecord[]>;
  onOpenRun?: (runId: string) => void;
}) {
  const sorted = useMemo(() => [...occurrences].sort((a, b) => b.index - a.index), [occurrences]);
  const latest = sorted[0]?.run_id || "";
  const [toggled, setToggled] = useState<Record<string, boolean>>({});
  if (!sorted.length)
    return (
      <div className="code-pane-empty">
        <Icon name="activity" size={28} />
        <p>No runs yet.</p>
        <small>Each run of this automation appears here as one group.</small>
      </div>
    );
  return (
    <div className="code-activity-groups" data-scope="automation">
      {sorted.map((occ) => {
        const open = toggled[occ.run_id] ?? occ.run_id === latest;
        const when = occ.finished_at || occ.fired_at;
        return (
          <details
            key={occ.run_id}
            className={`code-activity-group is-${occ.status}`}
            data-group={`Run #${occ.index}`}
            open={open}
            onToggle={(event) => {
              const next = (event.currentTarget as HTMLDetailsElement).open;
              if (next !== open) setToggled((prev) => ({ ...prev, [occ.run_id]: next }));
            }}
          >
            <summary>
              <Icon name="chevronRight" size={14} className="code-activity-group-chevron" />
              <strong>Run #{occ.index}</strong>
              <span className="code-activity-group-detail">
                {when ? <time title={formatExactTime(when)}>{formatRelativeTime(when, nowMs)}</time> : null}
              </span>
              <span className={`code-step-dot is-${occ.status}`} aria-hidden="true" />
              <small>{occ.status}</small>
            </summary>
            {open ? <OccurrenceBody occurrence={occ} loadRecords={loadRecords} onOpenRun={onOpenRun} /> : null}
          </details>
        );
      })}
    </div>
  );
}

function OccurrenceBody({ occurrence, loadRecords, onOpenRun }: {
  occurrence: OccurrenceSummary;
  loadRecords: (runId: string, signal?: AbortSignal) => Promise<WorkflowRecord[]>;
  onOpenRun?: (runId: string) => void;
}) {
  const [state, setState] = useState<{ status: "loading" } | { status: "error"; message: string } | { status: "ready"; records: WorkflowRecord[] }>({ status: "loading" });
  useEffect(() => {
    const abort = new AbortController();
    setState({ status: "loading" });
    loadRecords(occurrence.run_id, abort.signal)
      .then((records) => !abort.signal.aborted && setState({ status: "ready", records }))
      .catch((e) => !abort.signal.aborted && setState({ status: "error", message: `Run activity unavailable: ${formatError(e)}` }));
    return () => abort.abort();
  }, [occurrence.run_id, occurrence.status, loadRecords]);
  const rows = useMemo(() => (state.status === "ready" ? activity_rows(state.records, occurrence.run_id) : []), [state, occurrence.run_id]);
  return (
    <div className="code-activity-group-body">
      {occurrence.failure?.message ? <p className="code-error-text" role="alert">{occurrence.failure.message}</p> : null}
      {state.status === "loading" ? <p className="code-muted" role="status">Loading run activity…</p> : null}
      {state.status === "error" ? <p className="code-error-text" role="alert">{state.message}</p> : null}
      {state.status === "ready" ? (rows.length ? <ActivityGroups rows={rows} /> : <p className="code-muted">This run recorded no steps.</p>) : null}
      {onOpenRun ? (
        <button type="button" className="code-subtle-button" onClick={() => onOpenRun(occurrence.run_id)}>
          <Icon name="chat" size={13} /> <span>Open as conversation</span>
        </button>
      ) : null}
    </div>
  );
}

/** The gateway operator's shared workspace root (`/files/list|search`,
 * admin-only). Clicking a file attaches it to the next message. */
function SharedWorkspaceBrowser({
  policy,
  enabled,
  runId,
  onAttach,
}: {
  policy: WorkspacePolicy | null;
  enabled: boolean;
  runId: string;
  onAttach: (path: string) => Promise<void>;
}) {
  const [query, setQuery] = useState("");
  const [directory, setDirectory] = useState("");
  const [files, setFiles] = useState<FileRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [revision, setRevision] = useState(0);
  const [attaching, setAttaching] = useState("");
  useEffect(() => {
    const abort = new AbortController();
    setFiles([]);
    setError("");
    setNotice("");
    if (!enabled || !policy) return;
    setLoading(true);
    const timer = window.setTimeout(
      () => {
        const params = new URLSearchParams({ limit: "100" });
        if (query.trim()) params.set("query", query.trim());
        else if (directory) params.set("path", directory);
        void gatewayRequest(
          gatewayApiPath(`files/${query.trim() ? "search" : "list"}?${params}`),
          { signal: abort.signal },
        )
          .then((data) => {
            if (data.error) throw new Error(String(data.error));
            setFiles(Array.isArray(data.items) ? data.items : []);
            if (data.truncated || data.has_more)
              setNotice("Showing the first 100 results. Search to narrow the list.");
          })
          .catch((e) => {
            if (!abort.signal.aborted) setError(formatError(e));
          })
          .finally(() => {
            if (!abort.signal.aborted) setLoading(false);
          });
      },
      query ? 220 : 0,
    );
    return () => {
      window.clearTimeout(timer);
      abort.abort();
    };
  }, [enabled, policy, query, directory, revision, runId]);
  return (
    <>
      <div className="code-pane-intro">
        <Icon name="terminal" size={17} />
        <div>
          <strong>Shared workspace (admin)</strong>
          <span>{directory || "Authorized shared workspace"}</span>
        </div>
        <button className="code-icon-button" aria-label="Refresh files" onClick={() => setRevision((n) => n + 1)} disabled={!enabled}>
          <Icon name="refresh" size={14} />
        </button>
      </div>
      <input className="code-search" aria-label="Search workspace files" placeholder="Search files…" value={query} onChange={(e) => setQuery(e.target.value)} disabled={!enabled} />
      {directory ? (
        <button className="code-back" onClick={() => { setDirectory(directory.split("/").slice(0, -1).join("/")); setQuery(""); }}>
          ← Parent folder
        </button>
      ) : null}
      {!enabled ? <p className="code-pane-empty">Connect to browse your workspace.</p> : null}
      {loading ? <p className="code-muted" role="status">Loading files…</p> : null}
      {error ? (
        <div className="code-inline-error" role="alert">
          {error}
          <button onClick={() => setRevision((n) => n + 1)}>Retry</button>
        </div>
      ) : null}
      {notice ? <p className="code-field-help" role="status">{notice}</p> : null}
      <ul className="code-file-list">
        {files.map((file) => {
          const isDirectory = file.kind === "folder" || file.is_dir || file.type === "directory" || file.type === "dir";
          return (
            <li key={file.path}>
              <button
                title={isDirectory ? `Show ${file.path}` : `Attach ${file.path} to the conversation`}
                disabled={Boolean(attaching)}
                onClick={async () => {
                  if (isDirectory) {
                    setDirectory(file.path);
                    setQuery("");
                    return;
                  }
                  setAttaching(file.path);
                  setError("");
                  try {
                    await onAttach(file.path);
                    setNotice(`${file.name || file.path} attached.`);
                  } catch (e) {
                    setError(formatError(e));
                  } finally {
                    setAttaching("");
                  }
                }}
              >
                <Icon name={isDirectory ? "folder" : "paperclip"} size={14} />
                <span>{file.name || file.path}</span>
                {attaching === file.path ? <Icon name="loader" size={13} /> : <Icon name={isDirectory ? "chevronRight" : "plus"} size={13} className="code-file-action" />}
              </button>
            </li>
          );
        })}
      </ul>
      {!loading && !error && enabled && !files.length ? (
        <div className="code-pane-empty">
          <Icon name="terminal" size={28} />
          <p>{query ? "No matching files." : "No shared files yet."}</p>
          <small>{query ? "Try a shorter name or another folder." : "Browse files exposed by your gateway or attach files from your device."}</small>
        </div>
      ) : null}
    </>
  );
}

type ArtifactItem = { id: string; name: string; contentType: string; size?: number; created?: string };

/** "PNG image", "Text file", "PDF document"… — the name of an artifact that has no filename. */
export function artifactTypeLabel(contentType: string): string {
  const type = String(contentType || "").split(";", 1)[0].trim().toLowerCase();
  const sub = (type.split("/")[1] || "").replace(/^x-/, "").split("+")[0];
  if (type === "application/pdf") return "PDF document";
  if (type === "application/json" || type.endsWith("+json")) return "JSON file";
  if (type.startsWith("image/")) return `${sub.toUpperCase()} image`;
  if (type.startsWith("audio/")) return `${sub.toUpperCase()} audio`;
  if (type.startsWith("video/")) return `${sub.toUpperCase()} video`;
  if (type === "text/markdown") return "Markdown file";
  if (type.startsWith("text/")) return "Text file";
  return "File";
}

function artifactItem(item: any): ArtifactItem {
  const id = String(item.artifact_id || item.id || "");
  const contentType = String(item.content_type || item.mime_type || "");
  return {
    id,
    name: String(item.filename || item.metadata?.filename || item.name || artifactTypeLabel(contentType)),
    contentType,
    ...(typeof item.size_bytes === "number" ? { size: item.size_bytes } : typeof item.size === "number" ? { size: item.size } : {}),
    ...(item.created_at || item.timestamp ? { created: String(item.created_at || item.timestamp) } : {}),
  };
}

/**
 * How an artifact is previewed in the kit viewer: its text (markdown, code, JSON — the kit's
 * AfCodeBlock — and plain text), an object URL (image, PDF, and audio in the kit waveform player),
 * or nothing (a binary: download only).
 */
export function artifactPreviewMode(kind: ReturnType<typeof fileViewerKind>): "text" | "url" | "none" {
  if (fileViewerNeedsText(kind)) return "text";
  return kind === "image" || kind === "pdf" || kind === "audio" ? "url" : "none";
}

type ArtifactPreview =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; text?: string; url?: string; partial: boolean; total?: number; contentType: string };

/** Run artifacts (generated output, attachments): a row per file — name, size, date, download; a click previews it. */
function Artifacts({ runId, enabled, refreshKey }: { runId: string; enabled: boolean; refreshKey?: string }) {
  const [items, setItems] = useState<ArtifactItem[]>([]);
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  const [selected, setSelected] = useState<ArtifactItem | null>(null);
  const [preview, setPreview] = useState<ArtifactPreview>({ status: "loading" });
  const nowMs = Date.now();
  useEffect(() => {
    setItems([]);
    setError("");
    setSelected(null);
    if (!runId || !enabled) return;
    const abort = new AbortController();
    void gatewayRequest(gatewayApiPath(`runs/${encodeURIComponent(runId)}/artifacts?limit=200`), { signal: abort.signal })
      .then((data) => setItems((Array.isArray(data.items) ? data.items : Array.isArray(data.artifacts) ? data.artifacts : []).map(artifactItem)))
      .catch((e) => {
        if (!abort.signal.aborted) setError(formatError(e));
      });
    return () => abort.abort();
  }, [runId, enabled, revision, refreshKey]);
  useEffect(() => {
    if (!selected) return;
    let alive = true;
    let url = "";
    setPreview({ status: "loading" });
    void gateway
      .get_run_artifact_blob(runId, selected.id)
      .then(async ({ blob, content_type }) => {
        const mode = artifactPreviewMode(fileViewerKind(selected.name, content_type));
        if (mode === "text") {
          const text = await blob.slice(0, PREVIEW_TEXT_LIMIT).text();
          if (alive) setPreview({ status: "ready", text, partial: blob.size > PREVIEW_TEXT_LIMIT, total: blob.size, contentType: content_type });
        } else if (mode === "url") {
          url = URL.createObjectURL(blob.type ? blob : new Blob([blob], { type: content_type }));
          if (alive) setPreview({ status: "ready", url, partial: false, contentType: content_type });
        } else if (alive) setPreview({ status: "ready", partial: false, contentType: content_type });
      })
      .catch((e) => alive && setPreview({ status: "error", message: `Preview failed: ${formatError(e)}` }));
    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [runId, selected]);
  const download = (item: ArtifactItem) => void downloadArtifact(runId, item.id, item.name).catch((e) => setError(formatError(e)));
  return (
    <>
      <div className="code-rail-toolbar">
        <button className="code-icon-button" aria-label="Refresh outputs" title="Refresh" onClick={() => setRevision((n) => n + 1)}>
          <Icon name="refresh" size={14} />
        </button>
      </div>
      {error ? <p role="alert" className="code-error-text">{error}</p> : null}
      {selected ? (
        <FileViewer
          name={selected.name}
          sizeBytes={selected.size}
          modified={selected.created}
          nowMs={nowMs}
          contentType={preview.status === "ready" ? preview.contentType : selected.contentType}
          status={preview.status}
          {...(preview.status === "error" ? { error: preview.message } : {})}
          {...(preview.status === "ready" ? { text: preview.text, url: preview.url, ...(preview.partial ? { partialNote: partialPreviewNote(preview.total) } : {}) } : {})}
          onDownload={() => download(selected)}
          onClose={() => setSelected(null)}
        />
      ) : items.length ? (
        <ul className="code-file-rows">
          {items.map((item) => (
            <li key={item.id} className="code-file-row">
              <button className="code-file-row-name" title={`Preview ${item.name} · ${item.id}`} onClick={() => setSelected(item)}>
                <Icon name="file" size={14} />
                <span>{item.name}</span>
              </button>
              <span className="code-file-row-meta">
                {item.size !== undefined ? <span>{formatFileSize(item.size)}</span> : null}
                {item.created ? <time title={formatExactTime(item.created)}>{formatRelativeTime(item.created, nowMs)}</time> : null}
              </span>
              <button className="code-icon-button" aria-label={`Download ${item.name}`} title="Download" onClick={() => download(item)}>
                <Icon name="download" size={14} />
              </button>
            </li>
          ))}
        </ul>
      ) : !error ? (
        <p className="code-muted">Generated files and attachments appear here.</p>
      ) : null}
    </>
  );
}
