// Round 4 (DESIGN §3): the conversation card `title / Oct 2 · 2 turns · 7 tools` and the
// automation header (§4). Red when a card loses its tool figure, counts tools itself, shows a
// year, or the header shows the full path / an ellipsis label / loses a control.
import React from "react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { AutomationSummary } from "@abstractframework/ui-kit";

import { normalizeSessionSummaries } from "./catalog";
import { ConversationCard } from "./sidebar_cards";
import { AutomationHeaderBar, HEADER_BUTTONS } from "./automation_header";

const run = (session: string, id: string, at: string, tools: unknown) => ({
  run_id: id, session_id: session, status: "completed", created_at: at, updated_at: at, parent_run_id: null,
  ...(tools === undefined ? {} : { tool_calls: tools }),
});

describe("shared conversation card", () => {
  const card = (item: ReturnType<typeof normalizeSessionSummaries>[number]) => renderToStaticMarkup(<ConversationCard item={item} selected onClick={() => {}} />);
  it("presents known Gateway totals and hides unknown tool counts", () => {
    const [known] = normalizeSessionSummaries({ items: [run("s1", "r2", "2026-10-02T10:05:00Z", 4), run("s1", "r1", "2026-10-02T10:00:00Z", 3)] });
    expect(known.toolCalls).toBe(7);
    expect(card(known)).toContain('aria-label="7 tool calls"');
    const [unknown] = normalizeSessionSummaries({ items: [run("s1", "r2", "2026-10-02T10:05:00Z", 4), run("s1", "r1", "2026-10-02T10:00:00Z", null)] });
    expect(unknown.toolCalls).toBeUndefined();
    expect(card(unknown)).not.toContain('tool calls');
  });
  it("uses the shared selected state and compact statistics", () => {
    const [s] = normalizeSessionSummaries({items:[run("s1","r1","2026-10-02T10:00:00Z",7)]});
    const html=card(s);
    expect(html).toContain('class="af-session-card" data-selected="true"');
    expect(html).toContain('class="af-session-card__title"');
    expect(html).toContain('class="af-session-card__meta"');
    expect(html).toContain('data-session-id="s1"');
  });
  it("keeps attention independent of both unread and selection", () => {
    const [s] = normalizeSessionSummaries({items:[run("s1","r1","2026-10-02T10:00:00Z",0)]});
    const html=renderToStaticMarkup(<ConversationCard item={s} selected unread needsAttention onClick={() => {}} />);
    expect(html).toContain('data-unread="true" data-attention="true"');
    expect(html).toContain('Needs you');
    expect(html).not.toContain('0 tool');
  });
});

const FIXTURES = join(__dirname, "../../../tui/tests/fixtures/automations");
const list = (): AutomationSummary[] => JSON.parse(readFileSync(join(FIXTURES, "list.json"), "utf8")).items;
const NOW = Date.parse("2026-09-27T06:35:00Z");

describe("automation header (DESIGN §4)", () => {
  const header = (s: AutomationSummary, over: Record<string, unknown> = {}) =>
    renderToStaticMarkup(
      <AutomationHeaderBar
        summary={s}
        occurrences={[]}
        triggerSources={[]}
        busy={false}
        nowMs={NOW}
        onCommand={async () => ({})}
        onToggleActive={async () => ({})}
        onEdit={() => {}}
        onOpenFolder={() => {}}
        onCopyPath={async () => true}
        {...over}
      />,
    );

  it("title, Active switch, waiting badge, the timing line", () => {
    const inbox = list().find((s) => s.title === "Inbox triage")!;
    const html = header(inbox);
    expect(html).toContain('<h2 class="code-auto-header__title" tabindex="-1">Inbox triage</h2>');
    expect(html).toMatch(/role="switch"[^>]*data-action="active" aria-checked="true"[\s\S]*?af-switch__label">Active</);
    expect(html).toContain('data-field="waiting">waiting for you<');
    // An approval is pending on run #7: waiting since it fired (06:30), never "running now".
    expect(html).toContain('data-field="timing">Every 30 minutes (UTC) · waiting since 4 min · next in 25 min<');
    expect(html).not.toContain("running now");
    const news = list().find((s) => s.title === "AI news monitor")!;
    expect(header(news)).not.toContain("waiting for you");
    // The badge means an approval is pending — not merely unseen results.
    expect(header({ ...news, attention: { ...news.attention, unseen_count: 3, unread: true, pending_waits: 0 } })).not.toContain("waiting for you");
    expect(header({ ...news, attention: { ...news.attention, unseen_count: 0, pending_waits: 1 } })).toContain("waiting for you");
  });

  it("the workspace as a short name with open and copy icons, never the full path on screen", () => {
    const s = { ...list()[1], workspace_root: "/srv/gateway/runtime/automations/auto_7f3c9a2b" };
    const html = header(s);
    const visible = html.replace(/<[^>]+>/g, " ");
    expect(visible).toContain("auto_7f3c9a2b");
    expect(visible).not.toContain("/srv/gateway");
    expect(html).toMatch(/data-action="open-folder" aria-label="Open folder"/);
    expect(html).toMatch(/data-action="copy-path" aria-label="Copy path"/);
  });

  it("Run now · Stop · Edit · Archive, short labels and no ellipsis; the kit rule decides what is enabled", () => {
    expect(HEADER_BUTTONS.map((b) => b.label)).toEqual(["Run now", "Stop", "Edit", "Archive"]);
    const news = list().find((s) => s.title === "AI news monitor")!;
    const html = header(news);
    const toolbar = /role="toolbar" aria-label="Automation controls">([\s\S]*?)<\/div>/.exec(html)?.[1] ?? "";
    const labels = [...toolbar.matchAll(/<span>([^<]+)<\/span><\/button>/g)].map((m) => m[1]);
    expect(labels).toEqual(["Run now", "Stop", "Edit", "Archive"]);
    expect(html).not.toMatch(/…|\.\.\./);
    // Nothing runs on AI news monitor: Run now enabled, Stop disabled with the kit's reason.
    expect(toolbar).toMatch(/data-action="run_now" title="Run it once now/);
    expect(toolbar).toMatch(/data-action="stop_current" disabled="" title="Nothing is running\./);
    // An archived automation: every control is off, the switch unavailable.
    const archived = header({ ...news, status: "archived" });
    expect(archived.match(/data-action="(run_now|stop_current|edit|archive)" disabled=""/g)).toHaveLength(4);
  });

  it("says when this gateway cannot run the trigger (the kit's check)", () => {
    const news = list().find((s) => s.title === "AI news monitor")!;
    const html = header(news, { triggerSources: [{ id: "manual", version: 1 }] });
    expect(html).toMatch(/data-field="trigger-problem">[\s\S]*does not list the trigger source schedule@1/);
    expect(header(news)).not.toContain("trigger-problem");
  });
});
