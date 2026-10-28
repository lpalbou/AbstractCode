// The view path of the Activity rows against the ledger path, for a conformance case: the rows
// only one side has (as `kind | title | detail | status | statusLabel` per run), and the
// iteration groups ("Step N: <status>") when they differ. Used by activity_rows_view.test.ts.
import { replayToolActivities, type ReplayViewActivity } from "@abstractframework/panel-chat";
import { activity_rows, activity_rows_from_view, type ActivityEntry, type ActivityRow, type ActivityView } from "./activity_rows";
import { activity_groups } from "./activity_groups";

/** The case's ledger as the controller held it: arrival order (time, then run, then seq). */
export function ledger_entries(records: Record<string, Record<string, any>[]>): ActivityEntry[] {
  const runs = Object.keys(records);
  return runs
    .flatMap((runId) => records[runId].map((record) => ({ runId, record, cursor: Number(record.seq) })))
    .sort((x, y) => {
      const tx = String(x.record.ended_at || x.record.started_at || "");
      const ty = String(y.record.ended_at || y.record.started_at || "");
      return tx < ty ? -1 : tx > ty ? 1 : runs.indexOf(x.runId) - runs.indexOf(y.runId) || x.cursor - y.cursor;
    });
}

const line = (row: ActivityRow) => [row.runId.slice(0, 4), row.kind, row.title, row.detail, row.status, row.statusLabel].join(" | ");

function minus(a: string[], b: string[]): string[] {
  const rest = [...b];
  return a.filter((x) => {
    const i = rest.indexOf(x);
    if (i < 0) return true;
    rest.splice(i, 1);
    return false;
  });
}

export type ParityDiff = { ledgerOnly: string[]; viewOnly: string[]; groups?: { ledger: string; view: string } };

export function parity(view: ActivityView, records: Record<string, Record<string, any>[]>): ParityDiff {
  const fromLedger = activity_rows(ledger_entries(records), view.root_run_id);
  const fromView = activity_rows_from_view(view, (a) => replayToolActivities(a as unknown as ReplayViewActivity));
  const L = fromLedger.map(line);
  const V = fromView.map(line);
  const gl = activity_groups(fromLedger).map((g) => `${g.title}: ${g.statusLabel}`).join(", ");
  const gv = activity_groups(fromView).map((g) => `${g.title}: ${g.statusLabel}`).join(", ");
  return { ledgerOnly: minus(L, V), viewOnly: minus(V, L), ...(gl === gv ? {} : { groups: { ledger: gl, view: gv } }) };
}
