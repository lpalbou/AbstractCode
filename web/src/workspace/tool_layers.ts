// Code's tools are the kit's layers (backlog 0033, ADR 0001; decision 2026-10-27): the gateway
// offers and defaults each tool (GET discovery/tools), this app's layer holds the person's
// Off / Ask / Auto choices (ui-kit createToolLayerStore, "Saved in this browser"), and a run gets
// planRunTools/withRunTools: Auto tools in `_runtime.tool_policy.auto_approve_tools` (they run
// without a client open), Ask tools in `require_approval_tools`, Off tools outside
// `_runtime.allowed_tools`. With no choice at all nothing is sent and the gateway's grant applies.
//
// The Permissions tier (default / read / write / all) and the per-tool picker of earlier versions
// are migrated ONCE into the app layer (migrateLegacyTools) and the Tools panel says so
// (TOOLS_MIGRATION_NOTICE) until dismissed.
import {
  bulkAutoReaches,
  effectiveToolLayers,
  hasToolChoices,
  planRunTools,
  resolveToolModes,
  withRunTools,
  type ToolCatalogItem,
  type ToolMode,
  type ToolModeLayer,
} from "@abstractframework/ui-kit";

/** What earlier versions stored in the run preferences (`abstractcode.workspace.v2:<identity>`). */
export type LegacyToolPreferences = {
  tools?: { mode?: string; selected?: unknown; approval?: Record<string, unknown> };
  toolsCustomized?: boolean;
  permissions?: string;
};

export const TOOLS_MIGRATION_NOTICE = "Your Permissions setting became per-tool defaults on this device — review them";

/** Where the one-time migration records itself: "pending" (notice shown) or "dismissed". */
export function toolsMigrationKey(identity: string): string {
  return `abstractcode.tools.migrated.v1:${identity}`;
}

/** The legacy fields of a stored preferences object (absent ones are not legacy choices). */
export function legacyToolPreferences(saved: unknown): LegacyToolPreferences | null {
  const o = saved && typeof saved === "object" ? (saved as Record<string, unknown>) : {};
  const permissions = typeof o.permissions === "string" ? o.permissions : "default";
  const toolsCustomized = o.toolsCustomized === true;
  if (!toolsCustomized && (permissions === "default" || !permissions)) return null;
  return {
    tools: o.tools && typeof o.tools === "object" ? (o.tools as LegacyToolPreferences["tools"]) : undefined,
    toolsCustomized,
    permissions,
  };
}

/** The tier rule of the Permissions select (read: rank 1 or default-auto; write: rank ≤ 2, plus
 * write_file / edit_file), restricted to what the kit's bulk rule reaches (never a tool whose
 * destination the model chooses). */
function tierAuto(item: ToolCatalogItem, tier: string): boolean {
  if (!bulkAutoReaches(item)) return false;
  const rank = item.riskRank ?? 4;
  if (tier === "read") return item.gatewayDefault === "auto" || rank <= 1;
  if (tier === "write") return rank <= 2 || ["write_file", "edit_file"].includes(item.name);
  return false;
}

/**
 * The one-time migration of a legacy tool setting into the app layer (sparse: entries equal to the
 * gateway's default dropped):
 * - a custom selection: unpicked tools Off;
 * - explicit pins map exactly ("approve" → Auto, "ask" → Ask);
 * - "all": every offered tool the kit's bulk rule reaches → Auto (risk ≤ 2, never a model-chosen
 *   destination);
 * - "read" / "write": Auto for the tools those tiers covered, within the same rule.
 */
export function migrateLegacyTools(legacy: LegacyToolPreferences, catalog: readonly ToolCatalogItem[]): ToolModeLayer {
  const out: Record<string, ToolMode> = {};
  const custom = legacy.toolsCustomized === true && legacy.tools?.mode === "custom";
  const picked = new Set(Array.isArray(legacy.tools?.selected) ? (legacy.tools!.selected as unknown[]).filter((x): x is string => typeof x === "string") : []);
  const pins = legacy.toolsCustomized === true ? legacy.tools?.approval || {} : {};
  const tier = String(legacy.permissions || "default");
  for (const item of catalog) {
    if (!item.offered) continue;
    if (custom && !picked.has(item.name)) {
      out[item.name] = "off";
      continue;
    }
    const pin = pins[item.name];
    if (pin === "approve") out[item.name] = "auto";
    else if (pin === "ask") out[item.name] = "ask";
    else if (tier === "all" ? bulkAutoReaches(item) : tierAuto(item, tier)) out[item.name] = "auto";
  }
  return effectiveToolLayers(catalog, { app: out }).app;
}

