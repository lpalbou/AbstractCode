import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

// Resolve through the package's own `exports` map rather than a path into a
// sibling checkout: this asserts against the theme.css we actually SHIP with,
// and it keeps the test working wherever the repo is cloned.
const require_ = createRequire(import.meta.url);

describe("@abstractframework/ui-kit theme.css", () => {
  it("tokenizes AfSelect typography so font scale applies", () => {
    const css = readFileSync(require_.resolve("@abstractframework/ui-kit/theme.css"), "utf8");
    expect(css).toMatch(/\.af-select-trigger\s*\{[\s\S]*?font-size:\s*var\(--font-size-base\)/);
    expect(css).toMatch(/\.af-select-trigger\s*\{[\s\S]*?font-family:\s*var\(--font-sans\)/);
    expect(css).toMatch(/\.af-select-option\s*\{[\s\S]*?font-size:\s*var\(--font-size-base\)/);
  });
});
