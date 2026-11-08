// The left sidebar's Conversations and Automations lists (operator 2026-11-08: "can you try to
// improve the session/automation cards?"). WebKit, disposable fixture gateway on 18781 and the
// Code app on 18782; the automation list is the kit's recorded fixture (ui-kit
// scripts/fixtures/automations/list.json), its times moved to now.
//   npx playwright test e2e/sidebar_rows.spec.ts --browser=webkit
// SIDEBAR_SHOTS=<dir> also writes desktop and 400 px screenshots in dark and light themes.
import { closeWorkspaceDrawer, openWorkspaceSection } from "./drawer_navigation";
import { expect, test, type Page } from "@playwright/test";
import { mkdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const fixtureGateway = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const fixtureUser = process.env.ABSTRACTCODE_E2E_USER || "web-tester";
const fixtureToken = process.env.ABSTRACTCODE_E2E_TOKEN || "abstractcode-e2e-only";
const shots = process.env.SIDEBAR_SHOTS || "";
/** SIDEBAR_BEFORE=1: only the screenshots (the design before this pass fails the checks by design). */
const before = Boolean(process.env.SIDEBAR_BEFORE);
const automationsFixture = fileURLToPath(new URL("../../../abstractuic/ui-kit/scripts/fixtures/automations/list.json", import.meta.url));

const TITLES = {
  short: "Fix the login bug",
  two: "Summarise the release notes and open questions",
  long: "Review the whole voice pipeline from the microphone to the speaker, including the Bluetooth headphone route, the read-aloud player, the device change handling and the Safari autoplay rules, then write a short report with the fixes in order of priority",
};

/** The recorded automations, their times moved around now (values otherwise as recorded). */
function automations(): unknown {
  const list = JSON.parse(readFileSync(automationsFixture, "utf8"));
  const now = Date.now();
  const nexts = [25 * 60_000, 6 * 3_600_000, null, 22 * 3_600_000, 40 * 60_000];
  list.items.forEach((item: any, i: number) => {
    const next = nexts[i] === null || nexts[i] === undefined ? null : new Date(now + Number(nexts[i])).toISOString();
    item.next_run_at = next;
    item.next_fire_at = next;
  });
  // A calendar rule on five weekdays (its compact form names them "Weekdays").
  const news = list.items[1];
  news.trigger = { ...news.trigger, source_version: 2, config: { kind: "weekly", days: ["mon", "tue", "wed", "thu", "fri"], at: "09:00", time_zone: "Europe/Paris" } };
  news.schedule_rule_text = "Every Monday, Tuesday, Wednesday, Thursday and Friday at 09:00 (Europe/Paris)";
  return list;
}

async function chooseWorkflow(page: Page, name: string): Promise<void> {
  await openWorkspaceSection(page, "Workflow");
  const picker = page.getByRole("combobox", { name: "Workflow", exact: true });
  await expect(picker).toBeEnabled();
  await picker.click();
  const exact = new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`);
  await page.getByRole("listbox", { name: "Workflow" }).locator('[role="option"]')
    .filter({ has: page.locator(".af-workflow-picker__name", { hasText: exact }) }).first().click();
  await expect(page.locator(".code-workflow-select")).not.toHaveAttribute("aria-busy", "true");
  await closeWorkspaceDrawer(page);
}

async function open(page: Page, theme: string): Promise<void> {
  await page.addInitScript((t) => {
    // Only the first load: a theme the test sets later survives its reloads.
    try { if (!localStorage.getItem("af_appearance_abstractcode_v1")) localStorage.setItem("af_appearance_abstractcode_v1", JSON.stringify({ theme: t, font_scale: "md", header_density: "standard" })); } catch { /* */ }
  }, theme);
  const list = automations();
  await page.route("**/api/gateway/automations*", (route) => {
    const url = new URL(route.request().url());
    if (route.request().method() !== "GET" || !url.pathname.endsWith("/api/gateway/automations") || url.searchParams.get("status") === "archived") return route.continue();
    return route.fulfill({ json: list });
  });
  if (process.env.SIDEBAR_DEBUG) page.on("request", (r) => { if (r.url().includes("automation")) console.log("REQ", r.method(), r.url()); });
  await page.goto("/", { waitUntil: "domcontentloaded" });
  const dialog = page.getByRole("dialog", { name: "Gateway connection" });
  if (await dialog.isVisible().catch(() => false)) {
    await page.locator("#gateway-session-url").fill(fixtureGateway);
    await page.locator("#gateway-session-user").fill(fixtureUser);
    await page.locator("#gateway-session-token").fill(fixtureToken);
    await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(dialog).toBeHidden();
  }
}

/** Three conversations whose names take one line, two lines, and more than two lines. */
async function seedConversations(page: Page): Promise<void> {
  for (const text of [TITLES.long, TITLES.two, TITLES.short]) {
    await page.locator(".code-new-chat").click();
    await chooseWorkflow(page, "Assistant contract");
    await page.locator(".code-conversation .pc-composer textarea").fill(text);
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect(page.locator(".code-run-strip")).toContainText("Completed", { timeout: 20_000 });
  }
}

async function shoot(page: Page, name: string): Promise<void> {
  if (!shots) return;
  mkdirSync(shots, { recursive: true });
  await page.locator(".code-sidebar").screenshot({ path: `${shots}/${name}.png` });
}

const row = (page: Page, title: string) => page.locator(".code-sessions .af-session-card").filter({ has: page.locator(".af-session-card__title", { hasText: title.slice(0, 20) }) }).first();

test.describe.configure({ mode: "serial" });
// The production build registers the PWA service worker; its fetches would bypass page.route.
test.use({ serviceWorkers: "block" });

test("conversation and automation rows: equal heights, quiet meta, a long name revealed whole", async ({ page }) => {
  test.setTimeout(180_000);
  await open(page, "observer-night");
  await seedConversations(page);
  await expect(page.locator(".code-automation-item")).toHaveCount(5, { timeout: 20_000 });

  const heights = async (selector: string) => page.locator(selector).evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().height)));
  if (!before) await checkRows(page, heights);
  // 400 px: the sidebar is a drawer; rows keep their rhythm, nothing overflows.
  for (const theme of ["observer-night", "light"]) {
    await page.evaluate((t) => localStorage.setItem("af_appearance_abstractcode_v1", JSON.stringify({ theme: t, font_scale: "md", header_density: "standard" })), theme);
    await page.setViewportSize({ width: 1440, height: 960 });
    await page.reload({ waitUntil: "domcontentloaded" });
    await expect(page.locator(".code-automation-item")).toHaveCount(5, { timeout: 20_000 });
    await expect(row(page, TITLES.short)).toBeVisible();
    // Each section header in its colour (kit tokens): Conversations --info, Automations --warning.
    if (!before) {
      const tones = await page.evaluate(() => {
        const resolve = (token: string) => {
          const probe = document.createElement("span");
          probe.style.color = `var(${token})`;
          document.querySelector(".code-sidebar")!.appendChild(probe);
          const value = getComputedStyle(probe).color;
          probe.remove();
          return value;
        };
        const toggle = (panel: string) => getComputedStyle(document.querySelector(`.code-sidebar .code-panel-header[data-panel="${panel}"] .code-panel-toggle`)!).color;
        return { conversations: toggle("conversations"), info: resolve("--info"), automations: toggle("automations"), warning: resolve("--warning") };
      });
      expect(tones.conversations, `${theme} Conversations header`).toBe(tones.info);
      expect(tones.automations, `${theme} Automations header`).toBe(tones.warning);
    }
    await page.mouse.move(700, 500);
    await shoot(page, `desktop-${theme === "light" ? "light" : "dark"}`);
    await page.setViewportSize({ width: 400, height: 860 });
    await page.locator(".code-mobile-nav").click();
    await page.waitForFunction(() => getComputedStyle(document.querySelector(".code-sidebar")!).transform === "none");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    if (!before) {
      const narrow = await heights(".code-sessions .af-session-card");
      expect(new Set(narrow).size, `400 px conversation rows ${narrow.join(", ")}`).toBe(1);
    }
    await shoot(page, `400-${theme === "light" ? "light" : "dark"}`);
  }
});

async function checkRows(page: Page, heights: (selector: string) => Promise<number[]>): Promise<void> {
  // Equal row heights within each list: a one-line and a two-line name give the same row.
  const conversationHeights = await heights(".code-sessions .af-session-card");
  expect(conversationHeights.length).toBeGreaterThanOrEqual(3);
  expect(new Set(conversationHeights).size, `conversation rows ${conversationHeights.join(", ")}`).toBe(1);
  const automationHeights = await heights(".code-automation-item .af-session-card");
  expect(new Set(automationHeights).size, `automation rows ${automationHeights.join(", ")}`).toBe(1);
  // Lines of text actually laid out (the title box itself always keeps room for two).
  const lines = async (title: string) => row(page, title).locator(".af-session-card__title").evaluate((el) => {
    const range = document.createRange();
    range.selectNodeContents(el);
    return new Set(Array.from(range.getClientRects()).map((r) => Math.round(r.top))).size;
  });
  const clamped = (title: string) => row(page, title).locator(".af-session-card__title").evaluate((el) => el.scrollHeight > el.clientHeight + 1);
  expect(await lines(TITLES.short)).toBe(1);
  expect(await lines(TITLES.two)).toBe(2);
  expect(await clamped(TITLES.two)).toBe(false);
  expect(await clamped(TITLES.long)).toBe(true); // two lines shown, ending in "…"

  // The details line is the kit's symbol row, as before (operator 2026-11-08: "put them back"):
  // each figure keeps its icon; only the automation's schedule reads in its compact form.
  const meta = (title: string) => row(page, title).locator(".af-session-card__meta");
  const symbols = (loc: import("@playwright/test").Locator) => loc.locator("svg").evaluateAll((svgs) => svgs.filter((s) => getComputedStyle(s).display !== "none" && s.getBoundingClientRect().width > 0).length);
  expect(await symbols(meta(TITLES.short)), "conversation: chat + clock").toBe(2);
  expect(await rendered(meta(TITLES.short))).toMatch(/^≥?1 (just now|\d+ min ago)$/);
  const morning = page.locator(".code-automation-item .af-session-card").filter({ hasText: "Morning briefing" });
  expect(await symbols(morning.locator(".af-session-card__meta")), "automation: runs + schedule + next").toBe(3);
  // The schedule in its compact form beside its clock, from the rule's structure (daily 08:00).
  expect(await rendered(morning.locator(".af-session-card__meta"))).toMatch(/^7 Daily 08:00 2[12]h$/);
  const triage = page.locator(".code-automation-item .af-session-card").filter({ hasText: "Inbox triage" });
  expect(await symbols(triage.locator(".af-session-card__meta")), "a waiting automation keeps every symbol").toBe(3);
  expect(await rendered(triage.locator(".af-session-card__meta"))).toMatch(/^7 Every 30 min \d+m Needs you 2$/);
  const news = page.locator(".code-automation-item").filter({ hasText: "AI news monitor" });
  expect(await symbols(news.locator(".af-session-card__meta"))).toBe(3);
  expect(await rendered(news.locator(".af-session-card__meta"))).toMatch(/^6 Weekdays 09:00 [56]h$/);
  // Nothing is cut inside a row's details (the symbol row wraps instead).
  const cut = await page.locator(".code-sidebar .af-session-card__meta").evaluateAll((metas) => metas.flatMap((m) =>
    [m, ...Array.from(m.querySelectorAll("*"))].filter((el) => getComputedStyle(el).display !== "none" && (el as HTMLElement).scrollWidth > (el as HTMLElement).clientWidth + 1).map((el) => el.textContent || el.className)));
  expect(cut).toEqual([]);
  // The state marker and the count sit in one column at the right edge.
  const centreX = (loc: import("@playwright/test").Locator) => loc.evaluate((el) => { const r = el.getBoundingClientRect(); return Math.round(r.left + r.width / 2); });
  expect(Math.abs((await centreX(triage.locator(".af-session-card__marker"))) - (await centreX(triage.locator(".af-automation-card__unseen"))))).toBeLessThanOrEqual(1);

  // A name longer than two lines: clamped with "…", the whole name in the kit tooltip on hover and
  // on keyboard focus. A name that fits shows no tooltip (nothing to reveal).
  const longOpen = row(page, TITLES.long).locator(".af-session-card__open");
  await expect(longOpen).toHaveAttribute("data-af-tip", TITLES.long);
  await expect(row(page, TITLES.short).locator(".af-session-card__open")).not.toHaveAttribute("data-af-tip", /.+/);
  await longOpen.hover();
  await expect(page.locator(".af-tooltip:not([hidden])")).toHaveText(TITLES.long, { timeout: 2_000 });
  await page.mouse.move(700, 500);
  await expect(page.locator(".af-tooltip:not([hidden])")).toHaveCount(0, { timeout: 2_000 });
  await page.keyboard.press("Tab"); // keyboard mode
  await longOpen.focus();
  await expect(page.locator(".af-tooltip:not([hidden])")).toHaveText(TITLES.long, { timeout: 2_000 });
  await page.keyboard.press("Escape");

  // The active row: a tone and a hairline, never a 2 px frame.
  await row(page, TITLES.two).locator(".af-session-card__open").click();
  const selected = row(page, TITLES.two);
  await expect(selected).toHaveAttribute("data-selected", "true");
  expect(await selected.evaluate((el) => parseFloat(getComputedStyle(el).borderTopWidth) <= 1)).toBe(true);
  await page.mouse.move(700, 500);

}

/** The meta row as shown: each visible part with its generated words and separators. */
function rendered(meta: import("@playwright/test").Locator): Promise<string> {
  return meta.evaluate((root) => {
    const pseudo = (el: Element, which: string) => {
      const value = getComputedStyle(el, which).content;
      if (!value || value === "none" || value === "normal") return "";
      if (value.startsWith("attr(")) return el.getAttribute(value.slice(5, -1)) || "";
      return value.replace(/^"|"$/g, "");
    };
    const shown = (n: Node) => n.nodeType === 3 || (n.nodeType === 1 && getComputedStyle(n as Element).display !== "none" && (n as Element).tagName.toLowerCase() !== "svg");
    const order = (n: Node) => (n.nodeType === 1 ? Number(getComputedStyle(n as Element).order) || 0 : 0);
    return Array.from(root.childNodes).filter(shown).map((n, i) => ({ n, i })).sort((a, b) => order(a.n) - order(b.n) || a.i - b.i).map(({ n: part }) => {
      if (part.nodeType === 3) return part.textContent || "";
      const el = part as Element;
      const own = Array.from(el.childNodes).filter(shown).map((n) => n.textContent || "").join("");
      return `${pseudo(el, "::before")}${own} ${pseudo(el, "::after")}`;
    }).join(" ").replace(/\s+/g, " ").trim();
  });
}

// The operator's gateway answers every listing with has_more: the counts are lower bounds. The
// row keeps the kit's compact form ("≥1" beside the chat symbol, "At least 1 turn" for screen
// readers and the tooltip), never a sentence.
test("counts the gateway gives as a lower bound keep the kit's ≥ beside the symbol", async ({ page }) => {
  test.setTimeout(120_000);
  await page.route("**/api/gateway/runs?*", async (route) => {
    const url = new URL(route.request().url());
    if (route.request().method() !== "GET" || !url.pathname.endsWith("/api/gateway/runs")) return route.continue();
    try {
      const response = await route.fetch();
      const body = await response.json();
      return await route.fulfill({ response, json: { ...body, has_more: true } });
    } catch {
      return route.abort().catch(() => undefined); // the app dropped this request itself (a newer list)
    }
  });
  await open(page, "observer-night");
  await seedConversations(page);
  const line = row(page, TITLES.short).locator(".af-session-card__meta");
  await expect.poll(() => rendered(line)).toMatch(/^≥1 (just now|\d+ min ago)$/);
  await expect(line.locator("[aria-label]").first()).toHaveAttribute("aria-label", "At least 1 turn");
  expect(await line.locator("svg").count()).toBe(2);
});
