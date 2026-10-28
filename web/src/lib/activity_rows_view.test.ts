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
import { readdirSync, readFileSync } from "node:fs";
import { parity } from "./activity_rows_parity";
import { EXPECTED_DIFFERENCES, REASONS } from "./activity_rows_parity_expected";
import aView from "./fixtures/replay_view/a_delegated_child_approval/view.json";
import a2View from "./fixtures/replay_view/a2_delegated_approval_completed/view.json";
import a2Records from "./fixtures/replay_view/a2_delegated_approval_completed/records.json";
import mView from "./fixtures/replay_view/m_session_tool_policy/view.json";

type View = { root_run_id: string; activities: ViewActivity[] };
const FIXTURES = `${__dirname}/fixtures/replay_view`;
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
  it("one row per node execution in the view's order, plus one per completed run", () => {
    const view = a2View as unknown as View & { runs: { run_id: string; status: string }[] };
    const rows = activity_rows_from_view(view, tools);
    const finished = rows.filter((row) => row.kind === "run");
    expect(finished.map((row) => row.runId).sort()).toEqual(view.runs.filter((r) => r.status === "completed").map((r) => r.run_id).sort());
    expect(finished.find((row) => row.runId === view.root_run_id)?.title).toBe("run finished");
    expect(rows.at(-1)?.title).toBe("run finished");
    const steps = rows.filter((row) => row.kind !== "run");
    expect(steps.map((row) => row.key)).toEqual(view.activities.map((a) => a.id));
    expect(steps.every((row) => row.entries.length === 1 && row.entries[0].record === view.activities.find((a) => a.id === row.key))).toBe(true);
  });

  it("matches the ledger path on every conformance case, except the differences listed with their reason", () => {
    const cases = readdirSync(FIXTURES, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name).sort();
    expect(cases).toHaveLength(15);
    expect(Object.keys(EXPECTED_DIFFERENCES).sort()).toEqual(cases);
    for (const name of cases) {
      const view = JSON.parse(readFileSync(`${FIXTURES}/${name}/view.json`, "utf8"));
      const records = JSON.parse(readFileSync(`${FIXTURES}/${name}/records.json`, "utf8"));
      const got = parity(view, records);
      const want = EXPECTED_DIFFERENCES[name];
      expect({ case: name, ledgerOnly: got.ledgerOnly, viewOnly: got.viewOnly, groups: got.groups }).toEqual({
        case: name,
        ledgerOnly: want.ledgerOnly.map(([row]) => row),
        viewOnly: want.viewOnly.map(([row]) => row),
        groups: want.groups ? { ledger: want.groups.ledger, view: want.groups.view } : undefined,
      });
      for (const [, why] of [...want.ledgerOnly, ...want.viewOnly]) expect(REASONS[why]).toBeTruthy();
    }
  });

  it("ask, message and event-wait rows carry the view's own text, never the effect type", () => {
    const rows = (name: string) => activity_rows_from_view(JSON.parse(readFileSync(`${FIXTURES}/${name}/view.json`, "utf8")), tools);
    const texts = [...rows("g_ask_user"), ...rows("j_event_answered_then_pause"), ...rows("i_wait_kinds"), ...rows("l_answer_then_completion")]
      .filter((row) => ["ask", "message", "wait"].includes(row.kind))
      .map((row) => row.detail);
    expect(texts.length).toBeGreaterThan(6);
    for (const detail of texts) expect(detail).not.toMatch(/^(ask_user|answer_user|wait_event)$/);
    expect(rows("i_wait_kinds").some((row) => row.detail === "Which colour should the chart use?")).toBe(true);
    expect(rows("l_answer_then_completion").map((row) => row.title)).toContain("run finished");
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
    expect(activityRowsOf([], view.root_run_id, view as never)).toEqual(activity_rows_from_view(view as never, tools));
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
