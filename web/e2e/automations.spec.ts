import { expect, test, type Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// Automations against the isolated fixture gateway (e2e/gateway_fixture.py):
// create from the web app with the toolbar's workflow, see the run and its
// tool approval, approve it, read the folder, discuss a run (a real chat in
// place), archive (hidden, history kept). Real gateway routes, no mocks.
const appOrigin = process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782";
const fixtureGateway = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const fixtureUser = process.env.ABSTRACTCODE_E2E_USER || "web-tester";
const fixtureToken = process.env.ABSTRACTCODE_E2E_TOKEN || "abstractcode-e2e-only";
const artifactsDir = join(fileURLToPath(new URL(".", import.meta.url)), "artifacts");

async function capture(page: Page, name: string): Promise<void> {
  mkdirSync(artifactsDir, { recursive: true });
  await page.screenshot({ path: join(artifactsDir, `${name}.png`), fullPage: true });
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
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin === new URL(appOrigin).origin) return route.continue();
    await route.abort("blockedbyclient");
  });
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

test("creates, runs, approves, browses, discusses and archives an automation", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await signIn(page);
  const title = `E2E automation ${Date.now()}`;

  // The toolbar's workflow is the target: the fixture's native tool call.
  await chooseWorkflow(page, "Native tool approval");

  // Create with the kit dialog: ask before each tool call.
  await page.getByRole("button", { name: "New automation" }).click();
  const dialog = page.getByRole("dialog", { name: "Schedule a task" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("Runs Native tool approval (the toolbar's workflow).")).toBeVisible();
  await dialog.getByLabel("Task").fill("Write the fixture file");
  await dialog.getByText("Ask me before each tool call").click();
  await dialog.getByText("Advanced").click();
  await dialog.getByLabel("Title").fill(title);
  await dialog.getByRole("button", { name: "Create automation" }).click();
  await expect(dialog).toBeHidden();

  // The row states the gateway's truth: text + icon, next run from next_fire_at.
  const row = page.locator(".code-auto-row", { hasText: title });
  await expect(row).toBeVisible();
  // The kit's state label: the word, then its icon.
  await expect(row.locator('[data-field="state"] [data-state="active"]')).toHaveText("Active");
  await expect(row.locator('[data-field="state"] [data-state="active"] svg')).toHaveCount(1);
  await expect(row.locator('[data-field="next"]')).toContainText("UTC");

  // The first run starts now and parks on a typed tool approval.
  const main = page.locator(".code-automation-main");
  await expect(main.getByRole("button", { name: "Approve" }).first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".code-breadcrumb")).toContainText(title);
  await capture(page, "automation-approval");
  await main.getByRole("button", { name: "Approve" }).first().click();
  await expect(main.locator('.af-auto-occ[data-index="1"] .af-auto-turn__status')).toHaveText("completed", { timeout: 30_000 });

  // The run's ledger opens through this app's proxy, never as a raw gateway link.
  const occ1 = main.locator('.af-auto-occ[data-index="1"]');
  await occ1.getByText("Run details").click();
  const [ledgerTab] = await Promise.all([page.waitForEvent("popup"), occ1.locator('[data-action="open-ledger-json"]').click()]);
  await ledgerTab.waitForLoadState();
  expect(await ledgerTab.evaluate(() => document.contentType)).toBe("application/json");
  expect(await ledgerTab.evaluate(() => location.protocol)).toBe("blob:");
  await ledgerTab.close();

  // Run now works again (a second run, again waiting for approval) — then deny it.
  await main.getByRole("button", { name: "Run now", exact: true }).click();
  await expect(main.locator('.af-auto-occ[data-index="2"]')).toBeVisible({ timeout: 30_000 });
  await expect(main.locator('.af-auto-occ[data-index="2"]').getByRole("button", { name: "Deny" })).toBeVisible({ timeout: 30_000 });
  await main.locator('.af-auto-occ[data-index="2"]').getByRole("button", { name: "Deny" }).click();
  await expect(main.locator('.af-auto-occ[data-index="2"] .af-auto-turn__status')).not.toHaveText(/waiting|running/, { timeout: 30_000 });

  // The folder is the automation's workspace_root, browsed through the gateway
  // with the shared WorkspaceBrowser.
  const folder = main.getByRole("region", { name: "Automation folder" });
  await expect(folder).toBeVisible();
  await expect(folder.locator('[data-path="fixture-tool-approval.txt"]')).toBeVisible({ timeout: 15_000 });
  // A text file opens in a new tab as plain text (never with a type that could run in the app origin).
  const [tab] = await Promise.all([
    page.waitForEvent("popup"),
    folder.locator('[data-path="fixture-tool-approval.txt"] [data-action="open-file"]').click(),
  ]);
  await tab.waitForLoadState();
  expect(await tab.evaluate(() => document.contentType)).toBe("text/plain");
  await tab.close();
  // The panel's workspace fact opens the same folder pane.
  await main.locator('[data-fact="workspace"] button').first().click();
  await expect(main.getByRole("region", { name: "Automation folder" })).toBeVisible();

  // Discuss run #1: the fork opens as THIS app's conversation, in place.
  await main.locator('.af-auto-occ[data-index="1"] [data-action="discuss"]').click();
  await main.getByLabel("Your message").fill("What did run 1 write?");
  await main.getByRole("button", { name: "Start discussion" }).click();
  await expect(page.locator(".code-automation-main")).toHaveCount(0);
  await expect(page.locator(".code-breadcrumb")).toContainText("Conversations");
  await expect(page.getByText("What did run 1 write?").first()).toBeVisible({ timeout: 15_000 });
  await expect(page).toHaveURL(/session=discussion-session/);
  // The fork says where it works: its own workspace, the automation's read-only.
  await expect(page.getByText(/Discussion forked from run #1\. It works in its own workspace .*mounted read-only/)).toBeVisible();
  await capture(page, "automation-discussion");

  // Archive: asks first; then hidden from the list, history kept (Show archived).
  await row.click();
  await main.getByRole("button", { name: "Archive…" }).click();
  await main.locator('[data-action="archive-confirm"]').click();
  await expect(row).toHaveCount(0, { timeout: 15_000 });
  await page.locator('[data-action="show-archived"]').check();
  await expect(row.locator('[data-field="state"] [data-state="archived"]')).toHaveText("Archived");
  await row.click();
  await expect(main.locator('.af-auto-occ[data-index="1"]')).toBeVisible();
  expect(errors).toEqual([]);
});

// DESIGN §12 (space on phones): the automation detail's own lists (occurrences, folder) are
// disclosures, open by default and remembered per viewer; at phone width the detail is flat
// (no card around an occurrence) and a fact shares one line with its label.
test("the automation detail's lists collapse, stay collapsed after a reload, and use the phone width", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  const title = `E2E lists ${Date.now()}`;
  await chooseWorkflow(page, "Native tool approval");
  await page.locator(".code-mobile-nav").click();
  await page.getByRole("button", { name: "New automation" }).click();
  const dialog = page.getByRole("dialog", { name: "Schedule a task" });
  await dialog.getByLabel("Task").fill("Write the fixture file");
  await dialog.getByText("Advanced").click();
  await dialog.getByLabel("Title").fill(title);
  await dialog.getByRole("button", { name: "Create automation" }).click();
  await expect(dialog).toBeHidden();

  const main = page.locator(".code-automation-main");
  await expect(main.locator(".af-auto-occ").first()).toBeVisible({ timeout: 30_000 });
  const occToggle = main.locator("#code-detail-occurrences-toggle");
  const folderToggle = main.locator("#code-detail-folder-toggle");
  await expect(occToggle).toHaveAttribute("aria-expanded", "true");
  await expect(folderToggle).toHaveAttribute("aria-expanded", "true");
  // The header sits right before the kit's timeline and names it.
  expect(await occToggle.evaluate((el) => el.parentElement?.nextElementSibling?.id)).toBe("code-detail-occurrences");
  expect((await occToggle.boundingBox())!.height).toBeGreaterThanOrEqual(32);

  // Phone width: an occurrence is a flat section, the When fact shares its label's line.
  const occ = main.locator(".af-auto-occ").first();
  expect(await occ.evaluate((el) => getComputedStyle(el).borderLeftWidth)).toBe("0px");
  const when = main.locator(".af-auto__head .af-auto__facts dt").first();
  const whenValue = main.locator(".af-auto__head .af-auto__facts dd").first();
  expect(Math.abs((await when.boundingBox())!.y - (await whenValue.boundingBox())!.y)).toBeLessThan(4);

  await occToggle.click();
  await expect(occToggle).toHaveAttribute("aria-expanded", "false");
  await expect(main.locator(".af-auto__timeline")).toBeHidden();
  await folderToggle.click();
  await expect(main.getByRole("region", { name: "Automation folder" })).toBeHidden();

  // Remembered per viewer: reopen the same automation after a reload.
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.locator(".code-mobile-nav").click();
  await page.locator(".code-auto-row", { hasText: title }).click();
  await expect(main.locator("#code-detail-occurrences-toggle")).toHaveAttribute("aria-expanded", "false");
  await expect(main.locator(".af-auto__timeline")).toBeHidden();
  await expect(main.locator("#code-detail-folder-toggle")).toHaveAttribute("aria-expanded", "false");
  await main.locator("#code-detail-occurrences-toggle").click();
  await expect(main.locator(".af-auto__timeline")).toBeVisible();
});
