// Activity in foldable groups (round 4): one group per agent iteration — a
// model step and everything it caused (tool calls, approvals, sub-flows)
// until the next model step. Rows before the first model step (the run
// starting, inputs) form a "Start" group. Pure: the rows come from
// `activity_rows`, the order is theirs (first arrival).
import type { ActivityRow } from "./activity_rows";

export type ActivityGroup = {
  key: string;
  /** "Start", "Step 1", "Step 2", … */
  title: string;
  /** The model step's own line (its title), "" for the Start group. */
  detail: string;
  /** running | waiting | failed | completed (the most urgent of its rows). */
  status: string;
  rows: ActivityRow[];
  /** Tool calls in the group (sum over its `tools` rows' entries). */
  toolRows: number;
};

const URGENCY = ["running", "waiting", "failed", "completed"];

function groupStatus(rows: ActivityRow[]): string {
  for (const status of URGENCY) if (rows.some((row) => row.status === status)) return status;
  return rows.length ? rows[rows.length - 1].status : "completed";
}

export function activity_groups(rows: readonly ActivityRow[]): ActivityGroup[] {
  const groups: ActivityGroup[] = [];
  let current: ActivityGroup | null = null;
  let step = 0;
  for (const row of rows) {
    if (row.kind === "llm" || !current) {
      const isStep = row.kind === "llm";
      if (isStep) step += 1;
      current = {
        key: `${isStep ? "step" : "start"}:${row.key}`,
        title: isStep ? `Step ${step}` : "Start",
        detail: isStep ? row.title : "",
        status: "completed",
        rows: [],
        toolRows: 0,
      };
      groups.push(current);
    }
    current.rows.push(row);
    if (row.kind === "tools") current.toolRows += 1;
  }
  for (const group of groups) group.status = groupStatus(group.rows);
  return groups;
}

/** The group shown open by default: the newest one. */
export function default_open_group(groups: readonly ActivityGroup[]): string {
  return groups.length ? groups[groups.length - 1].key : "";
}
