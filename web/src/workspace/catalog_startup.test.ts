// The first screen's requests (CODE-WEB-LOAD-PROFILE 2026-11-10, fixes 2 and 3): each part of the catalog is shown
// when its own answers are in, the `Archived · N` count is read once the conversations are on screen (without turn
// metrics), and the events catch-up does not read the conversation list again.
//
// This suite has no DOM, so the real hooks run under a minimal hook runner (state, refs, callbacks, effects, one
// component) against a gateway whose answers the test releases one at a time.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

const hooks = vi.hoisted(() => {
  const same = (a?: readonly unknown[], b?: readonly unknown[]) =>
    Boolean(a && b && a.length === b.length && a.every((v, i) => Object.is(v, b[i])));
  const h = { slots: [] as any[], at: 0, effects: [] as Array<() => void>, component: (() => {}) as () => void, queued: false };
  const render = () => {
    h.queued = false;
    h.at = 0;
    h.effects = [];
    h.component();
    for (const effect of h.effects.splice(0)) effect();
  };
  const schedule = () => {
    if (h.queued) return;
    h.queued = true;
    queueMicrotask(render);
  };
  return {
    h,
    render,
    mount(component: () => void) {
      for (const slot of h.slots) slot?.cleanup?.();
      h.slots = [];
      h.component = component;
      render();
    },
    useState(init: unknown) {
      const i = h.at++;
      if (!h.slots[i]) {
        const slot: any = { value: typeof init === "function" ? (init as () => unknown)() : init };
        slot.set = (next: unknown) => {
          slot.value = typeof next === "function" ? (next as (v: unknown) => unknown)(slot.value) : next;
          schedule();
        };
        h.slots[i] = slot;
      }
      return [h.slots[i].value, h.slots[i].set];
    },
    useRef(init: unknown) {
      const i = h.at++;
      h.slots[i] ??= { current: init };
      return h.slots[i];
    },
    useCallback(fn: unknown, deps: unknown[]) {
      const i = h.at++;
      if (!h.slots[i] || !same(h.slots[i].deps, deps)) h.slots[i] = { fn, deps };
      return h.slots[i].fn;
    },
    useEffect(fn: () => void | (() => void), deps?: unknown[]) {
      const i = h.at++;
      const previous = h.slots[i];
      if (previous && deps && same(previous.deps, deps)) return;
      const slot = (h.slots[i] = { deps, cleanup: previous?.cleanup });
      h.effects.push(() => {
        slot.cleanup?.();
        slot.cleanup = fn() || undefined;
      });
    },
  };
});

vi.mock("react", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react")>()),
  useState: hooks.useState,
  useRef: hooks.useRef,
  useCallback: hooks.useCallback,
  useEffect: hooks.useEffect,
}));

const net = vi.hoisted(() => ({
  log: [] as string[],
  open: [] as Array<{ path: string; resolve(value: unknown): void; reject(reason: unknown): void }>,
  answered: 0,
}));

vi.mock("./transport", () => ({
  gatewayRequest: (path: string, init: { signal?: AbortSignal } = {}) =>
    new Promise((resolve, reject) => {
      net.log.push(path);
      net.open.push({ path, resolve, reject });
      init.signal?.addEventListener("abort", () => reject(Object.assign(new Error("aborted"), { name: "AbortError" })));
    }),
  formatError: (error: any) => String(error?.message || error),
}));

import { useCallback } from "react";
import { ARCHIVED_COUNT_PATH, ARCHIVED_RUNS_PATH } from "./archived_conversations";
import { useRefreshOnEvents, useWorkspaceCatalog } from "./use_workspace_catalog";

const F = JSON.parse(readFileSync(join(__dirname, "fixtures/notes_background.json"), "utf8"));
const TOOLS = JSON.parse(readFileSync(join(__dirname, "fixtures/discovery_tools_command_sandbox.json"), "utf8"));
const RUNS = /^api\/gateway\/runs\?root_only=true&include_ledger_len=false&include_metrics=true&limit=\d+$/;
const ARCHIVED = `api/gateway/${ARCHIVED_COUNT_PATH}`;
// The runs page counts 5 archived sessions; the archived listing holds 2 of Notes' (`notes_work.test.tsx`).
const runsPage = { ...F.runs, archived_sessions: 5 };
const archivedPage = { ...F.runs, items: F.runs.items.filter((r: any) => r.session_kind === "chat") };
const START_SET: Array<[string, unknown]> = [
  ["bundles?all_versions=true&include_drafts=false", F.bundles],
  ["workspace/policy", { max_attachment_bytes: 1000 }],
  ["discovery/tools", TOOLS],
  ["workflow-catalog?scope=tenant", { items: [] }],
  ["config/capability-defaults", {}],
  ["bundles?executable_for=abstractcode.agent.v1", { items: [], interface: "abstractcode.agent.v1", filtered: true }],
];

