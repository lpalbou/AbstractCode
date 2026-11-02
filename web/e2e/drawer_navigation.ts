import { expect, type Page } from "@playwright/test";

// Round 5: the right panel is the kit rail drawer with one icon per subject:
// Activity, Files, Model, Workflow, Workspace, Tools, Skills, Voice. Each is
// its own panel (`#code-rail-panel-<id>`); there is no Settings panel.
export const RAIL_PANELS = ["Activity", "Files", "Model", "Agent", "Workspace", "Voice"] as const;
const canonical = (name: RailPanelName): string => ["Workflow", "Tools", "Skills"].includes(name) ? "Agent" : name;
export type RailPanelName = (typeof RAIL_PANELS)[number] | "Workflow" | "Tools" | "Skills";

export function railTab(page: Page, panel: RailPanelName) {
  return page.getByRole("tablist", { name: "Workspace panels", exact: true }).getByRole("tab", { name: canonical(panel), exact: true });
}

export async function openWorkspaceSection(page: Page, section: RailPanelName) {
  if (!RAIL_PANELS.includes(canonical(section) as (typeof RAIL_PANELS)[number])) throw new Error(`no rail panel named ${section}`);
  const tab = railTab(page, section);
  const panel = page.locator(`#code-rail-panel-${canonical(section).toLowerCase()}`);
  // A viewport change collapses a floating panel a render later: decide on what is visible, retrying.
  // It must STAY visible across a short settle (a panel shown just before the collapse would not).
  await expect(async () => {
    if (!(await panel.isVisible())) await tab.click();
    await expect(panel).toBeVisible({ timeout: 1_000 });
    await page.waitForTimeout(250);
    await expect(panel).toBeVisible({ timeout: 100 });
  }).toPass({ timeout: 15_000 });
  if (section === "Tools") {
    while (await panel.locator('.af-tool-modes__toggle[aria-expanded="false"]:visible').count()) await panel.locator('.af-tool-modes__toggle[aria-expanded="false"]:visible').first().click();
  }
  if (section === "Skills") {
    const summary = panel.locator('.code-agent-section > summary').filter({hasText: /^Skills/});
    if (await summary.locator('..').getAttribute('open') === null) await summary.click();
  }
  return panel;
}

/** The Workflow panel: the workflow picker, then that workflow's inputs. */
export async function openWorkflowInputs(page: Page) {
  const panel = await openWorkspaceSection(page, "Workflow");
  await expect(panel.getByRole("combobox", {name:"Workflow",exact:true})).toBeVisible();
  return panel;
}

/** Collapse the rail to its icons (the panel's collapse button). */
export async function closeWorkspaceDrawer(page: Page) {
  const collapse = page.locator('.code-rail .af-rail__section:not([hidden]) [data-action="collapse-panel"]');
  if (await collapse.count() && await collapse.first().isVisible()) await collapse.first().click();
}
