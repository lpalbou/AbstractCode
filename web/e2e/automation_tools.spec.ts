import { openWorkspaceSection, openWorkflowInputs, closeWorkspaceDrawer } from "./drawer_navigation";
import { expect, test } from "@playwright/test";
import http from "node:http";
import { createCodeServer } from "../bin/server.js";

const gateway = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const origin = process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782";
let app: http.Server;
test.beforeAll(async () => {
  app = createCodeServer({ defaultGatewayUrl: gateway });
  await new Promise<void>(resolve => app.listen(Number(new URL(origin).port), "127.0.0.1", resolve));
});
test.afterAll(async () => {
  app?.closeAllConnections();
  await new Promise<void>(resolve => app.close(() => resolve()));
});

test("workflow and tool pickers persist; email recipients remain visible; refresh keeps sidebar layout", async ({ page }) => {
  await page.route("**/api/gateway/me/email", route => route.fulfill({ json: { configured: true, enabled: true, admin_enabled: true, effective_enabled: true } }));
  await page.goto(origin);
  await page.locator("#gateway-session-url").fill(gateway);
  await page.locator("#gateway-session-user").fill("web-tester");
  await page.locator("#gateway-session-token").fill("abstractcode-e2e-only");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await openWorkspaceSection(page, "Model & behavior");
  await page.getByRole("combobox", { name: "Workflow", exact: true }).click();
  await page.getByRole("option").filter({ has: page.locator(".af-workflow-picker__name", { hasText: "Basic agent defaults" }) }).click();
  await expect(page.locator(".code-workflow-select")).not.toHaveAttribute("aria-busy", "true");
  await closeWorkspaceDrawer(page);
  await page.locator(".code-conversation .pc-composer textarea").fill("Conversation kept during refresh");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.locator(".pc-chat-item--assistant").first()).toBeVisible();
  await page.getByRole("button", { name: "New automation", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Schedule a task" });
  await dialog.getByLabel("Task", { exact: true }).fill("Tool selection fixture");
  await dialog.getByRole("switch", { name: "Email result", exact: true }).click();
  await dialog.getByRole("radio", { name: "Me and these addresses", exact: true }).check();
  await dialog.getByRole("textbox", { name: "Me and these addresses", exact: true }).fill("reviewer@example.test");
  await dialog.getByRole("combobox", { name: "Automation workflow", exact: true }).click();
  await dialog.getByRole("option").filter({ has: page.locator(".af-workflow-picker__name", { hasText: "Authored model contract" }) }).click();
  await expect(dialog.getByRole("textbox", { name: "Me and these addresses", exact: true })).toHaveValue("reviewer@example.test");
  await expect(page.locator("#code-workflow-picker")).toContainText("Basic agent defaults");
  await page.screenshot({ path: "e2e/artifacts/automation-workflow-email-create.png" });
  await page.setViewportSize({ width: 390, height: 844 });
  const box = await dialog.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(390);
  expect(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
  await dialog.getByRole("textbox", { name: "Me and these addresses", exact: true }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: "e2e/artifacts/automation-workflow-email-mobile.png" });
  await page.setViewportSize({ width: 1440, height: 960 });
  // Exercise recipient controls without enabling real delivery in the isolated fixture.
  await dialog.getByRole("switch", { name: "Email result", exact: true }).click();
  const picker = dialog.locator('[data-field="tool-selection"]');
  await picker.getByLabel("Use workflow default tools").uncheck();
  const clear = picker.getByRole("button", { name: "Clear", exact: true });
  if (await clear.isEnabled()) await clear.click();
  await picker.getByRole("button", { name: /^Select/ }).click();
  await picker.getByRole("textbox", { name: "Filter options" }).fill("web_search");
  await picker.getByLabel("web_search", { exact: true }).check();
  await expect(picker.getByRole("button", { name: "Remove web_search" })).toBeVisible();
  await page.screenshot({ path: "e2e/artifacts/automation-tools-create.png" });
  const creation = page.waitForRequest(r => r.method() === "POST" && new URL(r.url()).pathname.endsWith("/automations"));
  await dialog.getByRole("button", { name: "Create automation", exact: true }).click();
  const createdBody = (await creation).postDataJSON();
  expect(createdBody.target.flow_id).toBe("authored-contract");
  expect(createdBody.target.input_data.tools).toEqual(["web_search"]);
  expect(createdBody.target.input_data._runtime.allowed_tools).toEqual(["web_search"]);
  const main = page.getByRole("main", { name: "Automation", exact: true });
  await expect(main.locator('[data-fact="workflow"]')).toContainText("authored-contract");
  await expect(page.getByRole("main", { name: "Automation", exact: true })).toBeVisible();
  await main.locator('[data-action="edit"]').click();
  const edit = main.locator(".af-auto__revise");
  await expect(edit.getByRole("button", { name: "Remove web_search" })).toBeVisible();
  await edit.getByRole("combobox", { name: "Automation workflow", exact: true }).click();
  await edit.getByRole("option").filter({ has: page.locator(".af-workflow-picker__name", { hasText: "Basic agent defaults" }) }).click();
  await page.screenshot({ path: "e2e/artifacts/automation-workflow-edit.png" });
  await edit.getByRole("button", { name: "Clear", exact: true }).click();
  const revision = page.waitForRequest(r => r.method() === "PATCH" && new URL(r.url()).pathname.includes("/automations/"));
  await edit.getByRole("button", { name: "Save changes", exact: true }).click();
  const body = (await revision).postDataJSON();
  expect(body.changes.target.flow_id).toBe("basic-agent-contract");
  expect(body.changes.target.input_data.tools).toEqual([]);
  expect(body.changes.target.input_data._runtime.allowed_tools).toEqual([]);
  await expect(edit).not.toBeVisible();
  // PATCH queues a durable command; wait for the committed revision before reopening.
  await expect(main.locator('[data-fact="revision"]')).toHaveText("2");
  await expect(main.locator('[data-fact="workflow"]')).toContainText("basic-agent-contract");
  await main.locator('[data-action="edit"]').click();
  await expect(edit.getByRole("button", { name: /^Select.*No tools enabled/ })).toBeVisible();
  await expect(edit.getByLabel("Use workflow default tools")).not.toBeChecked();
  await edit.getByRole("combobox", { name: "Automation workflow", exact: true }).click();
  await edit.getByRole("option").filter({ has: page.locator(".af-workflow-picker__name", { hasText: "Prompt structured" }) }).click();
  let invalidPatch = false;
  const watchInvalidPatch = (request: import("@playwright/test").Request) => { if (request.method() === "PATCH" && new URL(request.url()).pathname.includes("/automations/")) invalidPatch = true; };
  page.on("request", watchInvalidPatch);
  await edit.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(main.getByRole("alert")).toContainText(/ticket/i);
  expect(invalidPatch).toBe(false);
  page.off("request", watchInvalidPatch);
  await edit.getByRole("button", { name: "Cancel", exact: true }).click();

  await page.route("**/api/gateway/**", async route => {
    if (route.request().method() === "GET") await new Promise(resolve => setTimeout(resolve, 450));
    await route.continue();
  });
  // Compare position in the scrollable list, since clicking an offscreen refresh
  // button may legitimately scroll a fixture with many previous automations.
  const position = async () => page.locator("#code-panel-conversations-toggle").evaluate(el =>
    el.getBoundingClientRect().y + (el.closest(".code-sidebar-lists")?.scrollTop || 0));
  const before = await position();
  for (const name of ["Refresh automations", "Refresh conversations"]) {
    const refresh = page.getByRole("button", { name, exact: true });
    await refresh.click();
    await expect(refresh).toBeDisabled();
    await expect(page.getByText("Loading automations…", { exact: true })).not.toBeVisible();
    await expect(page.getByText("Loading conversations…", { exact: true })).not.toBeVisible();
    expect(await position()).toBe(before);
    await expect(refresh).toBeEnabled();
  }
  const sessions = page.locator(".code-sessions .code-session");
  const titles = await sessions.allTextContents();
  expect(titles.length).toBeGreaterThan(0);
  await page.route("**/api/gateway/runs?*", route => route.fulfill({ status: 503, json: { detail: "Temporary fixture failure" } }));
  await page.getByRole("button", { name: "Refresh conversations", exact: true }).click();
  await expect(page.getByRole("button", { name: "Refresh conversations", exact: true })).toBeEnabled();
  expect(await sessions.allTextContents()).toEqual(titles);
});
