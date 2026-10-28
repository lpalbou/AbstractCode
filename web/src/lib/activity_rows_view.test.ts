// The Activity tab from the gateway's replay view (panel-chat 0.6.0, runtime backlog 0854).
// Fixtures: AbstractRuntime 0.15.0 conformance cases as AbstractGateway 0.17.0 serves them
// (`view.json` = the final `GET /view` body, captured by panel-chat's regen script) next to the
// same case's ledger records (`records.json`), so the view path is checked against the ledger path.
import { describe, expect, it } from "vitest";
import { replayToolActivities, type ReplayViewActivity } from "@abstractframework/panel-chat";
import { activity_rows, activity_rows_from_view, view_progress_records, type ActivityEntry, type ViewActivity } from "./activity_rows";
import { activity_groups } from "./activity_groups";
import { llm_phase_text } from "./llm_phase";
import { activityRowsOf } from "../workspace/workspace_panels";
import aView from "./fixtures/replay_view/a_delegated_child_approval/view.json";
import a2View from "./fixtures/replay_view/a2_delegated_approval_completed/view.json";
import a2Records from "./fixtures/replay_view/a2_delegated_approval_completed/records.json";
import mView from "./fixtures/replay_view/m_session_tool_policy/view.json";

type View = { root_run_id: string; activities: ViewActivity[] };
const tools = (activity: ViewActivity) => replayToolActivities(activity as unknown as ReplayViewActivity);

/** The case's ledger as the controller's records (arrival order: by time, then run, then seq). */
function ledgerEntries(records: Record<string, Record<string, any>[]>): ActivityEntry[] {
  const runs = Object.keys(records);
  return runs
    .flatMap((runId) => records[runId].map((record) => ({ runId, record, cursor: Number(record.seq) })))
    .sort((x, y) => {
      const tx = String(x.record.ended_at || x.record.started_at || "");
      const ty = String(y.record.ended_at || y.record.started_at || "");
      return tx < ty ? -1 : tx > ty ? 1 : runs.indexOf(x.runId) - runs.indexOf(y.runId) || x.cursor - y.cursor;
    });
}

describe("activity_rows_from_view", () => {
  it("one row per node execution, in the view's order", () => {
    const view = a2View as unknown as View;
    const rows = activity_rows_from_view(view, tools);
    expect(rows).toHaveLength(view.activities.length);
    expect(rows.map((row) => row.key)).toEqual(view.activities.map((a) => a.id));
    expect(rows.every((row) => row.entries.length === 1 && row.entries[0].record === view.activities.find((a) => a.id === row.key))).toBe(true);
  });

  it("describes model steps, tool batches and sub-flows as the ledger path does", () => {
    const view = a2View as unknown as View;
    const fromView = activity_rows_from_view(view, tools);
    const fromLedger = activity_rows(ledgerEntries(a2Records as never), view.root_run_id);
    const titles = (rows: typeof fromView, kind: string) => rows.filter((row) => row.kind === kind).map((row) => row.title).sort();
    for (const kind of ["llm", "tools", "subflow"]) {
      expect(titles(fromView, kind).length).toBeGreaterThan(0);
      expect(titles(fromView, kind)).toEqual(titles(fromLedger, kind));
    }
    // Same iterations ("Step N" groups) and every step finished.
    expect(activity_groups(fromView).map((g) => g.title)).toEqual(activity_groups(fromLedger).map((g) => g.title));
    expect(fromView.filter((row) => row.kind === "tools").every((row) => row.status === "completed")).toBe(true);
  });

  it("a delegated approval waits on the child, as approval needed", () => {
    const view = aView as unknown as View;
    const rows = activity_rows_from_view(view, tools);
    const waiting = rows.filter((row) => row.status === "waiting");
    expect(waiting.some((row) => row.kind === "tools" && row.statusLabel === "approval needed" && row.runId !== view.root_run_id)).toBe(true);
    const group = activity_groups(rows).find((g) => g.rows.some((row) => row.statusLabel === "approval needed"));
    expect(group?.statusLabel).toBe("Waiting for you");
  });

  it("tool rows carry the runtime's tool calls, a policy denial as failed with its provenance", () => {
    const rows = activity_rows_from_view(mView as unknown as View, tools);
    const calls = rows.flatMap((row) => (row.tools || []) as { name: string; status: string; policy?: { decision: string } }[]);
    expect(calls.find((c) => c.name === "fetch_url")).toMatchObject({ status: "completed", policy: { decision: "auto_approved" } });
    expect(calls.find((c) => c.name === "execute_command")).toMatchObject({ status: "failed", policy: { decision: "denied" } });
  });

  it("the workspace reads the view when the snapshot has one, the records otherwise", () => {
    const view = a2View as unknown as View;
    expect(activityRowsOf([], view.root_run_id, view as never).length).toBe(view.activities.length);
    expect(activityRowsOf([], view.root_run_id, null)).toEqual([]);
    const ledger = ledgerEntries(a2Records as never);
    expect(activityRowsOf(ledger as never, view.root_run_id, null).length).toBeGreaterThan(0);
  });

  it("the status bar's model phase comes from the view's progress", () => {
    const view: View = {
      root_run_id: "r",
      activities: [{ id: "r:s1", run_id: "r", kind: "llm", effect_type: "llm_call", status: "running", step_ids: ["s1"], progress: { kind: "llm", phase: "prefill", prompt_tokens: 12000, prefill_processed_tokens: 6000, step_id: "s1", run_id: "r" } }],
    };
    expect(llm_phase_text(view_progress_records(view))).not.toBe("");
    expect(llm_phase_text(view_progress_records({ activities: [] }))).toBe("");
  });
});
