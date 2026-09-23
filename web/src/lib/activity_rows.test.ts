import { describe, expect, it } from "vitest";
import { records } from "./activity_rows.fixtures";
import {
  activity_rows,
  attach_progress,
  describe_record,
  format_llm_progress,
  format_media_progress,
  group_key,
  merge_group,
  progress_payload,
  short_model,
  subflow_label,
  tool_names,
  type ActivityEntry,
} from "./activity_rows";
import { llm_phase_text } from "./llm_phase";

const PARENT = "39e7393f-8c25-4c89-95a9-57af127e98a7";
const CHILD = "081d8daa-d898-4a72-9f57-8ebea6ec1dd6";

/** A ledger stream: the fixture records in the order the runtime wrote them. */
function stream(items: [string, string][]): ActivityEntry[] {
  return items.map(([runId, name], index) => {
    const record = records[name];
    if (!record) throw new Error(`no fixture named ${name}`);
    return { cursor: index, runId, record };
  });
}

/** One turn of the operator's own run: parent follows child, child does work. */
function turn(): ActivityEntry[] {
  return stream([
    [PARENT, "subflow_started"],
    [PARENT, "subflow_waiting"],
    [CHILD, "llm_started"],
    [CHILD, "progress_prefill"],
    [CHILD, "progress_generate_first"],
    [CHILD, "progress_generate"],
    [CHILD, "progress_complete_first"],
    [CHILD, "llm_completed"],
    [CHILD, "tools_started"],
    [CHILD, "tools_waiting"],
    [CHILD, "tools_completed"],
    [CHILD, "resume_tool_approval"],
    [CHILD, "status_completed"],
    [CHILD, "run_finished"],
    [PARENT, "resume_subworkflow"],
    [PARENT, "parent_run_finished"],
  ]);
}

describe("describe_record", () => {
  it("names an llm call by provider, model and what it cost", () => {
    const started = describe_record(records.llm_started);
    expect(started.kind).toBe("llm");
    expect(started.title).toBe("llm · mlx · Qwen3.8-Flash-Next-oQ4e-mtp");
    expect(started.detail).toBe(
      "temperature 0.2 · thinking low · speculation native_mtp",
    );

    const done = describe_record(records.llm_completed);
    expect(done.title).toBe("llm · mlx · Qwen3.8-Flash-Next-oQ4e-mtp");
    expect(done.detail).toBe(
      "7,016 tokens in · cold cache · 205 out · 15.8 s · 46 tok/s · 3 tool calls",
    );
  });

  it("names tool calls and how they ended", () => {
    expect(describe_record(records.tools_started)).toMatchObject({
      kind: "tools",
      title: "tools · web_search ×3",
      detail: "query AI consciousness debate reddit 2026",
    });
    expect(describe_record(records.tools_completed).detail).toBe(
      "query AI consciousness debate reddit 2026 · 3 ok",
    );
  });

  it("abbreviates a url too long to read in a narrow panel", () => {
    // The url is one the agent actually fetched in this run.
    const described = describe_record({
      status: "completed",
      effect: {
        type: "tool_calls",
        payload: {
          tool_calls: [
            {
              name: "fetch_url",
              arguments: {
                url: "https://www.reddit.com/r/samharris/comments/1twnyyj/no_artificial_intelligence_is_not_conscious/",
              },
            },
          ],
        },
      },
      result: { results: [{ name: "fetch_url", success: true }] },
    });
    expect(described.detail).toBe(
      "url www.reddit.com · …/no_artificial_intelligence_is_not_conscious · 1 ok",
    );
  });

  it("reads a subflow as the work it starts, not as 'start subworkflow'", () => {
    expect(describe_record(records.subflow_started)).toMatchObject({
      kind: "subflow",
      title: "subflow · agent loop",
    });
    expect(describe_record(records.subflow_started).detail).toBe(
      "abstractassistant-default · no, i meant, what do people think or argue about online ? do a much deeper search",
    );
  });

  it("reads the one-variable status helper as a status push", () => {
    expect(describe_record(records.subflow_status_helper)).toMatchObject({
      title: "subflow · status",
      detail: "Thinking...",
    });
  });

  it("recovers a bundle name buried in a base64 catalog workflow id", () => {
    expect(describe_record(records.subflow_catalog_started).detail).toContain(
      "abstractassistant-orchestrator",
    );
  });

  it("names waits, asks and messages", () => {
    expect(describe_record(records.wait_until_started)).toMatchObject({
      kind: "wait",
      title: "wait · delay 3 s",
      detail: "until 23:54:33 · resumes at end",
    });
    expect(describe_record(records.wait_event_started)).toMatchObject({
      kind: "wait",
      title: "wait · event",
      detail: "Streaming voice synthesis is running.",
    });
    expect(describe_record(records.ask_user_started)).toMatchObject({
      kind: "ask",
      title: "ask · your input",
      detail: "The coding agent believes the task is done after 4 cycle(s).",
    });
    expect(describe_record(records.answer_user_completed)).toMatchObject({
      kind: "message",
      detail: "coding round 1 of 2: build",
    });
  });

  it("names the terminal record by its outcome, never by its node id", () => {
    expect(describe_record(records.run_finished)).toMatchObject({
      kind: "run",
      title: "run finished",
      detail: "final answer · 4 iterations",
    });
    // The parent run carries the subflow's answer and no stop_reason of its own.
    const parent = describe_record(records.parent_run_finished);
    expect(parent.title).toBe("run finished");
    expect(parent.detail.length).toBeGreaterThan(20);
  });

  it("hides progress, status and resume records", () => {
    for (const name of [
      "progress_prefill",
      "progress_generate",
      "progress_complete",
      "progress_complete_first",
    ]) {
      expect(describe_record(records[name])).toMatchObject({
        isProgress: true,
        hidden: true,
      });
    }
    expect(describe_record(records.status_completed)).toMatchObject({
      isStatus: true,
      hidden: true,
    });
    expect(describe_record(records.resume_tool_approval)).toMatchObject({
      isResume: true,
      hidden: true,
    });
  });

  it("still shows an ordinary emitted event, with name, scope and payload", () => {
    const described = describe_record({
      effect: {
        type: "emit_event",
        payload: {
          name: "fixture.ping",
          scope: "session",
          payload: { source: "abstractcode-e2e" },
        },
      },
      status: "completed",
    });
    expect(described.hidden).toBe(false);
    expect(described.title).toBe("event · fixture.ping (session)");
    expect(described.detail).toBe("source abstractcode-e2e");
  });

  it("never throws on a malformed or empty record", () => {
    for (const bad of [null, undefined, 42, "x", {}, { effect: 1 }, { effect: {} }]) {
      expect(() => describe_record(bad)).not.toThrow();
    }
    expect(describe_record({}).hidden).toBe(false);
  });
});

