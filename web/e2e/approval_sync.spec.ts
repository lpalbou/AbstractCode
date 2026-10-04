import { openWorkspaceSection, openWorkflowInputs, closeWorkspaceDrawer } from "./drawer_navigation";
import { expect, test, type Page } from "@playwright/test";

// The approval gate is the same in every client (operator rulings: one session
// pool for all clients; "The run waits for your approval, in every client").
// Two browser contexts — two devices — against the isolated fixture gateway
// (e2e/gateway_fixture.py, no model): client A starts "Delegated tool
// approval", whose ROOT run parks on `subworkflow:<child>` while the CHILD
// asks to write a file (the shape of a basic-agent turn). Client B opens the
// same conversation afterwards and must see the same gate from durable state,
// not "Running a tool"; one decision, taken in B, settles the run in A too.
//
// 2026-09-29 operator report: the originating remote Safari showed the gate;
// the gateway machine's Safari and a phone opening the same run showed
// "Running a tool write_file" and a Steer composer. Red before panel-chat 0.1.20.

const appOrigin = process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782";
const fixtureGateway = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const fixtureUser = process.env.ABSTRACTCODE_E2E_USER || "web-tester";
const fixtureToken = process.env.ABSTRACTCODE_E2E_TOKEN || "abstractcode-e2e-only";

/** The page may reach only the locally-started Code proxy (never a live gateway). */
async function blockExternalTraffic(page: Page): Promise<void> {
  const allowedOrigin = new URL(appOrigin).origin;
  await page.route("**/*", async (route) => {
    const url = new URL(route.request().url());
    if (url.origin === allowedOrigin) return route.continue();
    await route.abort("blockedbyclient");
  });
}

/** Round 3: every workflow the Code picker offers declares abstractcode.agent.v1, so a turn
 * starts from the composer (the inputs drawer says "Back to chat"; there is no "Run workflow"). */
async function sendTurn(page: Page, text = "Run the fixture."): Promise<void> {
  const drawer = page.locator(".code-rail .af-rail__panel");
  await closeWorkspaceDrawer(page);
  const composer = page.locator(".code-conversation .pc-composer textarea");
  if (!(await composer.inputValue()).trim()) await composer.fill(text);
  await page.getByRole("button", { name: "Send", exact: true }).click();
}

/** The header's kit WorkflowPicker (round 3: no "Show all workflows" — it lists only what the
 * gateway returns for abstractcode.agent.v1): open it and choose the entry named `name`. */
