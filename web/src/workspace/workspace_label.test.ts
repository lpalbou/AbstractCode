import { describe, expect, it } from "vitest";
import { workspaceLabel } from "./sidebar_panels";

describe("workspaceLabel (operator QA 2026-10-29, ADR 0043)", () => {
  it("names the gateway's own folders in words, never by their id", () => {
    expect(workspaceLabel("/data/workspaces/session-automation-7f7613b4-fd8b-597d-bf94-e2090-74e1f57022c1")).toBe("This automation's folder");
    expect(workspaceLabel("/data/workspaces/session-agws_1Cwqw8dT")).toBe("This conversation's folder");
  });
  it("keeps a folder the person chose by its own name", () => {
    expect(workspaceLabel("/Users/me/projects/site/")).toBe("site");
    expect(workspaceLabel("")).toBe("Gateway managed");
  });
});
