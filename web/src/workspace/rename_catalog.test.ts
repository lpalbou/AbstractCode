// Backlog 1343: renaming a conversation through the catalog hook, against a gateway whose answers the test
// releases one at a time (the minimal hook runner of catalog_startup.test.ts: no DOM). The name shows at once,
// reconciles with the gateway's normalised name, goes back on a refusal, survives a list read that started
// before the gateway answered, gives way to another device's newer name afterwards, and never reorders the list
// or changes its "updated" times. An older gateway (no `session_titles` capability) is never asked.
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
  log: [] as Array<{ path: string; method: string; body?: string }>,
  open: [] as Array<{ path: string; resolve(value: unknown): void; reject(reason: unknown): void }>,
}));

vi.mock("./transport", () => ({
  gatewayRequest: (path: string, init: { signal?: AbortSignal; method?: string; body?: string } = {}) =>
    new Promise((resolve, reject) => {
      net.log.push({ path, method: init.method || "GET", body: init.body });
      net.open.push({ path, resolve, reject });
      init.signal?.addEventListener("abort", () => reject(Object.assign(new Error("aborted"), { name: "AbortError" })));
    }),
  formatError: (error: any) => String(error?.message || error),
}));

import { useWorkspaceCatalog } from "./use_workspace_catalog";
import { conversationTitle } from "./sidebar_cards";

const RUNS = /^api\/gateway\/runs\?root_only=true&include_ledger_len=false&include_metrics=true&limit=\d+$/;
const TITLE = (id: string) => `api/gateway/sessions/${id}/title`;
const row = (session: string, at: string, prompt: string, title?: string) => ({
  run_id: `run-${session}`, session_id: session, session_kind: "chat", status: "completed", parent_run_id: null,
  created_at: at, updated_at: at, tool_calls: 0, input_data: { prompt }, ...(title ? { session_title: title } : {}),
});
const page = (...items: unknown[]) => ({ items, has_more: false, archived_sessions: 0 });
const LIST = () => page(row("s1", "2026-11-20T10:00:00Z", "Write a snake game"), row("s2", "2026-11-19T10:00:00Z", "Fix the parser"));

const onAuthError = vi.fn();
const settle = () => new Promise<void>((resolve) => setImmediate(resolve));
async function release(match: string | RegExp, body: unknown, fail = false) {
  const at = net.open.findIndex((p) => (typeof match === "string" ? p.path === match : match.test(p.path)));
  if (at < 0) throw new Error(`no open request for ${match}; open: ${net.open.map((p) => p.path).join(", ")}`);
  const [request] = net.open.splice(at, 1);
  if (fail) request.reject(body);
  else request.resolve(body);
  await settle();
}
const writes = () => net.log.filter((r) => r.method !== "GET");
const refusal = (status: number, message: string) => Object.assign(new Error(message), { status });
// The outcome of a rename, caught at once (a refusal lands before the test looks at it).
const attempt = (rename: Promise<void>) => rename.then(() => "stored", (e: Error) => e.message);

afterEach(() => {
  hooks.mount(() => {});
  net.log = [];
  net.open = [];
  onAuthError.mockReset();
});

async function mountListed(list = LIST()) {
  const view: { catalog: ReturnType<typeof useWorkspaceCatalog> } = {} as any;
  hooks.mount(() => {
    view.catalog = useWorkspaceCatalog("default:admin", onAuthError);
  });
  await settle();
  await release(RUNS, list);
  return view;
}
const names = (view: { catalog: ReturnType<typeof useWorkspaceCatalog> }) => view.catalog.sessions.map((s) => conversationTitle(s));
const facts = (view: { catalog: ReturnType<typeof useWorkspaceCatalog> }) => view.catalog.sessions.map((s) => [s.sessionId, s.updatedAt]);