// Stable, as app.tsx's (a useCallback): a new one each render would re-run the catalog's refresh.
const onAuthError = () => {};
const settle = () => new Promise<void>((resolve) => setImmediate(resolve));
async function release(match: string | RegExp, body: unknown, fail = false) {
  const at = net.open.findIndex((p) => (typeof match === "string" ? p.path === (match.startsWith("api/") ? match : `api/gateway/${match}`) : match.test(p.path)));
  if (at < 0) throw new Error(`no open request for ${match}; open: ${net.open.map((p) => p.path).join(", ")}`);
  const [request] = net.open.splice(at, 1);
  net.answered += 1;
  if (fail) request.reject(body);
  else request.resolve(body);
  await settle();
}
const answerLabels = async () => {
  while (net.open.some((p) => p.path.endsWith("/input_data"))) await release(/\/input_data$/, {});
};
const listings = () => net.log.filter((path) => RUNS.test(path)).length;
const archivedReads = () => net.log.filter((path) => path.includes("archived_only=true")).length;

afterEach(() => {
  hooks.mount(() => {});
  net.log = [];
  net.open = [];
  net.answered = 0;
  vi.useRealTimers();
});

function mountCatalog(onPaint?: (catalog: ReturnType<typeof useWorkspaceCatalog>) => void) {
  const view: { catalog: ReturnType<typeof useWorkspaceCatalog> } = {} as any;
  hooks.mount(() => {
    view.catalog = useWorkspaceCatalog("default:admin", onAuthError);
    onPaint?.(view.catalog);
  });
  return view;
}

describe("the catalog shows each part as its answers land", () => {
  it("conversations after their own answer, the archived count after them, workflows after the start set", async () => {
    // Answers in before each part is first on screen (the old hook: 9 for every part, 8 catalog + archived).
    const answeredAt: Record<string, number> = {};
    const view = mountCatalog((c) => {
      if (c.sessionsLoaded) answeredAt.conversations ??= net.answered;
      if (!c.capabilitiesLoading) answeredAt.automations ??= net.answered;
      if (!c.loading && c.workflows.length) answeredAt.workflows ??= net.answered;
      if (c.archivedSessions) answeredAt.archived ??= net.answered;
    });
    await settle();
    expect(net.log).toHaveLength(8);
    expect(view.catalog.sessionsLoading && !view.catalog.sessionsLoaded).toBe(true);

    await release(RUNS, runsPage);
    expect(view.catalog.sessions.map((s) => s.sessionId).sort()).toEqual(["acode-4b0e9d7c2f1a", "acode-8ec4cd0251ba", "acode-9f8e7d6c5b4a"]);
    expect(view.catalog.sessionsLoading).toBe(false);
    expect([view.catalog.loading, view.catalog.capabilitiesLoading, view.catalog.workflows.length]).toEqual([true, true, 0]);
    // The count is asked only now, while the list is shown, and without turn metrics.
    expect(net.open.map((p) => p.path)).toContain(ARCHIVED);
    expect(ARCHIVED).not.toContain("include_metrics");
    expect(ARCHIVED_RUNS_PATH).toContain("include_metrics=true"); // the footer's rows still show "N tools"
    expect(view.catalog.archivedSessions).toBe(0);

    await release("discovery/capabilities", { capabilities: { contracts: { automations: { version: 1 } } } });
    expect(view.catalog.capabilitiesLoading).toBe(false);
    expect(view.catalog.capabilities).toEqual({ automations: { version: 1 } });
    expect(view.catalog.loading).toBe(true);

    await release(ARCHIVED, archivedPage);
    expect(view.catalog.archivedSessions).toBe(3);

    for (const [path, body] of START_SET) await release(path, body);
    expect(view.catalog.loading).toBe(false);
    expect(view.catalog.workflows.length).toBeGreaterThan(0);
    expect(view.catalog.errors).toEqual([]);
    expect(answeredAt).toEqual({ conversations: 1, automations: 2, archived: 3, workflows: 9 });
  });

  it("the workflows wait for every answer a turn's start reads (a turn sent earlier would lose the tool choices)", async () => {
    const view = mountCatalog();
    await settle();
    await release("discovery/capabilities", {});
    for (const [path, body] of START_SET.filter(([path]) => path !== "discovery/tools")) await release(path, body);
    expect([view.catalog.loading, view.catalog.workflows.length, view.catalog.executable.status]).toEqual([true, 0, "loading"]);
    await release("discovery/tools", TOOLS);
    expect(view.catalog.loading).toBe(false);
    expect(view.catalog.workflows.length).toBeGreaterThan(0);
  });

  it("a failed conversation list is still said, and stays said once the rest lands", async () => {
    const view = mountCatalog();
    await settle();
    await release(RUNS, Object.assign(new Error("HTTP 503"), { status: 503 }), true);
    expect(view.catalog.errors).toEqual(["Conversations: HTTP 503"]);
    expect([view.catalog.sessionsLoading, view.catalog.sessionsLoaded]).toEqual([false, false]);
    await release("discovery/capabilities", Object.assign(new Error("HTTP 500"), { status: 500 }), true);
    for (const [path, body] of START_SET) await release(path, body);
    expect(view.catalog.errors).toEqual(["Capabilities: HTTP 500", "Conversations: HTTP 503"]);
    expect(view.catalog.capabilitiesError).toBe("Could not read the gateway's capabilities: HTTP 500");
    expect(archivedReads()).toBe(0);
  });
});

