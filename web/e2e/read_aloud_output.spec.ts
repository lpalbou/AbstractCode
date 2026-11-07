// Operator 2026-11-07 (Safari, Bluetooth headphones): after Stop the composer showed "Stopped";
// Read aloud was on but the new reply was not read, Play and the speaker Test stayed silent; the
// Output device control was a disabled "System default". Run in WebKit against the disposable
// fixture gateway (gateway_fixture.py) and the Code app on 18782:
//   npx playwright test e2e/read_aloud_output.spec.ts --browser=webkit
// Speech bytes are deterministic (silence); the conversation runs on the real fixture gateway.
import { closeWorkspaceDrawer, openWorkspaceSection } from "./drawer_navigation";
import { expect, test, type Page } from "@playwright/test";
import { mkdirSync } from "node:fs";

const appOrigin = process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782";
const fixtureGateway = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const fixtureUser = process.env.ABSTRACTCODE_E2E_USER || "web-tester";
const fixtureToken = process.env.ABSTRACTCODE_E2E_TOKEN || "abstractcode-e2e-only";
const shots = process.env.ABSTRACTCODE_AUDIO_SHOTS || "";

async function shot(page: Page, name: string): Promise<void> {
  if (!shots) return;
  mkdirSync(shots, { recursive: true });
  await page.screenshot({ path: `${shots}/${name}.png` });
}

/** One second of silence, 8 kHz mono PCM: real decoding and playback, nothing audible. */
function silence(): Buffer {
  const rate = 8000, data = rate * 2;
  const wav = Buffer.alloc(44 + data);
  wav.write("RIFF", 0); wav.writeUInt32LE(36 + data, 4); wav.write("WAVEfmt ", 8);
  wav.writeUInt32LE(16, 16); wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(rate, 24); wav.writeUInt32LE(rate * 2, 28); wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34);
  wav.write("data", 36); wav.writeUInt32LE(data, 40);
  return wav;
}

