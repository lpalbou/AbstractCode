import { describe, expect, it } from "vitest";
import type { ToolPolicySelection } from "@abstractframework/ui-kit";
import type { ToolSpec } from "./catalog";
import {
  intersectToolPermissions,
  resolveToolPermissions,
  type PermissionLevel,
} from "./tool_permissions";

const tool = (name: string, extra: Partial<ToolSpec> = {}): ToolSpec => ({
  name,
  description: `${name} fixture`,
  enabled: true,
  servedDisabled: false,
  raw: {},
  ...extra,
});
const selection = (extra: Partial<ToolPolicySelection> = {}): ToolPolicySelection => ({
  mode: "all",
  selected: [],
  approval: {},
  ...extra,
});
const names = (values: string[]) => [...values].sort();
const inventory = [
  tool("read_file", { approval: "auto", riskRank: 1 }),
  tool("write_file", { approval: "ask", riskRank: 2 }),
  tool("edit_file", { approval: "ask" }),
  tool("execute_command", { approval: "ask", riskRank: 2 }),
  tool("send_email", { approval: "auto", riskRank: 3 }),
  tool("unknown_tool"),
];

describe("resolveToolPermissions independent safety contracts", () => {
  it("All means all selected and enabled tools, not unchecked or unavailable tools", () => {
    const rows = [
      ...inventory,
      tool("operator_disabled", { enabled: false }),
      tool("served_disabled", { servedDisabled: true }),
    ];
    const state = selection({
      mode: "custom",
      selected: ["read_file", "execute_command", "operator_disabled", "served_disabled", "missing_from_inventory"],
      approval: { write_file: "approve", operator_disabled: "approve", served_disabled: "approve", missing_from_inventory: "approve" },
    });
    const policy = resolveToolPermissions(rows, state, "all");
    expect(names(policy.enabledTools)).toEqual(["execute_command", "read_file"]);
    expect(names(policy.autoApproveTools)).toEqual(["execute_command", "read_file"]);
    expect(policy.requireApprovalTools).toEqual([]);
  });

  it.each<PermissionLevel>(["default", "read", "write", "all"])("an explicit empty selection denies all under %s", level => {
    expect(resolveToolPermissions(inventory, selection({ mode: "custom", selected: [], approval: { execute_command: "approve" } }), level)).toEqual({
      enabledTools: [], autoApproveTools: [], requireApprovalTools: [],
    });
    expect(resolveToolPermissions([], selection({ approval: { execute_command: "approve" } }), level)).toEqual({
      enabledTools: [], autoApproveTools: [], requireApprovalTools: [],
    });
  });

  it("the Read/Write tier combines approval dial and risk band as a floor like the TUI", () => {
    const read = resolveToolPermissions(inventory, selection(), "read");
    expect(read.autoApproveTools).toEqual(["read_file"]);
    const write = resolveToolPermissions(inventory, selection(), "write");
    expect(names(write.autoApproveTools)).toEqual(["edit_file", "read_file", "write_file"]);
    expect(names(write.requireApprovalTools)).toEqual(["execute_command", "send_email", "unknown_tool"]);
    const all = resolveToolPermissions(inventory, selection(), "all");
    expect(names(all.autoApproveTools)).toEqual(names(inventory.map(row => row.name)));
  });

  it("the Gateway-default posture honors served defaults without treating them as explicit user pins", () => {
    const policy = resolveToolPermissions(inventory, selection(), "default");
    expect(names(policy.autoApproveTools)).toEqual(["read_file", "send_email"]);
    expect(names(policy.requireApprovalTools)).toEqual(["edit_file", "execute_command", "unknown_tool", "write_file"]);
    const tightened = resolveToolPermissions(inventory, selection(), "read");
    expect(tightened.autoApproveTools).not.toContain("send_email");
  });

  it("an explicit Ask pin wins over All, including a tool normally auto-approved", () => {
    const policy = resolveToolPermissions(inventory, selection({ approval: { read_file: "ask", execute_command: "ask" } }), "all");
    expect(names(policy.requireApprovalTools)).toEqual(["execute_command", "read_file"]);
    expect(policy.autoApproveTools).not.toContain("read_file");
    expect(policy.autoApproveTools).not.toContain("execute_command");
    expect(policy.enabledTools).toContain("execute_command");
  });

  it("an explicit Approve pin may bypass the tier but never the selected/served-enabled gate", () => {
    const rows = [...inventory, tool("blocked", { enabled: false, approval: "auto" })];
    const policy = resolveToolPermissions(rows, selection({
      mode: "custom", selected: ["execute_command", "blocked"],
      approval: { execute_command: "approve", blocked: "approve", read_file: "approve" },
    }), "read");
    expect(policy).toEqual({ enabledTools: ["execute_command"], autoApproveTools: ["execute_command"], requireApprovalTools: [] });
  });

  it("unknown classifications and unknown pin spellings do not lower the permission requirement", () => {
    const policy = resolveToolPermissions([
      tool("unknown_tool"), tool("bad_dial", { approval: "surprise" }),
    ], selection({ approval: { unknown_tool: "unexpected" as "ask" } }), "write");
    expect(policy.autoApproveTools).toEqual([]);
    expect(names(policy.requireApprovalTools)).toEqual(["bad_dial", "unknown_tool"]);
  });

  it("risk bands prevent a served auto dial from lowering act/outreach/destroy tools", () => {
    const rows = [1, 2, 3, 4, 9].map(rank => tool(`rank-${rank}`, { approval: "auto", riskRank: rank }));
    expect(resolveToolPermissions(rows, selection(), "read").autoApproveTools).toEqual(["rank-1"]);
    expect(resolveToolPermissions(rows, selection(), "write").autoApproveTools).toEqual(["rank-1", "rank-2"]);
    expect(resolveToolPermissions(rows, selection(), "all").autoApproveTools).toHaveLength(5);
  });

  it("zero is an unknown served risk rank, not an absent risk band", () => {
    // Rust server_tier matches Some(1), Some(2), Some(_) => All, None => Read.
    const rows = [tool("zero_rank", { approval: "auto", riskRank: 0 })];
    expect(resolveToolPermissions(rows, selection(), "read").autoApproveTools).toEqual([]);
    expect(resolveToolPermissions(rows, selection(), "write").autoApproveTools).toEqual([]);
    expect(resolveToolPermissions(rows, selection(), "all").autoApproveTools).toEqual(["zero_rank"]);
  });

  it("normalizes served approval text the same way as the TUI", () => {
    const rows = [tool("read_one", { approval: " AUTO ", riskRank: 1 }), tool("read_two", { approval: "Approve", riskRank: 1 })];
    expect(names(resolveToolPermissions(rows, selection(), "read").autoApproveTools)).toEqual(["read_one", "read_two"]);
  });

  it("leaves caller-owned inventory, selection and explicit pin objects untouched", () => {
    const rows = Object.freeze([Object.freeze(tool("read_file", { approval: "auto", riskRank: 1 }))]);
    const chosen = Object.freeze({ mode: "custom" as const, selected: Object.freeze(["read_file"]), approval: Object.freeze({ read_file: "ask" as const }) });
    const before = JSON.stringify({ rows, chosen });
    const output = resolveToolPermissions(rows, chosen as unknown as ToolPolicySelection, "all");
    output.enabledTools.push("not-a-real-tool");
    output.requireApprovalTools.length = 0;
    expect(JSON.stringify({ rows, chosen })).toBe(before);
  });
});

