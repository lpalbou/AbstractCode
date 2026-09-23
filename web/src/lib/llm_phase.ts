/**
 * Live LLM phase feedback: "Prefill · 2,100 / 5,642 tokens (37%)" while the
 * prompt is processed (lanes that observe their prefill chunk by chunk), else
 * "Prefill · 5,899 tokens (4,096 cached, 1,803 new)".
 *
 * AbstractCore providers that can observe the prefill/generation boundary
 * (today: every MLX lane) call back with phase events; AbstractRuntime turns
 * each one into a durable `abstract.progress` EMIT_EVENT ledger record with a
 * `kind: "llm"` discriminator, scoped to the run that made the call. The
 * `llm_call` of a chat turn lives in the agent's SUBWORKFLOW run, so a client
 * only sees these once it follows child ledgers — the same requirement that
 * already applies to the "Thinking…" `abstract.status` event.
 *
 * Everything here is pure so it can be unit-tested without a gateway: callers
 * hand in ledger records (newest last) and get back a line to render, or null
 * when this run has no phase signal at all (the provider had none to give, and
 * the caller must fall back to its existing status text rather than invent a
 * phase).
 */

export type LlmPhase = "prefill" | "generate" | "complete";

export type LlmPhaseEvent = {
  kind?: string;
  phase?: string;
  run_id?: string;
  step_id?: string;
  node_id?: string;
  event_index?: number;
  provider?: string;
  model?: string;
  prompt_tokens?: number;
  cached_tokens?: number;
  fed_tokens?: number;
  /** Prefill events only: prompt tokens already in the KV cache (restored + fed so far). */
  prefill_processed_tokens?: number;
  prefill_tokens_per_second?: number;
  generated_tokens?: number;
  tokens_per_second?: number;
  ttft_s?: number;
  elapsed_s?: number;
  final?: boolean;
  [k: string]: unknown;
};

const PROGRESS_EVENT_NAMES = new Set(["abstract.progress", "abstractcode.progress"]);

function is_object(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function finite(value: unknown): number | null {
  const n = typeof value === "number" ? value : Number(value);
  return Number.isFinite(n) ? n : null;
}

/**
 * Pull an llm phase event out of one ledger record, or null.
 *
 * Both the effect payload and the completed result carry the payload; the
 * result is preferred because a STARTED row can be superseded, and a progress
 * record is only ever appended already-completed.
 */
export function extract_llm_phase_event(record: unknown): LlmPhaseEvent | null {
  if (!is_object(record)) return null;
  const effect = is_object(record.effect) ? record.effect : null;
  if (!effect || String(effect.type || "") !== "emit_event") return null;
  const effect_payload = is_object(effect.payload) ? effect.payload : null;
  const name = String((effect_payload?.name ?? effect_payload?.event_name ?? "") as string).trim();
  if (!PROGRESS_EVENT_NAMES.has(name)) return null;
  const result = is_object(record.result) ? record.result : null;
  const from_result = is_object(result?.payload) ? (result?.payload as Record<string, unknown>) : null;
  const from_effect = is_object(effect_payload?.payload)
    ? (effect_payload?.payload as Record<string, unknown>)
    : null;
  const payload = from_result || from_effect;
  if (!payload) return null;
  // Media progress rides the same event name. Only the llm discriminator is
  // ours; anything else belongs to the generated-media consumers.
  if (String(payload.kind || "") !== "llm") return null;
  const phase = String(payload.phase || "").trim();
  if (phase !== "prefill" && phase !== "generate" && phase !== "complete") return null;
  return payload as LlmPhaseEvent;
}

/**
 * The phase of the newest LLM call still in flight, or null.
 *
 * "Newest" is by arrival order (ledger records are appended), and a call whose
 * `complete` event has landed is finished: reporting "Generating · 551 tokens"
 * after the model stopped is worse than reporting nothing. Records from several
 * runs may be interleaved (root + followed children), so the winning step wins
 * across all of them.
 */
export function latest_llm_phase(records: readonly unknown[]): LlmPhaseEvent | null {
  let latest: LlmPhaseEvent | null = null;
  for (const record of records || []) {
    const event = extract_llm_phase_event(record);
    if (!event) continue;
    latest = event;
  }
  if (!latest) return null;
  if (latest.phase === "complete" || latest.final === true) return null;
  return latest;
}

function count(n: number): string {
  return Math.round(n).toLocaleString();
}

/**
 * Live prefill position, or null when the provider measured none.
 *
 * `prefill_processed_tokens` is only ever present when the lane observed its
 * prompt pass (restored tokens count from the start); an HTTP provider or a
 * one-shot prefill never sends it, and the caller then shows the total only.
 * The percentage is the measured ratio, rounded, and held at 99% until the
 * first token: "100%" while the model has not answered would be a lie.
 */
export function prefill_progress(
  event: LlmPhaseEvent | null | undefined,
): { processed: number; total: number; percent: number } | null {
  if (!event || event.phase !== "prefill") return null;
  const total = finite(event.prompt_tokens);
  const processed = finite(event.prefill_processed_tokens);
  if (total === null || total <= 0 || processed === null || processed < 0) return null;
  const clamped = Math.min(processed, total);
  const percent = Math.min(99, Math.round((clamped / total) * 100));
  return { processed: clamped, total, percent };
}

/** "2,100 / 5,642 tokens (37%)" — the shared processed/total form. */
export function format_prefill_progress(progress: { processed: number; total: number; percent: number }): string {
  return `${count(progress.processed)} / ${count(progress.total)} tokens (${progress.percent}%)`;
}

/**
 * "Prefill · 2,100 / 5,642 tokens (37%)" while the provider reports progress,
 * "Prefill · 5,899 tokens (4,096 cached, 1,803 new)" when it reports only the
 * size, "Generating · 120 tokens · 45 tok/s" once the first token is out.
 */
export function format_llm_phase(event: LlmPhaseEvent | null | undefined): string {
  if (!event) return "";
  if (event.phase === "prefill") {
    const live = prefill_progress(event);
    if (live) return `Prefill · ${format_prefill_progress(live)}`;
    const prompt = finite(event.prompt_tokens);
    if (prompt === null || prompt <= 0) return "Prefill";
    const cached = finite(event.cached_tokens);
    const fed = finite(event.fed_tokens);
    const parts: string[] = [];
    if (cached !== null && cached > 0) parts.push(`${count(cached)} cached`);
    if (fed !== null && fed > 0) parts.push(`${count(fed)} new`);
    const split = parts.length ? ` (${parts.join(", ")})` : "";
    return `Prefill · ${count(prompt)} tokens${split}`;
  }
  if (event.phase === "generate") {
    const generated = finite(event.generated_tokens) ?? 0;
    const unit = Math.round(generated) === 1 ? "token" : "tokens";
    const rate = finite(event.tokens_per_second);
    const tail = rate !== null && rate > 0 ? ` · ${Math.round(rate)} tok/s` : "";
    return `Generating · ${count(generated)} ${unit}${tail}`;
  }
  return "";
}

/** One call: records in, display line out (empty string when there is no signal). */
export function llm_phase_text(records: readonly unknown[]): string {
  return format_llm_phase(latest_llm_phase(records));
}
