// Round 5 (DESIGN R5.3): no "Show archived" switch; a quiet `Archived · N` footer at the end of EACH
// sidebar list (automations and conversations) opens the archived items inline, each with
// Unarchive. N is the gateway's (`archived_automations`, `archived_sessions`); the footer is absent
// at 0; whether it is open survives a reload. Real fixture gateway (e2e/gateway_fixture.py), no
// mocks, no model.
import { expect, test, type Page } from "@playwright/test";
import http from "node:http";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { createCodeServer } from "../bin/server.js";
import { openWorkspaceSection } from "./drawer_navigation";

const gateway = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const origin = process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782";
const TOKEN = process.env.ABSTRACTCODE_E2E_TOKEN || "abstractcode-e2e-only";
const shots = process.env.ABSTRACTCODE_E2E_SHOTS || "";
let app: http.Server;

test.beforeAll(async () => {
  app = createCodeServer({ defaultGatewayUrl: gateway });
  await new Promise<void>((resolve) => app.listen(Number(new URL(origin).port), "127.0.0.1", resolve));
});
test.afterAll(async () => {
  app?.closeAllConnections();
  await new Promise<void>((resolve) => app.close(() => resolve()));
});

async function api(path: string, init: RequestInit = {}) {
  const r = await fetch(`${gateway}/api/gateway/${path}`, {
    ...init,
    headers: { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json", ...(init.headers || {}) },
  });
  const text = await r.text();
  if (!r.ok) throw new Error(`${init.method || "GET"} ${path} -> ${r.status}: ${text}`);
  return text ? JSON.parse(text) : {};
}

async function signIn(page: Page) {
  await page.goto(origin);
  await page.locator("#gateway-session-url").fill(gateway);
  await page.locator("#gateway-session-user").fill("web-tester");
  await page.locator("#gateway-session-token").fill(TOKEN);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Gateway connection" })).toBeHidden();
}

async function shot(page: Page, name: string) {
  if (!shots) return;
  mkdirSync(shots, { recursive: true });
  await page.screenshot({ path: join(shots, `${name}.png`) });
}

test("automations: Archived · N footer, inline list, Unarchive brings it back (paused); no Show archived switch", async ({ page }) => {
  const title = `Archive me ${Date.now()}`;
  const created = await api("automations", {
    method: "POST",
    body: JSON.stringify({
      request_id: `arch-${Date.now()}`,
      title,
      target: { bundle_ref: "abstractcode-web-e2e@0.0.1", flow_id: "prompt-structured", input_data: { prompt: "Say hi" } },
      trigger: { source_id: "manual", source_version: 1, config: {} },
    }),
  });
  const id = created.automation_id;
  await api(`automations/${encodeURIComponent(id)}/commands`, { method: "POST", body: JSON.stringify({ type: "automation.archive", command_id: `archive-${Date.now()}-${Math.random()}` }) });
  await expect.poll(async () => (await api("automations")).archived_automations).toBeGreaterThan(0);
  const n = (await api("automations")).archived_automations;

  await signIn(page);
  const section = page.locator(".code-automations");
  await expect(section.getByText(/Show archived/)).toHaveCount(0);
  await expect(section.locator('[data-action="show-archived"]')).toHaveCount(0);
  await expect(section.locator(".code-auto-card", { hasText: title })).toHaveCount(0);
  const footer = section.locator('.code-archived[data-list="automations"]');
  const toggle = footer.locator(".code-archived-toggle");
  await expect(toggle).toHaveText(`Archived · ${n}`);
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await toggle.click();
  const row = footer.locator(`.code-archived-row[data-id="${id}"]`);
  await expect(row).toContainText(title);
  await shot(page, "archived-automations-open");
  // Remembered across a reload.
  await page.reload();
  await expect(page.locator('.code-archived[data-list="automations"] .code-archived-toggle')).toHaveAttribute("aria-expanded", "true");
  await page.locator(`.code-archived[data-list="automations"] .code-archived-row[data-id="${id}"] [data-action="unarchive"]`).click();
  // Back in the list (paused: the Active switch is off), gone from the archived ones.
  const card = section.locator(".code-auto-card", { hasText: title });
  await expect(card).toBeVisible();
  await expect(card.getByRole("switch", { name: "Active" })).toHaveAttribute("aria-checked", "false");
  await expect(page.locator(`.code-archived-row[data-id="${id}"]`)).toHaveCount(0);
  expect((await api(`automations/${encodeURIComponent(id)}`)).summary.status).toBe("paused");
  // A count of 0 leaves no footer at all.
  if (n === 1) await expect(section.locator(".code-archived")).toHaveCount(0);
  else await expect(page.locator('.code-archived[data-list="automations"] .code-archived-toggle')).toHaveText(`Archived · ${n - 1}`);
  await api(`automations/${encodeURIComponent(id)}/commands`, { method: "POST", body: JSON.stringify({ type: "automation.archive", command_id: `archive-${Date.now()}-${Math.random()}` }) }).catch(() => {});
});

test("conversations: Archived · N footer, inline list, Unarchive brings it back", async ({ page }) => {
  await signIn(page);
  const prompt = `Archive this conversation ${Date.now()}`;
  const workflow = await openWorkspaceSection(page, "Workflow");
  await workflow.getByRole("combobox", { name: "Workflow", exact: true }).click();
  await page.getByRole("option").filter({ has: page.locator(".af-workflow-picker__name", { hasText: "Basic agent defaults" }) }).click();
  await expect(page.locator(".code-workflow-select")).not.toHaveAttribute("aria-busy", "true");
  await page.locator(".code-conversation .pc-composer textarea").fill(prompt);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.locator(".pc-chat-item--assistant").first()).toBeVisible();
  const runs = await api("runs?root_only=true&limit=20");
  const sessionId = String((runs.items || []).find((r: any) => r.session_id)?.session_id || "");
  expect(sessionId).not.toBe("");
  await api(`sessions/${encodeURIComponent(sessionId)}/archive`, { method: "POST", body: "{}" });
  const n = (await api("runs?root_only=true&limit=1")).archived_sessions;
  expect(n).toBeGreaterThan(0);

  await page.getByRole("button", { name: "Refresh conversations", exact: true }).click();
  const list = page.locator(".code-conversations");
  await expect(list.locator(`.code-session[data-session-id="${sessionId}"]`)).toHaveCount(0);
  const footer = list.locator('.code-archived[data-list="conversations"]');
  await expect(footer.locator(".code-archived-toggle")).toHaveText(`Archived · ${n}`);
  await footer.locator(".code-archived-toggle").click();
  const row = footer.locator(`.code-archived-row[data-id="${sessionId}"]`);
  await expect(row).toBeVisible();
  await shot(page, "archived-conversations-open");
  await row.locator('[data-action="unarchive"]').click();
  await expect(list.locator(`.code-session[data-session-id="${sessionId}"]`)).toBeVisible();
  await expect(footer.locator(`.code-archived-row[data-id="${sessionId}"]`)).toHaveCount(0);
  expect((await api("runs?root_only=true&limit=1")).archived_sessions).toBe(n - 1);
});
