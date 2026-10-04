import { openWorkspaceSection, openWorkflowInputs, closeWorkspaceDrawer } from "./drawer_navigation";
import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";

// The sidebar (round 2, item 10) against the isolated fixture gateway (e2e/gateway_fixture.py):
// the Workspace row never overflows, the two panel headers (round 3) never hide a row, conversations page by 25.
const appOrigin = process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782";
const fixtureGateway = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const fixtureUser = process.env.ABSTRACTCODE_E2E_USER || "web-tester";
const fixtureToken = process.env.ABSTRACTCODE_E2E_TOKEN || "abstractcode-e2e-only";
// A per-conversation folder named by a session id: the shape that used to wrap over the chevron.
const LONG_WS = "/srv/abstractframework/runtime/gateway/workspaces/sess_ed8c3b0bf5fd4d249477aca68da7f494-3f2a9c71e0b84d5e";

/** 40 synthetic conversations of 1-9 turns (200 root runs), appended after the fixture's own runs. */
function syntheticRuns(): Record<string, unknown>[] {
  const runs: Record<string, unknown>[] = [];
  const base = Date.parse("2020-01-01T12:00:00Z");
  for (let s = 0; s < 40; s++) {
    const turns = (s % 9) + 1;
    for (let t = 0; t < turns; t++) {
      const at = new Date(base - s * 3_600_000 - t * 60_000).toISOString();
      runs.push({
        run_id: `syn-${s}-${t}`, workflow_id: "fixture:synthetic", status: "completed",
        created_at: at, updated_at: at, session_id: `sess_synthetic_${String(s).padStart(2, "0")}`,
        parent_run_id: null, session_kind: "chat", input_data: { prompt: `Synthetic conversation ${s + 1}` },
        // The gateway's per-turn total (GET /runs?include_metrics=true): conversation s has s*turns tools.
        tool_calls: s,
      });
    }
  }
  return runs;
}

/** Only the local Code origin is reachable; the conversation list gets the synthetic runs appended
 * (limit/offset/has_more like the gateway); the open run's workspace_root is the long path. */
async function route(page: Page, runLimits: number[], opts: { syntheticOnly?: boolean } = {}): Promise<void> {
  const synthetic = syntheticRuns();
  await page.route("**/*", async (r) => {
    const url = new URL(r.request().url());
    if (url.origin !== new URL(appOrigin).origin) return r.abort("blockedbyclient");
    if (/\/runs$/.test(url.pathname) && url.searchParams.get("root_only") === "true" && !url.searchParams.get("session_id")) {
      const limit = Number(url.searchParams.get("limit") || 50);
      const offset = Number(url.searchParams.get("offset") || 0);
      runLimits.push(limit);
      const real = new URL(url);
      real.searchParams.set("limit", "1000");
      real.searchParams.set("offset", "0");
      // syntheticOnly: the list is exactly the 40 synthetic conversations, whatever other specs
      // left on the shared fixture gateway (automation sessions, earlier runs): deterministic counts.
      const body = opts.syntheticOnly ? { items: [] } : await (await r.fetch({ url: real.toString() })).json();
      const all = [...(body.items || []), ...synthetic];
      const items = all.slice(offset, offset + limit);
      return r.fulfill({ json: { items, count: items.length, offset, has_more: all.length > offset + limit } });
    }
    if (/\/runs\/syn-/.test(url.pathname)) return r.fulfill({ status: 404, json: { detail: "synthetic" } });
    if (/\/runs\/[^/]+(\/input_data)?$/.test(url.pathname) && r.request().method() === "GET") {
      const resp = await r.fetch();
      const text = (await resp.text()).replace(/"workspace_root"\s*:\s*"[^"]*"/g, `"workspace_root": ${JSON.stringify(LONG_WS)}`);
      return r.fulfill({ status: resp.status(), headers: resp.headers(), body: text });
    }
    return r.continue();
  });
}

/** Round 3: every workflow the Code picker offers declares abstractcode.agent.v1, so a turn
 * starts from the composer (the inputs drawer says "Back to chat"; there is no "Run workflow"). */
