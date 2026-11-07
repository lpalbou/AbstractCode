// Code's long-lived gateway streams (the replay view stream and the ledger stream of a conversation)
// each hold one of the browser's few connections per host (6 for HTTP/1.1, shared by every tab).
// Two rules keep them from starving every other request, a permission click included:
//   - a hidden tab holds none: the stream is closed when the tab is hidden and reopened when it is
//     visible again, from the cursor the stream last delivered (the same resume the stream already uses
//     after a dropped connection), so no event is lost and none is delivered twice;
//   - one stream per run and kind in a tab: opening a second one closes the first.
// The panel-chat controller keeps calling its transport as before; while the tab is hidden the call
// simply stays pending, so the controller's reconnect budget is never spent.

export type Visibility = {
  hidden(): boolean;
  /** Calls `fn` when the tab is shown or hidden; returns the way to stop. */
  subscribe(fn: () => void): () => void;
};

export const documentVisibility: Visibility = {
  hidden: () => typeof document !== "undefined" && document.visibilityState === "hidden",
  subscribe: (fn) => {
    if (typeof document === "undefined" || typeof document.addEventListener !== "function") return () => {};
    document.addEventListener("visibilitychange", fn);
    return () => document.removeEventListener("visibilitychange", fn);
  },
};

type Entry = { close: () => void; live?: boolean };

export class StreamGate {
  private readonly open = new Map<string, Entry>();
  constructor(private readonly visibility: Visibility = documentVisibility) {}

  /** The number of streams open right now (a hidden tab: zero). */
  get openCount(): number {
    return [...this.open.values()].filter((e) => e.live).length;
  }

  /**
   * Runs `open(signal)` (one stream; it resolves when the stream ends) under the rules above. `open`
   * is called again after every pause and must resume from the cursor it last delivered. Resolves when
   * the stream ends by itself, when `signal` aborts, or when a newer stream for `key` replaced it.
   */
  async run(key: string, signal: AbortSignal | undefined, open: (signal: AbortSignal) => Promise<void>): Promise<void> {
    this.open.get(key)?.close();
    let replaced = false;
    let inner: AbortController | null = null;
    let pausing = false;
    let wake: (() => void) | null = null;
    const entry: Entry = {
      close: () => {
        replaced = true;
        inner?.abort();
        wake?.();
      },
    };
    this.open.set(key, entry);
    const onVisibility = () => {
      if (this.visibility.hidden()) {
        if (inner && !inner.signal.aborted) {
          pausing = true;
          inner.abort();
        }
      } else wake?.();
    };
    const onAbort = () => {
      inner?.abort();
      wake?.();
    };
    const unsubscribe = this.visibility.subscribe(onVisibility);
    signal?.addEventListener("abort", onAbort, { once: true });
    const done = () => replaced || signal?.aborted === true;
    try {
      while (!done()) {
        while (this.visibility.hidden() && !done()) await new Promise<void>((resolve) => (wake = resolve));
        wake = null;
        if (done()) return;
        inner = new AbortController();
        pausing = false;
        entry.live = true;
        try {
          await open(inner.signal);
        } catch (error) {
          // The pause aborts the fetch; that is not a failure. Every other error is the stream's own.
          if (replaced) return; // closed by the newer stream for this key: not this stream's failure
          if (!pausing || done()) throw error;
        } finally {
          entry.live = false;
        }
        if (!pausing) return;
      }
    } finally {
      unsubscribe();
      signal?.removeEventListener("abort", onAbort);
      if (this.open.get(key) === entry) this.open.delete(key);
    }
  }
}
