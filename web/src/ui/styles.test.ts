import { describe, expect, it } from "vitest";

import css from "./styles.css?raw";

describe("AbstractCode Web styles", () => {
  it("uses shared typography tokens for base sizing", () => {
    expect(css).toMatch(/font-size:\s*var\(--font-size-base\)/);
    expect(css).toMatch(/line-height:\s*var\(--line-height-base\)/);
  });
});
