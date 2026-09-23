import { describe, expect, it } from "vitest";
import { capabilityContracts } from "./use_workspace_catalog";

describe("gateway client capability envelope", () => {
  it("unwraps the canonical discovery capabilities/contracts envelope", () => {
    const contracts = { assistant: { voice: { tts: { available: true } } } };
    expect(capabilityContracts({ capabilities: { contracts } })).toBe(
      contracts,
    );
    expect(capabilityContracts({ contracts })).toBe(contracts);
    expect(capabilityContracts(contracts)).toBe(contracts);
  });
  it("never infers capabilities from a missing or malformed response", () => {
    expect(capabilityContracts(null)).toEqual({});
    expect(capabilityContracts([])).toEqual({});
  });
});
