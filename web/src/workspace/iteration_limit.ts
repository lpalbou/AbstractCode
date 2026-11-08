// The iteration limit's Code-side seams (kit IterationLimitField; gateway agents.max_iterations).
import type { IterationLimitApplied, IterationLimitDefault } from "@abstractframework/ui-kit";

/** Automations run on a schedule, where the gateway iteration default does not apply: the workflow decides. */
export const AUTOMATION_ITERATION_DEFAULT: IterationLimitDefault = { value: null, source: "default", warnAbove: 200 };

const SOURCES = new Set(["request", "gateway_default", "builtin"]);

/** The `limits` receipt of a POST /runs/start answer; null when the gateway sent none (older gateway) or junk. */
export function runLimitsReceipt(raw: unknown): IterationLimitApplied | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const r = raw as Record<string, unknown>;
  const source = typeof r.source === "string" && SOURCES.has(r.source) ? (r.source as IterationLimitApplied["source"]) : null;
  if (!source) return null;
  const n = r.max_iterations;
  const max = typeof n === "number" && Number.isInteger(n) && n >= 1 ? n : null;
  if (source !== "builtin" && max === null) return null;
  return { max_iterations: max, source };
}
