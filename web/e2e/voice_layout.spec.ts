import { expect, test } from "@playwright/test";
import http from "node:http";
import { createCodeServer } from "../bin/server.js";

// Real app proxy + real disposable Gateway; only synthesis is a deterministic
// delayed WAV stream so this cannot invoke a paid or operator voice backend.
const fixture = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
let upstream: http.Server, app: http.Server, origin: string, gatewayUrl: string;
let finished = false, disconnected = false, calls = 0;
function wav() {
  const n = 16000 * 3, b = Buffer.alloc(44 + n * 2);
  b.write("RIFF"); b.writeUInt32LE(b.length - 8, 4); b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(1, 22);
  b.writeUInt32LE(16000, 24); b.writeUInt32LE(32000, 28);
  b.writeUInt16LE(2, 32); b.writeUInt16LE(16, 34); b.write("data", 36); b.writeUInt32LE(n * 2, 40);
  return b.toString("base64");
}
async function listen(server: http.Server, port = 0) {
  await new Promise<void>(resolve => server.listen(port, "127.0.0.1", resolve));
  return `http://127.0.0.1:${(server.address() as any).port}`;
}
test.beforeAll(async () => {
  upstream = http.createServer((req, res) => {
    if (req.url?.endsWith("/voice/tts/stream")) {
      calls++; finished = false; disconnected = false;
      res.writeHead(200, { "Content-Type": "application/x-ndjson" });
      res.write(JSON.stringify({ type: "start" }) + "\n");
      const first = setTimeout(() => res.write(JSON.stringify({ type: "audio", audio_b64: wav() }) + "\n"), 700);
      const done = setTimeout(() => { finished = true; res.end(JSON.stringify({ type: "done" }) + "\n"); }, 12000);
      res.on("close", () => { disconnected = true; clearTimeout(first); clearTimeout(done); });
      req.resume(); return;
    }
    const target = new URL(req.url || "/", fixture);
    const forward = http.request(target, { method: req.method, headers: { ...req.headers, host: target.host } }, r => {
      res.writeHead(r.statusCode || 500, r.headers); r.pipe(res);
    });
    forward.on("error", e => { res.writeHead(502); res.end(String(e)); });
    req.pipe(forward);
  });
  gatewayUrl = await listen(upstream);
  app = createCodeServer({ defaultGatewayUrl: gatewayUrl });
  origin = await listen(app, Number(new URL(process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782").port));
});
test.afterAll(async () => {
  app?.closeAllConnections(); upstream?.closeAllConnections();
  await Promise.all([app, upstream].map(s => new Promise<void>(resolve => s.close(() => resolve()))));
});

test("conversation and automation reuse narration, stream early and use the available width", async ({ page }) => {
  await page.route("**/api/gateway/discovery/capabilities", async route => {
    const response = await route.fetch(); const data = await response.json();
    data.capabilities.contracts.assistant.voice.tts = { available: true };
    await route.fulfill({ response, json: data });
  });
  await page.goto(origin);
  await page.locator("#gateway-session-url").fill(gatewayUrl);
  await page.locator("#gateway-session-user").fill("web-tester");
  await page.locator("#gateway-session-token").fill("abstractcode-e2e-only");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.getByRole("combobox", { name: "Workflow", exact: true }).click();
  await page.getByRole("option").filter({ has: page.locator(".af-workflow-picker__name", { hasText: "Basic agent defaults" }) }).click();
  await page.locator(".pc-composer textarea").fill("A visually distinct user request.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  const user = page.locator(".pc-chat-item--user").first();
  await expect(user).toHaveCSS("text-align", "right");
  expect(await user.evaluate(e => getComputedStyle(e).backgroundColor)).not.toBe("rgba(0, 0, 0, 0)");
  const speaker = page.getByRole("button", { name: "Speak (TTS)", exact: true }).last();
  await speaker.click();
  const spinner = page.locator(".pc-chat-speak-spinner");
  await expect(spinner).toBeVisible();
  await expect(spinner).toHaveCSS("animation-name", "pc-chat-speak-spin");
  await expect(page.getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  expect(finished).toBe(false); expect(calls).toBe(1);
  await page.getByRole("button", { name: "Stop spoken reply", exact: true }).click();
  await expect.poll(() => disconnected).toBe(true);
  await page.getByRole("button", { name: "New automation", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Schedule a task" });
  await dialog.getByLabel("Task").fill("A visually distinct automation trigger.");
  await dialog.getByText("Advanced", { exact: true }).click();
  await dialog.getByLabel("Title").fill(`Voice layout ${Date.now()}`);
  await dialog.getByRole("button", { name: "Create automation", exact: true }).click();
  const main = page.getByRole("main", { name: "Automation", exact: true });
  const answer = main.locator(".pc-chat-item--assistant").first();
  await expect(answer).toBeVisible();
  const trigger = main.locator(".pc-chat-item--user").first();
  await expect(trigger).toHaveCSS("text-align", "right");
  await expect(answer.getByRole("button", { name: "Speak (TTS)", exact: true })).toBeVisible();
  const bodyWidth = (await main.locator(".code-auto-main-body").boundingBox())!.width;
  expect((await answer.boundingBox())!.width).toBeGreaterThan(bodyWidth * .85);
  await answer.getByRole("button", { name: "Speak (TTS)", exact: true }).click();
  await expect(answer.locator(".pc-chat-speak-spinner")).toBeVisible();
  await expect(answer.getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  expect(finished).toBe(false);
  await main.getByRole("button", { name: "Stop audio", exact: true }).click();
  await expect.poll(() => disconnected).toBe(true);
  await page.screenshot({ path: "e2e/artifacts/voice-layout-desktop.png" });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator(".code-sidebar")).toBeHidden();
  await trigger.scrollIntoViewIfNeeded();
  await expect(trigger).toHaveCSS("text-align", "right");
  expect(await main.evaluate(e => e.scrollWidth <= e.clientWidth + 1)).toBe(true);
  await page.screenshot({ path: "e2e/artifacts/voice-layout-mobile.png" });
});
