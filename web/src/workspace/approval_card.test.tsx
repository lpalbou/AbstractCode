// The tool permission card (operator ruling 2026-11-07): exactly Deny, Allow (this tool) and Allow All
// (every tool), the two allows for the rest of THIS SESSION only, stored in the gateway's session tool
// policy; nothing the card does writes the app's default tool settings. End to end inside the page: the
// app's own transport and session-policy client (built exactly as app.tsx builds it) feeding
// panel-chat's controller, the gateway faked at the fetch boundary with the view AbstractGateway
// 0.17.0 serves for a delegated approval (the child asks to approve write_file).
import { readFileSync } from "node:fs";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkflowChat, WorkflowSessionController, presentInteraction, workflowPendingInteraction } from "@abstractframework/panel-chat";
import { sessionToolPolicyCapability } from "@abstractframework/ui-kit";
import { createWorkflowTransport } from "./session_transport";
import { createCodeSessionPolicyClient, SESSION_POLICY_TIMEOUT_MS } from "./session_policy_client";
import view from "../lib/fixtures/replay_view/a_delegated_child_approval/view.json";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const layersSource = readFileSync(new URL("./tool_layers.ts", import.meta.url), "utf8");
const ROOT = view.root_run_id;
const CHILD = view.interaction!.run_id;
const SESSION = view.session_id as string;
const ETAG = `"${view.cursor.token}"`;
const tick = () => new Promise((resolve) => setTimeout(resolve, 5));

