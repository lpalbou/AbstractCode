import { expect, type Page } from "@playwright/test";

// Round 4: the right panel is the kit rail drawer (Activity / Files /
// Settings). The old drawer categories map onto it: Activity and Files are
// panels; Model & behavior, Tools & skills, Workspace and Voice are groups of
// the Settings panel.
const SETTINGS_GROUPS: Record<string, string> = {
  "Model & behavior": "model",
  "Tools & skills": "tools",
  Workspace: "workspace",
  Voice: "voice",
};

export function railTab(page: Page, panel: "Activity" | "Files" | "Settings") {
  return page.getByRole("tablist", { name: "Workspace panels", exact: true }).getByRole("tab", { name: panel, exact: true });
}

export async function openWorkspaceSection(page: Page, section: string) {
  const panelName = section === "Activity" || section === "Files" ? section : "Settings";
  const tab = railTab(page, panelName as "Activity" | "Files" | "Settings");
  if ((await tab.getAttribute("aria-selected")) !== "true") await tab.click();
  const panel = page.locator(`#code-rail-panel-${panelName.toLowerCase()}`);
  await expect(panel).toBeVisible();
  const group = SETTINGS_GROUPS[section];
  if (group) await panel.locator(`#code-settings-${group}`).scrollIntoViewIfNeeded();
  return panel;
}

export async function openWorkflowInputs(page: Page) {
  const panel = await openWorkspaceSection(page, "Model & behavior");
  const details = panel.locator(".code-panel-inputs");
  if (await details.getAttribute("open") === null) await details.locator(":scope > summary").click();
  await expect(details).toHaveAttribute("open", "");
  return panel;
}

/** Collapse the rail to its icons (the panel's collapse button). */
export async function closeWorkspaceDrawer(page: Page) {
  const collapse = page.locator('.code-rail .af-rail__section:not([hidden]) [data-action="collapse-panel"]');
  if (await collapse.count() && await collapse.first().isVisible()) await collapse.first().click();
}
