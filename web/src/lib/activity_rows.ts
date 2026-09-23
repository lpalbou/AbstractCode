/**
 * Activity rows: ledger records → lines a human can read.
 *
 * The workspace inspector used to render one row per ledger record with the
 * effect type as its label ("emit event", "llm call", "start subworkflow"),
 * which produced three rows per step (started / waiting / completed) and,
 * worse, one row per `abstract.progress` record — twelve "emit event" rows for
 * a single model call. Progress records are not activity: they are the live
 * numbers behind a step that is already on screen.
 *
 * This module is pure so it can be unit-tested against real ledger records:
 *
 *   describe_record(record)   one record  → { kind, title, detail, ... }
 *   activity_rows(entries)    the stream  → collapsed, described, progress-fed
 *   attach_progress(rows, e)  folds `abstract.progress` into the owning row
 *
 * Collapsing: the started / waiting / completed records of ONE step share a
 * run + effect type + idempotency key (a `tool_calls` completion is written
 * under a fresh step_id, so step_id alone is not enough), and they describe the
 * same thing at three moments. They become one row whose status advances. The
 * timeline is not lost — every raw record of the group stays on the row and is
 * rendered on expand, in arrival order, with its cursor.
 *
 * Resumes: a `wait` is closed by a separate `resume` record, not by a
 * `completed` record on the waiting step. A resume is therefore not a row of
 * its own; it closes the group whose wait_key it carries.
 */

import { toolPreview } from "@abstractframework/panel-chat";

import { format_prefill_progress, prefill_progress } from "./llm_phase";

export type ActivityKind =
  | "llm"
  | "tools"
  | "subflow"
  | "event"
  | "wait"
  | "ask"
  | "message"
  | "run"
  | "progress"
  | "step";

/** Structurally what the workspace hands us (panel-chat's `WorkflowRecord`). */
export type ActivityEntry = {
  cursor: number;
  record: unknown;
  runId: string;
};

export type RecordDescription = {
  kind: ActivityKind;
  title: string;
  detail: string;
  /** `abstract.progress` — feeds progress UI, never a row of its own. */
  isProgress: boolean;
  /** `abstract.status` — the status-bar text, never a row of its own. */
  isStatus: boolean;
  /** `resume` — closes the step it names, never a row of its own. */
  isResume: boolean;
  /** True for every record that must not appear as a standalone row. */
  hidden: boolean;
};

export type ActivityRow = {
  key: string;
  runId: string;
  /** Cursor of the newest record in the group — rows sort by first arrival. */
  cursor: number;
  kind: ActivityKind;
  title: string;
  detail: string;
  /** running | waiting | completed | failed */
  status: string;
  statusLabel: string;
  nodeId: string;
  stepIds: string[];
  /** Live or final progress sub-line, "" when this step reported none. */
  progress: string;
  progressFinal: boolean;
  /** Raw records of the group, arrival order — the JSON shown on expand. */
  entries: ActivityEntry[];
  /** Raw `abstract.progress` payloads owned by this step, arrival order. */
  progressEvents: Record<string, unknown>[];
  /** The merged record the description was computed from. */
  merged: Record<string, unknown>;
};

const PROGRESS_EVENT_NAMES = new Set([
  "abstract.progress",
  "abstractcode.progress",
]);
const STATUS_EVENT_NAMES = new Set(["abstract.status", "abstractcode.status"]);

/* ------------------------------------------------------------------ utils */

function obj(value: unknown): Record<string, any> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, any>)
    : null;
}

/** A payload field the ledger replaced with a `$slim` pointer carries no data. */
function is_slim(value: unknown): boolean {
  const o = obj(value);
  return Boolean(o && o.$slim);
}

function finite(value: unknown): number | null {
  const n = typeof value === "number" ? value : Number(value);
  return Number.isFinite(n) ? n : null;
}

function group_digits(value: number): string {
  return Math.round(value).toLocaleString("en-US");
}

