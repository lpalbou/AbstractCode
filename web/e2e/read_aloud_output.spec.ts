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

// Review 2026-11-08 D1: Stop pressed at once on the first message of a new conversation, while the
// gateway is still starting the run (its answer is held back 1.2 s here, as on a busy gateway).
// The run the gateway returns is stopped as soon as it is loaded: no "Could not stop" banner, no
// reply, "Stopped" at the top, the message box back to the microphone and Send.
test("Stop pressed while a new conversation's run is starting stops that run", async ({ page }) => {
  test.setTimeout(90_000);
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const allowed = new URL(appOrigin).origin;
  let started = 0;
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== allowed) return route.abort("blockedbyclient");
    if (!url.pathname.endsWith("/api/gateway/runs/start")) return route.continue();
    started += 1;
    const response = await route.fetch(); // the gateway starts the run now ...
    await new Promise((r) => setTimeout(r, 1200)); // ... and its answer arrives later
    return route.fulfill({ response });
  });
  await page.goto("/", { waitUntil: "domcontentloaded" });
  const dialog = page.getByRole("dialog", { name: "Gateway connection" });
  await page.locator("#gateway-session-url").fill(fixtureGateway);
  await page.locator("#gateway-session-user").fill(fixtureUser);
  await page.locator("#gateway-session-token").fill(fixtureToken);
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(dialog).toBeHidden();
  await chooseWorkflow(page, "Event listener");
  const composer = page.locator(".code-conversation .pc-composer");
  const primary = composer.locator(".pc-composer__send--compact");
  await page.locator(".code-conversation .pc-composer textarea").fill("Stop me at once.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  const sentAt = Date.now();
  await expect(primary).toHaveAttribute("aria-label", "Stop", { timeout: 1_000 });
  await primary.click();
  expect(Date.now() - sentAt).toBeLessThan(400); // pressed while the start is in flight
  // D3 (review 2026-11-08): the press is answered at once, long before the start returns.
  await expect(page.locator(".code-run-strip")).toContainText("Stopping…", { timeout: 600 });
  await expect(primary).toHaveAttribute("data-mode", "send", { timeout: 600 });
  await expect(page.locator(".code-conversation .pc-composer textarea")).toHaveValue("", { timeout: 600 });
  await expect(page.locator(".pc-chat-item--user").last()).toContainText("Stop me at once."); // the message stays on screen
  await shot(page, "stop-at-once-stopping-desktop");
  await expect(page.locator(".code-run-strip")).toContainText("Stopped", { timeout: 20_000 });
  // D4: the pointer still rests on the button it pressed; no "Stop" tooltip over the idle Send.
  await primary.hover();
  await page.waitForTimeout(600);
  const tips = await page.locator(".af-tooltip:not([hidden])").allTextContents();
  expect(tips.filter((t) => /Stop/.test(t))).toEqual([]);
  await expect(page.getByText(/Could not stop the run/)).toHaveCount(0);
  await expect(primary).toHaveAttribute("data-mode", "send");
  await expect(primary).toHaveAttribute("aria-label", "Send");
  await expect(composer).not.toContainText("Stopped");
  expect(await composer.locator('button:visible:not([aria-label="Attach file"])').count()).toBe(2);
  await expect(page.locator(".pc-chat-item--assistant")).toHaveCount(0);
  await expect(page.locator(".code-conversation .pc-composer textarea")).toHaveValue(""); // it was sent
  expect(started).toBe(1);
  await page.waitForTimeout(3_000); // nothing comes back later
  await expect(page.getByText(/Could not stop the run/)).toHaveCount(0);
  await expect(page.locator(".code-run-strip")).toContainText("Stopped");
  await shot(page, "stop-at-once-new-conversation-desktop");
  await page.setViewportSize({ width: 400, height: 860 });
  await expect(primary).toHaveAttribute("data-mode", "send");
  await shot(page, "stop-at-once-new-conversation-400");
  expect(errors).toEqual([]);
});

