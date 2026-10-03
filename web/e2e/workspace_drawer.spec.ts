import { expect, test } from "@playwright/test";
import http from "node:http";
import { createCodeServer } from "../bin/server.js";
const gateway = process.env.ABSTRACTCODE_E2E_GATEWAY_URL || "http://127.0.0.1:18781";
const origin = process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782";
let app: http.Server;
test.beforeAll(async () => {
  app = createCodeServer({ defaultGatewayUrl: gateway });
  await new Promise<void>(resolve => app.listen(Number(new URL(origin).port), "127.0.0.1", resolve));
});
test.afterAll(async () => { app?.closeAllConnections(); await new Promise<void>(resolve => app.close(() => resolve())); });

for (const [name, width, height] of [["desktop",1440,960],["ipad",768,1024],["ipad-landscape",1024,768],["phone",390,844],["small-phone",320,568],["phone-landscape",844,390]] as const) {
  test.describe(name, () => {
  test.use({ viewport: {width, height}, hasTouch: name !== "desktop", isMobile: name !== "desktop" });
  test(`one drawer, six reachable categories on ${name}`, async ({page}) => {
    await page.setViewportSize({width,height});
    await page.goto(origin);
    await page.locator("#gateway-session-url").fill(gateway);
    await page.locator("#gateway-session-user").fill("web-tester");
    await page.locator("#gateway-session-token").fill("abstractcode-e2e-only");
    await page.getByRole("button",{name:"Sign in",exact:true}).click();
    const opener=page.getByRole("button",{name:"Workspace & settings",exact:true});
    await expect(opener).toHaveCount(1);
    const actions = page.getByRole("group", {name: "App actions", exact: true});
    await expect(actions.getByRole("button", {name: "Appearance (theme and typography)", exact: true})).toBeVisible();
    await expect(actions.getByRole("button", {name: "About AbstractCode", exact: true})).toBeVisible();
    await expect(actions.getByRole("button", {name: "Disconnect from gateway", exact: true})).toBeVisible();
    const assistantButton = actions.getByRole("button", {name: "Code assistant (docs-grounded)", exact: true});
    await assistantButton.click();
    const assistant = page.getByRole("complementary", {name: "Code assistant", exact: true});
    await expect(assistant).toBeVisible();
    await expect(assistant.getByRole("textbox")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(assistant).not.toBeVisible();
    await actions.getByRole("button", {name: "Appearance (theme and typography)", exact: true}).click();
    await expect(page.getByRole("dialog", {name: "Appearance", exact: true})).toBeVisible();
    await page.keyboard.press("Escape");
    await actions.getByRole("button", {name: "About AbstractCode", exact: true}).click();
    await expect(page.getByRole("dialog", {name: "About AbstractCode", exact: true})).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("button",{name:"Run settings",exact:true})).toHaveCount(0);
    await expect(page.getByRole("button",{name:"Toggle workspace inspector",exact:true})).toHaveCount(0);
    if (name === "desktop") await opener.click(); else await opener.tap();
    const drawer=page.getByRole("complementary",{name:"Workspace & settings",exact:true});
    await expect(drawer).toBeVisible();
    const openerBox = (await opener.boundingBox())!;
    expect(openerBox.x + openerBox.width).toBeLessThanOrEqual(width + 1);
    expect(await opener.evaluate(el => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
    expect(await drawer.evaluate(el => getComputedStyle(el).backgroundColor)).toMatch(/^rgb\(/);
    const box=(await drawer.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0); expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.x+box.width).toBeLessThanOrEqual(width+1); expect(box.y+box.height).toBeLessThanOrEqual(height+1);
    const tabs=drawer.getByRole("tab");await expect(tabs).toHaveCount(6);
    for(const label of ["Activity","Files","Model & behavior","Tools & skills","Workspace","Voice"]){
      const tab=drawer.getByRole("tab",{name:label,exact:true}); if (name === "desktop") await tab.click(); else await tab.tap();
      await expect(tab).toHaveAttribute("aria-selected","true");
      const bounds=(await tab.boundingBox())!; expect(bounds.height).toBeGreaterThanOrEqual(44);
      expect(bounds.y+bounds.height).toBeLessThanOrEqual(height+1);
      expect(await drawer.evaluate(el=>el.scrollWidth<=el.clientWidth+1)).toBe(true);
      expect(await drawer.locator(".code-workspace-tabs > .af-tabs__panel").evaluate(el=>el.scrollWidth<=el.clientWidth+1)).toBe(true);
      await expect(drawer.getByRole("tabpanel")).toHaveCount(1);
    }
    await drawer.getByRole("tab",{name:"Model & behavior",exact:true}).click();
    await drawer.getByRole("combobox",{name:"Workflow",exact:true}).click();
    await drawer.getByRole("option").filter({has:page.locator(".af-workflow-picker__name",{hasText:"Basic agent defaults"})}).click();
    await drawer.getByRole("tab",{name:"Files",exact:true}).click();
    await drawer.getByRole("tab",{name:"Model & behavior",exact:true}).click();
    await expect(drawer.getByRole("combobox",{name:"Workflow",exact:true})).toContainText("Basic agent defaults");
    await page.screenshot({path:`e2e/artifacts/drawer-${name}.png`});
    // Scroll the body to the bottom without losing the fixed categories or close control.
    await drawer.locator(".code-workspace-tabs > .af-tabs__panel").evaluate(el => { el.scrollTop = el.scrollHeight; });
    await expect(drawer.getByRole("button", {name: "Close panel", exact: true})).toBeInViewport();
    await expect(drawer.getByRole("tab", {name: "Model & behavior", exact: true})).toBeInViewport();
    const active=drawer.getByRole("tab",{name:"Model & behavior",exact:true});await active.focus();
    await page.keyboard.press("Home");await expect(drawer.getByRole("tab",{name:"Activity",exact:true})).toBeFocused();
    await page.keyboard.press("End");await expect(drawer.getByRole("tab",{name:"Voice",exact:true})).toBeFocused();
    await page.keyboard.press("Escape");await expect(drawer).not.toBeVisible();await expect(opener).toBeFocused();
    await opener.click();await expect(drawer.getByRole("tab",{name:"Voice",exact:true})).toHaveAttribute("aria-selected","true");
    if (width < 1024) {
      await page.getByRole("button", {name: "Open conversation navigation", exact: true}).tap();
      await expect(drawer).not.toBeVisible();
      await expect(page.locator(".code-app")).toHaveClass(/code-app--nav-open/);
      await page.getByRole("button", {name: "Close navigation", exact: true}).tap();
      await opener.tap();
    }
    await drawer.getByRole("button",{name:"Close panel",exact:true}).click();await expect(drawer).not.toBeVisible();
    await page.locator(".code-conversation .pc-composer textarea").fill("Drawer navigation preserves the conversation.");
    await page.getByRole("button",{name:"Send",exact:true}).click();
    await expect(page.locator(".pc-chat-item--assistant").first()).toBeVisible();
    await opener.click();await drawer.getByRole("tab",{name:"Activity",exact:true}).click();
    await expect(drawer.getByRole("tabpanel")).not.toContainText("No activity yet");
  });
  });
}
