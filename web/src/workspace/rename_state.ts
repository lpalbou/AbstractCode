// Backlog 1343: the host side of renaming a conversation from its sidebar card. The card says when its
// editor opens and closes (onRenameEditingChange); this keeps which rows are being renamed (so a search
// never hides one while its editor is open or its save is pending) and the refusal shown under each.
import { useCallback, useEffect, useRef, useState } from "react";
import { formatError } from "./transport";

/**
 * `store` saves a name (the catalog's renameSession: optimistic, reverted on refusal, rejecting with
 * the refusal). Each row has its own error: shown while its editor is open, cleared when that editor
 * closes, when a later rename of it succeeds and when the signed-in identity changes. A refusal of an
 * older attempt, or one that lands after the row's editor closed, is never shown.
 */
export function useConversationRename(identity: string, store: (sessionId: string, name: string) => Promise<void>) {
  // The rows whose editor is open (or whose save is pending), and each row's refusal.
  const [editing, setEditing] = useState<ReadonlySet<string>>(() => new Set());
  const [errors, setErrors] = useState<Readonly<Record<string, string>>>({});
  // The latest attempt per conversation while its editor is open; numbers never repeat.
  const latest = useRef<Record<string, number>>({});
  const counter = useRef(0);
  useEffect(() => {
    latest.current = {};
    setEditing(new Set());
    setErrors({});
  }, [identity]);
  const clear = (sessionId: string) =>
    setErrors((cur) => {
      if (!(sessionId in cur)) return cur;
      const { [sessionId]: _gone, ...rest } = cur;
      return rest;
    });
  const rename = useCallback(
    async (sessionId: string, name: string) => {
      const attempt = ++counter.current;
      latest.current[sessionId] = attempt;
      clear(sessionId);
      try {
        await store(sessionId, name);
      } catch (e) {
        if (latest.current[sessionId] === attempt) setErrors((cur) => ({ ...cur, [sessionId]: `Not renamed: ${formatError(e)}` }));
        throw e;
      }
      if (latest.current[sessionId] === attempt) clear(sessionId);
    },
    [store],
  );
  const editingChange = useCallback((sessionId: string, open: boolean) => {
    if (open) {
      setEditing((cur) => (cur.has(sessionId) ? cur : new Set(cur).add(sessionId)));
      return;
    }
    // Closed (or gone with its row): a refusal arriving later has nowhere to be read, so it is dropped.
    delete latest.current[sessionId];
    setEditing((cur) => {
      if (!cur.has(sessionId)) return cur;
      const next = new Set(cur);
      next.delete(sessionId);
      return next;
    });
    clear(sessionId);
  }, []);
  return { editing, errors, rename, editingChange };
}