// R12 (review 2026-11-08): Stop while the run is starting, then a reload before the gateway
// answers. The page that pressed Stop is gone; the next page of the tab cancels that run.
test("a Stop pressed while starting is finished after a reload", async ({ page }) => {
  test.setTimeout(90_000);
  const allowed = new URL(appOrigin).origin;
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== allowed) return route.abort("blockedbyclient");
    if (!url.pathname.endsWith("/api/gateway/runs/start")) return route.continue();
    const response = await route.fetch(); // the gateway starts the run now ...
    await new Promise((r) => setTimeout(r, 4000)); // ... its answer comes after the reload
    return route.fulfill({ response }).catch(() => undefined);
  });
  await page.goto("/", { waitUntil: "domcontentloaded" });
  const dialog = page.getByRole("dialog", { name: "Gateway connection" });
  await page.locator("#gateway-session-url").fill(fixtureGateway);
  await page.locator("#gateway-session-user").fill(fixtureUser);
  await page.locator("#gateway-session-token").fill(fixtureToken);
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(dialog).toBeHidden();
  await chooseWorkflow(page, "Event listener");
  const primary = page.locator(".code-conversation .pc-composer .pc-composer__send--compact");
  await page.locator(".code-conversation .pc-composer textarea").fill("Stop me, then reload.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(primary).toHaveAttribute("aria-label", "Stop", { timeout: 1_000 });
  await primary.click();
  await expect(page.locator(".code-run-strip")).toContainText("Stopping…", { timeout: 600 });
  const key = "abstractcode.stop-while-starting.v1";
  const pending = await page.evaluate((k) => JSON.parse(sessionStorage.getItem(k) || "[]"), key);
  expect(pending).toHaveLength(1);
  await page.waitForTimeout(500); // the gateway has created the run; its answer is still held
  await page.reload({ waitUntil: "domcontentloaded" });
  await expect.poll(() => page.evaluate((k) => sessionStorage.getItem(k), key), { timeout: 30_000 }).toBeNull();
  const runs = await page.evaluate(async (sid) => {
    const r = await fetch(`api/gateway/runs?session_id=${encodeURIComponent(sid)}&root_only=true&limit=20`, { credentials: "same-origin" });
    return ((await r.json()).items || []).map((run: any) => ({ id: run.run_id, status: run.status }));
  }, pending[0].sessionId);
  expect(runs.length).toBeGreaterThanOrEqual(1);
  await expect.poll(async () => {
    const now = await page.evaluate(async (sid) => {
      const r = await fetch(`api/gateway/runs?session_id=${encodeURIComponent(sid)}&root_only=true&limit=20`, { credentials: "same-origin" });
      return ((await r.json()).items || []).map((run: any) => run.status);
    }, pending[0].sessionId);
    return now.every((status: string) => status === "cancelled");
  }, { timeout: 20_000 }).toBe(true);
});

async function signIn(page: import("@playwright/test").Page): Promise<void> {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  const dialog = page.getByRole("dialog", { name: "Gateway connection" });
  await page.locator("#gateway-session-url").fill(fixtureGateway);
  await page.locator("#gateway-session-user").fill(fixtureUser);
  await page.locator("#gateway-session-token").fill(fixtureToken);
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(dialog).toBeHidden();
}

// D5 (review 2026-11-08): Stop while the SECOND message of a conversation is starting. The previous
// run is already over ("Stopped" here); that must not end "Stopping…", and the message stays shown.
test("Stop while the second message of a conversation is starting", async ({ page }) => {
  test.setTimeout(120_000);
  const allowed = new URL(appOrigin).origin;
  let hold = 0;
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== allowed) return route.abort("blockedbyclient");
    if (!url.pathname.endsWith("/api/gateway/runs/start") || !hold) return route.continue();
    const response = await route.fetch();
    await new Promise((r) => setTimeout(r, hold));
    return route.fulfill({ response });
  });
  await signIn(page);
  await chooseWorkflow(page, "Event listener");
  const strip = page.locator(".code-run-strip");
  const primary = page.locator(".code-conversation .pc-composer .pc-composer__send--compact");
  const box = page.locator(".code-conversation .pc-composer textarea");
  // First message: started, then stopped normally (the conversation's previous run is over).
  await box.fill("First message.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(strip).toContainText("Working");
  await primary.click();
  await expect(strip).toContainText("Stopped", { timeout: 20_000 });
  // Second message: the gateway answers the start 2.5 s later; Stop pressed at once.
  hold = 2500;
  await box.fill("Second message.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(primary).toHaveAttribute("aria-label", "Stop", { timeout: 1_000 });
  await primary.click();
  // Sampled every 100 ms for 1.5 s: "Stopping…", never the previous run's "Stopped"; the message shown.
  for (let i = 0; i < 15; i++) {
    await expect(strip).toContainText("Stopping…", { timeout: 300 });
    await expect(page.locator(".pc-chat-item--user").last()).toContainText("Second message.");
    await page.waitForTimeout(100);
  }
  await expect(box).toHaveValue("");
  await shot(page, "stop-second-message-stopping-desktop");
  // Then the run this start returned is shown stopped.
  await expect(strip).toContainText("Stopped", { timeout: 20_000 });
  await expect(strip).not.toContainText("Stopping…");
  const runId = new URLSearchParams(new URL(page.url()).hash.slice(1)).get("run");
  const run = await page.evaluate(async (id) => (await (await fetch(`api/gateway/runs/${id}`, { credentials: "same-origin" })).json()), runId);
  expect(run.status).toBe("cancelled");
  await expect(page.locator(".pc-chat-item--user").filter({ hasText: "Second message." })).toHaveCount(1);
  await shot(page, "stop-second-message-stopped-desktop");
});

