import { openWorkspaceSection, openWorkflowInputs, closeWorkspaceDrawer } from "./drawer_navigation";
import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// Email automations (framework backlog 0992 WP6) against the isolated fixture
// gateway (e2e/gateway_fixture.py, run with the email branches of core,
// runtime and gateway). No mailbox is ever reached: the fixture account points
// at 127.0.0.1:1 (refused) and is stored with `test: false`; every address is
// under the reserved example.test domain.
const appOrigin = process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782";
const fixtureGateway = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const fixtureUser = process.env.ABSTRACTCODE_E2E_USER || "web-tester";
const fixtureToken = process.env.ABSTRACTCODE_E2E_TOKEN || "abstractcode-e2e-only";
const auth = { Authorization: `Bearer ${fixtureToken}` };
const artifactsDir = join(fileURLToPath(new URL(".", import.meta.url)), "artifacts");

async function capture(page: Page, name: string): Promise<void> {
  mkdirSync(artifactsDir, { recursive: true });
  await page.screenshot({ path: join(artifactsDir, `${name}.png`), fullPage: true });
}

/** The header's kit WorkflowPicker (round 3: no "Show all workflows" — it lists only what the
 * gateway returns for abstractcode.agent.v1): open it and choose the entry named `name`. */
async function chooseWorkflow(page: Page, name: string): Promise<void> {
  await openWorkspaceSection(page, "Model & behavior");
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
  await openWorkspaceSection(page, "Model & behavior");
  await expect(page.getByLabel("Workflow", { exact: true })).toBeEnabled();
  await closeWorkspaceDrawer(page);
}

async function disconnect(request: APIRequestContext): Promise<void> {
  const r = await request.delete(`${fixtureGateway}/api/gateway/me/email`, { headers: auth });
  expect([200, 404]).toContain(r.status());
}

async function connectFixtureAccount(request: APIRequestContext): Promise<void> {
  const r = await request.put(`${fixtureGateway}/api/gateway/me/email`, {
    headers: auth,
    data: {
      address: "me@example.test",
      password: "fixture-only",
      imap: { host: "127.0.0.1", port: 1, security: "ssl" },
      smtp: { host: "127.0.0.1", port: 1, security: "ssl" },
      test: false,
    },
  });
  expect(r.status(), await r.text()).toBe(200);
  const status = await (await request.get(`${fixtureGateway}/api/gateway/me/email`, { headers: auth })).json();
  expect(status.effective_enabled).toBe(true);
}

async function definitionOf(request: APIRequestContext, title: string): Promise<any> {
  const page = await (await request.get(`${fixtureGateway}/api/gateway/automations?limit=200`, { headers: auth })).json();
  const row = page.items.find((s: any) => s.title === title);
  expect(row, `automation ${title} listed`).toBeTruthy();
  const detail = await (await request.get(`${fixtureGateway}/api/gateway/automations/${row.automation_id}`, { headers: auth })).json();
  return detail.definition;
}

async function openDialog(page: Page) {
  await chooseWorkflow(page, "Native tool approval");
  await page.getByRole("button", { name: "New automation" }).click();
  const dialog = page.getByRole("dialog", { name: "Schedule a task" });
  await expect(dialog).toBeVisible();
  return dialog;
}

test("without a usable account the email options say so and stay off", async ({ page, request }) => {
  await disconnect(request);
  await signIn(page);
  const dialog = await openDialog(page);
  await expect(dialog.getByText("Connect a mailbox first —").first()).toBeVisible();
  await expect(dialog.getByRole("button", { name: "open My email" }).first()).toBeVisible();
  await expect(dialog.getByRole("radio", { name: "When an email arrives" })).toBeDisabled();
  await expect(dialog.getByRole("switch", { name: "Email result", exact: true })).toBeDisabled();
  await capture(page, "email-not-set-up");
});

test("creates an email-triggered automation with filters and result recipients", async ({ page, request }) => {
  await connectFixtureAccount(request);
  await signIn(page);
  const title = `E2E email automation ${Date.now()}`;
  const dialog = await openDialog(page);
  await expect(dialog.getByText("Connect a mailbox first —")).toHaveCount(0);
  await dialog.getByLabel("Task").fill("Summarise the new invoices");
  await dialog.getByRole("radio", { name: "When an email arrives" }).check();
  await expect(dialog.getByText("checks once an hour by default")).toBeVisible();
  await expect(dialog.getByLabel("Check interval amount")).toHaveValue("1");
  await dialog.getByLabel("From these domains").fill("example.test");
  await dialog.getByLabel("Subject contains").fill("invoice");
  await dialog.getByRole("switch", { name: "Email result", exact: true }).check();
  await dialog.getByRole("radio", { name: "Me and these addresses" }).check();
  await dialog.getByRole("textbox", { name: "Me and these addresses" }).fill("colleague@example.test");
  await capture(page, "email-trigger-dialog");
  await dialog.getByText("Advanced").click();
  await dialog.getByLabel("Title").fill(title);
  await dialog.getByRole("button", { name: "Create automation" }).click();
  await expect(dialog).toBeHidden();

  const def = await definitionOf(request, title);
  expect(def.trigger.source_id).toBe("email.received");
  expect(def.trigger.source_version).toBe(1);
  expect(def.trigger.config).toMatchObject({ uses_model: true, every: "1h", max_batch: 100, filter: { from_domain_in: ["example.test"], subject_contains: "invoice" } });
  expect(def.policy.email_allowed_recipients).toEqual(["self"]);
  expect(def.notify).toEqual({ channels: ["console", "email"], recipients: ["self", "colleague@example.test"] });

  // The panel's definition card reads the same truth.
  await page.getByText("Definition").first().click();
  await expect(page.getByText("Me and colleague@example.test")).toBeVisible();
  await expect(page.getByText(/when an email arrives · from example\.test/).first()).toBeVisible();
});

test("Email result is stored as notify.channels email", async ({ page, request }) => {
  await connectFixtureAccount(request);
  await signIn(page);
  const title = `E2E email notify ${Date.now()}`;
  const dialog = await openDialog(page);
  await dialog.getByLabel("Task").fill("Check the build and tell me");
  await dialog.getByRole("switch", { name: "Email result", exact: true }).check();
  await dialog.getByText("Advanced").click();
  await dialog.getByLabel("Title").fill(title);
  await dialog.getByRole("button", { name: "Create automation" }).click();
  await expect(dialog).toBeHidden();
  const def = await definitionOf(request, title);
  expect(def.notify).toEqual({ channels: ["console", "email"] });
});
