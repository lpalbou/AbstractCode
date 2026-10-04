// An automation's run settings ARE its definition (round 4): the Settings
// panel edits `definition.target.input_data` in place and every change is
// saved as a new revision through the gateway (`PATCH /automations/{id}`
// with `expected_revision`). These two pure functions map the definition's
// input to the panel's `RunPreferences` and back, using the SAME keys a
// conversation turn writes (catalog.ts `assignCommonRuntimeInputs`):
//   provider/model        input.provider/model + _runtime.provider/model
//   reasoning             _runtime.thinking
//   MTP                   _runtime.speculation
//   iteration/token limit _limits.max_iterations / _limits.max_tokens
//   instructions          _runtime.system_prompt_extra
//   tools                 input.tools + _runtime.allowed_tools (kit withAutomationTools)
//   approvals             _runtime.tool_policy {auto_approve_tools, require_approval_tools}
//   workspace folders     workspace_allowed_paths (the chosen set, within the
//                         account's folders; absent = follows the account;
//                         the retired workspace_access_mode is removed on save)
//   skills                input.skills
// An unset value is REMOVED (never written as ""), so the gateway default
// applies again — "Gateway default" until overridden.
import { automationToolSelection, withAutomationTools, type AutomationChanges, type AutomationDefinition, type ToolPolicySelection } from "@abstractframework/ui-kit";
import { DEFAULT_PREFERENCES, type RunPreferences } from "./settings_panel";

type Json = Record<string, any>;

const obj = (v: unknown): Json => (v && typeof v === "object" && !Array.isArray(v) ? { ...(v as Json) } : {});
const str = (v: unknown): string => (typeof v === "string" ? v : typeof v === "number" ? String(v) : "");
const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string" && x.trim() !== "") : []);

/** The panel's preferences for an automation definition's input. */
export function automationRunPreferences(input: Json | undefined): RunPreferences {
  const data = obj(input);
  const runtime = obj(data._runtime);
  const limits = obj(data._limits);
  const provider = str(data.provider) || str(runtime.provider);
  const model = str(data.model) || str(runtime.model);
  const selected = automationToolSelection(data);
  const policy = obj(runtime.tool_policy);
  const approval: ToolPolicySelection["approval"] = {};
  for (const name of strings(policy.auto_approve_tools)) approval[name] = "approve";
  for (const name of strings(policy.require_approval_tools)) approval[name] = "ask";
  const customized = selected !== null || Object.keys(approval).length > 0;
  return {
    ...DEFAULT_PREFERENCES,
    provider: provider && model ? provider : "",
    model: provider && model ? model : "",
    reasoning: str(runtime.thinking),
    ...(runtime.speculation !== undefined ? { speculation: runtime.speculation } : {}),
    maxIterations: str(limits.max_iterations),
    maxTokens: str(limits.max_tokens),
    system: str(runtime.system_prompt_extra),
    workspaceFolders: Array.isArray(data.workspace_allowed_paths) ? strings(data.workspace_allowed_paths) : null,
    tools: selected === null ? { mode: "all", selected: [], approval } : { mode: "custom", selected, approval },
    toolsCustomized: customized,
    permissions: "default",
    skills: strings(data.skills),
    workflow: DEFAULT_PREFERENCES.workflow,
    streamReplies: "gateway_default",
  };
}

function setOrDelete(target: Json, key: string, value: unknown): void {
  if (value === undefined || value === null || value === "" || (Array.isArray(value) && !value.length)) delete target[key];
  else target[key] = value;
}

function positiveInt(value: string): number | undefined {
  const n = Number(value);
  return Number.isSafeInteger(n) && n > 0 ? n : undefined;
}

/** The definition's input with the panel's preferences applied (everything else kept). */
export function withAutomationRunPreferences(input: Json | undefined, prefs: RunPreferences): Json {
  let next = obj(input);
  const runtime = obj(next._runtime);
  const limits = obj(next._limits);
  const both = Boolean(prefs.provider && prefs.model);
  setOrDelete(next, "provider", both ? prefs.provider : "");
  setOrDelete(next, "model", both ? prefs.model : "");
  setOrDelete(runtime, "provider", both ? prefs.provider : "");
  setOrDelete(runtime, "model", both ? prefs.model : "");
  setOrDelete(runtime, "thinking", prefs.reasoning);
  setOrDelete(runtime, "speculation", prefs.speculation);
  setOrDelete(runtime, "system_prompt_extra", prefs.system.trim());
  setOrDelete(limits, "max_iterations", positiveInt(prefs.maxIterations));
  setOrDelete(limits, "max_tokens", positiveInt(prefs.maxTokens));
  const approve = Object.entries(prefs.tools.approval || {}).filter(([, v]) => v === "approve").map(([k]) => k);
  const ask = Object.entries(prefs.tools.approval || {}).filter(([, v]) => v === "ask").map(([k]) => k);
  const policy = prefs.toolsCustomized && (approve.length || ask.length)
    ? { ...(approve.length ? { auto_approve_tools: approve } : {}), ...(ask.length ? { require_approval_tools: ask } : {}) }
    : undefined;
  setOrDelete(runtime, "tool_policy", policy);
  if (Object.keys(runtime).length) next._runtime = runtime;
  else delete next._runtime;
  if (Object.keys(limits).length) next._limits = limits;
  else delete next._limits;
  // Round 9: the access mode is gone (the gateway always scopes to the
  // effective folders); the chosen set is stored as is, [] = shared only.
  delete next.workspace_access_mode;
  if (prefs.workspaceFolders === null) delete next.workspace_allowed_paths;
  else next.workspace_allowed_paths = [...prefs.workspaceFolders];
  setOrDelete(next, "skills", prefs.skills);
  next = withAutomationTools(next as any, prefs.toolsCustomized && prefs.tools.mode === "custom" ? prefs.tools.selected : null) as Json;
  return next;
}

/** The PATCH `changes` for new preferences, or null when the input would not change. */
export function automationSettingsChanges(definition: Pick<AutomationDefinition, "target">, prefs: RunPreferences): AutomationChanges | null {
  const before = obj(definition.target.input_data);
  const after = withAutomationRunPreferences(before, prefs);
  if (stableJson(before) === stableJson(after)) return null;
  return { target: { bundle_ref: definition.target.bundle_ref, flow_id: definition.target.flow_id, input_data: after as any } };
}

function stableJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
  if (value && typeof value === "object") {
    const o = value as Json;
    return `{${Object.keys(o).sort().map((k) => `${JSON.stringify(k)}:${stableJson(o[k])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}
