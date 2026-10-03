// Round 4 right panel against the disposable fixture gateway (e2e/gateway_fixture.py):
// gear-only Settings button; the kit rail drawer (Activity / Files / Settings) at the
// right edge — docked + resizable from 1024 px, floating below; files previewed in the
// shared viewer; activity in foldable groups; Settings bound to the selection (an
// automation's definition is saved as a new revision through the gateway); the
// Assistant-style Voice rows. Real gateway routes, no mocks, no model.
import { expect, test, type Page } from "@playwright/test";
import http from "node:http";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createCodeServer } from "../bin/server.js";
import { railTab } from "./drawer_navigation";

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

async function noHorizontalOverflow(page: Page) {
  expect(await page.evaluate(() => document.scrollingElement!.scrollWidth <= window.innerWidth + 1)).toBe(true);
}

async function shot(page: Page, name: string) {
  if (!shots) return;
  mkdirSync(shots, { recursive: true });
  await page.screenshot({ path: join(shots, `${name}.png`) });
}

for (const [name, width, height] of [["desktop", 1440, 960], ["tablet", 834, 1112], ["phone", 390, 844]] as const) {
  test.describe(name, () => {
    test.use({ viewport: { width, height }, hasTouch: name !== "desktop", isMobile: name === "phone" });

    test(`gear Settings, rail at the right edge, panels beside it (${name})`, async ({ page }) => {
      await signIn(page);
      // D1: the usual gear, icon-only, tooltip "Settings"; no sliders, no "Workspace & settings".
      const gear = page.locator(".code-topbar .code-panel-opener");
      await expect(gear).toHaveAttribute("aria-label", "Settings");
      await expect(gear).toHaveAttribute("title", "Settings");
      await expect(gear).toHaveText("");
      await expect(page.getByRole("button", { name: "Workspace & settings" })).toHaveCount(0);
      await expect(page.getByText("Workspace & settings")).toHaveCount(0);

      // D2: a vertical icon rail on the right edge; 44 px targets; no horizontal tab strip.
      const rail = page.getByRole("tablist", { name: "Workspace panels", exact: true });
      await expect(rail).toHaveAttribute("aria-orientation", "vertical");
      const tabs = rail.getByRole("tab");
      await expect(tabs).toHaveCount(3);
      for (const label of ["Activity", "Files", "Settings"]) {
        const box = (await railTab(page, label as any).boundingBox())!;
        expect(box.width).toBeGreaterThanOrEqual(44);
        expect(box.height).toBeGreaterThanOrEqual(44);
        expect(box.x + box.width).toBeGreaterThan(width - 52);
        expect(box.x + box.width).toBeLessThanOrEqual(width + 1);
      }
      await expect(page.locator(".code-rail .af-tabs, .code-rail [role=tablist][aria-orientation=horizontal]")).toHaveCount(0);

      // Opening a panel: beside the rail; the rail stays.
      await railTab(page, "Activity").click();
      const panel = page.locator("#code-rail-panel-activity");
      await expect(panel).toBeVisible();
      const railBox = (await rail.boundingBox())!;
      const wrapBox = (await page.locator(".code-rail .af-rail__panel-wrap").boundingBox())!;
      expect(wrapBox.x + wrapBox.width).toBeLessThanOrEqual(railBox.x + 1);
      await expect(rail).toBeVisible();
      await noHorizontalOverflow(page);

      if (width >= 1024) {
        // Docked: the conversation shrinks beside it, the separator resizes (keyboard + pointer), the width persists.
        const main = (await page.locator("#code-conversation").boundingBox())!;
        expect(main.x + main.width).toBeLessThanOrEqual(wrapBox.x + 1);
        const sep = page.getByRole("separator", { name: /Resize the Activity panel/ });
        const before = Number(await sep.getAttribute("aria-valuenow"));
        await sep.focus();
        await page.keyboard.press("ArrowLeft");
        await page.keyboard.press("ArrowLeft");
        await expect(sep).toHaveAttribute("aria-valuenow", String(before + 32));
        const sepBox = (await sep.boundingBox())!;
        await page.mouse.move(sepBox.x + sepBox.width / 2, sepBox.y + 200);
        await page.mouse.down();
        await page.mouse.move(sepBox.x - 60, sepBox.y + 200, { steps: 5 });
        await page.mouse.up();
        const dragged = Number(await sep.getAttribute("aria-valuenow"));
        expect(dragged).toBeGreaterThan(before + 32 + 40);
        await page.reload();
        await expect(page.locator("#code-rail-panel-activity")).toBeVisible();
        await expect(page.getByRole("separator", { name: /Resize the Activity panel/ })).toHaveAttribute("aria-valuenow", String(dragged));
      } else {
        // Overlay: floats over the content with a backdrop; Escape collapses and focus returns to the icon.
        await expect(page.locator(".code-rail .af-rail__backdrop")).toBeVisible();
        await page.keyboard.press("Escape");
        await expect(panel).toBeHidden();
        await expect(railTab(page, "Activity")).toBeFocused();
        await railTab(page, "Activity").click();
      }

      // Collapses to icons only: clicking the open icon closes the panel; the rail stays.
      await railTab(page, "Activity").click();
      await expect(page.locator("#code-rail-panel-activity")).toBeHidden();
      await expect(rail).toBeVisible();

      // The gear opens Settings in the rail; sections for the selected conversation.
      await gear.click();
      const settings = page.locator("#code-rail-panel-settings");
      await expect(settings).toBeVisible();
      await expect(railTab(page, "Settings")).toHaveAttribute("aria-selected", "true");
      await expect(settings.locator(".code-settings-binding")).toContainText("Conversation");
      for (const title of ["Model & behavior", "Tools & skills", "Workspace", "Voice"])
        await expect(settings.locator(".af-settings-group__title", { hasText: new RegExp(`^${title.replace(/[&]/g, "\\$&")}$`) })).toHaveCount(1);
      await expect(settings).not.toContainText("close the drawer and choose Edit");
      // D7: the Assistant's Voice layout, compact.
      const voice = settings.locator("#code-settings-voice");
      for (const label of ["Text → speech", "Speech → text", "Output device", "Voice latency"]) await expect(voice.getByText(label, { exact: true })).toBeVisible();
      await expect(voice.getByRole("switch", { name: /Read aloud/ })).toBeVisible();
      await expect(voice.locator('[data-setting="tts"] .af-override__summary')).toContainText("Gateway default");
      await expect(voice.locator('[data-setting="stt"] .af-override__summary')).toContainText("Gateway default");
      await expect(voice.locator('[data-action="use-default"]')).toHaveCount(0);
      await voice.getByRole("switch", { name: /Read aloud/ }).click();
      await expect(voice.getByRole("switch", { name: /Read aloud/ })).toHaveAttribute("aria-checked", "true");
      await noHorizontalOverflow(page);
      await shot(page, `settings-conversation-${name}`);
      await gear.click();
      await expect(settings).toBeHidden();
    });
  });
}

