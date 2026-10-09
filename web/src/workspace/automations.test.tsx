import React from "react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AutomationSummary, AutomationsClient, OccurrenceRow } from "@abstractframework/ui-kit";

import {
  AutomationsController,
  myEmailConsoleUrl,
  automationRowView,
  automationTarget,
  automationsAvailability,
  codeAutomationsClient,
  visibleAutomations,
  archivedAutomationCount,
  waitAnswerPayload,
  waitResumeCommand,
} from "./automations";
import { AutomationsSection, NewAutomationDialog, automationPanelProps, folderRefreshKey, folderTitle } from "./automations_view";
import { AutomationCard } from "./sidebar_cards";
import { servedSummary } from "./served_summary";

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
  it("read what runs now from current_occurrence and the next run from the SERVED next_run_at/next_run_local (R16.1)", () => {
    const inbox = automationRowView(byTitle("Inbox triage"), NOW);
    expect(inbox.current).toBe("Run #7 running");
    expect(inbox.next).toBe("2026-09-27 09:00 Europe/Paris (in 25 min)");
    // A changed gateway value changes the row; next_fire_at alone is never read (nothing computed here).
    expect(automationRowView({ ...byTitle("Inbox triage"), next_run_at: "2026-09-27T08:05:00+00:00", next_run_local: "2026-09-27T10:05:00+02:00" }, NOW).next).toBe("2026-09-27 10:05 Europe/Paris (in 1 h 30 min)");
    expect(automationRowView({ ...byTitle("Inbox triage"), next_run_at: undefined, next_run_local: undefined }, NOW).next).toBe("none scheduled");
    const brief = automationRowView(byTitle("Morning briefing"), NOW);
    expect(brief.cadence).toBe("Every day at 08:00 (Europe/Paris)");
    expect(brief.next).toBe("2026-09-28 08:00 Europe/Paris (in 23 h 25 min)");
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

  it("archived automations are never list rows; N is the gateway's `archived_automations`", () => {
    const items = list().map((s, i) => (i === 1 ? { ...s, status: "archived" as const } : s));
    expect(visibleAutomations(items).map((s) => s.title)).not.toContain(items[1].title);
    expect(visibleAutomations(items)).toHaveLength(items.length - 1);
    expect(archivedAutomationCount({ items: [], next_cursor: null, archived_automations: 6 })).toBe(6);
    expect(archivedAutomationCount({ items: [], next_cursor: null })).toBe(0);
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
    previewSchedule: vi.fn(async (trigger: any) => {
      calls.push(`preview ${JSON.stringify(trigger.config)}`);
      return { trigger, time_zone: "Europe/Paris", schedule_rule_text: "Every day at 08:00 (Europe/Paris)", schedule_text: "Every day at 08:00 (Europe/Paris) · next Mon 28 Sep 08:00", next_run_at: "2026-09-28T06:00:00+00:00", next_run_local: "2026-09-28T08:00:00+02:00", first_run_sentence: "Runs every day at 08:00 (Europe/Paris), first run Mon 28 Sep 08:00." };
    }),
    getMyEmail: vi.fn(async () => {
      calls.push("me/email");
      return { configured: true, enabled: true, admin_enabled: true, effective_enabled: true };
    }),
    ...overrides,
  };
  return Object.assign(client, { calls });
}

describe("the controller", () => {
  it("R16.1: previewSchedule is the gateway's schedule-preview, and the panel (Edit form) gets it", async () => {
    const client = stubClient();
    const ctl = new AutomationsController(client, vi.fn(), []);
    const trigger = { source_id: "schedule", source_version: 2, config: { kind: "daily", at: "08:00" } };
    expect((await ctl.previewSchedule(trigger)).first_run_sentence).toBe("Runs every day at 08:00 (Europe/Paris), first run Mon 28 Sep 08:00.");
    expect(client.calls).toEqual(['preview {"kind":"daily","at":"08:00"}']);
    await ctl.refresh();
    await ctl.select(INBOX);
    const p = automationPanelProps(ctl, { openWorkspace() {}, openConversation() {}, openRun() {} } as any)!;
    expect(p.previewSchedule).toBe(ctl.previewSchedule);
  });

  it("reads every page, opens one automation, and re-reads after a command", async () => {
    const client = stubClient();
    const ctl = new AutomationsController(client, vi.fn(), []);
    await ctl.refresh();
    expect(client.calls.slice(0, 2)).toEqual(["list ", "list p2"]);
    expect(ctl.state.items).toHaveLength(5);
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
    nowMs: NOW,
  };
  const state = (items: AutomationSummary[]) => ({ ...new AutomationsController(stubClient(), vi.fn()).state, items, loaded: true });

  it("round 5 cards: name, the waiting badge, `every · last` then `next` + the Active switch on the second line", () => {
    const html = renderToStaticMarkup(<AutomationsSection {...base} state={state(list())} />);
    const card = (title: string) => {
      const id = list().find((s) => s.title === title)!.automation_id;
      const at = html.indexOf(`data-automation-id="${id}"`);
      expect(at, title).toBeGreaterThan(-1);
      return html.slice(at, html.indexOf('</div>', html.indexOf('role="switch"', at)));
    };
    const field = (name: string) => (c: string) => new RegExp(`data-field="${name}">([\\s\\S]*?)<\\/small>`).exec(c)?.[1].replace(/<[^>]+>/g, "");
    const line1 = field("timing");
    const line2 = field("next");
    const timing = (c: string) => [line1(c), line2(c)].filter(Boolean).join(" / ");
    // The operator's line, from the gateway's facts only (NOW = 2026-09-27 06:35 UTC).
    // Run #7 waits for an approval: "waiting since", never "running now" beside the badge.
    expect(timing(card("Inbox triage"))).toBe("Every 30 minutes (UTC) · waiting since 4 min / next in 25 min");
    // Line 1 starts with the ↻ icon for a schedule; line 2 holds `next` and the Active switch, in that order.
    expect(card("Inbox triage")).toMatch(/data-field="timing"><svg[^>]*code-card-cadence-icon/);
    expect(card("Inbox triage")).toMatch(/<div class="code-card-line2"><small class="code-card-meta" data-field="next">next in 25 min<\/small><button[^>]*role="switch"/);
    const executing = { ...list()[0], attention: { ...list()[0].attention, pending_waits: 0, unseen_count: 0 } };
    const run = renderToStaticMarkup(<AutomationsSection {...base} state={state([executing])} />);
    expect(timing(run)).toBe("Every 30 minutes (UTC) · running now / next in 25 min");
    expect(timing(card("AI news monitor"))).toBe("Every 8 hours (UTC) · last 6 h ago / next in 1 h");
    // Nothing scheduled (paused): line 2 keeps only the switch.
    expect(timing(card("Weekly journal monitor"))).toBe(`${"Every 7 days (UTC) \u00b7 12 runs max"} · last 6 d ago`);
    expect(card("Weekly journal monitor")).toContain('data-field="next"></small>');
    // An approval is pending only on Inbox triage (pending_waits 2): the badge there and nowhere else.
    expect(card("Inbox triage")).toContain('data-field="waiting">waiting for you<');
    expect(card("AI news monitor")).not.toContain("waiting for you");
    // The state is a switch labelled by the feature: on = active, off = paused (never a verb).
    // Icon-only in the card (the name keeps its full row): "Active" is the accessible name and tooltip.
    expect(card("AI news monitor")).toMatch(/role="switch" class="af-switch af-switch--sm code-card-switch code-switch-icon-only" data-action="active" aria-checked="true" aria-label="Active"[^>]*title="Active\n/);
    expect(card("AI news monitor")).toContain('<span class="code-visually-hidden">Active</span>');
    expect(card("Weekly journal monitor")).toMatch(/data-action="active" aria-checked="false"/);
    expect(html).not.toMatch(/>(Pause|Resume)</);
    // No year and no seconds anywhere on the cards.
    for (const line of html.matchAll(/data-field="timing">([\s\S]*?)<\/small>/g)) expect(line[1].replace(/<[^>]+>/g, "")).not.toMatch(/20\d\d|\bsec|\d+ ?s\b/);
    // The legacy row's switch is unavailable, with the kit's reason as its tooltip.
    expect(card("echo")).toMatch(/aria-disabled="true"[^>]*title="Legacy schedule/);
  });

  it("the card body selects and the switch flips Active through the controller (never nested buttons)", () => {
    const onSelect = vi.fn();
    const onToggleActive = vi.fn();
    const items = list();
    const html = renderToStaticMarkup(<AutomationsSection {...base} onSelect={onSelect} onToggleActive={onToggleActive} state={state(items)} />);
    expect(html).not.toMatch(/<button[^>]*>(?:(?!<\/button>)[\s\S])*<button/);
    const tree = AutomationsSection({ ...base, onSelect, onToggleActive, state: state(items) } as any);
    const cards: any[] = [];
    const visit = (n: any) => {
      if (Array.isArray(n)) return n.forEach(visit);
      if (!React.isValidElement(n)) return;
      if (n.type === AutomationCard) cards.push(n);
      visit((n.props as any).children);
    };
    visit(tree);
    expect(cards).toHaveLength(items.length);
    (cards[1].props as any).onSelect();
    expect(onSelect).toHaveBeenCalledWith(items[1].automation_id);
    (cards[1].props as any).onToggleActive();
    expect(onToggleActive).toHaveBeenCalledWith(items[1]);
  });

  it("toggleActive pauses an active automation, resumes a paused one, and keeps a refusal for the card", async () => {
    const client = stubClient();
    const ctl = new AutomationsController(client, vi.fn(), []);
    await ctl.toggleActive({ automation_id: "a1", status: "active" });
    await ctl.toggleActive({ automation_id: "a2", status: "paused" });
    const sent = (client.sendAutomationCommand as any).mock.calls.map((c: any[]) => [c[0], c[1].type]);
    expect(sent).toEqual([["a1", "automation.pause"], ["a2", "automation.resume"]]);
    const refusing = stubClient({ sendAutomationCommand: vi.fn(async () => Promise.reject({ status: 409, code: "not_permitted", message: "no" })) });
    const ctl2 = new AutomationsController(refusing, vi.fn(), []);
    await expect(ctl2.toggleActive({ automation_id: "a3", status: "active" })).rejects.toMatchObject({ code: "not_permitted" });
    expect(ctl2.state.rowError).toMatchObject({ automationId: "a3", error: { code: "not_permitted" } });
    const items = list().map((s, i) => (i === 0 ? { ...s, automation_id: "a3" } : s));
    const html = renderToStaticMarkup(<AutomationsSection {...base} state={{ ...state(items), rowError: ctl2.state.rowError }} />);
    expect(html).toMatch(/data-automation-id="a3"[\s\S]*?<\/div><p class="code-inline-error" role="alert" data-code="not_permitted">/);
  });

  it("R5: no Show archived switch; a quiet `Archived · N` footer at the end opens them inline with Unarchive", () => {
    const all = list().map((s, i) => (i === 1 || i === 2 ? { ...s, status: "archived" as const } : s));
    const items = all;
    const live = all.filter((s) => s.status !== "archived");
    const withArchived = (archived: AutomationSummary[] | null) => ({ ...state(live), archivedCount: 2, archived });
    const closed = renderToStaticMarkup(<AutomationsSection {...base} state={withArchived(null)} />);
    expect(closed).not.toMatch(/Show archived|show-archived/);
    expect(closed).not.toContain(`data-automation-id="${items[1].automation_id}"`);
    // The footer is the LAST thing in the list region.
    expect(closed).toMatch(/<div class="code-archived" data-list="automations" data-open="false"><button type="button" class="code-archived-toggle" aria-expanded="false" aria-controls="code-archived-automations"><span>Archived · 2<\/span>[\s\S]*?<\/button><\/div><\/div><\/section>$/);
    expect(closed).not.toContain('data-action="unarchive"');
    const onUnarchive = vi.fn();
    expect(renderToStaticMarkup(<AutomationsSection {...base} archivedOpen state={withArchived(null)} />)).toContain("Loading archived automations…");
    const open = renderToStaticMarkup(<AutomationsSection {...base} archivedOpen onUnarchive={onUnarchive} state={withArchived([all[1], all[2]])} />);
    expect(open).toContain('aria-expanded="true"');
    for (const s of [items[1], items[2]])
      expect(open).toMatch(new RegExp(`<li class="code-archived-row" data-id="${s.automation_id}">[\\s\\S]*?data-action="unarchive"[^>]*>[\\s\\S]*?<span>Unarchive</span>`));
    // Nothing archived: no footer at all.
    expect(renderToStaticMarkup(<AutomationsSection {...base} state={{ ...state(live), archivedCount: 0, archived: [] }} />)).not.toContain("code-archived");
  });

  it("refresh reads the gateway's count; the opened footer lists `status=archived` (all pages) and stays fresh", async () => {
    const archivedRow = { ...list()[1], status: "archived" as const };
    const client = stubClient({
      listAutomations: vi.fn(async (q: any = {}) =>
        q.status === "archived" ? { items: [archivedRow], next_cursor: null } : ({ items: list().slice(0, 1), next_cursor: null, archived_automations: 1 } as any),
      ),
    });
    const ctl = new AutomationsController(client, vi.fn(), []);
    await ctl.refresh();
    expect(ctl.state.archivedCount).toBe(1);
    expect(ctl.state.archived).toBeNull();
    expect((client.listAutomations as any).mock.calls.every((c: any[]) => !c[0]?.status)).toBe(true);
    await ctl.loadArchived();
    expect(ctl.state.archived?.map((s) => s.automation_id)).toEqual([archivedRow.automation_id]);
    expect((client.listAutomations as any).mock.calls.at(-1)[0]).toMatchObject({ status: "archived" });
  });

  it("Unarchive sends the gateway's automation.unarchive (the kit control) and keeps a refusal for the row", async () => {
    const client = stubClient();
    const ctl = new AutomationsController(client, vi.fn(), []);
    await ctl.unarchive("a9");
    expect((client.sendAutomationCommand as any).mock.calls.map((c: any[]) => [c[0], c[1].type])).toEqual([["a9", "automation.unarchive"]]);
    const refusing = stubClient({ sendAutomationCommand: vi.fn(async () => Promise.reject({ status: 409, code: "not_permitted", message: "no" })) });
    const ctl2 = new AutomationsController(refusing, vi.fn(), []);
    await expect(ctl2.unarchive("a3")).rejects.toMatchObject({ code: "not_permitted" });
    expect(ctl2.state.rowError).toMatchObject({ automationId: "a3" });
  });

  it("says why when the gateway lacks the API", () => {
    const html = renderToStaticMarkup(
      <AutomationsSection {...base} available={automationsAvailability({})} state={state([])} />,
    );
    expect(html).toContain('data-unavailable="true"');
    expect(html).toContain("does not advertise the Automations API");
  });
});

describe("email automations (framework backlog 0992 WP6)", () => {
  it("reads GET /me/email with the list, and an unreadable answer counts as not set up", async () => {
    const client = stubClient();
    const ctl = new AutomationsController(client, vi.fn(), []);
    await ctl.refresh();
    expect(client.calls).toContain("me/email");
    expect(ctl.state.emailStatus?.effective_enabled).toBe(true);
    const failing = stubClient({ getMyEmail: vi.fn(async () => Promise.reject({ status: 403, code: "email_principal_refused", message: "no" })) });
    const ctl2 = new AutomationsController(failing, vi.fn(), []);
    await ctl2.refresh();
    expect(ctl2.state.emailStatus).toBeNull();
    expect(ctl2.state.listError).toBeNull();
  });

  it("opens My email in the gateway console's Users tab", () => {
    expect(myEmailConsoleUrl("http://127.0.0.1:18850")).toBe("http://127.0.0.1:18850/console#users");
    expect(myEmailConsoleUrl("https://gw.example.test/prefix/")).toBe("https://gw.example.test/prefix/console#users");
    expect(myEmailConsoleUrl("")).toBeNull();
    expect(myEmailConsoleUrl("not a url")).toBeNull();
  });

  const dialog = (ctl: AutomationsController, onOpenMyEmail?: () => void) =>
    renderToStaticMarkup(
      <NewAutomationDialog open onClose={() => {}} target={{ flow_id: "@default", interface: "abstractcode.agent.v1" }} workflowLabel="the agent" initialPrompt="Summarise new mail" ctl={ctl} onCreated={() => {}} onOpenMyEmail={onOpenMyEmail} />,
    );

  it("the New automation dialog offers When an email arrives only with a usable account", async () => {
    const ctl = new AutomationsController(stubClient(), vi.fn(), []);
    await ctl.refresh();
    const ok = dialog(ctl, () => {});
    expect(ok).toMatch(/<input type="radio" name="[^"]+" value="email"\/> When an email arrives/);
    // "Email me the result" is the kit's switch (ui-kit 0.3.3), labelled by the feature.
    expect(ok).toMatch(/role="switch"[^>]*data-action="notify-email"/);
    expect(ok).not.toContain('type="checkbox"');
    expect(ok).not.toContain('data-email-setup="missing"');
    const none = new AutomationsController(stubClient({ getMyEmail: vi.fn(async () => ({ configured: false, effective_enabled: false })) }), vi.fn(), []);
    await none.refresh();
    const html = dialog(none, () => {});
    expect(html).toMatch(/disabled="" value="email"\/> When an email arrives/);
    expect(html).toContain("Connect a mailbox first — ");
    expect(html).toContain('data-action="open-my-email"');
  });

  it("hands the email status and My email to the panel's Edit form", async () => {
    const ctl = new AutomationsController(stubClient(), vi.fn(), []);
    await ctl.refresh();
    await ctl.select(INBOX);
    const openMyEmail = vi.fn();
    const props = automationPanelProps(ctl, { openConversation: vi.fn(), openRun: vi.fn(), openWorkspace: vi.fn(), openMyEmail })!;
    expect(props.emailStatus?.effective_enabled).toBe(true);
    props.onOpenMyEmail?.();
    expect(openMyEmail).toHaveBeenCalledTimes(1);
  });

  it("sends the dialog's email body unchanged", async () => {
    const client = stubClient();
    const ctl = new AutomationsController(client, vi.fn(), []);
    const body = {
      request_id: "rid-e",
      title: "Invoices",
      target: { flow_id: "@default" as const, interface: "abstractcode.agent.v1", input_data: { prompt: "Summarise invoices" } },
      trigger: { source_id: "email.received", source_version: 1, config: { uses_model: true, every: "1h", max_batch: 100, filter: { from_domain_in: ["example.test"] } } },
      policy: { tool_approval: "auto" as const, email_allowed_recipients: ["self", "boss@example.test"] },
      notify: { channels: ["console" as const, "email" as const] },
    };
    await ctl.create(body);
    expect(client.createAutomation).toHaveBeenCalledWith(body);
  });
});

// 2026-10-09 operator report: a gateway before round 16 (0.13.x) serves no time_zone /
// next_run_at / next_run_local / schedule_text / schedule_rule_text — only next_fire_at.
// The row below is a COPY of that live summary's shape (tui/tests/fixtures/legacy_summary).
describe("a pre-round-16 gateway row (served fields optional)", () => {
  const legacyPage = () => JSON.parse(readFileSync(join(__dirname, "../../../tui/tests/fixtures/legacy_summary/list-gateway-0.13.json"), "utf8"));
  const LEGACY_NOW = Date.parse("2026-10-09T16:53:29Z");

  it("normalizes: next run from next_fire_at (UTC), rule and sentence '—', served values never overridden", () => {
    const raw = legacyPage().items[0];
    const s = servedSummary(raw);
    expect(s.next_run_at).toBe("2026-10-09T19:53:29.622724+00:00");
    expect(s.next_run_local).toBe("2026-10-09T19:53:29.622724+00:00");
    expect(s.time_zone).toBe("UTC");
    expect(s.schedule_rule_text).toBe("—");
    expect(s.schedule_text).toBe("—");
    const row = automationRowView(s, LEGACY_NOW);
    expect(row.cadence).toBe("—");
    expect(row.next).toBe("2026-10-09 19:53 UTC (in 3 h)");
    // Invalid types are as absent as missing ones.
    const bad = servedSummary({ ...raw, time_zone: 5, schedule_rule_text: ["x"], next_run_local: null });
    expect([bad.time_zone, bad.schedule_rule_text]).toEqual(["UTC", "—"]);
    // No next run served: none scheduled (never a computed one).
    const { next_fire_at: _drop, ...noNext } = raw;
    expect(automationRowView(servedSummary(noNext), LEGACY_NOW).next).toBe("none scheduled");
    // A round-16 row keeps its served words.
    const r16 = servedSummary({ ...raw, time_zone: "Europe/Paris", next_run_at: raw.next_fire_at, next_run_local: "2026-10-09T21:53:29.622724+02:00", schedule_rule_text: "Every 24 hours (UTC)", schedule_text: "Every 24 hours (UTC) · next Fri 9 Oct 21:53" });
    expect(automationRowView(r16, LEGACY_NOW)).toMatchObject({ cadence: "Every 24 hours (UTC)", next: "2026-10-09 21:53 Europe/Paris (in 3 h)" });
  });

  it("the controller lists and opens it; the card renders '—' and the next run", async () => {
    const page = legacyPage();
    const raw = page.items[0];
    const client = stubClient({
      listAutomations: vi.fn(async () => page),
      getAutomation: vi.fn(async () => ({ definition: { revision: 4 } as any, active_revision: 4, summary: raw })),
    });
    const ctl = new AutomationsController(client, vi.fn(), []);
    await ctl.refresh();
    expect(ctl.state.listError).toBeNull();
    expect(ctl.state.items.map((s) => [s.title, s.time_zone, s.schedule_rule_text])).toEqual([["Daily price watch", "UTC", "—"]]);
    await ctl.select(raw.automation_id);
    expect(ctl.state.detail?.summary.next_run_at).toBe(raw.next_fire_at);
    const html = renderToStaticMarkup(<AutomationCard summary={ctl.state.items[0]} selected={false} busy={false} nowMs={LEGACY_NOW} onSelect={() => {}} onToggleActive={() => {}} />);
    expect(html).toContain("Daily price watch");
    expect(html).toContain("—");
    expect(html).toContain("next in 3 h");
  });
});