describe("helpers", () => {
  it("shortens a model id to its last path segment", () => {
    expect(short_model("Jundot/Qwen3.8-Flash-Next-oQ4e-mtp")).toBe(
      "Qwen3.8-Flash-Next-oQ4e-mtp",
    );
    expect(short_model("")).toBe("");
  });

  it("labels workflow ids in all three shapes the runtime emits", () => {
    expect(subflow_label("basic-agent@0.0.4:15f19f7f")).toEqual({
      name: "basic-agent",
      node: "15f19f7f",
    });
    expect(
      subflow_label(
        "visual_react_agent_abstractassistant-default_0_0_0_662ad0f5_node-2",
      ),
    ).toEqual({ name: "abstractassistant-default", node: "node-2" });
    expect(
      subflow_label(
        "visual_react_agent___catalog__v2__tenant_catalog__ZGVmYXVsdA__YWJzdHJhY3Rhc3Npc3RhbnQtb3JjaGVzdHJhdG9y_0_0_3_c53b1579_assistant_agent",
      ),
    ).toEqual({ name: "abstractassistant-orchestrator", node: "assistant_agent" });
    expect(subflow_label(undefined)).toEqual({ name: "", node: "" });
  });

  it("counts repeated tool names instead of repeating them", () => {
    expect(
      tool_names(records.tools_started.effect.payload.tool_calls),
    ).toBe("web_search ×3");
    expect(tool_names([{ name: "read_file" }, { name: "web_search" }])).toBe(
      "read_file, web_search",
    );
    expect(tool_names([])).toBe("");
  });
});