async function chooseWorkflow(page: Page, name: string): Promise<void> {
  await openWorkspaceSection(page, "Workflow");
  const picker = page.getByRole("combobox", { name: "Workflow", exact: true });
  await expect(picker).toBeEnabled();
  await picker.click();
  const exact = new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`);
  await page.getByRole("listbox", { name: "Workflow" }).locator('[role="option"]')
    .filter({ has: page.locator(".af-workflow-picker__name", { hasText: exact }) }).first().click();
  await expect(page.locator("#code-workflow-picker .af-workflow-picker__name")).toHaveText(name);
  await expect(page.locator(".code-workflow-select")).not.toHaveAttribute("aria-busy", "true");
  await closeWorkspaceDrawer(page);
}

async function send(page: Page, text: string): Promise<void> {
  await closeWorkspaceDrawer(page);
  await page.locator(".code-conversation .pc-composer textarea").fill(text);
  await page.getByRole("button", { name: "Send", exact: true }).click();
}

// The production build registers the PWA service worker; its fetches would bypass page.route.
test.use({ serviceWorkers: "block" });

type AudioProbe = { contexts: number; states: string[]; started: number; ended: number };
const audio = (page: Page): Promise<AudioProbe> =>
  page.evaluate(() => {
    const a = (window as any).__audio;
    return { contexts: a.contexts.length, states: a.contexts.map((c: AudioContext) => c.state), started: a.started, ended: a.ended };
  });

test("after Stop the composer is idle; Read aloud speaks each reply, also after a device change; Output device is always usable", async ({ page }) => {
  test.setTimeout(120_000);
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  // Count every AudioContext and every buffer source that starts and ends (playback really ran).
  await page.addInitScript(() => {
    const probe = { contexts: [] as AudioContext[], started: 0, ended: 0 };
    (window as any).__audio = probe;
    const Real = window.AudioContext;
    class Probed extends Real {
      constructor(...args: any[]) { super(...(args as [])); probe.contexts.push(this); }
      createBufferSource(): AudioBufferSourceNode {
        const source = super.createBufferSource();
        const start = source.start.bind(source);
        source.start = (...args: any[]) => { probe.started += 1; source.addEventListener("ended", () => { probe.ended += 1; }); return start(...(args as [])); };
        return source;
      }
    }
    (window as any).AudioContext = Probed;
  });
  // Only the local Code proxy is reachable; speech comes back as deterministic bytes.
  const allowed = new URL(appOrigin).origin;
  const speech: string[] = [];
  const wav = silence().toString("base64");
  await page.route("**/*", (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== allowed) return route.abort("blockedbyclient");
    if (!url.pathname.endsWith("/voice/tts/stream")) return route.continue();
    let text = "";
    try { text = String(JSON.parse(route.request().postData() || "{}").text || ""); } catch (e) { text = `unreadable body: ${String(e)}`; }
    speech.push(text);
    return route.fulfill({ contentType: "application/x-ndjson", body: [{ type: "start" }, { type: "audio", audio_b64: wav }, { type: "done" }].map((e) => JSON.stringify(e) + "\n").join("") });
  });

  await page.goto("/", { waitUntil: "domcontentloaded" });
  const dialog = page.getByRole("dialog", { name: "Gateway connection" });
  await page.locator("#gateway-session-url").fill(fixtureGateway);
  await page.locator("#gateway-session-user").fill(fixtureUser);
  await page.locator("#gateway-session-token").fill(fixtureToken);
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(dialog).toBeHidden();

  // 1. Stop: the composer goes back to mic + Send; "Stopped" is in the run status only.
  await chooseWorkflow(page, "Event listener");
  await send(page, "Wait for the event.");
  const composer = page.locator(".code-conversation .pc-composer");
  const primary = composer.locator(".pc-composer__send--compact");
  await expect(page.locator(".code-run-strip")).toContainText("Working"); // the run is live, nothing typed
  await expect(primary).toHaveAttribute("data-mode", "stop");
  await primary.click();
  await expect(page.locator(".code-run-strip")).toContainText("Stopped");
  await expect(primary).toHaveAttribute("data-mode", "send");
  await expect(primary).toHaveAttribute("aria-label", "Send");
  await expect(composer).not.toContainText("Stopped");
  await expect(composer.locator(".pc-workflow-chat__stop-state")).toHaveCount(0);
  // Beside the message: the microphone and Send only (Attach stays on the left, as before).
  expect(await composer.locator('button:visible:not([aria-label="Attach file"])').count()).toBe(2);
  await shot(page, "composer-after-cancel-desktop");
  await page.setViewportSize({ width: 400, height: 860 });
  await expect(primary).toHaveAttribute("data-mode", "send");
  await shot(page, "composer-after-cancel-400");
  await page.setViewportSize({ width: 1440, height: 960 });

  // 2. Settings → Voice: Read aloud on; the Output device control is enabled with what WebKit can do.
  await page.locator(".code-new-chat").click();
  const voice = await openWorkspaceSection(page, "Voice");
  const readAloud = voice.getByRole("switch", { name: /Read aloud/ });
  if ((await readAloud.getAttribute("aria-checked")) !== "true") await readAloud.click();
  await expect(readAloud).toHaveAttribute("aria-checked", "true");
  const output = voice.getByRole("combobox", { name: "Output device", exact: true });
  await expect(output).toBeEnabled();
  await expect(output).toContainText("Follow the system output (default)");
  await output.click();
  await expect(page.getByRole("option", { name: /^Follow the system output \(default\)/ })).toBeVisible();
  await expect(page.getByRole("option", { name: /^Re-open audio/ })).toBeVisible();
  await shot(page, "output-device-open-desktop");
  await page.keyboard.press("Escape");

  // 3. A reply arrives: it is spoken (the TTS answer decoded, a source started on a running context and ended).
  await chooseWorkflow(page, "Assistant contract");
  await send(page, "Say something short.");
  await expect(page.locator(".pc-chat-item--assistant").last()).toBeVisible();
  await expect.poll(() => speech.length, { timeout: 20_000 }).toBe(1);
  await expect.poll(async () => (await audio(page)).ended, { timeout: 15_000 }).toBeGreaterThanOrEqual(1);
  const first = await audio(page);
  expect(first.states.at(-1)).toBe("running");
  expect(first.started).toBeGreaterThanOrEqual(1);

  // 4. The devices change (headphones connect): the idle player is closed; the next reply plays on a new one.
  await page.evaluate(() => navigator.mediaDevices.dispatchEvent(new Event("devicechange")));
  await expect.poll(async () => (await audio(page)).states[first.contexts - 1], { timeout: 5_000 }).toBe("closed");
  await send(page, "And once more.");
  await expect.poll(() => speech.length, { timeout: 20_000 }).toBe(2);
  await expect.poll(async () => (await audio(page)).ended, { timeout: 15_000 }).toBeGreaterThan(first.ended);
  const second = await audio(page);
  expect(second.contexts).toBeGreaterThan(first.contexts);
  expect(second.states.at(-1)).toBe("running");

  // 5. Re-open audio: the player is rebuilt on the current output; the Test chime plays and says so.
  await openWorkspaceSection(page, "Voice");
  await output.click();
  await page.getByRole("option", { name: /^Re-open audio/ }).click();
  await expect(voice.locator('[data-voice-note="speaker"]')).toHaveText("Audio re-opened on the current system output. Press Test to hear it.");
  const reopened = await audio(page);
  expect(reopened.states[second.contexts - 1]).toBe("closed");
  expect(reopened.states.at(-1)).toBe("running");
  await expect(output).toContainText("Follow the system output (default)");
  await voice.locator('[data-action="test-speaker"]').click();
  await expect(voice.locator('[data-voice-note="speaker"]')).toContainText("Chime played.", { timeout: 10_000 });
  await voice.locator("[data-voice-output]").evaluate((el) => (el.closest("section") || el).scrollIntoView({ block: "start" }));
  await shot(page, "output-device-desktop");
  await page.setViewportSize({ width: 400, height: 860 });
  await openWorkspaceSection(page, "Voice");
  await voice.locator("[data-voice-output]").evaluate((el) => (el.closest("section") || el).scrollIntoView({ block: "start" }));
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  await shot(page, "output-device-400");
  expect(errors).toEqual([]);
});
