import { useCallback, useEffect, useRef, useState } from "react";
import { gatewayApiPath, type TimeZonePreference } from "@abstractframework/ui-kit";
import type { WorkflowDefinition } from "./catalog";
import { readPreferences } from "./preferences";
import { gatewayRequest } from "./transport";
import { CODE_AGENT_INTERFACE, GATEWAY_DEFAULT } from "./workflow_selection";

/** The account's default workflow per app lives on the GATEWAY (round 14, R14.2):
 * `GET/PUT /api/gateway/accounts/me/preferences`, `default_workflow` {<interface>: null |
 * "bundle:flow" | "catalog:bundle:flow"} where null = the gateway's per-app default. Shared with
 * the Assistant, the console (Accounts → Preferences) and every browser. A gateway without the
 * route (older than 0.13.1) answers 404: the choice then stays in this browser, as before. */
export const ACCOUNT_PREFERENCES_PATH = gatewayApiPath("accounts/me/preferences");

export type AccountWorkflowRow = {
  interface: string;
  value: string | null;
  state: string;
  reason: string | null;
  gatewayDefaultLabel: string;
  choices: Array<{ value: string; label: string; workflowId: string }>;
};

export type AccountWorkflowState =
  | { status: "loading" }
  | { status: "unsupported" }
  | { status: "error"; message: string }
  | { status: "ok"; row: AccountWorkflowRow; timeZone: TimeZonePreference };

/** The `time_zone` block of the answer (round 16, R16.1 A2), checked: a missing block or field is
 * said, never guessed — the IANA list is the gateway's, never the browser's. */
export function accountTimeZone(answer: unknown): TimeZonePreference {
  const block = record(record(answer)?.time_zone);
  if (!block || !Array.isArray(block.choices) || typeof block.gateway_default !== "string" || typeof block.effective !== "string")
    throw new Error("The gateway's account preferences answer has no time_zone block (choices, gateway_default, effective).");
  return {
    value: typeof block.value === "string" && block.value ? block.value : null,
    gateway_default: block.gateway_default,
    effective: block.effective,
    label: typeof block.label === "string" ? block.label : "Time zone",
    help: typeof block.help === "string" ? block.help : "",
    choices: (block.choices as unknown[]).map(String),
  };
}

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

/** The app's row of the gateway answer, checked; a missing row or field is said, never guessed. */
export function accountWorkflowRow(answer: unknown, interfaceId: string = CODE_AGENT_INTERFACE): AccountWorkflowRow {
  const body = record(answer);
  if (!body || !Array.isArray(body.apps) || !record(record(body.preferences)?.default_workflow))
    throw new Error("The gateway's account preferences answer has no apps or default_workflow.");
  const row = (body.apps as unknown[]).map(record).find((r) => r?.interface === interfaceId);
  if (!row) throw new Error(`The gateway's account preferences have no row for ${interfaceId}.`);
  if (typeof row.gateway_default_label !== "string" || !Array.isArray(row.choices))
    throw new Error(`The gateway's account preferences row for ${interfaceId} has no gateway_default_label or choices.`);
  return {
    interface: interfaceId,
    value: typeof row.value === "string" && row.value.trim() ? row.value : null,
    state: typeof row.state === "string" ? row.state : "default",
    reason: typeof row.reason === "string" && row.reason ? row.reason : null,
    gatewayDefaultLabel: row.gateway_default_label,
    choices: (row.choices as unknown[]).map(record).filter(Boolean).map((c) => ({
      value: String(c!.value),
      label: String(c!.label ?? c!.name ?? c!.value),
      workflowId: String(c!.workflow_id ?? ""),
    })),
  };
}

/** The account value of a picker selection (`<scope>:bundle@version:flow`, or "@default"):
 * version-less, so it follows new versions; the gateway's own registry has no prefix. */
export function accountValueFromSelection(selection: string): string | null {
  const text = String(selection || "").trim();
  if (!text || text === GATEWAY_DEFAULT) return null;
  const scope = text.startsWith("tenant_catalog:") ? "catalog" : text.startsWith("private:") ? "private" : "";
  const rest = scope ? text.slice(text.indexOf(":") + 1) : text;
  const colon = rest.indexOf(":");
  if (colon <= 0) return null;
  const bundle = rest.slice(0, colon).split("@")[0];
  const flow = rest.slice(colon + 1);
  if (!bundle || !flow) return null;
  return scope === "catalog" ? `catalog:${bundle}:${flow}` : `${bundle}:${flow}`;
}

