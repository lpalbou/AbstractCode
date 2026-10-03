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
  waitAnswerPayload,
  waitResumeCommand,
} from "./automations";
import { AutomationsSection, NewAutomationDialog, automationPanelProps, folderRefreshKey, folderTitle } from "./automations_view";
import { AutomationCard } from "./sidebar_cards";

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
    getMyEmail: vi.fn(async () => {
      calls.push("me/email");
      return { configured: true, enabled: true, admin_enabled: true, effective_enabled: true };
    }),
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

  it("round 4 cards: name, the Active switch, the waiting badge, one timing line (every · last · next)", () => {
    const html = renderToStaticMarkup(<AutomationsSection {...base} state={state(list())} />);
    const card = (title: string) => {
      const id = list().find((s) => s.title === title)!.automation_id;
      const at = html.indexOf(`data-automation-id="${id}"`);
      expect(at, title).toBeGreaterThan(-1);
      return html.slice(at, html.indexOf('</div>', html.indexOf('role="switch"', at)));
    };
    const timing = (c: string) => /data-field="timing">([\s\S]*?)<\/small>/.exec(c)?.[1].replace(/<[^>]+>/g, "");
    // The operator's line, from the gateway's facts only (NOW = 2026-09-27 06:35 UTC).
    // Run #7 waits for an approval: "waiting since", never "running now" beside the badge.
    expect(timing(card("Inbox triage"))).toBe("every 30 min · waiting since 4 min · next in 25 min");
    const executing = { ...list()[0], attention: { ...list()[0].attention, pending_waits: 0, unseen_count: 0 } };
    const run = renderToStaticMarkup(<AutomationsSection {...base} state={state([executing])} />);
    expect(timing(run)).toBe("every 30 min · running now · next in 25 min");
    expect(timing(card("AI news monitor"))).toBe("every 8 h · last 6 h ago · next in 1 h");
    expect(timing(card("Weekly journal monitor"))).toBe("every 7 d · last 6 d ago");
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

  it("hides archived rows behind a toggle", () => {
    const items = list().map((s, i) => (i === 1 ? { ...s, status: "archived" as const } : s));
    const hidden = renderToStaticMarkup(<AutomationsSection {...base} state={state(items)} />);
    expect(hidden).not.toContain(`data-automation-id="${items[1].automation_id}"`);
    expect(hidden).toContain("Show archived (1)");
    const shown = renderToStaticMarkup(<AutomationsSection {...base} state={state(items, true)} />);
    expect(shown).toContain(`data-automation-id="${items[1].automation_id}" data-status="archived"`);
  });

  it("Show archived is a switch labelled by the feature, not a checkbox (state toggles)", () => {
    const items = list().map((s, i) => (i === 1 ? { ...s, status: "archived" as const } : s));
    const onShowArchived = vi.fn();
    const off = renderToStaticMarkup(<AutomationsSection {...base} onShowArchived={onShowArchived} state={state(items)} />);
    expect(off).toMatch(/<button type="button" role="switch" class="af-switch af-switch--sm code-auto-archived" data-action="show-archived" aria-checked="false"[^>]*>[\s\S]*?<span class="af-switch__label">Show archived \(1\)<\/span>/);
    expect(off).not.toContain('type="checkbox"');
    const on = renderToStaticMarkup(<AutomationsSection {...base} state={state(items, true)} />);
    expect(on).toMatch(/data-action="show-archived" aria-checked="true"/);
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