describe("collapsing", () => {
  it("gives started, waiting and completed of one step the same key", () => {
    const entries = stream([
      [CHILD, "tools_started"],
      [CHILD, "tools_waiting"],
      [CHILD, "tools_completed"],
    ]);
    const keys = new Set(entries.map(group_key));
    expect(keys.size).toBe(1);
    // Load-bearing: the completion is written under a FRESH step_id, so a
    // step_id-only key would split this step into two rows.
    expect(records.tools_completed.step_id).not.toBe(records.tools_started.step_id);
    expect(records.tools_completed.idempotency_key).toBe(
      records.tools_started.idempotency_key,
    );
  });

  it("keeps the concrete payload the ledger later replaced with a $slim pointer", () => {
    const merged = merge_group(
      stream([
        [PARENT, "subflow_started"],
        [PARENT, "subflow_waiting"],
      ]),
    );
    expect(merged.status).toBe("waiting");
    expect(records.subflow_waiting.effect.payload.vars.$slim).toBeTruthy();
    expect(merged.effect.payload.vars.$slim).toBeUndefined();
    expect(merged.effect.payload.vars.context.task).toContain("deeper search");
  });

  it("turns one whole turn into a handful of rows, not one row per record", () => {
    const entries = turn();
    const rows = activity_rows(entries);
    expect(entries.length).toBe(16);
    expect(rows.map((row) => `${row.title} — ${row.statusLabel}`)).toEqual([
      "subflow · agent loop — completed",
      "llm · mlx · Qwen3.8-Flash-Next-oQ4e-mtp — completed",
      "tools · web_search ×3 — completed",
      "run finished — completed",
      "run finished — completed",
    ]);
  });

  it("closes a waiting step with the resume record that names its wait_key", () => {
    const waiting = activity_rows(
      stream([
        [PARENT, "subflow_started"],
        [PARENT, "subflow_waiting"],
      ]),
    );
    expect(waiting[0].statusLabel).toBe("waiting for subflow");
    const resumed = activity_rows(
      stream([
        [PARENT, "subflow_started"],
        [PARENT, "subflow_waiting"],
        [PARENT, "resume_subworkflow"],
      ]),
    );
    expect(resumed).toHaveLength(1);
    expect(resumed[0].statusLabel).toBe("completed");
  });

  it("calls a tool wait an approval, because that is what it is", () => {
    const rows = activity_rows(
      stream([
        [CHILD, "tools_started"],
        [CHILD, "tools_waiting"],
      ]),
    );
    expect(rows[0].statusLabel).toBe("approval needed");
  });

  it("loses no record: every input lands on a row or in its progress", () => {
    const entries = turn();
    const rows = activity_rows(entries);
    const kept = new Set<number>();
    for (const row of rows) {
      for (const entry of row.entries) kept.add(entry.cursor);
    }
    const progress = entries.filter((entry) => progress_payload(entry.record));
    const status = entries.filter(
      (entry) =>
        (entry.record as any)?.effect?.payload?.name === "abstract.status",
    );
    const resume = entries.filter(
      (entry) => (entry.record as any)?.effect?.type === "resume",
    );
    expect(progress).toHaveLength(4);
    expect(status).toHaveLength(1);
    expect(resume).toHaveLength(2);
    expect(kept.size + progress.length + status.length + resume.length).toBe(
      entries.length,
    );
    const carried = rows.flatMap((row) => row.progressEvents);
    expect(carried).toHaveLength(progress.length);
  });
});

