// Backlog 1343: the host side of renaming a conversation from its sidebar card. The card says when its
// editor opens and closes (onRenameEditingChange); this keeps which row is being renamed (so a search
// never hides it while its editor is open or its save is pending) and the refusal shown under it.
import { useCallback, useEffect, useRef, useState } from "react";
import { formatError } from "./transport";

export type RenameError = { sessionId: string; message: string };

/**
 * `store` saves a name (the catalog's renameSession: optimistic, reverted on refusal, rejecting with
 * the refusal). The error under a row is cleared when its editor closes, when a later rename of it
 * succeeds and when the signed-in identity changes; a late refusal of an older attempt never shows
 * over a newer one.
 */
export function useConversationRename(identity: string, store: (sessionId: string, name: string) => Promise<void>) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [error, setError] = useState<RenameError | null>(null);
  // The latest attempt per conversation; numbers never repeat, even across identities.
  const latest = useRef<Record<string, number>>({});
  const counter = useRef(0);
  useEffect(() => {
    latest.current = {};
    setEditingId(null);
    setError(null);
  }, [identity]);
  const clear = (sessionId: string) => setError((cur) => (cur?.sessionId === sessionId ? null : cur));
  const rename = useCallback(
    async (sessionId: string, name: string) => {
      const attempt = ++counter.current;
      latest.current[sessionId] = attempt;
      clear(sessionId);
      try {
        await store(sessionId, name);
      } catch (e) {
        if (latest.current[sessionId] === attempt) setError({ sessionId, message: `Not renamed: ${formatError(e)}` });
        throw e;
      }
      if (latest.current[sessionId] === attempt) clear(sessionId);
    },
    [store],
  );
  const editingChange = useCallback((sessionId: string, editing: boolean) => {
    if (editing) {
      setEditingId(sessionId);
      return;
    }
    setEditingId((cur) => (cur === sessionId ? null : cur));
    clear(sessionId);
  }, []);
  return { editingId, error, rename, editingChange };
}