async function chooseWorkflow(page: Page, name: string): Promise<void> {
  await openWorkspaceSection(page, "Workflow");
  const picker = page.getByRole("combobox", { name: "Workflow", exact: true });
  await expect(picker).toBeEnabled();
  await picker.click();
  const exact = new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`);
  await page
    .getByRole("listbox", { name: "Workflow" })
    .locator('[role="option"]')
    .filter({ has: page.locator(".af-workflow-picker__name", { hasText: exact }) })
    .first()
    .click();
  await expect(page.locator("#code-workflow-picker .af-workflow-picker__name")).toHaveText(name);
  // The chosen workflow's inputs have loaded (a send before that is refused).
  await expect(page.locator(".code-workflow-select")).not.toHaveAttribute("aria-busy", "true");
  await closeWorkspaceDrawer(page);
}

async function signIn(page: Page): Promise<void> {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  const dialog = page.getByRole("dialog", { name: "Gateway connection" });
  await expect(dialog).toBeVisible();
  await page.locator("#gateway-session-url").fill(fixtureGateway);
  await page.locator("#gateway-session-user").fill(fixtureUser);
  await page.locator("#gateway-session-token").fill(fixtureToken);
  await dialog.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(dialog).toBeHidden();
  await openWorkspaceSection(page, "Workflow");
  await expect(page.getByLabel("Workflow", { exact: true })).toBeEnabled();
  await closeWorkspaceDrawer(page);
  await expect(page.locator(".code-statusbar").getByText("Connected", { exact: true })).toBeVisible();
}

async function selectWorkflow(page: Page, name: string): Promise<void> {
  await chooseWorkflow(page, name);
}

async function expectGate(page: Page, who: string): Promise<void> {
  await expect(page.getByRole("heading", { name: "1 action needs permission" }), `${who}: the approval card`).toBeVisible();
  await expect(page.getByText(/Allow write_file to run\?/), `${who}: the question`).toBeVisible();
  await expect(page.getByRole("button", { name: "Allow once", exact: true }), `${who}: Allow once`).toBeVisible();
  await expect(page.getByRole("button", { name: "Deny", exact: true }), `${who}: Deny`).toBeVisible();
  const strip = page.locator(".code-run-strip");
  await expect(strip, `${who}: the run strip`).toContainText("Approval needed");
  await expect(strip, `${who}: never "Running a tool" while parked`).not.toContainText("Running a tool");
  // The composer waits for the decision: its send button says so and the
  // textarea points at the card (not "Guide the current workflow… / Steer").
  await expect(page.locator(".pc-composer").getByRole("button", { name: "Waiting for you" }), `${who}: the composer waits for the decision`).toBeVisible();
  await expect(page.locator(".code-conversation .pc-composer textarea"), `${who}: the composer points at the request`).toHaveAttribute("placeholder", /Answer the request above/);
}

test("a client that opens the conversation later sees the gate the originating client sees", async ({ browser }) => {
  const errors: string[] = [];
  const contextA = await browser.newContext();
  const a = await contextA.newPage();
  a.on("pageerror", (error) => errors.push(`A: ${error.message || String(error)}`));
  await blockExternalTraffic(a);
  await signIn(a);
  await selectWorkflow(a, "Delegated tool approval");
  await sendTurn(a);
  await expectGate(a, "client A (started the turn)");
  await expect(a).toHaveURL(/(?:#|&)run=/);
  const conversationUrl = a.url();
  const runId = new URLSearchParams(new URL(conversationUrl).hash.slice(1)).get("run");
  expect(runId).toBeTruthy();

  // Durable state: the ROOT is parked on its subflow; the approval is the CHILD's wait.
  const root = await (await a.request.get(`${appOrigin}/api/gateway/runs/${encodeURIComponent(runId!)}`)).json();
  expect(root.status).toBe("waiting");
  expect(root.waiting?.reason).toBe("subworkflow");
  const children = await (await a.request.get(`${appOrigin}/api/gateway/runs?parent_run_id=${encodeURIComponent(runId!)}&limit=10`)).json();
  const child = (children.items || []).find((run: any) => run.status === "waiting");
  expect(child, "the child run is the one waiting").toBeTruthy();
  const childRun = await (await a.request.get(`${appOrigin}/api/gateway/runs/${encodeURIComponent(child.run_id)}`)).json();
  expect(childRun.waiting?.details?.mode).toBe("approval_required");
  expect(childRun.waiting?.details?.tool_calls?.[0]?.name).toBe("write_file");

  // Client B: another device opens the same conversation from durable state.
  const contextB = await browser.newContext();
  const b = await contextB.newPage();
  b.on("pageerror", (error) => errors.push(`B: ${error.message || String(error)}`));
  await blockExternalTraffic(b);
  await signIn(b);
  await b.goto(conversationUrl, { waitUntil: "domcontentloaded" });
  await expect(b).toHaveURL(new RegExp(`run=${runId}`));
  await expectGate(b, "client B (opened the conversation later)");
  // Nothing was decided by merely looking.
  const stillRoot = await (await b.request.get(`${appOrigin}/api/gateway/runs/${encodeURIComponent(runId!)}`)).json();
  expect(stillRoot.status).toBe("waiting");

  // One decision, in B, settles the run for A as well.
  await b.getByRole("button", { name: "Deny", exact: true }).click();
  await expect(b.locator(".code-run-strip")).not.toContainText("Approval needed");
  await expect(a.locator(".code-run-strip")).not.toContainText("Approval needed");
  await expect(a.getByRole("button", { name: "Deny", exact: true })).toBeHidden();
  await expect.poll(async () => (await (await a.request.get(`${appOrigin}/api/gateway/runs/${encodeURIComponent(runId!)}`)).json()).status).not.toBe("waiting");

  expect(errors).toEqual([]);
  await contextA.close();
  await contextB.close();
});
