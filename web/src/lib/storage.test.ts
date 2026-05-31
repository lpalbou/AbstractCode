import { beforeEach, describe, expect, it } from "vitest";

import { load_settings, save_settings, type Settings } from "./storage";

describe("settings storage", () => {
  beforeEach(() => {
    const data = new Map<string, string>();
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: {
        getItem: (key: string) => data.get(key) ?? null,
        setItem: (key: string, value: string) => void data.set(key, value),
        removeItem: (key: string) => void data.delete(key),
        clear: () => void data.clear(),
      },
    });
  });

  it("does not persist Gateway bearer tokens", () => {
    const settings: Settings = {
      ...load_settings(),
      gateway_url: "https://gateway.example",
      gateway_user: "alice",
      gateway_auth_mode: "session",
      gateway_remember: true,
      auth_token: "agw_secret",
    };

    save_settings(settings);

    const raw = String(localStorage.getItem("abstractcode.settings.v1") || "");
    expect(raw).not.toContain("agw_secret");
    expect(load_settings().auth_token).toBe("");
    expect(load_settings().gateway_user).toBe("alice");
    expect(load_settings().gateway_auth_mode).toBe("session");
  });
});
