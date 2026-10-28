// The workflow transport reads the gateway's replay view through the app's authenticated proxy.
import { afterEach, describe, expect, it, vi } from "vitest";
import { ReplayViewFold } from "@abstractframework/panel-chat";
import { createWorkflowTransport } from "./session_transport";
import view from "../lib/fixtures/replay_view/a_delegated_child_approval/view.json";

const calls: { url: string; init: RequestInit }[] = [];
afterEach(() => {
  calls.length = 0;
  vi.unstubAllGlobals();
});

function stubFetch(respond: (url: string, init: RequestInit) => Response) {
  vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    return respond(url, init);
  });
}

describe("createWorkflowTransport().replayView", () => {
  it("is offered, so the controller renders from the view", () => {
    expect(createWorkflowTransport({ onDeltaError: () => {} }).replayView).toBeTruthy();
  });

  it("GET runs/{id}/view through the same-origin proxy, with the ETag", async () => {
    const body = JSON.stringify(view);
    stubFetch(() => new Response(body, { status: 200, headers: { ETag: `"${view.cursor.token}"` } }));
    const rv = createWorkflowTransport({ onDeltaError: () => {} }).replayView!;
    const read = await rv.getView(view.root_run_id);
    expect(calls[0].url).toBe(`api/gateway/runs/${view.root_run_id}/view`);
    expect(calls[0].init.credentials).toBe("same-origin");
    expect(read.notModified).toBe(false);
    expect(read.etag).toBe(`"${view.cursor.token}"`);
  });

  it("the stream resumes with Last-Event-ID and its frames fold", async () => {
    const frame = `id: ${view.cursor.token}\nevent: view\ndata: ${JSON.stringify(view)}\n\nevent: done\ndata: {}\n\n`;
    stubFetch(() => new Response(frame, { status: 200, headers: { "Content-Type": "text/event-stream" } }));
    const rv = createWorkflowTransport({ onDeltaError: () => {} }).replayView!;
    const fold = new ReplayViewFold();
    await rv.streamView(view.root_run_id, { lastEventId: "held-token", signal: new AbortController().signal, onFrame: (f) => fold.apply(f) });
    expect(calls[0].url).toBe(`api/gateway/runs/${view.root_run_id}/view/stream`);
    expect(new Headers(calls[0].init.headers).get("Last-Event-ID")).toBe("held-token");
    expect(fold.view?.cursor.token).toBe(view.cursor.token);
    expect(fold.terminal).toBe(true);
  });

  it("a gateway without the route is told apart from a missing run", async () => {
    stubFetch(() => new Response('{"detail":"Not Found"}', { status: 404 }));
    const rv = createWorkflowTransport({ onDeltaError: () => {} }).replayView!;
    await expect(rv.getView("r")).rejects.toMatchObject({ name: "ReplayViewUnavailableError" });
  });
});
