import { LedgerStreamEvent, ToolCall, WaitState } from "./types";
import { extract_tool_calls_from_wait, extract_wait_from_record } from "./runtime_extractors";

export function resolve_blocking_wait(args: {
  root_run_id: string | null | undefined;
  root_wait: WaitState | null | undefined;
  records: LedgerStreamEvent[];
}): { wait: WaitState | null; wait_run_id: string; tool_calls: ToolCall[] } {
  const root_run_id = String(args?.root_run_id || "").trim();
  const root_wait = args?.root_wait || null;
  const records = Array.isArray(args?.records) ? args.records : [];

  if (!root_run_id) return { wait: null, wait_run_id: "", tool_calls: [] };
  if (!root_wait) return { wait: null, wait_run_id: root_run_id, tool_calls: [] };

  const reason = String(root_wait.reason || "").trim();
  if (reason !== "subworkflow") {
    return { wait: root_wait, wait_run_id: root_run_id, tool_calls: extract_tool_calls_from_wait(root_wait) };
  }

  const wk = String(root_wait.wait_key || "").trim();
  const sub_from_details = String((root_wait as any)?.details?.sub_run_id || "").trim();
  const sub_from_key = wk.startsWith("subworkflow:") ? String(wk.split(":", 2)[1] || "").trim() : "";
  const sub_run_id = sub_from_details || sub_from_key;
  if (!sub_run_id) return { wait: root_wait, wait_run_id: root_run_id, tool_calls: extract_tool_calls_from_wait(root_wait) };

  // Prefer the newest WAITING record in the subrun (if present).
  for (let i = records.length - 1; i >= 0; i--) {
    const ev = records[i];
    const rec: any = ev?.record as any;
    const rid = String(rec?.run_id || "").trim();
    if (rid !== sub_run_id) continue;
    const st = String(rec?.status || "").trim();
    if (st !== "waiting") continue;
    const w = extract_wait_from_record(rec);
    if (!w) continue;
    return { wait: w, wait_run_id: sub_run_id, tool_calls: extract_tool_calls_from_wait(w) };
  }

  return { wait: root_wait, wait_run_id: root_run_id, tool_calls: extract_tool_calls_from_wait(root_wait) };
}

