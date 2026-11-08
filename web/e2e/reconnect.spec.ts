// Backlog 1163: the page reconnects its live streams after the app server or the gateway restarts
// under it, and says so meanwhile. 2026-11-08 the Code app server was restarted under the
// operator's open page; nineteen minutes later the run parked on a tool approval, the tray said
// "needs you", and the page kept showing "Running" for seven hours.
//
// This spec owns its own disposable fixture gateway and Code app server (ports 18791 / 18792, a
// fresh root under $TMPDIR), so it can stop and relaunch each process under the open page:
//   - the app server: SIGTERM then a new `node bin/cli.js`, the way the gateway restarts an app;
//   - the gateway: SIGTERM then the same fixture on the same root.
// The run is "Delayed tool approval" (a 12 s timer, then the write_file approval), so the wait is
// raised after the page has lost its streams. Each test writes what the page showed, second by
// second, to $RECONNECT_LOG/<name>.json (the status bar, the composer, the approval card).
// Run alone: npx playwright test e2e/reconnect.spec.ts --workers=1 (no other fixture is needed).
import { expect, test, type Page } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, openSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { closeWorkspaceDrawer, openWorkspaceSection } from "./drawer_navigation";

const WEB = fileURLToPath(new URL("..", import.meta.url));
const FRAMEWORK = join(WEB, "..", "..");
const PYTHON = process.env.ABSTRACTCODE_E2E_PYTHON || join(FRAMEWORK, ".venv", "bin", "python");
const GATEWAY_PORT = 18791;
const APP_PORT = 18792;
const gateway = `http://127.0.0.1:${GATEWAY_PORT}`;
const origin = `http://127.0.0.1:${APP_PORT}`;
const TOKEN = "abstractcode-e2e-only";
const logDir = process.env.RECONNECT_LOG || "";
const shots = process.env.RECONNECT_SHOTS || "";
const root = mkdtempSync(join(tmpdir(), "code-reconnect-"));

let gatewayProcess: ChildProcess | null = null;
let appProcess: ChildProcess | null = null;

const cleanEnv = () => {
  const env = { ...process.env };
  delete env.ABSTRACTGATEWAY_AUTH_TOKEN;
  delete env.ABSTRACTGATEWAY_URL;
  // The servers get a scratch HOME (nothing under the operator's home is read or written).
  env.HOME = join(root, "home");
  return env;
};

async function until(check: () => Promise<boolean>, what: string, ms = 30_000): Promise<void> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check().catch(() => false)) return;
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`timed out waiting for ${what}`);
}

const answers = (url: string) => async () => {
  const r = await fetch(url);
  return r.status < 500;
};

let gatewayStarts = 0;
async function startGateway(): Promise<void> {
  const again = gatewayStarts++ > 0 ? ["--restart"] : [];
  gatewayProcess = spawn(PYTHON, [join(WEB, "e2e", "gateway_fixture.py"), "--port", String(GATEWAY_PORT), "--root", root, "--app-origin", origin, ...again], { env: cleanEnv(), stdio: ["ignore", openSync(join(root, "gateway.log"), "a"), openSync(join(root, "gateway.log"), "a")] });
  // Ready = an authenticated read answers (the first one warms the gateway up).
  await until(async () => (await fetch(`${gateway}/api/gateway/runs?limit=1`, { headers: { Authorization: `Bearer ${TOKEN}` } })).ok, "the fixture gateway", 60_000);
}

async function startApp(): Promise<void> {
  appProcess = spawn(process.execPath, [join(WEB, "bin", "cli.js"), "--port", String(APP_PORT), "--gateway-url", gateway], { cwd: WEB, env: cleanEnv(), stdio: ["ignore", openSync(join(root, "app.log"), "a"), openSync(join(root, "app.log"), "a")] });
  await until(answers(`${origin}/`), "the Code app server");
}

