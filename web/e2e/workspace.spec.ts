import { expect, test, type Page } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const appOrigin = process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782";
const fixtureGateway =
  process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const fixtureUser = process.env.ABSTRACTCODE_E2E_USER || "web-tester";
const fixtureToken =
  process.env.ABSTRACTCODE_E2E_TOKEN || "abstractcode-e2e-only";
const artifactsDir = join(
  fileURLToPath(new URL(".", import.meta.url)),
  "artifacts",
);
const pageErrors = new WeakMap<Page, string[]>();

function unique(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
}

async function capture(page: Page, name: string): Promise<void> {
  mkdirSync(artifactsDir, { recursive: true });
  await page.screenshot({
    path: join(artifactsDir, `${name}.png`),
    fullPage: true,
  });
}

/** The page is allowed to reach only the locally-started Code proxy.
 * Gateway traffic therefore has to travel through the proxy; a live or
 * browser-configured remote gateway is blocked before it can receive a byte.
 */
async function blockExternalTraffic(page: Page): Promise<void> {
  const allowedOrigin = new URL(appOrigin).origin;
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin === allowedOrigin) return route.continue();
    await route.abort("blockedbyclient");
  });
}

async function signIn(page: Page, captureLogin = false): Promise<void> {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  const dialog = page.getByRole("dialog", { name: "Gateway connection" });
  await expect(dialog).toBeVisible();
  if (captureLogin) await capture(page, "login"); // Before the test token is entered.
  await page.locator("#gateway-session-url").fill(fixtureGateway);
  await page.locator("#gateway-session-user").fill(fixtureUser);
  await page.locator("#gateway-session-token").fill(fixtureToken);
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByLabel("Workflow", { exact: true })).toBeEnabled();
  await expect(
    page.locator(".code-statusbar").getByText("Connected", { exact: true }),
  ).toBeVisible();
}

async function selectWorkflow(page: Page, name: string): Promise<void> {
  const select = page.getByLabel("Workflow", { exact: true });
  await expect(select).toBeEnabled();
  await select.selectOption({ label: name });
  await expect(select).toHaveValue(/.+/);
}

