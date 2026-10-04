import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AfAboutDialog, appIdentity, aboutVersionsFromGateway } from "@abstractframework/ui-kit";
import { aboutVersions } from "./about_rows";

// Wrap the kit's reader (behaviour unchanged) so the tests can prove the
// versions come from it and not from a local copy.
vi.mock("@abstractframework/ui-kit", async (importOriginal) => {
  const kit = await importOriginal<typeof import("@abstractframework/ui-kit")>();
  return { ...kit, aboutVersionsFromGateway: vi.fn(kit.aboutVersionsFromGateway) };
});
const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");

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

  it("R5: the shared kit About — app + framework + gateway versions, links, licence line, NO package list", () => {
    const html = renderToStaticMarkup(
      <AfAboutDialog
        open
        onClose={() => {}}
        identity={appIdentity("abstractcode", __APP_VERSION__)}
        versions={aboutVersions({ ok: true, value: about })}
      />,
    );
    expect(html).toContain(`AbstractCode`);
    expect(html).toContain(packageVersion);
    expect(html).toContain("0.4.4"); // gateway
    expect(html).toContain("0.3.3"); // framework
    for (const href of [
      "https://abstractframework.ai/code",
      "https://github.com/lpalbou/AbstractCode",
      "https://github.com/lpalbou/AbstractCode/issues",
    ])
      expect(html).toContain(`href="${href}"`);
    // No package list: the per-package versions of the payload never reach the screen.
    expect(html).not.toContain("abstractruntime");
    expect(html).not.toContain("0.4.36");
    expect(html).not.toContain("2.15.3");
  });
  it("app.tsx passes the kit About its versions (never extra rows)", () => {
    expect(appSource).toMatch(/about=\{\{ identity: APP_IDENTITY, versions: aboutVersions\(gatewayAbout\), onOpen: refreshGatewayAbout \}\}/);
    expect(appSource).not.toContain("extraRows");
  });
});

describe("gateway versions for About", () => {
  it("reads GET /api/gateway/about with the kit helper", () => {
    vi.mocked(aboutVersionsFromGateway).mockClear();
    expect(aboutVersions({ ok: true, value: about })).toEqual({ framework: "0.3.3", gateway: "0.4.4" });
    expect(vi.mocked(aboutVersionsFromGateway)).toHaveBeenCalledWith(about);
  });
  it("says when the gateway host has no AbstractFramework distribution", () => {
    expect(aboutVersions({ ok: true, value: { abstractframework: null, abstractgateway: "0.4.4", packages: {} } })).toMatchObject({
      gateway: "0.4.4",
      frameworkNote: "not installed on the gateway host",
    });
  });
  it("shows a failed fetch in place of the gateway version, never hides it", () => {
    expect(aboutVersions({ ok: false, status: 404, message: "Not Found" }).gatewayNote).toBe("unavailable (HTTP 404)");
    expect(aboutVersions({ ok: false, message: "network down" }).gatewayNote).toBe("unavailable (network down)");
  });
  it("says it is checking while the request is in flight", () => {
    expect(aboutVersions(undefined)).toMatchObject({ gatewayNote: "checking…", frameworkNote: "checking…" });
  });
});
