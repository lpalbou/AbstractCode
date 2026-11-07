// Web 0.15.3 (operator, 2026-11-07): AbstractNote's background work is not among Code's
// conversations or automations; a conversation whose workflow version is gone continues on the
// loaded version; a Notes session opened by link reads only. The fixture gateway
// (e2e/gateway_fixture.py) serves the app; the runs list, the bundles, the automations and the
// opened runs are the real-shaped records of src/workspace/fixtures/notes_background.json (read
// from a live gateway, GET only). Nothing is started: no Send reaches a gateway.
import { expect, test, type Page, type Route } from "@playwright/test";
import http from "node:http";
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createCodeServer } from "../bin/server.js";

const gateway = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const origin = process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782";
const TOKEN = process.env.ABSTRACTCODE_E2E_TOKEN || "abstractcode-e2e-only";
const shots = process.env.ABSTRACTCODE_E2E_SHOTS || "";
const F = JSON.parse(readFileSync(new URL("../src/workspace/fixtures/notes_background.json", import.meta.url), "utf8"));
const byRun = new Map<string, any>(F.runs.items.map((r: any) => [r.run_id, r]));
const runOf = (sessionId: string) => F.runs.items.find((r: any) => r.session_id === sessionId);
let app: http.Server;

test.beforeAll(async () => {
  app = createCodeServer({ defaultGatewayUrl: gateway });
  await new Promise<void>((resolve) => app.listen(Number(new URL(origin).port), "127.0.0.1", resolve));
});
test.afterAll(async () => {
  app?.closeAllConnections();
  await new Promise<void>((resolve) => app.close(() => resolve()));
});