function one_line(value: string, max = 120): string {
  const text = value.replace(/\s+/g, " ").trim();
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

/** A one-line preview of an arbitrary payload, for rows that have no schema. */
export function preview_value(value: unknown, max = 120): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return one_line(value, max);
  if (typeof value === "number" || typeof value === "boolean")
    return String(value);
  if (Array.isArray(value))
    return one_line(
      value.length === 0
        ? "[]"
        : `${value.length} ${value.length === 1 ? "item" : "items"}`,
      max,
    );
  const o = obj(value);
  if (!o) return "";
  if (is_slim(o)) {
    const field = String(o.$slim?.field || "");
    const bytes = finite(o.$slim?.bytes);
    return `${field || "payload"} stored${bytes ? ` · ${group_digits(bytes)} bytes` : ""}`;
  }
  const parts: string[] = [];
  for (const [key, raw] of Object.entries(o)) {
    const rendered =
      typeof raw === "string"
        ? one_line(raw, 60)
        : typeof raw === "number" || typeof raw === "boolean"
          ? String(raw)
          : Array.isArray(raw)
            ? `${raw.length} items`
            : raw === null || raw === undefined
              ? ""
              : "{…}";
    if (!rendered) continue;
    parts.push(`${key.replace(/_/g, " ")} ${rendered}`);
    if (parts.join(" · ").length >= max) break;
  }
  return one_line(parts.join(" · "), max);
}

function seconds(value: number): string {
  const text =
    value >= 100
      ? String(Math.round(value))
      : value.toFixed(value < 1 ? 2 : 1).replace(/\.0+$/, "");
  return `${text} s`;
}

function duration_s(started: unknown, ended: unknown): number | null {
  const a = Date.parse(String(started || ""));
  const b = Date.parse(String(ended || ""));
  if (!Number.isFinite(a) || !Number.isFinite(b)) return null;
  const delta = (b - a) / 1000;
  return delta >= 0 ? delta : null;
}

/** "23:54:33" from an ISO timestamp — stable across locales, unlike toLocale*. */
function clock(value: unknown): string {
  const text = String(value || "");
  const match = /T(\d{2}:\d{2}:\d{2})/.exec(text);
  return match ? match[1] : "";
}

/** "Jundot/Qwen3.8-Flash-Next-oQ4e-mtp" → "Qwen3.8-Flash-Next-oQ4e-mtp". */
export function short_model(model: unknown): string {
  const text = String(model || "").trim();
  if (!text) return "";
  const tail = text.split("/").pop() || text;
  return tail.length > 48 ? `${tail.slice(0, 47)}…` : tail;
}

function join(parts: (string | null | undefined | false)[], sep = " · "): string {
  return parts.filter((part): part is string => Boolean(part)).join(sep);
}

/* -------------------------------------------------------- subflow labelling */

const VISUAL_PREFIX = "visual_react_agent_";

/**
 * A published catalog workflow buries the bundle name in a base64url segment
 * (`YWJzdHJhY3Rhc3Npc3RhbnQtb3JjaGVzdHJhdG9y` → `abstractassistant-orchestrator`).
 * Decode it when it decodes to something printable; otherwise keep the raw id —
 * a wrong-but-pretty label is worse than an ugly true one.
 */
function decode_catalog_segment(segment: string): string | null {
  if (!/^[A-Za-z0-9_-]{8,}$/.test(segment)) return null;
  try {
    const padded = segment.replace(/-/g, "+").replace(/_/g, "/");
    const decoded = atob(padded + "=".repeat((4 - (padded.length % 4)) % 4));
    if (!/^[ -~]{3,}$/.test(decoded)) return null;
    if (!/[a-z]/i.test(decoded)) return null;
    return decoded;
  } catch {
    return null;
  }
}

