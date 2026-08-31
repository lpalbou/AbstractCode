import { describe, it, expect } from "vitest";
import { compute_settings_on_gateway_connect } from "./settings_defaults";
import type { Settings } from "./storage";

// Minimal Settings stub — the helper only touches the fields below.
function base(overrides: Partial<Settings> = {}): Settings {
  return {
    tools: [],
    tools_initialized: false,
    gateway_was_connected: false,
    client_id: "",
    provider: "",
    model: "",
  } as unknown as Settings;
}

describe("compute_settings_on_gateway_connect — tool enablement (gateway c4573)", () => {
  it("seeds only ENABLED tools on first connect; disabled rows are discoverable but not granted", () => {
    const { next } = compute_settings_on_gateway_connect({
      current: base(),
      discovered_providers: [],
      discovered_tools: [
        { name: "read_file" },                    // no field -> enabled
        { name: "write_file", enabled: true },
        { name: "send_email", enabled: false },   // env-gated toolset, off
        { name: "send_telegram_message", enabled: false },
      ],
    });
    expect(next.tools).toEqual(["read_file", "write_file"]);
    expect(next.tools_initialized).toBe(true);
  });

  it("prunes a tool that became disabled (toolset turned off) from an existing allowlist", () => {
    const { next, changed } = compute_settings_on_gateway_connect({
      current: base({ tools: ["read_file", "send_email"], tools_initialized: true } as any),
      discovered_providers: [],
      discovered_tools: [
        { name: "read_file", enabled: true },
        { name: "send_email", enabled: false },   // operator disabled the comms toolset
      ],
    });
    expect(next.tools).toEqual(["read_file"]);
    expect(changed).toBe(true);
  });

  it("older gateway without the enabled field keeps every tool (backward-compatible)", () => {
    const { next } = compute_settings_on_gateway_connect({
      current: base(),
      discovered_providers: [],
      discovered_tools: [{ name: "read_file" }, { name: "write_file" }, { name: "execute_command" }],
    });
    expect(next.tools).toEqual(["execute_command", "read_file", "write_file"]);
  });
});
