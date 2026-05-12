import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("@abstractframework/ui-kit theme.css", () => {
  it("tokenizes AfSelect typography so font scale applies", () => {
    const css = readFileSync(new URL("../../../../abstractuic/ui-kit/src/theme.css", import.meta.url), "utf8");
    expect(css).toMatch(/\.af-select-trigger\s*\{[\s\S]*?font-size:\s*var\(--font-size-base\)/);
    expect(css).toMatch(/\.af-select-trigger\s*\{[\s\S]*?font-family:\s*var\(--font-sans\)/);
    expect(css).toMatch(/\.af-select-option\s*\{[\s\S]*?font-size:\s*var\(--font-size-base\)/);
  });
});