/** `{ name, node }` for a workflow id, best effort, never throws. */
export function subflow_label(workflowId: unknown): {
  name: string;
  node: string;
} {
  const id = String(workflowId || "").trim();
  if (!id) return { name: "", node: "" };
  // `basic-agent@0.0.4:15f19f7f`
  const at = /^([^@]+)@([^:]+)(?::(.+))?$/.exec(id);
  if (at) return { name: at[1], node: at[3] || "" };
  if (id.startsWith(VISUAL_PREFIX)) {
    const rest = id.slice(VISUAL_PREFIX.length);
    const shaped = /^(.*)_(\d+)_(\d+)_(\d+)_([0-9a-f]{6,12})_(.+)$/.exec(rest);
    if (shaped) {
      let name = shaped[1];
      if (name.includes("__")) {
        const segments = name.split("__").filter(Boolean);
        for (let i = segments.length - 1; i >= 0; i -= 1) {
          const decoded = decode_catalog_segment(segments[i]);
          if (decoded && decoded.length > 3) {
            name = decoded;
            break;
          }
        }
      }
      return { name, node: shaped[6] };
    }
    return { name: rest, node: "" };
  }
  return { name: id, node: "" };
}

/* ------------------------------------------------------------- tool naming */

/** "web_search ×3" / "read_file, web_search" — names, deduped, in order. */
export function tool_names(calls: unknown): string {
  if (!Array.isArray(calls) || !calls.length) return "";
  const counts = new Map<string, number>();
  for (const call of calls) {
    const name = String(obj(call)?.name || "").trim() || "tool";
    counts.set(name, (counts.get(name) || 0) + 1);
  }
  return [...counts.entries()]
    .map(([name, n]) => (n > 1 ? `${name} ×${n}` : name))
    .join(", ");
}

/* ------------------------------------------------------------ progress line */

export type ProgressEvent = Record<string, any>;

/** The `abstract.progress` payload of a record, or null. */
export function progress_payload(record: unknown): ProgressEvent | null {
  const r = obj(record);
  if (!r) return null;
  const effect = obj(r.effect);
  if (!effect || String(effect.type || "") !== "emit_event") return null;
  const effectPayload = obj(effect.payload);
  const name = String(
    effectPayload?.name ?? effectPayload?.event_name ?? "",
  ).trim();
  if (!PROGRESS_EVENT_NAMES.has(name)) return null;
  const fromResult = obj(obj(r.result)?.payload);
  const fromEffect = obj(effectPayload?.payload);
  return fromResult || fromEffect;
}

function is_status_record(record: unknown): boolean {
  const effect = obj(obj(record)?.effect);
  if (!effect || String(effect.type || "") !== "emit_event") return false;
  const payload = obj(effect.payload);
  const name = String(payload?.name ?? payload?.event_name ?? "").trim();
  return STATUS_EVENT_NAMES.has(name);
}

/**
 * The live sub-line for an LLM step:
 *   "prefill 2,100 / 8,407 tokens (37%)"   (while the provider reports progress)
 *   "prefill 8,407 tokens (6,900 cached) → generating 89 tokens · 34 tok/s"
 * and, once the model has stopped:
 *   "5,899 tokens in · 4,096 cached · 312 out · 9.8 s · 34 tok/s"
 */
export function format_llm_progress(events: readonly ProgressEvent[]): string {
  if (!events.length) return "";
  const latest = events[events.length - 1];
  let prompt: number | null = null;
  let cached: number | null = null;
  let generated: number | null = null;
  let rate: number | null = null;
  let elapsed: number | null = null;
  for (const event of events) {
    prompt = finite(event.prompt_tokens) ?? prompt;
    cached = finite(event.cached_tokens) ?? cached;
    generated = finite(event.generated_tokens) ?? generated;
    rate = finite(event.tokens_per_second) ?? rate;
    elapsed = finite(event.elapsed_s) ?? elapsed;
  }
  const done = latest.final === true || String(latest.phase || "") === "complete";
  if (done) {
    return join([
      prompt !== null ? `${group_digits(prompt)} tokens in` : null,
      cached !== null && cached > 0 ? `${group_digits(cached)} cached` : null,
      generated !== null ? `${group_digits(generated)} out` : null,
      elapsed !== null ? seconds(elapsed) : null,
      rate !== null && rate > 0 ? `${Math.round(rate)} tok/s` : null,
    ]);
  }
  const prefill =
    prompt !== null
      ? `prefill ${group_digits(prompt)} tokens${
          cached !== null && cached > 0 ? ` (${group_digits(cached)} cached)` : ""
        }`
      : "prefill";
  if (String(latest.phase || "") === "prefill") {
    // Same processed/total form as the run strip, from the newest event only:
    // an older event's position is stale, never carried forward.
    const live = prefill_progress(latest as any);
    return live ? `prefill ${format_prefill_progress(live)}` : prefill;
  }
  const generating = join(
    [
      `generating ${group_digits(generated ?? 0)} ${
        Math.round(generated ?? 0) === 1 ? "token" : "tokens"
      }`,
      rate !== null && rate > 0 ? `${Math.round(rate)} tok/s` : null,
    ],
  );
  return `${prefill} → ${generating}`;
}