/** "Allow all enabled tools" (the approval card): every enabled tool the bulk rule reaches → Auto. */
export function withAllEnabledAuto(catalog: readonly ToolCatalogItem[], layer: ToolModeLayer): ToolModeLayer {
  const modes = resolveToolModes(catalog, { app: layer });
  const out: Record<string, ToolMode> = { ...layer };
  for (const item of catalog) if (bulkAutoReaches(item) && modes[item.name].mode === "ask") out[item.name] = "auto";
  return effectiveToolLayers(catalog, { app: out }).app;
}

/** Every enabled tool the bulk rule reaches already runs on Auto (the run strip's "all" line). */
export function allEnabledAuto(catalog: readonly ToolCatalogItem[], layer: ToolModeLayer): boolean {
  const modes = resolveToolModes(catalog, { app: layer });
  const reach = catalog.filter((item) => bulkAutoReaches(item) && modes[item.name].mode !== "off");
  return reach.length > 0 && reach.some((item) => item.gatewayDefault !== "auto") && reach.every((item) => modes[item.name].mode === "auto");
}

/** Revoke: the layer's Auto choices go back to the gateway's default (Off and Ask choices stay). */
export function withoutAutoChoices(catalog: readonly ToolCatalogItem[], layer: ToolModeLayer): ToolModeLayer {
  const out: Record<string, ToolMode> = {};
  for (const [name, mode] of Object.entries(layer)) if (mode !== "auto") out[name] = mode;
  return effectiveToolLayers(catalog, { app: out }).app;
}

type Json = Record<string, any>;
const obj = (v: unknown): Json => (v && typeof v === "object" && !Array.isArray(v) ? (v as Json) : {});
const names = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string" && x.trim() !== "") : []);

/** A run input's tools as a layer: tools outside `_runtime.allowed_tools` (else `tools`) Off, the
 * `tool_policy` lists Auto / Ask; reduced to the choices that differ from the gateway's defaults. */
export function inputToolLayer(input: Json | undefined, catalog: readonly ToolCatalogItem[]): ToolModeLayer {
  const data = obj(input);
  const runtime = obj(data._runtime);
  const ceiling = Array.isArray(runtime.allowed_tools) ? names(runtime.allowed_tools) : Array.isArray(data.tools) ? names(data.tools) : null;
  const policy = obj(runtime.tool_policy);
  const out: Record<string, ToolMode> = {};
  for (const item of catalog) if (ceiling && !ceiling.includes(item.name)) out[item.name] = "off";
  for (const name of names(policy.require_approval_tools)) if (out[name] !== "off") out[name] = "ask";
  for (const name of names(policy.auto_approve_tools)) if (out[name] !== "off") out[name] = "auto";
  return effectiveToolLayers(catalog, { app: out }).app;
}

/** A run input with `layer` applied (planRunTools + withRunTools); no choice = the tool keys removed
 * (`tools`, `_runtime.allowed_tools`, `_runtime.tool_policy`) so the gateway's grant applies. */
export function withToolLayer(input: Json | undefined, layer: ToolModeLayer, catalog: readonly ToolCatalogItem[], opts: { ceiling?: readonly string[] | null } = {}): Json {
  const data: Json = { ...obj(input) };
  if (hasToolChoices(catalog, { app: layer })) return withRunTools(data, planRunTools(catalog, { app: layer }, opts));
  const runtime: Json = { ...obj(data._runtime) };
  delete data.tools;
  delete runtime.allowed_tools;
  delete runtime.tool_policy;
  if (Object.keys(runtime).length) data._runtime = runtime;
  else delete data._runtime;
  return data;
}
