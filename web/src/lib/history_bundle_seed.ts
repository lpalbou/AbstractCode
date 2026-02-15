import type { ReplMessage } from "./storage";

function _now_iso(): string {
  try {
    return new Date().toISOString();
  } catch {
    return "";
  }
}

function _ts_from_record(rec: any): string {
  const ended = typeof rec?.ended_at === "string" ? String(rec.ended_at) : "";
  const started = typeof rec?.started_at === "string" ? String(rec.started_at) : "";
  return ended || started || _now_iso();
}

function _push_user(out: ReplMessage[], args: { content: string; ts: string; run_id?: string; meta?: any }): void {
  const content = String(args.content || "");
  if (!content.trim()) return;
  out.push({ role: "user", content, ts: String(args.ts || _now_iso()), run_id: args.run_id, meta: args.meta });
}

function _push_assistant(out: ReplMessage[], args: { content: string; ts: string; run_id?: string; meta?: any }): void {
  const content = String(args.content || "");
  if (!content.trim()) return;
  out.push({ role: "assistant", content, ts: String(args.ts || _now_iso()), run_id: args.run_id, meta: args.meta });
}

function _seed_from_session_turns(bundle: any): ReplMessage[] {
  const turns = bundle?.session?.turns;
  if (!Array.isArray(turns) || !turns.length) return [];
  const out: ReplMessage[] = [];
  for (const t of turns) {
    if (!t || typeof t !== "object") continue;
    const run_id = String((t as any).run_id || "").trim() || undefined;
    const ts_user = String((t as any).created_at || (t as any).updated_at || "").trim() || _now_iso();
    const ts_asst = String((t as any).updated_at || (t as any).created_at || "").trim() || _now_iso();
    const prompt = String((t as any).prompt || "");
    const answer = String((t as any).answer || "");
    const answer_meta = (t as any).answer_meta;
    const stats = (t as any).stats;
    if (prompt.trim()) _push_user(out, { content: prompt, ts: ts_user, run_id });
    if (answer.trim()) _push_assistant(out, { content: answer, ts: ts_asst, run_id, meta: { _repl: { ...(answer_meta || {}), ...(stats ? { stats } : {}) } } });
  }
  return out;
}

function _extract_telegram_from_resume_payload(obj: any): { text: string; meta: any } | null {
  try {
    const telegram = obj?.effect?.payload?.payload?.payload?.telegram;
    if (!telegram || typeof telegram !== "object") return null;
    const text = String((telegram as any).text || "").trim();
    if (!text) return null;
    const attachments = obj?.effect?.payload?.payload?.payload?.attachments;
    const meta = { _kind: "telegram_in", telegram: { ...(telegram as any) }, attachments: Array.isArray(attachments) ? attachments : undefined };
    return { text, meta };
  } catch {
    return null;
  }
}

function _extract_telegram_out_from_tool_calls_record(obj: any): string[] {
  const out: string[] = [];
  try {
    const eff = obj?.effect;
    if (!eff || typeof eff !== "object") return out;
    if (String((eff as any).type || "") !== "tool_calls") return out;
    // Only count tool calls once they were actually executed; ledgers often contain
    // both `started` and `completed` records for the same tool_calls payload.
    const status = String((obj as any)?.status || "").trim().toLowerCase();
    if (status !== "completed") return out;
    const results = Array.isArray((obj as any)?.result?.results) ? (obj as any).result.results : [];
    if (!results.length) return out;
    const payload = (eff as any).payload;
    const tool_calls = Array.isArray(payload?.tool_calls) ? payload.tool_calls : [];
    for (const tc of tool_calls) {
      const name = String((tc as any)?.name || "").trim();
      if (name !== "send_telegram_message") continue;
      const txt = String((tc as any)?.arguments?.text || "").trim();
      if (txt) out.push(txt);
    }
  } catch {
    // ignore
  }
  return out;
}

function _seed_from_telegram_ledgers(bundle: any): ReplMessage[] {
  const ledgers = bundle?.ledgers;
  if (!ledgers || typeof ledgers !== "object") return [];
  const events: Array<{ ts: string; kind: "in" | "out"; text: string; run_id?: string; meta?: any }> = [];

  for (const [run_id, ledger] of Object.entries(ledgers as Record<string, any>)) {
    const items = Array.isArray((ledger as any)?.items) ? (ledger as any).items : [];
    for (const it of items) {
      const rec = it?.record;
      if (!rec || typeof rec !== "object") continue;
      const eff_type = String(rec?.effect?.type || "").trim();
      if (eff_type === "resume") {
        const hit = _extract_telegram_from_resume_payload(rec);
        if (hit) events.push({ ts: _ts_from_record(rec), kind: "in", text: hit.text, run_id, meta: hit.meta });
      }
      if (eff_type === "tool_calls") {
        const outs = _extract_telegram_out_from_tool_calls_record(rec);
        for (const text of outs) events.push({ ts: _ts_from_record(rec), kind: "out", text, run_id, meta: { _kind: "telegram_out" } });
      }
    }
  }

  if (!events.length) return [];
  events.sort((a, b) => String(a.ts).localeCompare(String(b.ts)));
  const out: ReplMessage[] = [];
  for (const ev of events) {
    if (ev.kind === "in") _push_user(out, { content: ev.text, ts: ev.ts, run_id: ev.run_id, meta: ev.meta });
    else _push_assistant(out, { content: ev.text, ts: ev.ts, run_id: ev.run_id, meta: ev.meta });
  }
  return out;
}

