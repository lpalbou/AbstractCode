import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  finishPendingStops,
  forgetStopWhileStarting,
  noteStartedRun,
  pendingStops,
  rememberStopWhileStarting,
} from "./start_stop";

// Review 2026-11-08 R12: a Stop pressed while a run was starting must stop that run even when the
// page that pressed it is gone (a reload) or shows another conversation.
describe("stop while starting", () => {
  let store: Map<string, string>;
  let calls: Array<{ url: string; body?: any }>;
  let runs: Array<Record<string, unknown>>;
  let refuseCancel = false;
  beforeEach(() => {
    store = new Map();
    calls = [];
    runs = [];
    refuseCancel = false;
    vi.stubGlobal("sessionStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => store.set(k, String(v)),
      removeItem: (k: string) => store.delete(k),
    });
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", async (url: string, init: RequestInit = {}) => {
      calls.push({ url, body: init.body ? JSON.parse(String(init.body)) : undefined });
      if (url.startsWith("api/gateway/runs?")) return new Response(JSON.stringify({ items: runs }), { status: 200 });
      if (url === "api/gateway/commands") return refuseCancel ? new Response(JSON.stringify({ detail: "busy" }), { status: 503 }) : new Response("{}", { status: 200 });
      return new Response("{}", { status: 404 });
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  it("keeps the stop until the started run is cancelled, by id once known", async () => {
    const id = rememberStopWhileStarting("s1");
    expect(pendingStops()).toHaveLength(1);
    noteStartedRun(id, "run-7");
    await finishPendingStops();
    const cancel = calls.find((c) => c.url === "api/gateway/commands");
    expect(cancel?.body).toMatchObject({ run_id: "run-7", type: "cancel" });
    expect(pendingStops()).toHaveLength(0);
  });

  it("after a reload with no run id: cancels the conversation's run started around the press, nothing older or finished", async () => {
    rememberStopWhileStarting("s1");
    const at = pendingStops()[0].at;
    runs = [
      { run_id: "older", status: "running", created_at: new Date(at - 60_000).toISOString() },
      { run_id: "done", status: "completed", created_at: new Date(at + 500).toISOString() },
      { run_id: "started", status: "running", created_at: new Date(at + 800).toISOString() },
    ];
    await finishPendingStops();
    expect(calls[0].url).toContain("session_id=s1");
    expect(calls.filter((c) => c.url === "api/gateway/commands").map((c) => c.body.run_id)).toEqual(["started"]);
    expect(pendingStops()).toHaveLength(0);
  });

  it("a run the gateway does not list yet keeps the stop for the next try", async () => {
    rememberStopWhileStarting("s1");
    runs = [];
    expect(await finishPendingStops()).toBe(1);
    runs = [{ run_id: "late", status: "running", created_at: new Date(pendingStops()[0].at + 200).toISOString() }];
    expect(await finishPendingStops()).toBe(0);
    expect(calls.filter((c) => c.url === "api/gateway/commands").map((c) => c.body.run_id)).toEqual(["late"]);
  });

  it("a refused cancel is kept for the next load; a start that failed is forgotten", async () => {
    const id = rememberStopWhileStarting("s1");
    noteStartedRun(id, "run-8");
    refuseCancel = true;
    await finishPendingStops();
    expect(pendingStops()).toHaveLength(1);
    forgetStopWhileStarting(id);
    expect(pendingStops()).toHaveLength(0);
  });

  it("drops a stop older than ten minutes", () => {
    store.set("abstractcode.stop-while-starting.v1", JSON.stringify([{ id: "x", sessionId: "s", at: Date.now() - 11 * 60_000 }]));
    expect(pendingStops()).toHaveLength(0);
  });
});