/**
 * Media and any other non-LLM progress: whatever of step/total/percent/frame
 * the producer chose to report, in the order a human reads it.
 */
export function format_media_progress(events: readonly ProgressEvent[]): string {
  if (!events.length) return "";
  const latest = events[events.length - 1];
  const step = finite(latest.step) ?? finite(latest.index);
  const total = finite(latest.total) ?? finite(latest.steps);
  const percent = finite(latest.percent) ?? finite(latest.progress);
  const frame = finite(latest.frame);
  const elapsed = finite(latest.elapsed_s);
  return join([
    typeof latest.label === "string" ? one_line(latest.label, 60) : null,
    typeof latest.phase === "string" && !latest.label
      ? one_line(latest.phase, 40)
      : null,
    step !== null
      ? `step ${group_digits(step)}${total !== null ? `/${group_digits(total)}` : ""}`
      : null,
    frame !== null ? `frame ${group_digits(frame)}` : null,
    percent !== null ? `${Math.round(percent <= 1 ? percent * 100 : percent)}%` : null,
    elapsed !== null ? seconds(elapsed) : null,
  ]);
}

export function format_progress(events: readonly ProgressEvent[]): string {
  if (!events.length) return "";
  const kind = String(events[events.length - 1].kind || "");
  return kind === "llm"
    ? format_llm_progress(events)
    : format_media_progress(events);
}

/* --------------------------------------------------------- describe_record */

function describe_llm(
  payload: Record<string, any>,
  result: Record<string, any> | null,
  record: Record<string, any>,
): { title: string; detail: string } {
  const provider = String(payload.provider || "").trim();
  const model = short_model(payload.model || result?.model);
  const title = join(["llm", provider, model]) || "llm call";
  if (!result) {
    const params = obj(payload.params) || {};
    return {
      title,
      detail: join([
        finite(params.temperature) !== null
          ? `temperature ${params.temperature}`
          : null,
        params.thinking ? `thinking ${params.thinking}` : null,
        obj(params.speculation)?.mode
          ? `speculation ${obj(params.speculation)!.mode}`
          : null,
      ]),
    };
  }
  const usage = obj(result.usage) || {};
  const metadata = obj(result.metadata) || {};
  const cache = obj(metadata.prompt_cache) || {};
  const performance = obj(metadata.performance) || {};
  const input = finite(usage.prompt_tokens) ?? finite(usage.input_tokens);
  const output = finite(usage.completion_tokens) ?? finite(usage.output_tokens);
  const cachedTokens = finite(cache.cached_tokens);
  const outcome = String(cache.outcome || "").trim();
  const elapsed = duration_s(record.started_at, record.ended_at);
  const rate = finite(performance.generation_tokens_per_second);
  const calls = Array.isArray(result.tool_calls) ? result.tool_calls.length : 0;
  return {
    title,
    detail: join([
      input !== null ? `${group_digits(input)} tokens in` : null,
      cachedTokens !== null && cachedTokens > 0
        ? `${group_digits(cachedTokens)} cached`
        : outcome
          ? `${outcome} cache`
          : null,
      output !== null ? `${group_digits(output)} out` : null,
      elapsed !== null ? seconds(elapsed) : null,
      rate !== null && rate > 0 ? `${Math.round(rate)} tok/s` : null,
      calls ? `${calls} tool ${calls === 1 ? "call" : "calls"}` : null,
    ]),
  };
}

/**
 * #TRUNCATION: a long url or path abbreviates to the part that identifies it,
 * the way panel-chat's tool cards abbreviate a path. The exact target is one
 * click away in the expanded record; a three-line wrapped url in a 300px panel
 * identifies nothing.
 */
