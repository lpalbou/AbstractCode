import { describe, expect, it } from "vitest";
import { appVersionFrom } from "../../app_version";

describe("build-time app version", () => {
  it("reads web/package.json's version", () => {
    expect(appVersionFrom(JSON.stringify({ version: "0.4.2" }))).toBe("0.4.2");
  });

  it("fails the build when the version is missing or empty, never 'undefined'", () => {
    expect(() => appVersionFrom(JSON.stringify({ name: "x" }))).toThrow(/no "version"/);
    expect(() => appVersionFrom(JSON.stringify({ version: "" }))).toThrow(/no "version"/);
    expect(() => appVersionFrom(JSON.stringify({ version: 3 }))).toThrow(/no "version"/);
  });
});
