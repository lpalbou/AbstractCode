// Responsive capture screens for AbstractCode web (harness: untracked/responsive/harness/capture.mjs).
//
// Drives every main screen against the isolated fixture gateway (e2e/gateway_fixture.py, no model):
//   signin → conversation (prompt-structured: answer_user + durable ask_user) → approval (tool-approval card)
//   → automations (sidebar list) → automation-form (Schedule a task, Advanced open) → automation-detail
//   (occurrences + folder) → automation-occurrence (first occurrence transcript) → conversations (both sidebar
//   lists open) → workspace (inspector Files pane) → settings (drawer) → about (dialog)
//   → sidebar-collapsed (Automations panel collapsed, Conversations filling the sidebar).
//
//   node harness/capture.mjs --app code --url http://127.0.0.1:18782 --screens web/e2e/responsive.screens.mjs --out <dir> --sweep
//
// Env overrides: ABSTRACTCODE_E2E_GATEWAY_URL / _USER / _TOKEN (fixture defaults below);
// ABSTRACTCODE_RESPONSIVE_THEME=<theme id> (e.g. light) for a light capture.

const GATEWAY = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const USER = process.env.ABSTRACTCODE_E2E_USER || "web-tester";
const TOKEN = process.env.ABSTRACTCODE_E2E_TOKEN || "abstractcode-e2e-only";
const AUTOMATION_TITLE = "Responsive check automation";

// Space metrics (DESIGN §12) measure the layer the user is looking at. On screens that show a modal
// layer (the navigation drawer, a dialog, a settings drawer) the conversation or automation behind the
// scrim is excluded (`spaceIgnore`): it is not the screen's content and keeps its own one scroll.
const BEHIND_OVERLAY = [".code-conversation", ".code-automation-main"];

/** Per-page state (one page per viewport). */
const state = new WeakMap();
const st = (page) => {
  if (!state.has(page)) state.set(page, {});
  return state.get(page);
};

async function blockExternal(page, baseUrl) {
  const allowed = new URL(baseUrl).origin;
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin === allowed) return route.continue();
    return route.abort("blockedbyclient");
  });
}

async function closeOverlays(page) {
  // Escape closes the kit dialogs/drawers and the app's navigation drawer.
  for (let i = 0; i < 3; i++) {
    await page.keyboard.press("Escape").catch(() => {});
    await page.waitForTimeout(80);
  }
}

async function signIn(page) {
  const dialog = page.getByRole("dialog", { name: "Gateway connection" });
  const select = page.getByLabel("Workflow", { exact: true });
  // Either the sign-in dialog shows, or the stored session connects (select enabled).
  const end = Date.now() + 20000;
  for (;;) {
    if (await dialog.isVisible().catch(() => false)) break;
    if (await select.isEnabled().catch(() => false)) return;
    // A conversation with a live run locks the workflow select; the status bar still says Connected.
    if (await page.locator(".code-statusbar").getByText("Connected", { exact: true }).isVisible().catch(() => false)) return;
    // Phone landscape hides the status bar: the top bar's connection pill says the same.
    if (await page.locator(".af-topbar__pill--connected").isVisible().catch(() => false)) return;
    if (Date.now() > end) throw new Error("neither the sign-in dialog nor a connected workspace appeared");
    await page.waitForTimeout(150);
  }
  await page.locator("#gateway-session-url").fill(GATEWAY);
  await page.locator("#gateway-session-user").fill(USER);
  await page.locator("#gateway-session-token").fill(TOKEN);
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await dialog.waitFor({ state: "hidden", timeout: 15000 });
  await waitEnabled(page.getByLabel("Workflow", { exact: true }));
}

async function waitEnabled(locator, timeout = 20000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    if (await locator.isEnabled().catch(() => false)) return;
    await new Promise((r) => setTimeout(r, 150));
  }
  throw new Error("timed out waiting for an enabled control");
}

/** Opens the conversation navigation when it is a drawer (narrow widths); no-op when docked. */
async function openNav(page) {
  const opener = page.locator(".code-mobile-nav");
  if (await opener.isVisible().catch(() => false)) {
    await opener.click();
    await page.waitForTimeout(250);
  }
}

/** Clicks like a user; if the control is covered or off-screen (a layout defect the report records), falls back to a DOM click. */
async function press(page, locator, note) {
  try {
    await locator.click({ timeout: 5000 });
  } catch {
    const vp = page.viewportSize();
    process.stderr.write(`  [screens] ${vp?.width}x${vp?.height}: "${note}" not clickable (covered/off-screen) -> DOM click\n`);
    await locator.dispatchEvent("click");
  }
}

async function newConversation(page) {
  // The sidebar button works whether the sidebar is docked or an off-canvas drawer.
  await page.locator(".code-new-chat").dispatchEvent("click");
  await page.waitForTimeout(300);
}

