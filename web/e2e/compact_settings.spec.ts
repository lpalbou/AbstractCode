import { expect, test } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { openWorkspaceSection } from "./drawer_navigation";

const shots = process.env.ABSTRACTCODE_SETTINGS_SHOTS;
for (const theme of ["dark", "light"]) {
  test(`compact settings and shared docs layout (${theme})`, async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", e => errors.push(e.message));
    await page.addInitScript(t => localStorage.setItem("af_appearance_abstractcode_v1", JSON.stringify({theme:t === "dark" ? "observer-night" : "light",font_scale:"md",header_density:"standard"})), theme);
    await page.goto("/");
    const connection = page.getByRole("dialog", {name:"Gateway connection"});
    await page.locator("#gateway-session-url").fill(process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781");
    await page.locator("#gateway-session-user").fill("web-tester");
    await page.locator("#gateway-session-token").fill("abstractcode-e2e-only");
    await connection.getByRole("button", {name:"Sign in",exact:true}).click();
    await expect(connection).toBeHidden();
    const rail = page.getByRole("tablist", {name:"Workspace panels",exact:true});
    await expect(rail.getByRole("tab")).toHaveCount(6);
    await expect(rail.getByRole("tab", {name:"Workflow",exact:true})).toHaveCount(0);
    await expect(rail.getByRole("tab", {name:"Tools",exact:true})).toHaveCount(0);
    await expect(rail.getByRole("tab", {name:"Skills",exact:true})).toHaveCount(0);
    const saveShot = async (name:string) => { if(shots) {mkdirSync(shots,{recursive:true});await page.screenshot({path:join(shots,`${name}-${theme}.png`)});} };

    const model = await openWorkspaceSection(page,"Model");
    await expect(model.locator(".af-pmp__options")).not.toHaveAttribute("open", "");
    await expect(model.getByText("Reasoning & speed", {exact:false})).toBeVisible();
    await expect(model.getByLabel("Additional instructions")).toHaveCount(0);
    await expect(model.getByLabel("Context token limit")).toBeHidden();
    await model.locator(".code-settings-advanced > summary").click();
    await expect(model.getByLabel("Context token limit")).toBeVisible();
    await model.locator(".code-settings-advanced > summary").click();
    await expect(model.getByText("Loading providers…",{exact:true})).toHaveCount(0);
    await saveShot("model");

    const agent = await openWorkspaceSection(page,"Agent");
    await expect(agent.getByRole("combobox",{name:"Workflow",exact:true})).toBeVisible();
    await expect(agent.getByText("Time zone",{exact:true})).toHaveCount(0);
    const groups = agent.locator(".af-tool-modes__toggle:visible");
    await expect(groups.first()).toBeVisible();
    expect(await groups.count()).toBeGreaterThan(0);
    expect(await groups.evaluateAll(es=>es.every(e=>e.getAttribute("aria-expanded")==="false"))).toBe(true);
    const category = agent.locator('.af-tool-modes__group').filter({has:page.locator('[data-group-mode$=":off"]:enabled')}).first();
    await category.locator('[data-group-mode$=":off"]').click();
    await expect(category.locator('[data-group-mode$=":off"]')).toHaveAttribute("aria-pressed","true");
    await expect(category.locator(".af-tool-modes__toggle")).toHaveAttribute("aria-expanded","false");
    await category.locator(".af-tool-modes__toggle").click();
    const tool = category.locator(".af-tool-mode-row:not(.is-unavailable)").first();
    await expect(tool).toBeVisible();
    await tool.locator('[data-tool-mode$=":ask"]').click();
    await expect(tool).toHaveAttribute("data-mode","ask");
    await expect(category.locator('[data-group-mode$=":off"]')).toHaveAttribute("aria-pressed","false");
    await saveShot("agent-expanded");
    await category.locator(".af-tool-modes__toggle").click();
    const info = agent.getByRole("button",{name:"About tool permissions",exact:true});
    await info.hover();
    await page.waitForTimeout(100);
    await expect(page.locator(".af-tooltip:visible")).toHaveCount(0);
    await expect(page.locator(".af-tooltip:visible")).toBeVisible();
    await expect(page.locator(".af-tooltip:visible")).toContainText("Gateway restrictions always apply");
    await page.keyboard.press("Escape");
    await page.mouse.move(1,1);
    await saveShot("agent");

    const workspace = await openWorkspaceSection(page,"Workspace");
    await expect(workspace.locator('.af-workspace--compact')).toBeVisible();
    await expect(workspace.locator('[data-workspace="gateway-line"],[data-workspace="effective"]')).toHaveCount(0);
    await expect(workspace.getByRole("button",{name:"About workspace access"})).toBeVisible();
    await saveShot("workspace");

    await page.getByRole("button",{name:"Docs assistant",exact:true}).click();
    const docs = page.locator(".pc-docs-assistant-drawer");
    await expect(docs).toBeVisible();
    await expect(docs.locator(".af-drawer__body")).toHaveCount(1);
    await expect(docs.locator(".pc-docs-assistant__composer")).toBeVisible();
    const anchored = () => docs.evaluate(el=>{const body=el.querySelector('.af-drawer__body')!.getBoundingClientRect(),footer=el.querySelector('.pc-docs-assistant__footer')!.getBoundingClientRect();return Math.abs(body.bottom-footer.bottom)<3;});
    expect(await anchored()).toBe(true);
    await saveShot("docs");
    await docs.getByRole("button",{name:/^Close/}).click();
    await expect(workspace).toBeVisible();
    await page.getByRole("button",{name:"Docs assistant",exact:true}).click();
    for(const width of [390,320]) {
      await page.setViewportSize({width,height:844});
      if (!await docs.isVisible()) await page.getByRole("button",{name:"Docs assistant",exact:true}).click();
      await expect(docs).toBeVisible();
      expect(await anchored()).toBe(true);
      expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1)).toBe(true);
      await saveShot(`docs-${width}`);
      await docs.getByRole("button",{name:/^Close/}).click();
      for(const name of ["Model","Agent","Workspace"] as const) {
        const settings = await openWorkspaceSection(page,name);
        expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1)).toBe(true);
        expect(await settings.locator('.af-rail__body').evaluate(e=>e.scrollWidth<=e.clientWidth+1)).toBe(true);
        await saveShot(`${name.toLowerCase()}-${width}`);
      }

    }
    expect(errors).toEqual([]);
  });
}
