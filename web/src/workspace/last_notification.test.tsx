// Round 16: the gateway's `last_notification` on an automation's card and header. Red when the line
// is missing for a failed notice, shown for a successful one, or worded by the app instead of the
// served `text`.
import React from "react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { AutomationSummary, AutomationsClient } from "@abstractframework/ui-kit";

import { AutomationHeaderBar } from "./automation_header";
import { failedNotification } from "./last_notification";
import { withServedSummaries } from "./served_summary";
import { AutomationCard } from "./sidebar_cards";

const FIXTURES = join(__dirname, "../../../tui/tests/fixtures/automations");
// Shared with the terminal client's tests: one failed and one sent notice, in the gateway's shape.
const NOTICES = JSON.parse(readFileSync(join(__dirname, "../../../tui/tests/fixtures/notifications/last_notification.json"), "utf8"));
const base = (): AutomationSummary => JSON.parse(readFileSync(join(FIXTURES, "list.json"), "utf8")).items.find((s: AutomationSummary) => s.title === "AI news monitor");
const NOW = Date.parse("2026-09-27T06:35:00Z");
const withNotice = (n: unknown): AutomationSummary => ({ ...base(), last_notification: n }) as unknown as AutomationSummary;
const FAILED_TEXT = NOTICES.failed.text;

const card = (s: AutomationSummary) =>
  renderToStaticMarkup(<AutomationCard summary={s} selected={false} busy={false} nowMs={NOW} onSelect={() => {}} onToggleActive={() => {}} />);
const header = (s: AutomationSummary) =>
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
    />,
  );
const esc = (t: string) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

describe("last_notification (the last Email result)", () => {
  it("a failed notice: the card's third line is the kit error pill with the served text verbatim, then the time", () => {
    const html = card(withNotice(NOTICES.failed));
    expect(html).toMatch(
      new RegExp(
        `<small class="code-notif-line code-card-notif" data-field="last-notification" data-status="failed"><span class="af-chip af-chip--error af-chip--sm code-notif-pill"[^>]*><span class="af-chip__label">${esc(FAILED_TEXT)}</span></span><span class="code-notif-time">3 h ago</span></small>`,
      ),
    );
  });

  it("a failed notice: the header shows the same line", () => {
    const html = header(withNotice(NOTICES.failed));
    expect(html).toContain('class="code-notif-line code-auto-header__notif" data-field="last-notification" data-status="failed"');
    expect(html).toContain(`<span class="af-chip__label">${FAILED_TEXT}</span>`);
    expect(html).toContain('<span class="code-notif-time">3 h ago</span>');
  });

  it("a later successful send (the gateway's newest notice) clears the line; null or absent shows nothing", () => {
    for (const n of [NOTICES.sent, null, undefined]) {
      expect(card(withNotice(n))).not.toContain("last-notification");
      expect(header(withNotice(n))).not.toContain("last-notification");
    }
    expect(card(withNotice(NOTICES.sent))).not.toContain("Email result");
  });

  it("never words it itself: a different served text is shown as served", () => {
    const other = { ...NOTICES.failed, text: "Served words only" };
    expect(failedNotification(withNotice(other))).toEqual({ text: "Served words only", at: NOTICES.failed.at });
    expect(card(withNotice(other))).toContain('<span class="af-chip__label">Served words only</span>');
  });

  it("survives the served-summary pass on list, get and create", async () => {
    const s = withNotice(NOTICES.failed);
    const client = withServedSummaries({
      listAutomations: async () => ({ items: [s], next_cursor: null }),
      getAutomation: async () => ({ summary: s }),
      createAutomation: async () => ({ summary: s }),
    } as unknown as AutomationsClient);
    const listed = (await client.listAutomations({})).items[0];
    const got = (await client.getAutomation("x")).summary;
    const made = (await client.createAutomation({} as never)).summary;
    for (const x of [listed, got, made]) expect(failedNotification(x)?.text).toBe(FAILED_TEXT);
  });
});