function fakeGateway(options: { hangPolicy?: boolean } = {}) {
  const requests: { url: string; method: string; body?: string; signal?: AbortSignal | null }[] = [];
  let revision = 0;
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init: RequestInit = {}) => {
    const url = String(input);
    const method = init.method || "GET";
    requests.push({ url, method, body: typeof init.body === "string" ? init.body : undefined, signal: init.signal });
    // A gateway that never answers the policy write (and ignores the abort).
    if (options.hangPolicy && url.includes("/tool-policy")) return new Promise<Response>(() => {});
    if (url.includes(`/sessions/${SESSION}/tool-policy`)) {
      const body = typeof init.body === "string" ? JSON.parse(init.body) : {};
      revision += 1;
      const approve = body.approve?.all ? { write_file: "all", read_file: "all", execute_command: "all" } : Object.fromEntries((body.approve?.tools || []).map((n: string) => [n, "tool"]));
      const policy = { version: 1, session_id: SESSION, revision, approve, deny: {}, ask: [], clamped: [] };
      return new Response(JSON.stringify({ policy, revision, scope: { approve: "every run of this conversation", deny: "every run" } }), { status: 200 });
    }
    if (url.endsWith("/view/stream")) {
      const body = new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode(`id: ${view.cursor.token}\nevent: view\ndata: ${JSON.stringify(view)}\n\n`));
          init.signal?.addEventListener("abort", () => controller.error(new DOMException("aborted", "AbortError")));
        },
      });
      return new Response(body, { status: 200, headers: { "Content-Type": "text/event-stream" } });
    }
    if (url.endsWith("/view")) return new Response(JSON.stringify(view), { status: 200, headers: { ETag: ETAG, "Content-Type": "application/json" } });
    if (url.includes("/ledger/stream")) {
      const body = new ReadableStream<Uint8Array>({ start: (c) => init.signal?.addEventListener("abort", () => c.error(new DOMException("aborted", "AbortError"))) });
      return new Response(body, { status: 200, headers: { "Content-Type": "text/event-stream" } });
    }
    if (url.endsWith(`/runs/${ROOT}`)) return new Response(JSON.stringify({ run_id: ROOT, status: "waiting", session_id: SESSION }), { status: 200 });
    if (url.endsWith("/commands")) return new Response(JSON.stringify({ accepted: true }), { status: 200 });
    if (url.includes(`/runs/${ROOT}/history_bundle?include_session=true&include_subruns=false&ledger_max_items=1`))
      return new Response(JSON.stringify({ session: { session_id: SESSION, turns: [{ run_id: ROOT }] } }), { status: 200 });
    throw new Error(`unexpected request ${method} ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  vi.stubGlobal("document", { cookie: "" });
  // The app's tool defaults live in this browser's storage: the card must never write there.
  const storage = { getItem: vi.fn(() => null), setItem: vi.fn(), removeItem: vi.fn(), clear: vi.fn(), key: vi.fn(() => null), length: 0 };
  vi.stubGlobal("localStorage", storage);
  return { requests, storage };
}

async function openApproval(options: { hangPolicy?: boolean; timeoutMs?: number } = {}) {
  const gw = fakeGateway(options);
  const controller = new WorkflowSessionController(createWorkflowTransport({ onDeltaError: () => {} }));
  // The session-policy client app.tsx injects (createCodeSessionPolicyClient, pinned in session_policy.test.ts).
  const capability = sessionToolPolicyCapability({ capabilities: { session_tool_policy: { supported: true, version: 1, methods: ["GET", "PUT", "POST", "DELETE"] } } });
  controller.setSessionToolPolicyClient(createCodeSessionPolicyClient(SESSION, capability, options.timeoutMs ? { timeoutMs: options.timeoutMs } : {}));
  await controller.load(ROOT);
  for (let i = 0; i < 20 && controller.getSnapshot().connection !== "connected"; i += 1) await tick();
  const snapshot = controller.getSnapshot();
  // The options app.tsx passes (pinned below against its source).
  const interaction = presentInteraction(workflowPendingInteraction(snapshot), controller, { records: snapshot.records, currentRun: snapshot.run, sessionPolicy: true });
  if (interaction?.kind !== "tool-approval") throw new Error("not an approval");
  return { ...gw, controller, snapshot, interaction };
}

afterEach(() => vi.unstubAllGlobals());

describe("the tool permission card: Deny, Allow, Allow All, for this session only", () => {
  it("app.tsx asks for the session grant and nothing else; the per-browser allow-all is gone", () => {
    expect(appSource).toMatch(/presentInteraction\(pendingInteraction, controller, \{\s*records: snapshot\.records,\s*currentRun: snapshot\.run,\s*sessionPolicy: true,\s*\}\)/);
    for (const gone of ["onPermissionsAll", "approveAllScope", "withAllEnabledAuto", "allEnabledAuto", "withoutAutoChoices", "Auto: every enabled tool", "Allow all enabled tools"]) {
      expect(appSource).not.toContain(gone);
      expect(layersSource).not.toContain(gone);
    }
  });

  it("renders exactly three choices and no explanatory blocks", async () => {
    const { controller, snapshot, interaction } = await openApproval();
    const chat = renderToStaticMarkup(<WorkflowChat messages={snapshot.messages} draft="" onDraftChange={() => {}} onSend={() => {}} interaction={interaction} />);
    const decisions = [...chat.matchAll(/<button[^>]*data-decision="([^"]+)"[^>]*>([^<]*)</g)].map((m) => [m[1], m[2]]);
    expect(decisions).toEqual([["deny", "Deny"], ["approve", "Allow"], ["approve-all", "Allow All"]]);
    expect(chat).toContain("1 action needs permission");
    expect(chat).toContain("Review full tool arguments (JSON)");
    for (const gone of ["Allow once", "Allow for this chat", "Allow all", "this browser", "Review the targets below", "The broader choices", "What the broader choices allow", "<summary>More</summary>", "Always deny", "covers this request only"])
      expect(chat).not.toContain(gone);
    controller.dispose();
  });

  it("Allow stores this tool for the session on the gateway, then answers; the app's defaults are untouched", async () => {
    const { controller, requests, storage, interaction } = await openApproval();
    await interaction.onApprove();
    const writes = requests.filter((r) => r.url.includes("/tool-policy"));
    expect(writes).toHaveLength(1);
    expect(writes[0].url).toContain(`/sessions/${SESSION}/tool-policy`);
    expect(writes[0].method).toBe("POST");
    expect(JSON.parse(writes[0].body!)).toEqual({ approve: { tools: ["write_file"] }, client: "panel-chat" });
    const command = requests.find((r) => r.url.endsWith("/commands"));
    expect(requests.indexOf(command!)).toBeGreaterThan(requests.indexOf(writes[0]));
    expect(JSON.parse(command!.body!)).toMatchObject({ type: "resume", run_id: CHILD, payload: { payload: { approved: true } } });
    expect(storage.setItem).not.toHaveBeenCalled();
    controller.dispose();
  });

  it("Allow All stores every tool for the session on the gateway, then answers; the app's defaults are untouched", async () => {
    const { controller, requests, storage, interaction } = await openApproval();
    await interaction.onApproveAll!();
    const writes = requests.filter((r) => r.url.includes("/tool-policy"));
    expect(writes.map((r) => [r.method, JSON.parse(r.body!)])).toEqual([["POST", { approve: { all: true }, client: "panel-chat" }]]);
    expect(JSON.parse(requests.find((r) => r.url.endsWith("/commands"))!.body!)).toMatchObject({ run_id: CHILD, payload: { payload: { approved: true } } });
    expect(storage.setItem).not.toHaveBeenCalled();
    controller.dispose();
  });

  it("a gateway that never answers the policy write: the click fails with the reason after the bound, nothing is answered, Deny still works", async () => {
    expect(SESSION_POLICY_TIMEOUT_MS).toBe(15_000);
    const { controller, requests, interaction } = await openApproval({ hangPolicy: true, timeoutMs: 60 });
    const started = Date.now();
    await expect(interaction.onApprove()).rejects.toThrow(/did not answer within/);
    expect(Date.now() - started).toBeLessThan(2_000);
    const write = requests.find((r) => r.url.includes("/tool-policy"));
    expect(write?.signal?.aborted).toBe(true);
    expect(requests.some((r) => r.url.endsWith("/commands"))).toBe(false);
    await expect(interaction.onApproveAll!()).rejects.toThrow(/did not answer within/);
    await interaction.onDeny();
    expect(JSON.parse(requests.find((r) => r.url.endsWith("/commands"))!.body!)).toMatchObject({ run_id: CHILD, payload: { payload: { approved: false } } });
    controller.dispose();
  });

  it("Deny refuses this request and writes no policy", async () => {
    const { controller, requests, storage, interaction } = await openApproval();
    await interaction.onDeny();
    expect(requests.some((r) => r.url.includes("/tool-policy"))).toBe(false);
    expect(JSON.parse(requests.find((r) => r.url.endsWith("/commands"))!.body!)).toMatchObject({ run_id: CHILD, payload: { payload: { approved: false } } });
    expect(storage.setItem).not.toHaveBeenCalled();
    controller.dispose();
  });
});
