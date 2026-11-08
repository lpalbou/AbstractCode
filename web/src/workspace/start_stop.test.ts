import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  WAIT_FOR_RUN_MS,
  finishPendingStops,
  forgetStopWhileStarting,
  forgetStopsOfConversation,
  noteStartedRun,
  pendingStops,
  rememberStopWhileStarting,
} from "./start_stop";

// Reviews 2026-11-08 R12 and D6: a Stop pressed while a run was starting is finished after a
// reload, but only on the run it was for: exactly one candidate is cancelled, several are left
// to the person, none waits (bounded); a new run of the conversation drops the stop.
describe("stop while starting", () => {
  let store: Map<string, string>;
  let calls: Array<{ url: string; body?: any }>;
  let runs: Array<Record<string, unknown>>;
  let cancelAnswer: { status: number; body: unknown };
  const cancels = () => calls.filter((c) => c.url === "api/gateway/commands").map((c) => c.body.run_id);
  const run = (id: string, at: number, status = "running") => ({ run_id: id, status, created_at: new Date(at).toISOString() });
  beforeEach(() => {
    store = new Map();
    calls = [];
    runs = [];
    cancelAnswer = { status: 200, body: { accepted: true } };
    vi.stubGlobal("sessionStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => store.set(k, String(v)),
      removeItem: (k: string) => store.delete(k),
    });
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", async (url: string, init: RequestInit = {}) => {
      calls.push({ url, body: init.body ? JSON.parse(String(init.body)) : undefined });
      if (url.startsWith("api/gateway/runs?")) return new Response(JSON.stringify({ items: runs }), { status: 200 });
      if (url === "api/gateway/commands") return new Response(JSON.stringify(cancelAnswer.body), { status: cancelAnswer.status });
      return new Response("{}", { status: 404 });
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  it("cancels by id once the start answered", async () => {
    const id = rememberStopWhileStarting("s1");
    noteStartedRun(id, "run-7");
    expect(await finishPendingStops()).toEqual({ waiting: 0, choices: [], dropped: [] });
    expect(cancels()).toEqual(["run-7"]);
    expect(pendingStops()).toHaveLength(0);
  });

  it("after a reload: exactly one unfinished run in the press's window is cancelled; older, later or finished ones never", async () => {
    rememberStopWhileStarting("s1");
    const at = pendingStops()[0].at;
    runs = [
      run("older", at - 3_000),
      run("done", at + 500, "completed"),
      run("started", at - 300),
      run("next-day", at + 61_000),
    ];
    await finishPendingStops();
    expect(calls[0].url).toContain("session_id=s1");
    expect(cancels()).toEqual(["started"]);
    expect(pendingStops()).toHaveLength(0);
  });

  it("the window starts at the Send: a run created after the Send but more than 1 s before the Stop is matched (R14)", async () => {
    const sentAt = Date.now() - 2_500;
    rememberStopWhileStarting("s1", sentAt);
    const at = pendingStops()[0].at;
    runs = [run("slow-start", at - 1_500), run("before-send", sentAt - 5_000)];
    await finishPendingStops();
    expect(cancels()).toEqual(["slow-start"]);
  });

  it("two candidates: nothing is cancelled, the person chooses", async () => {
    rememberStopWhileStarting("s1");
    const at = pendingStops()[0].at;
    runs = [run("a", at + 100), run("b", at + 4_000)];
    const { waiting, choices } = await finishPendingStops();
    expect(cancels()).toEqual([]);
    expect(waiting).toBe(0);
    expect(choices).toHaveLength(1);
    expect(choices[0].candidates?.map((c) => c.runId)).toEqual(["a", "b"]);
    // Asked again: still no guess, no new list request.
    const before = calls.length;
    await finishPendingStops();
    expect(calls.length).toBe(before);
    forgetStopWhileStarting(choices[0].id);
    expect(pendingStops()).toHaveLength(0);
  });

  it("no run listed yet: the stop waits, then is dropped after the bound", async () => {
    rememberStopWhileStarting("s1");
    const at = pendingStops()[0].at;
    expect((await finishPendingStops(() => at + 2_000)).waiting).toBe(1);
    runs = [run("late", at + 1_500)];
    expect((await finishPendingStops(() => at + 5_000)).waiting).toBe(0);
    expect(cancels()).toEqual(["late"]);
    rememberStopWhileStarting("s2");
    runs = [];
    const at2 = pendingStops()[0].at;
    const late = await finishPendingStops(() => at2 + WAIT_FOR_RUN_MS + 1);
    expect(late.waiting).toBe(0);
    // Dropped without a match: returned, so the conversation says so (never silent).
    expect(late.dropped.map((p) => p.sessionId)).toEqual(["s2"]);
    expect(pendingStops()).toHaveLength(0);
  });

  it("a refused cancel (an error or accepted: false) keeps the stop", async () => {
    const id = rememberStopWhileStarting("s1");
    noteStartedRun(id, "run-8");
    cancelAnswer = { status: 503, body: { detail: "busy" } };
    await finishPendingStops();
    expect(pendingStops()).toHaveLength(1);
    cancelAnswer = { status: 200, body: { accepted: false, detail: "not yet" } };
    await finishPendingStops();
    expect(pendingStops()).toHaveLength(1);
    cancelAnswer = { status: 200, body: { accepted: true } };
    await finishPendingStops();
    expect(pendingStops()).toHaveLength(0);
  });

  it("a stop dropped while its list is in flight (the person sent again) cancels nothing (R13)", async () => {
    rememberStopWhileStarting("s1");
    const at = pendingStops()[0].at;
    vi.stubGlobal("fetch", async (url: string, init: RequestInit = {}) => {
      calls.push({ url, body: init.body ? JSON.parse(String(init.body)) : undefined });
      if (url.startsWith("api/gateway/runs?")) {
        forgetStopsOfConversation("s1"); // Send pressed while the list was on its way
        return new Response(JSON.stringify({ items: [run("new-turn", at + 2_000)] }), { status: 200 });
      }
      return new Response(JSON.stringify({ accepted: true }), { status: 200 });
    });
    await finishPendingStops();
    expect(cancels()).toEqual([]);
  });

  it("a new run started by the tab in that conversation drops the stop there (never cancels the new run)", async () => {
    rememberStopWhileStarting("s1");
    rememberStopWhileStarting("s2");
    forgetStopsOfConversation("s1");
    expect(pendingStops().map((p) => p.sessionId)).toEqual(["s2"]);
  });

  it("drops a stop older than ten minutes", () => {
    store.set("abstractcode.stop-while-starting.v1", JSON.stringify([{ id: "x", sessionId: "s", at: Date.now() - 11 * 60_000 }]));
    expect(pendingStops()).toHaveLength(0);
  });
});