function abbreviate_target(label: string, value: string): string {
  if (value.length <= 60) return one_line(value, 60);
  if (label === "url") {
    try {
      const url = new URL(value);
      const tail = url.pathname.split("/").filter(Boolean).pop() || "";
      return one_line(`${url.host}${tail ? ` · …/${tail}` : ""}`, 72);
    } catch {
      /* not a url after all — fall through */
    }
  }
  if (label === "file path" || label === "path") {
    const tail = value.split(/[\\/]/).pop() || value;
    return one_line(`${tail} · ${value.slice(0, 24)}…`, 60);
  }
  return one_line(value, 60);
}

function describe_tools(
  payload: Record<string, any>,
  result: Record<string, any> | null,
): { title: string; detail: string } {
  const calls = Array.isArray(payload.tool_calls) ? payload.tool_calls : [];
  const names = tool_names(calls);
  const first = obj(calls[0]);
  const preview = first ? toolPreview(first.arguments) : { label: "", value: "" };
  const results = Array.isArray(result?.results) ? result!.results : [];
  const ok = results.filter((r: any) => obj(r)?.success === true).length;
  const failed = results.length - ok;
  return {
    title: join(["tools", names || "tool call"]),
    detail: join([
      preview.value
        ? `${preview.label} ${abbreviate_target(preview.label, preview.value)}`
        : null,
      results.length ? `${ok} ok` : null,
      failed > 0 ? `${failed} failed` : null,
    ]),
  };
}

function describe_subflow(
  payload: Record<string, any>,
  result: Record<string, any> | null,
): { title: string; detail: string } {
  const { name, node } = subflow_label(payload.workflow_id);
  const vars = obj(payload.vars);
  const usable = vars && !is_slim(vars) ? vars : null;
  const context = obj(usable?.context);
  const task =
    typeof context?.task === "string"
      ? context.task
      : typeof usable?.request === "string"
        ? usable.request
        : "";
  const subRun = String(
    obj(obj(result?.wait)?.details)?.sub_run_id || "",
  ).slice(0, 8);
  // The status helper is a one-variable subflow whose whole job is to push a
  // label into the UI. Calling it "start subworkflow" is how the operator ended
  // up with unreadable rows.
  if (usable && typeof usable.state === "string" && !context) {
    return {
      title: "subflow · status",
      detail: join([one_line(usable.state, 80), subRun && `run ${subRun}`]),
    };
  }
  if (task) {
    return {
      title: "subflow · agent loop",
      detail: join([name, one_line(task, 90), subRun && `run ${subRun}`]),
    };
  }
  return {
    title: join(["subflow", name || "workflow"]),
    detail: join([
      node,
      usable ? preview_value(usable, 80) : null,
      subRun && `run ${subRun}`,
    ]),
  };
}

function describe_wait_until(
  payload: Record<string, any>,
  record: Record<string, any>,
): { title: string; detail: string } {
  const until = payload.until;
  const delay = duration_s(record.started_at, until);
  const at = clock(until);
  return {
    title: delay !== null ? `wait · delay ${seconds(delay)}` : `wait · until ${at}`,
    detail: join([
      delay !== null && at ? `until ${at}` : null,
      payload.resume_to_node ? `resumes at ${payload.resume_to_node}` : null,
    ]),
  };
}

