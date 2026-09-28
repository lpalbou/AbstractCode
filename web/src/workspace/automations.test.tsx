import React from "react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AutomationSummary, AutomationsClient, OccurrenceRow } from "@abstractframework/ui-kit";

import {
  AutomationsController,
  automationRowView,
  automationTarget,
  automationsAvailability,
  codeAutomationsClient,
  visibleAutomations,
  waitAnswerPayload,
  waitResumeCommand,
} from "./automations";
import { AutomationsSection, automationPanelProps, folderRefreshKey, folderTitle } from "./automations_view";

// The ui-kit's canonical wire fixtures, vendored byte-identical (checksums
// verified by the terminal client's contract test in this repo).
const FIXTURES = join(__dirname, "../../../tui/tests/fixtures/automations");
const fixture = (name: string) => JSON.parse(readFileSync(join(FIXTURES, name), "utf8"));
const list = (): AutomationSummary[] => fixture("list.json").items;
const occurrences = (): OccurrenceRow[] => fixture("occurrences.json").items;
const INBOX = "53443dd0-25c4-5fa8-bdad-e1ac3fdfff8e";
const byTitle = (title: string) => list().find((s) => s.title === title)!;
const NOW = Date.parse("2026-09-27T06:35:00Z");

describe("automation rows", () => {
  it("read what runs now from current_occurrence and the next run from next_fire_at", () => {
    const inbox = automationRowView(byTitle("Inbox triage"), NOW);
    expect(inbox.current).toBe("Run #7 running");
    expect(inbox.next).toBe("2026-09-27 07:00 UTC (in 25 min)");
    expect(inbox.attention).toBe("2 unseen · 2 waiting for you");
    const paused = automationRowView(byTitle("Weekly journal monitor"), NOW);
    expect(paused.current).toBeNull();
    expect(paused.next).toBe("none while paused");
    const legacy = list().find((s) => s.legacy)!;
    expect(automationRowView(legacy, NOW).legacy).toBe(true);
  });

  it("re-list the folder when a run finishes, not only when one starts", () => {
    const running = { occurrence_count: 1, current_occurrence: { index: 1, run_id: "r1", attempt: 1, status: "running" as const } };
    const done = { occurrence_count: 1, current_occurrence: null, last_occurrence: { run_id: "r1", index: 1, status: "completed", attempts: 1, fired_at: "t", finished_at: "t2", excerpt: "", notify: null } };
    expect(folderRefreshKey(running)).not.toBe(folderRefreshKey(done));
    // The run finished between two reads that both show nothing in flight.
    const seenRunning = { ...done, last_occurrence: { ...done.last_occurrence, status: "running", finished_at: undefined } };
    expect(folderRefreshKey(seenRunning)).not.toBe(folderRefreshKey(done));
  });

  it("name the folder pane after the automation or the run it shows", () => {
    const occ = occurrences();
    expect(folderTitle(INBOX, INBOX, occ)).toBe("Automation folder");
    expect(folderTitle(INBOX, occ[0].run_id, occ)).toBe(`Run #${occ[0].index} folder`);
  });

  it("hide archived automations until asked", () => {
    const items = list().map((s, i) => (i === 1 ? { ...s, status: "archived" as const } : s));
    expect(visibleAutomations(items, false).map((s) => s.title)).not.toContain(items[1].title);
    expect(visibleAutomations(items, true)).toHaveLength(items.length);
  });
});

describe("the create target", () => {
  it("is the gateway default agent, or the published workflow", () => {
    expect(automationTarget("@default", null)).toEqual({ flow_id: "@default", interface: "abstractcode.agent.v1" });
    const wf = { id: "x", workflowId: "b@1.2.0:f", bundleId: "b", bundleVersion: "1.2.0", flowId: "f", name: "B", description: "" };
    expect(automationTarget("x", wf)).toEqual({ bundle_ref: "b@1.2.0", flow_id: "f" });
    expect(automationTarget("x", null)).toBeNull();
  });
});

describe("availability", () => {
  it("follows capabilities.contracts.common.automations", () => {
    expect(automationsAvailability({ common: { automations: { available: true, version: 1 } } })).toEqual({ available: true, reason: "" });
    expect(automationsAvailability({ common: { automations: { available: false } } }).available).toBe(false);
    expect(automationsAvailability({}).reason).toContain("does not advertise the Automations API");
  });
});

