import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("AbstractCode Web styles", () => {
  it("uses shared typography tokens for base sizing", () => {
    const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
    expect(css).toMatch(/font-size:\s*var\(--font-size-base\)/);
    expect(css).toMatch(/line-height:\s*var\(--line-height-base\)/);
    expect(css).toMatch(/min-height:\s*calc\(var\(--header-height\)/);
    expect(css).toMatch(/\.btn\s*\{[^}]*font-size:\s*var\(--font-size-md\)/);
    expect(css).toContain("calc(44px * var(--header-density))");
    expect(css).toMatch(/\.app\s*\{[^}]*position:\s*fixed/);
    expect(css).toMatch(/\.app\s*\{[^}]*height:\s*calc\(var\(--vh, 1vh\)\s*\*\s*100\)/);
  });

  it("avoids fixed px/rem font sizes (respects --font-scale)", () => {
    const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
    expect(css).not.toMatch(/font-size:\s*\d+(?:\.\d+)?px\b/);
    expect(css).not.toMatch(/font-size:\s*\d+(?:\.\d+)?rem\b/);
  });

  it("derives panel backgrounds from theme tokens (no hard-coded base palette)", () => {
    const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
    expect(css).toMatch(/\.panel\s*\{[^}]*background:\s*var\(--bg-card\)/);
    expect(css).not.toMatch(/rgba\(22,\s*33,\s*62/);
  });

  it("makes assistant message stats swipeable on mobile", () => {
    const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
    expect(css).toMatch(/@media\s*\(max-width:\s*480px\)\s*\{[\s\S]*\.chat_stats_bar\s*\{[\s\S]*overflow-x:\s*auto;/);
    expect(css).toMatch(/@media\s*\(max-width:\s*480px\)\s*\{[\s\S]*\.chat_stats_bar\s*\{[\s\S]*justify-content:\s*flex-start;/);
  });

  it("uses a desktop-first settings grid layout (appearance above gateway)", () => {
    const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
    expect(css).toMatch(/\.settings_grid\s*\{[\s\S]*grid-template-areas:\s*[\s\S]*"appearance model"[\s\S]*"gateway model"[\s\S]*"tools tools"/);
    expect(css).toMatch(/\.settings_card_gateway\s*\{\s*grid-area:\s*gateway;\s*\}/);
    expect(css).toMatch(/\.settings_card_model\s*\{\s*grid-area:\s*model;\s*\}/);
    expect(css).toMatch(/\.settings_card_appearance\s*\{\s*grid-area:\s*appearance;\s*\}/);
    expect(css).toMatch(/\.settings_card_tools\s*\{\s*grid-area:\s*tools;\s*\}/);
  });

  it("keeps settings single-column order on mobile (appearance → gateway → model)", () => {
    const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
    expect(css).toMatch(/@media\s*\(max-width:\s*820px\)\s*\{[\s\S]*\.settings_grid\s*\{[\s\S]*grid-template-columns:\s*1fr;[\s\S]*grid-template-areas:\s*[\s\S]*"appearance"[\s\S]*"gateway"[\s\S]*"model"[\s\S]*"tools"/);
  });
});