describe("progress is progress, not activity", () => {
  it("feeds the owning llm row a live line while the model is running", () => {
    const rows = activity_rows(
      stream([
        [CHILD, "llm_started"],
        [CHILD, "progress_prefill"],
        [CHILD, "progress_generate_first"],
        [CHILD, "progress_generate"],
      ]),
    );
    expect(rows).toHaveLength(1);
    expect(rows[0].statusLabel).toBe("running");
    expect(rows[0].progress).toBe(
      "prefill 7,016 tokens → generating 19 tokens · 35 tok/s",
    );
    expect(rows[0].progressEvents).toHaveLength(3);
  });

  it("shows cached prefill tokens when the provider reports them", () => {
    expect(
      format_llm_progress([
        { kind: "llm", phase: "prefill", prompt_tokens: 8407, cached_tokens: 6900 },
        {
          kind: "llm",
          phase: "generate",
          prompt_tokens: 8407,
          cached_tokens: 6900,
          generated_tokens: 89,
          tokens_per_second: 34,
        },
      ]),
    ).toBe("prefill 8,407 tokens (6,900 cached) → generating 89 tokens · 34 tok/s");
  });

  it("shows live prefill progress as processed / total while the prompt is processed", () => {
    const start = { kind: "llm", phase: "prefill", prompt_tokens: 8407, cached_tokens: 6900 };
    const mid = { ...start, phase: "prefill", prefill_processed_tokens: 7300 };
    expect(format_llm_progress([start, mid])).toBe("prefill 7,300 / 8,407 tokens (87%)");
    // An older event's position is never carried into a newer one without it.
    expect(format_llm_progress([mid, start])).toBe("prefill 8,407 tokens (6,900 cached)");
    // After the first token the prefill is over: the total form returns.
    expect(
      format_llm_progress([
        start,
        mid,
        { kind: "llm", phase: "generate", prompt_tokens: 8407, cached_tokens: 6900, generated_tokens: 1 },
      ]),
    ).toBe("prefill 8,407 tokens (6,900 cached) → generating 1 token");
  });

  it("summarises the call once the final progress event lands", () => {
    expect(
      format_llm_progress([
        {
          kind: "llm",
          phase: "complete",
          final: true,
          prompt_tokens: 5899,
          cached_tokens: 4096,
          generated_tokens: 312,
          elapsed_s: 9.8,
          tokens_per_second: 34,
        },
      ]),
    ).toBe("5,899 tokens in · 4,096 cached · 312 out · 9.8 s · 34 tok/s");
  });

  it("does not repeat itself under a completed row that already says it", () => {
    const rows = activity_rows(
      stream([
        [CHILD, "llm_started"],
        [CHILD, "progress_prefill"],
        [CHILD, "progress_complete_first"],
        [CHILD, "llm_completed"],
      ]),
    );
    expect(rows).toHaveLength(1);
    expect(rows[0].detail).toContain("7,016 tokens in");
    expect(rows[0].progress).toBe("");
    expect(rows[0].progressFinal).toBe(true);
    expect(rows[0].progressEvents).toHaveLength(2);
  });

  it("gives media progress the same treatment on the row that owns it", () => {
    const owner = {
      run_id: "r1",
      step_id: "s-owner",
      node_id: "render",
      status: "started",
      effect: { type: "tool_calls", payload: { tool_calls: [{ name: "generate_image" }] } },
      idempotency_key: "idem-1",
    };
    const tick = (step: number, percent: number) => ({
      run_id: "r1",
      step_id: `s-emit-${step}`,
      node_id: "render",
      status: "completed",
      effect: {
        type: "emit_event",
        payload: {
          name: "abstract.progress",
          scope: "run",
          payload: {
            kind: "image",
            phase: "denoise",
            run_id: "r1",
            step_id: "s-owner",
            node_id: "render",
            step,
            total: 50,
            percent,
          },
        },
      },
    });
    const rows = activity_rows([
      { cursor: 0, runId: "r1", record: owner },
      { cursor: 1, runId: "r1", record: tick(12, 24) },
      { cursor: 2, runId: "r1", record: tick(25, 50) },
    ]);
    expect(rows).toHaveLength(1);
    expect(rows[0].title).toBe("tools · generate_image");
    expect(rows[0].progress).toBe("denoise · step 25/50 · 50%");
  });

  it("keeps orphan progress visible as its own row rather than dropping it", () => {
    const rows = activity_rows([
      {
        cursor: 0,
        runId: CHILD,
        record: records.progress_generate,
      },
    ]);
    expect(rows).toHaveLength(1);
    expect(rows[0].kind).toBe("progress");
    expect(rows[0].progress).toContain("prefill 7,016 tokens");
  });

  it("formats whatever fields a media producer chose to report", () => {
    expect(format_media_progress([{ kind: "video", frame: 120, elapsed_s: 4 }])).toBe(
      "frame 120 · 4 s",
    );
    expect(format_media_progress([{ kind: "audio", percent: 0.42 }])).toBe("42%");
    expect(format_media_progress([])).toBe("");
  });

  it("still feeds the run strip: the same records drive llm_phase_text", () => {
    // Hiding progress rows must not starve the other consumer of these
    // records. The Activity tab and the run strip read the SAME stream; the
    // inspector filters its own rows, never the snapshot.
    const entries = stream([
      [CHILD, "llm_started"],
      [CHILD, "progress_prefill"],
      [CHILD, "progress_generate_first"],
      [CHILD, "progress_generate"],
    ]);
    expect(llm_phase_text(entries.map((entry) => entry.record))).toBe(
      "Generating · 19 tokens · 35 tok/s",
    );
    expect(activity_rows(entries)[0].progress).toContain("generating 19 tokens");
  });

  it("attach_progress leaves rows alone when there is no progress at all", () => {
    const rows = activity_rows(stream([[CHILD, "tools_completed"]]));
    const same = attach_progress(rows, []);
    expect(same).toHaveLength(1);
    expect(same[0].progress).toBe("");
  });
});
