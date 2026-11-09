// The app in replay-view mode, end to end inside the page: the app's own workflow transport
// (fetch → /view, /view/stream) feeding panel-chat's controller, then the app's attention badge,
// run status strip, Activity rows and approval card rendered from that snapshot. The gateway is
// faked at the fetch boundary with the view AbstractGateway 0.17.0 serves for the 2026-09-29
// delegated approval (conformance case a: the root waits on its agent loop, the child asks to
// approve write_file).
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkflowChat, WorkflowSessionController, presentInteraction, workflowPendingInteraction } from "@abstractframework/panel-chat";
import { createWorkflowTransport } from "./session_transport";
import { activityAttention, activityRowsOf } from "./workspace_panels";
import { RunStatusBar } from "./run_status_bar";
import view from "../lib/fixtures/replay_view/a_delegated_child_approval/view.json";
import answered from "../lib/fixtures/replay_view/l_answer_then_completion/view.json";

const ROOT = view.root_run_id;
const CHILD = view.interaction!.run_id;
const ETAG = `"${view.cursor.token}"`;
const tick = () => new Promise((resolve) => setTimeout(resolve, 5));

function fakeGateway() {
  const requests: { url: string; method: string; headers: Headers; body?: string }[] = [];
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init: RequestInit = {}) => {
    const url = String(input);
    const headers = new Headers(init.headers);
    requests.push({ url, method: init.method || "GET", headers, body: typeof init.body === "string" ? init.body : undefined });
    if (url.endsWith("/view/stream")) {
      // One `view` frame, then the stream stays open (a waiting run) until the controller aborts.
      const body = new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode(`id: ${view.cursor.token}\nevent: view\ndata: ${JSON.stringify(view)}\n\n`));
          init.signal?.addEventListener("abort", () => controller.error(new DOMException("aborted", "AbortError")));
        },
      });
      return new Response(body, { status: 200, headers: { "Content-Type": "text/event-stream" } });
    }
    if (url.endsWith("/view")) {
      if (headers.get("If-None-Match") === ETAG) return new Response(null, { status: 304, headers: { ETag: ETAG } });
      return new Response(JSON.stringify(view), { status: 200, headers: { ETag: ETAG, "Content-Type": "application/json" } });
    }
    if (url.includes("/ledger/stream")) {
      const body = new ReadableStream<Uint8Array>({ start: (c) => init.signal?.addEventListener("abort", () => c.error(new DOMException("aborted", "AbortError"))) });
      return new Response(body, { status: 200, headers: { "Content-Type": "text/event-stream" } });
    }
    if (url.endsWith(`/runs/${ROOT}`)) return new Response(JSON.stringify({ run_id: ROOT, status: "waiting", session_id: view.session_id }), { status: 200 });
    if (url.endsWith("/commands")) return new Response(JSON.stringify({ accepted: true }), { status: 200 });
    // panel-chat 0.6.2: the conversation's turns (the runtime's selection), one turn here.
    if (url.includes(`/runs/${ROOT}/history_bundle?include_session=true&include_subruns=false&ledger_max_items=1`))
      return new Response(JSON.stringify({ session: { session_id: view.session_id, turns: [{ run_id: ROOT }] } }), { status: 200 });
    throw new Error(`unexpected request ${init.method || "GET"} ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  vi.stubGlobal("document", { cookie: "" });
  return { requests };
}

afterEach(() => vi.unstubAllGlobals());

describe("replay-view mode through the app's transport", () => {
  it("badge, status strip, Activity and the approval card come from the view; the answer goes to the child", async () => {
    const { requests } = fakeGateway();
    const controller = new WorkflowSessionController(createWorkflowTransport({ onDeltaError: () => {} }));
    await controller.load(ROOT);
    for (let i = 0; i < 20 && controller.getSnapshot().connection !== "connected"; i += 1) await tick();
    const snapshot = controller.getSnapshot();
    expect(snapshot.source).toBe("replay_view");
    expect(snapshot.records).toHaveLength(0);
    // No ledger pages and no full history bundle; the only bundle read is the kit's small listing of
    // the conversation's turns (panel-chat 0.6.2 listSessionTurns: no sub-runs, one ledger item).
    expect(requests.some((r) => r.url.includes("/ledger?"))).toBe(false);
    const bundles = requests.filter((r) => r.url.includes("history_bundle"));
    expect(bundles).toHaveLength(1);
    expect(bundles[0].url).toContain("include_session=true&include_subruns=false&ledger_max_items=1");
    expect(snapshot.messages.some((m) => /could not be listed|are not shown/.test(String(m.content)))).toBe(false);
    expect(requests.find((r) => r.url.endsWith("/view/stream"))?.headers.get("Last-Event-ID")).toBe(view.cursor.token);

    // The rail badge: the child's approval is the one row waiting.
    const attention = activityAttention(snapshot.records, ROOT, snapshot.view);
    expect(attention.count).toBeGreaterThanOrEqual(1);
    expect(attention.hint).toMatch(/waiting/);
    const rows = activityRowsOf(snapshot.records, ROOT, snapshot.view);
    expect(rows.find((row) => row.statusLabel === "approval needed")?.runId).toBe(CHILD);

    // The status strip says what the person is asked.
    const strip = renderToStaticMarkup(<RunStatusBar active paused={false} snapshot={snapshot} onCommand={() => {}} onActivity={() => {}} />);
    expect(strip).toContain("Approval needed");

    // The approval card, answered with the view's command (the CHILD run, its wait key).
    const interaction = presentInteraction(workflowPendingInteraction(snapshot), controller, { records: snapshot.records, currentRun: snapshot.run });
    expect(interaction?.kind).toBe("tool-approval");
    const chat = renderToStaticMarkup(<WorkflowChat messages={snapshot.messages} draft="" onDraftChange={() => {}} onSend={() => {}} interaction={interaction} />);
    expect(chat).toContain(">Allow<");
    expect(chat).toContain("write_file");
    if (interaction?.kind !== "tool-approval") throw new Error("not an approval");
    await interaction.onApprove();
    const command = requests.find((r) => r.url.endsWith("/commands"));
    expect(JSON.parse(command!.body!)).toMatchObject({ type: "resume", run_id: CHILD, payload: { wait_key: view.interaction!.wait_key, payload: { approved: true } } });
    controller.dispose();
  });

  it("a conditional read of an unchanged view is a 304 with nothing to parse", async () => {
    const { requests } = fakeGateway();
    const rv = createWorkflowTransport({ onDeltaError: () => {} }).replayView!;
    const first = await rv.getView(ROOT);
    expect(first.notModified).toBe(false);
    const again = await rv.getView(ROOT, { ifNoneMatch: first.etag });
    expect(again).toMatchObject({ notModified: true, bytes: 0 });
    expect(requests.at(-1)?.headers.get("If-None-Match")).toBe(ETAG);
  });
});

describe("an agent that says nothing (a message addressed to entities, AbstractGateway c26d38a2)", () => {
  // The gateway tells the agent the mentioned entities answer for themselves; it may answer with an
  // empty reply, and its view then holds the person's words alone (test_gateway_agent_mentions_line:
  // roles == ["user"]). Code shows no bubble for it: the words, then the entities' answers.
  const silent = { ...answered, messages: answered.messages.filter((m) => m.role === "user") };
  const shown = async (served: typeof answered) => {
    const root = served.root_run_id;
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      const json = (body: unknown) => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json", ETag: `"${served.cursor.token}"` } });
      if (url.endsWith("/view")) return json(served);
      if (url.endsWith(`/runs/${root}`)) return json({ run_id: root, status: served.status, session_id: served.session_id });
      if (url.includes(`/runs/${root}/history_bundle`)) return json({ session: { session_id: served.session_id, turns: [{ run_id: root }] } });
      throw new Error(`unexpected request ${url}`);
    }));
    const controller = new WorkflowSessionController(createWorkflowTransport({ onDeltaError: () => {} }));
    await controller.load(root);
    for (let i = 0; i < 20 && controller.getSnapshot().status !== "completed"; i += 1) await tick();
    const snapshot = controller.getSnapshot();
    controller.dispose();
    vi.unstubAllGlobals();
    return snapshot;
  };

  it("its completed turn shows the person's words and no bubble; the same turn with words shows them (control)", async () => {
    const quiet = await shown(silent);
    expect(quiet.source).toBe("replay_view");
    expect(quiet.status).toBe("completed");
    expect(quiet.messages.map((m) => m.role)).toEqual(["user"]);
    const chat = renderToStaticMarkup(<WorkflowChat messages={quiet.messages} draft="" onDraftChange={() => {}} onSend={() => {}} />);
    expect(chat).toContain("Say hello");
    const spoken = await shown(answered);
    expect(spoken.messages.filter((m) => m.role === "assistant").map((m) => m.content)).toContain("Hello there.");
  });
});
