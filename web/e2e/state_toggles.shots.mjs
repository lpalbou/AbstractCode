// State-toggle screenshots for AbstractCode web (desktop 1440x900, tablet 834x1194, phone 390x844; light + dark).
//
// Reuses the responsive screens module (sign-in, workflow selection, the automation it creates) and
// adds the on/off surfaces: the toolbar's "Show all workflows", an automation's "Active" (on, then
// off), the sidebar's "Show archived", the Run settings Tools and Skills tabs.
//
//   ABSTRACTCODE_E2E_GATEWAY_URL=http://127.0.0.1:18850 node e2e/state_toggles.shots.mjs \
//     --url http://127.0.0.1:18851 --out <dir> [--pw <node_modules with playwright-core>]
//
// Output: <dir>/<screen>.<desktop|tablet|phone>.<light|dark>.png (full page) and .viewport.png.
// Works on the pre-switch build too (clicks "Pause" when there is no Active switch), so the same
// script takes the BEFORE and AFTER captures.
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import screensMod from "./responsive.screens.mjs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const PW_ROOT = args.pw || path.resolve(path.dirname(new URL(import.meta.url).pathname), "../node_modules");
const pw = createRequire(path.join(PW_ROOT, "noop.js"))("playwright-core");
const OUT = path.resolve(args.out);
fs.mkdirSync(OUT, { recursive: true });

const VIEWPORTS = [
  { name: "desktop", width: 1440, height: 900, touch: false },
  { name: "tablet", width: 834, height: 1194, touch: true },
  { name: "phone", width: 390, height: 844, touch: true },
];
const byName = Object.fromEntries(screensMod.screens.map((s) => [s.name, s]));
const main = (page) => page.locator(".code-automation-main");

/** Brings the automation to `active` (true) or paused (false) through its on/off control, whichever build is running. */
async function setActive(page, want) {
  const sw = main(page).getByRole("switch", { name: "Active" });
  if (await sw.count()) {
    if ((await sw.first().getAttribute("aria-checked")) !== String(want)) await sw.first().click();
    await main(page).locator(`[role="switch"][aria-checked="${want}"]`).first().waitFor({ timeout: 10000 });
  } else {
    const verb = main(page).getByRole("button", { name: want ? "Resume" : "Pause", exact: true });
    if (await verb.count()) await verb.first().click();
    await main(page).getByRole("button", { name: want ? "Pause" : "Resume", exact: true }).first().waitFor({ timeout: 10000 });
  }
  await page.waitForTimeout(800);
}

// DESIGN §3 type-scale guard (same rule as the kit's checkLabelScale): every rendered label,
// switch label and field caption <= 15 px and weight <= 600. Hits go to <out>/typescale.json.
const SCALE_SELECTOR = ["label", ".af-switch__label", ".af-form__label", ".af-field-caption", "[data-af-caption]"].join(", ");
async function labelScaleHits(page) {
  return page.evaluate((sel) => {
    const w = (v) => (v === "bold" ? 700 : v === "normal" || !v ? 400 : Number.parseFloat(v) || 400);
    return Array.from(document.querySelectorAll(sel))
      .filter((el) => el.getClientRects().length > 0)
      .map((el) => {
        const cs = getComputedStyle(el);
        return { cls: `${el.tagName.toLowerCase()}.${String(el.getAttribute("class") || "").trim().split(/\s+/).join(".")}`, text: (el.textContent || "").trim().slice(0, 60), size: Number.parseFloat(cs.fontSize), weight: w(cs.fontWeight) };
      })
      .filter((h) => h.size > 15.01 || h.weight > 600);
  }, SCALE_SELECTOR);
}
const scaleReport = {};

const SCREENS = [
  { name: "toolbar", run: async (page) => { await byName.signin.run(page); await byName.conversation.run(page); } },
  { name: "automations", run: byName.automations.run },
  { name: "automation-active", run: async (page, info) => { await byName["automation-detail"].run(page, info); await setActive(page, true); } },
  { name: "automation-paused", run: async (page) => setActive(page, false) },
  {
    name: "automation-form",
    run: async (page, info) => {
      await byName["automation-form"].run(page, info);
    },
  },
  {
    name: "settings-tools",
    run: async (page) => {
      await page.keyboard.press("Escape").catch(() => {});
      await page.waitForTimeout(200);
      await byName.settings.run(page);
      await page.getByRole("tab", { name: /Tools/ }).click();
    },
  },
  {
    name: "settings-skills",
    run: async (page) => {
      await page.getByRole("tab", { name: /Skills/ }).click();
      await page.waitForTimeout(600);
    },
  },
];

const browser = await pw.chromium.launch({ headless: true });
try {
  for (const scheme of ["light", "dark"]) {
    for (const vp of VIEWPORTS) {
      const ctx = await browser.newContext({
        viewport: { width: vp.width, height: vp.height },
        deviceScaleFactor: 2,
        hasTouch: vp.touch,
        isMobile: vp.touch,
        colorScheme: scheme,
        reducedMotion: "reduce",
      });
      // Code themes itself (kit appearance settings, default Observer Night), not prefers-color-scheme.
      const theme = scheme === "light" ? "light" : "observer-night";
      await ctx.addInitScript((t) => {
        try {
          const key = "af_appearance_abstractcode_v1";
          const cur = JSON.parse(localStorage.getItem(key) || "{}");
          localStorage.setItem(key, JSON.stringify({ ...cur, theme: t }));
        } catch {}
      }, theme);
      const page = await ctx.newPage();
      const info = { baseUrl: args.url, viewport: vp, browser: "chromium" };
      await page.goto(args.url, { waitUntil: "domcontentloaded" });
      await screensMod.setup(page, info);
      for (const s of SCREENS) {
        const base = path.join(OUT, `${s.name}.${vp.name}.${scheme}`);
        try {
          await s.run(page, info);
          await page.waitForTimeout(700);
          await page.screenshot({ path: `${base}.viewport.png` });
          await page.screenshot({ path: `${base}.png`, fullPage: true });
          scaleReport[path.basename(base)] = await labelScaleHits(page);
          process.stdout.write(`ok ${path.basename(base)}\n`);
        } catch (e) {
          process.stdout.write(`FAIL ${path.basename(base)}: ${String(e.message || e).split("\n")[0]}\n`);
          await page.screenshot({ path: `${base}.error.png` }).catch(() => {});
        }
      }
      await ctx.close();
    }
  }
} finally {
  await browser.close();
  fs.writeFileSync(path.join(OUT, "typescale.json"), JSON.stringify(scaleReport, null, 2));
}
