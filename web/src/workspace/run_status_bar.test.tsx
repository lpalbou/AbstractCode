import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { WorkflowSessionController, type WorkflowSessionSnapshot } from "@abstractframework/panel-chat";

import { activity_rows } from "../lib/activity_rows";
import { RunStatusBar } from "./run_status_bar";

// Live model progress (backlog: 0.10.1 operator report "Thinking · Generating the
// next response" with no prefill/decode numbers). The agent loop of the shipped
// Basic agent runs in a CHILD run (root → start_subworkflow → child llm_call), so
// the numbers arrive as `abstract.progress` records on the CHILD's ledger stream.
// This drives the real panel-chat controller with a fake gateway stream and
// renders the real strip and Activity rows from its snapshot.

const ROOT = "root-run";
const CHILD = "child-run";
const CALL = "call-1";
const at = "2026-10-02T08:50:00+00:00";

function progress(cursor: number, payload: Record<string, unknown>) {
  const full = { run_id: CHILD, step_id: CALL, kind: "llm", provider: "mlx", model: "fake-27b", prompt_tokens: 17110, ...payload };
  return {
    cursor,
    record: {
      run_id: CHILD, step_id: `p-${cursor}`, node_id: "reason", status: "completed", started_at: at, ended_at: at,
      effect: { type: "emit_event", payload: { name: "abstract.progress", scope: "run", payload: full } },
      result: { emitted: true, name: "abstract.progress", payload: full },
    },
  };
}

const rootRecords = [
  { cursor: 1, record: { run_id: ROOT, step_id: "s1", node_id: "agent", status: "started", started_at: at, effect: { type: "start_subworkflow", payload: { workflow_id: "basic-agent_node-2" } } } },
  { cursor: 2, record: { run_id: ROOT, step_id: "s1", node_id: "agent", status: "waiting", started_at: at, effect: { type: "start_subworkflow", payload: { workflow_id: "basic-agent_node-2" } }, result: { sub_run_id: CHILD, wait: { reason: "subworkflow", wait_key: `subworkflow:${CHILD}`, details: { sub_run_id: CHILD } } } } },
];
const childStart = { cursor: 1, record: { run_id: CHILD, step_id: CALL, node_id: "reason", status: "started", started_at: at, effect: { type: "llm_call", payload: { provider: "mlx", model: "fake-27b" } } } };

async function session() {
  let push: ((item: unknown) => void) | null = null;
  const transport: any = {
    getRun: async (id: string) => (id === ROOT ? { run_id: ROOT, status: "waiting", waiting: { reason: "subworkflow", wait_key: `subworkflow:${CHILD}`, details: { sub_run_id: CHILD } } } : { run_id: CHILD, status: "running" }),
    getHistory: async () => ({}),
    getLedger: async (id: string, after: number) => {
      const items = (id === ROOT ? rootRecords : [childStart]).filter((item) => item.cursor > after);
      return { items, next_after: after + items.length };
    },
    streamLedger: (id: string, _after: number, onStep: (item: unknown) => void, signal: AbortSignal) =>
      new Promise<void>((resolve) => {
        if (id === CHILD) push = onStep;
        signal.addEventListener("abort", () => resolve());
      }),
    submitCommand: async () => ({}),
  };
  const controller = new WorkflowSessionController(transport);
  void controller.load(ROOT);
  for (let i = 0; i < 50 && !push; i++) await new Promise((r) => setTimeout(r, 5));
  if (!push) throw new Error("the controller never followed the child run's ledger stream");
  return { controller, push: push as (item: unknown) => void };
}

function strip(snapshot: WorkflowSessionSnapshot): string {
  return renderToStaticMarkup(
    <RunStatusBar active paused={false} snapshot={snapshot} onRevokeApproval={() => {}} onCommand={() => {}} onActivity={() => {}} />,
  );
}

function llmRow(snapshot: WorkflowSessionSnapshot) {
  const rows = activity_rows(snapshot.records.map((entry) => ({ cursor: entry.cursor, record: entry.record, runId: entry.runId })), ROOT);
  const row = rows.find((r) => r.kind === "llm");
  if (!row) throw new Error("no llm row");
  return row;
}

describe("live model progress in the run strip and on the Activity llm row", () => {
  it("prefill shows tokens done / total (%), decode shows tokens and tok/s, from a child run's stream", async () => {
    const { controller, push } = await session();
    push(progress(2, { phase: "prefill", event_index: 0, generated_tokens: 0 }));
    push(progress(3, { phase: "prefill", event_index: 1, generated_tokens: 0, prefill_processed_tokens: 2560 }));
    let html = strip(controller.getSnapshot());
    expect(html).toContain("Thinking");
    expect(html).toContain("Prefill · 2,560 / 17,110 tokens (15%)");
    expect(html).not.toContain("Generating the next response");
    // Phones hide the static detail, never a live phase.
    expect(html).toContain('class="code-run-strip__detail code-run-strip__detail--live"');
    expect(llmRow(controller.getSnapshot()).progress).toContain("2,560 / 17,110");

    push(progress(4, { phase: "generate", event_index: 2, generated_tokens: 46, tokens_per_second: 38.2 }));
    html = strip(controller.getSnapshot());
    expect(html).toContain("Generating · 46 tokens · 38 tok/s");
    expect(llmRow(controller.getSnapshot()).progress).toMatch(/46 tokens.*38 tok\/s/);
    controller.dispose();
  });

  it("a child run's terminal record reads as the subflow finishing, never as the run being done", () => {
    const rows = activity_rows([
      ...rootRecords.map((item) => ({ ...item, runId: ROOT })),
      { cursor: 9, runId: CHILD, record: { run_id: CHILD, step_id: "end", node_id: "end", status: "completed", started_at: at, ended_at: at, result: { completed: true, output: { state: "Thinking…", success: true } } } },
    ], ROOT);
    const finished = rows.find((r) => r.kind === "run");
    expect(finished?.title).toBe("subflow finished");
    const own = activity_rows([{ cursor: 1, runId: ROOT, record: { run_id: ROOT, step_id: "end", status: "completed", result: { completed: true, output: { answer: "done" } } } }], ROOT);
    expect(own.find((r) => r.kind === "run")?.title).toBe("run finished");
  });

  it("the phone layouts hide only the static detail, never the live phase", () => {
    const css = readFileSync(new URL("./workspace.css", import.meta.url), "utf8");
    expect(css).not.toMatch(/\.code-run-strip__detail\s*\{\s*display:\s*none/);
    expect(css.match(/\.code-run-strip__detail:not\(\.code-run-strip__detail--live\)\s*\{\s*display:\s*none/g)?.length).toBe(2);
  });
});