async function selectWorkflow(page, name) {
  const select = page.getByLabel("Workflow", { exact: true });
  await waitEnabled(select);
  const listed = (await select.locator("option").allTextContents()).map((t) => t.trim());
  if (!listed.includes(name)) {
    // DOM click: on narrow widths the toolbar's controls can overlap the checkbox (a baseline finding).
    const all = page.getByLabel("Show all workflows");
    if (!(await all.isChecked())) await all.evaluate((el) => el.click());
    await page.waitForTimeout(200);
  }
  await select.selectOption({ label: name });
}

async function openConversationScreen(page) {
  const s = st(page);
  if (s.conversationUrl) {
    // A hash-only goto keeps the SPA state (e.g. the automation view): reload for a real open.
    await page.goto(s.conversationUrl, { waitUntil: "domcontentloaded" });
    await page.reload({ waitUntil: "domcontentloaded" });
    await signIn(page);
  }
  await page.getByText("A question for you").first().waitFor({ state: "visible", timeout: 30000 });
}

async function ensureAutomation(page) {
  await openNav(page);
  const row = page.locator(".code-auto-row", { hasText: AUTOMATION_TITLE }).first();
  if (await row.isVisible({ timeout: 4000 }).catch(() => false)) return;
  await closeOverlays(page);
  await newConversation(page);
  await closeOverlays(page);
  await selectWorkflow(page, "Native tool approval");
  await openNav(page);
  await press(page, page.getByRole("button", { name: "New automation" }), "New automation");
  const dialog = page.getByRole("dialog", { name: "Schedule a task" });
  await dialog.waitFor({ state: "visible", timeout: 10000 });
  await dialog.getByLabel("Task").fill("Write the fixture file");
  await dialog.getByText("Advanced").click();
  await dialog.getByLabel("Title").fill(AUTOMATION_TITLE);
  await dialog.getByRole("button", { name: "Create automation" }).click();
  await dialog.waitFor({ state: "hidden", timeout: 15000 });
  await page.waitForTimeout(500);
  await openNav(page);
  await row.waitFor({ state: "visible", timeout: 15000 });
}