function _seed_tool_cards(bundle: any, run_id: string): ReplMessage[] {
  const rid = String(run_id || "").trim();
  if (!rid) return [];
  const ledger = bundle?.ledgers?.[rid];
  const items = Array.isArray(ledger?.items) ? ledger.items : [];
  const out: ReplMessage[] = [];

  for (const it of items) {
    const rec = it?.record;
    if (!rec || typeof rec !== "object") continue;
    const eff = rec?.effect;
    if (!eff || typeof eff !== "object") continue;
    if (String(eff.type || "") !== "tool_calls") continue;

    const payload = eff.payload;
    const tool_calls = Array.isArray(payload?.tool_calls) ? payload.tool_calls : [];
    const results = Array.isArray(rec?.result?.results) ? rec.result.results : [];
    const by_call_id = new Map<string, any>();
    for (const r of results) {
      const cid = String((r as any)?.call_id || (r as any)?.id || "").trim();
      if (!cid) continue;
      by_call_id.set(cid, r);
    }

    for (const tc of tool_calls) {
      const name = String((tc as any)?.name || "").trim();
      if (!name) continue;
      // Transport tools are noisy in chat replay.
      if (name === "send_telegram_message" || name === "send_telegram_artifact") continue;
      const call_id = String((tc as any)?.call_id || (tc as any)?.id || (tc as any)?.runtime_call_id || "").trim();
      const args = (tc as any)?.arguments;
      const res = call_id ? by_call_id.get(call_id) : null;
      const success = typeof (res as any)?.success === "boolean" ? Boolean((res as any).success) : undefined;
      const error = String((res as any)?.error || "").trim() || undefined;
      const output = (res as any)?.output;
      let output_preview = "";
      try {
        output_preview = output === undefined ? "" : typeof output === "string" ? output : JSON.stringify(output, null, 2);
      } catch {
        output_preview = "";
      }
      if (output_preview.length > 8000) output_preview = `${output_preview.slice(0, 8000)}\n… (truncated)`;

      out.push({
        role: "system",
        content: "",
        ts: _ts_from_record(rec),
        run_id: rid,
        meta: {
          _kind: "tool",
          tool: {
            name,
            call_id: call_id || undefined,
            pending: false,
            success,
            error,
            arguments: args,
            output_preview,
          },
        },
      });
    }
  }
  return out;
}

export function seed_repl_messages_from_history_bundle(
  bundle: any,
  opts?: { now_iso?: () => string; include_tool_calls_for_run_id?: string }
): ReplMessage[] {
  const now_iso = typeof opts?.now_iso === "function" ? opts.now_iso : _now_iso;

  // Primary: session turns (covers normal AbstractCode root runs).
  const from_turns = _seed_from_session_turns(bundle);
  if (from_turns.length) {
    const extra_tools = opts?.include_tool_calls_for_run_id ? _seed_tool_cards(bundle, opts.include_tool_calls_for_run_id) : [];
    return [...from_turns, ...extra_tools].map((m) => ({ ...m, ts: String(m.ts || now_iso()) || now_iso() }));
  }

  // Fallback: Telegram event-driven flows typically have the conversation in ledgers + tool calls.
  const from_tg = _seed_from_telegram_ledgers(bundle);
  const extra_tools = opts?.include_tool_calls_for_run_id ? _seed_tool_cards(bundle, opts.include_tool_calls_for_run_id) : [];
  if (from_tg.length) return [...from_tg, ...extra_tools].map((m) => ({ ...m, ts: String(m.ts || now_iso()) || now_iso() }));

  // As a last resort, seed from root input_data.prompt if present.
  const root_prompt = String(bundle?.input_data?.prompt || bundle?.input_data?.context?.task || "").trim();
  if (!root_prompt) return extra_tools;
  return [{ role: "user", content: root_prompt, ts: now_iso(), run_id: String(bundle?.root_run_id || "").trim() || undefined }, ...extra_tools];
}
