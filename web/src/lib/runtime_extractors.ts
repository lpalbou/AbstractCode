import { StepRecord, ToolCall, WaitState } from "./types";

export function extract_wait_from_record(rec: StepRecord | null | undefined): WaitState | null {
  const r: any = rec as any;
  const wait = r?.result?.wait;
  if (!wait || typeof wait !== "object") return null;
  return wait as WaitState;
}

export function extract_emit_event(rec: StepRecord | null | undefined): { name: string; payload: any; scope?: string } | null {
  const r: any = rec as any;
  const eff = r?.effect;
  if (!eff || typeof eff !== "object") return null;
  if (String(eff.type || "") !== "emit_event") return null;
  const name = String(eff?.payload?.name || eff?.payload?.event_name || "").trim();
  if (!name) return null;
  const payload = eff?.payload?.payload;
  const scope = typeof eff?.payload?.scope === "string" ? String(eff.payload.scope) : undefined;
  return { name, payload, scope };
}

export function extract_tool_calls_from_wait(wait: WaitState | null | undefined): ToolCall[] {
  const w: any = wait as any;
  const details = w?.details;
  if (!details || typeof details !== "object") return [];
  const tc = details?.tool_calls;
  return Array.isArray(tc) ? (tc as ToolCall[]) : [];
}

