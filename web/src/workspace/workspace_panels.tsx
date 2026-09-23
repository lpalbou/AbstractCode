import React, { useEffect, useMemo, useState } from "react";
import { Icon, type IconName } from "@abstractframework/ui-kit";
import { JsonViewer, type WorkflowRecord } from "@abstractframework/panel-chat";
import { downloadArtifact, formatError, gatewayRequest } from "./transport";
import type { WorkspacePolicy } from "./catalog";
import { navigateTabs } from "./tabs";
import {
  activity_rows,
  type ActivityKind,
  type ActivityRow,
} from "../lib/activity_rows";

export type InspectorTab = "files" | "activity" | "artifacts";
type FileRow = {
  path: string;
  name?: string;
  kind?: string;
  is_dir?: boolean;
  type?: string;
  size_bytes?: number;
};

export function WorkspaceInspector({
  tab,
  onTab,
  policy,
  runId,
  records,
  enabled,
  onAttach,
  onClose,
}: {
  tab: InspectorTab;
  onTab: (tab: InspectorTab) => void;
  policy: WorkspacePolicy | null;
  runId: string;
  records: WorkflowRecord[];
  enabled: boolean;
  onAttach: (path: string) => Promise<void>;
  onClose: () => void;
}) {
  // Rows, not records: the three records of one step are one row, and the
  // `abstract.progress` / `abstract.status` records are progress UI rather than
  // activity. The badge counts what the operator can actually read — a turn
  // that wrote 156 records has ten things in it.
  const rows = useMemo(() => activity_rows(records), [records]);
  return (
    <aside className="code-inspector" aria-label="Workspace inspector">
      <div className="code-inspector-heading">
        <span>WORKSPACE</span>
        <button
          className="code-icon-button"
          aria-label="Close workspace inspector"
          onClick={onClose}
        >
          <Icon name="x" size={15} />
        </button>
      </div>
      <div
        className="code-inspector-tabs"
        role="tablist"
        aria-label="Workspace views"
        onKeyDown={navigateTabs}
      >
        {(["files", "activity", "artifacts"] as const).map((name) => (
          <button
            id={`inspector-tab-${name}`}
            role="tab"
            tabIndex={tab === name ? 0 : -1}
            aria-selected={tab === name}
            aria-controls={`inspector-panel-${name}`}
            key={name}
            onClick={() => onTab(name)}
          >
            {name === "files"
              ? "Files"
              : name === "activity"
                ? "Activity"
                : "Artifacts"}
            {name === "activity" && rows.length ? (
              <span>{rows.length}</span>
            ) : null}
          </button>
        ))}
      </div>
      <div
        className="code-inspector-content"
        id={`inspector-panel-${tab}`}
        role="tabpanel"
        aria-labelledby={`inspector-tab-${tab}`}
      >
        {tab === "files" ? (
          <FileBrowser
            policy={policy}
            enabled={enabled}
            runId={runId}
            onAttach={onAttach}
          />
        ) : tab === "activity" ? (
          <Activity rows={rows} />
        ) : (
          <Artifacts runId={runId} enabled={enabled} />
        )}
      </div>
      <div className="code-inspector-foot">
        <Icon name="server" size={14} />
        <span>Access enforced by your gateway</span>
      </div>
    </aside>
  );
}

