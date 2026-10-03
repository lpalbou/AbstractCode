import { expect, type Page } from "@playwright/test";
export async function openWorkspaceSection(page: Page, section: string) {
  const drawer = page.getByRole("complementary", { name: "Workspace & settings", exact: true });
  if (!(await drawer.isVisible())) await page.getByRole("button", { name: "Workspace & settings", exact: true }).click();
  await drawer.getByRole("tab", { name: section, exact: true }).click();
  return drawer;
}
export async function openWorkflowInputs(page: Page) {
  const drawer = await openWorkspaceSection(page, "Model & behavior");
  const details = drawer.locator(".code-panel-inputs");
  if (await details.getAttribute("open") === null) await details.locator(":scope > summary").click();
  await expect(details).toHaveAttribute("open", "");
  return drawer;
}
export async function closeWorkspaceDrawer(page: Page) {
  const drawer = page.getByRole("complementary", { name: "Workspace & settings", exact: true });
  if (await drawer.isVisible()) await drawer.getByRole("button", { name: "Close panel", exact: true }).click();
}
