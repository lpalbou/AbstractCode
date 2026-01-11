import React, { useEffect, useMemo, useRef, useState } from "react";

import { GatewayClient } from "../lib/gateway_client";
import { random_id } from "../lib/ids";
import { extract_tool_calls_from_wait, extract_wait_from_record } from "../lib/runtime_extractors";
import { LedgerStreamEvent, StepRecord, ToolCall, WaitState } from "../lib/types";
import { MarkdownRenderer } from "./markdown_renderer";
import { MultiSelect } from "./multi_select";
import {
  create_new_repl_session,
  load_current_repl_session,
  load_settings,
  ReplMessage,
  ReplState,
  ReplTemplate,
  reset_repl_state,
  save_current_repl_session,
  save_settings,
  Settings,
} from "../lib/storage";

type Route = { name: "console" } | { name: "new" } | { name: "sessions" } | { name: "settings" };

type AgentTemplate = {
  bundle_id: string;
  flow_id: string;
  name: string;
  description: string;
  interfaces: string[];
};

type AttachedFile = {
  path: string;
  content: string | null;
  loading: boolean;
  error?: string;
};

function parse_route(): Route {
  const h = String(window.location.hash || "").replace(/^#/, "");
  const parts = h.split("/").filter(Boolean);
  if (!parts.length) return { name: "console" };
  if (parts[0] === "new") return { name: "new" };
  if (parts[0] === "sessions") return { name: "sessions" };
  if (parts[0] === "settings") return { name: "settings" };
  return { name: "console" };
}

function set_route(r: Route): void {
  if (r.name === "console") window.location.hash = "#/";
  else if (r.name === "new") window.location.hash = "#/new";
  else if (r.name === "sessions") window.location.hash = "#/sessions";
  else if (r.name === "settings") window.location.hash = "#/settings";
}

function safe_json(v: any): string {
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}

function parse_iso_ms(ts: string): number | null {
  const s = String(ts || "").trim();
  if (!s) return null;
  const ms = Date.parse(s);
  return Number.isFinite(ms) ? ms : null;
}

type ActiveToken = { start: number; end: number; query: string };

function extract_active_token(text: string, cursor: number, token: string): ActiveToken | null {
  const t = typeof text === "string" ? text : String(text ?? "");
  const cur = Number.isFinite(Number(cursor)) ? Math.max(0, Math.min(t.length, Math.trunc(cursor))) : t.length;
  const mark = String(token || "").trim();
  if (!mark) return null;
  const before = t.slice(0, cur);
  const idx = before.lastIndexOf(mark);
  if (idx < 0) return null;
  // Require the marker to start a token (start-of-string or whitespace before it).
  if (idx > 0 && !/\s/.test(t[idx - 1] || "")) return null;
  const between = t.slice(idx + mark.length, cur);
  // Token ends at whitespace.
  if (/\s/.test(between)) return null;
  return { start: idx, end: cur, query: between };
}

function now_iso(): string {
  return new Date().toISOString();
}

function parse_tools_allowlist(raw: string[]): string[] {
  const parts = Array.isArray(raw) ? raw.map((x) => String(x || "").trim()).filter(Boolean) : [];
  const uniq: string[] = [];
  const seen = new Set<string>();
  for (const p of parts) {
    if (seen.has(p)) continue;
    seen.add(p);
    uniq.push(p);
  }
  return uniq;
}

type UsageSummary = { input_tokens: number; output_tokens: number; total_tokens: number };

function parse_usage_summary(value: any): UsageSummary | null {
  if (!value || typeof value !== "object") return null;
  const v: any = value;
  const in_tok = Number(v.input_tokens ?? v.prompt_tokens ?? v.prompt ?? v.input ?? v.in ?? 0);
  const out_tok = Number(v.output_tokens ?? v.completion_tokens ?? v.completion ?? v.output ?? v.out ?? 0);
  const total_tok = Number(v.total_tokens ?? v.total ?? (Number.isFinite(in_tok) && Number.isFinite(out_tok) ? in_tok + out_tok : 0));
  if (!Number.isFinite(in_tok) && !Number.isFinite(out_tok) && !Number.isFinite(total_tok)) return null;
  return {
    input_tokens: Number.isFinite(in_tok) ? Math.max(0, Math.trunc(in_tok)) : 0,
    output_tokens: Number.isFinite(out_tok) ? Math.max(0, Math.trunc(out_tok)) : 0,
    total_tokens: Number.isFinite(total_tok) ? Math.max(0, Math.trunc(total_tok)) : 0,
  };
}

function compute_run_stats(events: LedgerStreamEvent[]): {
  duration_ms: number;
  llm_calls: number;
  tool_calls: number;
  usage: UsageSummary;
} {
  const out: { duration_ms: number; llm_calls: number; tool_calls: number; usage: UsageSummary } = {
    duration_ms: 0,
    llm_calls: 0,
    tool_calls: 0,
    usage: { input_tokens: 0, output_tokens: 0, total_tokens: 0 },
  };

  let min_ms: number | null = null;
  let max_ms: number | null = null;

  for (const ev of events) {
    const rec: any = ev?.record;
    if (!rec || typeof rec !== "object") continue;
    const st = String(rec?.status || "").trim();

    const ms_start = parse_iso_ms(String(rec?.started_at || ""));
    const ms_end = parse_iso_ms(String(rec?.ended_at || ""));
    const ms = ms_end ?? ms_start;
    if (ms !== null) {
      if (min_ms === null || ms < min_ms) min_ms = ms;
      if (max_ms === null || ms > max_ms) max_ms = ms;
    }

    const eff_type = String(rec?.effect?.type || "").trim();
    if (eff_type === "llm_call" && st === "completed") {
      out.llm_calls += 1;
      const res_obj: any = rec?.result && typeof rec.result === "object" ? (rec.result as any) : null;
      const usage =
        (res_obj ? res_obj.usage || res_obj.token_usage || res_obj.tokens : null) ||
        (res_obj && res_obj.output && typeof res_obj.output === "object" ? (res_obj.output as any).usage || (res_obj.output as any).token_usage || (res_obj.output as any).tokens : null) ||
        null;
      const parsed = parse_usage_summary(usage);
      if (parsed) {
        out.usage.input_tokens += parsed.input_tokens;
        out.usage.output_tokens += parsed.output_tokens;
        out.usage.total_tokens += parsed.total_tokens || parsed.input_tokens + parsed.output_tokens;
      }
    }

    if (eff_type === "tool_calls" && st === "completed") {
      const payload = rec?.effect?.payload;
      const calls = payload && typeof payload === "object" ? (payload as any).tool_calls : null;
      if (Array.isArray(calls)) out.tool_calls += calls.length;
    }
  }

  if (min_ms !== null && max_ms !== null) out.duration_ms = Math.max(0, max_ms - min_ms);
  return out;
}

function format_duration_short(ms: number): string {
  const v = Number(ms);
  if (!Number.isFinite(v) || v < 0) return "—";
  if (v < 1000) return `${Math.round(v)}ms`;
  const s = v / 1000;
  if (s < 60) return `${s.toFixed(1)}s`;
  const m = Math.floor(s / 60);
  const rs = Math.round(s % 60);
  return `${m}m${rs}s`;
}

function clamp_text(text: string, max_chars: number): string {
  const s = String(text || "");
  if (s.length <= max_chars) return s;
  return `${s.slice(0, Math.max(0, max_chars - 1))}…`;
}

function format_tool_arg_value_inline(v: any): string {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  if (typeof v === "string") return JSON.stringify(clamp_text(v, 80));
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  if (Array.isArray(v)) return "[…]";
  if (typeof v === "object") return "{…}";
  return JSON.stringify(clamp_text(String(v), 80));
}

function tool_call_signature(name: string, args: any): string {
  const n = String(name || "").trim() || "tool";
  if (args == null) return `${n}()`;
  if (typeof args !== "object" || Array.isArray(args)) return `${n}(${clamp_text(String(args), 120)})`;
  const entries = Object.entries(args as Record<string, any>)
    .map(([k, v]) => [String(k || "").trim(), v] as const)
    .filter(([k]) => Boolean(k))
    .sort(([a], [b]) => a.localeCompare(b));
  const parts: string[] = [];
  for (const [k, v] of entries) {
    if (parts.length >= 3) break;
    parts.push(`${k}=${format_tool_arg_value_inline(v)}`);
  }
  const inside = clamp_text(parts.join(", "), 160);
  return inside ? `${n}(${inside})` : `${n}()`;
}

function extract_tool_signatures(events: LedgerStreamEvent[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const ev of events) {
    const rec: any = ev?.record;
    const eff = rec?.effect;
    if (!eff || typeof eff !== "object") continue;
    if (String(eff.type || "") !== "tool_calls") continue;
    if (String(rec?.status || "") !== "completed") continue;
    const calls = eff?.payload && typeof eff.payload === "object" ? (eff.payload as any).tool_calls : null;
    if (!Array.isArray(calls)) continue;
    for (const c of calls) {
      if (!c || typeof c !== "object") continue;
      const sig = tool_call_signature(String((c as any).name || "").trim(), (c as any).arguments);
      if (!sig || seen.has(sig)) continue;
      seen.add(sig);
      out.push(sig);
    }
  }
  return out;
}

function normalize_ui_event_name(name: string): string {
  const s = String(name || "").trim();
  if (s.startsWith("abstractcode.")) return `abstract.${s.slice("abstractcode.".length)}`;
  return s;
}

function event_name_from_wait_key(wait_key: string): string {
  const wk = String(wait_key || "").trim();
  if (wk.startsWith("evt:")) {
    const parts = wk.split(":", 4);
    if (parts.length === 4 && parts[3]) return String(parts[3]).trim();
  }
  return wk;
}

function is_abstract_status(name: string): boolean {
  const s = normalize_ui_event_name(name);
  return s === "abstract.status";
}

function is_abstract_message(name: string): boolean {
  const s = normalize_ui_event_name(name);
  return s === "abstract.message";
}

function is_abstract_tool_execution(name: string): boolean {
  const s = normalize_ui_event_name(name);
  return s === "abstract.tool_execution";
}

function is_abstract_tool_result(name: string): boolean {
  const s = normalize_ui_event_name(name);
  return s === "abstract.tool_result";
}

function parse_message_payload(payload: any): { level: "info" | "warn" | "error"; title?: string; text: string } | null {
  if (payload == null) return null;
  if (typeof payload === "string") {
    const text = payload.trim();
    if (!text) return null;
    return { level: "info", text };
  }
  if (typeof payload !== "object") {
    const text = String(payload).trim();
    if (!text) return null;
    return { level: "info", text };
  }
  const p: any = payload;
  const level_raw = String(p.level || "info").trim().toLowerCase();
  const level: "info" | "warn" | "error" = level_raw === "error" ? "error" : level_raw === "warn" || level_raw === "warning" ? "warn" : "info";
  const title = String(p.title || "").trim() || undefined;
  const text_raw = p.text ?? p.message ?? p.value ?? "";
  const text = typeof text_raw === "string" ? text_raw.trim() : String(text_raw ?? "").trim();
  if (!text) return null;
  return { level, title, text };
}

function json_fenced(value: any): string {
  try {
    return `\`\`\`json\n${JSON.stringify(value, null, 2)}\n\`\`\``;
  } catch {
    return `\`\`\`\n${String(value ?? "")}\n\`\`\``;
  }
}

function extract_emit_event(rec: StepRecord | null | undefined): { name: string; payload: any; scope?: string } | null {
  const r: any = rec as any;
  const eff = r?.effect;
  if (!eff || typeof eff !== "object") return null;
  if (String(eff.type || "") !== "emit_event") return null;
  const name = String(eff?.payload?.name || eff?.payload?.event_name || "").trim();
  if (!name) return null;
  const payload = eff?.payload?.payload;
  const scope = eff?.payload?.scope;
  return { name, payload, scope: typeof scope === "string" ? scope : undefined };
}

function parse_status_payload(payload: any): { text: string; duration_s: number } {
  if (typeof payload === "string") return { text: payload.trim(), duration_s: -1 };
  if (!payload || typeof payload !== "object") return { text: String(payload ?? "").trim(), duration_s: -1 };
  const text = String((payload as any)?.text ?? (payload as any)?.value ?? "").trim();
  const duration_s_raw = (payload as any)?.duration ?? (payload as any)?.duration_s;
  const duration_s = Number.isFinite(Number(duration_s_raw)) ? Number(duration_s_raw) : -1;
  return { text, duration_s };
}

function extract_flow_end_output(rec: StepRecord | null | undefined): { response: string; meta: any } | null {
  const r: any = rec as any;
  const out0 = r?.result?.output;
  if (typeof out0 === "string") {
    const response = out0.trim();
    if (response) return { response, meta: null };
  }
  if (out0 && typeof out0 === "object") {
    const msg = (out0 as any)?.answer ?? (out0 as any)?.response ?? (out0 as any)?.message ?? (out0 as any)?.text ?? (out0 as any)?.content;
    const response = String(msg ?? "").trim();
    if (response) {
      const meta: any = { ...(out0 as any) };
      delete meta.answer;
      delete meta.response;
      delete meta.message;
      delete meta.text;
      delete meta.content;
      // Avoid persisting huge transcripts in the web UI meta.
      if (Array.isArray(meta.messages)) delete meta.messages;
      return { response, meta };
    }
  }

  const out = r?.result?.output?.result;
  if (!out || typeof out !== "object") return null;
  const msg = out?.response ?? out?.message ?? out?.text ?? out?.content;
  const response = String(msg ?? "").trim();
  if (!response) return null;
  return { response, meta: out?.meta ?? null };
}

function extract_user_prompt_from_run_input(raw: any): string | null {
  if (!raw || typeof raw !== "object") return null;
  const v: any = raw;
  const input = v?.input_data && typeof v.input_data === "object" ? v.input_data : v;

  const candidates = [
    input?.request,
    input?.message,
    input?.prompt,
    input?.task,
    input?.context?.task,
    input?.context?.request,
    input?.context?.message,
  ];
  for (const c of candidates) {
    if (typeof c === "string" && c.trim()) return c.trim();
  }

  const msgs = input?.context?.messages;
  if (Array.isArray(msgs)) {
    for (const m of msgs) {
      if (!m || typeof m !== "object") continue;
      const role = String((m as any).role || "").trim();
      const content = (m as any).content;
      if (role === "user" && typeof content === "string" && content.trim()) return content.trim();
    }
  }
  return null;
}

function extract_context_messages_from_run_input(raw: any): { role: string; content: string; ts: string }[] {
  if (!raw || typeof raw !== "object") return [];
  const v: any = raw;
  const input = v?.input_data && typeof v.input_data === "object" ? v.input_data : v;
  const ctx = input?.context;
  const msgs = ctx && typeof ctx === "object" ? (ctx as any).messages : null;
  if (!Array.isArray(msgs)) return [];

  const out: { role: string; content: string; ts: string }[] = [];
  for (const m of msgs) {
    if (!m || typeof m !== "object") continue;
    const role = String((m as any).role || "").trim();
    if (!role) continue;
    const content = String((m as any).content || "").trim();
    if (!content) continue;
    const ts = String((m as any).timestamp || (m as any).ts || "").trim() || now_iso();
    out.push({ role, content, ts });
  }
  return out;
}

type WorkflowRef = { bundle_id: string; flow_id: string; kind: "bundle" | "visual_react" };

function parse_workflow_ref(workflow_id: string): WorkflowRef | null {
  const wid = String(workflow_id || "").trim();
  if (!wid) return null;
  if (wid.includes(":")) {
    const [bundle_id, flow_id] = wid.split(":", 2);
    if (bundle_id?.trim() && flow_id?.trim()) return { bundle_id: bundle_id.trim(), flow_id: flow_id.trim(), kind: "bundle" };
  }
  const prefix = "visual_react_agent_";
  if (wid.startsWith(prefix)) {
    const rest = wid.slice(prefix.length);
    const parts = rest.split("_");
    if (parts.length >= 2) {
      const bundle_id = String(parts[0] || "").trim();
      const flow_id = String(parts[1] || "").trim();
      if (bundle_id && flow_id) return { bundle_id, flow_id, kind: "visual_react" };
    }
  }
  return null;
}

function infer_agent_template_from_workflow_id(workflow_id: string, templates: AgentTemplate[]): AgentTemplate | null {
  const ref = parse_workflow_ref(workflow_id);
  if (!ref) return null;
  return templates.find((t) => t.bundle_id === ref.bundle_id && t.flow_id === ref.flow_id) || null;
}

async function list_agent_templates(gateway: GatewayClient): Promise<AgentTemplate[]> {
  const res = await gateway.list_bundles();
  const items = Array.isArray(res?.items) ? res.items : [];
  const out: AgentTemplate[] = [];
  for (const b of items) {
    const bid = String((b as any)?.bundle_id || "").trim();
    const eps = Array.isArray((b as any)?.entrypoints) ? (b as any).entrypoints : [];
    for (const ep of eps) {
      const flow_id = String((ep as any)?.flow_id || "").trim();
      if (!bid || !flow_id) continue;
      const interfaces = Array.isArray((ep as any)?.interfaces) ? (ep as any).interfaces.map((x: any) => String(x || "").trim()).filter(Boolean) : [];
      const name = String((ep as any)?.name || "").trim() || `${bid}:${flow_id}`;
      const description = String((ep as any)?.description || "").trim();
      if (!interfaces.includes("abstractcode.agent.v1")) continue;
      out.push({ bundle_id: bid, flow_id, name, description, interfaces });
    }
  }
  out.sort((a, b) => `${a.bundle_id}:${a.flow_id}`.localeCompare(`${b.bundle_id}:${b.flow_id}`));
  return out;
}

export function App(): React.ReactElement {
  const [settings, set_settings] = useState<Settings>(() => load_settings());
  const [route, set_route_state] = useState<Route>(() => parse_route());
  const [session, set_session] = useState<{ session_id: string; state: ReplState }>(() => load_current_repl_session());
  const [pending_attach, set_pending_attach] = useState<{ run_id: string; template: ReplTemplate | null } | null>(null);
  const repl = session.state;
  const session_id = session.session_id;

  const gateway = useMemo(
    () => new GatewayClient({ base_url: settings.gateway_url, auth_token: settings.auth_token }),
    [settings.gateway_url, settings.auth_token]
  );

  useEffect(() => {
    save_settings(settings);
  }, [settings]);

  useEffect(() => {
    save_current_repl_session(session_id, repl);
  }, [session_id, repl]);

  useEffect(() => {
    const on_hash = () => set_route_state(parse_route());
    window.addEventListener("hashchange", on_hash);
    return () => window.removeEventListener("hashchange", on_hash);
  }, []);

  return (
    <div className="app">
      <Header route={route} on_nav={(name) => set_route({ name })} />
      <div className="content">
        {route.name === "console" ? (
          <ConsolePage
            gateway={gateway}
            settings={settings}
            on_settings={set_settings}
            session_id={session_id}
            repl={repl}
            attach_run_id={pending_attach?.run_id || null}
            on_attach_consumed={() => set_pending_attach(null)}
            on_repl={(sid, updater) =>
              set_session((prev) => {
                if (prev.session_id !== sid) return prev;
                return { session_id: sid, state: updater(prev.state) };
              })
            }
          />
        ) : null}
        {route.name === "new" ? (
          <NewChatPage
            gateway={gateway}
            repl={repl}
            on_start={(t) => {
              const created = create_new_repl_session(t);
              set_session({ session_id: created.session_id, state: created.state });
            }}
            on_done={() => set_route({ name: "console" })}
          />
        ) : null}
        {route.name === "sessions" ? (
          <SessionsPage
            gateway={gateway}
            on_open_session={(session_id, run_id, template) => {
              const sid = String(session_id || "").trim() || String(run_id || "").trim();
              if (!sid) return;
              set_session({ session_id: sid, state: reset_repl_state({ template }, sid) });
              set_pending_attach({ run_id, template });
              set_route({ name: "console" });
            }}
          />
        ) : null}
        {route.name === "settings" ? (
          <SettingsPage gateway={gateway} settings={settings} on_change={set_settings} on_done={() => set_route({ name: "console" })} />
        ) : null}
      </div>
    </div>
  );
}

function Header(props: { route: Route; on_nav: (name: Route["name"]) => void }): React.ReactElement {
  const r = props.route;
  const title = r.name === "settings" ? "Settings" : r.name === "new" ? "New Chat" : r.name === "sessions" ? "Sessions" : "AbstractCode";
  return (
    <div className="header">
      <div className="title">{title}</div>
      <div className="nav">
        <button className="btn" onClick={() => props.on_nav("console")} disabled={r.name === "console"}>
          Console
        </button>
        <button className="btn" onClick={() => props.on_nav("new")} disabled={r.name === "new"}>
          New
        </button>
        <button className="btn" onClick={() => props.on_nav("sessions")} disabled={r.name === "sessions"}>
          Sessions
        </button>
        <button className="btn" onClick={() => props.on_nav("settings")} disabled={r.name === "settings"}>
          Settings
        </button>
      </div>
    </div>
  );
}

type RemoteRunSummary = {
  run_id: string;
  workflow_id?: string | null;
  status?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
  session_id?: string | null;
  parent_run_id?: string | null;
  ledger_len?: number | null;
};

function SessionsPage(props: {
  gateway: GatewayClient;
  on_open_session: (session_id: string, run_id: string, template: ReplTemplate | null) => void;
}): React.ReactElement {
  const [templates, set_templates] = useState<AgentTemplate[]>([]);
  const [runs, set_runs] = useState<RemoteRunSummary[]>([]);
  const [loading, set_loading] = useState(false);
  const [error, set_error] = useState("");
  const [attach_id, set_attach_id] = useState("");

  const workflow_to_template = useMemo(() => {
    const m = new Map<string, AgentTemplate>();
    for (const t of templates) m.set(`${t.bundle_id}:${t.flow_id}`, t);
    return m;
  }, [templates]);

  const bundle_to_template = useMemo(() => {
    const m = new Map<string, AgentTemplate>();
    for (const t of templates) {
      if (!m.has(t.bundle_id)) m.set(t.bundle_id, t);
    }
    return m;
  }, [templates]);

  const refresh = async (): Promise<void> => {
    set_loading(true);
    set_error("");
    try {
      const [tpls, r] = await Promise.all([list_agent_templates(props.gateway), props.gateway.list_runs({ limit: 500 })]);
      const items = Array.isArray((r as any)?.items) ? (r as any).items : [];
      set_templates(tpls);

      const allowed_bundles = new Set(tpls.map((t) => t.bundle_id));
      const filtered: RemoteRunSummary[] = items
        .map((it: any) => ({
          run_id: String(it?.run_id || "").trim(),
          workflow_id: typeof it?.workflow_id === "string" ? it.workflow_id : it?.workflow_id ?? null,
          status: typeof it?.status === "string" ? it.status : it?.status ?? null,
          created_at: typeof it?.created_at === "string" ? it.created_at : it?.created_at ?? null,
          updated_at: typeof it?.updated_at === "string" ? it.updated_at : it?.updated_at ?? null,
          session_id: typeof it?.session_id === "string" ? it.session_id : it?.session_id ?? null,
          parent_run_id: typeof it?.parent_run_id === "string" ? it.parent_run_id : it?.parent_run_id ?? null,
          ledger_len: typeof it?.ledger_len === "number" ? it.ledger_len : it?.ledger_len ?? null,
        }))
        .filter((it: RemoteRunSummary) => {
          if (!it.run_id) return false;
          const wid = String(it.workflow_id || "").trim();
          const ref = parse_workflow_ref(wid);
          if (!ref) return false;
          if (ref.kind === "visual_react") return false;
          if (!allowed_bundles.has(ref.bundle_id)) return false;
          // Sessions view is user-facing; hide internal child runs (subflows, listeners, etc).
          if (String(it.parent_run_id || "").trim()) return false;
          return true;
        });

      // Group into durable sessions (session_id) with best-effort fallback to run_id.
      const by_session = new Map<string, RemoteRunSummary>();
      for (const r of filtered) {
        const sid = String(r.session_id || r.run_id).trim();
        if (!sid) continue;
        const prev = by_session.get(sid);
        if (!prev) {
          by_session.set(sid, r);
          continue;
        }
        const ta = parse_iso_ms(String(r.updated_at || r.created_at || "")) ?? 0;
        const tb = parse_iso_ms(String(prev.updated_at || prev.created_at || "")) ?? 0;
        if (ta >= tb) by_session.set(sid, r);
      }

      const sessions = Array.from(by_session.values());
      sessions.sort((a, b) => {
        const ta = parse_iso_ms(String(a.updated_at || a.created_at || "")) ?? 0;
        const tb = parse_iso_ms(String(b.updated_at || b.created_at || "")) ?? 0;
        return tb - ta;
      });
      set_runs(sessions);
    } catch (e: any) {
      set_runs([]);
      set_templates([]);
      set_error(String(e?.message || e || "Failed to load runs"));
    } finally {
      set_loading(false);
    }
  };

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.gateway]);

  return (
    <div className="panel">
      <h2>Sessions</h2>
      <div className="muted">Sessions retrieved from the Gateway (durable runtime source of truth).</div>

      <div style={{ display: "flex", gap: 10, marginTop: 12, alignItems: "center", flexWrap: "wrap" }}>
        <button className="btn" type="button" onClick={() => void refresh()} disabled={loading}>
          {loading ? "Loading…" : "Refresh"}
        </button>

        <div style={{ display: "flex", gap: 8, alignItems: "center", flex: 1, minWidth: 260 }}>
          <input
            className="input mono"
            placeholder="attach run_id…"
            value={attach_id}
            onChange={(e) => set_attach_id(e.target.value)}
            spellCheck={false}
          />
          <button
            className="btn"
            type="button"
            onClick={() =>
              void (async () => {
                const rid = String(attach_id || "").trim();
                if (!rid) return;
                let sid = rid;
                try {
                  const run = await props.gateway.get_run(rid);
                  const s = String(run?.session_id || "").trim();
                  if (s) sid = s;
                } catch {
                  // ignore
                }
                props.on_open_session(sid, rid, null);
              })()
            }
            disabled={!String(attach_id || "").trim()}
          >
            Open
          </button>
        </div>
      </div>

      {error ? (
        <div className="error" style={{ marginTop: 12 }}>
          {error}
        </div>
      ) : null}

      {!loading && !error && !runs.length ? <div className="muted" style={{ marginTop: 10 }}>No sessions yet.</div> : null}

      <div className="list">
        {runs.map((r) => {
          const wid = String(r.workflow_id || "").trim();
          const ref = parse_workflow_ref(wid);
          const key = ref ? `${ref.bundle_id}:${ref.flow_id}` : "";
          const tpl = key ? workflow_to_template.get(key) : undefined;
          const tpl2 = !tpl && ref?.bundle_id ? bundle_to_template.get(ref.bundle_id) : undefined;
          const label = tpl?.name || tpl2?.name || wid || "—";
          const ts = String(r.updated_at || r.created_at || "").trim();
          const status = String(r.status || "").trim();
          const status_cls = status === "failed" ? "danger" : "muted";
          const open_template = ref ? { bundle_id: ref.bundle_id, flow_id: ref.flow_id, name: tpl?.name || tpl2?.name } : null;
          const sid = String(r.session_id || r.run_id).trim();
          return (
            <div key={sid || r.run_id} className="list_item" style={{ display: "flex", justifyContent: "space-between", gap: 10 }}>
              <div style={{ minWidth: 0 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <div className="mono" style={{ fontSize: 13, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                    {sid || r.run_id}
                  </div>
                  {status ? <span className={`pill ${status_cls}`}>{status}</span> : null}
                </div>
                <div className="muted mono" style={{ marginTop: 4 }}>
                  {label} {ts ? `• ${new Date(ts).toLocaleString()}` : ""}
                </div>
                {sid && sid !== r.run_id ? <div className="muted mono" style={{ marginTop: 4 }}>run: {r.run_id}</div> : null}
              </div>
              <div style={{ display: "flex", gap: 8, flexShrink: 0 }}>
                <button
                  className="btn"
                  onClick={() => props.on_open_session(sid || r.run_id, r.run_id, open_template)}
                  type="button"
                >
                  Open
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function SettingsPage(props: { gateway: GatewayClient; settings: Settings; on_change: (s: Settings) => void; on_done: () => void }): React.ReactElement {
  const s = props.settings;
  const [gateway_connected, set_gateway_connected] = useState(false);
  const [gateway_connecting, set_gateway_connecting] = useState(false);
  const [gateway_error, set_gateway_error] = useState("");
  const [providers, set_providers] = useState<any[]>([]);
  const [models, set_models] = useState<string[]>([]);
  const [tools, set_tools] = useState<any[]>([]);
  const [loading_providers, set_loading_providers] = useState(false);
  const [loading_models, set_loading_models] = useState(false);
  const [loading_tools, set_loading_tools] = useState(false);
  const [error_providers, set_error_providers] = useState("");
  const [error_models, set_error_models] = useState("");
  const [error_tools, set_error_tools] = useState("");

  useEffect(() => {
    // Treat gateway settings as “disconnected” until the user explicitly connects.
    set_gateway_connected(false);
    set_gateway_connecting(false);
    set_gateway_error("");
    set_providers([]);
    set_models([]);
    set_tools([]);
    set_loading_providers(false);
    set_loading_models(false);
    set_loading_tools(false);
    set_error_providers("");
    set_error_models("");
    set_error_tools("");
  }, [props.gateway]);

  async function connect_gateway(): Promise<void> {
    set_gateway_connecting(true);
    set_gateway_error("");
    set_loading_providers(true);
    set_loading_tools(true);
    set_error_providers("");
    set_error_tools("");
    try {
      const [prov_res, tool_res] = await Promise.all([props.gateway.discovery_providers(), props.gateway.discovery_tools()]);
      const prov_items = Array.isArray(prov_res?.items) ? prov_res.items : [];
      const tool_items = Array.isArray(tool_res?.items) ? tool_res.items : [];
      set_providers(prov_items);
      set_tools(tool_items);
      set_gateway_connected(true);
    } catch (e: any) {
      set_gateway_error(String(e?.message || e || "Failed to connect to gateway"));
      set_gateway_connected(false);
      set_providers([]);
      set_tools([]);
    } finally {
      set_gateway_connecting(false);
      set_loading_providers(false);
      set_loading_tools(false);
    }
  }

  function disconnect_gateway(): void {
    set_gateway_connected(false);
    set_gateway_error("");
    set_providers([]);
    set_models([]);
    set_tools([]);
    set_error_providers("");
    set_error_models("");
    set_error_tools("");
  }

  // Provider → models.
  useEffect(() => {
    let stopped = false;
    const prov = String(s.provider || "").trim();
    if (!gateway_connected) {
      set_models([]);
      return;
    }
    if (!prov) {
      set_models([]);
      return;
    }
    const run = async () => {
      set_loading_models(true);
      set_error_models("");
      try {
        const res = await props.gateway.discovery_provider_models(prov);
        if (stopped) return;
        if (Array.isArray(res?.models)) {
          set_models(res.models.map((x: any) => String(x || "").trim()).filter(Boolean));
        } else {
          set_models([]);
          if (res?.error) set_error_models(String(res.error));
        }
      } catch (e: any) {
        if (stopped) return;
        set_error_models(String(e?.message || e || "Failed to load models"));
        set_models([]);
      } finally {
        if (!stopped) set_loading_models(false);
      }
    };
    run();
    return () => {
      stopped = true;
    };
  }, [gateway_connected, props.gateway, s.provider]);

  // Auto-default provider/model when discovery loads.
  useEffect(() => {
    if (!providers.length) return;
    const current = String(s.provider || "").trim();
    const names = providers
      .map((p: any) => String(p?.name || "").trim())
      .filter(Boolean);
    if (!names.length) return;
    if (!current || !names.includes(current)) {
      props.on_change({ ...s, provider: names[0], model: "" });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [providers]);

  useEffect(() => {
    if (!models.length) return;
    const current = String(s.model || "").trim();
    if (!current || !models.includes(current)) {
      props.on_change({ ...s, model: models[0] });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [models]);

  // Default tools: select all once when tools are discovered.
  useEffect(() => {
    if (!tools.length) return;
    if (s.tools_initialized) return;
    const names = tools.map((t: any) => String(t?.name || "").trim()).filter(Boolean);
    if (!names.length) return;
    props.on_change({ ...s, tools: names, tools_initialized: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tools]);

  const provider_options = useMemo(() => {
    const items = providers
      .map((p: any) => ({
        name: String(p?.name || "").trim(),
        display_name: String(p?.display_name || p?.displayName || p?.name || "").trim(),
      }))
      .filter((p: any) => Boolean(p.name));
    items.sort((a: any, b: any) => a.display_name.localeCompare(b.display_name));
    return items;
  }, [providers]);

  const tool_options = useMemo(() => {
    const names = tools.map((t: any) => String(t?.name || "").trim()).filter(Boolean);
    names.sort((a, b) => a.localeCompare(b));
    return names;
  }, [tools]);

  return (
    <div className="panel">
      <h2>Gateway</h2>
      <div className="field">
        <label>Gateway URL</label>
        <div className="row" style={{ marginTop: 0 }}>
          <input
            style={{ flex: 1, minWidth: 0 }}
            value={s.gateway_url}
            onChange={(e) => props.on_change({ ...s, gateway_url: e.target.value })}
            placeholder="http://127.0.0.1:8080"
          />
          <button
            className="btn"
            type="button"
            onClick={gateway_connected ? disconnect_gateway : () => void connect_gateway()}
            disabled={gateway_connecting}
          >
            {gateway_connecting ? "Connecting…" : gateway_connected ? "Disconnect" : "Connect"}
          </button>
        </div>
        {gateway_error ? <div className="error">{gateway_error}</div> : null}
      </div>
      <div className="field">
        <label>Auth token</label>
        <input
          type="password"
          value={s.auth_token}
          onChange={(e) => props.on_change({ ...s, auth_token: e.target.value })}
          placeholder="Bearer token (optional)"
        />
      </div>
      <div className="field">
        <label>Client id</label>
        <input value={s.client_id} onChange={(e) => props.on_change({ ...s, client_id: e.target.value })} placeholder="abstractcode_web" />
      </div>

      <h2 style={{ marginTop: 18 }}>Model</h2>
      <div className="field">
        <label>Provider</label>
        <select
          className="mono"
          value={s.provider}
          onChange={(e) => props.on_change({ ...s, provider: e.target.value, model: "" })}
          disabled={!gateway_connected || loading_providers || !provider_options.length}
        >
          {!gateway_connected ? <option value="">(click Connect)</option> : null}
          {gateway_connected && !provider_options.length ? <option value="">(no providers)</option> : null}
          {provider_options.map((p) => (
            <option key={p.name} value={p.name}>
              {p.display_name || p.name}
            </option>
          ))}
        </select>
        {loading_providers ? <div className="muted">Loading providers…</div> : null}
        {error_providers ? <div className="error">{error_providers}</div> : null}
      </div>
      <div className="field">
        <label>Model</label>
        <select
          className="mono"
          value={s.model}
          onChange={(e) => props.on_change({ ...s, model: e.target.value })}
          disabled={!gateway_connected || !s.provider || loading_models || !models.length}
        >
          {!gateway_connected ? <option value="">(click Connect)</option> : null}
          {!s.provider ? <option value="">(select provider first)</option> : null}
          {s.provider && !models.length ? <option value="">(no models)</option> : null}
          {models.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
        {loading_models ? <div className="muted">Loading models…</div> : null}
        {error_models ? <div className="error">{error_models}</div> : null}
      </div>
      <div className="field">
        <label>Max iterations</label>
        <input
          value={String(s.max_iterations)}
          onChange={(e) => props.on_change({ ...s, max_iterations: Number.isFinite(Number(e.target.value)) ? Number(e.target.value) : 20 })}
          placeholder="20"
        />
      </div>
      <div className="field">
        <label>Temperature</label>
        <input
          value={String(s.temperature)}
          onChange={(e) => props.on_change({ ...s, temperature: Number.isFinite(Number(e.target.value)) ? Number(e.target.value) : 0.7 })}
          placeholder="0.7"
        />
      </div>
      <div className="field">
        <label>Seed</label>
        <input
          value={String(s.seed)}
          onChange={(e) => props.on_change({ ...s, seed: Number.isFinite(Number(e.target.value)) ? Number(e.target.value) : -1 })}
          placeholder="-1"
        />
        <div className="muted">-1 = random/unset; ≥ 0 = deterministic (provider permitting)</div>
      </div>
      <div className="field">
        <label>Tools allowlist</label>
        <MultiSelect
          options={tool_options}
          value={s.tools}
          placeholder={!gateway_connected ? "(click Connect)" : "(none)"}
          disabled={!gateway_connected || loading_tools || !tool_options.length}
          onChange={(next) => props.on_change({ ...s, tools: next, tools_initialized: true })}
        />
        {loading_tools ? <div className="muted">Loading tools…</div> : null}
        {error_tools ? <div className="error">{error_tools}</div> : null}
      </div>

      <div className="actions">
        <button className="btn primary" onClick={() => props.on_done()}>
          Done
        </button>
      </div>
    </div>
  );
}

function NewChatPage(props: { gateway: GatewayClient; repl: ReplState; on_start: (t: ReplTemplate | null) => void; on_done: () => void }): React.ReactElement {
  const [templates, set_templates] = useState<AgentTemplate[]>([]);
  const [loading, set_loading] = useState(false);
  const [error, set_error] = useState("");
  const [selected, set_selected] = useState<AgentTemplate | null>(null);

  useEffect(() => {
    let stopped = false;
    const run = async () => {
      set_loading(true);
      set_error("");
      try {
        const items = await list_agent_templates(props.gateway);
        if (stopped) return;
        set_templates(items);
        const cur = props.repl.template;
        if (cur && items.some((t) => t.bundle_id === cur.bundle_id && t.flow_id === cur.flow_id)) {
          const t = items.find((x) => x.bundle_id === cur.bundle_id && x.flow_id === cur.flow_id) || null;
          set_selected(t);
        } else {
          set_selected(items.find((t) => t.bundle_id === "basic-agent") || items[0] || null);
        }
      } catch (e: any) {
        if (stopped) return;
        set_error(String(e?.message || e || "Failed to load agents"));
      } finally {
        if (!stopped) set_loading(false);
      }
    };
    run();
    return () => {
      stopped = true;
    };
  }, [props.gateway]);

  function start(): void {
    const t = selected;
    if (!t) return;
    props.on_start({ bundle_id: t.bundle_id, flow_id: t.flow_id, name: t.name });
    props.on_done();
  }

  return (
    <div className="panel">
      <h2>Agents</h2>
      <div className="muted">Pick an agent workflow (must implement `abstractcode.agent.v1`).</div>
      {loading ? <div className="muted" style={{ marginTop: 10 }}>Loading…</div> : null}
      {error ? <div className="error">{error}</div> : null}

      <div className="grid">
        {templates.slice(0, 200).map((t) => {
          const active = selected?.bundle_id === t.bundle_id && selected?.flow_id === t.flow_id;
          return (
            <button key={`${t.bundle_id}:${t.flow_id}`} className={`card ${active ? "active" : ""}`} onClick={() => set_selected(t)}>
              <div className="card_title">
                <span className="mono">{t.name}</span>
                <span className="pill">agent</span>
              </div>
              <div className="muted mono">
                {t.bundle_id}:{t.flow_id}
              </div>
              {t.description ? <div className="card_desc">{t.description}</div> : null}
            </button>
          );
        })}
      </div>

      <div className="actions">
        <button className="btn primary" disabled={!selected} onClick={() => start()}>
          Start new chat
        </button>
      </div>
    </div>
  );
}

function ConsolePage(props: {
  gateway: GatewayClient;
  settings: Settings;
  on_settings: (next: Settings) => void;
  session_id: string;
  repl: ReplState;
  attach_run_id: string | null;
  on_attach_consumed: () => void;
  on_repl: (session_id: string, updater: (prev: ReplState) => ReplState) => void;
}): React.ReactElement {
  const [templates, set_templates] = useState<AgentTemplate[]>([]);
  const [template_error, set_template_error] = useState("");
  const [model_caps, set_model_caps] = useState<any>(null);
  const [model_caps_error, set_model_caps_error] = useState("");

  const [composer, set_composer] = useState("");
  const [composer_cursor, set_composer_cursor] = useState(0);
  const [error, set_error] = useState("");
  const [cmd_active, set_cmd_active] = useState(0);
  const [file_active, set_file_active] = useState(0);

  const [active_run_id, set_active_run_id] = useState<string | null>(null);
  const [records, set_records] = useState<LedgerStreamEvent[]>([]);
  const records_ref = useRef<LedgerStreamEvent[]>([]);
  const [status_text, set_status_text] = useState<string>("");
  const status_timer_ref = useRef<number | null>(null);

  const [details_open, set_details_open] = useState(false);
  const [resuming, set_resuming] = useState(false);

  const input_ref = useRef<HTMLTextAreaElement | null>(null);
  const [attached_files, set_attached_files] = useState<AttachedFile[]>([]);
  const pending_files = attached_files.some((f) => f.loading);
  const [file_matches, set_file_matches] = useState<string[]>([]);
  const [file_loading, set_file_loading] = useState(false);
  const [file_error, set_file_error] = useState("");

  const abort_ref = useRef<AbortController | null>(null);
  const cursor_ref = useRef<number>(0);
  const seen_cursors_ref = useRef<Set<number>>(new Set());
  const seen_wait_keys_ref = useRef<Set<string>>(new Set());
  const seen_tool_call_ids_ref = useRef<Set<string>>(new Set());

  const last_record: StepRecord | null = records.length ? records[records.length - 1].record : null;
  const wait_state: WaitState | null = useMemo(() => extract_wait_from_record(last_record), [last_record]);
  const tool_calls_for_wait: ToolCall[] = useMemo(() => extract_tool_calls_from_wait(wait_state), [wait_state]);
  const wait_reason = String(wait_state?.reason || "").trim();
  const wait_key = String(wait_state?.wait_key || "").trim();
  const wait_event_name = wait_reason === "event" ? normalize_ui_event_name(event_name_from_wait_key(wait_key)) : "";
  const is_user_wait = wait_reason === "user";
  const is_ask_event_wait = wait_reason === "event" && wait_event_name === "abstract.ask";
  const can_user_answer_wait = is_user_wait || is_ask_event_wait;
  const is_working = Boolean(active_run_id) && !wait_state && !resuming;

  const repl_template = props.repl.template;
  const template_label = repl_template?.name || (repl_template ? `${repl_template.bundle_id}:${repl_template.flow_id}` : "");

  useEffect(() => {
    const rid = String(props.attach_run_id || "").trim();
    if (!rid) return;
    props.on_attach_consumed();
    set_attached_files([]);
    set_file_matches([]);
    set_file_error("");
    set_file_loading(false);
    set_composer("");
    set_error("");
    clear_status();
    if (rid !== active_run_id) set_active_run_id(rid);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.attach_run_id]);

  function update_repl(updater: (prev: ReplState) => ReplState): void {
    props.on_repl(props.session_id, updater);
  }

  useEffect(() => {
    let stopped = false;
    const model = String(props.settings.model || "").trim();
    if (!model) {
      set_model_caps(null);
      set_model_caps_error("");
      return;
    }
    const run = async () => {
      set_model_caps_error("");
      try {
        const res = await props.gateway.discovery_model_capabilities(model);
        if (stopped) return;
        set_model_caps(res);
        if (res?.error) set_model_caps_error(String(res.error));
      } catch (e: any) {
        if (stopped) return;
        set_model_caps(null);
        set_model_caps_error(String(e?.message || e || "Failed to load model capabilities"));
      }
    };
    run();
    return () => {
      stopped = true;
    };
  }, [props.gateway, props.settings.provider, props.settings.model]);

  const context_meter = useMemo(() => {
    const history = (props.repl.messages || []).filter((m) => m.role === "user" || m.role === "assistant");
    const joined = history.map((m) => `${m.role}: ${m.content}`).join("\n\n");
    const next_text = composer.trim() ? `${joined}\n\nuser: ${composer.trim()}` : joined;
    const files_text = attached_files
      .filter((f) => !f.loading && !f.error && typeof f.content === "string" && f.content.trim())
      .map((f) => String(f.content || "").trim())
      .join("\n\n");
    const next_with_files = files_text ? `${next_text}\n\nsystem: ${files_text}` : next_text;
    const used = Math.max(0, Math.ceil(next_with_files.length / 4));
    const caps = model_caps && typeof model_caps === "object" ? (model_caps as any).capabilities : null;
    const max_tokens = caps && typeof caps === "object" ? Number((caps as any).max_tokens ?? 0) : 0;
    const pct = max_tokens > 0 ? (used / max_tokens) * 100 : 0;
    return { used, max_tokens: max_tokens > 0 ? max_tokens : null, pct };
  }, [props.repl.messages, composer, model_caps, attached_files]);

  useEffect(() => {
    let stopped = false;
    const run = async () => {
      set_template_error("");
      try {
        const items = await list_agent_templates(props.gateway);
        if (stopped) return;
        set_templates(items);
        if (props.repl.template) {
          const cur = props.repl.template;
          if (cur && !items.some((t) => t.bundle_id === cur.bundle_id && t.flow_id === cur.flow_id)) {
            set_template_error("Selected agent not found on gateway. Pick a new one in New.");
          }
        }
      } catch (e: any) {
        if (stopped) return;
        set_template_error(String(e?.message || e || "Failed to load agents"));
      }
    };
    run();
    return () => {
      stopped = true;
    };
  }, [props.gateway]);

  // Only auto-select a default template when the user is starting a fresh chat,
  // not when attaching to an existing run (where the agent must be inferred from workflow_id).
  useEffect(() => {
    if (props.repl.template) return;
    if (active_run_id) return;
    if ((props.repl.messages || []).length) return;
    const def = templates.find((t) => t.bundle_id === "basic-agent") || templates[0] || null;
    if (!def) return;
    update_repl((prev) => ({ ...prev, template: { bundle_id: def.bundle_id, flow_id: def.flow_id, name: def.name }, updated_at: now_iso() }));
  }, [templates, props.repl.template, active_run_id, props.repl.messages]);

  function clear_status(): void {
    set_status_text("");
    if (status_timer_ref.current !== null) {
      window.clearTimeout(status_timer_ref.current);
      status_timer_ref.current = null;
    }
  }

  function set_status(text: string, duration_s: number): void {
    const t = String(text || "").trim();
    if (!t || t.toLowerCase() === "completed" || t.toLowerCase() === "done") {
      clear_status();
      return;
    }
    set_status_text(t);
    if (status_timer_ref.current !== null) {
      window.clearTimeout(status_timer_ref.current);
      status_timer_ref.current = null;
    }
    if (Number.isFinite(duration_s) && duration_s > 0) {
      status_timer_ref.current = window.setTimeout(() => clear_status(), Math.floor(duration_s * 1000));
    }
  }

  function append_message(m: ReplMessage): void {
    update_repl((prev) => ({
      ...prev,
      messages: [...(prev.messages || []), m].slice(-200),
      updated_at: now_iso(),
    }));
  }

  function append_tool_blocks_from_effect(rec: StepRecord): void {
    const payload: any = (rec as any)?.effect?.payload;
    if (!payload || typeof payload !== "object") return;
    const calls = Array.isArray(payload.tool_calls) ? payload.tool_calls : [];
    if (!calls.length) return;

    const result: any = (rec as any)?.result;
    const results = result && typeof result === "object" ? result.results : null;
    const res_list = Array.isArray(results) ? results : [];
    const by_call_id = new Map<string, any>();
    for (const r of res_list) {
      if (!r || typeof r !== "object") continue;
      const cid = String((r as any).call_id || (r as any).id || "").trim();
      if (cid) by_call_id.set(cid, r);
    }

    const MAX_OUTPUT_CHARS = 4000;

    for (let i = 0; i < calls.length; i++) {
      const c: any = calls[i];
      if (!c || typeof c !== "object") continue;
      const name = String(c.name || "").trim();
      const call_id = String(c.call_id || c.id || "").trim();
      if (call_id && seen_tool_call_ids_ref.current.has(call_id)) continue;
      if (call_id) seen_tool_call_ids_ref.current.add(call_id);
      const args = c.arguments;
      const r = (call_id && by_call_id.has(call_id) ? by_call_id.get(call_id) : null) || (res_list[i] as any) || null;
      const success = r && typeof r === "object" ? Boolean((r as any).success) : null;
      const error = r && typeof r === "object" ? String((r as any).error || "").trim() : "";
      const output_raw = r && typeof r === "object" ? ((r as any).output ?? (r as any).result ?? null) : null;
      const output_s = output_raw == null ? "" : typeof output_raw === "string" ? output_raw : safe_json(output_raw);
      const output_preview = output_s.length > MAX_OUTPUT_CHARS ? `${output_s.slice(0, MAX_OUTPUT_CHARS)}\n…(truncated)…` : output_s;

      append_message({
        role: "system",
        level: error ? "error" : success === false ? "warn" : "info",
        title: name ? `Tool: ${name}` : "Tool",
        content: "",
        ts: now_iso(),
        meta: {
          _kind: "tool",
          tool: {
            name,
            call_id: call_id || undefined,
            success,
            error: error || undefined,
            arguments: args,
            output_preview,
          },
        },
      });
    }
  }

  function build_input_data(request: string): Record<string, any> {
    const tools = parse_tools_allowlist(props.settings.tools);
    const history_msgs: ReplMessage[] = (props.repl.messages || []).filter((m) => m.role === "user" || m.role === "assistant" || m.role === "system");
    const ctx_messages: { role: string; content: string }[] = history_msgs
      .filter((m) => m.role === "user" || m.role === "assistant")
      .map((m) => ({ role: m.role, content: m.content }));

    // Durable, agent-consumable context (structured) so workflows can remain stateful
    // even when the UI reloads and has to reconstruct history from runs.
    const turns: any[] = [];
    let pending_request: string | null = null;
    let pending_tools: any[] = [];
    for (const m of history_msgs) {
      if (m.role === "user") {
        pending_request = String(m.content || "");
        pending_tools = [];
        continue;
      }
      if (m.role === "system") {
        const kind = String(m.meta?._kind || "").trim();
        if (kind === "tool") {
          const tool_meta = m.meta?.tool;
          const name = String(tool_meta?.name || "").trim();
          const args = tool_meta?.arguments;
          const ok = typeof tool_meta?.success === "boolean" ? tool_meta.success : null;
          const entry: any = { name: name || "tool", ok };
          if (args && typeof args === "object") {
            const path = typeof (args as any).path === "string" ? String((args as any).path).trim() : "";
            const query = typeof (args as any).query === "string" ? String((args as any).query).trim() : "";
            const command = typeof (args as any).command === "string" ? String((args as any).command).trim() : "";
            if (path) entry.path = path;
            if (query) entry.query = query;
            if (command) entry.command = command;
          }
          pending_tools.push(entry);
        }
        continue;
      }
      if (m.role === "assistant") {
        if (pending_request) {
          const stats = m.meta && typeof m.meta === "object" ? (m.meta as any)._repl : null;
          turns.push({
            request: pending_request,
            answer: String(m.content || ""),
            run_id: m.run_id || null,
            stats: stats && typeof stats === "object" ? stats : null,
            tools: pending_tools.slice(0, 50),
          });
          pending_request = null;
          pending_tools = [];
        }
      }
    }

    let last_run_id: string | null = null;
    for (let i = history_msgs.length - 1; i >= 0; i--) {
      const m = history_msgs[i];
      if (m.role !== "assistant") continue;
      const rid = String(m.run_id || "").trim();
      if (rid) {
        last_run_id = rid;
        break;
      }
    }
    const file_contents = attached_files
      .filter((f) => !f.loading && !f.error && typeof f.content === "string" && f.content.trim())
      .map((f) => String(f.content || "").trim());
    const file_labels = attached_files
      .filter((f) => !f.loading && !f.error && typeof f.content === "string" && f.content.trim())
      .map((f) => `@${String(f.path || "").trim()}`)
      .filter(Boolean);
    if (file_contents.length) {
      const header = file_labels.length ? `Attached files: ${file_labels.join(", ")}` : "Attached files:";
      ctx_messages.push({ role: "system", content: `${header}\n\n${file_contents.join("\n\n")}` });
    }

    if (turns.length) {
      const recent = turns.slice(-3);
      const lines: string[] = [];
      for (let i = 0; i < recent.length; i++) {
        const t = recent[i] || {};
        const tools_used = Array.isArray((t as any).tools) ? ((t as any).tools as any[]) : [];
        const tool_names: string[] = [];
        const file_paths: string[] = [];
        const commands: string[] = [];
        for (const tc of tools_used) {
          if (!tc || typeof tc !== "object") continue;
          const name = String((tc as any).name || "").trim();
          if (name && !tool_names.includes(name)) tool_names.push(name);
          const path = String((tc as any).path || "").trim();
          if (path && !file_paths.includes(path)) file_paths.push(path);
          const cmd = String((tc as any).command || "").trim();
          if (cmd && !commands.includes(cmd)) commands.push(cmd);
        }
        const tools_part = tool_names.length ? tool_names.slice(0, 8).join(", ") : "—";
        const files_part = file_paths.length ? `; files: ${file_paths.slice(0, 6).join(", ")}` : "";
        const cmd_part = commands.length ? `; commands: ${commands.slice(0, 3).join(" | ")}` : "";
        lines.push(`- turn ${Math.max(1, turns.length - recent.length + i + 1)}: tools: ${tools_part}${files_part}${cmd_part}`);
      }
      const digest = `Recent tool activity (auto):\n${lines.join("\n")}`;
      ctx_messages.push({ role: "system", content: digest.length > 1500 ? `${digest.slice(0, 1500)}…` : digest });
    }

    return {
      request,
      provider: props.settings.provider || null,
      model: props.settings.model || null,
      tools: tools.length ? tools : null,
      context: {
        messages: ctx_messages,
        session: {
          id: String(props.session_id || "").trim() || null,
          last_run_id,
          turns: turns.slice(-12),
        },
      },
      max_iterations: Number.isFinite(Number(props.settings.max_iterations)) ? Number(props.settings.max_iterations) : 20,
      temperature: Number.isFinite(Number(props.settings.temperature)) ? Number(props.settings.temperature) : 0.7,
      seed: Number.isFinite(Number(props.settings.seed)) ? Number(props.settings.seed) : -1,
    };
  }

  async function start_turn(text: string): Promise<void> {
    const t = String(text || "").trim();
    if (!t) return;
    if (active_run_id) return;
    if (pending_files) {
      set_error("Wait for attached files to finish loading.");
      return;
    }
    if (!props.repl.template) {
      set_error("Pick an agent in New.");
      return;
    }
    if (!props.settings.provider.trim() || !props.settings.model.trim()) {
      set_error("Set provider + model in Settings.");
      return;
    }

    set_error("");
    clear_status();
    records_ref.current = [];
    set_records([]);
    cursor_ref.current = 0;
    seen_cursors_ref.current = new Set();
    seen_wait_keys_ref.current = new Set();
    seen_tool_call_ids_ref.current = new Set();
    seen_wait_keys_ref.current = new Set();
    seen_tool_call_ids_ref.current = new Set();

    append_message({ role: "user", content: t, ts: now_iso() });

    const attach_errors = attached_files.filter((f) => !f.loading && String(f.error || "").trim());
    if (attach_errors.length) {
      append_message({
        role: "system",
        level: "warn",
        title: "Attachments",
        content: ["Some attached files could not be included:", ...attach_errors.map((f) => `- @${f.path}: ${f.error}`)].join("\n"),
        ts: now_iso(),
      });
    }

    set_status("working…", -1);
    const input_data = build_input_data(t);
    try {
      const run_id = await props.gateway.start_run(props.repl.template.flow_id, input_data, {
        bundle_id: props.repl.template.bundle_id,
        session_id: String(props.session_id || "").trim() || undefined,
      });
      set_active_run_id(run_id);
      set_attached_files([]);
    } catch (e: any) {
      clear_status();
      set_error(String(e?.message || e || "Failed to start run"));
    }
  }

  function stop_stream(): void {
    if (abort_ref.current) abort_ref.current.abort();
    abort_ref.current = null;
  }

  function finish_run_with_response(resp: { response: string; meta: any }, run_id: string): void {
    const stats = compute_run_stats(records_ref.current);
    const tool_sigs = extract_tool_signatures(records_ref.current);
    const meta_obj: any = {};
    if (resp.meta !== null && resp.meta !== undefined) meta_obj.workflow_meta = resp.meta;
    meta_obj._repl = {
      duration_ms: stats.duration_ms,
      llm_calls: stats.llm_calls,
      tool_calls: stats.tool_calls,
      usage: stats.usage,
      tok_s: stats.duration_ms > 0 ? stats.usage.total_tokens / (stats.duration_ms / 1000) : null,
    };
    append_message({ role: "assistant", content: resp.response, ts: now_iso(), meta: meta_obj, run_id });

    const digest_lines: string[] = [];
    digest_lines.push(`**outcome:** completed`);
    digest_lines.push(`- duration: ${format_duration_short(stats.duration_ms)}`);
    digest_lines.push(`- llm calls: ${stats.llm_calls}`);
    digest_lines.push(`- tool calls: ${stats.tool_calls}`);
    digest_lines.push(
      stats.usage.total_tokens
        ? `- tokens: in ${stats.usage.input_tokens} • out ${stats.usage.output_tokens} • total ${stats.usage.total_tokens}`
        : `- tokens: —`
    );
    digest_lines.push(
      meta_obj._repl.tok_s != null && Number.isFinite(Number(meta_obj._repl.tok_s)) ? `- speed: ${Number(meta_obj._repl.tok_s).toFixed(1)} tok/s` : `- speed: —`
    );
    if (tool_sigs.length) {
      digest_lines.push("");
      digest_lines.push("**tools used:**");
      digest_lines.push(...tool_sigs.slice(0, 40).map((s) => `- \`${s}\``));
      if (tool_sigs.length > 40) digest_lines.push(`- …and ${tool_sigs.length - 40} more`);
    }

    append_message({
      role: "system",
      level: "info",
      title: "Digest",
      ts: now_iso(),
      run_id,
      content: digest_lines.join("\n"),
    });
    clear_status();
    stop_stream();
    set_active_run_id(null);
  }

  function finish_run_without_output(outcome: "completed" | "failed" | "cancelled", run_id: string): void {
    const stats = compute_run_stats(records_ref.current);
    const tool_sigs = extract_tool_signatures(records_ref.current);

    const digest_lines: string[] = [];
    digest_lines.push(`Run ${outcome}.`);
    digest_lines.push("");
    digest_lines.push(`**digest:**`);
    digest_lines.push(`- duration: ${format_duration_short(stats.duration_ms)}`);
    digest_lines.push(`- llm calls: ${stats.llm_calls}`);
    digest_lines.push(`- tool calls: ${stats.tool_calls}`);
    digest_lines.push(
      stats.usage.total_tokens
        ? `- tokens: in ${stats.usage.input_tokens} • out ${stats.usage.output_tokens} • total ${stats.usage.total_tokens}`
        : `- tokens: —`
    );
    digest_lines.push(
      stats.duration_ms > 0 && stats.usage.total_tokens > 0 ? `- speed: ${(stats.usage.total_tokens / (stats.duration_ms / 1000)).toFixed(1)} tok/s` : `- speed: —`
    );
    if (tool_sigs.length) {
      digest_lines.push("");
      digest_lines.push("**tools used:**");
      digest_lines.push(...tool_sigs.slice(0, 40).map((s) => `- \`${s}\``));
      if (tool_sigs.length > 40) digest_lines.push(`- …and ${tool_sigs.length - 40} more`);
    }

    append_message({
      role: "system",
      level: outcome === "failed" ? "error" : outcome === "cancelled" ? "warn" : "info",
      title: outcome.toUpperCase(),
      ts: now_iso(),
      run_id,
      content: digest_lines.join("\n"),
    });
    clear_status();
    stop_stream();
    set_active_run_id(null);
  }

  function handle_record(ev: LedgerStreamEvent): void {
    const rec = ev.record as StepRecord;
    if (seen_cursors_ref.current.has(ev.cursor)) return;
    seen_cursors_ref.current.add(ev.cursor);
    cursor_ref.current = Math.max(cursor_ref.current, ev.cursor);
    records_ref.current = [...records_ref.current, ev].slice(-2000);
    set_records(records_ref.current);

    const st = String((rec as any)?.status || "").trim();
    const eff_type = String((rec as any)?.effect?.type || "").trim();

    const emit = extract_emit_event(rec);
    if (emit && is_abstract_status(emit.name)) {
      const { text, duration_s } = parse_status_payload(emit.payload);
      set_status(text, duration_s);
    }
    if (emit && is_abstract_message(emit.name)) {
      const parsed = parse_message_payload(emit.payload);
      if (parsed) {
        append_message({ role: "system", content: parsed.text, ts: now_iso(), level: parsed.level, title: parsed.title });
      }
    }
    if (emit && is_abstract_tool_execution(emit.name)) {
      const items = Array.isArray(emit.payload) ? emit.payload : emit.payload != null ? [emit.payload] : [];
      for (const it of items.slice(0, 30)) {
        const tool = String((it as any)?.tool || (it as any)?.name || "").trim();
        const call_id = String((it as any)?.call_id || (it as any)?.id || "").trim();
        const args = (it as any)?.arguments ?? (it as any)?.args ?? (it as any)?.params ?? (it as any)?.parameters ?? null;
        append_message({
          role: "system",
          level: "info",
          title: tool ? `Tool: ${tool}` : "Tool",
          content: "",
          ts: now_iso(),
          meta: {
            _kind: "tool",
            tool: {
              name: tool || undefined,
              call_id: call_id || undefined,
              arguments: args ?? undefined,
              output_preview: "",
              pending: true,
            },
          },
        });
      }
    }
    if (emit && is_abstract_tool_result(emit.name)) {
      const items = Array.isArray(emit.payload) ? emit.payload : emit.payload != null ? [emit.payload] : [];
      for (const it of items.slice(0, 30)) {
        const tool = String((it as any)?.tool || (it as any)?.name || "").trim();
        const call_id = String((it as any)?.call_id || (it as any)?.id || "").trim();
        const args = (it as any)?.arguments ?? (it as any)?.args ?? (it as any)?.params ?? (it as any)?.parameters ?? null;
        const success_raw = (it as any)?.success;
        const success = typeof success_raw === "boolean" ? success_raw : null;
        const err = String((it as any)?.error || "").trim();
        const output_raw = (it as any)?.output ?? (it as any)?.result ?? (it as any)?.response ?? (it as any)?.value ?? null;
        const output_s = output_raw == null ? "" : typeof output_raw === "string" ? output_raw : safe_json(output_raw);
        const output_preview = output_s.length > 4000 ? `${output_s.slice(0, 4000)}\n…(truncated)…` : output_s;
        append_message({
          role: "system",
          level: err ? "error" : success === false ? "warn" : "info",
          title: tool ? `Tool: ${tool}` : "Tool",
          content: "",
          ts: now_iso(),
          meta: {
            _kind: "tool",
            tool: {
              name: tool || undefined,
              call_id: call_id || undefined,
              arguments: args ?? undefined,
              output_preview,
              success,
              error: err || undefined,
            },
          },
        });
      }
    }

    if (eff_type === "tool_calls" && st === "completed") {
      append_tool_blocks_from_effect(rec);
    }

    if (eff_type === "answer_user" && st === "completed") {
      const payload: any = (rec as any)?.effect?.payload;
      const result: any = (rec as any)?.result;
      const msg_raw = (result && typeof result === "object" ? (result as any).message : null) ?? payload?.message ?? payload?.text ?? payload?.content ?? "";
      const text = String(msg_raw ?? "").trim();
      if (text) {
        const level_raw = (result && typeof result === "object" ? (result as any).level : null) ?? payload?.level ?? "message";
        const level_s = String(level_raw || "").trim().toLowerCase();
        const lvl: "info" | "warn" | "error" = level_s === "error" ? "error" : level_s === "warning" || level_s === "warn" ? "warn" : "info";
        const title = lvl === "error" ? "ERROR" : lvl === "warn" ? "WARNING" : "MESSAGE";
        append_message({ role: "system", content: text, ts: now_iso(), level: lvl, title });
      }
    }

    if (String(rec?.status || "").trim() === "waiting") {
      const w = extract_wait_from_record(rec);
      const reason = String((w as any)?.reason || "").trim();
      const wk = String((w as any)?.wait_key || "").trim();
      const prompt = String((w as any)?.prompt || "").trim();
      const details = (w as any)?.details;
      const tool_calls = details && typeof details === "object" ? (details as any)?.tool_calls : null;
      const is_tool_wait = Array.isArray(tool_calls) && tool_calls.length > 0;
      const ev_name = reason === "event" ? normalize_ui_event_name(event_name_from_wait_key(wk)) : "";
      const is_ask_wait = reason === "event" && ev_name === "abstract.ask";
      const is_user_wait = reason === "user";

      if (wk && prompt && (is_user_wait || is_ask_wait) && !is_tool_wait && !seen_wait_keys_ref.current.has(wk)) {
        clear_status(); // matches AbstractCode UX (spinner clears when awaiting user input)
        seen_wait_keys_ref.current.add(wk);
        append_message({ role: "assistant", content: prompt, ts: now_iso(), meta: { kind: "ask", wait_key: wk } });
      }
    }

    const out = extract_flow_end_output(rec);
    if (out && active_run_id) {
      finish_run_with_response(out, active_run_id);
    }

    if (st === "failed" && active_run_id) {
      const err = String((rec as any)?.error || (rec as any)?.result?.error || "step failed").trim();
      append_message({ role: "assistant", content: `Error: ${err}`, ts: now_iso(), run_id: active_run_id });
      finish_run_without_output("failed", active_run_id);
    }
  }

  useEffect(() => {
    const rid = String(active_run_id || "").trim();
    if (!rid) return;

    let stopped = false;
    set_error("");
    clear_status();
    records_ref.current = [];
    set_records([]);
    cursor_ref.current = 0;
    seen_cursors_ref.current = new Set();
    seen_wait_keys_ref.current = new Set();
    seen_tool_call_ids_ref.current = new Set();

    if (abort_ref.current) abort_ref.current.abort();
    abort_ref.current = new AbortController();

    const append_page = async (after: number): Promise<void> => {
      const page = await props.gateway.get_ledger(rid, { after, limit: 2000 });
      const items = Array.isArray(page.items) ? page.items : [];
      const start_cursor = after + 1;
      for (let i = 0; i < items.length; i++) {
        const rec = items[i] as StepRecord;
        handle_record({ cursor: start_cursor + i, record: rec });
      }
      const next = typeof page.next_after === "number" ? page.next_after : after + items.length;
      cursor_ref.current = Math.max(cursor_ref.current, next);
    };

    const run = async () => {
      try {
        // If attaching to an existing run (Sessions/Open), seed the UI with the original user prompt.
        //
        // Note: the durable ledger for subworkflow runs may only contain an "enriched request".
        // The original user request is typically stored in the root parent run input_data.
        if (!props.repl.messages.length) {
          try {
            let current_run: any = null;
            try {
              current_run = await props.gateway.get_run(rid);
            } catch {
              current_run = null;
            }
            if (stopped) return;

            const workflow_id = String(current_run?.workflow_id || "").trim();
            const ref = parse_workflow_ref(workflow_id);
            if (ref) {
              const match = infer_agent_template_from_workflow_id(workflow_id, templates);
              update_repl((prev) => ({
                ...prev,
                template: { bundle_id: ref.bundle_id, flow_id: ref.flow_id, name: match?.name || prev.template?.name || ref.bundle_id },
                updated_at: now_iso(),
              }));
            }

            // Walk up parent_run_id to find the root run (best-effort).
            let root_run_id = rid;
            let parent = String(current_run?.parent_run_id || "").trim();
            let safety = 0;
            while (parent && parent !== root_run_id && safety < 10) {
              root_run_id = parent;
              safety += 1;
              try {
                const pr = await props.gateway.get_run(parent);
                parent = String(pr?.parent_run_id || "").trim();
              } catch {
                break;
              }
            }
            if (stopped) return;

            const [root_input, cur_input] = await Promise.all([
              props.gateway.get_run_input_data(root_run_id).catch(() => null),
              root_run_id === rid ? Promise.resolve(null) : props.gateway.get_run_input_data(rid).catch(() => null),
            ]);
            if (stopped) return;

            const root_prompt = extract_user_prompt_from_run_input(root_input);
            const cur_prompt = extract_user_prompt_from_run_input(cur_input);

            // Prefer the root run's context.messages for durable chat history; fall back to the current run.
            const ctx_msgs_raw =
              extract_context_messages_from_run_input(root_input).length > 0
                ? extract_context_messages_from_run_input(root_input)
                : extract_context_messages_from_run_input(cur_input);

            const seeded: ReplMessage[] = [];
            for (const m of ctx_msgs_raw) {
              const role_raw = String(m.role || "").trim().toLowerCase();
              if (role_raw !== "user" && role_raw !== "assistant") continue;
              seeded.push({ role: role_raw as any, content: m.content, ts: m.ts });
            }

            const req_text = String(root_prompt || cur_prompt || "").trim();
            const last = seeded.length ? seeded[seeded.length - 1] : null;
            if (req_text && !(last && last.role === "user" && last.content.trim() === req_text)) {
              seeded.push({ role: "user", content: req_text, ts: now_iso(), run_id: root_run_id });
            }

            if (root_prompt && cur_prompt && root_prompt.trim() && cur_prompt.trim() && root_prompt.trim() !== cur_prompt.trim()) {
              seeded.push({ role: "system", level: "info", title: "Enriched request", content: cur_prompt.trim(), ts: now_iso(), run_id: rid });
            }

            if (seeded.length) {
              update_repl((prev) => ({ ...prev, messages: seeded.slice(-200), updated_at: now_iso() }));
            }
          } catch {
            // best-effort
          }
        } else {
          // Even when we don't seed messages, try to keep the agent label consistent with the run.
          try {
            const current_run = await props.gateway.get_run(rid);
            if (stopped) return;
            const workflow_id = String(current_run?.workflow_id || "").trim();
            const ref = parse_workflow_ref(workflow_id);
            if (ref) {
              const match = infer_agent_template_from_workflow_id(workflow_id, templates);
              if (!props.repl.template || props.repl.template.bundle_id !== ref.bundle_id || props.repl.template.flow_id !== ref.flow_id) {
                update_repl((prev) => ({
                  ...prev,
                  template: { bundle_id: ref.bundle_id, flow_id: ref.flow_id, name: match?.name || prev.template?.name || ref.bundle_id },
                  updated_at: now_iso(),
                }));
              }
            }
          } catch {
            // ignore
          }
        }
        set_status("working…", -1);
        await append_page(0);
        if (stopped) return;
        while (!stopped) {
          try {
            await props.gateway.stream_ledger(rid, {
              after: cursor_ref.current,
              signal: abort_ref.current?.signal,
              on_step: (ev) => {
                if (stopped) return;
                handle_record(ev);
              },
            });
          } catch (e: any) {
            if (stopped) return;
            if (String(e?.name || "") === "AbortError") return;
            throw e;
          }

          if (stopped) return;

          try {
            await append_page(cursor_ref.current);
          } catch {
            // ignore
          }
          if (stopped) return;

          // SSE streams may close even when the run is still active; poll status and reconnect.
          let run_status = "";
          try {
            const info = await props.gateway.get_run(rid);
            if (stopped) return;
            run_status = String(info?.status || "").trim().toLowerCase();
          } catch {
            run_status = "";
          }

          if (run_status === "completed" || run_status === "failed" || run_status === "cancelled") {
            finish_run_without_output(run_status as any, rid);
            return;
          }

          await new Promise((r) => window.setTimeout(r, 900));
        }
      } catch (e: any) {
        if (stopped) return;
        if (String(e?.name || "") === "AbortError") return;
        set_error(String(e?.message || e || "Failed to attach"));
      }
    };
    run();

    return () => {
      stopped = true;
      stop_stream();
    };
  }, [active_run_id, props.gateway]);

  async function submit_resume(payload_obj: any): Promise<void> {
    if (!active_run_id || !wait_key) return;
    set_error("");
    set_resuming(true);
    try {
      await props.gateway.submit_command({
        command_id: random_id(),
        run_id: active_run_id,
        type: "resume",
        payload: { wait_key, payload: payload_obj || {} },
        client_id: props.settings.client_id || "abstractcode_web",
      });
      try {
        const after = cursor_ref.current;
        const page = await props.gateway.get_ledger(active_run_id, { after, limit: 2000 });
        const items = Array.isArray(page.items) ? page.items : [];
        const start_cursor = after + 1;
        for (let i = 0; i < items.length; i++) {
          const rec = items[i] as StepRecord;
          handle_record({ cursor: start_cursor + i, record: rec });
        }
        const next = typeof page.next_after === "number" ? page.next_after : after + items.length;
        cursor_ref.current = Math.max(cursor_ref.current, next);
      } catch {
        // ignore
      }
    } catch (e: any) {
      set_error(String(e?.message || e || "resume failed"));
    } finally {
      set_resuming(false);
    }
  }

  async function submit_answer(text: string): Promise<void> {
    const t = String(text || "").trim();
    if (!t) return;
    append_message({ role: "user", content: t, ts: now_iso() });
    await submit_resume({ response: t });
  }

  const can_type = !active_run_id && !resuming;
  const can_send = can_type && !pending_files;
  const cmd_query = useMemo(() => {
    const raw = composer.trimStart();
    if (!raw.startsWith("/")) return "";
    const rest = raw.slice(1);
    const first = rest.split(/\s+/, 1)[0] || "";
    return first.trim().toLowerCase();
  }, [composer]);

  const commands = useMemo(
    () => [
      { name: "help", desc: "Show available commands" },
      { name: "clear", desc: "Clear chat (keep agent)" },
      { name: "sessions", desc: "Open Sessions view" },
      { name: "new", desc: "Start a new chat (pick agent)" },
      { name: "settings", desc: "Open Settings" },
      { name: "status", desc: "Show current run status" },
      { name: "temperature", desc: "Show/set temperature" },
      { name: "seed", desc: "Show/set seed (-1=random)" },
      { name: "max-iterations", desc: "Show/set max iterations" },
    ],
    []
  );

  const cmd_matches = useMemo(() => {
    const raw = composer.trimStart();
    if (!raw.startsWith("/")) return [];
    // Only show menu for the first token (before a space).
    if (raw.includes(" ")) return [];
    const q = cmd_query;
    const out = commands.filter((c) => (q ? c.name.startsWith(q) : true));
    return out.slice(0, 12);
  }, [composer, cmd_query, commands]);

  const file_token = useMemo(() => extract_active_token(composer, composer_cursor, "@"), [composer, composer_cursor]);
  const file_query = useMemo(() => String(file_token?.query || "").trim(), [file_token]);

  useEffect(() => {
    set_file_active(0);
  }, [file_query, file_matches.length]);

  useEffect(() => {
    if (!file_token) {
      set_file_matches([]);
      set_file_error("");
      set_file_loading(false);
      return;
    }
    const q = file_query;
    if (!q) {
      set_file_matches([]);
      set_file_error("");
      set_file_loading(false);
      return;
    }

    let stopped = false;
    set_file_loading(true);
    set_file_error("");

    const handle = window.setTimeout(() => {
      void (async () => {
        try {
          const res = await props.gateway.files_search(q, { limit: 12 });
          if (stopped) return;
          const items = Array.isArray(res?.items) ? res.items : [];
          const paths = items
            .map((it: any) => String(it?.path || "").trim())
            .filter(Boolean)
            .slice(0, 12);
          set_file_matches(paths);
        } catch (e: any) {
          if (stopped) return;
          set_file_matches([]);
          set_file_error(String(e?.message || e || "File search failed"));
        } finally {
          if (!stopped) set_file_loading(false);
        }
      })();
    }, 180);

    return () => {
      stopped = true;
      window.clearTimeout(handle);
    };
  }, [props.gateway, file_query, Boolean(file_token)]);

  useEffect(() => {
    set_cmd_active(0);
  }, [cmd_query, cmd_matches.length]);

  async function run_command(raw: string): Promise<boolean> {
    const t = String(raw || "").trim();
    if (!t.startsWith("/")) return false;
    const parts = t.slice(1).split(/\s+/);
    const cmd = String(parts[0] || "").trim().toLowerCase();
    const args = parts.slice(1);

    const say = (text: string) => append_message({ role: "system", level: "info", title: "Command", content: text, ts: now_iso() });

    if (!cmd || cmd === "help") {
      say(
        [
          "Commands:",
          "- `/help`",
          "- `/clear`",
          "- `/sessions`",
          "- `/new`",
          "- `/settings`",
          "- `/status`",
          "- `/temperature [n]`",
          "- `/seed [n]`",
          "- `/max-iterations [n]`",
        ].join("\n")
      );
      return true;
    }

    if (cmd === "sessions") {
      set_route({ name: "sessions" });
      return true;
    }

    if (cmd === "new") {
      set_route({ name: "new" });
      return true;
    }

    if (cmd === "settings") {
      set_route({ name: "settings" });
      return true;
    }

    if (cmd === "clear") {
      clear_status();
      records_ref.current = [];
      set_records([]);
      set_active_run_id(null);
      update_repl(() => reset_repl_state({ template: props.repl.template }, props.session_id));
      return true;
    }

    if (cmd === "status") {
      const lines = [
        `agent: ${template_label || "—"}`,
        `provider/model: ${props.settings.provider || "—"} / ${props.settings.model || "—"}`,
        `run: ${active_run_id ? active_run_id : "(idle)"}`,
        `status: ${status_text ? status_text : "(none)"}`,
      ];
      if (active_run_id && wait_state) {
        lines.push(`waiting: ${wait_reason || "unknown"}${wait_event_name ? `:${wait_event_name}` : ""}`);
      }
      say(lines.join("\n"));
      return true;
    }

    if (cmd === "temperature") {
      if (!args.length) {
        say(`temperature: ${String(props.settings.temperature)}`);
        return true;
      }
      const v = Number(args[0]);
      if (!Number.isFinite(v) || v < 0) {
        say("Usage: /temperature <number>=0");
        return true;
      }
      props.on_settings({ ...props.settings, temperature: v });
      say(`temperature set: ${v}`);
      return true;
    }

    if (cmd === "seed") {
      if (!args.length) {
        say(`seed: ${String(props.settings.seed)}`);
        return true;
      }
      const v = Number(args[0]);
      if (!Number.isFinite(v)) {
        say("Usage: /seed <-1|0|...>");
        return true;
      }
      props.on_settings({ ...props.settings, seed: Math.trunc(v) });
      say(`seed set: ${Math.trunc(v)}`);
      return true;
    }

    if (cmd === "max-iterations" || cmd === "max_iterations" || cmd === "max") {
      if (!args.length) {
        say(`max_iterations: ${String(props.settings.max_iterations)}`);
        return true;
      }
      const v = Number(args[0]);
      if (!Number.isFinite(v) || v < 1) {
        say("Usage: /max-iterations <int>=1");
        return true;
      }
      props.on_settings({ ...props.settings, max_iterations: Math.trunc(v) });
      say(`max_iterations set: ${Math.trunc(v)}`);
      return true;
    }

    say(`Unknown command: /${cmd} (try /help)`);
    return true;
  }

  async function attach_file(path: string, token: ActiveToken | null): Promise<void> {
    const p = String(path || "").trim();
    if (!p) return;

    // Clear the active @token from the composer (Cursor-style chips instead of inline tags).
    if (token && token.start >= 0 && token.end >= token.start && token.end <= composer.length) {
      const before = composer.slice(0, token.start);
      const after = composer.slice(token.end);
      const next = `${before}${after}`.replace(/\s{2,}/g, " ");
      set_composer(next);
      set_composer_cursor(before.length);
    }
    set_file_matches([]);
    set_file_error("");
    set_file_loading(false);

    set_attached_files((prev) => {
      if (prev.some((f) => f.path === p)) return prev;
      return [...prev, { path: p, content: null, loading: true }].slice(-12);
    });

    try {
      const res = await props.gateway.files_read(p);
      const content_raw = typeof res?.content === "string" ? res.content : String(res?.content ?? "");
      const content = content_raw.trimEnd();
      const is_err = content.startsWith("Error:") || content.startsWith("Refused:");
      const err = is_err ? content.split("\n")[0] : "";
      set_attached_files((prev) =>
        prev.map((f) => (f.path === p ? { ...f, loading: false, content: is_err ? null : content, error: err || undefined } : f))
      );
    } catch (e: any) {
      const msg = String(e?.message || e || "Failed to read file");
      set_attached_files((prev) => prev.map((f) => (f.path === p ? { ...f, loading: false, content: null, error: msg } : f)));
    } finally {
      try {
        input_ref.current?.focus();
      } catch {
        // ignore
      }
    }
  }

  function remove_attached_file(path: string): void {
    const p = String(path || "").trim();
    if (!p) return;
    set_attached_files((prev) => prev.filter((f) => f.path !== p));
  }

  return (
    <div className="repl">
      <div className="panel repl_panel">
        <div className="repl_meta">
          <div>
            <div className="muted">agent</div>
            <div className="mono">{template_label || "—"}</div>
          </div>
          <div>
            <div className="muted">provider/model</div>
            <div className="mono">
              {props.settings.provider || "—"} / {props.settings.model || "—"}
            </div>
            <div className="muted" style={{ marginTop: 6 }}>
              Context(next):{" "}
              <span className="mono">
                {context_meter.used.toLocaleString()}
                {context_meter.max_tokens ? `/${context_meter.max_tokens.toLocaleString()}` : "/?"} tk
              </span>
              {context_meter.max_tokens ? ` (${context_meter.pct.toFixed(0)}%)` : ""}
              {model_caps_error ? <span className="error" style={{ marginTop: 6 }}>{model_caps_error}</span> : null}
            </div>
          </div>
          <div className="repl_actions">
            <button className="btn" onClick={() => set_details_open((v) => !v)}>
              {details_open ? "Hide details" : "Show details"}
            </button>
          <button className="btn" onClick={() => update_repl(() => reset_repl_state({ template: props.repl.template }, props.session_id))}>
            Clear chat
          </button>
        </div>
      </div>

        {template_error ? <div className="warn">{template_error}</div> : null}
        {!props.settings.provider.trim() || !props.settings.model.trim() ? (
          <div className="warn">Set provider + model in Settings. (These agent workflows require them.)</div>
        ) : null}
        {!props.settings.gateway_url.trim() ? (
          <div className="warn">Set a Gateway URL in Settings (or host this app on the same origin as the gateway).</div>
        ) : null}

        {error ? <div className="error">{error}</div> : null}

        <div className="repl_chat">
          {!props.repl.messages.length ? <div className="muted">Start typing to begin.</div> : null}
          {props.repl.messages.map((m, idx) => (
            <ChatMessageCard key={`${m.ts}:${idx}`} m={m} />
          ))}
        </div>

        {active_run_id ? (
          <div className="repl_wait">
            {wait_state ? (
              <>
                <div className="muted mono">
                  waiting: {wait_reason || "unknown"}
                  {wait_event_name ? `:${wait_event_name}` : ""} • {wait_key}
                </div>

                {wait_reason === "subworkflow" ? (
                  <div className="warn" style={{ display: "flex", justifyContent: "space-between", gap: 10, alignItems: "center" }}>
                    <div style={{ minWidth: 0 }}>
                      Waiting on a subworkflow. This chat stays attached to the parent run, but you can open the child run to observe it.
                      <div className="muted mono" style={{ marginTop: 4, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                        sub_run_id:{" "}
                        {String((wait_state as any)?.details?.sub_run_id || "").trim() ||
                          (wait_key.startsWith("subworkflow:") ? wait_key.split(":", 2)[1] : "—")}
                      </div>
                    </div>
                    <button
                      className="btn"
                      type="button"
                      onClick={() => {
                        const sub =
                          String((wait_state as any)?.details?.sub_run_id || "").trim() ||
                          (wait_key.startsWith("subworkflow:") ? wait_key.split(":", 2)[1] : "");
                        if (sub) set_active_run_id(sub);
                      }}
                    >
                      Open
                    </button>
                  </div>
                ) : null}

                {tool_calls_for_wait.length ? (
                  <>
                    <div className="muted">Approve to run these tools.</div>
                    <div style={{ marginTop: 10, display: "flex", flexDirection: "column", gap: 8 }}>
                      {tool_calls_for_wait.map((tc, idx) => {
                        const name = String(tc?.name || "").trim();
                        const call_id = String(tc?.call_id || tc?.id || "").trim();
                        return (
                          <ToolBlockCard
                            key={`${call_id || name || "tool"}:${idx}`}
                            meta={{
                              name: name || undefined,
                              call_id: call_id || undefined,
                              arguments: tc?.arguments ?? undefined,
                              output_preview: "",
                              pending: true,
                            }}
                          />
                        );
                      })}
                    </div>
                    <div className="actions">
                      <button className="btn primary" disabled={resuming} onClick={() => submit_resume({ approved: true })}>
                        Approve + resume
                      </button>
                      <button className="btn" disabled={resuming} onClick={() => submit_resume({ approved: false, reason: "Denied by user" })}>
                        Deny
                      </button>
                    </div>
                  </>
                ) : can_user_answer_wait ? (
                  <AskForm wait={wait_state} disabled={resuming} on_submit={(val) => submit_answer(val)} />
                ) : (
                  <div className="warn">
                    This run is waiting ({wait_reason || "unknown"}
                    {wait_event_name ? `:${wait_event_name}` : ""}). This web host can only answer user waits and `abstract.ask`.
                  </div>
                )}
              </>
            ) : (
              <div className="run_working" aria-live="polite">
                <span className="run_spinner" aria-label="working" />
                <div style={{ minWidth: 0 }}>
                  <div className="run_working_title">Working…</div>
                  <div className="muted mono" style={{ marginTop: 2, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                    {status_text || "working…"}
                  </div>
                </div>
              </div>
            )}
          </div>
        ) : null}

        {attached_files.length ? (
          <div className="file_chips">
            {attached_files.map((f) => {
              const p = String(f.path || "").trim();
              const cls = f.error ? "file_chip error" : f.loading ? "file_chip loading" : "file_chip";
              return (
                <div key={p} className={cls} title={f.error ? String(f.error) : p}>
                  <span className="mono">@{p}</span>
                  {f.loading ? <span className="muted">loading…</span> : null}
                  {f.error ? <span className="muted">{String(f.error)}</span> : null}
                  <button className="chip_remove" type="button" onClick={() => remove_attached_file(p)} aria-label="Remove file">
                    ×
                  </button>
                </div>
              );
            })}
          </div>
        ) : null}

        {file_token && file_query ? (
          <div className="cmd_menu">
            {file_loading ? <div className="cmd_notice muted">Searching files…</div> : null}
            {file_error ? <div className="cmd_notice error">{file_error}</div> : null}
            {!file_loading && !file_error && !file_matches.length ? <div className="cmd_notice muted">No matches.</div> : null}
            {file_matches.map((p, idx) => (
              <button
                key={p}
                className={`cmd_item ${idx === file_active ? "active" : ""}`}
                type="button"
                onClick={() => void attach_file(p, file_token)}
              >
                <span className="mono">@{p}</span>
                <span className="muted">attach</span>
              </button>
            ))}
          </div>
        ) : null}

        {cmd_matches.length ? (
          <div className="cmd_menu">
            {cmd_matches.map((c, idx) => (
              <button
                key={c.name}
                className={`cmd_item ${idx === cmd_active ? "active" : ""}`}
                type="button"
                onClick={() => {
                  set_composer(`/${c.name} `);
                }}
              >
                <span className="mono">/{c.name}</span>
                <span className="muted">{c.desc}</span>
              </button>
            ))}
          </div>
        ) : null}

        <div className="repl_composer">
          <textarea
            className="mono"
            ref={input_ref}
            value={composer}
            rows={3}
            onChange={(e) => {
              set_composer(e.target.value);
              const pos = typeof e.target.selectionStart === "number" ? e.target.selectionStart : e.target.value.length;
              set_composer_cursor(pos);
            }}
            onClick={(e) => {
              const el = e.currentTarget;
              const pos = typeof el.selectionStart === "number" ? el.selectionStart : el.value.length;
              set_composer_cursor(pos);
            }}
            onKeyUp={(e) => {
              const el = e.currentTarget;
              const pos = typeof el.selectionStart === "number" ? el.selectionStart : el.value.length;
              set_composer_cursor(pos);
            }}
            placeholder={!can_type ? "Waiting for the current run…" : pending_files ? "Loading attached files…" : "Type a message…"}
            disabled={!can_type}
            onKeyDown={(e) => {
              if (file_token) {
                if (e.key === "Escape") {
                  if (file_matches.length || file_error) {
                    e.preventDefault();
                    set_file_matches([]);
                    set_file_error("");
                    set_file_loading(false);
                    return;
                  }
                }
                if (file_matches.length) {
                  if (e.key === "ArrowDown") {
                    e.preventDefault();
                    set_file_active((v) => Math.min(file_matches.length - 1, v + 1));
                    return;
                  }
                  if (e.key === "ArrowUp") {
                    e.preventDefault();
                    set_file_active((v) => Math.max(0, v - 1));
                    return;
                  }
                  if (e.key === "Tab") {
                    e.preventDefault();
                    const picked = file_matches[file_active] || file_matches[0];
                    if (picked) void attach_file(picked, file_token);
                    return;
                  }
                }
              }
              if (cmd_matches.length) {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  set_cmd_active((v) => Math.min(cmd_matches.length - 1, v + 1));
                  return;
                }
                if (e.key === "ArrowUp") {
                  e.preventDefault();
                  set_cmd_active((v) => Math.max(0, v - 1));
                  return;
                }
                if (e.key === "Tab") {
                  e.preventDefault();
                  const picked = cmd_matches[cmd_active] || cmd_matches[0];
                  if (picked) set_composer(`/${picked.name} `);
                  return;
                }
              }
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                if (pending_files) {
                  set_error("Wait for attached files to finish loading.");
                  return;
                }
                const v = composer;
                set_composer("");
                void (async () => {
                  const handled = await run_command(v);
                  if (handled) return;
                  await start_turn(v);
                })();
              }
            }}
          />
          <button
            className="btn primary"
            disabled={!can_send || !composer.trim()}
            onClick={() => {
              const v = composer;
              set_composer("");
              void (async () => {
                const handled = await run_command(v);
                if (handled) return;
                await start_turn(v);
              })();
            }}
          >
            Send
          </button>
        </div>

        {details_open ? (
          <div style={{ marginTop: 12 }}>
            <h2>Details</h2>
            <div className="field">
              <label>active run id</label>
              <input className="mono" readOnly value={active_run_id || ""} />
            </div>
            <div className="field">
              <label>ledger (latest)</label>
              <textarea className="mono" readOnly value={safe_json(records.map((r) => r.record))} rows={12} />
            </div>
          </div>
        ) : null}
      </div>

    </div>
  );
}

function ChatMessageCard(props: { m: ReplMessage }): React.ReactElement {
  const m = props.m;
  const meta_obj: any = m.meta && typeof m.meta === "object" ? (m.meta as any) : null;
  const kind = meta_obj && typeof meta_obj._kind === "string" ? String(meta_obj._kind) : "";
  const repl_meta = meta_obj && meta_obj._repl && typeof meta_obj._repl === "object" ? (meta_obj._repl as any) : null;
  const usage = repl_meta && repl_meta.usage && typeof repl_meta.usage === "object" ? (repl_meta.usage as any) : null;
  const usage_parsed = parse_usage_summary(usage);
  const dur_ms = repl_meta && Number.isFinite(Number(repl_meta.duration_ms)) ? Number(repl_meta.duration_ms) : null;
  const tok_s = repl_meta && Number.isFinite(Number(repl_meta.tok_s)) ? Number(repl_meta.tok_s) : null;

  const who = m.role === "assistant" ? "assistant" : m.role === "system" ? (m.title || m.level || "system") : "you";
  const cls =
    m.role === "assistant"
      ? "assistant"
      : m.role === "system"
        ? m.level === "error"
          ? "message error"
          : m.level === "warn"
            ? "message warn"
            : "status"
        : "user";
  const [copied, set_copied] = useState(false);
  return (
    <div className={`chat_item ${cls}`}>
      <div className="meta mono">
        <span>
          {who}
          {m.run_id ? ` • ${m.run_id.slice(0, 8)}` : ""} • {new Date(m.ts).toLocaleTimeString()}
          {m.role === "assistant" && (usage_parsed || dur_ms !== null) ? (
            <>
              {" "}
              •{" "}
              <span className="muted">
                {usage_parsed ? `in ${usage_parsed.input_tokens} • out ${usage_parsed.output_tokens}` : ""}
                {usage_parsed && dur_ms !== null ? " • " : ""}
                {dur_ms !== null ? format_duration_short(dur_ms) : ""}
                {(usage_parsed || dur_ms !== null) && tok_s !== null ? ` • ${tok_s.toFixed(1)} tok/s` : ""}
              </span>
            </>
          ) : null}
        </span>
        <button
          className="btn mini"
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(String(m.content || ""));
              set_copied(true);
              window.setTimeout(() => set_copied(false), 900);
            } catch {
              // ignore
            }
          }}
          type="button"
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      {kind === "tool" ? <ToolBlockCard meta={meta_obj?.tool} /> : null}
      {kind !== "tool" ? (
        <div className="body markdown">
          <MarkdownRenderer markdown={m.content} />
        </div>
      ) : null}
      {m.meta ? (
        <details style={{ marginTop: 8 }}>
          <summary className="muted">meta</summary>
          <pre className="mono" style={{ margin: 0, whiteSpace: "pre-wrap" }}>
            {safe_json(m.meta)}
          </pre>
        </details>
      ) : null}
    </div>
  );
}

function ToolBlockCard(props: { meta: any }): React.ReactElement {
  const t: any = props.meta && typeof props.meta === "object" ? props.meta : {};
  const name = String(t.name || "").trim() || "(unknown tool)";
  const call_id = String(t.call_id || "").trim();
  const pending = t.pending === true;
  const success_raw = t.success;
  const success = typeof success_raw === "boolean" ? success_raw : null;
  const error = String(t.error || "").trim();
  const args = t.arguments;
  const output_preview = String(t.output_preview || "").trim();

  const status = pending ? "pending" : error ? "error" : success === false ? "failed" : success === true ? "ok" : "done";
  const badge_cls =
    status === "pending"
      ? "tool_badge pending"
      : status === "error" || status === "failed"
        ? "tool_badge error"
        : "tool_badge ok";

  return (
    <details className="tool_block">
      <summary className="tool_summary">
        <div className="tool_left">
          <span className={badge_cls}>{status}</span>
          <span className="mono">{name}</span>
          {call_id ? <span className="muted mono">• {call_id}</span> : null}
        </div>
        <span className="muted">details</span>
      </summary>

      <div className="tool_body">
        {error ? <div className="error">Error: {error}</div> : null}
        {pending ? <div className="muted">Awaiting approval / execution.</div> : null}
        <div className="field">
          <label>arguments</label>
          <textarea className="mono" readOnly rows={6} value={safe_json(args)} />
        </div>
        {!pending || output_preview ? (
          <div className="field">
            <label>output (preview)</label>
            <textarea className="mono" readOnly rows={10} value={output_preview} />
          </div>
        ) : null}
      </div>
    </details>
  );
}

function AskForm(props: { wait: WaitState; disabled?: boolean; on_submit: (value: string) => void }): React.ReactElement {
  const [value, set_value] = useState("");
  const choices = Array.isArray(props.wait.choices) ? props.wait.choices : [];
  const allow_free_text = props.wait.allow_free_text !== false;
  const disabled = props.disabled === true;
  const prompt = String(props.wait.prompt || "").trim() || "(no prompt provided)";

  useEffect(() => {
    set_value("");
  }, [props.wait.wait_key]);

  return (
    <>
      <div className="field">
        <label>prompt</label>
        <textarea className="mono" readOnly value={prompt} rows={4} />
      </div>

      {choices.length ? (
        <div className="field">
          <label>choices</label>
          <select className="mono" value={value} onChange={(e) => set_value(e.target.value)}>
            <option value="">(select)</option>
            {choices.map((c, idx) => (
              <option key={idx} value={String(c)}>
                {String(c)}
              </option>
            ))}
          </select>
        </div>
      ) : null}

      {allow_free_text ? (
        <div className="field">
          <label>response</label>
          <input className="mono" value={value} onChange={(e) => set_value(e.target.value)} placeholder="Type response…" />
        </div>
      ) : null}

      <div className="actions">
        <button className="btn primary" disabled={disabled || !value.trim()} onClick={() => props.on_submit(value.trim())}>
          Submit
        </button>
      </div>
    </>
  );
}