/** One ledger record → what it is. Pure; never throws on a malformed record. */
export function describe_record(record: unknown): RecordDescription {
  const r = obj(record) || {};
  const effect = obj(r.effect);
  const type = String(effect?.type || "").trim();
  const payload = obj(effect?.payload) || {};
  const result = obj(r.result);

  if (type === "emit_event") {
    const name = String(payload.name ?? payload.event_name ?? "").trim();
    if (PROGRESS_EVENT_NAMES.has(name))
      return {
        kind: "progress",
        title: "progress",
        detail: "",
        isProgress: true,
        isStatus: false,
        isResume: false,
        hidden: true,
      };
    if (STATUS_EVENT_NAMES.has(name))
      return {
        kind: "event",
        title: "status",
        detail: preview_value(payload.payload, 80),
        isProgress: false,
        isStatus: true,
        isResume: false,
        hidden: true,
      };
    const scope = String(payload.scope || "").trim();
    return {
      kind: "event",
      title: `event · ${name || "unnamed"}${scope ? ` (${scope})` : ""}`,
      detail: preview_value(payload.payload ?? result?.payload, 110),
      isProgress: false,
      isStatus: false,
      isResume: false,
      hidden: false,
    };
  }

  if (type === "resume")
    return {
      kind: "step",
      title: "resume",
      detail: String(payload.wait_reason || ""),
      isProgress: false,
      isStatus: false,
      isResume: true,
      hidden: true,
    };

  const visible = {
    isProgress: false,
    isStatus: false,
    isResume: false,
    hidden: false,
  } as const;

  if (type === "llm_call")
    return { kind: "llm", ...describe_llm(payload, result, r), ...visible };

  if (type === "tool_calls")
    return { kind: "tools", ...describe_tools(payload, result), ...visible };

  if (type === "start_subworkflow")
    return { kind: "subflow", ...describe_subflow(payload, result), ...visible };

  if (type === "wait_until")
    return { kind: "wait", ...describe_wait_until(payload, r), ...visible };

  if (type === "wait_event")
    return {
      kind: "wait",
      title: "wait · event",
      detail: join([
        typeof payload.prompt === "string" ? one_line(payload.prompt, 90) : null,
        !payload.prompt && payload.wait_key
          ? one_line(String(payload.wait_key), 70)
          : null,
      ]),
      ...visible,
    };

  if (type === "ask_user")
    return {
      kind: "ask",
      title: "ask · your input",
      detail: one_line(
        String(payload.prompt || obj(result?.wait)?.prompt || "").split("\n")[0],
        110,
      ),
      ...visible,
    };

  if (type === "answer_user")
    return {
      kind: "message",
      title: "message",
      detail: one_line(String(payload.message || ""), 110),
      ...visible,
    };

  if (!type) {
    if (result?.completed === true) {
      const output = obj(result.output) || {};
      const stop = obj(output.stop_reason) || {};
      const iterations = finite(output.iterations);
      const outcome = join([
        String(stop.code || output.outcome || "").replace(/_/g, " ") || null,
        iterations !== null
          ? `${group_digits(iterations)} ${iterations === 1 ? "iteration" : "iterations"}`
          : null,
      ]);
      // A parent run hands back its subflow's answer with no stop_reason of its
      // own; an empty "run finished" row is exactly the uninformative row this
      // module exists to remove.
      const answer = [output.answer, output.response, output.result].find(
        (value) => typeof value === "string" && value.trim(),
      );
      return {
        kind: "run",
        title: "run finished",
        detail:
          outcome ||
          (typeof answer === "string"
            ? one_line(answer, 110)
            : preview_value(output, 110)),
        ...visible,
      };
    }
    return {
      kind: "step",
      title: String(r.node_id || "workflow step"),
      detail: "",
      ...visible,
    };
  }

  return {
    kind: "step",
    title: type.replace(/_/g, " "),
    detail: preview_value(payload, 110),
    ...visible,
  };
}

/* ------------------------------------------------------------ status label */

export function status_label(
  record: unknown,
  kind: ActivityKind,
  resumed: boolean,
): { status: string; statusLabel: string } {
  const r = obj(record) || {};
  const raw = String(r.status || "").trim();
  if (r.error) return { status: "failed", statusLabel: "failed" };
  if (raw === "failed") return { status: "failed", statusLabel: "failed" };
  if (raw === "completed") return { status: "completed", statusLabel: "completed" };
  if (raw === "waiting") {
    if (resumed) return { status: "completed", statusLabel: "completed" };
    const wait = obj(obj(r.result)?.wait);
    const mode = String(obj(wait?.details)?.mode || "");
    if (mode === "approval_required")
      return { status: "waiting", statusLabel: "approval needed" };
    if (kind === "subflow")
      return { status: "waiting", statusLabel: "waiting for subflow" };
    if (kind === "ask") return { status: "waiting", statusLabel: "waiting for you" };
    if (kind === "wait")
      return {
        status: "waiting",
        statusLabel:
          String(wait?.reason || "") === "until" ? "waiting" : "waiting for event",
      };
    return { status: "waiting", statusLabel: "waiting" };
  }
  if (raw === "started") return { status: "started", statusLabel: "running" };
  return { status: raw || "started", statusLabel: raw || "running" };
}

