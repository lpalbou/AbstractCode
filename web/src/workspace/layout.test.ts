import { readFileSync } from "node:fs";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  escapeTarget,
  initialInspector,
  inspectorOnModeChange,
  inspectorOnToggle,
  keyboardOpen,
  paneModeFrom,
  sidebarOnModeChange,
  type InspectorState,
  type PaneMode,
} from "./layout";
import { SidebarDrawer } from "./sidebar_drawer";

const css = readFileSync(new URL("./workspace.css", import.meta.url), "utf8");
const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");

/** The body of the first `@media <query> { ... }` block (balanced braces). */
function mediaBlock(query: string): string {
  const start = css.indexOf(`@media ${query} {`);
  expect(start, `@media ${query} exists`).toBeGreaterThanOrEqual(0);
  let depth = 0;
  for (let i = css.indexOf("{", start); i < css.length; i++) {
    if (css[i] === "{") depth++;
    if (css[i] === "}" && --depth === 0) return css.slice(start, i + 1);
  }
  throw new Error("unbalanced");
}

/** Pane mode of a viewport width (the app derives it from AF_MEDIA.lg / AF_MEDIA.md). */
const modeAt = (width: number): PaneMode => paneModeFrom(width <= 1439.98, width <= 1023.98);

/** Replays a live resize through the widths, the way app.tsx's pane-mode effect does. */
function resize(widths: number[], start: InspectorState = initialInspector(modeAt(widths[0]))) {
  let state = start;
  let mode = modeAt(widths[0]);
  const trail: boolean[] = [state.open];
  for (const w of widths.slice(1)) {
    const next = modeAt(w);
    state = inspectorOnModeChange(state, mode, next);
    mode = next;
    trail.push(state.open);
  }
  return { state, trail };
}

describe("pane modes (DESIGN §5.3)", () => {
  it("docks three panes only from 1440 px; 1024-1439 is overlay; below 1024 drawers", () => {
    expect(modeAt(2560)).toBe("docked");
    expect(modeAt(1440)).toBe("docked");
    expect(modeAt(1439)).toBe("overlay");
    expect(modeAt(1180)).toBe("overlay");
    expect(modeAt(1024)).toBe("overlay");
    expect(modeAt(1023)).toBe("drawers");
    expect(modeAt(375)).toBe("drawers");
  });

  it("opens the inspector by default only where three panes dock", () => {
    expect(initialInspector("docked").open).toBe(true);
    expect(initialInspector("overlay").open).toBe(false);
    expect(initialInspector("drawers").open).toBe(false);
  });

  it("resizing down from 1440 folds the docked inspector at 1439 (never three panes below xl)", () => {
    const { trail } = resize([1512, 1440, 1439, 1280, 1100]);
    expect(trail).toEqual([true, true, false, false, false]);
  });

  it("resizing below 1024 folds the panes and returns the inspector above 1440", () => {
    const { trail, state } = resize([1512, 1280, 900, 360, 900, 1280, 1440, 1728]);
    expect(trail).toEqual([true, false, false, false, false, false, true, true]);
    expect(state).toEqual({ open: true, wanted: true });
  });

  it("remembers a user who closed the docked inspector: it stays closed back at xl", () => {
    let state = inspectorOnToggle(initialInspector("docked"), "docked", false);
    expect(state).toEqual({ open: false, wanted: false });
    state = resize([1512, 900, 1512], state).state;
    expect(state.open).toBe(false);
  });

  it("an overlay opened below xl does not become the docked preference", () => {
    let state: InspectorState = { open: false, wanted: false };
    state = inspectorOnToggle(state, "overlay", true);
    expect(state).toEqual({ open: true, wanted: false });
    state = inspectorOnModeChange(state, "overlay", "docked");
    expect(state.open).toBe(false);
  });

  it("leaving drawer mode closes the navigation drawer; entering keeps it as it was", () => {
    expect(sidebarOnModeChange(true, "overlay")).toBe(false);
    expect(sidebarOnModeChange(true, "docked")).toBe(false);
    expect(sidebarOnModeChange(false, "drawers")).toBe(false);
  });

  it("app.tsx wires the resize effect and the toggle through these reducers", () => {
    expect(appSource).toMatch(/inspectorOnModeChange\(state, from, paneMode\)/);
    expect(appSource).toMatch(/sidebarOnModeChange\(open, paneMode\)/);
    expect(appSource).toMatch(/inspectorOnToggle\(/);
  });
});

describe("Escape closes one layer", () => {
  const base = {
    key: "Escape",
    defaultPrevented: false,
    overlayOpen: false,
    sidebarOpen: false,
    inspectorOpen: false,
    inspectorIsOverlay: false,
  };

  it("closes the navigation drawer, then an overlay inspector", () => {
    expect(escapeTarget({ ...base, sidebarOpen: true })).toBe("sidebar");
    expect(escapeTarget({ ...base, inspectorOpen: true, inspectorIsOverlay: true })).toBe("inspector");
    expect(escapeTarget({ ...base, inspectorOpen: true, inspectorIsOverlay: false })).toBeNull();
  });

  it("leaves the drawer open when Run settings or a dialog above it takes the Escape", () => {
    expect(escapeTarget({ ...base, sidebarOpen: true, overlayOpen: true })).toBeNull();
    expect(escapeTarget({ ...base, sidebarOpen: true, defaultPrevented: true })).toBeNull();
    expect(escapeTarget({ ...base, inspectorOpen: true, inspectorIsOverlay: true, overlayOpen: true })).toBeNull();
  });

  it("ignores other keys", () => {
    expect(escapeTarget({ ...base, key: "Enter", sidebarOpen: true })).toBeNull();
  });

  it("app.tsx counts every drawer and dialog above the panes as the owner of the key", () => {
    const call = appSource.slice(appSource.indexOf("escapeTarget({"), appSource.indexOf("});", appSource.indexOf("escapeTarget({")));
    for (const owner of ["settingsOpen", "inputsOpen", "voiceOpen", "newAutomationOpen", "appearanceOpen", "kitDialogOpen()"]) {
      expect(call, owner).toContain(owner);
    }
    expect(call).toContain("defaultPrevented: event.defaultPrevented");
  });
});

describe("closed navigation drawer", () => {
  it("is inert below 1024 px when closed, and interactive when open or docked", () => {
    const html = (isDrawer: boolean, open: boolean) =>
      renderToStaticMarkup(React.createElement(SidebarDrawer, { isDrawer, open }, "x"));
    expect(html(true, false)).toMatch(/<aside[^>]*\binert=""/);
    expect(html(true, true)).not.toMatch(/\binert/);
    expect(html(false, false)).not.toMatch(/\binert/);
  });

  it("is visibility: hidden off-canvas below 1024 px and visible when open (CSS)", () => {
    const md = mediaBlock("(max-width: 1023.98px)");
    expect(md).toMatch(/\.code-sidebar \{[^}]*position: fixed;[^}]*visibility: hidden;/);
    expect(md).toMatch(/\.code-app--nav-open \.code-sidebar \{[^}]*visibility: visible;/);
  });

  it("app.tsx renders the sidebar through SidebarDrawer", () => {
    expect(appSource).toMatch(/<SidebarDrawer isDrawer=\{panesAreDrawers\} open=\{sidebarOpen\}>/);
  });
});

