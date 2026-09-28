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
  await page.getByLabel("Show all workflows").check();
  await page.getByLabel("Workflow", { exact: true }).selectOption({ label: "Native tool approval" });

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
  await expect(row.locator('[data-field="state"]')).toHaveText("Active ▶");
  await expect(row.locator('[data-field="next"]')).toContainText("UTC");

  // The first run starts now and parks on a typed tool approval.
  const main = page.locator(".code-automation-main");
  await expect(main.getByRole("button", { name: "Approve" }).first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator(".code-breadcrumb")).toContainText(title);
  await capture(page, "automation-approval");
  await main.getByRole("button", { name: "Approve" }).first().click();
  await expect(main.locator('.af-auto-occ[data-index="1"] .af-auto-turn__status')).toHaveText("completed", { timeout: 30_000 });

  // Run now works again (a second run, again waiting for approval) — then deny it.
  await main.getByRole("button", { name: "Run now", exact: true }).click();
  await expect(main.locator('.af-auto-occ[data-index="2"]')).toBeVisible({ timeout: 30_000 });
  await expect(main.locator('.af-auto-occ[data-index="2"]').getByRole("button", { name: "Deny" })).toBeVisible({ timeout: 30_000 });
  await main.locator('.af-auto-occ[data-index="2"]').getByRole("button", { name: "Deny" }).click();
  await expect(main.locator('.af-auto-occ[data-index="2"] .af-auto-turn__status')).not.toHaveText(/waiting|running/, { timeout: 30_000 });

  // The folder is the automation's workspace_root, browsed through the gateway.
  const folder = main.locator(".code-auto-folder");
  await expect(folder.getByText("Automation folder")).toBeVisible();
  await expect(folder.getByText("fixture-tool-approval.txt")).toBeVisible({ timeout: 15_000 });

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
  await expect(row.locator('[data-field="state"]')).toHaveText("Archived ▪");
  await row.click();
  await expect(main.locator('.af-auto-occ[data-index="1"]')).toBeVisible();
  expect(errors).toEqual([]);
});