/* ---------------------------------------------------------------- grouping */

/**
 * The identity of a step across its started / waiting / completed records.
 *
 * `idempotency_key` is the runtime's own identity for an effect and survives
 * the fresh `step_id` a `tool_calls` completion is written under; `attempt`
 * keeps a retry as its own row, which is what a reader wants to see.
 */
export function group_key(entry: ActivityEntry): string {
  const r = obj(entry.record) || {};
  const type = String(obj(r.effect)?.type || "(none)");
  const identity = String(r.idempotency_key || r.step_id || entry.cursor);
  return `${entry.runId}|${type}|${identity}|${r.attempt ?? 1}`;
}

/**
 * One record standing for the whole group: the newest status and result, but
 * the concrete payload fields, which the ledger replaces with `$slim` pointers
 * once a step has finished.
 */
export function merge_group(entries: readonly ActivityEntry[]): Record<string, any> {
  const records = entries.map((entry) => obj(entry.record) || {});
  const last = records[records.length - 1] || {};
  const payload: Record<string, any> = {};
  for (const record of records) {
    const p = obj(obj(record.effect)?.payload);
    if (!p) continue;
    for (const [key, value] of Object.entries(p)) {
      if (is_slim(value) && key in payload) continue;
      payload[key] = value;
    }
  }
  let result: unknown = null;
  let error: unknown = null;
  let startedAt: unknown = null;
  let endedAt: unknown = null;
  for (const record of records) {
    if (record.result !== null && record.result !== undefined) result = record.result;
    if (record.error !== null && record.error !== undefined) error = record.error;
    if (!startedAt && record.started_at) startedAt = record.started_at;
    if (record.ended_at) endedAt = record.ended_at;
  }
  return {
    ...last,
    effect: last.effect ? { ...last.effect, payload } : last.effect,
    result,
    error,
    started_at: startedAt,
    ended_at: endedAt,
  };
}

/** Every wait_key a group announced, so a `resume` can find its owner. */
function wait_keys(entries: readonly ActivityEntry[]): string[] {
  const keys: string[] = [];
  for (const entry of entries) {
    const wait = obj(obj(obj(entry.record)?.result)?.wait);
    const key = String(wait?.wait_key || "").trim();
    if (key) keys.push(key);
  }
  return keys;
}

/* ------------------------------------------------------------ progress fold */

function owner_key(runId: string, payload: ProgressEvent): string {
  return `${runId}|${String(payload.step_id || payload.node_id || "")}`;
}

/**
 * Fold `abstract.progress` records into the rows they belong to.
 *
 * A progress record names its owner in its payload (`run_id` + `step_id` of the
 * step that emitted it), so the fold is exact: no timestamp heuristics. A
 * progress stream whose owning step is not in `rows` (a child run whose step
 * records were trimmed, say) becomes its own row rather than vanishing.
 */