describe("typed wait answers", () => {
  it("follow the kind and match the fixture's resume commands", () => {
    expect(waitAnswerPayload("tool_approval", { approved: true })).toEqual({ approved: true });
    expect(() => waitAnswerPayload("tool_approval", { response: "approve" })).toThrow();
    expect(waitAnswerPayload("ask_user", { response: "Tuesday" })).toEqual({ response: "Tuesday" });
    expect(() => waitAnswerPayload("mystery", {})).toThrow(/no known kind/);
    const commands = fixture("commands.json").items;
    for (const name of ["answer ask_user wait", "answer tool_approval wait"]) {
      const want = commands.find((c: any) => c.name === name).request.body;
      const built = waitResumeCommand(want.command_id, want.run_id, want.payload.wait_key, want.payload.payload);
      expect({ ...built, client_id: "web_pwa" }).toEqual(want);
    }
  });
});

describe("the client over the app proxy", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });
  it("sends the CSRF header and same-origin credentials", async () => {
    vi.stubGlobal("document", { cookie: "other=1; abstractcode_gateway_csrf=tok%2B1" });
    const calls: Array<{ url: string; init: RequestInit }> = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string, init: RequestInit) => {
        calls.push({ url, init });
        return new Response(JSON.stringify({ command_id: "c1", accepted: true, duplicate: false, seq: 1 }), { status: 200 });
      }),
    );
    await codeAutomationsClient().sendAutomationCommand(INBOX, { type: "automation.pause", command_id: "c1" });
    expect(calls[0].url).toBe(`api/gateway/automations/${INBOX}/commands`);
    expect(calls[0].init.credentials).toBe("same-origin");
    expect((calls[0].init.headers as Record<string, string>)["X-AbstractCode-CSRF"]).toBe("tok+1");
    expect(JSON.parse(String(calls[0].init.body))).toEqual({ command_id: "c1", type: "automation.pause" });
  });
});

function stubClient(overrides: Partial<AutomationsClient> = {}): AutomationsClient & { calls: string[] } {
  const calls: string[] = [];
  const summaries = list();
  const client: AutomationsClient = {
    listAutomations: vi.fn(async (q = {}) => {
      calls.push(`list ${q.cursor ?? ""}`);
      return q.cursor ? { items: summaries.slice(2), next_cursor: null } : { items: summaries.slice(0, 2), next_cursor: "p2" };
    }),
    getAutomation: vi.fn(async (id: string) => {
      calls.push(`get ${id}`);
      const summary = summaries.find((s) => s.automation_id === id)!;
      return { definition: { revision: 1 } as any, active_revision: 1, summary };
    }),
    listOccurrences: vi.fn(async (id: string) => {
      calls.push(`occurrences ${id}`);
      return { items: occurrences(), next_cursor: null };
    }),
    listTriggerSources: vi.fn(async () => fixture("trigger-sources.json")),
    sendAutomationCommand: vi.fn(async (id: string, r: any) => {
      calls.push(`command ${id} ${r.type} ${r.command_id}`);
      return { command_id: r.command_id, accepted: true, duplicate: false, seq: 1 };
    }),
    reviseAutomation: vi.fn(async () => ({ command_id: "r", accepted: true, duplicate: false, seq: 1 })),
    createAutomation: vi.fn(async (body: any) => {
      calls.push(`create ${body.request_id}`);
      return { automation_id: INBOX, revision: 1, summary: summaries[0] };
    }),
    discuss: vi.fn(async (id: string, r: any) => {
      calls.push(`discuss ${id} ${r.occurrence_index} ${r.request_id}`);
      return { session_id: "discussion-session:x", run_id: "run-x", session_kind: "discussion", workspace_root: "/own", mounted_workspace: "/auto" } as const;
    }),
    markSeen: vi.fn(async (id: string, c: string) => {
      calls.push(`seen ${id} ${c}`);
      return { attention_cursor: c };
    }),
    listAttention: vi.fn(async () => ({ items: [], next_cursor: null })),
    ...overrides,
  };
  return Object.assign(client, { calls });
}