/** The picker selection an account value stands for: the listed workflow with that bundle and
 * flow (its latest version), else the gateway default (the gateway says why in `reason`). */
export function selectionFromAccountValue(value: string | null, workflows: readonly WorkflowDefinition[]): string {
  if (!value) return GATEWAY_DEFAULT;
  const catalog = value.startsWith("catalog:");
  const rest = catalog ? value.slice("catalog:".length) : value;
  const colon = rest.indexOf(":");
  if (colon <= 0) return GATEWAY_DEFAULT;
  const bundle = rest.slice(0, colon);
  const flow = rest.slice(colon + 1);
  const match = workflows.find(
    (w) =>
      w.bundleId === bundle &&
      w.flowId === flow &&
      ((w.registryScope ?? "private") === "private") !== catalog,
  );
  return match ? match.id : GATEWAY_DEFAULT;
}

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

function status(reason: unknown): number {
  const r = reason as { status?: unknown } | null;
  return typeof r?.status === "number" ? r.status : 0;
}

export type GatewayCall = (path: string, init?: RequestInit) => Promise<unknown>;

/** Read the account's default workflow and run the ONE-TIME migration of a choice saved in
 * this browser before the gateway kept it (`device`, a picker selection): uploaded when the
 * account has none, then `onDeviceCleared` removes it from this browser's preferences (an
 * account choice made elsewhere wins). A refusal also clears it; a network failure keeps it
 * for the next load. A 404 = a gateway older than 0.13.1 ("unsupported"). */
export async function loadAccountWorkflow(
  request: GatewayCall,
  device: string,
  onDeviceCleared: () => void,
): Promise<AccountWorkflowState> {
  let row: AccountWorkflowRow;
  let timeZone: TimeZonePreference;
  try {
    const answer = await request(ACCOUNT_PREFERENCES_PATH);
    row = accountWorkflowRow(answer);
    timeZone = accountTimeZone(answer);
  } catch (reason) {
    return status(reason) === 404 ? { status: "unsupported" } : { status: "error", message: errorText(reason) };
  }
  if (device && device !== GATEWAY_DEFAULT) {
    let clear = true;
    if (row.value === null) {
      const value = accountValueFromSelection(device);
      if (value) {
        try {
          const answer = await request(ACCOUNT_PREFERENCES_PATH, {
            method: "PUT",
            body: JSON.stringify({ default_workflow: { [CODE_AGENT_INTERFACE]: value } }),
          });
          row = accountWorkflowRow(answer);
          timeZone = accountTimeZone(answer);
        } catch (reason) {
          clear = status(reason) === 400; // refused: the old choice no longer runs
        }
      }
    }
    if (clear) onDeviceCleared();
  }
  return { status: "ok", row, timeZone };
}

/** Read / write the account's default workflow for AbstractCode (loadAccountWorkflow runs the
 * one-time migration of this browser's old choice). */
export function useAccountWorkflow(identity: string, onDeviceCleared: () => void) {
  const [state, setState] = useState<AccountWorkflowState>({ status: "loading" });
  const cleared = useRef(onDeviceCleared);
  cleared.current = onDeviceCleared;
  useEffect(() => {
    let alive = true;
    setState({ status: "loading" });
    if (!identity) return;
    void loadAccountWorkflow(gatewayRequest, readPreferences(identity).workflow, () => {
      if (alive) cleared.current();
    }).then((next) => {
      if (alive) setState(next);
    });
    return () => {
      alive = false;
    };
  }, [identity]);
  /** One PUT; resolves with the new row, rejects with the gateway's sentence. */
  const put = useCallback(async (changes: Record<string, unknown>) => {
    const answer = await gatewayRequest(ACCOUNT_PREFERENCES_PATH, { method: "PUT", body: JSON.stringify(changes) });
    const row = accountWorkflowRow(answer);
    const timeZone = accountTimeZone(answer);
    setState({ status: "ok", row, timeZone });
    return { row, timeZone };
  }, []);
  const save = useCallback(async (value: string | null) => (await put({ default_workflow: { [CODE_AGENT_INTERFACE]: value } })).row, [put]);
  /** The account's time zone (null = the gateway default): one PUT, at once (round 16). */
  const saveTimeZone = useCallback(async (value: string | null) => (await put({ time_zone: value })).timeZone, [put]);
  return { state, save, saveTimeZone };
}
