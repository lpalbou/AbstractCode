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
    const strip = renderToStaticMarkup(<RunStatusBar active paused={false} snapshot={snapshot} onRevokeApproval={() => {}} onCommand={() => {}} onActivity={() => {}} />);
    expect(strip).toContain("Approval needed");

    // The approval card, answered with the view's command (the CHILD run, its wait key).
    const interaction = presentInteraction(workflowPendingInteraction(snapshot), controller, { records: snapshot.records, currentRun: snapshot.run });
    expect(interaction?.kind).toBe("tool-approval");
    const chat = renderToStaticMarkup(<WorkflowChat messages={snapshot.messages} draft="" onDraftChange={() => {}} onSend={() => {}} interaction={interaction} />);
    expect(chat).toContain("Allow once");
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