async function sendTurn(page: Page, text = "Run the fixture."): Promise<void> {
  const drawer = page.locator(".code-rail .af-rail__panel");
  await closeWorkspaceDrawer(page);
  const composer = page.locator(".code-conversation .pc-composer textarea");
  if (!(await composer.inputValue()).trim()) await composer.fill(text);
  await page.getByRole("button", { name: "Send", exact: true }).click();
}

/** The header's kit WorkflowPicker (round 3: no "Show all workflows" — it lists only what the
 * gateway returns for abstractcode.agent.v1): open it and choose the entry named `name`. */
async function chooseWorkflow(page: Page, name: string): Promise<void> {
  await openWorkspaceSection(page, "Workflow");
  const picker = page.getByRole("combobox", { name: "Workflow", exact: true });
  await expect(picker).toBeEnabled();
  await picker.click();
  const exact = new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`);
  await page
    .getByRole("listbox", { name: "Workflow" })
    .locator('[role="option"]')
    .filter({ has: page.locator(".af-workflow-picker__name", { hasText: exact }) })
    .first()
    .click();
  await expect(page.locator("#code-workflow-picker .af-workflow-picker__name")).toHaveText(name);
  // The chosen workflow's inputs have loaded (a send before that is refused).
  await expect(page.locator(".code-workflow-select")).not.toHaveAttribute("aria-busy", "true");
  await closeWorkspaceDrawer(page);
}

async function signIn(page: Page): Promise<void> {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  const dialog = page.getByRole("dialog", { name: "Gateway connection" });
  await expect(dialog).toBeVisible();
  await page.locator("#gateway-session-url").fill(fixtureGateway);
  await page.locator("#gateway-session-user").fill(fixtureUser);
  await page.locator("#gateway-session-token").fill(fixtureToken);
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(dialog).toBeHidden();
  await openWorkspaceSection(page, "Workflow");
  await expect(page.getByRole("combobox", { name: "Workflow", exact: true })).toBeEnabled();
  await closeWorkspaceDrawer(page);
}

async function runPromptConversation(page: Page): Promise<void> {
  await chooseWorkflow(page, "Prompt structured");
  await openWorkflowInputs(page);
  const drawer = page.locator(".code-rail .af-rail__panel");
  await drawer.getByLabel(/Ticket/).fill(`sidebar-${Date.now()}`);
  await page.locator(".code-conversation .pc-composer textarea").fill("Sidebar check.");
  await sendTurn(page);
  await expect(page.getByText("A question for you").first()).toBeVisible({ timeout: 30_000 });
}

test.describe("AbstractCode sidebar", () => {
  test.describe.configure({ mode: "serial" });
  // Label hydration may still be in flight through a route when a test ends: never a failure of the next one.
  test.afterEach(async ({ page }) => {
    await page.unrouteAll({ behavior: "ignoreErrors" });
  });

  test("Current workspace preserves the full long path without overflow at desktop, tablet and phone sizes", async ({ page }) => {
    await route(page, []);
    await signIn(page);
    await runPromptConversation(page);
    for (const [width, height] of [[1440, 900], [834, 1194], [390, 844]]) {
      await page.setViewportSize({ width, height });
      const drawer = await openWorkspaceSection(page, "Workspace");
      const path = drawer.locator(".code-current-workspace");
      // Round 4: the workspace shows ONCE as its short name; the full path is its tooltip.
      await expect(path).toContainText(LONG_WS.split("/").filter(Boolean).pop()!);
      await expect(path.locator("code")).toHaveAttribute("title", LONG_WS);
      await path.scrollIntoViewIfNeeded();
      expect(await path.evaluate(el => el.scrollWidth <= el.clientWidth + 1), `${width}: the short name fits its group`).toBe(true);
      const bounds = (await path.boundingBox())!;
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(width + 1);
      await page.keyboard.press("Escape");
    }
  });

  test("Automations and Conversations headers are 44 px rows on the raised surface (with the New conversation + in the Conversations header); no row is ever hidden under a header", async ({ page }) => {
    // 5 automations (the 0.9.0 panel clipped the fourth): the ui-kit's canonical wire fixture + one copy.
    const fixture = JSON.parse(readFileSync(new URL("../../tui/tests/fixtures/automations/list.json", import.meta.url), "utf8"));
    const autos = [...fixture.items, { ...fixture.items[1], automation_id: "copy-5", title: "Release notes digest" }];
    await route(page, []);
    // Registered last = consulted first (Playwright routes run newest-first).
    await page.route((url) => /\/automations$/.test(url.pathname), (r) => (r.request().method() === "GET" ? r.fulfill({ json: { items: autos, next_cursor: null } }) : r.fallback()));
    for (const [width, height] of [[1440, 900], [390, 844]]) {
      await page.setViewportSize({ width, height });
      if (width === 1440) await signIn(page);
      const opener = page.locator(".code-mobile-nav");
      if (await opener.isVisible()) await opener.click();
      // The drawer slides in (0.2 s): measure once it has arrived.
      await page.waitForFunction(() => getComputedStyle(document.querySelector(".code-sidebar")!).transform === "none");
      await expect(page.locator(".code-auto-card")).toHaveCount(5);
      // Round 4: no big top button; New conversation is the "+" beside the Conversations refresh.
      await expect(page.locator(".code-conversations .code-panel-header").getByRole("button", { name: "New conversation", exact: true })).toBeVisible();
      await expect(page.locator(".code-conversations .code-panel-header").getByRole("button", { name: "Refresh conversations", exact: true })).toBeVisible();
      const g = await page.evaluate(() => {
        // The raised surface the headers wear (ui-surface-2), resolved by the browser.
        const probe = document.createElement("div");
        probe.style.background = "var(--ui-surface-2)";
        document.querySelector(".code-sidebar")!.appendChild(probe);
        const newChat = getComputedStyle(probe).backgroundColor;
        probe.remove();
        const headers = [...document.querySelectorAll(".code-sidebar .code-panel-header")].map((h) => {
          const r = h.getBoundingClientRect();
          const toggle = h.querySelector("button.code-panel-toggle")!;
          const actions = h.querySelector(".code-panel-actions")!.getBoundingClientRect();
          return {
            background: getComputedStyle(h).backgroundColor, height: r.height,
            expanded: toggle.getAttribute("aria-expanded"), chevronFirst: toggle.firstElementChild?.tagName.toLowerCase() === "svg",
            actionsRight: r.right - actions.right <= 8 && actions.left > toggle.getBoundingClientRect().left + 40,
          };
        });
        const lists = [...document.querySelectorAll(".code-sidebar .code-panel > nav, .code-sidebar .code-panel > [role=region]")].map((l) => ({
          background: getComputedStyle(l).backgroundColor, overflowY: getComputedStyle(l).overflowY,
        }));
        const hidden: string[] = [];
        for (const row of document.querySelectorAll<HTMLElement>(".code-auto-card, .code-sessions .code-session")) {
          row.scrollIntoView({ block: "nearest" });
          const last = (row.querySelector(".code-card-meta") || row).getBoundingClientRect();
          const hit = document.elementFromPoint(last.left + 4, last.top + last.height / 2);
          if (!hit || !row.contains(hit)) hidden.push(row.textContent || "");
        }
        const sidebar = document.querySelector(".code-sidebar")!.getBoundingClientRect();
        const bottom = document.querySelector(".code-sidebar-bottom")!.getBoundingClientRect();
        return { newChat, headers, lists, hidden, bottomPinned: bottom.bottom <= sidebar.bottom + 1 };
      });
      expect(g.headers, `${width}`).toHaveLength(2);
      for (const h of g.headers) {
        expect(h.background, `${width} header background`).toBe(g.newChat);
        expect(h.height, `${width} header height`).toBeGreaterThanOrEqual(44);
        expect(h.expanded).toBe("true");
        expect(h.chevronFirst).toBe(true);
        expect(h.actionsRight, `${width} actions at the right`).toBe(true);
      }
      for (const l of g.lists) {
        expect(l.background, `${width} items on the sidebar background`).toBe("rgba(0, 0, 0, 0)");
        // Round 4: each list scrolls inside its own drawer (rows reachable, never under a header).
        expect(l.overflowY, `${width} list scrolls inside its drawer`).toBe("auto");
      }
      expect(g.hidden, `${width} rows hidden under a header`).toEqual([]);
      expect(g.bottomPinned, `${width} bottom block pinned`).toBe(true);
      // Keyboard: Enter on the focused header folds the list; the state survives a reload.
      await page.locator("#code-panel-automations-toggle").focus();
      await page.keyboard.press("Enter");
      await expect(page.locator("#code-panel-automations")).toBeHidden();
      await page.keyboard.press(" ");
      await expect(page.locator("#code-panel-automations")).toBeVisible();
      await page.keyboard.press("Escape");
    }
  });

  test("round 4: two stacking drawers fold and split as DESIGN §3, lists scroll inside, the state survives a reload; cards carry one meta line", async ({ page }) => {
    const fixture = JSON.parse(readFileSync(new URL("../../tui/tests/fixtures/automations/list.json", import.meta.url), "utf8"));
    // 14 automations: enough to overflow half the sidebar at every size.
    const autos = Array.from({ length: 14 }, (_, i) => ({ ...fixture.items[i % 3], automation_id: `auto-${i}`, title: `Automation ${i + 1}` }));
    const runLimits: number[] = [];
    await route(page, runLimits, { syntheticOnly: true });
    await page.route((url) => /\/automations$/.test(url.pathname), (r) => (r.request().method() === "GET" ? r.fulfill({ json: { items: autos, next_cursor: null } }) : r.fallback()));
    await page.setViewportSize({ width: 1440, height: 900 });
    await signIn(page);
    // The list asks the gateway for the per-turn tool totals.
    expect(runLimits.length).toBeGreaterThan(0);
    // Synthetic conversation 3 (s = 2): 3 turns, 2 tools each.
    await expect(page.locator(".code-sessions .code-session", { hasText: "Synthetic conversation 3" }).locator('[data-field="meta"]')).toHaveText(/ · 3 turns · 6 tools$/);
    // Automation cards (R5): name, then `every … · last …`, then `next …` with the Active switch right-aligned on that line.
    const first = page.locator(".code-auto-card").first();
    await expect(first.getByRole("switch", { name: "Active" })).toBeVisible();
    await expect(first.locator('[data-field="timing"]')).toHaveText(/^every [^·]+ · (last [^·]+ ago|running now|waiting since [^·]+|last never)$/);
    await expect(first.locator('[data-field="next"]')).toHaveText(/^(next (in [^·]+|due now))?$/);
    const line2 = (await first.locator(".code-card-line2").boundingBox())!;
    const sw = (await first.getByRole("switch", { name: "Active" }).boundingBox())!;
    const next = (await first.locator('[data-field="next"]').boundingBox())!;
    const card = (await first.boundingBox())!;
    expect(Math.abs(sw.y + sw.height / 2 - (line2.y + line2.height / 2))).toBeLessThan(4); // same line as `next`
    expect(card.x + card.width - (sw.x + sw.width)).toBeLessThan(16); // right-aligned
    expect(sw.x).toBeGreaterThan(next.x);

    for (const [width, height] of [[1440, 900], [834, 1194], [390, 844]] as const) {
      await page.setViewportSize({ width, height });
      const opener = page.locator(".code-mobile-nav");
      if (await opener.isVisible()) {
        await opener.click();
        await page.waitForFunction(() => getComputedStyle(document.querySelector(".code-sidebar")!).transform === "none");
      }
      // The lists have loaded (after the previous size's reload too).
      await expect(page.locator(".code-auto-card")).toHaveCount(14);
      await expect(page.locator(".code-sessions .code-session[data-session-id]")).toHaveCount(25);
      const geometry = () =>
        page.evaluate(() => {
          const box = (sel: string) => document.querySelector(sel)!.getBoundingClientRect();
          const lists = box(".code-sidebar-lists");
          const aHead = box(".code-automations .code-panel-header");
          const cHead = box(".code-conversations .code-panel-header");
          const aList = document.querySelector<HTMLElement>("#code-panel-automations")!;
          const cList = document.querySelector<HTMLElement>("#code-panel-conversations")!;
          const listBox = (el: HTMLElement) => (el.hidden ? null : { top: el.getBoundingClientRect().top, bottom: el.getBoundingClientRect().bottom, scrolls: el.scrollHeight > el.clientHeight + 1, overflowY: getComputedStyle(el).overflowY });
          return { top: lists.top, bottom: lists.bottom, height: lists.height, aHead: { top: aHead.top, bottom: aHead.bottom }, cHead: { top: cHead.top, bottom: cHead.bottom }, aList: listBox(aList), cList: listBox(cList), docOverflow: document.documentElement.scrollWidth > window.innerWidth };
        });
      const set = async (panel: "automations" | "conversations", open: boolean) => {
        const toggle = page.locator(`#code-panel-${panel}-toggle`);
        if ((await toggle.getAttribute("aria-expanded")) !== String(open)) await toggle.click();
        await expect(toggle).toHaveAttribute("aria-expanded", String(open));
      };
      const mid = (g: Awaited<ReturnType<typeof geometry>>) => g.top + g.height / 2;

      // Both closed: the two header rows at the top, nothing below.
      await set("automations", false);
      await set("conversations", false);
      let g = await geometry();
      expect(g.aHead.top - g.top, `${width} both closed: Automations header at the top`).toBeLessThanOrEqual(12);
      expect(g.cHead.top - g.aHead.bottom, `${width} both closed: Conversations header right below`).toBeLessThanOrEqual(12);
      expect(g.aList).toBeNull();
      expect(g.cList).toBeNull();

      // Automations open: it takes the space above the Conversations header, which sits mid-height.
      await set("automations", true);
      g = await geometry();
      expect(Math.abs(g.cHead.top - mid(g)), `${width} A open: Conversations header mid-height`).toBeLessThanOrEqual(10);
      expect(g.aList!.bottom, `${width} A list ends above the Conversations header`).toBeLessThanOrEqual(g.cHead.top + 1);
      expect(g.aList!.scrolls && g.aList!.overflowY === "auto", `${width} A list scrolls inside its drawer`).toBe(true);
      expect(g.cList).toBeNull();

      // Conversations open alone: it takes the rest below its header.
      await set("automations", false);
      await set("conversations", true);
      g = await geometry();
      expect(g.cHead.top - g.aHead.bottom, `${width} C open: its header right below Automations`).toBeLessThanOrEqual(12);
      expect(g.bottom - g.cList!.bottom, `${width} C list reaches the bottom`).toBeLessThanOrEqual(12);
      expect(g.cList!.scrolls && g.cList!.overflowY === "auto", `${width} C list scrolls inside its drawer`).toBe(true);

      // Both open: an even split, the Conversations header pinned mid-height; each list scrolls.
      await set("automations", true);
      g = await geometry();
      expect(Math.abs(g.cHead.top - mid(g)), `${width} both open: Conversations header mid-height`).toBeLessThanOrEqual(10);
      expect(g.aList!.scrolls && g.cList!.scrolls, `${width} both lists scroll inside their drawers`).toBe(true);
      // The last automation card is reachable by scrolling ITS drawer, and not under a header.
      const last = page.locator(".code-auto-card").last();
      await last.scrollIntoViewIfNeeded();
      const lb = (await last.boundingBox())!;
      expect(lb.y + lb.height, `${width} last card above the Conversations header`).toBeLessThanOrEqual(g.cHead.top + 1);
      expect(g.docOverflow, `${width} no horizontal overflow`).toBe(false);

      // Remembered: Automations closed survives a reload.
      await set("automations", false);
      await page.reload({ waitUntil: "domcontentloaded" });
      if (await page.locator(".code-mobile-nav").isVisible()) await page.locator(".code-mobile-nav").click();
      await expect(page.locator("#code-panel-automations-toggle")).toHaveAttribute("aria-expanded", "false");
      await expect(page.locator("#code-panel-conversations-toggle")).toHaveAttribute("aria-expanded", "true");
      await set("automations", true);
      if (await page.locator(".code-mobile-close").isVisible()) await page.locator(".code-mobile-close").click();
    }
  });

  test("Conversations show 25 and Load more adds 25, counted in conversations rather than runs", async ({ page }) => {
    const runLimits: number[] = [];
    await route(page, runLimits, { syntheticOnly: true });
    await page.setViewportSize({ width: 1440, height: 900 });
    await signIn(page);
    const rows = page.locator(".code-sessions .code-session:not(.is-selected)");
    await expect(rows).toHaveCount(25);
    // 100 runs fold into fewer than 26 conversations here: the fetch grew instead of showing fewer.
    expect(runLimits[0]).toBe(100);
    expect(runLimits).toContain(200);
    await page.getByRole("button", { name: "Load more conversations" }).click();
    // 40 synthetic conversations, nothing else: Load more shows all of them (fewer than 50).
    await expect(rows).toHaveCount(40);
  });
});