describe("workspace.css responsive contract", () => {
  it("uses only the named breakpoints (480/768/1024/1440, max-height 500)", () => {
    const widths = [...css.matchAll(/@media[^{]*?(?:max|min)-width:\s*([\d.]+)px/g)].map((m) => m[1]);
    expect(widths.length).toBeGreaterThan(0);
    for (const w of widths) expect(["479.98", "767.98", "1023.98", "1439.98", "1440"]).toContain(w);
    const heights = [...css.matchAll(/@media[^{]*?max-height:\s*([\d.]+)px/g)].map((m) => m[1]);
    for (const h of heights) expect(h).toBe("500");
  });

  it("sizes the shell to the visible viewport, never a bare 100vh or the old --vh variable", () => {
    const shell = /\.code-app \{[^}]*\}/.exec(css)?.[0] || "";
    expect(shell).toContain("var(--vv-height, var(--vh-full");
    expect(css).not.toContain("var(--vh,");
  });

  it("wraps the toolbar below 1024 px, the workflow group keeping a usable basis", () => {
    const md = mediaBlock("(max-width: 1023.98px)");
    expect(md).toMatch(/\.code-toolbar \{[^}]*flex-wrap: wrap;/);
    expect(md).toMatch(/\.code-workflow-select \{[^}]*flex: 1 1 18rem;/);
  });

  it("makes the inspector an overlay (not a docked pane) below 1440 px", () => {
    const lg = mediaBlock("(max-width: 1439.98px)");
    expect(lg).toMatch(/\.code-inspector \{[^}]*position: fixed;/);
    expect(lg).toMatch(/\.code-inspector-scrim \{[^}]*display: block;/);
  });

  it("keeps the narrowest composer on two control rows (Stop icon-only, Send may ellipsize)", () => {
    expect(css).toMatch(/@container pc-composer \(max-width: 359\.98px\) \{[\s\S]*?\.pc-workflow-chat__stop > span \{[^}]*clip-path: inset\(50%\);/);
    expect(css).toMatch(/@container pc-composer \(max-width: 359\.98px\) \{[\s\S]*?\.pc-composer__row > \.pc-btn \{[^}]*max-width: 45%;/);
  });

  it("compacts the chrome while the on-screen keyboard is up and caps the composer field", () => {
    expect(css).toMatch(/:root\[data-keyboard="open"\] \.code-toolbar,[\s\S]*?\{\s*display: none;/);
    expect(css).toMatch(/:root\[data-keyboard="open"\] \.code-conversation \.pc-composer__textarea \{[^}]*max-height: calc\(var\(--vv-height[^;]*\* 0\.3\);/);
  });

  it("never overrides the kit drawer's inline top with !important (topOffset is measured)", () => {
    expect(css).not.toMatch(/\.code-settings-drawer \{[^}]*!important/);
    expect(appSource).toMatch(/topOffset=\{drawerTop\}/);
  });
});

describe("on-screen keyboard flag", () => {
  it("reads --keyboard-inset as open only when it is a positive length", () => {
    expect(keyboardOpen("300px")).toBe(true);
    expect(keyboardOpen(" 12.5px ")).toBe(true);
    expect(keyboardOpen("0px")).toBe(false);
    expect(keyboardOpen("")).toBe(false);
    expect(keyboardOpen(undefined)).toBe(false);
  });
});

describe("index.html viewport", () => {
  it("allows pinch zoom and covers the safe areas", () => {
    const html = readFileSync(new URL("../../index.html", import.meta.url), "utf8");
    const meta = /<meta name="viewport" content="([^"]+)"/.exec(html)?.[1] || "";
    expect(meta).toContain("viewport-fit=cover");
    expect(meta).not.toMatch(/user-scalable|maximum-scale/);
  });
});
