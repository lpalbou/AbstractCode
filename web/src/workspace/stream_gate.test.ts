// Code's streams under the connection rules: a hidden tab holds none and resumes where it left off;
// the ledger stream of a run is open once per tab. Fake gateway at the fetch boundary, fake tab visibility.
import { afterEach, describe, expect, it, vi } from "vitest";
import { createWorkflowTransport } from "./session_transport";
import { StreamGate, type Visibility } from "./stream_gate";

const tick = () => new Promise((resolve) => setTimeout(resolve, 5));

function fakeTab() {
  let hidden = false;
  const listeners = new Set<() => void>();
  const visibility: Visibility = {
    hidden: () => hidden,
    subscribe: (fn) => (listeners.add(fn), () => listeners.delete(fn)),
  };
  return {
    visibility,
    set(value: boolean) {
      hidden = value;
      for (const fn of [...listeners]) fn();
    },
  };
}

type Open = { url: string; headers: Record<string, string>; signal: AbortSignal; push: (text: string) => void };

/** Every fetch is a stream that stays open, delivering what the test pushes, until it is aborted. */
function fakeStreams() {
  const opened: Open[] = [];
  vi.stubGlobal("fetch", (url: string, init: RequestInit) => {
    const signal = init.signal as AbortSignal;
    let controller!: ReadableStreamDefaultController<Uint8Array>;
    const body = new ReadableStream<Uint8Array>({
      start: (c) => {
        controller = c;
        signal.addEventListener("abort", () => c.error(Object.assign(new Error("aborted"), { name: "AbortError" })), { once: true });
      },
    });
    const headers = Object.fromEntries(Object.entries((init.headers as Record<string, string>) || {}));
    opened.push({ url, headers, signal, push: (text) => controller.enqueue(new TextEncoder().encode(text)) });
    return Promise.resolve(new Response(body, { status: 200, headers: { "Content-Type": "text/event-stream" } }));
  });
  return opened;
}

const step = (cursor: number) =>
  `id: ${cursor}\nevent: step\ndata: ${JSON.stringify({ cursor, record: { run_id: "r1", step_id: `s${cursor}`, status: "completed", effect: { type: "llm_call" } } })}\n\n`;

afterEach(() => vi.unstubAllGlobals());

describe("a hidden tab holds no stream", () => {
  it("closes the ledger stream when hidden and reopens it after the last cursor when visible", async () => {
    const opened = fakeStreams();
    const tab = fakeTab();
    const transport = createWorkflowTransport({ onDeltaError: () => {}, streams: new StreamGate(tab.visibility) });
    const steps: number[] = [];
    const ended = vi.fn();
    void transport.streamLedger("r1", 0, (e) => steps.push(e.cursor), new AbortController().signal).then(ended);
    await tick();
    expect(opened).toHaveLength(1);
    expect(opened[0].url).toContain("after=0");
    opened[0].push(step(2) + step(3));
    await tick();
    tab.set(true);
    await tick();
    expect(opened[0].signal.aborted).toBe(true);
    // Hidden: nothing reopens, and the controller's call is still pending (its retry budget is untouched).
    await tick();
    expect(opened).toHaveLength(1);
    expect(ended).not.toHaveBeenCalled();
    tab.set(false);
    await tick();
    expect(opened).toHaveLength(2);
    expect(opened[1].url).toContain("after=3");
    opened[1].push(step(4));
    await tick();
    expect(steps).toEqual([2, 3, 4]);
  });

  it("closes the replay view stream when hidden and reopens it with the last Last-Event-ID", async () => {
    const opened = fakeStreams();
    const tab = fakeTab();
    const transport = createWorkflowTransport({ onDeltaError: () => {}, streams: new StreamGate(tab.visibility) });
    const frames: string[] = [];
    void transport.replayView!.streamView("r1", { lastEventId: "t0", onFrame: (f) => frames.push(String(f.id)), signal: new AbortController().signal });
    await tick();
    expect(opened[0].headers["Last-Event-ID"]).toBe("t0");
    opened[0].push(`id: t1\nevent: delta\ndata: {}\n\nid: t2\nevent: delta\ndata: {}\n\n`);
    await tick();
    tab.set(true);
    await tick();
    expect(opened[0].signal.aborted).toBe(true);
    expect(opened).toHaveLength(1);
    tab.set(false);
    await tick();
    expect(opened).toHaveLength(2);
    expect(opened[1].headers["Last-Event-ID"]).toBe("t2");
    expect(frames).toEqual(["t1", "t2"]);
  });

  it("a tab that starts hidden opens nothing until it is shown", async () => {
    const opened = fakeStreams();
    const tab = fakeTab();
    tab.set(true);
    const transport = createWorkflowTransport({ onDeltaError: () => {}, streams: new StreamGate(tab.visibility) });
    void transport.streamLedger("r1", 5, () => {}, new AbortController().signal);
    await tick();
    expect(opened).toHaveLength(0);
    tab.set(false);
    await tick();
    expect(opened).toHaveLength(1);
    expect(opened[0].url).toContain("after=5");
  });

  it("the controller's own abort ends a paused stream for good", async () => {
    const opened = fakeStreams();
    const tab = fakeTab();
    const transport = createWorkflowTransport({ onDeltaError: () => {}, streams: new StreamGate(tab.visibility) });
    const abort = new AbortController();
    const ended = vi.fn();
    void transport.streamLedger("r1", 0, () => {}, abort.signal).then(ended, ended);
    await tick();
    tab.set(true);
    await tick();
    abort.abort();
    await tick();
    expect(ended).toHaveBeenCalled();
    tab.set(false);
    await tick();
    expect(opened).toHaveLength(1);
  });

  it("a real stream error is still the controller's to handle", async () => {
    const tab = fakeTab();
    vi.stubGlobal("fetch", () => Promise.resolve(new Response("nope", { status: 503 })));
    const transport = createWorkflowTransport({ onDeltaError: () => {}, streams: new StreamGate(tab.visibility) });
    await expect(transport.streamLedger("r1", 0, () => {}, new AbortController().signal)).rejects.toThrow();
  });
});

describe("one stream per run in a tab", () => {
  it("a second ledger stream for the same run closes the first; another run is independent", async () => {
    const opened = fakeStreams();
    const gate = new StreamGate(fakeTab().visibility);
    const transport = createWorkflowTransport({ onDeltaError: () => {}, streams: gate });
    void transport.streamLedger("r1", 0, () => {}, new AbortController().signal);
    await tick();
    void transport.streamLedger("r1", 0, () => {}, new AbortController().signal);
    await tick();
    expect(opened[0].signal.aborted).toBe(true);
    expect(opened[1].signal.aborted).toBe(false);
    expect(gate.openCount).toBe(1);
    void transport.streamLedger("r2", 0, () => {}, new AbortController().signal);
    await tick();
    expect(gate.openCount).toBe(2);
  });
});

describe("the Activity panel opens no stream of its own", () => {
  it("only the session transport (through the gate) opens ledger streams; Activity reads the controller's snapshot", async () => {
    const { readdirSync, readFileSync } = await import("node:fs");
    const dir = new URL("./", import.meta.url);
    const users = readdirSync(dir)
      .filter((f) => /\.tsx?$/.test(f) && !f.includes(".test."))
      .filter((f) => readFileSync(new URL(f, dir), "utf8").includes("stream_ledger"));
    expect(users).toEqual(["session_transport.ts"]);
    for (const f of ["workspace_panels.tsx", "right_rail.tsx"]) expect(readFileSync(new URL(f, dir), "utf8")).not.toMatch(/ledger\/stream|streamLedger/);
  });
});
