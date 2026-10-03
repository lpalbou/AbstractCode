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
import { ConversationCard, conversationMetaLine } from "./sidebar_cards";
import { AutomationHeaderBar, HEADER_BUTTONS } from "./automation_header";

const run = (session: string, id: string, at: string, tools: unknown) => ({
  run_id: id, session_id: session, status: "completed", created_at: at, updated_at: at, parent_run_id: null,
  ...(tools === undefined ? {} : { tool_calls: tools }),
});

describe("conversation card", () => {
  it("sums the gateway's per-turn tool_calls into the conversation's total", () => {
    const [s] = normalizeSessionSummaries({
      items: [run("s1", "r2", "2026-10-02T10:05:00Z", 4), run("s1", "r1", "2026-10-02T10:00:00Z", 3)],
    });
    expect(s.turnCount).toBe(2);
    expect(s.toolCalls).toBe(7);
    expect(conversationMetaLine(s, { locale: "en-US", timeZone: "UTC" })).toBe("Oct 2 · 2 turns · 7 tools");
  });

  it("shows no tool figure when a listed turn lacks the count (an older gateway), never a partial sum", () => {
    const [s] = normalizeSessionSummaries({
      items: [run("s1", "r2", "2026-10-02T10:05:00Z", 4), run("s1", "r1", "2026-10-02T10:00:00Z", null)],
    });
    expect(s.toolCalls).toBeUndefined();
    expect(conversationMetaLine(s, { locale: "en-US", timeZone: "UTC" })).toBe("Oct 2 · 2 turns");
    const [none] = normalizeSessionSummaries({ items: [run("s2", "r9", "2026-10-02T10:00:00Z", undefined)] });
    expect(none.toolCalls).toBeUndefined();
  });

  it("singular words, no figure for zero tools, and no year", () => {
    expect(conversationMetaLine({ updatedAt: "2025-01-31T12:00:00Z", turnCount: 1, toolCalls: 1 }, { locale: "en-US", timeZone: "UTC" })).toBe("Jan 31 · 1 turn · 1 tool");
    expect(conversationMetaLine({ updatedAt: "2026-10-02T12:00:00Z", turnCount: 3, toolCalls: 0 }, { locale: "en-US", timeZone: "UTC" })).toBe("Oct 2 · 3 turns");
    expect(conversationMetaLine({ turnCount: 2, toolCalls: 5 })).toBe("Saved conversation · 2 turns · 5 tools");
  });

  it("renders the title on one line then the meta line, full width (no leading icon)", () => {
    const html = renderToStaticMarkup(
      <ConversationCard
        item={{ sessionId: "sess_1234567890", state: "done", updatedAt: "2026-10-02T10:00:00Z", firstRunId: "r1", latestRunId: "r2", runIds: ["r2", "r1"], turnCount: 2, toolCalls: 7, prompt: "Fix the build", truncated: false }}
        selected
        onClick={() => {}}
      />,
    );
    expect(html).toMatch(/^<button type="button" class="code-session code-card is-selected" aria-current="page" data-session-id="sess_1234567890" title="Fix the build"><span><strong class="code-card-title">Fix the build<\/strong><small class="code-card-meta" data-field="meta">[^<]+ · 2 turns · 7 tools<\/small><\/span><\/button>$/);
    expect(html).not.toContain("<svg");
  });

  it("the title is one ellipsised line; the touch rule keeps card text at 14 px or more", () => {
    const css = readFileSync(new URL("./workspace.css", import.meta.url), "utf8");
    expect(css).toMatch(/\.code-card \.code-card-title \{[^}]*overflow: hidden;[^}]*text-overflow: ellipsis;[^}]*white-space: nowrap;/);
    const touch = css.slice(css.lastIndexOf("@media (pointer: coarse)"));
    expect(touch).toMatch(/\.code-card \.code-card-title,\n\s*\.code-card \.code-card-meta,[\s\S]*?font-size: max\(14px/);
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
    expect(html).toContain('data-field="timing">every 30 min · waiting since 4 min · next in 25 min<');
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