export function attach_progress(
  rows: readonly ActivityRow[],
  entries: readonly ActivityEntry[],
): ActivityRow[] {
  const byOwner = new Map<string, { entries: ActivityEntry[]; payloads: ProgressEvent[] }>();
  for (const entry of entries) {
    const payload = progress_payload(entry.record);
    if (!payload) continue;
    const key = owner_key(entry.runId, payload);
    const bucket = byOwner.get(key) || { entries: [], payloads: [] };
    bucket.entries.push(entry);
    bucket.payloads.push(payload);
    byOwner.set(key, bucket);
  }
  if (!byOwner.size) return rows.map((row) => ({ ...row }));

  const claimed = new Set<string>();
  const out = rows.map((row) => {
    const buckets = row.stepIds
      .map((stepId) => `${row.runId}|${stepId}`)
      .filter((key) => byOwner.has(key));
    if (!buckets.length) return { ...row };
    const payloads: ProgressEvent[] = [];
    const raw: ActivityEntry[] = [];
    for (const key of buckets) {
      claimed.add(key);
      payloads.push(...byOwner.get(key)!.payloads);
      raw.push(...byOwner.get(key)!.entries);
    }
    const line = format_progress(payloads);
    const last = payloads[payloads.length - 1];
    const final = last.final === true || String(last.phase || "") === "complete";
    // A finished step already carries its own summary from the ledger result;
    // repeating the progress numbers under it is noise.
    const keep = !(final && row.status === "completed" && row.detail);
    return {
      ...row,
      progress: keep ? line : "",
      progressFinal: final,
      progressEvents: raw.map((entry) => obj(entry.record) || {}),
    };
  });

  for (const [key, bucket] of byOwner.entries()) {
    if (claimed.has(key)) continue;
    const payloads = bucket.payloads;
    const last = payloads[payloads.length - 1];
    const kind = String(last.kind || "");
    const first = bucket.entries[0];
    out.push({
      key: `progress:${key}`,
      runId: first.runId,
      cursor: bucket.entries[bucket.entries.length - 1].cursor,
      kind: "progress",
      title: kind === "llm" ? join(["llm", String(last.provider || ""), short_model(last.model)]) : join(["progress", kind]),
      detail: "",
      status: last.final === true || String(last.phase || "") === "complete" ? "completed" : "started",
      statusLabel:
        last.final === true || String(last.phase || "") === "complete"
          ? "completed"
          : "running",
      nodeId: String(last.node_id || ""),
      stepIds: [String(last.step_id || "")],
      progress: format_progress(payloads),
      progressFinal: last.final === true || String(last.phase || "") === "complete",
      entries: bucket.entries,
      progressEvents: bucket.entries.map((entry) => obj(entry.record) || {}),
      merged: obj(bucket.entries[bucket.entries.length - 1].record) || {},
    });
  }
  return out;
}

/* ------------------------------------------------------------ activity_rows */

/**
 * The ledger stream → the rows the Activity tab renders.
 *
 * Hidden entirely: `abstract.progress` (folded into its owner), `abstract.status`
 * (the status bar's text) and `resume` (it closes a wait, it is not an event).
 * Everything else collapses per step and is described.
 */
export function activity_rows(entries: readonly ActivityEntry[]): ActivityRow[] {
  const groups = new Map<string, ActivityEntry[]>();
  const order: string[] = [];
  const resumeKeys: string[] = [];
  const progressEntries: ActivityEntry[] = [];

  for (const entry of entries || []) {
    const description = describe_record(entry.record);
    if (description.isProgress) {
      progressEntries.push(entry);
      continue;
    }
    if (description.isStatus) continue;
    if (description.isResume) {
      const key = String(
        obj(obj(entry.record)?.effect)?.payload?.wait_key || "",
      ).trim();
      if (key) resumeKeys.push(key);
      continue;
    }
    const key = group_key(entry);
    if (!groups.has(key)) {
      groups.set(key, []);
      order.push(key);
    }
    groups.get(key)!.push(entry);
  }

  const resumed = new Set(resumeKeys);
  const rows: ActivityRow[] = order.map((key) => {
    const groupEntries = groups.get(key)!;
    const merged = merge_group(groupEntries);
    const description = describe_record(merged);
    const wasResumed = wait_keys(groupEntries).some((k) => resumed.has(k));
    const state = status_label(merged, description.kind, wasResumed);
    return {
      key,
      runId: groupEntries[0].runId,
      cursor: groupEntries[groupEntries.length - 1].cursor,
      kind: description.kind,
      title: description.title,
      detail: description.detail,
      status: state.status,
      statusLabel: state.statusLabel,
      nodeId: String(merged.node_id || ""),
      stepIds: [
        ...new Set(
          groupEntries
            .map((entry) => String(obj(entry.record)?.step_id || ""))
            .filter(Boolean),
        ),
      ],
      progress: "",
      progressFinal: false,
      entries: groupEntries,
      progressEvents: [],
      merged,
    };
  });

  return attach_progress(rows, progressEntries);
}