async function expectPersistedCompletion(page: Page): Promise<void> {
  await expect(page).toHaveURL(/(?:#|&)run=/);
  const runId = new URLSearchParams(new URL(page.url()).hash.slice(1)).get(
    "run",
  );
  expect(runId).toBeTruthy();
  // Share the browser's authenticated same-origin session. A completion
  // ledger event alone can precede (and hide a failure of) checkpoint saving.
  await expect
    .poll(async () => {
      const response = await page.request.get(
        `${appOrigin}/api/gateway/runs/${encodeURIComponent(runId!)}`,
      );
      expect(response.ok()).toBe(true);
      return (await response.json()).status;
    })
    .toBe("completed");
}

async function startPromptWorkflow(page: Page, prompt: string): Promise<void> {
  await selectWorkflow(page, "Prompt structured");
  await page
    .getByRole("button", { name: "Configure inputs", exact: true })
    .click();
  const drawer = page.getByRole("complementary", { name: "Workflow inputs" });
  await expect(drawer).toBeVisible();
  await drawer.getByLabel(/Ticket/).fill(unique("ticket"));
  await page.locator(".pc-composer textarea").fill(prompt);
  await drawer
    .getByRole("button", { name: "Run workflow", exact: true })
    .click();
  // The real Ask User node receives the flow's Prompt input as its durable
  // question; asserting the submitted value proves it is not a mock dialog.
  await expect(page.getByText(prompt, { exact: true })).toBeVisible();
}

test.describe("AbstractCode isolated gateway workspace", () => {
  test.describe.configure({ mode: "serial" });

  test.beforeEach(async ({ page }) => {
    const errors: string[] = [];
    pageErrors.set(page, errors);
    page.on("pageerror", (error) =>
      errors.push(error.message || String(error)),
    );
    await blockExternalTraffic(page);
  });

  test.afterEach(async ({ page }) => {
    expect(pageErrors.get(page) || []).toEqual([]);
    const header = await page.locator(".code-topbar").boundingBox();
    if (header)
      expect(
        header.y,
        "Transcript scrolling must not move the app header",
      ).toBeGreaterThanOrEqual(0);
  });

  test("signs in, runs schema-backed workflow, reloads history, switches sessions, and changes theme", async ({
    page,
  }) => {
    const prompt = `Review the workspace setup — ${unique("e2e")}`;
    await signIn(page, true);
    await capture(page, "desktop");

    await startPromptWorkflow(page, prompt);
    await expect(
      page.getByRole("heading", { name: "A question for you" }),
    ).toBeVisible();
    await capture(page, "workflow-question");

    await page.getByRole("button", { name: "continue", exact: true }).click();
    await expect(page.getByText(/structured-output/).first()).toBeVisible();
    await expect(page.getByText("Completed", { exact: true })).toBeVisible();

    // Reload must rebuild the transcript from the durable run history, not a
    // client-only optimistic message cache.
    await page.reload({ waitUntil: "domcontentloaded" });
    await expect(page.getByText(/structured-output/).first()).toBeVisible();
    const restoredTranscript = page.locator(".pc-chat-thread");
    await expect(
      restoredTranscript.getByText(prompt, { exact: true }),
    ).toBeVisible();
    await expect(
      restoredTranscript.getByText(
        "Fixture is live. Please answer the durable question.",
        { exact: true },
      ),
    ).toBeVisible();
    const restoredItems = await page
      .locator(".pc-chat-thread .pc-chat-item")
      .allTextContents();
    const promptIndex = restoredItems.findIndex((text) =>
      text.includes(prompt),
    );
    const initialAnswerIndex = restoredItems.findIndex((text) =>
      text.includes("Fixture is live. Please answer the durable question."),
    );
    const finalIndex = restoredItems.findIndex((text) =>
      text.includes("structured-output"),
    );
    expect(promptIndex).toBeGreaterThanOrEqual(0);
    expect(initialAnswerIndex).toBeGreaterThan(promptIndex);
    expect(finalIndex).toBeGreaterThan(initialAnswerIndex);

    // Creating and then reopening the prior session asserts the sidebar is a
    // durable session selector rather than a transient run list.
    await page.locator(".code-new-chat").click();
    const priorSession = page.locator(".code-session", { hasText: prompt });
    await expect(priorSession).toBeVisible();
    await priorSession.click();
    await expect(page.getByText(/structured-output/).first()).toBeVisible();

    await page
      .getByRole("button", { name: "Appearance (theme and typography)" })
      .click();
    const appearance = page.getByRole("dialog", { name: "Appearance" });
    await expect(appearance).toBeVisible();
    await appearance.locator(".af-select-trigger").first().click();
    await page.getByRole("option", { name: "Light", exact: true }).click();
    await expect(page.locator("html")).toHaveClass(/theme-light/);
    // Theme colours transition independently of the theme attribute. Wait for
    // the real top-bar control to reach the new foreground token before
    // taking the human-review screenshot, rather than capturing its old
    // dark-theme text mid-transition.
    const settledBodyText = await page
      .locator("body")
      .evaluate((element) => getComputedStyle(element).color);
    await expect(
      page.getByRole("button", { name: "Disconnect from gateway" }),
    ).toHaveCSS("color", settledBodyText);
    await appearance
      .getByRole("button", { name: "Close", exact: true })
      .click();
    await capture(page, "light");

    await page.getByRole("button", { name: "Settings", exact: true }).click();
    const settings = page.getByRole("complementary", {
      name: "Run settings",
    });
    await expect(settings).toBeVisible();
    await expect(
      settings.getByRole("tab", { name: "Tools", exact: true }),
    ).toBeVisible();
    const closeSettings = settings.getByRole("button", {
      name: "Close panel",
      exact: true,
    });
    await closeSettings.focus();
    await page.keyboard.press("Tab");
    await expect(
      settings.getByRole("tab", { name: "Model & behavior", exact: true }),
    ).toBeFocused();
    await capture(page, "skills");
  });

  test("restores two completed structured turns from one durable session", async ({
    page,
  }) => {
    const firstPrompt = `Map the first workflow milestone — ${unique("first")}`;
    const secondPrompt = `Plan the second workflow milestone — ${unique("second")}`;
    await signIn(page);
    await startPromptWorkflow(page, firstPrompt);
    await page.getByRole("button", { name: "continue", exact: true }).click();
    await expect(page.getByText(/structured-output/).first()).toBeVisible();
    await expect(page.getByText("Completed", { exact: true })).toBeVisible();

    // The second turn deliberately reuses the active durable session and
    // starts through the generic workflow Inputs surface rather than an agent
    // prompt shortcut.
    await page.getByRole("button", { name: "Inputs", exact: true }).click();
    const secondInputs = page.getByRole("complementary", {
      name: "Workflow inputs",
    });
    await expect(secondInputs).toBeVisible();
    await secondInputs.getByLabel(/Ticket/).fill(unique("ticket"));
    await page.locator(".pc-composer textarea").fill(secondPrompt);
    await secondInputs
      .getByRole("button", { name: "Run workflow", exact: true })
      .click();
    await expect(page.getByText(secondPrompt, { exact: true })).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "A question for you" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "continue", exact: true }).click();
    await expect(page.getByText("Completed", { exact: true })).toBeVisible();

    await page.reload({ waitUntil: "domcontentloaded" });
    const transcript = page.locator(".pc-chat-thread");
    await expect(
      transcript.getByText(firstPrompt, { exact: true }),
    ).toBeVisible();
    await expect(
      transcript.getByText(secondPrompt, { exact: true }),
    ).toBeVisible();
    const turns = await transcript.locator(".pc-chat-item").allTextContents();
    const firstPromptIndex = turns.findIndex((text) =>
      text.includes(firstPrompt),
    );
    const secondPromptIndex = turns.findIndex((text) =>
      text.includes(secondPrompt),
    );
    const finalIndexes = turns.flatMap((text, index) =>
      text.includes("structured-output") ? [index] : [],
    );
    expect(firstPromptIndex).toBeGreaterThanOrEqual(0);
    expect(secondPromptIndex).toBeGreaterThan(firstPromptIndex);
    expect(finalIndexes).toHaveLength(2);
    expect(finalIndexes[0]).toBeGreaterThan(firstPromptIndex);
    expect(finalIndexes[0]).toBeLessThan(secondPromptIndex);
    expect(finalIndexes[1]).toBeGreaterThan(secondPromptIndex);
  });

  test("delivers a manually entered JSON event to a real event wait", async ({
    page,
  }) => {
    await signIn(page);
    await selectWorkflow(page, "Event listener");
    await page
      .getByRole("button", { name: "Run workflow", exact: true })
      .click();

    await expect(
      page.getByRole("heading", { name: "Waiting for an event" }),
    ).toBeVisible();
    const eventData = page.getByLabel("Event data (JSON)");
    await eventData.fill('{"trigger":"manual-browser"}');
    await page.getByRole("button", { name: "Send event", exact: true }).click();
    await expect(
      page.getByText("fixture.ping delivered", { exact: true }),
    ).toBeVisible();
    await expect(page.getByText("Completed", { exact: true })).toBeVisible();
    await expectPersistedCompletion(page);
    await page.reload();
    await expect(page.getByText("Completed", { exact: true })).toBeVisible();
    await expectPersistedCompletion(page);
    await expect(
      page.getByText("fixture.ping delivered", { exact: true }),
    ).toBeVisible();

    await page.locator(".code-new-chat").click();
    await selectWorkflow(page, "Event emitter");
    await page
      .getByRole("button", { name: "Run workflow", exact: true })
      .click();
    await expect(page.getByText("Completed", { exact: true })).toBeVisible();
    await expectPersistedCompletion(page);
  });

  test("renders Resume after the gateway accepts a pause on an event wait", async ({
    page,
  }) => {
    await signIn(page);
    await selectWorkflow(page, "Event listener");
    await page
      .getByRole("button", { name: "Run workflow", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Waiting for an event" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Pause", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "Resume", exact: true }),
    ).toBeVisible();
  });

  test("requires explicit tool decisions for deny and allow", async ({
    page,
  }) => {
    await signIn(page);
    const commands: Array<{ type?: string; payload?: unknown }> = [];
    page.on("request", (request) => {
      if (
        new URL(request.url()).origin !== new URL(appOrigin).origin ||
        !request.url().includes("/api/gateway/commands")
      )
        return;
      try {
        commands.push(
          request.postDataJSON() as { type?: string; payload?: unknown },
        );
      } catch {
        /* assertion below reports no command */
      }
    });

    await selectWorkflow(page, "Native tool approval");
    await page
      .getByRole("button", { name: "Run workflow", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "1 action needs permission" }),
    ).toBeVisible();
    await expect(page.getByText(/Allow write_file to run\?/)).toBeVisible();
    await page.waitForTimeout(350);
    expect(commands.filter((command) => command.type === "resume")).toEqual([]);
    await capture(page, "tool-approval");

    await page.getByRole("button", { name: "Deny", exact: true }).click();
    await expect(page.getByText("Completed", { exact: true })).toBeVisible();

    await page.locator(".code-new-chat").click();
    await selectWorkflow(page, "Native tool approval");
    await page
      .getByRole("button", { name: "Run workflow", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "1 action needs permission" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Allow once", exact: true }).click();
    await expect(
      page.locator(".code-run-strip").getByText("Completed", { exact: true }),
    ).toBeVisible();
    const tools = page.locator(".pc-tool-activity");
    await expect(tools).toHaveCount(1);
    await expect(tools).toContainText("fixture-tool-approval.txt");
    await tools.locator(":scope > summary").click();
    await expect(
      tools.getByRole("heading", { name: "Parameters" }),
    ).toBeVisible();
    await expect(tools.getByRole("heading", { name: "Result" })).toBeVisible();
    await capture(page, "tool-evidence");
    await tools.locator(":scope > summary").click();
    const stats = page.getByLabel("Stats").last();
    await stats.scrollIntoViewIfNeeded();
    await expect(stats).toContainText("1 tool");
    await expect(stats).toContainText("1 file changed");
    await capture(page, "tool-turn-metrics");
    expect(commands.some((command) => command.type === "resume")).toBeTruthy();
  });

  test("compacts tool evidence and persists enabled-tool permissions with explicit revocation", async ({
    page,
  }) => {
    await signIn(page);
    const commands: any[] = [];
    page.on("request", (request) => {
      if (
        request.method() === "POST" &&
        request.url().includes("/api/gateway/commands")
      )
        commands.push(request.postDataJSON());
    });
    await selectWorkflow(page, "Tool supervision");
    await page
      .getByRole("button", { name: "Run workflow", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "3 actions need permission" }),
    ).toBeVisible();
    await expect(page.locator(".code-run-strip")).toContainText(
      "Approval needed",
    );
    const approval = page.locator(".pc-workflow-interaction");
    await expect(approval.locator(".pc-tool-activity")).toHaveCount(3);
    await expect(approval).toContainText("this batch only");
    await capture(page, "compact-tool-approval");
    await page
      .getByRole("button", { name: "Allow all enabled tools", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "A question for you" }),
    ).toBeVisible();
    await expect(page.locator(".code-run-strip")).toContainText(
      "Your answer is needed",
    );
    await expect(
      page.getByRole("button", { name: "Revoke", exact: true }),
    ).toBeVisible();
    const group = page.locator(".pc-chat-thread > .pc-tool-group");
    await expect(group).toHaveCount(1);
    await expect(group.locator(".pc-tool-activity")).toHaveCount(6);
    await expect(group.locator(".pc-tool-activity--failed")).toHaveCount(1);
    expect((await group.boundingBox())!.height).toBeLessThan(480);
    const groupBox = (await group.boundingBox())!;
    const finalRowBox = (await group
      .locator(".pc-tool-activity")
      .last()
      .boundingBox())!;
    expect(finalRowBox.y + finalRowBox.height).toBeLessThanOrEqual(
      groupBox.y + groupBox.height,
    );
    await expect(group.locator(".pc-chat-header")).toHaveCount(0);
    const failed = group.locator(".pc-tool-activity--failed");
    await failed.locator(":scope > summary").focus();
    await page.keyboard.press("Enter");
    await expect(failed).toHaveAttribute("open", "");
    await expect(
      failed.getByRole("heading", { name: "Parameters" }),
    ).toBeVisible();
    await page.keyboard.press("Enter");
    await page.locator(".pc-chat-thread").evaluate((element) => {
      element.scrollTop = 0;
    });
    await capture(page, "compact-tool-supervision");
    await page
      .getByRole("button", { name: "Appearance (theme and typography)" })
      .click();
    await page
      .getByRole("dialog", { name: "Appearance" })
      .locator(".af-select-trigger")
      .first()
      .click();
    await page
      .getByRole("option", { name: "Tokyo Night", exact: true })
      .click();
    await expect(page.locator("html")).toHaveClass(/theme-tokyo/);
    await page.keyboard.press("Escape");
    await page.waitForFunction(() =>
      document
        .getAnimations()
        .every(
          (animation) =>
            animation.playState !== "running" ||
            animation.effect?.getTiming().iterations === Infinity,
        ),
    );
    await capture(page, "compact-tool-supervision-tokyo");
    await page
      .getByRole("button", { name: "Appearance (theme and typography)" })
      .click();
    await page
      .getByRole("dialog", { name: "Appearance" })
      .locator(".af-select-trigger")
      .first()
      .click();
    await page.getByRole("option", { name: "Light", exact: true }).click();
    await page
      .getByRole("dialog", { name: "Appearance" })
      .getByRole("button", { name: "Close", exact: true })
      .click();
    await page.waitForFunction(() =>
      document
        .getAnimations()
        .every(
          (animation) =>
            animation.playState !== "running" ||
            animation.effect?.getTiming().iterations === Infinity,
        ),
    );
    await capture(page, "compact-tool-supervision-light");
    await page.setViewportSize({ width: 390, height: 844 });
    if (
      (await page.locator(".code-app").getAttribute("class"))?.includes(
        "code-app--nav-open",
      )
    )
      await page
        .getByRole("button", { name: "Close navigation", exact: true })
        .click();
    if (await page.locator(".code-inspector").count())
      await page
        .getByRole("button", { name: "Close workspace inspector", exact: true })
        .click();
    await expect(page.locator(".code-app")).not.toHaveClass(
      /code-app--nav-open/,
    );
    await expect(page.locator(".code-inspector")).toHaveCount(0);
    await page.waitForFunction(() => {
      const sidebar = document.querySelector(".code-sidebar");
      return sidebar && sidebar.getBoundingClientRect().right <= 0;
    });
    await page.locator(".pc-chat-thread").evaluate((element) => {
      element.scrollTop = 0;
    });
    await capture(page, "compact-tool-supervision-mobile");
    await page
      .getByRole("button", { name: "Your answer is needed", exact: true })
      .click();
    await expect(page.locator(".pc-workflow-interaction h3")).toBeFocused();
    await expect(
      page.getByRole("button", { name: "continue", exact: true }),
    ).toBeInViewport();
    await capture(page, "compact-tool-supervision-mobile-question");
    await page.setViewportSize({ width: 1440, height: 960 });
    await page.reload();
    await expect(page.locator(".code-run-strip")).toContainText("Permissions: all");
    await expect(page.getByRole("heading", { name: "A question for you" })).toBeVisible();
    await page.getByRole("button", { name: "Revoke", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "Revoke", exact: true }),
    ).toBeHidden();
    await page.getByRole("button", { name: "continue", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "1 action needs permission" }),
    ).toBeVisible();
    const resumesBefore = commands.filter(
      (command) => command.type === "resume",
    ).length;
    await page.getByRole("button", { name: "Allow once", exact: true }).click();
    await expectPersistedCompletion(page);
    expect(commands.filter((command) => command.type === "resume").length).toBe(
      resumesBefore + 1,
    );
    const approved = commands.filter(
      (command) => command.payload?.payload?.approved === true,
    );
    expect(
      new Set(
        approved.map(
          (command) => `${command.run_id}:${command.payload.wait_key}`,
        ),
      ).size,
    ).toBe(approved.length);
    await page.reload();
    await expect(
      page.getByRole("button", { name: "Revoke", exact: true }),
    ).toBeHidden();
    await expect(page.locator(".code-run-strip")).toContainText("Completed");
    await expect(
      page.locator(
        ".pc-chat-thread > .pc-tool-group .pc-tool-activity--failed",
      ),
    ).toHaveCount(1);
  });

  test("keeps the connected workspace usable on a small viewport", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await signIn(page);
    await expect(
      page.getByRole("button", { name: "Open conversation navigation" }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Open conversation navigation" })
      .click();
    await expect(page.locator(".code-app")).toHaveClass(/code-app--nav-open/);
    await page.getByRole("button", { name: "Close navigation" }).click();
    await page.waitForFunction(() => {
      const app = document.querySelector(".code-app");
      const sidebar = document.querySelector(".code-sidebar");
      return (
        !app?.classList.contains("code-app--nav-open") &&
        !document.querySelector(".code-nav-scrim") &&
        Boolean(sidebar && sidebar.getBoundingClientRect().right <= 0)
      );
    });
    await capture(page, "mobile");
  });

  test("shows live thinking without mistaking a helper's completed event for run completion", async ({
    page,
  }) => {
    await signIn(page);
    await selectWorkflow(page, "Event listener");
    await page
      .getByRole("button", { name: "Run workflow", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Waiting for an event", exact: true }),
    ).toBeVisible();
    const runId = new URLSearchParams(new URL(page.url()).hash.slice(1)).get(
      "run",
    )!;
    const runPath = `/api/gateway/runs/${runId}`;
    const began = new Date(Date.now() - 21000).toISOString();
    // Explicit visual fixture: only browser read responses are substituted.
    // The real isolated run remains safely parked on its event; no LLM or web
    // tools execute. Ledger shapes match controller regressions.
    await page.route(
      (url) => url.pathname === runPath,
      async (route) => {
        const response = await route.fetch();
        const run = await response.json();
        await route.fulfill({
          response,
          json: {
            ...run,
            status: "waiting",
            created_at: began,
            waiting: { reason: "subworkflow", wait_key: "foreground-working" },
          },
        });
      },
    );
    await page.route(
      (url) => url.pathname === `${runPath}/history_bundle`,
      async (route) => {
        const response = await route.fetch();
        const bundle = await response.json();
        const calls = [
          {
            name: "web_search",
            arguments: { query: "Latest world and technology news", count: 5 },
          },
          {
            name: "web_search",
            arguments: {
              query: "News from official company announcements",
              count: 5,
            },
          },
          {
            name: "fetch_url",
            arguments: { url: "https://example.com/news/industry-report" },
          },
        ];
        const items = calls.map((call, index) => ({
          cursor: 1000 + index,
          record: {
            run_id: runId,
            step_id: `tool-${index}`,
            status: "completed",
            started_at: began,
            ended_at: new Date(Date.parse(began) + 1600).toISOString(),
            effect: {
              type: "tool_calls",
              payload: {
                tool_calls: [{ ...call, runtime_call_id: `tool-${index}` }],
              },
            },
            result: {
              results: [
                {
                  runtime_call_id: `tool-${index}`,
                  success: index !== 2,
                  output: index === 2 ? null : "Sources collected for review",
                  ...(index === 2
                    ? { error: "HTTP Error 403: Forbidden" }
                    : {}),
                },
              ],
            },
          },
        }));
        items.push({
          cursor: 1003,
          record: {
            run_id: runId,
            step_id: "llm-current",
            status: "started",
            started_at: new Date(Date.parse(began) + 2000).toISOString(),
            effect: { type: "llm_call", payload: {} },
          },
        } as any);
        items.push({
          cursor: 1004,
          record: {
            run_id: runId,
            step_id: "helper",
            status: "completed",
            effect: {
              type: "emit_event",
              payload: {
                name: "abstract.status",
                payload: { value: "completed" },
              },
            },
          },
        } as any);
        await route.fulfill({
          response,
          json: {
            ...bundle,
            input_data: {
              prompt: "Summarize the latest news with verified sources.",
            },
            ledgers: { [runId]: { items } },
          },
        });
      },
    );
    await page.reload();
    await expect(page.locator(".code-run-strip")).toContainText("Thinking");
    await expect(page.locator(".code-run-strip")).toContainText(
      "Generating the next response",
    );
    await expect(page.locator(".code-run-strip")).not.toContainText(
      "completed",
    );
    await expect(page.locator(".pc-tool-activity")).toHaveCount(3);
    await capture(page, "compact-live-thinking");
  });

  test("reuses real Assistant defaults on successive chat turns without fabricated required fields", async ({
    page,
  }) => {
    const submissions: any[] = [];
    page.on("request", (request) => {
      if (
        request.method() === "POST" &&
        request.url().endsWith("/api/gateway/runs/start")
      )
        submissions.push(request.postDataJSON());
    });
    await signIn(page);
    await selectWorkflow(page, "Assistant contract");
    const composer = page.locator(".pc-composer textarea");
    await composer.fill("Check the workflow defaults");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect(
      page.locator(".code-run-strip").getByText("Completed", { exact: true }),
    ).toBeVisible();
    await composer.fill("Continue with the same defaults");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect(
      page.locator(".code-run-strip").getByText("Completed", { exact: true }),
    ).toBeVisible();
    await expect.poll(() => submissions.length).toBe(2);
    await expect(
      page
        .locator(".pc-chat-item--user")
        .filter({ hasText: "Continue with the same defaults" }),
    ).toBeVisible();
    await expect(page.locator(".pc-chat-item--assistant")).toHaveCount(2);
    await expect(
      page.locator(".code-run-strip").getByText("Completed", { exact: true }),
    ).toBeVisible();
    await expect(composer).toHaveValue("");
    for (const submission of submissions) {
      expect(submission.input_data.max_iterations).toBe(24);
      expect(submission.input_data).not.toHaveProperty("max_in_tokens");
      expect(submission.input_data).not.toHaveProperty(
        "primary_image_artifact",
      );
      expect(submission.input_data).not.toHaveProperty("provider");
      expect(submission.input_data.use_context).toBe(false);
    }
    expect(submissions[1].input_data.context.task).toBe(
      "Continue with the same defaults",
    );
    await capture(page, "assistant-defaults-continuation");
  });

  test("chats with Basic Agent on a legacy Gateway without opening configuration", async ({
    page,
  }) => {
    const submissions: any[] = [];
    const requiredByLegacyGateway: string[][] = [];
    const sourceReads: string[] = [];
    // Emulate the exact OLD descriptor bug on a real, isolated Gateway. Both
    // required surfaces used !hasDefault, losing author intent. Raw flow,
    // auth, requests, execution and durable continuation remain real.
    await page.route(
      "**/api/gateway/bundles/*/flows/*/input_schema?*",
      async (route) => {
        const response = await route.fetch();
        const descriptor = await response.json();
        for (const pin of descriptor.inputs)
          pin.required = !Object.hasOwn(descriptor.defaults, pin.id);
        descriptor.input_data_schema.required = descriptor.inputs
          .filter((pin: any) => pin.required)
          .map((pin: any) => pin.id);
        if (descriptor.flow_id === "basic-agent-contract")
          requiredByLegacyGateway.push(descriptor.input_data_schema.required);
        await route.fulfill({ response, json: descriptor });
      },
    );
    page.on("request", (request) => {
      if (
        request.method() === "POST" &&
        request.url().endsWith("/api/gateway/runs/start")
      )
        submissions.push(request.postDataJSON());
      if (/\/flows\/basic-agent-contract\?bundle_version=/.test(request.url()))
        sourceReads.push(request.url());
    });
    await signIn(page);
    await selectWorkflow(page, "Basic agent defaults");
    const composer = page.getByRole("textbox", {
      name: "Message",
      exact: true,
    });
    const drawer = page.getByRole("complementary", { name: "Workflow inputs" });
    for (const [index, message] of [
      "Hello, use the usual defaults",
      "Continue without any setup",
    ].entries()) {
      await composer.fill(message);
      await page.getByRole("button", { name: "Send", exact: true }).click();
      await expect(page.locator(".pc-chat-item--assistant")).toHaveCount(
        index + 1,
      );
      await expect(
        page.locator(".code-run-strip").getByText("Completed", { exact: true }),
      ).toBeVisible();
      await expectPersistedCompletion(page);
      await expect(drawer).toBeHidden();
      await expect(composer).toHaveValue("");
      await page.reload();
      await expect(page.locator(".pc-chat-item--assistant")).toHaveCount(
        index + 1,
      );
    }
    expect(submissions).toHaveLength(2);
    expect(sourceReads.length).toBeGreaterThan(0);
    expect(requiredByLegacyGateway[0]).toEqual(
      expect.arrayContaining([
        "memory",
        "provider",
        "model",
        "system",
        "max_in_tokens",
        "resp_schema",
      ]),
    );
    for (const { input_data: input } of submissions) {
      for (const name of [
        "memory",
        "provider",
        "model",
        "system",
        "max_in_tokens",
        "resp_schema",
      ])
        expect(input).not.toHaveProperty(name);
      expect(input).toMatchObject({
        use_context: false,
        use_session_history: true,
        max_iterations: 20,
      });
    }
    expect(submissions[1].input_data.context.task).toBe(
      "Continue without any setup",
    );
    await capture(page, "basic-agent-zero-configuration");

    await page.getByRole("button", { name: "Inputs", exact: true }).click();
    await expect(
      drawer.getByText("Ready to chat", { exact: true }),
    ).toBeVisible();
    await expect(drawer.getByLabel("memory", { exact: true })).toBeHidden();
    await capture(page, "basic-agent-optional-advanced-inputs");
    await drawer
      .getByText("Advanced workflow inputs", { exact: false })
      .click();
    await expect(drawer.getByLabel("memory", { exact: true })).toBeVisible();
    await expect(drawer.locator('[aria-label="required"]')).toHaveCount(0);
    await drawer.getByRole("button", { name: "Back to chat" }).click();
    await expect(drawer).toBeHidden();

    // The same legacy adapter must not erase a real author requirement.
    await page.locator(".code-new-chat").click();
    await selectWorkflow(page, "Prompt structured");
    await composer.fill("A workflow whose author requires a ticket");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect(drawer).toBeVisible();
    await expect(page.getByText(/Ticket is required\./)).toBeVisible();
    expect(submissions).toHaveLength(2);
  });

  test("shows one published choice across installed copies and catalog versions", async ({ page }) => {
    const submissions: any[] = [];
    page.on("request", request => {
      if (request.method() === "POST" && request.url().endsWith("/api/gateway/runs/start")) submissions.push(request.postDataJSON());
    });
    await signIn(page);
    const selector = page.getByLabel("Workflow", { exact: true });
    await expect(selector.locator("option").filter({ hasText: "Published coding" })).toHaveCount(1);
    expect(await selector.locator("option").allTextContents()).not.toEqual(expect.arrayContaining([expect.stringContaining(" · shared")]));
    await selectWorkflow(page, "Published coding");
    await expect(selector).toHaveValue("tenant_catalog:e2e-published-coding@1.0.0:coding-contract");
    await page.locator(".pc-composer textarea").fill("Use the preferred published coding workflow");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expectPersistedCompletion(page);
    expect(submissions[0]).toMatchObject({ bundle_id: "e2e-published-coding", bundle_version: "1.0.0", registry_scope: "tenant_catalog" });
    await page.reload();
    await expect(selector).toHaveValue("tenant_catalog:e2e-published-coding@1.0.0:coding-contract");
    await capture(page, "published-workflow-selection");
  });

  test("sends coding workflow requests with published defaults and restores them without configuration", async ({ page }) => {
    const submissions: any[] = [];
    page.on("request", request => {
      if (request.method() === "POST" && request.url().endsWith("/api/gateway/runs/start")) submissions.push(request.postDataJSON());
    });
    await signIn(page);
    await selectWorkflow(page, "Coding request defaults");
    const composer = page.locator(".pc-composer textarea");
    const request = "Build a playable arcade game in the Gateway workspace";
    await composer.fill(request);
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expectPersistedCompletion(page);
    await expect(page.getByText("Coding workflow accepted your request with its published defaults. No model was invoked.", { exact: true })).toBeVisible();
    expect(submissions).toHaveLength(1);
    expect(submissions[0].input_data).toMatchObject({ request, build_command: "", run_command: "", max_rounds: 2 });
    for (const field of ["provider", "model", "prompt", "context", "messages"]) expect(submissions[0].input_data).not.toHaveProperty(field);
    await page.getByRole("button", { name: "Inputs", exact: true }).click();
    const drawer = page.getByRole("complementary", { name: "Workflow inputs" });
    await expect(drawer.getByText("Ready to chat", { exact: true })).toBeVisible();
    await expect(drawer.getByLabel("build command", { exact: true })).toBeHidden();
    await capture(page, "coding-workflow-defaults");
    await drawer.getByRole("button", { name: "Close panel" }).click();
    await page.reload();
    await expect(page.locator(".pc-chat-thread").getByText(request, { exact: true })).toBeVisible();
    await composer.fill("Continue the arcade game");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect.poll(() => submissions.length).toBe(2);
    await expectPersistedCompletion(page);
    expect(submissions[1].input_data.request).toBe("Continue the arcade game");
    expect(submissions[1].input_data.max_rounds).toBe(2);
    await capture(page, "coding-workflow-conversation");
  });

  test("permissions all never executes a tool unchecked in the enabled selection", async ({ page }) => {
    const submissions: any[] = [];
    page.on("request", request => {
      if (request.method() === "POST" && request.url().endsWith("/api/gateway/runs/start")) submissions.push(request.postDataJSON());
    });
    await signIn(page);
    await selectWorkflow(page, "Native tool approval");
    await page.getByRole("button", { name: "Tools", exact: true }).click();
    const settings = page.getByRole("complementary", { name: "Run settings" });
    await settings.getByLabel("Permissions", { exact: true }).selectOption("all");
    await settings.getByRole("button", { name: "Custom allowlist", exact: true }).click();
    await settings.getByRole("button", { name: "Select all", exact: true }).click();
    await settings.getByPlaceholder("Filter tools...").fill("write_file");
    const row = settings.locator(".af-tool-row").filter({ hasText: "write_file" });
    await row.getByRole("checkbox").uncheck();
    await capture(page, "permissions-all-enabled-tools");
    await settings.getByRole("button", { name: "Close panel", exact: true }).click();
    await page.getByRole("button", { name: "Run workflow", exact: true }).click();
    await expectPersistedCompletion(page);
    await expect(page.locator(".pc-tool-activity--failed")).toContainText("not allowed");
    await expect(page.locator(".pc-workflow-interaction")).toHaveCount(0);
    expect(submissions[0].input_data._runtime.allowed_tools).not.toContain("write_file");
    expect(submissions[0].input_data._runtime.tool_policy.require_approval_tools).not.toContain("write_file");
    await capture(page, "unchecked-tool-denied");
    // The previous turn's immutable ceiling must not prevent an explicit
    // enablement for the next run in the same conversation.
    await page.getByRole("button", { name: "Tools", exact: true }).click();
    await settings.getByPlaceholder("Filter tools...").fill("write_file");
    await settings.getByRole("checkbox", { name: "Enable write_file", exact: true }).check();
    await settings.getByRole("button", { name: "Close panel", exact: true }).click();
    await page.getByRole("button", { name: "Inputs", exact: true }).click();
    await page.getByRole("complementary", { name: "Workflow inputs" }).getByRole("button", { name: "Run workflow", exact: true }).click();
    await expect.poll(() => submissions.length).toBe(2);
    await expectPersistedCompletion(page);
    expect(submissions[1].input_data._runtime.allowed_tools).toContain("write_file");
    await expect(page.locator(".pc-tool-activity--completed")).toHaveCount(1);
  });

  test("revokes a permission level selected before starting the run", async ({ page }) => {
    const submissions: any[] = [];
    page.on("request", request => {
      if (request.method() === "POST" && request.url().endsWith("/api/gateway/runs/start")) submissions.push(request.postDataJSON());
    });
    await signIn(page);
    await selectWorkflow(page, "Tool supervision");
    await page.getByRole("button", { name: "Tools", exact: true }).click();
    const settings = page.getByRole("complementary", { name: "Run settings" });
    await settings.getByLabel("Permissions", { exact: true }).selectOption("all");
    await settings.getByRole("button", { name: "Close panel", exact: true }).click();
    await page.getByRole("button", { name: "Run workflow", exact: true }).click();
    await expect(page.getByRole("heading", { name: "A question for you" })).toBeVisible();
    expect(submissions[0].input_data._runtime.tool_policy.require_approval_tools).toContain("write_file");
    expect(submissions[0].input_data._runtime.tool_policy).not.toHaveProperty("auto_approve_tools");
    await page.getByRole("button", { name: "Revoke", exact: true }).click();
    await page.getByRole("button", { name: "continue", exact: true }).click();
    await expect(page.getByRole("heading", { name: "1 action needs permission" })).toBeVisible();
    await page.getByRole("button", { name: "Deny", exact: true }).click();
    await expectPersistedCompletion(page);
  });

  test("cascades typed provider, model and reasoning controls without changing authored defaults", async ({
    page,
  }) => {
    // Only discovery is simulated. Authentication, schema, runs and history
    // still use the disposable real Gateway, never the operator's model.
    await page.route("**/api/gateway/discovery/providers?*", (route) =>
      route.fulfill({
        json: {
          items: [{ name: "fixture-authored" }, { name: "fixture-other" }],
        },
      }),
    );
    await page.route("**/api/gateway/discovery/providers/*/models*", (route) =>
      route.fulfill({
        json: {
          models: route.request().url().includes("fixture-other")
            ? ["reasoner-other", "plain-other"]
            : ["reasoner-authored"],
        },
      }),
    );
    await page.route("**/api/gateway/discovery/models/capabilities*", (route) =>
      route.fulfill({
        json: {
          capabilities: route.request().url().includes("plain-other")
            ? {}
            : {
                thinking_support: true,
                reasoning_levels: ["low", "medium", "high"],
              },
        },
      }),
    );
    await signIn(page);
    await selectWorkflow(page, "Typed workflow inputs");
    await page.getByRole("button", { name: "Inputs", exact: true }).click();
    const drawer = page.getByRole("complementary", { name: "Workflow inputs" });
    await drawer.getByText("Advanced workflow inputs", { exact: false }).click();
    const group = drawer.locator(".code-settings-section").filter({
      has: page.getByRole("heading", { name: "Text model", exact: true }),
    });
    await expect(
      group.getByRole("button", { name: "Provider", exact: true }),
    ).toContainText("fixture-authored");
    await expect(
      group.getByRole("button", { name: "Model", exact: true }),
    ).toContainText("reasoner-authored");
    await group.getByRole("button", { name: "Provider", exact: true }).click();
    await page
      .getByRole("option", { name: "fixture-other", exact: true })
      .click();
    await group.getByRole("button", { name: "Model", exact: true }).click();
    await expect(
      page.getByRole("option", { name: "reasoner-authored", exact: true }),
    ).toHaveCount(0);
    await page
      .getByRole("option", { name: "reasoner-other", exact: true })
      .click();
    await group.getByRole("button", { name: "Reasoning effort" }).click();
    await page.getByRole("option", { name: "high", exact: true }).click();
    await expect(
      group.getByRole("button", { name: "Reasoning effort" }),
    ).toContainText("high");
    await capture(page, "typed-workflow-model-reasoning");
    await group.getByRole("button", { name: "Model", exact: true }).click();
    await page
      .getByRole("option", { name: "plain-other", exact: true })
      .click();
    await expect(
      group.getByRole("button", { name: "Reasoning effort" }),
    ).toHaveCount(0);
    await drawer.getByRole("button", { name: "Close panel" }).click();
    await selectWorkflow(page, "Authored model contract");
    await page.locator(".code-model-button").click();
    await expect(
      page
        .getByRole("complementary", { name: "Run settings" })
        .getByText("Workflow default: fixture-authored · reasoner-authored."),
    ).toBeVisible();
  });

  test("shares per-message read, pause, resume and stop with configurable Gateway voice", async ({
    page,
  }) => {
    // Deterministic speech bytes test browser playback, not a live synthesis
    // backend. The durable conversation still executes on the real fixture.
    const sampleRate = 8000,
      dataLength = sampleRate * 2 * 20;
    const wav = Buffer.alloc(44 + dataLength);
    wav.write("RIFF", 0);
    wav.writeUInt32LE(36 + dataLength, 4);
    wav.write("WAVEfmt ", 8);
    wav.writeUInt32LE(16, 16);
    wav.writeUInt16LE(1, 20);
    wav.writeUInt16LE(1, 22);
    wav.writeUInt32LE(sampleRate, 24);
    wav.writeUInt32LE(sampleRate * 2, 28);
    wav.writeUInt16LE(2, 32);
    wav.writeUInt16LE(16, 34);
    wav.write("data", 36);
    wav.writeUInt32LE(dataLength, 40);
    let requestBody: any;
    await page.route("**/api/gateway/voice/voices*", (route) =>
      route.fulfill({
        json: {
          providers: ["fixture-voice", "fixture-alternative"],
          tts_models_by_provider: { "fixture-voice": ["fixture-speaker"] },
          items: [
            {
              id: "warm",
              label: "Warm · clear and conversational",
              provider: "fixture-voice",
              model: "fixture-speaker",
              voice_kind: "profile",
            },
            {
              id: "warm",
              label: "Warm",
              provider: "fixture-alternative",
              model: "alternative-speaker",
              voice_kind: "profile",
            },
          ],
          controls: {
            instructions: { supported: true },
            quality_preset: {
              supported: true,
              values: ["low", "standard", "high"],
            },
          },
        },
      }),
    );
    await page.route("**/api/gateway/runs/*/voice/tts", (route) => {
      requestBody = route.request().postDataJSON();
      return route.fulfill({
        json: { audio_artifact: { $artifact: "browser-voice-fixture" } },
      });
    });
    await page.route(
      "**/api/gateway/runs/*/artifacts/browser-voice-fixture/content",
      (route) => route.fulfill({ contentType: "audio/wav", body: wav }),
    );
    await signIn(page);
    await selectWorkflow(page, "Assistant contract");
    await page
      .locator(".pc-composer textarea")
      .fill("Explain this change clearly");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    const message = page.locator(".pc-chat-item--assistant").last();
    await expect(message).toBeVisible();
    await page
      .getByRole("button", { name: "Voice settings", exact: true })
      .click();
    const drawer = page.getByRole("complementary", { name: "Voice settings" });
    await drawer.getByRole("button", { name: "AI voice" }).click();
    await expect(
      page.getByRole("option", {
        name: "Warm · fixture-alternative",
        exact: true,
      }),
    ).toBeVisible();
    await page
      .getByRole("option", {
        name: "Warm · clear and conversational · fixture-voice",
        exact: true,
      })
      .click();
    await expect(
      drawer.getByRole("button", { name: "Speech provider", exact: true }),
    ).toContainText("fixture-voice");
    await expect(
      drawer.getByRole("button", { name: "Speech model", exact: true }),
    ).toContainText("fixture-speaker");
    await drawer
      .getByRole("button", { name: "Reset to Gateway defaults", exact: true })
      .click();
    await expect(
      drawer.getByRole("tab", { name: "Gateway default", exact: true }),
    ).toHaveAttribute("aria-selected", "true");
    await drawer.getByRole("tab", { name: "Custom", exact: true }).click();
    await drawer
      .getByRole("button", { name: "Speech provider", exact: true })
      .click();
    await page
      .getByRole("option", { name: "fixture-voice", exact: true })
      .click();
    await drawer
      .getByRole("button", { name: "Speech model", exact: true })
      .click();
    await page
      .getByRole("option", { name: "fixture-speaker", exact: true })
      .click();
    await drawer.getByRole("button", { name: "AI voice" }).click();
    await page
      .getByRole("option", {
        name: "Warm · clear and conversational",
        exact: true,
      })
      .click();
    await drawer
      .getByLabel("Speech speed", { exact: true })
      .selectOption("1.25");
    await capture(page, "voice-settings");
    await drawer.getByRole("button", { name: "Close panel" }).click();
    await message.hover();
    await expect(
      message.getByRole("button", { name: "Copy message", exact: true }),
    ).toBeVisible();
    await message
      .getByRole("button", { name: "Speak (TTS)", exact: true })
      .click();
    await expect(
      message.getByRole("button", { name: "Pause", exact: true }),
    ).toBeVisible();
    expect(requestBody).toMatchObject({
      provider: "fixture-voice",
      model: "fixture-speaker",
      profile: "warm",
      speed: 1.25,
    });
    await message.getByRole("button", { name: "Pause", exact: true }).click();
    await expect(
      message.getByRole("button", { name: "Resume", exact: true }),
    ).toBeVisible();
    await capture(page, "message-voice-paused");
    await message.getByRole("button", { name: "Resume", exact: true }).click();
    await expect(
      message.getByRole("button", { name: "Pause", exact: true }),
    ).toBeVisible();
    await page.locator(".code-new-chat").click();
    await expect(
      page.getByRole("button", { name: "Stop spoken reply" }),
    ).toHaveCount(0);
  });

  test("cancels a pending microphone permission without duplicating or leaking capture", async ({
    page,
  }) => {
    await page.addInitScript(() => {
      const state = { requests: 0, stops: 0, resolve: null as any };
      (window as any).__microphoneFixture = state;
      Object.defineProperty(navigator, "mediaDevices", {
        configurable: true,
        value: {
          getUserMedia: () => {
            state.requests += 1;
            return new Promise((resolve) => {
              state.resolve = () =>
                resolve({
                  getTracks: () => [
                    {
                      stop: () => {
                        state.stops += 1;
                      },
                    },
                  ],
                });
            });
          },
        },
      });
    });
    await signIn(page);
    await selectWorkflow(page, "Assistant contract");
    await page.locator(".pc-composer textarea").fill("Prepare dictation");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    const mic = page.getByRole("button", {
      name: "Hold to dictate",
      exact: true,
    });
    await expect(mic).toBeEnabled();
    await mic.dispatchEvent("pointerdown", { button: 0 });
    await mic.dispatchEvent("pointerdown", { button: 0 });
    expect(
      await page.evaluate(() => (window as any).__microphoneFixture.requests),
    ).toBe(1);
    await page.locator(".code-new-chat").click();
    await page.evaluate(() => (window as any).__microphoneFixture.resolve());
    await expect
      .poll(() =>
        page.evaluate(() => (window as any).__microphoneFixture.stops),
      )
      .toBe(1);
    await expect(page.locator(".pc-composer textarea")).toHaveValue("");
    await expect(
      page.getByRole("button", {
        name: "Recording — release to transcribe",
        exact: true,
      }),
    ).toHaveCount(0);
  });

  test("discards queued recorder events from an earlier conversation", async ({
    page,
  }) => {
    await page.addInitScript(() => {
      const state = { stoppedTracks: 0, queued: [] as Array<() => void> };
      (window as any).__recorderFixture = state;
      Object.defineProperty(navigator, "mediaDevices", {
        configurable: true,
        value: {
          getUserMedia: async () => ({
            getTracks: () => [
              {
                stop: () => {
                  state.stoppedTracks += 1;
                },
              },
            ],
          }),
        },
      });
      (window as any).MediaRecorder = class {
        static isTypeSupported() {
          return true;
        }
        onstop: any;
        ondataavailable: any;
        onerror: any;
        state = "inactive";
        start() {
          this.state = "recording";
        }
        stop() {
          if (this.state !== "recording") throw new Error("Already inactive");
          this.state = "inactive";
          const data = this.ondataavailable,
            stopped = this.onstop;
          state.queued.push(() => {
            data?.({
              data: new Blob(["old recording"], { type: "audio/webm" }),
            });
            stopped?.();
          });
        }
      };
    });
    const audioUploads: string[] = [];
    page.on("request", (request) => {
      if (
        request.method() === "POST" &&
        /attachments|audio\/transcribe/.test(request.url())
      )
        audioUploads.push(request.url());
    });
    await signIn(page);
    await selectWorkflow(page, "Assistant contract");
    const start = async (prompt: string) => {
      await page.locator(".pc-composer textarea").fill(prompt);
      await page.getByRole("button", { name: "Send", exact: true }).click();
      await expect(
        page.getByRole("button", { name: "Hold to dictate", exact: true }),
      ).toBeEnabled();
    };
    await start("First microphone owner");
    await page
      .getByRole("button", { name: "Hold to dictate", exact: true })
      .dispatchEvent("pointerdown", { button: 0 });
    await expect(
      page.getByRole("button", {
        name: "Recording — release to transcribe",
        exact: true,
      }),
    ).toBeVisible();
    await page
      .getByRole("button", {
        name: "Recording — release to transcribe",
        exact: true,
      })
      .dispatchEvent("pointerup", { button: 0 });
    await page.locator(".code-new-chat").click();
    await start("Second microphone owner");
    await page
      .getByRole("button", { name: "Hold to dictate", exact: true })
      .dispatchEvent("pointerdown", { button: 0 });
    await expect(
      page.getByRole("button", {
        name: "Recording — release to transcribe",
        exact: true,
      }),
    ).toBeVisible();
    await page.evaluate(() => (window as any).__recorderFixture.queued[0]());
    await expect(
      page.getByRole("button", {
        name: "Recording — release to transcribe",
        exact: true,
      }),
    ).toBeVisible();
    expect(audioUploads).toEqual([]);
    await expect(page.locator(".pc-composer textarea")).toHaveValue("");
    await page.locator(".code-new-chat").click();
    await expect
      .poll(() =>
        page.evaluate(() => (window as any).__recorderFixture.stoppedTracks),
      )
      .toBe(2);
  });

  test("browses and attaches fixture files, uploads a browser file, and signs out", async ({
    page,
  }) => {
    await signIn(page);
    const fileSearch = page.getByLabel("Search workspace files");
    await fileSearch.fill("welcome.md");
    const sharedFile = page.getByRole("button", { name: /welcome\.md/ });
    await expect(sharedFile).toBeVisible();
    await sharedFile.click();
    await expect(
      page
        .getByLabel("Attached files")
        .getByText("welcome.md", { exact: true }),
    ).toBeVisible();

    await page.locator('input[type="file"]').setInputFiles({
      name: "browser-upload.txt",
      mimeType: "text/plain",
      buffer: Buffer.from("AbstractCode isolated browser upload\n"),
    });
    await expect(
      page
        .getByLabel("Attached files")
        .getByText("browser-upload.txt", { exact: true }),
    ).toBeVisible();

    await page
      .getByRole("button", { name: "Disconnect from gateway", exact: true })
      .click();
    const signedOutDialog = page.getByRole("dialog", {
      name: "Gateway connection",
    });
    await expect(signedOutDialog).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Connect to gateway", exact: true }),
    ).toBeVisible();
    await expect(
      signedOutDialog.getByText("Signed out", { exact: true }),
    ).toBeVisible();
  });
});