describe("intersectToolPermissions", () => {
  it("the authored/active workflow allowlist is a ceiling, never a union", () => {
    const policy = resolveToolPermissions(inventory, selection({ approval: { write_file: "ask" } }), "all");
    const narrowed = intersectToolPermissions(policy, ["read_file", "write_file", "not-in-inventory"]);
    expect(narrowed).toEqual({
      enabledTools: ["read_file", "write_file"],
      autoApproveTools: ["read_file"],
      requireApprovalTools: ["write_file"],
    });
    expect(intersectToolPermissions(narrowed, ["execute_command", "read_file"])).toEqual({
      enabledTools: ["read_file"], autoApproveTools: ["read_file"], requireApprovalTools: [],
    });
  });

  it("an explicit empty workflow allowlist denies all, unlike absent metadata", () => {
    const policy = resolveToolPermissions(inventory, selection(), "all");
    expect(intersectToolPermissions(policy, [])).toEqual({ enabledTools: [], autoApproveTools: [], requireApprovalTools: [] });
    expect(intersectToolPermissions(policy, undefined)).toEqual(policy);
    expect(intersectToolPermissions(policy, null)).toEqual(policy);
  });

  it("ignores malformed entries in an array and never grants names absent from policy", () => {
    const policy = resolveToolPermissions(inventory, selection(), "all");
    expect(intersectToolPermissions(policy, [null, 5, {}, "missing", "read_file", "read_file"]).enabledTools).toEqual(["read_file"]);
  });

  it("does not mutate the original permission lists or workflow allowlist", () => {
    const policy = {
      enabledTools: ["read_file", "write_file"], autoApproveTools: ["read_file"], requireApprovalTools: ["write_file"],
    };
    const allowed = Object.freeze(["read_file"]);
    const before = structuredClone(policy);
    const result = intersectToolPermissions(policy, allowed);
    result.enabledTools.push("another");
    result.autoApproveTools.length = 0;
    expect(policy).toEqual(before);
    expect(allowed).toEqual(["read_file"]);
  });
});
