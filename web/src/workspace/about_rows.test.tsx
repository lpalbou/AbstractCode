import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AfAboutDialog, appIdentity } from "@abstractframework/ui-kit";
import { aboutExtraRows, gatewayPackageVersions } from "./about_rows";

const packageVersion = JSON.parse(
  readFileSync(new URL("../../package.json", import.meta.url), "utf8"),
).version;

const capabilities = {
  capabilities: {
    abstractgateway: { installed: true, version: "0.4.4" },
    abstractruntime: { installed: true, version: "0.4.36" },
    abstractvoice: { installed: false, error: "No module named 'abstractvoice'" },
    visualflow: { installed: true },
    contracts: {},
  },
};

describe("About AbstractCode", () => {
  it("is built with the package.json version", () => {
    expect(__APP_VERSION__).toBe(packageVersion);
  });

  it("shows name + version, the framework, the author and the five links", () => {
    const html = renderToStaticMarkup(
      <AfAboutDialog
        open
        onClose={() => {}}
        identity={appIdentity("abstractcode", __APP_VERSION__)}
        extraRows={aboutExtraRows({
          capabilities: { ok: true, value: capabilities },
          about: { ok: true, value: { abstractframework: "0.3.3" } },
        })}
      />,
    );
    expect(html).toContain(`AbstractCode ${packageVersion}`);
    expect(html).toContain("AbstractFramework — https://abstractframework.ai");
    expect(html).toContain("Laurent-Philippe Albou, PhD (2023-2026)");
    for (const label of ["Website", "Source", "Documentation", "Report an issue", "Give feedback"])
      expect(html).toContain(label);
    for (const href of [
      "https://abstractframework.ai/code",
      "https://github.com/lpalbou/AbstractCode",
      "https://github.com/lpalbou/AbstractCode/tree/main/docs",
      "https://github.com/lpalbou/AbstractCode/issues",
    ])
      expect(html).toContain(`href="${href}"`);
    expect(html).toContain("AbstractFramework on the gateway");
    expect(html).toContain("0.3.3");
    expect(html).toContain("abstractgateway");
    expect(html).toContain("0.4.36");
  });
});

describe("gateway version rows", () => {
  it("lists installed Abstract packages from the capabilities already fetched", () => {
    expect(gatewayPackageVersions(capabilities)).toEqual([
      ["abstractgateway", "0.4.4"],
      ["abstractruntime", "0.4.36"],
    ]);
  });

  it("shows a failed fetch as one Gateway row, never hides it", () => {
    expect(
      aboutExtraRows({
        capabilities: { ok: true, value: capabilities },
        about: { ok: false, status: 404, message: "Not Found" },
      }),
    ).toEqual([
      ["abstractgateway", "0.4.4"],
      ["abstractruntime", "0.4.36"],
      ["Gateway", "unavailable (HTTP 404)"],
    ]);
    expect(
      aboutExtraRows({
        capabilities: { ok: false, status: 401, message: "sign in" },
        about: { ok: false, status: 401, message: "sign in" },
      }),
    ).toEqual([["Gateway", "unavailable (HTTP 401)"]]);
    expect(
      aboutExtraRows({ capabilities: { ok: false, message: "network down" } }),
    ).toEqual([["Gateway", "unavailable (network down)"]]);
  });

  it("says when the gateway host has no AbstractFramework distribution", () => {
    expect(
      aboutExtraRows({ about: { ok: true, value: { abstractframework: null } } }),
    ).toEqual([["AbstractFramework on the gateway", "not installed"]]);
  });
});
