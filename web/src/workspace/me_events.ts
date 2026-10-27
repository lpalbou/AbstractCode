/**
 * The person's own event stream on the gateway (framework backlog 1046/1047),
 * for the Automations header badge.
 *
 * - `GET api/gateway/me/events?after=&limit=` → `{stream, events[], next_after, head, more}`
 *   (catch-up pages, oldest first);
 * - `GET api/gateway/me/events/stream` → SSE (`id: <sequence>`, `event: <type>`,
 *   `data: <CloudEvents JSON>`, keep-alive comments), resumed with `Last-Event-ID`.
 *
 * Both go through this app's authenticated proxy (relative `api/gateway/…`).
 * The live tail runs only while the tab is visible; a hidden tab reads one
 * catch-up page a minute. A 404 is a gateway without the stream: the follower
 * stops and the badge keeps the automation list's sum.
 */
import { gatewayApiPath } from "@abstractframework/ui-kit";
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";

import { SseParser } from "../lib/sse_parser";

export type UserEvent = {
  id?: string;
  type: string;
  sequence: number;
  data?: Record<string, any> | null;
};

/** Raised minus seen, folded in stream order. */
export type AttentionFold = {
  cursor: number;
  /** "run\u0000wait_key\u0000step_id" → session id (the same node asking again is a new wait). */
  waits: Record<string, string>;
  raised: Record<string, number[]>;
  seen: Record<string, number>;
  resolved: string[];
  /** Bumped by every automation-shaped event: the list is read again. */
  automationEpoch: number;
};

export const emptyFold = (): AttentionFold => ({ cursor: 0, waits: {}, raised: {}, seen: {}, resolved: [], automationEpoch: 0 });

const str = (d: Record<string, any>, k: string): string => (typeof d[k] === "string" && d[k] ? d[k] : "");
const num = (d: Record<string, any>, k: string): number => (typeof d[k] === "number" ? d[k] : 0);
const waitId = (run: string, key: string, step = "") => `${run}\u0000${key}\u0000${step}`;

/** Fold one event (pure: returns a new fold; a sequence already folded changes nothing). */
export function foldEvent(f: AttentionFold, ev: UserEvent): AttentionFold {
  const seq = Number(ev.sequence || 0);
  if (seq > 0 && seq <= f.cursor) return f;
  const out: AttentionFold = {
    cursor: Math.max(f.cursor, seq),
    waits: { ...f.waits },
    raised: { ...f.raised },
    seen: { ...f.seen },
    resolved: f.resolved,
    automationEpoch: f.automationEpoch,
  };
  const d = ev.data || {};
  switch (ev.type) {
    case "attention.raised": {
      const run = str(d, "run_id");
      const key = str(d, "wait_key");
      if (run && key) {
        const step = str(d, "step_id");
        if (str(d, "record_key") && f.resolved.includes(str(d, "record_key"))) break;
        if (step && f.resolved.includes(`step:${waitId(run, key, step)}`)) break;
        out.waits[waitId(run, key, step)] = str(d, "session_id");
      } else if (str(d, "automation_id") && num(d, "seq") > 0) {
        const aid = str(d, "automation_id");
        const q = num(d, "seq");
        const list = out.raised[aid] || [];
        if (q > (out.seen[aid] || 0) && !list.includes(q)) out.raised[aid] = [...list, q];
        out.automationEpoch += 1;
      }
      break;
    }
    case "attention.seen": {
      const record = str(d, "record_key");
      if (record) out.resolved = [...out.resolved, record].slice(-500);
      // The gateway's seen of a wait: {kind, run_id, wait_key, step_id, outcome}.
      const run = str(d, "run_id");
      const key = str(d, "wait_key");
      if (run && key) {
        const step = str(d, "step_id");
        if (step) out.resolved = [...out.resolved, `step:${waitId(run, key, step)}`].slice(-500);
        const prefix = `${run}\u0000${key}\u0000`;
        for (const k of Object.keys(out.waits))
          if (k.startsWith(prefix) && (!step || k === prefix || k === prefix + step)) delete out.waits[k];
      }
      const aid = str(d, "automation_id");
      if (aid) {
        const s = Math.max(out.seen[aid] || 0, num(d, "seq"));
        out.seen[aid] = s;
        out.raised[aid] = (out.raised[aid] || []).filter((q) => q > s);
        out.automationEpoch += 1;
      }
      break;
    }
    case "run.started":
    case "run.finished": {
      if (ev.type === "run.finished" && str(d, "run_id")) {
        const run = str(d, "run_id");
        for (const k of Object.keys(out.waits)) if (k.startsWith(`${run}\u0000`)) delete out.waits[k];
      }
      if (d.origin && typeof d.origin === "object" && d.origin.kind === "trigger") out.automationEpoch += 1;
      break;
    }
    case "trigger.event":
    case "wake.scheduled":
    case "wake.cancelled":
      out.automationEpoch += 1;
      break;
  }
  return out;
}

export const needsYou = (f: AttentionFold): number => Object.keys(f.waits).length;
export const newResults = (f: AttentionFold): number =>
  Object.entries(f.raised).reduce((n, [aid, seqs]) => n + seqs.filter((q) => q > (f.seen[aid] || 0)).length, 0);

export type FollowState = "connecting" | "following" | "unsupported" | "stopped";

type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

/**
 * The follower: catch-up pages, then (while `live()`) the SSE tail resumed
 * with `Last-Event-ID`; when not live, one catch-up per `catchUpMs`.
 */
