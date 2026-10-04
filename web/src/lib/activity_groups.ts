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
  /** The status in words. "Waiting for you" only when a row waits on a person
   * (a question or an approval); a timer or a sub-flow says what it waits for. */
  statusLabel: string;
  rows: ActivityRow[];
  /** Tool calls in the group (sum over its `tools` rows' entries). */
  toolRows: number;
};

const URGENCY = ["running", "waiting", "failed", "completed"];

const GROUP_STATUS_LABEL: Record<string, string> = {
  running: "Running",
  failed: "Failed",
  completed: "Done",
};
const ON_A_PERSON = new Set(["waiting for you", "approval needed"]);

function groupStatusLabel(status: string, rows: ActivityRow[]): string {
  if (status !== "waiting") return GROUP_STATUS_LABEL[status] || status;
  const waiting = rows.filter((row) => row.status === "waiting");
  if (waiting.some((row) => ON_A_PERSON.has(row.statusLabel))) return "Waiting for you";
  const label = waiting[waiting.length - 1]?.statusLabel || "waiting";
  return label.charAt(0).toUpperCase() + label.slice(1);
}

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
        statusLabel: "Done",
        rows: [],
        toolRows: 0,
      };
      groups.push(current);
    }
    current.rows.push(row);
    if (row.kind === "tools") current.toolRows += 1;
  }
  for (const group of groups) {
    group.status = groupStatus(group.rows);
    group.statusLabel = groupStatusLabel(group.status, group.rows);
  }
  return groups;
}

/** The group shown open by default: the newest one. */
export function default_open_group(groups: readonly ActivityGroup[]): string {
  return groups.length ? groups[groups.length - 1].key : "";
}
