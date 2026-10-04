// The archived conversations under the Conversations list's `Archived · N` footer (round 5).
// The gateway owns archiving (contract assigned 2026-10-04 to the gateway seat):
//   - `GET /runs?root_only=true` leaves archived sessions out and carries `archived_sessions: N`;
//   - `GET /runs?root_only=true&archived_only=true` lists them;
//   - `POST /sessions/{session_id}/unarchive` brings one back (history is kept either way).
// This file only reads and forwards; a refusal is shown as the gateway said it.
import React, { useEffect, useState } from "react";
import { gatewayApiPath } from "@abstractframework/ui-kit";
import { normalizeSessionSummaries, type SessionSummary } from "./catalog";
import { formatError, gatewayRequest } from "./transport";
import { ArchivedRow } from "./sidebar_panels";
import { conversationMetaLine, conversationTitle } from "./sidebar_cards";
import { LoadingStatus } from "./loading_status";

export const ARCHIVED_RUNS_PATH = "runs?root_only=true&archived_only=true&include_ledger_len=false&include_metrics=true&limit=200";
export const unarchiveSessionPath = (sessionId: string) => `sessions/${encodeURIComponent(sessionId)}/unarchive`;

/** `archived_sessions` of the runs list; a list without it has no archived sessions to show. */
export function archivedSessionCount(runs: unknown): number {
  const n = (runs as { archived_sessions?: unknown } | null)?.archived_sessions;
  return typeof n === "number" && Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
}

export function ArchivedConversations(props: {
  /** Fetch only while the footer is open and connected. */
  enabled: boolean;
  /** Changes when the gateway's count changes (refetch). */
  revision: number;
  onOpen(item: SessionSummary): void;
  onUnarchived(item: SessionSummary): void;
  request?: typeof gatewayRequest;
}): React.ReactElement {
  const request = props.request ?? gatewayRequest;
  const [items, setItems] = useState<SessionSummary[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState("");
  const [rowError, setRowError] = useState<{ id: string; message: string } | null>(null);
  useEffect(() => {
    if (!props.enabled) return;
    const abort = new AbortController();
    setError("");
    void request(gatewayApiPath(ARCHIVED_RUNS_PATH), { signal: abort.signal })
      .then((runs) => !abort.signal.aborted && setItems(normalizeSessionSummaries(runs)))
      .catch((e) => !abort.signal.aborted && setError(`Archived conversations unavailable: ${formatError(e)}`));
    return () => abort.abort();
  }, [props.enabled, props.revision]); // eslint-disable-line react-hooks/exhaustive-deps
  if (error) return <li className="code-inline-error" role="alert">{error}</li>;
  if (!items) return <li><LoadingStatus>Loading archived conversations…</LoadingStatus></li>;
  return (
    <>
      {items.map((item) => (
        <ArchivedRow
          key={item.sessionId}
          id={item.sessionId}
          title={conversationTitle(item)}
          meta={conversationMetaLine(item)}
          busy={busy === item.sessionId}
          error={rowError?.id === item.sessionId ? rowError.message : undefined}
          onOpen={() => props.onOpen(item)}
          onUnarchive={() => {
            setBusy(item.sessionId);
            setRowError(null);
            void request(gatewayApiPath(unarchiveSessionPath(item.sessionId)), { method: "POST", body: "{}" })
              .then(() => {
                setItems((prev) => (prev || []).filter((x) => x.sessionId !== item.sessionId));
                props.onUnarchived(item);
              })
              .catch((e) => setRowError({ id: item.sessionId, message: `Not unarchived: ${formatError(e)}` }))
              .finally(() => setBusy(""));
          }}
        />
      ))}
    </>
  );
}
