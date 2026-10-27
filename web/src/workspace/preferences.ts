import { normalizeSpeculationValue } from "@abstractframework/ui-kit";
import { DEFAULT_PREFERENCES, type RunPreferences } from "./settings_panel";
import { normalizeStreamReplies } from "./stream_replies";
import { legacyToolPreferences } from "./tool_layers";

export function preferencesKey(identity: string): string {
  return `abstractcode.workspace.v2:${identity}`;
}

/** Parse the saved run preferences. Anything unreadable falls back to the
 * defaults (a fresh browser uses the gateway default workflow). */
export function parsePreferences(raw: string | null): RunPreferences {
  try {
    // "showAllWorkflows" (the removed toolbar switch) is dropped: the gateway decides what the picker lists.
    // The pre-0033 tool fields (Permissions tier, tool picker) are kept aside as `legacyTools` until
    // their one-time migration into the kit's tool layer (tool_layers.ts); the layer itself is not
    // stored here (the kit's tool layer store keeps it).
    const { showAllWorkflows: _removed, tools, toolsCustomized, permissions, toolLayer: _layer, legacyTools: kept, ...saved } = JSON.parse(raw || "{}");
    const legacy = legacyToolPreferences({ tools, toolsCustomized, permissions }) ?? (kept && typeof kept === "object" ? kept : null);
    const workflow =
      typeof saved.workflow === "string" && saved.workflow.trim()
        ? saved.workflow.trim()
        : DEFAULT_PREFERENCES.workflow;
    return {
      ...DEFAULT_PREFERENCES,
      ...saved,
      workflow,
      speculation: normalizeSpeculationValue(saved.speculation),
      streamReplies: normalizeStreamReplies(saved.streamReplies),
      toolLayer: {},
      ...(legacy ? { legacyTools: legacy } : {}),
    };
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

export function readPreferences(identity: string): RunPreferences {
  try {
    return parsePreferences(localStorage.getItem(preferencesKey(identity)));
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

export function writePreferences(identity: string, value: RunPreferences): void {
  // Round 14: "workflow" is written only while it holds a choice of this browser (a gateway
  // older than 0.13.1); the gateway default is the absence of the key, so the one-time move of
  // the choice to the account (account_preferences.ts) removes it from this browser.
  const { workflow, toolLayer: _layer, ...rest } = value;
  const stored = workflow && workflow !== "@default" ? { ...rest, workflow } : rest;
  try {
    localStorage.setItem(preferencesKey(identity), JSON.stringify(stored));
  } catch {
    /* In-memory settings remain usable. */
  }
}
