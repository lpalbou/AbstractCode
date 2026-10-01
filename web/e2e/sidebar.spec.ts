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
      });
    }
  }
  return runs;
}

/** Only the local Code origin is reachable; the conversation list gets the synthetic runs appended
 * (limit/offset/has_more like the gateway); the open run's workspace_root is the long path. */
async function route(page: Page, runLimits: number[]): Promise<void> {
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
      const body = await (await r.fetch({ url: real.toString() })).json();
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

/** The header's kit WorkflowPicker (round 3: no "Show all workflows" — it lists only what the
 * gateway returns for abstractcode.agent.v1): open it and choose the entry named `name`. */
async function chooseWorkflow(page: Page, name: string): Promise<void> {
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
  await expect(page.getByLabel("Workflow", { exact: true })).toBeEnabled();
}

async function runPromptConversation(page: Page): Promise<void> {
  await chooseWorkflow(page, "Prompt structured");
  await page.getByRole("button", { name: "Configure inputs", exact: true }).click();
  const drawer = page.getByRole("complementary", { name: "Workflow inputs" });
  await drawer.getByLabel(/Ticket/).fill(`sidebar-${Date.now()}`);
  await page.locator(".pc-composer textarea").fill("Sidebar check.");
  await drawer.getByRole("button", { name: "Run workflow", exact: true }).click();
  await expect(page.getByText("A question for you").first()).toBeVisible({ timeout: 30_000 });
}

async function workspaceRowGeometry(page: Page) {
  return page.locator(".code-sidebar-bottom .code-workspace-row").evaluate((row) => {
    const icons = row.querySelectorAll("svg");
    const value = row.querySelector("small")!;
    const chevron = icons[icons.length - 1].getBoundingClientRect();
    const v = value.getBoundingClientRect();
    const r = row.getBoundingClientRect();
    const sidebar = row.closest(".code-sidebar")!.getBoundingClientRect();
    return {
      title: row.getAttribute("title"),
      text: value.textContent,
      valueRight: v.right, valueHeight: v.height, lineHeight: parseFloat(getComputedStyle(value).lineHeight) || v.height,
      chevronLeft: chevron.left, chevronRight: chevron.right, rowRight: r.right, sidebarRight: sidebar.right,
      ellipsis: getComputedStyle(value).textOverflow, clipped: value.scrollWidth > value.clientWidth,
    };
  });
}

test.describe("AbstractCode sidebar", () => {
  test.describe.configure({ mode: "serial" });

  test("Workspace row keeps a long folder on one ellipsised line left of the chevron at 1440, 834 and in the 390 drawer", async ({ page }) => {
    await route(page, []);
    await page.setViewportSize({ width: 1440, height: 900 });
    await signIn(page);
    await runPromptConversation(page);
    for (const [width, height] of [[1440, 900], [834, 1194], [390, 844]]) {
      await page.setViewportSize({ width, height });
      const opener = page.locator(".code-mobile-nav");
      if (await opener.isVisible()) await opener.click();
      const row = page.locator(".code-sidebar-bottom .code-workspace-row");
      await row.scrollIntoViewIfNeeded();
      await expect(row.locator("small")).toHaveText(LONG_WS.split("/").pop()!);
      const g = await workspaceRowGeometry(page);
      expect(g.title, `${width}`).toBe(`Workspace: ${LONG_WS}`);
      expect(g.ellipsis, `${width}`).toBe("ellipsis");
      expect(g.clipped, `${width}: the long value is cut by the ellipsis`).toBe(true);
      expect(g.valueHeight, `${width}: one line`).toBeLessThanOrEqual(g.lineHeight + 1);
      expect(g.valueRight, `${width}: value ends before the chevron`).toBeLessThanOrEqual(g.chevronLeft);
      expect(g.chevronRight, `${width}: chevron inside the row`).toBeLessThanOrEqual(g.rowRight);
      expect(g.rowRight, `${width}: row inside the sidebar`).toBeLessThanOrEqual(g.sidebarRight + 0.5);
      await page.keyboard.press("Escape");
    }
  });

  test("Automations and Conversations headers are 44 px rows on the New conversation surface; no row is ever hidden under a header", async ({ page }) => {
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
      await expect(page.locator(".code-auto-row")).toHaveCount(5);
      const g = await page.evaluate(() => {
        const newChat = getComputedStyle(document.querySelector(".code-new-chat")!).backgroundColor;
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
          background: getComputedStyle(l).backgroundColor, overflowY: getComputedStyle(l).overflowY, clipped: l.scrollHeight > l.clientHeight + 1,
        }));
        const hidden: string[] = [];
        for (const row of document.querySelectorAll<HTMLElement>(".code-auto-row, .code-sessions .code-session")) {
          row.scrollIntoView({ block: "nearest" });
          const last = (row.querySelector("span > small:last-child") || row).getBoundingClientRect();
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
        expect(l.clipped, `${width} list clipped`).toBe(false);
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

  test("Conversations show 25 and Load more adds 25, counted in conversations rather than runs", async ({ page }) => {
    const runLimits: number[] = [];
    await route(page, runLimits);
    await page.setViewportSize({ width: 1440, height: 900 });
    await signIn(page);
    const rows = page.locator(".code-sessions .code-session:not(.is-selected)");
    await expect(rows).toHaveCount(25);
    // 100 runs fold into fewer than 26 conversations here: the fetch grew instead of showing fewer.
    expect(runLimits[0]).toBe(100);
    expect(runLimits).toContain(200);
    await page.getByRole("button", { name: "Load more conversations" }).click();
    await expect(rows).toHaveCount(Math.min(50, 40 + (await realConversations(page))));
  });
});

async function realConversations(page: Page): Promise<number> {
  return page.evaluate(async () => {
    const r = await fetch("/api/gateway/runs?root_only=true&include_ledger_len=false&limit=1000", { headers: { accept: "application/json" }, credentials: "include" });
    const body = r.ok ? await r.json() : { items: [] };
    return new Set((body.items || []).filter((x: any) => !String(x.run_id).startsWith("syn-")).map((x: any) => x.session_id)).size;
  });
}