// D6 (review 2026-11-08): a start the network cut (no answer, page still here): the stop record is
// dropped and the person is told; nothing is left to match a later run.
test("a start cut by the network drops its stop and says so", async ({ page }) => {
  test.setTimeout(90_000);
  const allowed = new URL(appOrigin).origin;
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== allowed) return route.abort("blockedbyclient");
    if (!url.pathname.endsWith("/api/gateway/runs/start")) return route.continue();
    await new Promise((r) => setTimeout(r, 1200));
    return route.abort("failed");
  });
  await signIn(page);
  await chooseWorkflow(page, "Event listener");
  const primary = page.locator(".code-conversation .pc-composer .pc-composer__send--compact");
  await page.locator(".code-conversation .pc-composer textarea").fill("The network will cut this.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(primary).toHaveAttribute("aria-label", "Stop", { timeout: 1_000 });
  await primary.click();
  await expect(page.locator(".code-run-strip")).toContainText("Stopping…", { timeout: 600 });
  await expect(page.getByText("The gateway did not answer the start, so Code cannot tell whether a reply began. If one appears in this conversation, stop it.")).toBeVisible({ timeout: 10_000 });
  expect(await page.evaluate(() => sessionStorage.getItem("abstractcode.stop-while-starting.v1"))).toBeNull();
  await expect(page.locator(".code-run-strip")).toHaveCount(0);
});