describe("the events catch-up does not read the conversation list again", () => {
  it("one runs listing and one archived listing for the startup and its catch-up; a later event reads them once more", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const events = { cursor: 0, following: false };
    hooks.mount(() => {
      const catalog = useWorkspaceCatalog("default:admin", onAuthError);
      const refresh = useCallback(() => void catalog.refreshSessions(), [catalog.refreshSessions]);
      useRefreshOnEvents("default:admin", events.cursor, events.following, refresh);
    });
    await settle();
    await release(RUNS, runsPage);
    await release(ARCHIVED, archivedPage);
    await answerLabels();
    // The catch-up from 0: eleven pages of history, then "following" on the head.
    for (const cursor of [1000, 2000, 5000, 10000]) {
      events.cursor = cursor;
      hooks.render();
      await vi.advanceTimersByTimeAsync(200);
    }
    events.cursor = 10317;
    events.following = true;
    hooks.render();
    await vi.advanceTimersByTimeAsync(1000);
    await settle();
    expect([listings(), archivedReads()]).toEqual([1, 1]);

    // A live event: the list is read again, once.
    events.cursor = 10318;
    hooks.render();
    await vi.advanceTimersByTimeAsync(600);
    await settle();
    expect(listings()).toBe(2);
    await release(RUNS, runsPage);
    expect(archivedReads()).toBe(2);
    hooks.render();
    await vi.advanceTimersByTimeAsync(600);
    expect(listings()).toBe(2);
  });

  it("each identity starts a new baseline; events caught up after a reconnect are news", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const refresh = vi.fn();
    const props = { identity: "a", cursor: 0, following: false };
    hooks.mount(() => useRefreshOnEvents(props.identity, props.cursor, props.following, refresh));
    const step = async (next: Partial<typeof props>) => {
      Object.assign(props, next);
      hooks.render();
      await vi.advanceTimersByTimeAsync(600);
    };
    await step({ cursor: 40 });
    await step({ cursor: 50, following: true });
    expect(refresh).not.toHaveBeenCalled();
    await step({ following: false });
    await step({ cursor: 55 }); // a catch-up after the baseline (the stream reconnected)
    expect(refresh).toHaveBeenCalledTimes(1);
    await step({ identity: "b", cursor: 0, following: false });
    await step({ cursor: 30 });
    await step({ following: true });
    expect(refresh).toHaveBeenCalledTimes(1);
    await step({ cursor: 31 });
    expect(refresh).toHaveBeenCalledTimes(2);
  });
});