test("files preview in the shared viewer; activity groups; no Open button", async ({ page }) => {
  await signIn(page);
  await railTab(page, "Settings").click();
  const settingsPanel = page.locator("#code-rail-panel-settings");
  await settingsPanel.getByRole("combobox", { name: "Workflow", exact: true }).click();
  await page.getByRole("option").filter({ has: page.locator(".af-workflow-picker__name", { hasText: "Basic agent defaults" }) }).click();
  await expect(page.locator(".code-workflow-select")).not.toHaveAttribute("aria-busy", "true");
  await railTab(page, "Settings").click();
  await page.locator(".code-conversation .pc-composer textarea").fill("Rail drawer activity check.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.locator(".pc-chat-item--assistant").first()).toBeVisible();

  // Activity: foldable groups, the newest open.
  await railTab(page, "Activity").click();
  const activity = page.locator("#code-rail-panel-activity");
  const groups = activity.locator("details.code-activity-group");
  await expect(groups.first()).toBeVisible();
  const count = await groups.count();
  await expect(groups.nth(count - 1)).toHaveAttribute("open", "");
  for (let i = 0; i < count - 1; i++) await expect(groups.nth(i)).not.toHaveAttribute("open", "");

  // Files: write real files into the conversation's workspace on the gateway host.
  const runId = await page.evaluate(() => {
    const match = /run[=/]([0-9a-f-]{8,})/i.exec(location.href);
    return match?.[1] || "";
  });
  const sessions = await api("runs?limit=5");
  const run = runId || String((sessions.items || sessions.runs || [])[0]?.run_id || "");
  expect(run).not.toBe("");
  const where = await api(`runs/${encodeURIComponent(run)}/workspace`);
  mkdirSync(where.workspace_root, { recursive: true });
  writeFileSync(join(where.workspace_root, "README.md"), "# Plan\n\nSome **bold** text.\n");
  writeFileSync(join(where.workspace_root, "main.py"), "def hello():\n    return 42  # answer\n");
  writeFileSync(join(where.workspace_root, "dot.png"), Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==", "base64"));
  writeFileSync(join(where.workspace_root, "doc.pdf"), "%PDF-1.1\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj 2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj 3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n");
  await railTab(page, "Files").click();
  const files = page.locator("#code-rail-panel-files");
  await files.getByRole("button", { name: "Refresh the folder" }).first().click();
  const readme = files.locator('li[data-path="README.md"]');
  await expect(readme).toBeVisible();
  // Rows: name, size, generated date (relative, exact on hover), download icon; no "Open".
  await expect(readme.locator(".pc-ws__size")).not.toHaveText("");
  await expect(readme.locator("time.pc-ws__time")).toHaveText(/just now|min ago/);
  await expect(readme.locator("time.pc-ws__time")).toHaveAttribute("title", /\d{4}, \d{2}:\d{2}$/);
  await expect(readme.getByRole("button", { name: "Download README.md" })).toBeVisible();
  await expect(files.getByRole("button", { name: "Open", exact: true })).toHaveCount(0);
  // The root once, short, with open/copy icons.
  const root = files.locator(".pc-ws__root-name").first();
  await expect(root).toHaveAttribute("title", where.workspace_root);
  await expect(root).toHaveText(where.workspace_root.split("/").filter(Boolean).pop());
  await expect(files.getByRole("button", { name: "Copy the folder path" }).first()).toBeVisible();
  await expect(files.getByText(where.workspace_root, { exact: true })).toHaveCount(0);

  await readme.getByRole("button", { name: "README.md", exact: true }).click();
  const viewer = files.locator(".af-file-viewer");
  await expect(viewer.locator("strong", { hasText: "bold" })).toBeVisible();
  await expect(viewer.locator("h1, h2", { hasText: "Plan" })).toBeVisible();
  await expect(viewer.getByRole("button", { name: "Download README.md" })).toBeVisible();
  await expect(viewer.getByRole("button", { name: /Attach README\.md/ })).toBeVisible();
  await viewer.getByRole("button", { name: "Close the preview" }).click();

  await files.locator('li[data-path="main.py"]').getByRole("button", { name: "main.py", exact: true }).click();
  await expect(viewer.locator(".af-code__keyword", { hasText: "def" })).toBeVisible();
  await expect(viewer.locator(".af-code__comment", { hasText: "# answer" })).toBeVisible();
  await viewer.getByRole("button", { name: "Close the preview" }).click();

  await files.locator('li[data-path="dot.png"]').getByRole("button", { name: "dot.png", exact: true }).click();
  await expect(viewer.locator("img.af-file-viewer__image")).toBeVisible();
  await viewer.getByRole("button", { name: "Close the preview" }).click();

  await files.locator('li[data-path="doc.pdf"]').getByRole("button", { name: "doc.pdf", exact: true }).click();
  await expect(viewer.locator('object[type="application/pdf"]')).toHaveCount(1);
  await shot(page, "files-pdf-desktop");
  await noHorizontalOverflow(page);
});

test("automation selected: Settings edit its definition and save a new revision", async ({ page }) => {
  const created = await api("automations", {
    method: "POST",
    body: JSON.stringify({
      request_id: `rail-${Date.now()}`,
      title: `Rail settings ${Date.now()}`,
      target: { bundle_ref: "abstractcode-web-e2e@0.0.1", flow_id: "prompt-structured", input_data: { prompt: "Say hi" } },
      trigger: { source_id: "manual", source_version: 1, config: {} },
    }),
  });
  const id = created.automation_id;
  const rev0 = created.revision;
  await signIn(page);
  await page.locator(".code-sidebar").getByText(created.summary.title, { exact: true }).first().click();
  await railTab(page, "Settings").click();
  const settings = page.locator("#code-rail-panel-settings");
  await expect(settings.locator(".code-settings-binding")).toContainText("Automation");
  await expect(settings.getByTestId("automation-revision")).toHaveText(`Revision ${rev0}`);
  await expect(settings).not.toContainText("These are conversation settings");
  // Gateway default until overridden.
  await expect(settings.locator('input[placeholder="Workflow default"]').first()).toHaveValue("");
  const limit = settings.getByLabel("Iteration limit");
  await limit.fill("5");
  await expect(settings.locator(".code-settings-save")).toHaveText(`Saved as revision ${rev0 + 1}; applies from the next run.`, { timeout: 15_000 });
  await expect(settings.getByTestId("automation-revision")).toHaveText(`Revision ${rev0 + 1}`);
  const after = await api(`automations/${encodeURIComponent(id)}`);
  expect(after.definition.revision).toBe(rev0 + 1);
  expect(after.definition.target.input_data._limits.max_iterations).toBe(5);
  expect(after.definition.target.input_data.prompt).toBe("Say hi");
  // Back to the default removes the override (a new revision without it).
  await limit.fill("");
  await expect(settings.getByTestId("automation-revision")).toHaveText(`Revision ${rev0 + 2}`, { timeout: 15_000 });
  const reset = await api(`automations/${encodeURIComponent(id)}`);
  expect(reset.definition.target.input_data._limits?.max_iterations).toBeUndefined();
  // Activity: one group per occurrence (none yet).
  await railTab(page, "Activity").click();
  await expect(page.locator("#code-rail-panel-activity")).toContainText("No runs yet.");
  await shot(page, "settings-automation-desktop");
  await api(`automations/${encodeURIComponent(id)}/commands`, { method: "POST", body: JSON.stringify({ type: "automation.archive" }) }).catch(() => {});
});
