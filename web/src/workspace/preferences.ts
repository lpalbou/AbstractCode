import { normalizeSpeculationValue } from "@abstractframework/ui-kit";
import { DEFAULT_PREFERENCES, type RunPreferences } from "./settings_panel";

export function preferencesKey(identity: string): string {
  return `abstractcode.workspace.v2:${identity}`;
}

/** Parse the saved run preferences. Anything unreadable falls back to the
 * defaults (a fresh browser uses the gateway default workflow). */
export function parsePreferences(raw: string | null): RunPreferences {
  try {
    const saved = JSON.parse(raw || "{}");
    const workflow =
      typeof saved.workflow === "string" && saved.workflow.trim()
        ? saved.workflow.trim()
        : DEFAULT_PREFERENCES.workflow;
    return {
      ...DEFAULT_PREFERENCES,
      ...saved,
      workflow,
      showAllWorkflows: saved.showAllWorkflows === true,
      speculation: normalizeSpeculationValue(saved.speculation),
      tools: { ...DEFAULT_PREFERENCES.tools, ...saved.tools },
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
  try {
    localStorage.setItem(preferencesKey(identity), JSON.stringify(value));
  } catch {
    /* In-memory settings remain usable. */
  }
}