async function stop(child: ChildProcess | null): Promise<void> {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
  child.kill("SIGTERM");
  await Promise.race([exited, new Promise((r) => setTimeout(r, 10_000))]);
  if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
}

test.describe.configure({ mode: "serial" });
test.use({ serviceWorkers: "block", baseURL: origin });

test.beforeAll(async () => {
  await startGateway();
  await startApp();
});
test.afterAll(async () => {
  await stop(appProcess);
  await stop(gatewayProcess);
});

async function signIn(page: Page): Promise<void> {
  await page.goto(origin, { waitUntil: "domcontentloaded" });
  const dialog = page.getByRole("dialog", { name: "Gateway connection" });
  await expect(dialog).toBeVisible();
  await page.locator("#gateway-session-url").fill(gateway);
  await page.locator("#gateway-session-user").fill("web-tester");
  await page.locator("#gateway-session-token").fill(TOKEN);
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(dialog).toBeHidden();
}

async function startDelayedApproval(page: Page): Promise<void> {
  await openWorkspaceSection(page, "Workflow");
  const picker = page.getByRole("combobox", { name: "Workflow", exact: true });
  await expect(picker).toBeEnabled();
  await picker.click();
  await page.getByRole("listbox", { name: "Workflow" }).locator('[role="option"]').filter({ has: page.locator(".af-workflow-picker__name", { hasText: /^Delayed tool approval$/ }) }).first().click();
  await expect(page.locator(".code-workflow-select")).not.toHaveAttribute("aria-busy", "true");
  await closeWorkspaceDrawer(page);
  await page.locator(".code-conversation .pc-composer textarea").fill("Write the fixture file after the timer.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.locator(".code-run-strip")).toBeVisible({ timeout: 15_000 });
}

type Sample = { t: number; strip: string; send: string; composer: string; card: boolean };

async function sample(page: Page, t0: number): Promise<Sample> {
  const strip = (await page.locator(".code-run-strip").first().innerText().catch(() => "")).replace(/\s+/g, " ").trim();
  const sendButton = page.getByRole("button", { name: /^(Send|Stop)$/ }).first();
  const send = await sendButton.evaluate((el) => `${el.getAttribute("aria-label") || el.textContent}${(el as HTMLButtonElement).disabled ? " (disabled)" : ""}`).catch(() => "");
  const composer = (await page.locator(".code-conversation .pc-composer").first().evaluate((el) => {
    const notes = Array.from(el.closest(".code-conversation")?.querySelectorAll("[role=status], .code-composer-note, .pc-composer__notice") || []);
    return notes.map((n) => (n as HTMLElement).innerText).join(" | ");
  }).catch(() => "")).replace(/\s+/g, " ").trim();
  const card = await page.getByRole("heading", { name: "1 action needs permission" }).isVisible().catch(() => false);
  return { t: Math.round((Date.now() - t0) / 100) / 10, strip, send, composer, card };
}

/** Restarts one process under the open page and records, each second, what the page shows. */
async function restartUnderPage(page: Page, name: string, restart: () => Promise<{ down: number; up: number }>, t0: number) {
  const samples: Sample[] = [];
  let recording = true;
  const recorder = (async () => {
    while (recording) {
      samples.push(await sample(page, t0));
      await new Promise((r) => setTimeout(r, 1_000));
    }
  })();
  const { down, up } = await restart();
  let cardAt: number | null = null;
  const end = Date.now() + 60_000;
  while (Date.now() < end) {
    if (await page.getByRole("heading", { name: "1 action needs permission" }).isVisible().catch(() => false)) {
      cardAt = Math.round((Date.now() - t0) / 100) / 10;
      break;
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  // Healed: the status bar leaves "Reconnecting" and the composer is back.
  let healedAt: number | null = null;
  const healBy = Date.now() + 45_000;
  while (cardAt !== null && Date.now() < healBy) {
    const strip = await page.locator(".code-run-strip").first().innerText().catch(() => "");
    if (!/Reconnecting|Disconnected/.test(strip)) {
      healedAt = Math.round((Date.now() - t0) / 100) / 10;
      break;
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  await new Promise((r) => setTimeout(r, 1_500));
  recording = false;
  await recorder;
  const record = { name, down: Math.round((down - t0) / 100) / 10, up: Math.round((up - t0) / 100) / 10, cardAt, healedAt, samples };
  if (logDir) {
    mkdirSync(logDir, { recursive: true });
    writeFileSync(join(logDir, `${name}.json`), JSON.stringify(record, null, 2));
  }
  if (shots) {
    mkdirSync(shots, { recursive: true });
    await page.screenshot({ path: join(shots, `${name}.png`) });
  }
  return record;
}

async function gapShot(page: Page, name: string): Promise<void> {
  if (!shots) return;
  mkdirSync(shots, { recursive: true });
  await page.screenshot({ path: join(shots, `${name}.png`) });
  // The same moment at phone width: the notice and the status bar fit (no sideways scroll).
  const viewport = page.viewportSize();
  await page.setViewportSize({ width: 400, height: 860 });
  await page.waitForTimeout(300);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  await page.screenshot({ path: join(shots, `${name}-400.png`) });
  if (viewport) await page.setViewportSize(viewport);
}

/** The approval shows within seconds of the server's return; meanwhile the status bar says
 * "Reconnecting" (never a final "Disconnected") and the composer says why it waits. */
function expectRecovered(record: { up: number; down: number; cardAt: number | null; healedAt: number | null; samples: Sample[] }): void {
  const tail = JSON.stringify(record.samples.slice(-4));
  expect(record.cardAt, tail).not.toBeNull();
  // Within seconds of the server's return (and of the 12 s timer); the backoff tops out at 30 s.
  expect(record.cardAt! - Math.max(record.up, 12), tail).toBeLessThan(10);
  const gap = record.samples.filter((s) => s.t > record.down && s.t < record.up);
  expect(gap.some((s) => /Reconnecting/.test(s.strip)), JSON.stringify(gap)).toBe(true);
  expect(record.samples.filter((s) => /Disconnected/.test(s.strip)), "never a final Disconnected").toEqual([]);
  // Connected again (status bar, composer, card enabled) within moments of the card.
  expect(record.healedAt, tail).not.toBeNull();
  expect(record.healedAt! - record.cardAt!, tail).toBeLessThan(3);
  expect(gap.some((s) => /Reconnecting to this conversation\. You can send again as soon as it is back\./.test(s.composer)), JSON.stringify(gap)).toBe(true);
}

test("the app server restarts under the page: the approval raised after it appears, no reload", async ({ page }) => {
  test.setTimeout(180_000);
  await signIn(page);
  const t0 = Date.now();
  await startDelayedApproval(page);
  const record = await restartUnderPage(page, "app-server-restart", async () => {
    await new Promise((r) => setTimeout(r, 1_000));
    const down = Date.now();
    await stop(appProcess);
    await new Promise((r) => setTimeout(r, 7_000)); // the operator's gap (03:28:51 deploy)
    await gapShot(page, "app-server-restart-gap");
    await startApp();
    return { down, up: Date.now() };
  }, t0);
  expectRecovered(record);
});

test("the gateway restarts under the page: the approval appears, no reload", async ({ page }) => {
  test.setTimeout(180_000);
  await signIn(page);
  const t0 = Date.now();
  await startDelayedApproval(page);
  const record = await restartUnderPage(page, "gateway-restart", async () => {
    await new Promise((r) => setTimeout(r, 1_000));
    const down = Date.now();
    await stop(gatewayProcess);
    await new Promise((r) => setTimeout(r, 7_000));
    await gapShot(page, "gateway-restart-gap");
    await startGateway();
    return { down, up: Date.now() };
  }, t0);
  expectRecovered(record);
});