/** The fixture records in place of the fixture gateway's own lists; no write leaves the page. */
async function route(page: Page): Promise<void> {
  await page.route("**/api/gateway/**", async (r: Route) => {
    const url = new URL(r.request().url());
    const path = url.pathname.replace(/^.*\/api\/gateway\//, "");
    const method = r.request().method();
    if (method !== "GET" && /^(runs\/start|commands|automations)/.test(path)) return r.fulfill({ status: 409, json: { detail: "e2e: no writes" } });
    if (path === "runs" && url.searchParams.get("root_only") === "true") {
      if (url.searchParams.get("archived_only") === "true") return r.fulfill({ json: { items: [], has_more: false, archived_sessions: 0 } });
      return r.fulfill({ json: F.runs });
    }
    if (path === "bundles" && url.searchParams.get("all_versions") === "true") return r.fulfill({ json: F.bundles });
    if (path.startsWith("bundles") && url.searchParams.get("executable_for")) return r.fulfill({ json: { ...F.bundles, executable_for: url.searchParams.get("executable_for") } });
    if (path === "automations") return r.fulfill({ json: url.searchParams.get("status") === "archived" ? F.automations_archived : F.automations });
    const input = /^runs\/([^/]+)\/input_data$/.exec(path);
    if (input && F.input_data[decodeURIComponent(input[1])]) return r.fulfill({ json: F.input_data[decodeURIComponent(input[1])] });
    const schema = /^bundles\/([^/]+)\/flows\/([^/]+)\/input_schema$/.exec(path);
    if (schema) {
      const id = `${decodeURIComponent(schema[1])}@${url.searchParams.get("bundle_version")}:${decodeURIComponent(schema[2])}`;
      return F.input_schemas[id] ? r.fulfill({ json: F.input_schemas[id] }) : r.fulfill({ status: 404, json: { detail: `e2e: no schema for ${id}` } });
    }
    if (path === "me/session-reads" && method === "GET") return r.fulfill({ json: { items: [], next_after: 0, more: false } });
    const view = /^runs\/([^/]+)\/view$/.exec(path);
    if (view && F.views[decodeURIComponent(view[1])]) return r.fulfill({ json: F.views[decodeURIComponent(view[1])] });
    if (/^runs\/[^/]+\/view\/stream$/.test(path)) return r.fulfill({ status: 204, body: "" });
    const one = /^runs\/([^/]+)$/.exec(path);
    if (one && byRun.has(decodeURIComponent(one[1]))) return r.fulfill({ json: byRun.get(decodeURIComponent(one[1])) });
    if (/^runs\/[^/]+\/(ledger|view|history_bundle|artifacts)/.test(path) || (path === "runs" && url.searchParams.get("session_id"))) {
      const rid = decodeURIComponent(path.split("/")[1] || "");
      if (byRun.has(rid) || url.searchParams.get("session_id")) return r.fulfill({ json: { items: [], next_after: 0, has_more: false } });
    }
    return r.continue();
  });
}

async function signIn(page: Page, hash = "") {
  await page.goto(`${origin}/${hash}`);
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

test("Conversations and Automations leave Notes' background work out; names are never ids", async ({ page }) => {
  await route(page);
  await signIn(page);
  const cards = page.locator(".code-session-item");
  await expect(cards).toHaveCount(3);
  await expect(cards.first()).toContainText(/Fix the failing parser test|Explain the retry loop|Draft the release notes/);
  await expect(page.locator(".code-session-item[data-item-id^='note-']")).toHaveCount(0);
  await expect(page.getByText(/Conversation note-wat|note-watch|note-web/)).toHaveCount(0);
  await expect(page.getByText(/Conversation acode/)).toHaveCount(0);
  // Search follows the same rule.
  await page.getByLabel("Search conversations").first().fill("note-watch");
  await expect(cards).toHaveCount(0);
  await page.getByLabel("Search conversations").first().fill("");
  const automations = page.locator(".code-automation-item");
  await expect(automations.filter({ hasText: "Morning news brief" })).toHaveCount(1);
  await expect(page.getByText(/Process my notes|Reflect on my notes|Consolidate my notes/)).toHaveCount(0);
  await shot(page, `list-${page.viewportSize()?.width}`);
});

test("a conversation on a retired version continues on the loaded one and says so", async ({ page }) => {
  await route(page);
  const old = runOf("acode-4b0e9d7c2f1a");
  await signIn(page, `#session=${old.session_id}&run=${old.run_id}`);
  await expect(page.locator('[data-field="version-note"]')).toHaveText(
    "This conversation started with a version of Basic agent that is no longer available. It continues with the version available now, so its next answers may differ.",
  );
  await expect(page.getByText(/exact workflow version is unavailable/)).toHaveCount(0);
  await expect(page.locator(".code-alert")).toHaveCount(0);
  await shot(page, `version-${page.viewportSize()?.width}`);
});

test("a conversation whose workflow is gone is refused in plain words", async ({ page }) => {
  await route(page);
  const gone = runOf("acode-9f8e7d6c5b4a");
  await signIn(page, `#session=${gone.session_id}&run=${gone.run_id}`);
  await expect(page.locator(".code-alert")).toContainText("The workflow this conversation used is no longer available");
});

test("a Notes session opened by link reads only, with a plain note and no version banner", async ({ page }) => {
  await route(page);
  const capture = runOf("note-watch-e88aa3c2-88fc-4657-8bb1-b3c250a52f46");
  await signIn(page, `#session=${capture.session_id}&run=${capture.run_id}`);
  await expect(page.locator(".pc-workflow-chat__blocked")).toHaveText(
    "This is part of the background work of AbstractNote (it files and processes your notes). You can read it here, but it cannot be continued from AbstractCode.",
  );
  await expect(page.getByText(/exact workflow version is unavailable/)).toHaveCount(0);
  await expect(page.locator(".code-alert")).toHaveCount(0);
  await expect(page.locator(".code-conversation .pc-composer textarea")).toBeDisabled();
  await expect(page.locator(".code-session-item")).toHaveCount(3);
  await shot(page, `notes-session-${page.viewportSize()?.width}`);
});

test.describe("at phone width", () => {
  test.use({ viewport: { width: 400, height: 860 }, hasTouch: true, isMobile: true });

  test("the drawer lists conversations only; a Notes session and a retired version read plainly", async ({ page }) => {
    await route(page);
    const capture = runOf("note-watch-e88aa3c2-88fc-4657-8bb1-b3c250a52f46");
    await signIn(page, `#session=${capture.session_id}&run=${capture.run_id}`);
    await expect(page.locator(".pc-workflow-chat__blocked")).toBeVisible();
    await shot(page, "notes-session-400");
    await page.getByRole("button", { name: "Open conversation navigation" }).click();
    await expect(page.locator(".code-session-item")).toHaveCount(3);
    // The open Notes session's own folder chip names its workspace; the drawer names no Notes work.
    await expect(page.locator(".code-sidebar").getByText(/Process my notes|Reflect on my notes|Consolidate my notes|note-wat/)).toHaveCount(0);
    await shot(page, "list-400");
    const old = runOf("acode-4b0e9d7c2f1a");
    await page.locator(`.code-session-item[data-item-id="${old.session_id}"]`).click();
    await expect(page.locator('[data-field="version-note"]')).toBeVisible();
    await expect(page.locator(".code-app--nav-open")).toHaveCount(0);
    await expect(page.locator(".code-sidebar")).not.toBeInViewport();
    await shot(page, "version-400");
  });
});