function FileBrowser({
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
          `/api/gateway/files/${query.trim() ? "search" : "list"}?${params}`,
          { signal: abort.signal },
        )
          .then((data) => {
            if (data.error) throw new Error(String(data.error));
            setFiles(Array.isArray(data.items) ? data.items : []);
            if (data.truncated || data.has_more)
              setNotice(
                "Showing the first 100 results. Search to narrow the list.",
              );
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
          <strong>Gateway files</strong>
          <span>{directory || "Authorized shared workspace"}</span>
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
      <input
        className="code-search"
        aria-label="Search workspace files"
        placeholder="Search files…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        disabled={!enabled}
      />
      {directory ? (
        <button
          className="code-back"
          onClick={() => {
            setDirectory(directory.split("/").slice(0, -1).join("/"));
            setQuery("");
          }}
        >
          ← Parent folder
        </button>
      ) : null}
      {!enabled ? (
        <p className="code-pane-empty">Connect to browse your workspace.</p>
      ) : null}
      {loading ? (
        <p className="code-muted" role="status">
          Loading files…
        </p>
      ) : null}
      {error ? (
        <div className="code-inline-error" role="alert">
          {error}
          <button onClick={() => setRevision((n) => n + 1)}>Retry</button>
        </div>
      ) : null}
      {notice ? (
        <p className="code-field-help" role="status">
          {notice}
        </p>
      ) : null}
      <ul className="code-file-list">
        {files.map((file) => {
          const isDirectory =
            file.kind === "folder" ||
            file.is_dir ||
            file.type === "directory" ||
            file.type === "dir";
          return (
            <li key={file.path}>
              <button
                title={
                  isDirectory
                    ? `Open ${file.path}`
                    : `Attach ${file.path} to the conversation`
                }
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
                <Icon name={isDirectory ? "chevronRight" : "edit"} size={14} />
                <span>{file.name || file.path}</span>
                {attaching === file.path ? (
                  <Icon name="loader" size={13} />
                ) : (
                  <span className="code-file-action">
                    {isDirectory ? "Open" : "+"}
                  </span>
                )}
              </button>
            </li>
          );
        })}
      </ul>
      {!loading && !error && enabled && !files.length ? (
        <div className="code-pane-empty">
          <Icon name="terminal" size={28} />
          <p>{query ? "No matching files." : "No shared files yet."}</p>
          <small>
            {query
              ? "Try a shorter name or another folder."
              : "Browse files exposed by your gateway or attach files from your device. Generated output is available in Artifacts."}
          </small>
        </div>
      ) : null}
    </>
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

function Activity({ rows }: { rows: ActivityRow[] }) {
  if (!rows.length)
    return (
      <div className="code-pane-empty">
        <Icon name="playCircle" size={28} />
        <p>Follow the work as it happens.</p>
        <small>Workflow steps, tool calls, and results will appear here.</small>
      </div>
    );
  return (
    <div className="code-activity">
      {rows.map((row) => {
        const merged = row.merged as Record<string, any>;
        const effect = (merged.effect || {}) as Record<string, any>;
        return (
          <details
            className={`code-activity-row code-activity-row--${row.kind}`}
            key={row.key}
          >
            <summary>
              <span className="code-activity-icon" aria-hidden="true">
                <Icon name={ACTIVITY_ICONS[row.kind]} size={14} />
              </span>
              <span className="code-activity-text">
                <span className="code-activity-title">{row.title}</span>
                {row.detail ? (
                  <span className="code-activity-sub">{row.detail}</span>
                ) : null}
                {row.progress ? (
                  <span className="code-activity-progress">{row.progress}</span>
                ) : null}
              </span>
              <span className={`code-step-dot is-${row.status}`} />
              <small>{row.statusLabel}</small>
            </summary>
            <div className="code-activity-detail">
              <div className="code-field-help">
                {row.nodeId ? `${row.nodeId} · ` : ""}run {row.runId.slice(0, 8)}{" "}
                · {row.entries.length}{" "}
                {row.entries.length === 1 ? "record" : "records"}
                {row.progressEvents.length
                  ? ` · ${row.progressEvents.length} progress`
                  : ""}
              </div>
              <JsonViewer
                value={{
                  input: effect.payload,
                  result: merged.result,
                  error: merged.error,
                  records: row.entries.map((entry) => entry.record),
                  ...(row.progressEvents.length
                    ? { progress: row.progressEvents }
                    : {}),
                }}
                collapseAfterDepth={2}
              />
            </div>
          </details>
        );
      })}
    </div>
  );
}

function Artifacts({ runId, enabled }: { runId: string; enabled: boolean }) {
  const [items, setItems] = useState<any[]>([]);
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    setItems([]);
    setError("");
    if (!runId || !enabled) return;
    const abort = new AbortController();
    void gatewayRequest(
      `/api/gateway/runs/${encodeURIComponent(runId)}/artifacts?limit=200`,
      { signal: abort.signal },
    )
      .then((data) =>
        setItems(
          Array.isArray(data.items)
            ? data.items
            : Array.isArray(data.artifacts)
              ? data.artifacts
              : [],
        ),
      )
      .catch((e) => {
        if (!abort.signal.aborted) setError(formatError(e));
      });
    return () => abort.abort();
  }, [runId, enabled, revision]);
  return (
    <>
      <div className="code-pane-intro">
        <Icon name="paperclip" size={17} />
        <div>
          <strong>Run artifacts</strong>
          <span>Files and generated output</span>
        </div>
        <button
          className="code-icon-button"
          aria-label="Refresh artifacts"
          onClick={() => setRevision((n) => n + 1)}
        >
          <Icon name="refresh" size={14} />
        </button>
      </div>
      {error ? (
        <p role="alert" className="code-error-text">
          {error}
        </p>
      ) : null}
      {items.map((item) => {
        const id = item.artifact_id || item.id;
        const name = item.filename || item.metadata?.filename || id;
        return (
          <button
            key={id}
            className="code-artifact"
            onClick={() => {
              void downloadArtifact(runId, id, name).catch((e) =>
                setError(formatError(e)),
              );
            }}
          >
            <Icon name="download" size={16} />
            <span>
              <strong>{name}</strong>
              <small>{item.content_type || "Artifact"}</small>
            </span>
          </button>
        );
      })}
      {!items.length && !error ? (
        <div className="code-pane-empty">
          <Icon name="paperclip" size={28} />
          <p>Everything the workflow creates.</p>
          <small>
            Generated files and attachments are stored by the gateway and can be
            downloaded here.
          </small>
        </div>
      ) : null}
    </>
  );
}
