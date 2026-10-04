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
  // The count before (a reused fixture gateway already holds archived automations): wait for THIS archive.
  const before = (await api("automations")).archived_automations || 0;
  await api(`automations/${encodeURIComponent(id)}/commands`, { method: "POST", body: JSON.stringify({ type: "automation.archive", command_id: `archive-${Date.now()}-${Math.random()}` }) });
  await expect.poll(async () => (await api("automations")).archived_automations).toBeGreaterThan(before);
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

// Round 6 (DESIGN R6.2): Archive from the card's "⋯" and from the conversation header's "⋯",
// inline confirm, POST /sessions/{id}/archive (no DELETE is ever sent), the conversation moves
// under `Archived · N` (N + 1), the ACTIVE one hands over to the next; Unarchive returns it.
async function startConversation(page: Page, prompt: string): Promise<string> {
  await page.getByRole("button", { name: "New conversation", exact: true }).first().click();
  const workflow = await openWorkspaceSection(page, "Workflow");
  await workflow.getByRole("combobox", { name: "Workflow", exact: true }).click();
  await page.getByRole("option").filter({ has: page.locator(".af-workflow-picker__name", { hasText: "Basic agent defaults" }) }).click();
  await expect(page.locator(".code-workflow-select")).not.toHaveAttribute("aria-busy", "true");
  await page.locator(".code-conversation .pc-composer textarea").fill(prompt);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.locator(".pc-chat-item--assistant").first()).toBeVisible();
  // The new conversation is the selected card once the list knows it.
  const current = page.locator(".code-conversations .code-session[aria-current='page'][data-session-id]");
  await expect.poll(async () => {
    await page.getByRole("button", { name: "Refresh conversations", exact: true }).click();
    return (await current.count()) ? String(await current.getAttribute("title")) : "";
  }).toContain(prompt);
  return String(await current.getAttribute("data-session-id"));
}

test("archive a conversation from the card ⋯ and from the header ⋯; Unarchive returns it", async ({ page }) => {
  const methods: string[] = [];
  page.on("request", (req) => { if (req.url().includes("/sessions/")) methods.push(`${req.method()} ${new URL(req.url()).pathname}`); });
  await signIn(page);
  const stamp = Date.now();
  const first = await startConversation(page, `Card archive ${stamp}`);
  const second = await startConversation(page, `Header archive ${stamp}`);
  const third = await startConversation(page, `Stays active ${stamp}`);
  await page.getByRole("button", { name: "Refresh conversations", exact: true }).click();
  const list = page.locator(".code-conversations");
  for (const id of [first, second, third]) await expect(list.locator(`.code-session[data-session-id="${id}"]`)).toBeVisible();
  const n0 = (await api("runs?root_only=true&limit=1")).archived_sessions || 0;

  // 1) The card ⋯ of a conversation that is NOT active: confirm, Cancel first, then Archive.
  const item = list.locator(`.code-session-item[data-item-id="${first}"]`);
  await item.hover();
  await item.getByRole("button", { name: /^More actions for / }).click();
  await page.getByRole("menuitem", { name: "Archive", exact: true }).click();
  const confirm = item.locator(".code-archive-confirm");
  await expect(confirm).toContainText("Archive this conversation? It stays searchable and auditable; it just leaves this list.");
  await confirm.locator('[data-action="cancel-archive"]').click();
  await expect(item.locator(".code-archive-confirm")).toHaveCount(0);
  expect((await api("runs?root_only=true&limit=1")).archived_sessions || 0).toBe(n0);
  await item.hover();
  await item.getByRole("button", { name: /^More actions for / }).click();
  await page.getByRole("menuitem", { name: "Archive", exact: true }).click();
  await item.locator('[data-action="confirm-archive"]').click();
  await expect(list.locator(`.code-session[data-session-id="${first}"]`)).toHaveCount(0);
  const footer = list.locator('.code-archived[data-list="conversations"]');
  await expect(footer.locator(".code-archived-toggle")).toHaveText(`Archived · ${n0 + 1}`);
  // The active conversation did not change.
  await expect(list.locator(`.code-session[data-session-id="${third}"]`)).toHaveAttribute("aria-current", "page");

  // 2) The header ⋯ on the ACTIVE conversation: it hands over to the next one.
  await list.locator(`.code-session[data-session-id="${second}"]`).click();
  await expect(list.locator(`.code-session[data-session-id="${second}"]`)).toHaveAttribute("aria-current", "page");
  await page.locator(".code-topbar").getByRole("button", { name: "Conversation actions", exact: true }).click();
  await page.getByRole("menuitem", { name: "Archive", exact: true }).click();
  const bar = page.locator(".code-archive-confirm-bar .code-archive-confirm");
  await expect(bar).toContainText("It stays searchable and auditable");
  await bar.locator('[data-action="confirm-archive"]').click();
  await expect(bar).toHaveCount(0);
  await expect(list.locator(`.code-session[data-session-id="${second}"]`)).toHaveCount(0);
  await expect(footer.locator(".code-archived-toggle")).toHaveText(`Archived · ${n0 + 2}`);
  await expect(list.locator(".code-session[aria-current='page']")).toHaveCount(1);
  await expect(list.locator(`.code-session[aria-current='page']`)).not.toHaveAttribute("data-session-id", second);
  await shot(page, "archived-after-header-archive");

  // Nothing deleted: the gateway still reads both sessions' runs.
  for (const id of [first, second]) expect(((await api(`runs?session_id=${encodeURIComponent(id)}&limit=5`)).items || []).length).toBeGreaterThan(0);
  expect(methods.filter((m) => m.startsWith("DELETE"))).toEqual([]);
  expect(methods.filter((m) => m.endsWith("/archive")).length).toBe(2);

  // 3) Both under Archived · N; Unarchive brings one back without a reload.
  const toggle = footer.locator(".code-archived-toggle");
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
  await expect(footer.locator(`.code-archived-row[data-id="${first}"]`)).toBeVisible();
  await expect(footer.locator(`.code-archived-row[data-id="${second}"]`)).toBeVisible();
  await footer.locator(`.code-archived-row[data-id="${first}"] [data-action="unarchive"]`).click();
  await expect(list.locator(`.code-session[data-session-id="${first}"]`)).toBeVisible();
  await expect(toggle).toHaveText(`Archived · ${n0 + 1}`);
  await footer.locator(`.code-archived-row[data-id="${second}"] [data-action="unarchive"]`).click();
  await expect(list.locator(`.code-session[data-session-id="${second}"]`)).toBeVisible();
});
