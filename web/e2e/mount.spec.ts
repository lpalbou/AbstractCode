import { expect, test } from "@playwright/test";

// AbstractCode served THROUGH the gateway at /apps/code/ (the app-server kit's
// mount contract). Driven by a harness that starts a gateway managing this
// web build and hands over a one-use Open URL (the console's Open):
//   ABSTRACTCODE_E2E_URL=<gateway origin> ABSTRACTCODE_E2E_MOUNT_OPEN_URL=<open_url>
// Without the handover there is nothing to open, so the test does not run.
const openUrl = process.env.ABSTRACTCODE_E2E_MOUNT_OPEN_URL || "";

test.skip(!openUrl, "needs a gateway-managed AbstractCode and its Open URL (ABSTRACTCODE_E2E_MOUNT_OPEN_URL)");

test("works under /apps/code/: automations, approval, folder and discuss", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const gatewayRequestsOutsideTheMount: string[] = [];
  page.on("request", (r) => {
    const u = new URL(r.url());
    if (u.pathname.startsWith("/api/")) gatewayRequestsOutsideTheMount.push(u.pathname);
  });

  // The handover signs the browser in and lands on the app.
  await page.goto(openUrl);
  await expect(page).toHaveURL(/\/apps\/code\/(#.*)?$/);
  await expect(page.locator(".code-statusbar").getByText("Connected", { exact: true })).toBeVisible();

  // Create an automation with the gateway default agent (the toolbar's default).
  const title = `Mounted automation ${Date.now()}`;
  await page.getByRole("button", { name: "New automation" }).click();
  const dialog = page.getByRole("dialog", { name: "Schedule a task" });
  await dialog.getByLabel("Task").fill("Write the tick file");
  await dialog.getByText("Ask me before each tool call").click();
  await dialog.getByText("Advanced").click();
  await dialog.getByLabel("Title").fill(title);
  await dialog.getByRole("button", { name: "Create automation" }).click();
  await expect(dialog).toBeHidden();

  const row = page.locator(".code-auto-row", { hasText: title });
  await expect(row.locator('[data-field="state"] [data-state="active"]')).toHaveText("Active");
  const main = page.locator(".code-automation-main");
  await expect(main.getByRole("button", { name: "Approve" }).first()).toBeVisible({ timeout: 45_000 });
  await main.getByRole("button", { name: "Approve" }).first().click();
  await expect(main.locator('.af-auto-occ[data-index="1"] .af-auto-turn__status')).toHaveText("completed", { timeout: 45_000 });

  // The automation's folder, through the mount.
  const folder = main.getByRole("region", { name: "Automation folder" });
  await expect(folder.locator('[data-path="tick.txt"]')).toBeVisible({ timeout: 15_000 });

  // Discuss run #1: the fork opens here as a conversation, and asks before its tool.
  await main.locator('.af-auto-occ[data-index="1"] [data-action="discuss"]').click();
  await main.getByLabel("Your message").fill("What did run 1 write?");
  await main.getByRole("button", { name: "Start discussion" }).click();
  await expect(page).toHaveURL(/\/apps\/code\/#session=discussion-session/);
  await expect(page.getByRole("button", { name: "Allow once" })).toBeVisible({ timeout: 30_000 });
  await page.getByRole("button", { name: "Allow once" }).click();

  // Every gateway call went through the app's own base.
  expect(gatewayRequestsOutsideTheMount).toEqual([]);
  expect(errors).toEqual([]);
});