export default {
  async setup(page, info) {
    await blockExternal(page, info.baseUrl);
    // ABSTRACTCODE_RESPONSIVE_THEME=light captures the app's Light theme (the app's own setting;
    // the browser's prefers-color-scheme does not change it). Default: the app default.
    const theme = process.env.ABSTRACTCODE_RESPONSIVE_THEME;
    if (theme) {
      await page.addInitScript((id) => {
        try {
          localStorage.setItem("af_appearance_abstractcode_v1", JSON.stringify({ theme: id }));
        } catch {}
      }, theme);
    }
    await page.goto(info.baseUrl, { waitUntil: "domcontentloaded" });
  },
  screens: [
    {
      name: "signin",
      async run(page) {
        await page.getByRole("dialog", { name: "Gateway connection" }).waitFor({ state: "visible", timeout: 15000 });
      },
    },
    {
      name: "conversation",
      settle: 900,
      async run(page) {
        await signIn(page);
        await selectWorkflow(page, "Prompt structured");
        await page.getByRole("button", { name: "Configure inputs", exact: true }).click();
        const drawer = page.getByRole("complementary", { name: "Workflow inputs" });
        await drawer.waitFor({ state: "visible", timeout: 10000 });
        await drawer.getByLabel(/Ticket/).fill(`responsive-${Date.now()}`);
        await page.locator(".pc-composer textarea").fill("Show me how this conversation looks on this screen.");
        await drawer.getByRole("button", { name: "Run workflow", exact: true }).click();
        await page.getByText("A question for you").first().waitFor({ state: "visible", timeout: 30000 });
        await closeOverlays(page);
        st(page).conversationUrl = page.url();
      },
    },
    {
      name: "approval",
      settle: 900,
      async run(page) {
        await closeOverlays(page);
        await newConversation(page);
        await closeOverlays(page);
        await selectWorkflow(page, "Native tool approval");
        await page.getByRole("button", { name: "Run workflow", exact: true }).click();
        await page.getByRole("heading", { name: "1 action needs permission" }).waitFor({ state: "visible", timeout: 30000 });
        await page.getByRole("button", { name: "Allow once", exact: true }).scrollIntoViewIfNeeded().catch(() => {});
      },
    },
    {
      name: "automations",
      spaceIgnore: BEHIND_OVERLAY,
      async run(page) {
        await closeOverlays(page);
        await ensureAutomation(page);
        await page.locator(".code-automations").scrollIntoViewIfNeeded().catch(() => {});
      },
    },
    {
      name: "automation-form",
      spaceIgnore: [...BEHIND_OVERLAY, ".code-sidebar"],
      async run(page) {
        await closeOverlays(page);
        await openNav(page);
        await press(page, page.getByRole("button", { name: "New automation" }), "New automation");
        const dialog = page.getByRole("dialog", { name: "Schedule a task" });
        await dialog.waitFor({ state: "visible", timeout: 10000 });
        await dialog.getByLabel("Task").fill("Summarise yesterday's commits and open issues");
        await dialog.getByText("Advanced").click();
      },
    },
    {
      name: "automation-detail",
      settle: 900,
      async run(page) {
        await closeOverlays(page);
        await openNav(page);
        await press(page, page.locator(".code-auto-row", { hasText: AUTOMATION_TITLE }).first(), "automation row");
        const main = page.locator(".code-automation-main");
        await main.waitFor({ state: "visible", timeout: 15000 });
        await main.locator(".af-auto-occ").first().waitFor({ state: "visible", timeout: 30000 }).catch(() => {});
        const approve = main.getByRole("button", { name: "Approve" }).first();
        if (await approve.isVisible({ timeout: 3000 }).catch(() => false)) {
          await approve.click();
          await page.waitForTimeout(1500);
        }
        await main.getByRole("region", { name: "Automation folder" }).waitFor({ state: "visible", timeout: 15000 }).catch(() => {});
        await main.evaluate((el) => el.querySelector(".code-auto-main-body")?.scrollTo(0, 0));
      },
    },
    {
      // The selected automation's first occurrence (its trigger + answer transcript) scrolled into view.
      name: "automation-occurrence",
      settle: 900,
      async run(page) {
        const main = page.locator(".code-automation-main");
        if (!(await main.isVisible().catch(() => false))) {
          await closeOverlays(page);
          await openNav(page);
          await press(page, page.locator(".code-auto-row", { hasText: AUTOMATION_TITLE }).first(), "automation row");
          await main.waitFor({ state: "visible", timeout: 15000 });
        }
        const occ = main.locator(".af-auto-occ").first();
        await occ.waitFor({ state: "visible", timeout: 30000 }).catch(() => {});
        await occ.scrollIntoViewIfNeeded().catch(() => {});
      },
    },
    {
      // Both sidebar lists open (Automations + Conversations; inside the drawer on phones and tablets).
      name: "conversations",
      spaceIgnore: BEHIND_OVERLAY,
      async run(page) {
        await closeOverlays(page);
        await openConversationScreen(page);
        await openNav(page);
        for (const id of ["#code-panel-automations-toggle", "#code-panel-conversations-toggle"]) {
          const t = page.locator(id);
          if ((await t.getAttribute("aria-expanded")) === "false") await press(page, t, id);
        }
      },
    },
    {
      name: "workspace",
      async run(page) {
        await closeOverlays(page);
        await openConversationScreen(page);
        const toggle = page.getByRole("button", { name: "Toggle workspace inspector" });
        if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
        const files = page.locator(".code-inspector-tabs button", { hasText: /files/i }).first();
        if (await files.isVisible().catch(() => false)) await files.click();
      },
    },
    {
      name: "settings",
      spaceIgnore: BEHIND_OVERLAY,
      async run(page) {
        await closeOverlays(page);
        const toggle = page.getByRole("button", { name: "Toggle workspace inspector" });
        if ((await toggle.getAttribute("aria-pressed").catch(() => null)) === "true" && info_isNarrow(page)) await toggle.click();
        const model = page.locator(".code-model-button");
        if (await model.isVisible().catch(() => false)) await press(page, model, "model button");
        else {
          // Phones hide the model shortcut: Run settings opens from Tools, Model is its first tab.
          await press(page, page.getByRole("button", { name: "Tools", exact: true }), "Tools");
          await page.getByRole("tab", { name: /Model/ }).click();
        }
        await page.getByRole("complementary", { name: "Run settings" }).waitFor({ state: "visible", timeout: 10000 });
      },
    },
    {
      name: "about",
      spaceIgnore: BEHIND_OVERLAY,
      async run(page) {
        await closeOverlays(page);
        await press(page, page.locator(".af-topbar__btn--about"), "About");
        await page.getByRole("dialog").last().waitFor({ state: "visible", timeout: 10000 });
      },
    },
    {
      // Automations collapsed, Conversations filling the sidebar (inside the open drawer on phones).
      name: "sidebar-collapsed",
      spaceIgnore: BEHIND_OVERLAY,
      async run(page) {
        await closeOverlays(page);
        await openNav(page);
        const auto = page.locator("#code-panel-automations-toggle");
        const conv = page.locator("#code-panel-conversations-toggle");
        if ((await auto.getAttribute("aria-expanded")) === "true") await press(page, auto, "Automations toggle");
        if ((await conv.getAttribute("aria-expanded")) === "false") await press(page, conv, "Conversations toggle");
        await page.locator("#code-panel-automations").waitFor({ state: "hidden", timeout: 5000 });
      },
    },
  ],
  sweepScreen: "conversation",
};

function info_isNarrow(page) {
  const vp = page.viewportSize();
  return !!vp && vp.width < 1024;
}