describe("renaming a conversation through the catalog", () => {
  it("PUTs the name, shows it at once, then the gateway's normalised name; order and times unchanged", async () => {
    const view = await mountListed();
    const before = facts(view);
    const typed = "x".repeat(200);
    const done = attempt(view.catalog.renameSession("s2", typed));
    await settle();
    expect(writes()).toEqual([{ path: TITLE("s2"), method: "PUT", body: JSON.stringify({ title: typed }) }]);
    expect(names(view)).toEqual(["Write a snake game", typed]);
    await release(TITLE("s2"), { session_id: "s2", title: "x".repeat(120), changed: true });
    expect(await done).toBe("stored");
    expect(names(view)).toEqual(["Write a snake game", "x".repeat(120)]);
    expect(facts(view)).toEqual(before);
  });

  it("clearing the name shows the first message again", async () => {
    const view = await mountListed(page(row("s1", "2026-11-20T10:00:00Z", "Write a snake game", "Snake")));
    expect(names(view)).toEqual(["Snake"]);
    const done = attempt(view.catalog.renameSession("s1", ""));
    await settle();
    expect(writes()[0].body).toBe(JSON.stringify({ title: "" }));
    expect(names(view)).toEqual(["Write a snake game"]);
    await release(TITLE("s1"), { session_id: "s1", title: null, changed: true });
    expect(await done).toBe("stored");
    expect(names(view)).toEqual(["Write a snake game"]);
  });

  it.each([
    [404, "session_not_found"],
    [403, "forbidden"],
    [500, "session titles unreadable"],
  ])("a %i refusal goes back to the previous name and reaches the caller", async (status, message) => {
    const view = await mountListed(page(row("s1", "2026-11-20T10:00:00Z", "Write a snake game", "Snake")));
    const done = attempt(view.catalog.renameSession("s1", "Lizard"));
    await settle();
    expect(names(view)).toEqual(["Lizard"]);
    await release(TITLE("s1"), refusal(status, message), true);
    expect(await done).toBe(message);
    expect(names(view)).toEqual(["Snake"]);
    expect(onAuthError).not.toHaveBeenCalled();
  });

  it("a network failure goes back too; a 401 also says the sign-in ended", async () => {
    const view = await mountListed();
    const lost = attempt(view.catalog.renameSession("s1", "Snake"));
    await settle();
    await release(TITLE("s1"), new TypeError("Failed to fetch"), true);
    expect(await lost).toBe("Failed to fetch");
    expect(names(view)[0]).toBe("Write a snake game");
    const signedOut = attempt(view.catalog.renameSession("s1", "Snake"));
    await settle();
    await release(TITLE("s1"), refusal(401, "Gateway sign-in required"), true);
    expect(await signedOut).toBe("Gateway sign-in required");
    expect(onAuthError).toHaveBeenCalledTimes(1);
  });

  it("a slow answer never overwrites a newer rename; a late refusal never undoes it", async () => {
    const view = await mountListed();
    const first = view.catalog.renameSession("s1", "First");
    await settle();
    const second = view.catalog.renameSession("s1", "Second");
    await settle();
    expect(names(view)[0]).toBe("Second");
    await release(TITLE("s1"), { session_id: "s1", title: "First", changed: true });
    await first;
    expect(names(view)[0]).toBe("Second");
    await release(TITLE("s1"), { session_id: "s1", title: "Second", changed: true });
    await second;
    expect(names(view)[0]).toBe("Second");

    const third = attempt(view.catalog.renameSession("s1", "Third"));
    await settle();
    const fourth = view.catalog.renameSession("s1", "Fourth");
    await settle();
    await release(TITLE("s1"), refusal(500, "boom"), true);
    expect(await third).toBe("boom");
    expect(names(view)[0]).toBe("Fourth");
    await release(TITLE("s1"), { session_id: "s1", title: "Fourth", changed: true });
    await fourth;
    expect(names(view)[0]).toBe("Fourth");
  });

  it("a list read started before the answer keeps the new name; one started after shows another device's", async () => {
    const view = await mountListed();
    const done = attempt(view.catalog.renameSession("s1", "Snake"));
    await settle();
    void view.catalog.refreshSessions(); // e.g. an event while the PUT is out
    await settle();
    await release(RUNS, LIST()); // the gateway had not stored it yet
    expect(names(view)[0]).toBe("Snake");
    await release(TITLE("s1"), { session_id: "s1", title: "Snake", changed: true });
    expect(await done).toBe("stored");
    void view.catalog.refreshSessions();
    await settle();
    await release(RUNS, LIST()); // started after the answer: the list is the truth again (here: cleared elsewhere)
    expect(names(view)[0]).toBe("Write a snake game");
    void view.catalog.refreshSessions();
    await settle();
    await release(RUNS, page(row("s1", "2026-11-20T10:00:00Z", "Write a snake game", "Renamed elsewhere"), row("s2", "2026-11-19T10:00:00Z", "Fix the parser")));
    expect(names(view)).toEqual(["Renamed elsewhere", "Fix the parser"]);
  });

  it("search finds the stored name", async () => {
    const { searchConversations } = await import("./sidebar_cards");
    const view = await mountListed(page(row("s1", "2026-11-20T10:00:00Z", "Write a snake game", "Arcade project"), row("s2", "2026-11-19T10:00:00Z", "Fix the parser")));
    expect(searchConversations(view.catalog.sessions, "arcade").map((s) => s.sessionId)).toEqual(["s1"]);
    expect(searchConversations(view.catalog.sessions, "snake").map((s) => s.sessionId)).toEqual(["s1"]);
  });
});

describe("the capability read at start", () => {
  it("a gateway that stores names says so: sessionTitles", async () => {
    const view = await mountListed();
    expect(view.catalog.sessionTitles).toBe(false); // until the answer is in
    await release("api/gateway/discovery/capabilities", { capabilities: { session_titles: true, contracts: { automations: { version: 1 } } } });
    expect(view.catalog.sessionTitles).toBe(true);
  });
  it("an older gateway (no key, or an unreadable answer) gets no rename, and no PUT is ever sent", async () => {
    const older = await mountListed();
    await release("api/gateway/discovery/capabilities", { capabilities: { contracts: { automations: { version: 1 } } } });
    expect(older.catalog.sessionTitles).toBe(false);
    const failed = await mountListed();
    await release("api/gateway/discovery/capabilities", Object.assign(new Error("HTTP 500"), { status: 500 }), true);
    expect(failed.catalog.sessionTitles).toBe(false);
    expect(writes()).toEqual([]);
  });
});
