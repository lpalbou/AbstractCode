import type { ToolSpec } from "./catalog";
import type { ToolPolicySelection } from "@abstractframework/ui-kit";

export type PermissionLevel = "default" | "read" | "write" | "all";
export type ToolPermissions = {
  enabledTools: string[];
  autoApproveTools: string[];
  requireApprovalTools: string[];
};

/** Same tier/pin ordering as the Rust TUI: enabled gate, explicit pin, tier.
 * Unknown classifications ask below All; no copied tool inventory. */
export function resolveToolPermissions(
  inventory: readonly ToolSpec[],
  selection: ToolPolicySelection,
  level: PermissionLevel = "default",
): ToolPermissions {
  const enabled = inventory.filter(tool => tool.enabled && !tool.servedDisabled &&
    (selection.mode === "all" || selection.selected.includes(tool.name)));
  const accepts = (tool: ToolSpec) => {
    const pin = selection.approval[tool.name];
    if (pin === "ask") return false;
    if (pin === "approve") return true;
    if (level === "all") return true;
    const defaultAuto = ["auto", "approve"].includes((tool.approval || "").trim().toLowerCase());
    if (level === "default") return defaultAuto;
    const approvalTier = defaultAuto ? 1 : ["write_file", "edit_file"].includes(tool.name) ? 2 : 3;
    const riskTier = tool.riskRank === undefined || tool.riskRank === 1 ? 1 : tool.riskRank === 2 ? 2 : 3;
    return Math.max(approvalTier, riskTier) <= (level === "write" ? 2 : 1);
  };
  return {
    enabledTools: enabled.map(tool => tool.name),
    autoApproveTools: enabled.filter(accepts).map(tool => tool.name),
    requireApprovalTools: enabled.filter(tool => !accepts(tool)).map(tool => tool.name),
  };
}

/** A workflow's authored/active allowlist is another ceiling, not an override. */
export function intersectToolPermissions(policy: ToolPermissions, allowed: unknown): ToolPermissions {
  if (!Array.isArray(allowed)) return policy;
  const keep = (name: string) => allowed.includes(name);
  return {
    enabledTools: policy.enabledTools.filter(keep),
    autoApproveTools: policy.autoApproveTools.filter(keep),
    requireApprovalTools: policy.requireApprovalTools.filter(keep),
  };
}
