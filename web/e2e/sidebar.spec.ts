import { expect, test, type Page } from "@playwright/test";

// The sidebar (round 2, item 10) against the isolated fixture gateway (e2e/gateway_fixture.py):
// the Workspace row never overflows, the two lists are panels, conversations page by 25.
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
  const drawer = page.getByRole("complementary", { name: "Workflow inputs" });
  if (await drawer.isVisible()) await drawer.getByRole("button", { name: "Back to chat", exact: true }).click();
  const composer = page.locator(".pc-composer textarea");
  if (!(await composer.inputValue()).trim()) await composer.fill(text);
  await page.getByRole("button", { name: "Send", exact: true }).click();
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
  // The chosen workflow's inputs have loaded (a send before that is refused).
  await expect(page.locator(".code-workflow-select")).not.toHaveAttribute("aria-busy", "true");
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
  await page.getByRole("button", { name: "Inputs", exact: true }).click();
  const drawer = page.getByRole("complementary", { name: "Workflow inputs" });
  await drawer.getByLabel(/Ticket/).fill(`sidebar-${Date.now()}`);
  await page.locator(".pc-composer textarea").fill("Sidebar check.");
  await sendTurn(page);
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

  test("Automations and Conversations are panels: own surface, radius, inset, header inside", async ({ page }) => {
    await route(page, []);
    await page.setViewportSize({ width: 1440, height: 900 });
    await signIn(page);
    for (const panel of [".code-automations", ".code-conversations"]) {
      const s = await page.locator(`.code-sidebar ${panel}`).evaluate((el) => {
        const cs = getComputedStyle(el);
        const sidebar = el.closest(".code-sidebar")!.getBoundingClientRect();
        const r = el.getBoundingClientRect();
        return {
          background: cs.backgroundColor, sidebarBackground: getComputedStyle(el.closest(".code-sidebar")!).backgroundColor,
          radius: parseFloat(cs.borderTopLeftRadius), insetLeft: r.left - sidebar.left, insetRight: sidebar.right - r.right,
          headerInside: el.firstElementChild?.classList.contains("code-section-label") === true,
        };
      });
      expect(s.background, panel).not.toBe("rgba(0, 0, 0, 0)");
      expect(s.background, panel).not.toBe(s.sidebarBackground);
      expect(s.radius, panel).toBeGreaterThanOrEqual(8);
      expect(s.insetLeft, panel).toBeGreaterThanOrEqual(8);
      expect(s.insetRight, panel).toBeGreaterThanOrEqual(8);
      expect(s.headerInside, panel).toBe(true);
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