export class MeEventsFollower {
  fold: AttentionFold = emptyFold();
  state: FollowState = "stopped";
  private listeners = new Set<() => void>();
  private abort: AbortController | null = null;
  private wakeUp: (() => void) | null = null;
  /** A stopped loop that is still awaiting must not run on after a restart. */
  private generation = 0;

  constructor(
    private readonly opts: {
      fetch?: FetchLike;
      live: () => boolean;
      catchUpMs?: number;
      maxBackoffMs?: number;
    },
  ) {}

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private emit(): void {
    for (const fn of this.listeners) fn();
  }

  private set(state: FollowState): void {
    if (this.state === state) return;
    this.state = state;
    this.emit();
  }

  private apply(events: UserEvent[]): void {
    let f = this.fold;
    for (const e of events) f = foldEvent(f, e);
    if (f !== this.fold) {
      this.fold = f;
      this.emit();
    }
  }

  /** The tab became visible (or hidden): cut a wait short. */
  nudge(): void {
    this.wakeUp?.();
    if (!this.opts.live()) this.abort?.abort();
  }

  start(): void {
    if (this.state !== "stopped") return;
    this.set("connecting");
    this.generation += 1;
    void this.loop(this.generation);
  }

  stop(): void {
    this.set("stopped");
    this.abort?.abort();
    this.wakeUp?.();
  }

  private get fetcher(): FetchLike {
    return this.opts.fetch || ((input, init) => fetch(input, { ...init, credentials: "same-origin" }));
  }

  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const t = setTimeout(() => {
        this.wakeUp = null;
        resolve();
      }, ms);
      this.wakeUp = () => {
        clearTimeout(t);
        this.wakeUp = null;
        resolve();
      };
    });
  }

  /** One catch-up read of every page after the cursor. Throws `{status}` on HTTP errors. */
  async catchUp(): Promise<void> {
    for (let i = 0; i < 100; i += 1) {
      const r = await this.fetcher(gatewayApiPath(`me/events?after=${this.fold.cursor}&limit=1000`), { headers: { Accept: "application/json" } });
      if (!r.ok) throw Object.assign(new Error(`me/events: HTTP ${r.status}`), { status: r.status });
      const page = await r.json();
      this.apply(Array.isArray(page?.events) ? page.events : []);
      if (!page?.more) return;
    }
  }

  /** The live tail until it closes, breaks, or the tab is hidden. */
  async tail(): Promise<void> {
    const abort = new AbortController();
    this.abort = abort;
    const headers: Record<string, string> = { Accept: "text/event-stream" };
    if (this.fold.cursor > 0) headers["Last-Event-ID"] = String(this.fold.cursor);
    try {
      const r = await this.fetcher(gatewayApiPath("me/events/stream?heartbeat_s=15"), { headers, signal: abort.signal });
      if (!r.ok) throw Object.assign(new Error(`me/events/stream: HTTP ${r.status}`), { status: r.status });
      if (!r.body) return;
      const reader = r.body.getReader();
      const decoder = new TextDecoder("utf-8");
      const parser = new SseParser();
      // Read, fold, then ask whether to go on: a hidden tab ends the tail
      // (and `nudge` aborts a read that is waiting when the tab hides).
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return;
        const batch: UserEvent[] = [];
        parser.push(decoder.decode(value, { stream: true }), (ev) => {
          try {
            const e = JSON.parse(ev.data ?? "");
            if (e && typeof e === "object") {
              if (typeof e.sequence !== "number" && ev.id && /^\d+$/.test(ev.id)) e.sequence = Number(ev.id);
              batch.push(e);
            }
          } catch {
            /* a malformed frame is skipped; the next one resumes */
          }
        });
        this.apply(batch);
        if (this.state === "stopped" || !this.opts.live()) return;
      }
    } catch (err: any) {
      if (err?.name === "AbortError") return;
      throw err;
    } finally {
      if (this.abort === abort) this.abort = null;
    }
  }

  private async loop(generation: number): Promise<void> {
    let backoff = 1000;
    const max = this.opts.maxBackoffMs ?? 30_000;
    while (this.state !== "stopped" && generation === this.generation) {
      try {
        await this.catchUp();
        this.set("following");
        backoff = 1000;
        if (this.opts.live()) await this.tail();
        else await this.sleep(this.opts.catchUpMs ?? 60_000);
        continue;
      } catch (err: any) {
        const status = Number(err?.status || 0);
        if (status === 404) return this.set("unsupported");
        if (status === 401 || status === 403) return this.set("stopped");
      }
      if ((this.state as FollowState) === "stopped" || generation !== this.generation) return;
      await this.sleep(backoff);
      backoff = Math.min(max, backoff * 2);
    }
  }
}

/** One follower per signed-in identity; the tail runs only while the tab is visible. */
export function useMeEvents(identity: string, enabled: boolean): { fold: AttentionFold; state: FollowState } {
  const visible = () => typeof document === "undefined" || document.visibilityState !== "hidden";
  const follower = useMemo(() => new MeEventsFollower({ live: visible }), [identity]);
  const snapshot = useSyncExternalStore(
    (fn) => follower.subscribe(fn),
    () => follower.fold,
  );
  const [state, setState] = useState<FollowState>(follower.state);
  useEffect(() => follower.subscribe(() => setState(follower.state)), [follower]);
  useEffect(() => {
    if (!identity || !enabled) return;
    follower.start();
    const on = () => follower.nudge();
    document.addEventListener("visibilitychange", on);
    return () => {
      document.removeEventListener("visibilitychange", on);
      follower.stop();
    };
  }, [follower, identity, enabled]);
  return { fold: snapshot, state };
}