// D6 + (b): a stop an earlier page left behind. While its run is not listed, the tab asks again
// every 3 s, never while the tab is hidden; a stop that matches several runs is never guessed:
// the conversation names them, one Stop each, and the person decides.
test("a left-behind stop waits only while the tab is visible, and never guesses between runs", async ({ page }) => {
  test.setTimeout(120_000);
  const allowed = new URL(appOrigin).origin;
  const lists: number[] = [];
  const commands: any[] = [];
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== allowed) return route.abort("blockedbyclient");
    if (url.pathname.endsWith("/api/gateway/runs") && url.searchParams.get("session_id") === "planted-wait") lists.push(Date.now());
    if (url.pathname.endsWith("/api/gateway/commands")) commands.push(route.request().postDataJSON());
    return route.continue();
  });
  await signIn(page);
  await chooseWorkflow(page, "Event listener");
  // A conversation of this test with two unfinished runs started a moment apart.
  const box = page.locator(".code-conversation .pc-composer textarea");
  await box.fill("Run one.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.locator(".code-run-strip")).toContainText("Working");
  const route1 = new URLSearchParams(new URL(page.url()).hash.slice(1));
  const sessionId = String(route1.get("session"));
  const runs = await page.evaluate(async (sid) => ((await (await fetch(`api/gateway/runs?session_id=${sid}&root_only=true&limit=20`, { credentials: "same-origin" })).json()).items || []).map((r: any) => ({ id: r.run_id, created: r.created_at })), sessionId);
  expect(runs.length).toBeGreaterThanOrEqual(1);
  const at = Date.parse(runs[0].created);
  // Plant: one stop waiting for a run that never comes (another conversation), one with two candidates here.
  await page.evaluate(({ sid, at, run }) => {
    sessionStorage.setItem("abstractcode.stop-while-starting.v1", JSON.stringify([
      { id: "wait", sessionId: "planted-wait", at: Date.now() },
      { id: "choose", sessionId: sid, at, candidates: [{ runId: run, createdAt: new Date(at).toISOString() }, { runId: "another-run", createdAt: new Date(at + 2000).toISOString() }] },
    ]));
  }, { sid: sessionId, at, run: runs[0].id });
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  const choice = page.locator(".code-stop-choice");
  await expect(choice).toBeVisible({ timeout: 10_000 });
  await expect(choice.getByRole("button", { name: /^Stop the reply started at .* \((1|2) of 2\)$/ })).toHaveCount(2);
  expect(commands).toEqual([]); // nothing guessed
  // The waiting stop asks every 3 s while visible ...
  await expect.poll(() => lists.length, { timeout: 10_000 }).toBeGreaterThanOrEqual(2);
  // ... and not at all while the tab is hidden.
  await page.evaluate(() => { Object.defineProperty(document, "hidden", { configurable: true, get: () => true }); document.dispatchEvent(new Event("visibilitychange")); });
  await page.waitForTimeout(1_000);
  const hiddenFrom = lists.length;
  await page.waitForTimeout(7_000);
  expect(lists.length).toBe(hiddenFrom);
  await page.evaluate(() => { Object.defineProperty(document, "hidden", { configurable: true, get: () => false }); document.dispatchEvent(new Event("visibilitychange")); });
  await expect.poll(() => lists.length, { timeout: 5_000 }).toBeGreaterThan(hiddenFrom);
  await shot(page, "stop-choice-desktop");
  // The person chooses: that run (and only it) is stopped; the choice is gone.
  await page.setViewportSize({ width: 400, height: 860 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  await shot(page, "stop-choice-400");
  await page.setViewportSize({ width: 1440, height: 960 });
  await choice.getByRole("button", { name: /^Stop the reply started at .*\(1 of 2\)$/ }).click();
  await expect(choice).toHaveCount(0);
  await expect.poll(() => commands.length).toBe(1);
  expect(commands[0]).toMatchObject({ run_id: runs[0].id, type: "cancel" });
  const left = await page.evaluate(() => JSON.parse(sessionStorage.getItem("abstractcode.stop-while-starting.v1") || "[]").map((p: any) => p.id));
  expect(left).toEqual(["wait"]);
});

// R13 + R14 (review 2026-11-08): sending again in a conversation drops a stop an earlier page left
// there BEFORE the start leaves, so the new run is never taken for that stop's run (the start's
// answer is held 4 s while the tab's retry lists the conversation); and a stop that finds no run
// is dropped with a sentence in its conversation, never silently.
test("sending again drops a left-behind stop before the start leaves; an unmatched stop says so", async ({ page }) => {
  test.setTimeout(120_000);
  const allowed = new URL(appOrigin).origin;
  const commands: any[] = [];
  let hold = 0;
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== allowed) return route.abort("blockedbyclient");
    if (url.pathname.endsWith("/api/gateway/commands")) commands.push(route.request().postDataJSON());
    if (!url.pathname.endsWith("/api/gateway/runs/start") || !hold) return route.continue();
    const response = await route.fetch();
    await new Promise((r) => setTimeout(r, hold));
    return route.fulfill({ response });
  });
  await signIn(page);
  await chooseWorkflow(page, "Event listener");
  const strip = page.locator(".code-run-strip");
  const primary = page.locator(".code-conversation .pc-composer .pc-composer__send--compact");
  const box = page.locator(".code-conversation .pc-composer textarea");
  await box.fill("First turn.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(strip).toContainText("Working");
  await primary.click();
  await expect(strip).toContainText("Stopped", { timeout: 20_000 });
  const sessionId = String(new URLSearchParams(new URL(page.url()).hash.slice(1)).get("session"));
  const before = commands.length;
  // A stop an earlier page left here, waiting for its run (none listed yet).
  await page.evaluate((sid) => sessionStorage.setItem("abstractcode.stop-while-starting.v1", JSON.stringify([{ id: "left", sessionId: sid, at: Date.now(), sentAt: Date.now() }])), sessionId);
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  // Send again; its answer is held 4 s while the retry runs every 3 s.
  hold = 4000;
  await box.fill("Second turn, not to be stopped.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  expect(await page.evaluate(() => sessionStorage.getItem("abstractcode.stop-while-starting.v1"))).toBeNull();
  await page.waitForTimeout(6_000);
  hold = 0;
  await expect(strip).toContainText("Working", { timeout: 20_000 });
  expect(commands.slice(before)).toEqual([]);
  // A stop whose run never appears: dropped after its wait, and said in its conversation.
  await page.evaluate((sid) => sessionStorage.setItem("abstractcode.stop-while-starting.v1", JSON.stringify([{ id: "gone", sessionId: sid, at: Date.now() - 200_000, sentAt: Date.now() - 200_000 }])), sessionId);
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  const miss = page.locator(".code-stop-choice");
  await expect(miss).toContainText("The stop you pressed before the reload could not be matched to a reply; if a reply appears here, stop it.", { timeout: 10_000 });
  expect(commands.slice(before)).toEqual([]);
  await shot(page, "stop-unmatched-desktop");
  await page.setViewportSize({ width: 400, height: 860 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  await shot(page, "stop-unmatched-400");
  await page.setViewportSize({ width: 1440, height: 960 });
  await miss.getByRole("button", { name: "Dismiss", exact: true }).click();
  await expect(miss).toHaveCount(0);
});
