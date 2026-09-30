import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { inspectorOpenByDefault, panesAreDrawers } from "./layout";

/** A matchMedia stub for a viewport of the given width (only min/max-width queries). */
function viewport(width: number) {
  return (query: string) => {
    const min = /min-width:\s*([\d.]+)px/.exec(query);
    const max = /max-width:\s*([\d.]+)px/.exec(query);
    return { matches: (!min || width >= Number(min[1])) && (!max || width <= Number(max[1])) };
  };
}

describe("responsive layout state", () => {
  it("docks the inspector by default only where three panes fit (>= 1440 px)", () => {
    expect(inspectorOpenByDefault(viewport(2560))).toBe(true);
    expect(inspectorOpenByDefault(viewport(1512))).toBe(true);
    expect(inspectorOpenByDefault(viewport(1440))).toBe(true);
    expect(inspectorOpenByDefault(viewport(1439))).toBe(false); // lg: two docked panes at most
    expect(inspectorOpenByDefault(viewport(1180))).toBe(false); // iPad landscape
    expect(inspectorOpenByDefault(viewport(375))).toBe(false);
  });

  it("turns side panes into drawers below the md breakpoint (1024 px)", () => {
    expect(panesAreDrawers(viewport(1024))).toBe(false);
    expect(panesAreDrawers(viewport(1023))).toBe(true);
    expect(panesAreDrawers(viewport(852))).toBe(true); // phone landscape
    expect(panesAreDrawers(viewport(820))).toBe(true); // iPad portrait
  });

  it("falls back to docked panes when matchMedia is unavailable or throws", () => {
    expect(panesAreDrawers(() => {
      throw new Error("no media");
    })).toBe(false);
  });
});

describe("workspace.css responsive contract", () => {
  const css = readFileSync(new URL("./workspace.css", import.meta.url), "utf8");

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

  it("makes the sidebar a hidden off-canvas drawer below 1024 px", () => {
    const md = css.slice(css.indexOf("@media (max-width: 1023.98px)"));
    expect(md).toMatch(/\.code-sidebar \{[^}]*position: fixed;[^}]*visibility: hidden;/);
    expect(md).toMatch(/\.code-app--nav-open \.code-sidebar \{[^}]*visibility: visible;/);
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
