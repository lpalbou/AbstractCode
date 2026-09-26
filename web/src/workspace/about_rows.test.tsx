import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AfAboutDialog, appIdentity, gatewayVersionRows } from "@abstractframework/ui-kit";
import { aboutExtraRows } from "./about_rows";

// Wrap the kit's formatter (behaviour unchanged) so the tests can prove the
// gateway rows come from it and not from a local copy.
vi.mock("@abstractframework/ui-kit", async (importOriginal) => {
  const kit = await importOriginal<typeof import("@abstractframework/ui-kit")>();
  return { ...kit, gatewayVersionRows: vi.fn(kit.gatewayVersionRows) };
});

const packageVersion = JSON.parse(
  readFileSync(new URL("../../package.json", import.meta.url), "utf8"),
).version;

const about = {
  abstractframework: "0.3.3",
  abstractgateway: "0.4.4",
  packages: {
    abstractgateway: "0.4.4",
    abstractruntime: "0.4.36",
    abstractcore: "2.15.3",
    abstractvoice: null,
  },
};

describe("About AbstractCode", () => {
  it("is built with the package.json version", () => {
    expect(__APP_VERSION__).toBe(packageVersion);
  });

  it("shows name + version, the framework, the author, the links and the gateway rows", () => {
    const html = renderToStaticMarkup(
      <AfAboutDialog
        open
        onClose={() => {}}
        identity={appIdentity("abstractcode", __APP_VERSION__)}
        extraRows={aboutExtraRows({ ok: true, value: about })}
      />,
    );
    expect(html).toContain(`AbstractCode ${packageVersion}`);
    expect(html).toContain("AbstractFramework — ");
    expect(html).toContain("Laurent-Philippe Albou, PhD (2023-2026)");
    for (const label of ["Website", "Source", "Documentation", "Report an issue", "Give feedback"])
      expect(html).toContain(label);
    for (const href of [
      "https://abstractframework.ai",
      "https://abstractframework.ai/code",
      "https://github.com/lpalbou/AbstractCode",
      "https://github.com/lpalbou/AbstractCode/tree/main/docs",
      "https://github.com/lpalbou/AbstractCode/issues",
    ])
      expect(html).toContain(`href="${href}"`);
    expect(html).toContain("AbstractGateway 0.4.4");
    expect(html).toContain("AbstractFramework 0.3.3");
    expect(html).toContain("Gateway package abstractruntime");
  });
});

describe("gateway version rows", () => {
  it("formats GET /api/gateway/about with the kit helper", () => {
    vi.mocked(gatewayVersionRows).mockClear();
    expect(aboutExtraRows({ ok: true, value: about })).toEqual([
      ["Gateway", "AbstractGateway 0.4.4"],
      ["Gateway framework", "AbstractFramework 0.3.3"],
      ["Gateway package abstractcore", "2.15.3"],
      ["Gateway package abstractruntime", "0.4.36"],
    ]);
    expect(vi.mocked(gatewayVersionRows)).toHaveBeenCalledWith(about);
  });

  it("says when the gateway host has no AbstractFramework distribution", () => {
    expect(
      aboutExtraRows({
        ok: true,
        value: { abstractframework: null, abstractgateway: "0.4.4", packages: {} },
      }),
    ).toEqual([
      ["Gateway", "AbstractGateway 0.4.4"],
      ["Gateway framework", "not installed on the gateway host"],
    ]);
  });

  it("shows a failed fetch as one Gateway row, never hides it", () => {
    vi.mocked(gatewayVersionRows).mockClear();
    expect(aboutExtraRows({ ok: false, status: 404, message: "Not Found" })).toEqual([
      ["Gateway", "unavailable (HTTP 404)"],
    ]);
    expect(vi.mocked(gatewayVersionRows)).toHaveBeenCalledWith(null, "HTTP 404");
    expect(aboutExtraRows({ ok: false, message: "network down" })).toEqual([
      ["Gateway", "unavailable (network down)"],
    ]);
  });

  it("reports a body without the gateway version as unavailable", () => {
    expect(aboutExtraRows({ ok: true, value: {} })).toEqual([
      ["Gateway", "unavailable (the gateway did not report its version)"],
    ]);
  });

  it("says it is checking while the request is in flight", () => {
    expect(aboutExtraRows(undefined)).toEqual([["Gateway", "checking…"]]);
  });
});