describe("the controller", () => {
  it("reads every page, opens one automation, and re-reads after a command", async () => {
    const client = stubClient();
    const ctl = new AutomationsController(client, vi.fn(), []);
    await ctl.refresh();
    expect(client.calls.slice(0, 2)).toEqual(["list ", "list p2"]);
    expect(ctl.state.items).toHaveLength(4);
    await ctl.select(INBOX);
    expect(ctl.state.detail?.summary.title).toBe("Inbox triage");
    expect(ctl.state.detail?.occurrences.map((o) => o.index)[0]).toBe(7);
    client.calls.length = 0;
    await ctl.command(INBOX, "automation.stop_current", "cid-1");
    expect(client.calls[0]).toBe(`command ${INBOX} automation.stop_current cid-1`);
    expect(client.calls).toContain("list ");
    expect(client.calls).toContain(`get ${INBOX}`);
  });

  it("answers a wait with the payload of its kind, through the resume path", async () => {
    const answer = vi.fn(async () => {});
    const ctl = new AutomationsController(stubClient(), answer, []);
    await ctl.refresh();
    await ctl.select(INBOX);
    // The loaded occurrences carry one typed wait (run #7 asks a question).
    const ask = occurrences().flatMap((o) => o.waits).find((w) => w.kind === "ask_user");
    expect(ask).toBeTruthy();
    await ctl.answerWait(ask!.run_id, ask!.wait_key, { response: "Reply: Tuesday works" });
    expect(answer).toHaveBeenLastCalledWith(ask!.run_id, ask!.wait_key, { response: "Reply: Tuesday works" });
    await expect(ctl.answerWait(ask!.run_id, ask!.wait_key, { approved: true })).rejects.toMatchObject({ code: "client_error" });
  });

  it("creates with the dialog's request id and opens the new automation", async () => {
    const client = stubClient();
    const ctl = new AutomationsController(client, vi.fn(), []);
    await ctl.create({ request_id: "rid-9", title: "t", target: { flow_id: "@default", interface: "abstractcode.agent.v1" }, trigger: { source_id: "schedule", source_version: 1, config: { every: "5m" } } });
    expect(client.calls).toContain("create rid-9");
    expect(ctl.state.selectedId).toBe(INBOX);
  });

  it("wires Discuss to open the fork as this app's conversation", async () => {
    const client = stubClient();
    const ctl = new AutomationsController(client, vi.fn(), []);
    await ctl.refresh();
    await ctl.select(INBOX);
    const host = { openConversation: vi.fn(), openRun: vi.fn(), openWorkspace: vi.fn() };
    const props = automationPanelProps(ctl, host)!;
    await props.onDiscuss(6, "what changed?", { request_id: "req-6" });
    expect(client.calls).toContain(`discuss ${INBOX} 6 req-6`);
    expect(host.openConversation).toHaveBeenCalledWith("discussion-session:x", "run-x", expect.stringContaining("its own workspace /own"));
    await props.onCommand("automation.run_now", undefined, { command_id: "cid-2" });
    expect(client.calls).toContain(`command ${INBOX} automation.run_now cid-2`);
    await props.onSeen("att1:2");
    expect(client.calls).toContain(`seen ${INBOX} att1:2`);
    props.onOpenRun("run-7");
    expect(host.openRun).toHaveBeenCalledWith("run-7");
    props.onOpenWorkspace?.(INBOX);
    expect(host.openWorkspace).toHaveBeenCalledWith(INBOX);
  });
});

describe("the sidebar section", () => {
  const base = {
    available: { available: true, reason: "" },
    selectedId: "",
    onSelect: () => {},
    onNew: () => {},
    onRefresh: () => {},
    onShowArchived: () => {},
    nowMs: NOW,
  };
  const state = (items: AutomationSummary[], showArchived = false) => ({ ...new AutomationsController(stubClient(), vi.fn()).state, items, loaded: true, showArchived });

  it("lists state (the kit's word + icon), now and next as the gateway says", () => {
    const html = renderToStaticMarkup(<AutomationsSection {...base} state={state(list())} />);
    const label = (status: string, word: string) =>
      new RegExp(`data-state="${status}"><span class="af-auto__status-word">${word}</span><svg`);
    expect(html).toMatch(label("active", "Active"));
    expect(html).toMatch(label("paused", "Paused"));
    expect(html).toContain("now: Run #7 running");
    expect(html).toContain("next: none while paused");
    expect(html).toContain("legacy schedule");
  });

  it("hides archived rows behind a toggle", () => {
    const items = list().map((s, i) => (i === 1 ? { ...s, status: "archived" as const } : s));
    const hidden = renderToStaticMarkup(<AutomationsSection {...base} state={state(items)} />);
    expect(hidden).not.toContain(`data-automation-id="${items[1].automation_id}"`);
    expect(hidden).toContain("Show archived (1)");
    const shown = renderToStaticMarkup(<AutomationsSection {...base} state={state(items, true)} />);
    expect(shown).toContain('data-state="archived"');
  });

  it("says why when the gateway lacks the API", () => {
    const html = renderToStaticMarkup(
      <AutomationsSection {...base} available={automationsAvailability({})} state={state([])} />,
    );
    expect(html).toContain('data-unavailable="true"');
    expect(html).toContain("does not advertise the Automations API");
  });
});
