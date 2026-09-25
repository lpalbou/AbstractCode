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

const DEFAULT_SESSIONS_LIMIT = 500;
const defaultSessionsKey = (identity: string) =>
  `abstractcode.workspace.v2:${identity}:default-sessions`;

/** Conversations this browser started with the gateway default workflow. */
export function readDefaultSessions(identity: string): Set<string> {
  try {
    const saved = JSON.parse(localStorage.getItem(defaultSessionsKey(identity)) || "[]");
    return new Set(Array.isArray(saved) ? saved.filter((id) => typeof id === "string") : []);
  } catch {
    return new Set();
  }
}

/** Record (or clear) that a conversation runs on the gateway default. */
export function markDefaultSession(identity: string, sessionId: string, usesDefault: boolean): Set<string> {
  const sessions = readDefaultSessions(identity);
  sessions.delete(sessionId);
  if (usesDefault) sessions.add(sessionId);
  const kept = [...sessions].slice(-DEFAULT_SESSIONS_LIMIT);
  try {
    localStorage.setItem(defaultSessionsKey(identity), JSON.stringify(kept));
  } catch {
    /* The in-memory set still applies for this page. */
  }
  return new Set(kept);
}

export function writePreferences(identity: string, value: RunPreferences): void {
  try {
    localStorage.setItem(preferencesKey(identity), JSON.stringify(value));
  } catch {
    /* In-memory settings remain usable. */
  }
}
