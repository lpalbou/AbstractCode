import React, { useEffect, useMemo, useRef, useState } from "react";

import { GatewayClient, GatewayHttpError } from "../lib/gateway_client";
import { random_id } from "../lib/ids";
import { extract_wait_from_record } from "../lib/runtime_extractors";
import { choose_follow_run, infer_subworkflow_follow_kind, type FollowRunKind } from "../lib/subworkflow_follow";
import { LedgerStreamEvent, StepRecord, ToolCall, WaitState } from "../lib/types";
import type { AttachmentRef } from "../lib/types";
import { resolve_blocking_wait } from "../lib/wait_resolution";
import { ChatMessageContent } from "@abstractuic/panel-chat";
import { AgentCyclesPanel, build_agent_trace, type LedgerRecordItem } from "@abstractuic/monitor-flow";
import { registerMonitorGpuWidget } from "@abstractutils/monitor-gpu";
import { MarkdownRenderer } from "./markdown_renderer";
import { ToolPicker } from "./tool_picker";
import { Icon, type IconName } from "./icons";
import { copy_text } from "../lib/clipboard";
import { build_run_input_data } from "../lib/run_input";
import { seed_repl_messages_from_history_bundle } from "../lib/history_bundle_seed";
import { session_memory_owner_run_id } from "../lib/session_memory";
import {
  clear_active_run_id,
  clear_run_cursor,
  create_new_repl_session,
  load_active_run_id,
  load_current_repl_session,
  load_run_cursor,
  load_settings,
  ReplMessage,
  ReplState,
  ReplTemplate,
  reset_repl_state,
  save_active_run_id,
  save_current_repl_session,
  save_run_cursor,
  save_settings,
  Settings,
  switch_current_repl_session,
} from "../lib/storage";

type Route = { name: "console"; session_id?: string } | { name: "new" } | { name: "sessions" } | { name: "settings" };

type AgentTemplate = {
  bundle_id: string;
  flow_id: string;
  name: string;
  description: string;
  interfaces: string[];
};

type AttachedFile = {
  path: string;
  attachment: AttachmentRef | null;
  loading: boolean;
  error?: string;
  size_bytes?: number;
};

function parse_route(): Route {
  const h = String(window.location.hash || "").replace(/^#/, "");
  const parts = h.split("/").filter(Boolean);
  if (!parts.length) return { name: "console" };
  if (parts[0] === "session" && parts[1]) return { name: "console", session_id: String(parts[1] || "").trim() };
  if (parts[0] === "new") return { name: "new" };
  if (parts[0] === "sessions") return { name: "sessions" };
  if (parts[0] === "settings") return { name: "settings" };
  return { name: "console" };
}

function set_route(r: Route): void {
  if (r.name === "console") window.location.hash = r.session_id ? `#/session/${encodeURIComponent(r.session_id)}` : "#/";
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

type FileTarget = "server" | "client";

function parse_file_target_query(raw_query: string): { target: FileTarget; query: string } {
  const q = String(raw_query || "").trim();
  if (!q) return { target: "server", query: "" };
  const lower = q.toLowerCase();
  if (lower === "client") return { target: "client", query: "" };
  if (lower.startsWith("client:")) return { target: "client", query: q.slice("client:".length) };
  if (lower === "server") return { target: "server", query: "" };
  if (lower.startsWith("server:")) return { target: "server", query: q.slice("server:".length) };
  return { target: "server", query: q };
}

function now_iso(): string {
  return new Date().toISOString();
}

function flag_enabled(value: unknown): boolean {
  const s = String(value ?? "").trim().toLowerCase();
  return s === "1" || s === "true" || s === "yes" || s === "on";
}

function monitor_gpu_enabled(): boolean {
  if (typeof window === "undefined") return false;
  if (window.__ABSTRACT_UI_CONFIG__?.monitor_gpu === true) return true;
  if (flag_enabled(import.meta.env?.VITE_MONITOR_GPU)) return true;
  try {
    const q = new URLSearchParams(window.location.search);
    return flag_enabled(q.get("monitor-gpu"));
  } catch {
    return false;
  }
}

type UsageSummary = { input_tokens: number; output_tokens: number; total_tokens: number };

function parse_usage_summary(value: any): UsageSummary | null {
  if (!value || typeof value !== "object") return null;
  const v: any = value;
  const in_tok = Number(v.input_tokens ?? v.prompt_tokens ?? v.prompt ?? v.input ?? v.in ?? 0);
  const out_tok = Number(v.output_tokens ?? v.completion_tokens ?? v.completion ?? v.output ?? v.out ?? 0);
  const total_tok = Number(v.total_tokens ?? v.total ?? (Number.isFinite(in_tok) && Number.isFinite(out_tok) ? in_tok + out_tok : 0));
  if (!Number.isFinite(in_tok) && !Number.isFinite(out_tok) && !Number.isFinite(total_tok)) return null;
  const parsed = {
    input_tokens: Number.isFinite(in_tok) ? Math.max(0, Math.trunc(in_tok)) : 0,
    output_tokens: Number.isFinite(out_tok) ? Math.max(0, Math.trunc(out_tok)) : 0,
    total_tokens: Number.isFinite(total_tok) ? Math.max(0, Math.trunc(total_tok)) : 0,
  };
  if (parsed.input_tokens === 0 && parsed.output_tokens === 0 && parsed.total_tokens === 0) return null;
  return parsed;
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

function compute_llm_iteration_progress(
  events: LedgerStreamEvent[],
  run_id: string
): { completed: number; in_flight: boolean; current: number } {
  const rid = String(run_id || "").trim();
  if (!rid) return { completed: 0, in_flight: false, current: 0 };
  const by_step = new Map<string, string>();
  for (const ev of events) {
    const rec: any = ev?.record;
    if (!rec || typeof rec !== "object") continue;
    if (String(rec?.run_id || "").trim() !== rid) continue;
    const eff_type = String(rec?.effect?.type || "").trim();
    if (eff_type !== "llm_call") continue;
    const step_id = String(rec?.step_id || "").trim();
    if (!step_id) continue;
    const st = String(rec?.status || "").trim();
    by_step.set(step_id, st);
  }
  let completed = 0;
  let in_flight = false;
  for (const st of by_step.values()) {
    if (st === "completed") completed += 1;
    else if (st === "started" || st === "waiting") in_flight = true;
  }
  const current = completed + (in_flight ? 1 : 0);
  return { completed, in_flight, current };
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

function format_tokens_k(n: number): string {
  const v = Number(n);
  if (!Number.isFinite(v) || v <= 0) return "0k";
  const tok = Math.max(0, Math.trunc(v));
  const k = tok / 1000;
  const decimals = tok >= 10_000 ? 0 : 1;
  let out = k.toFixed(decimals);
  if (decimals > 0) out = out.replace(/\.0$/, "");
  return `${out}k`;
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

function tool_call_signature_primary(name: string, args: any, tool_spec?: any): string {
  const n = String(name || "").trim() || "tool";
  if (args == null) return `${n}()`;
  if (typeof args !== "object" || Array.isArray(args)) return `${n}(${clamp_text(String(args), 120)})`;

  const params = tool_spec && typeof tool_spec === "object" ? (tool_spec as any).parameters : null;
  const ordered_keys = params && typeof params === "object" && !Array.isArray(params) ? Object.keys(params as Record<string, any>) : Object.keys(args as Record<string, any>);

  let primary_key: string | null = null;
  for (const k of ordered_keys) {
    if (!k || !(k in (args as any))) continue;
    const v = (args as any)[k];
    if (v === null || v === undefined) continue;
    if (typeof v === "string" && !v.trim()) continue;
    if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") {
      primary_key = k;
      break;
    }
  }
  if (!primary_key) {
    for (const k of ordered_keys) {
      if (!k || !(k in (args as any))) continue;
      const v = (args as any)[k];
      if (v === null || v === undefined) continue;
      primary_key = k;
      break;
    }
  }
  if (!primary_key) return `${n}()`;
  return `${n}(${format_tool_arg_value_inline((args as any)[primary_key])})`;
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
    const pick_textish = (v: any): string => {
      if (typeof v === "string") return v.trim();
      if (v == null) return "";
      if (typeof v === "number" || typeof v === "boolean") return String(v);
      return "";
    };

    const msg =
      pick_textish((out0 as any)?.answer) ||
      pick_textish((out0 as any)?.response) ||
      pick_textish((out0 as any)?.message) ||
      pick_textish((out0 as any)?.text) ||
      pick_textish((out0 as any)?.content);

    if (msg) {
      // Respect the interface contract: meta is optional but should be under `meta`.
      const meta = (out0 as any)?.meta ?? null;
      return { response: msg, meta: meta && typeof meta === "object" ? meta : null };
    }
  }
  return null;
}

type WorkflowRef = {
  bundle_id: string; // base bundle id (no @version)
  bundle_version?: string; // optional @version
  flow_id: string;
  kind: "bundle" | "visual_react";
};

function split_bundle_ref(bundle_id: string): { base: string; version: string } {
  const raw = String(bundle_id || "").trim();
  if (!raw) return { base: "", version: "" };
  const i = raw.indexOf("@");
  if (i <= 0) return { base: raw, version: "" };
  return { base: raw.slice(0, i).trim(), version: raw.slice(i + 1).trim() };
}

function parse_workflow_ref(workflow_id: string): WorkflowRef | null {
  const wid = String(workflow_id || "").trim();
  if (!wid) return null;
  if (wid.includes(":")) {
    const [bundle_id_raw, flow_id] = wid.split(":", 2);
    const { base, version } = split_bundle_ref(bundle_id_raw);
    if (base && flow_id?.trim()) return { bundle_id: base, bundle_version: version || undefined, flow_id: flow_id.trim(), kind: "bundle" };
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
  const gpu_enabled = monitor_gpu_enabled();
  const monitor_gpu_ref = useRef<HTMLElement | null>(null);
  const [route, set_route_state] = useState<Route>(() => parse_route());
  const [session, set_session] = useState<{ session_id: string; state: ReplState }>(() => load_current_repl_session());
  const [pending_attach, set_pending_attach] = useState<{ run_id: string; template: ReplTemplate | null } | null>(null);
  const pending_console_sid_ref = useRef<string>("");
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
    if (!gpu_enabled) return;
    registerMonitorGpuWidget();
  }, [gpu_enabled]);

  useEffect(() => {
    if (!gpu_enabled) return;
    const el = monitor_gpu_ref.current as any;
    if (el) el.token = settings.auth_token || "";
  }, [gpu_enabled, settings.auth_token, route.name]);

  useEffect(() => {
    save_current_repl_session(session_id, repl);
  }, [session_id, repl]);

  useEffect(() => {
    const on_hash = () => set_route_state(parse_route());
    window.addEventListener("hashchange", on_hash);
    return () => window.removeEventListener("hashchange", on_hash);
  }, []);

  useEffect(() => {
    if (route.name !== "console") return;
    const sid = String((route as any)?.session_id || "").trim();
    if (!sid) return;
    if (sid === session.session_id) return;
    set_session(switch_current_repl_session(sid));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [route.name, (route as any)?.session_id]);

  useEffect(() => {
    if (route.name !== "console") return;
    const sid = session.session_id;
    const want = `#/session/${encodeURIComponent(sid)}`;
    const cur = String(window.location.hash || "");
    if (cur === want) return;
    if (cur === "#/" || cur === "" || cur === "#") {
      try {
        window.history.replaceState(null, "", want);
      } catch {
        window.location.hash = want;
      }
    }
  }, [route.name, session.session_id]);

  return (
    <div className="app">
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
            on_nav={(name) => {
              if (name === "console") set_route({ name: "console", session_id: session.session_id });
              else set_route({ name });
            }}
            monitor_gpu_enabled={gpu_enabled}
            monitor_gpu_ref={monitor_gpu_ref}
            gateway_url={settings.gateway_url}
          />
        ) : null}
        {route.name !== "console" ? (
          <div className="repl">
            <div className="panel repl_frame">
              <Header
                active={route.name}
                on_nav={(name) => {
                  if (name === "console") set_route({ name: "console", session_id: session.session_id });
                  else set_route({ name });
                }}
                monitor_gpu_enabled={gpu_enabled}
                monitor_gpu_ref={monitor_gpu_ref}
                gateway_url={settings.gateway_url}
              />
              <div className="repl_panel">
                <div className="repl_inset">
                  {route.name === "new" ? (
                    <NewChatPage
                      gateway={gateway}
                      repl={repl}
                      on_start={(t) => {
                        const created = create_new_repl_session(t);
                        pending_console_sid_ref.current = created.session_id;
                        set_session({ session_id: created.session_id, state: created.state });
                      }}
                      on_done={() => {
                        const sid = pending_console_sid_ref.current || session.session_id;
                        pending_console_sid_ref.current = "";
                        set_route({ name: "console", session_id: sid });
                      }}
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
                        set_route({ name: "console", session_id: sid });
                      }}
                    />
                  ) : null}
                  {route.name === "settings" ? (
                    <SettingsPage
                      gateway={gateway}
                      settings={settings}
                      on_change={set_settings}
                      on_done={() => set_route({ name: "console", session_id: session.session_id })}
                    />
                  ) : null}
                </div>
              </div>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function Header(props: {
  active: Route["name"];
  on_nav: (name: Route["name"]) => void;
  center?: React.ReactNode;
  monitor_gpu_enabled?: boolean;
  monitor_gpu_ref?: React.RefObject<HTMLElement>;
  gateway_url?: string;
}): React.ReactElement {
  const nav_items: { name: Route["name"]; label: string; icon: IconName }[] = [
    { name: "console", label: "Chat", icon: "chat" },
    { name: "new", label: "New", icon: "plus" },
    { name: "sessions", label: "History", icon: "history" },
    { name: "settings", label: "Settings", icon: "settings" },
  ];

  return (
    <header className="header header_integrated">
      <div className="brand" aria-label="AbstractCode">
        <span className="brand_mark" aria-hidden="true">
          <svg viewBox="0 0 24 24" width="18" height="18" fill="none" xmlns="http://www.w3.org/2000/svg">
            <path d="M7 10 12 17 17 10" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
            <path d="M7 10h10" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
            <circle cx="7" cy="10" r="1.55" fill="currentColor" />
            <circle cx="17" cy="10" r="1.55" fill="currentColor" />
            <circle cx="12" cy="17" r="1.55" fill="currentColor" />
            <path d="M12 4.85 13.05 6.9 15.1 7.95 13.05 9 12 11.05 10.95 9 8.9 7.95 10.95 6.9Z" fill="currentColor" />
          </svg>
        </span>
        <span className="brand_name">AbstractCode</span>
      </div>

      {props.center ? props.center : null}

      <div className="header_right">
        <nav className="nav" role="navigation" aria-label="Main navigation">
          {nav_items.map((item) => {
            const active = props.active === item.name;
            return (
              <button
                key={item.name}
                className="btn"
                onClick={() => {
                  if (active) return;
                  props.on_nav(item.name);
                }}
                aria-current={active ? "page" : undefined}
                title={item.label}
                type="button"
              >
                <Icon name={item.icon} className="nav-icon" />
                <span className="nav-label">{item.label}</span>
              </button>
            );
          })}
        </nav>
        {props.monitor_gpu_enabled ? (
          <monitor-gpu
            ref={props.monitor_gpu_ref as any}
            mode="icon"
            history-size="5"
            tick-ms="2000"
            base-url={String(props.gateway_url || "").trim()}
            title="GPU usage (host)"
            style={
              {
                ["--monitor-gpu-width" as any]: "34px",
                ["--monitor-gpu-bars-height" as any]: "28px",
                ["--monitor-gpu-padding" as any]: "0px 4px",
                ["--monitor-gpu-radius" as any]: "999px",
                ["--monitor-gpu-bg" as any]: "rgba(0,0,0,0.18)",
                ["--monitor-gpu-border" as any]: "rgba(255,255,255,0.16)",
                flexShrink: 0,
              } as React.CSSProperties
            }
          />
        ) : null}
      </div>
    </header>
  );
}

function Notice(props: {
  variant: "warn" | "error" | "info";
  children: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
  title?: string;
  onClick?: React.MouseEventHandler<HTMLDivElement>;
}): React.ReactElement {
  const variant = props.variant;
  const cls_base = variant === "error" ? "error" : "warn";
  const cls = `${cls_base}${variant === "info" ? " info" : ""}${props.className ? ` ${props.className}` : ""}`;
  const icon: IconName = variant === "error" ? "error" : variant === "warn" ? "warning" : "info";

  return (
    <div className={cls} style={props.style} title={props.title} onClick={props.onClick} role="status">
      <span className="notice_icon" aria-hidden="true">
        <Icon name={icon} size={14} />
      </span>
      <div className="notice_content">{props.children}</div>
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
  steps?: number | null;
  llm_calls?: number | null;
  tool_calls?: number | null;
  tokens_total?: number | null;
  limits?: any;
};

function SessionsPage(props: {
  gateway: GatewayClient;
  on_open_session: (session_id: string, run_id: string, template: ReplTemplate | null) => void;
}): React.ReactElement {
  const [templates, set_templates] = useState<AgentTemplate[]>([]);
  const [runs, set_runs] = useState<RemoteRunSummary[]>([]);
  const [page, set_page] = useState<number>(0);
  const [loading, set_loading] = useState(false);
  const [error, set_error] = useState("");
  const [attach_id, set_attach_id] = useState("");
  const page_size = 50;

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
      const [tpls, r] = await Promise.all([
        list_agent_templates(props.gateway),
        props.gateway.list_runs({ limit: 300, root_only: true, include_ledger_len: false, include_metrics: true }),
      ]);
      const items = Array.isArray((r as any)?.items) ? (r as any).items : [];
      set_templates(tpls);
      set_page(0);

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
          steps: typeof it?.steps === "number" ? it.steps : it?.steps ?? null,
          llm_calls: typeof it?.llm_calls === "number" ? it.llm_calls : it?.llm_calls ?? null,
          tool_calls: typeof it?.tool_calls === "number" ? it.tool_calls : it?.tool_calls ?? null,
          tokens_total: typeof it?.tokens_total === "number" ? it.tokens_total : it?.tokens_total ?? null,
          limits: it?.limits ?? null,
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
    <div className="sessions_page">
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
        <Notice variant="error" style={{ marginTop: 12 }}>
          {error}
        </Notice>
      ) : null}

      {!error && runs.length ? (
        <div style={{ display: "flex", gap: 10, alignItems: "center", marginTop: 12, flexWrap: "wrap" }}>
          <span className="muted mono">
            showing {Math.min(runs.length, page * page_size + 1)}-{Math.min(runs.length, (page + 1) * page_size)} of {runs.length}
          </span>
          <button className="btn mini" type="button" onClick={() => set_page((p) => Math.max(0, p - 1))} disabled={page <= 0}>
            Prev
          </button>
          <button
            className="btn mini"
            type="button"
            onClick={() => set_page((p) => ((p + 1) * page_size < runs.length ? p + 1 : p))}
            disabled={(page + 1) * page_size >= runs.length}
          >
            Next
          </button>
        </div>
      ) : null}

      {!loading && !error && !runs.length ? (
        <div className="sessions_empty">
          <span className="sessions_empty_icon">◇</span>
          <div>No sessions yet</div>
          <div className="muted">Start a new chat to create your first session</div>
        </div>
      ) : null}

      <div className="sessions_grid">
        {runs.slice(page * page_size, page * page_size + page_size).map((r) => {
          const wid = String(r.workflow_id || "").trim();
          const ref = parse_workflow_ref(wid);
          const key = ref ? `${ref.bundle_id}:${ref.flow_id}` : "";
          const tpl = key ? workflow_to_template.get(key) : undefined;
          const tpl2 = !tpl && ref?.bundle_id ? bundle_to_template.get(ref.bundle_id) : undefined;
          const agent_name = tpl?.name || tpl2?.name || ref?.bundle_id || "Agent";
          const ts = String(r.updated_at || r.created_at || "").trim();
          const status = String(r.status || "").toLowerCase().trim();
          const open_template = ref ? { bundle_id: ref.bundle_id, flow_id: ref.flow_id, name: tpl?.name || tpl2?.name } : null;
          const sid = String(r.session_id || r.run_id).trim();
          
          // Format relative time
          const time_ago = ts ? format_relative_time(new Date(ts)) : "";
          
          // Status styling
          const status_config: Record<string, { label: string; cls: string }> = {
            completed: { label: "Completed", cls: "success" },
            failed: { label: "Failed", cls: "error" },
            cancelled: { label: "Cancelled", cls: "muted" },
            waiting: { label: "Waiting", cls: "warning" },
            running: { label: "Running", cls: "info" },
          };
          const status_info = status_config[status] || { label: status || "Unknown", cls: "muted" };
          
          // Format dates
          const created = r.created_at ? new Date(r.created_at) : null;
          const updated = r.updated_at ? new Date(r.updated_at) : null;
          const created_str = created ? created.toLocaleDateString(undefined, { month: "short", day: "numeric", year: created.getFullYear() !== new Date().getFullYear() ? "numeric" : undefined }) : "—";
          const updated_str = updated ? format_relative_time(updated) : "—";
          const ctx_used_raw = (r as any)?.tokens_total ?? (r as any)?.limits?.tokens?.estimated_used;
          const ctx_used = Number.isFinite(Number(ctx_used_raw)) ? Math.max(0, Math.trunc(Number(ctx_used_raw))) : null;
          const llm_calls2 = Number.isFinite(Number(r.llm_calls)) ? Math.max(0, Math.trunc(Number(r.llm_calls))) : null;
          const tool_calls2 = Number.isFinite(Number(r.tool_calls)) ? Math.max(0, Math.trunc(Number(r.tool_calls))) : null;
          const steps2 = Number.isFinite(Number(r.ledger_len)) ? Math.max(0, Math.trunc(Number(r.ledger_len))) : Number.isFinite(Number(r.steps)) ? Math.max(0, Math.trunc(Number(r.steps))) : 0;
          
          return (
            <button
              key={sid || r.run_id}
              className="session_card"
              onClick={() => props.on_open_session(sid || r.run_id, r.run_id, open_template)}
              type="button"
            >
              <div className="session_card_header">
                <span className="session_card_agent">{agent_name}</span>
                <span className={`session_card_status ${status_info.cls}`}>{status_info.label}</span>
              </div>
              <div className="session_card_stats">
                <span className="stat_item" title="Steps (best-effort)">
                  <span className="stat_icon">≡</span>
                  {steps2}
                </span>
                {ctx_used !== null ? (
                  <span className="stat_item" title="Context tokens (estimated)">
                    <span className="stat_icon">◈</span>
                    {ctx_used}
                  </span>
                ) : null}
                {llm_calls2 !== null ? (
                  <span className="stat_item" title="LLM calls">
                    <span className="stat_icon">◉</span>
                    {llm_calls2}
                  </span>
                ) : null}
                {tool_calls2 !== null ? (
                  <span className="stat_item" title="Tool calls">
                    <span className="stat_icon">⚙</span>
                    {tool_calls2}
                  </span>
                ) : null}
                <span className="session_card_sep">•</span>
                <span title="Created date">created {created_str}</span>
                <span className="session_card_sep">•</span>
                <span title="Last activity">updated {updated_str}</span>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function format_relative_time(date: Date): string {
  const now = Date.now();
  const then = date.getTime();
  const diff = now - then;
  
  const seconds = Math.floor(diff / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  
  if (seconds < 60) return "Just now";
  if (minutes < 60) return `${minutes}m ago`;
  if (hours < 24) return `${hours}h ago`;
  if (days < 7) return `${days}d ago`;
  
  // For older dates, show the actual date
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function SettingsPage(props: { gateway: GatewayClient; settings: Settings; on_change: (s: Settings) => void; on_done: () => void }): React.ReactElement {
  const s = props.settings;
  const [gateway_connected, set_gateway_connected] = useState(false);
  const [gateway_connecting, set_gateway_connecting] = useState(false);
  const [gateway_error, set_gateway_error] = useState("");
  const [server_workspace_policy, set_server_workspace_policy] = useState<any>(null);
  const [server_workspace_policy_error, set_server_workspace_policy_error] = useState("");
  const [providers, set_providers] = useState<any[]>([]);
  const [models, set_models] = useState<string[]>([]);
  const [tools, set_tools] = useState<any[]>([]);
  const [loading_providers, set_loading_providers] = useState(false);
  const [loading_models, set_loading_models] = useState(false);
  const [loading_tools, set_loading_tools] = useState(false);
  const [error_providers, set_error_providers] = useState("");
  const [error_models, set_error_models] = useState("");
  const [error_tools, set_error_tools] = useState("");

  // Auto-connect if previously connected
  useEffect(() => {
    if (s.gateway_was_connected && s.gateway_url && !gateway_connected && !gateway_connecting) {
      void connect_gateway();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    // Reset state on gateway URL/token change
    set_gateway_connected(false);
    set_gateway_connecting(false);
    set_gateway_error("");
    set_server_workspace_policy(null);
    set_server_workspace_policy_error("");
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
    set_server_workspace_policy(null);
    set_server_workspace_policy_error("");
    set_loading_providers(true);
    set_loading_tools(true);
    set_error_providers("");
    set_error_tools("");
    try {
      const [prov_res, tool_res, ws_res] = await Promise.all([
        props.gateway.discovery_providers(),
        props.gateway.discovery_tools(),
        props.gateway.workspace_policy().catch((e: any) => ({ ok: false, error: String(e?.message || e || "Failed to load workspace policy") })),
      ]);
      const prov_items = Array.isArray(prov_res?.items) ? prov_res.items : [];
      const tool_items = Array.isArray(tool_res?.items) ? tool_res.items : [];
      set_providers(prov_items);
      set_tools(tool_items);
      if (ws_res && typeof ws_res === "object") {
        if (ws_res.ok && (ws_res as any).policy) set_server_workspace_policy((ws_res as any).policy);
        else if ((ws_res as any).error) set_server_workspace_policy_error(String((ws_res as any).error));
      }
      set_gateway_connected(true);
      // Remember that we were connected for auto-reconnect
      props.on_change({ ...s, gateway_was_connected: true });
    } catch (e: any) {
      set_gateway_error(String(e?.message || e || "Failed to connect to gateway"));
      set_gateway_connected(false);
      set_server_workspace_policy(null);
      set_server_workspace_policy_error("");
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
    set_server_workspace_policy(null);
    set_server_workspace_policy_error("");
    set_providers([]);
    set_models([]);
    set_tools([]);
    set_error_providers("");
    set_error_models("");
    set_error_tools("");
    // Clear auto-reconnect flag
    props.on_change({ ...s, gateway_was_connected: false });
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

  const tools_selected_set = useMemo(() => {
    const out = new Set<string>();
    for (const n of (s.tools || []).map((x) => String(x || "").trim()).filter(Boolean)) out.add(n);
    return out;
  }, [s.tools]);

  const tools_all_selected = useMemo(() => {
    if (!tool_options.length) return false;
    for (const n of tool_options) if (!tools_selected_set.has(n)) return false;
    return tools_selected_set.size === tool_options.length;
  }, [tool_options, tools_selected_set]);

  // UI state: user can open "Custom allowlist" even if everything is currently selected.
  const [tools_editor_open, set_tools_editor_open] = useState<boolean>(false);

  // If the selection becomes a strict subset (e.g. loaded from storage), auto-open the editor.
  useEffect(() => {
    if (!gateway_connected) return;
    if (!tool_options.length) return;
    if (!tools_all_selected) set_tools_editor_open(true);
  }, [gateway_connected, tool_options.length, tools_all_selected]);

  return (
    <div className="settings_page">
      <div className="settings_grid">
        <div className="panel settings_card">
          <div className="settings_card_header">
            <h2>Gateway</h2>
          </div>
          <div className="settings_card_header" style={{ marginTop: 6 }}>
            <div className="muted">Connect once to discover providers/models/tools. Settings are saved locally.</div>
            <span className={`chip ${gateway_connected ? "ok" : gateway_connecting ? "info" : "muted"}`}>
              {gateway_connecting ? "connecting" : gateway_connected ? "connected" : "disconnected"}
            </span>
          </div>

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
            {gateway_error ? (
              <Notice variant="error" className="inline">
                {gateway_error}
              </Notice>
            ) : null}
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

          <div className="settings_section_divider" />

          <div className="settings_section_header">
            <h3>Workspace</h3>
            <span className="muted mono">files/tools</span>
          </div>
          <div className="muted">Controls what the agent can access via filesystem tools and what appears in @file search.</div>
          {server_workspace_policy ? (
            <div className="muted mono" style={{ marginTop: 6 }}>
              server: overrides={server_workspace_policy.client_workspace_scope_overrides ? "on" : "off"}
              {Array.isArray(server_workspace_policy.mounts) && server_workspace_policy.mounts.length
                ? `; mounts=${server_workspace_policy.mounts.map((m: any) => String(m?.name || "").trim()).filter(Boolean).join(", ")}`
                : ""}
            </div>
          ) : server_workspace_policy_error ? (
            <div className="muted" style={{ marginTop: 6 }}>
              server: workspace policy unavailable ({server_workspace_policy_error})
            </div>
          ) : null}

          <div className="field">
            <div className="field_label_with_hint">
              <label>workspace_root</label>
              <span className="field_hint">Empty = gateway default (typically an isolated per-run workspace)</span>
            </div>
            <input
              className="mono"
              value={String(s.workspace_root || "")}
              onChange={(e) => props.on_change({ ...s, workspace_root: e.target.value })}
              placeholder="/Users/albou/abstractframework"
            />
          </div>

          <div className="field">
            <div className="field_label_with_hint">
              <label>workspace_access_mode</label>
              <span className="field_hint">Controls absolute path access</span>
            </div>
            {(() => {
              const allowed = Array.isArray(server_workspace_policy?.allowed_access_modes)
                ? (server_workspace_policy.allowed_access_modes as any[]).map((x) => String(x || "").trim()).filter(Boolean)
                : null;
              const allow_all_except_ignored = !allowed || allowed.includes("all_except_ignored");
              return (
            <select
              className="mono"
              value={String(s.workspace_access_mode || "workspace_only")}
              onChange={(e) => props.on_change({ ...s, workspace_access_mode: e.target.value })}
            >
              <option value="workspace_only">workspace_only</option>
              <option value="workspace_or_allowed">workspace_or_allowed</option>
              <option value="all_except_ignored" disabled={!allow_all_except_ignored}>
                all_except_ignored{!allow_all_except_ignored ? " (disabled by gateway)" : ""}
              </option>
            </select>
              );
            })()}
            <div className="field_hint">
              workspace_only: absolute paths must stay under workspace_root. workspace_or_allowed: allow additional roots from workspace_allowed_paths.
            </div>
          </div>

          <div className="field">
            <div className="field_label_with_hint">
              <label>workspace_allowed_paths</label>
              <span className="field_hint">Newline-separated directories (absolute or relative to workspace_root)</span>
            </div>
            <textarea
              className="mono"
              rows={3}
              value={String(s.workspace_allowed_paths || "")}
              onChange={(e) => props.on_change({ ...s, workspace_allowed_paths: e.target.value })}
              placeholder={"/Users/albou/projects/mnemosyne\n/Users/albou/abstractframework"}
            />
          </div>

          <div className="field">
            <div className="field_label_with_hint">
              <label>workspace_ignored_paths</label>
              <span className="field_hint">Newline-separated paths to block (absolute or relative to workspace_root)</span>
            </div>
            <textarea
              className="mono"
              rows={3}
              value={String(s.workspace_ignored_paths || "")}
              onChange={(e) => props.on_change({ ...s, workspace_ignored_paths: e.target.value })}
              placeholder={"node_modules\nruntime\nsecret"}
            />
          </div>
        </div>

        <div className="panel settings_card">
          <div className="settings_card_header">
            <h2>Model</h2>
            <span className="muted mono">{gateway_connected ? "discovered" : "connect first"}</span>
          </div>
          <div className="muted">Choose provider/model and runtime parameters.</div>

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
            {error_providers ? (
              <Notice variant="error" className="inline">
                {error_providers}
              </Notice>
            ) : null}
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
            {error_models ? (
              <Notice variant="error" className="inline">
                {error_models}
              </Notice>
            ) : null}
          </div>

          <div className="settings_row2">
            <div className="field">
              <label>Max iterations</label>
              <input
                value={String(s.max_iterations)}
                onChange={(e) => props.on_change({ ...s, max_iterations: Number.isFinite(Number(e.target.value)) ? Number(e.target.value) : 20 })}
                placeholder="20"
              />
            </div>
            <div className="field">
              <div className="field_label_with_hint">
                <label>Max in tokens</label>
                <span className="field_hint">0 = unset</span>
              </div>
              <input
                value={String(s.max_in_tokens || 0)}
                onChange={(e) =>
                  props.on_change({
                    ...s,
                    max_in_tokens: Number.isFinite(Number(e.target.value)) ? Math.max(0, Number(e.target.value)) : 0,
                  })
                }
                placeholder="0"
              />
            </div>
          </div>

          <div className="settings_row2">
            <div className="field">
              <label>Temperature</label>
              <input
                value={String(s.temperature)}
                onChange={(e) => props.on_change({ ...s, temperature: Number.isFinite(Number(e.target.value)) ? Number(e.target.value) : 0.7 })}
                placeholder="0.7"
              />
            </div>
            <div className="field">
              <div className="field_label_with_hint">
                <label>Seed</label>
                <span className="field_hint">-1 = random/unset; ≥ 0 = deterministic (provider permitting)</span>
              </div>
              <input
                value={String(s.seed)}
                onChange={(e) => props.on_change({ ...s, seed: Number.isFinite(Number(e.target.value)) ? Number(e.target.value) : -1 })}
                placeholder="-1"
              />
            </div>
          </div>

          <div className="field">
            <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <input
                type="checkbox"
                checked={Boolean(s.use_context)}
                onChange={(e) => props.on_change({ ...s, use_context: Boolean(e.target.checked) })}
              />
              <span>Use context</span>
            </label>
            <div className="field_hint">When enabled, workflows can include context.messages as history (Agent/LLM Call use_context).</div>
          </div>

          <div className="field">
            <label>System</label>
            <textarea
              className="mono"
              rows={3}
              placeholder="Optional system prompt (high priority instructions)…"
              value={String(s.system || "")}
              onChange={(e) => props.on_change({ ...s, system: e.target.value })}
            />
          </div>

          <div className="field">
            <div className="field_label_with_hint">
              <label>resp_schema</label>
              <span className="field_hint">Optional JSON Schema object (JSON)</span>
            </div>
            <textarea
              className="mono"
              rows={6}
              placeholder='{"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"]}'
              value={String(s.resp_schema || "")}
              onChange={(e) => props.on_change({ ...s, resp_schema: e.target.value })}
            />
          </div>
        </div>

        <div className="panel settings_card settings_card_full">
          <div className="settings_card_header">
            <h2>Tools</h2>
          </div>
          <div className="muted">Default is all tools. Switch to a custom allowlist only when needed.</div>

          <div className="tool_mode_row">
            <div className="segmented" role="group" aria-label="Tools mode">
              <button
                className={`seg_btn ${!tools_editor_open ? "active" : ""}`}
                type="button"
                disabled={!gateway_connected || loading_tools || !tool_options.length}
                onClick={() => {
                  set_tools_editor_open(false);
                  props.on_change({ ...s, tools: tool_options, tools_initialized: true });
                }}
              >
                All tools
              </button>
              <button
                className={`seg_btn ${tools_editor_open ? "active" : ""}`}
                type="button"
                disabled={!gateway_connected || loading_tools || !tool_options.length}
                onClick={() => {
                  if (tools_editor_open) return;
                  set_tools_editor_open(true);
                  // Default custom allowlist to 0 tools selected.
                  props.on_change({ ...s, tools: [], tools_initialized: true });
                }}
              >
                Custom allowlist
              </button>
            </div>
            {gateway_connected && tool_options.length > 0 ? (
              <span className="tools_counter">
                {tools_all_selected ? (
                  <>✓ All {tool_options.length} tools</>
                ) : (
                  <>✓ {tools_selected_set.size} of {tool_options.length} selected</>
                )}
              </span>
            ) : null}
          </div>

          {tools_editor_open ? (
            <ToolPicker
              tools={tools as any[]}
              selected={s.tools}
              disabled={!gateway_connected || loading_tools || !tool_options.length}
              onChange={(next) => props.on_change({ ...s, tools: next, tools_initialized: true })}
            />
          ) : null}

          {loading_tools ? <div className="muted" style={{ marginTop: 10 }}>Loading tools…</div> : null}
          {error_tools ? (
            <Notice variant="error">
              {error_tools}
            </Notice>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function NewChatPage(props: { gateway: GatewayClient; repl: ReplState; on_start: (t: ReplTemplate | null) => void; on_done: () => void }): React.ReactElement {
  const [templates, set_templates] = useState<AgentTemplate[]>([]);
  const [loading, set_loading] = useState(false);
  const [reloading_bundles, set_reloading_bundles] = useState(false);
  const [uploading_bundle, set_uploading_bundle] = useState(false);
  const [error, set_error] = useState("");
  const [selected, set_selected] = useState<AgentTemplate | null>(null);
  const upload_bundle_input_ref = useRef<HTMLInputElement | null>(null);

  const refresh_templates = async (opts?: { preserve_selection?: boolean }): Promise<void> => {
    set_loading(true);
    set_error("");
    try {
      const items = await list_agent_templates(props.gateway);
      set_templates(items);

      const cur = opts?.preserve_selection ? props.repl.template : null;
      if (cur && items.some((t) => t.bundle_id === cur.bundle_id && t.flow_id === cur.flow_id)) {
        const t = items.find((x) => x.bundle_id === cur.bundle_id && x.flow_id === cur.flow_id) || null;
        set_selected(t);
      } else {
        set_selected(items.find((t) => t.bundle_id === "basic-agent") || items[0] || null);
      }
    } catch (e: any) {
      set_error(String(e?.message || e || "Failed to load agents"));
    } finally {
      set_loading(false);
    }
  };

  const reload_gateway_bundles = async (): Promise<void> => {
    set_reloading_bundles(true);
    set_error("");
    try {
      await props.gateway.reload_bundles();
      await refresh_templates({ preserve_selection: true });
    } catch (e: any) {
      set_error(String(e?.message || e || "Bundle reload failed"));
    } finally {
      set_reloading_bundles(false);
    }
  };

  const upload_bundle = async (file: File): Promise<void> => {
    set_uploading_bundle(true);
    set_error("");
    try {
      await props.gateway.upload_bundle(file, { overwrite: false, reload: true });
      await refresh_templates({ preserve_selection: true });
    } catch (e: any) {
      set_error(String(e?.message || e || "Bundle upload failed"));
    } finally {
      set_uploading_bundle(false);
      if (upload_bundle_input_ref.current) upload_bundle_input_ref.current.value = "";
    }
  };

  useEffect(() => {
    let stopped = false;
    const run = async () => {
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
    set_loading(true);
    set_error("");
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
    <div className="new_chat_page">
      <h2>Agents</h2>
      <div className="muted">Pick a RunnableFlow workflow (must implement `abstractcode.agent.v1`).</div>
      {loading ? <div className="muted" style={{ marginTop: 10 }}>Loading…</div> : null}
      {error ? (
        <Notice variant="error">
          {error}
        </Notice>
      ) : null}

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
        <input
          ref={upload_bundle_input_ref}
          type="file"
          accept=".flow"
          style={{ display: "none" }}
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) void upload_bundle(f);
          }}
        />
        <button
          className="btn"
          type="button"
          disabled={loading || reloading_bundles || uploading_bundle}
          onClick={() => void reload_gateway_bundles()}
          title="Reload the gateway’s in-memory .flow bundles (useful after updating bundles on disk)."
        >
          {reloading_bundles ? "Reloading…" : "Reload bundles"}
        </button>
        <button
          className="btn"
          type="button"
          disabled={loading || reloading_bundles || uploading_bundle}
          onClick={() => upload_bundle_input_ref.current?.click()}
          title="Upload a .flow bundle to the gateway."
        >
          {uploading_bundle ? "Uploading…" : "Upload .flow"}
        </button>
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
  on_nav: (name: Route["name"]) => void;
  monitor_gpu_enabled?: boolean;
  monitor_gpu_ref?: React.RefObject<HTMLElement>;
  gateway_url?: string;
}): React.ReactElement {
  const [templates, set_templates] = useState<AgentTemplate[]>([]);
  const [template_error, set_template_error] = useState("");
  const [model_caps, set_model_caps] = useState<any>(null);
  const [model_caps_error, set_model_caps_error] = useState("");
  const [tool_specs_by_name, set_tool_specs_by_name] = useState<Record<string, any>>({});

  const [composer, set_composer] = useState("");
  const [composer_cursor, set_composer_cursor] = useState(0);
  const [error, set_error] = useState("");
  const [cmd_active, set_cmd_active] = useState(0);
  const [file_active, set_file_active] = useState(0);

  const [active_run_id, set_active_run_id] = useState<string | null>(null);
  const [records, set_records] = useState<LedgerStreamEvent[]>([]);
  const records_ref = useRef<LedgerStreamEvent[]>([]);
  const repl_messages_ref = useRef<ReplMessage[]>([]);
  const finalized_run_ref = useRef<string>("");
  const root_run_ref = useRef<string>("");
  const [root_last_record, set_root_last_record] = useState<StepRecord | null>(null);
  const [status_text, set_status_text] = useState<string>("");
  const status_timer_ref = useRef<number | null>(null);

  const [resuming, set_resuming] = useState(false);
  const [cancelling, set_cancelling] = useState(false);

  const input_ref = useRef<HTMLTextAreaElement | null>(null);
  const upload_input_ref = useRef<HTMLInputElement | null>(null);
  const chat_scroll_ref = useRef<HTMLDivElement | null>(null);
  const chat_content_ref = useRef<HTMLDivElement | null>(null);
  const chat_end_ref = useRef<HTMLDivElement | null>(null);
  const chat_at_bottom_ref = useRef<boolean>(true);
  const [chat_at_bottom, set_chat_at_bottom] = useState<boolean>(true);
  const scroll_follow_raf_ref = useRef<number | null>(null);
  const scroll_follow_raf2_ref = useRef<number | null>(null);
  const [attached_files, set_attached_files] = useState<AttachedFile[]>([]);
  const [attachment_preview, set_attachment_preview] = useState<AttachmentRef | null>(null);
  const pending_files = attached_files.some((f) => f.loading);
  const [file_matches, set_file_matches] = useState<string[]>([]);
  const [file_match_sizes, set_file_match_sizes] = useState<Record<string, number>>({});
  const [file_loading, set_file_loading] = useState(false);
  const [file_error, set_file_error] = useState("");
  const file_search_blocked_until_ref = useRef<number>(0);

  const abort_ref = useRef<AbortController | null>(null);
  const follow_abort_ref = useRef<AbortController | null>(null);
  const cursor_by_run_ref = useRef<Record<string, number>>({});
  const seen_keys_ref = useRef<Set<string>>(new Set());
  const seen_wait_keys_ref = useRef<Set<string>>(new Set());
  const seen_tool_call_ids_ref = useRef<Set<string>>(new Set());
  const force_full_replay_run_ref = useRef<string>("");
  const cursor_flush_timer_ref = useRef<number | null>(null);
  const cursor_flush_pending_ref = useRef<Record<string, number>>({});
  const [follow_run_id, set_follow_run_id] = useState<string>("");
  const follow_run_id_ref = useRef<string>("");
  const follow_run_kind_ref = useRef<FollowRunKind>("unknown");
  const [subworkflow_label, set_subworkflow_label] = useState<string>("");
  const [drop_active, set_drop_active] = useState(false);

  useEffect(() => {
    follow_run_id_ref.current = String(follow_run_id || "").trim();
    if (!follow_run_id_ref.current) follow_run_kind_ref.current = "unknown";
  }, [follow_run_id]);

  const set_follow_run = (run_id: string, kind: FollowRunKind): void => {
    const rid = String(run_id || "").trim();
    if (!rid) return;
    follow_run_id_ref.current = rid;
    follow_run_kind_ref.current = kind;
    set_follow_run_id(rid);
  };

  const clear_follow_run = (): void => {
    follow_run_id_ref.current = "";
    follow_run_kind_ref.current = "unknown";
    set_follow_run_id("");
  };

  const maybe_set_follow_run = (run_id: string, kind: FollowRunKind): void => {
    const prev = { run_id: follow_run_id_ref.current, kind: follow_run_kind_ref.current };
    const next = choose_follow_run(prev, { run_id, kind });
    const next_id = String(next.run_id || "").trim();
    if (!next_id) return;
    if (next_id === prev.run_id && next.kind === prev.kind) return;
    set_follow_run(next_id, next.kind);
  };

  const root_wait_state: WaitState | null = useMemo(() => extract_wait_from_record(root_last_record), [root_last_record]);
  const root_wait_reason = String(root_wait_state?.reason || "").trim();
  const root_wait_key = String(root_wait_state?.wait_key || "").trim();

  const resolved_wait = useMemo(
    () => resolve_blocking_wait({ root_run_id: active_run_id, root_wait: root_wait_state, records }),
    [active_run_id, records, root_wait_state]
  );
  const wait_state: WaitState | null = resolved_wait.wait;
  const wait_run_id = String(resolved_wait.wait_run_id || "").trim();
  const tool_calls_for_wait: ToolCall[] = resolved_wait.tool_calls;

  const wait_reason = String(wait_state?.reason || "").trim();
  const wait_key = String(wait_state?.wait_key || "").trim();
  const wait_event_name = wait_reason === "event" ? normalize_ui_event_name(event_name_from_wait_key(wait_key)) : "";
  const is_user_wait = wait_reason === "user";
  const is_ask_event_wait = wait_reason === "event" && wait_event_name === "abstract.ask";
  const can_user_answer_wait = is_user_wait || is_ask_event_wait;
  const is_working = Boolean(active_run_id) && !wait_state && !resuming && Boolean(status_text.trim());
  const wait_is_compact = (root_wait_state && root_wait_reason === "subworkflow") || (!root_wait_state && is_working);

  const progress_run_id = useMemo(() => {
    const rid = String(active_run_id || "").trim();
    if (root_wait_reason === "subworkflow") {
      const fid = String(follow_run_id || "").trim();
      if (fid) return fid;
    }
    return rid;
  }, [active_run_id, follow_run_id, root_wait_reason]);

  const iteration_progress = useMemo(() => compute_llm_iteration_progress(records, progress_run_id), [records, progress_run_id]);
  const max_iterations_ui = useMemo(() => {
    const v = Number(props.settings.max_iterations);
    return Number.isFinite(v) && v > 0 ? Math.trunc(v) : 25;
  }, [props.settings.max_iterations]);
  const iteration_badge = useMemo(() => {
    if (!progress_run_id) return "";
    const max = max_iterations_ui;
    if (!Number.isFinite(max) || max <= 0) return "";
    const cur = Math.max(0, Math.trunc(iteration_progress.current || 0));
    return `(${Math.min(cur, max)}/${max})`;
  }, [progress_run_id, iteration_progress.current, max_iterations_ui]);

  const sub_run_id_for_wait = useMemo(() => {
    if (root_wait_reason !== "subworkflow") return "";
    const from_details = String((root_wait_state as any)?.details?.sub_run_id || "").trim();
    if (from_details) return from_details;
    if (root_wait_key.startsWith("subworkflow:")) return String(root_wait_key.split(":", 2)[1] || "").trim();
    return "";
  }, [root_wait_reason, root_wait_key, root_wait_state]);

  useEffect(() => {
    if (root_wait_reason !== "subworkflow") {
      if (follow_run_kind_ref.current !== "user_facing") clear_follow_run();
      set_subworkflow_label("");
      return;
    }
    if (sub_run_id_for_wait) {
      const kind = infer_subworkflow_follow_kind(root_wait_state);
      maybe_set_follow_run(sub_run_id_for_wait, kind === "unknown" ? "background" : kind);
    }
  }, [root_wait_reason, root_wait_state, sub_run_id_for_wait]);

  useEffect(() => {
    if (!sub_run_id_for_wait) return;
    let stopped = false;
    set_subworkflow_label("");
    void (async () => {
      try {
        const info = await props.gateway.get_run(sub_run_id_for_wait);
        if (stopped) return;
        const wid = String(info?.workflow_id || "").trim();
        set_subworkflow_label(wid || "subflow");
      } catch {
        if (!stopped) set_subworkflow_label("subflow");
      }
    })();
    return () => {
      stopped = true;
    };
  }, [props.gateway, sub_run_id_for_wait]);

  const repl_template = props.repl.template;
  const template_label = repl_template?.name || (repl_template ? `${repl_template.bundle_id}:${repl_template.flow_id}` : "");

  useEffect(() => {
    repl_messages_ref.current = props.repl.messages || [];
  }, [props.repl.messages]);

  useEffect(() => {
    const rid = String(props.attach_run_id || "").trim();
    if (!rid) return;
    props.on_attach_consumed();
    force_full_replay_run_ref.current = rid;
    // Replay should be server-authoritative. Clear local messages so the
    // RunHistoryBundle seed is always used (prevents stale/duplicated prompts
    // and missing attachment chips after session switching).
    update_repl((prev) => ({ ...prev, messages: [], updated_at: now_iso() }));
    set_attached_files([]);
    set_file_matches([]);
    set_file_error("");
    set_file_loading(false);
    set_composer("");
    set_error("");
    clear_status();
    finalized_run_ref.current = "";
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

  useEffect(() => {
    let stopped = false;
    set_tool_specs_by_name({});
    void (async () => {
      try {
        const res = await props.gateway.discovery_tools();
        const items = Array.isArray(res?.items) ? res.items : [];
        const by_name: Record<string, any> = {};
        for (const it of items) {
          if (!it || typeof it !== "object") continue;
          const n = String((it as any).name || "").trim();
          if (!n) continue;
          by_name[n] = it;
        }
        if (!stopped) set_tool_specs_by_name(by_name);
      } catch {
        if (!stopped) set_tool_specs_by_name({});
      }
    })();
    return () => {
      stopped = true;
    };
  }, [props.gateway]);

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
            // Agent workflow may have been renamed, deleted, or gateway restarted
            // Auto-select a new agent instead of blocking the user
            const replacement = items.find((t) => t.bundle_id === cur.bundle_id) || items.find((t) => t.bundle_id === "basic-agent") || items[0];
            if (replacement) {
              update_repl((prev) => ({
                ...prev,
                template: { bundle_id: replacement.bundle_id, flow_id: replacement.flow_id, name: replacement.name },
                updated_at: now_iso(),
              }));
              set_template_error(`Agent "${cur.name || cur.bundle_id}" was updated or unavailable — switched to "${replacement.name}".`);
            } else {
              set_template_error(`Agent "${cur.name || cur.bundle_id}" unavailable. Select an agent in New.`);
            }
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

  // Auto-dismiss informational template messages after a few seconds
  useEffect(() => {
    if (!template_error || !template_error.includes("switched to")) return;
    const timer = setTimeout(() => set_template_error(""), 6000);
    return () => clearTimeout(timer);
  }, [template_error]);

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

  // Crash/reload recovery: re-attach to an in-flight run for this session.
  useEffect(() => {
    const sid = String(props.session_id || "").trim();
    if (!sid) return;
    if (active_run_id) return;

    // 1) Prefer the persisted active run id (written when the user started a run).
    const stored = load_active_run_id(sid);
    if (stored) {
      const already_has_answer = (props.repl.messages || []).some(
        (m) => m.role === "assistant" && String(m.run_id || "").trim() === stored && String(m.content || "").trim()
      );
      if (already_has_answer) {
        clear_active_run_id(sid);
        clear_run_cursor(stored);
        return;
      }
      set_active_run_id(stored);
      return;
    }

    // 2) Fallback: last user message may already be tagged with a run_id.
    const msgs = props.repl.messages || [];
    const last = msgs.length ? msgs[msgs.length - 1] : null;
    const msg_rid = last && last.role === "user" ? String((last as any).run_id || "").trim() : "";
    if (!msg_rid) return;

    const already_has_answer = msgs.some(
      (m) => m.role === "assistant" && String(m.run_id || "").trim() === msg_rid && String(m.content || "").trim()
    );
    if (already_has_answer) return;

    save_active_run_id(sid, msg_rid);
    set_active_run_id(msg_rid);
  }, [active_run_id, props.session_id, props.repl.messages]);

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

  function schedule_cursor_flush(run_id: string, cursor: number): void {
    const rid = String(run_id || "").trim();
    const cur = Number(cursor);
    if (!rid) return;
    if (!Number.isFinite(cur)) return;

    cursor_flush_pending_ref.current[rid] = Math.max(Number(cursor_flush_pending_ref.current[rid] || 0), Math.max(0, Math.trunc(cur)));
    if (cursor_flush_timer_ref.current !== null) return;
    cursor_flush_timer_ref.current = window.setTimeout(() => {
      const pending = cursor_flush_pending_ref.current;
      cursor_flush_pending_ref.current = {};
      cursor_flush_timer_ref.current = null;
      for (const [k, v] of Object.entries(pending)) {
        save_run_cursor(k, v);
      }
    }, 250);
  }

  useEffect(() => {
    return () => {
      if (cursor_flush_timer_ref.current !== null) {
        window.clearTimeout(cursor_flush_timer_ref.current);
        cursor_flush_timer_ref.current = null;
      }
      if (scroll_follow_raf_ref.current !== null) {
        window.cancelAnimationFrame(scroll_follow_raf_ref.current);
        scroll_follow_raf_ref.current = null;
      }
      if (scroll_follow_raf2_ref.current !== null) {
        window.cancelAnimationFrame(scroll_follow_raf2_ref.current);
        scroll_follow_raf2_ref.current = null;
      }
      cursor_flush_pending_ref.current = {};
    };
  }, []);

  function schedule_scroll_to_bottom(): void {
    if (scroll_follow_raf_ref.current !== null || scroll_follow_raf2_ref.current !== null) return;
    scroll_follow_raf_ref.current = window.requestAnimationFrame(() => {
      scroll_follow_raf_ref.current = null;
      scroll_follow_raf2_ref.current = window.requestAnimationFrame(() => {
        scroll_follow_raf2_ref.current = null;
        const el = chat_scroll_ref.current;
        if (!el) return;
        el.scrollTop = el.scrollHeight;
        try {
          chat_end_ref.current?.scrollIntoView({ block: "end" });
        } catch {
          // ignore
        }
        chat_at_bottom_ref.current = true;
        set_chat_at_bottom(true);
      });
    });
  }

  // Smart autoscroll: follow new messages only when the user is already at the bottom.
  useEffect(() => {
    const el = chat_scroll_ref.current;
    if (!el) return;
    const threshold_px = 80;
    const at_bottom_now = el.scrollTop + el.clientHeight >= el.scrollHeight - threshold_px;
    if (!chat_at_bottom_ref.current && !at_bottom_now) return;
    schedule_scroll_to_bottom();
  }, [props.repl.updated_at]);

  // When message contents expand after the initial render (markdown/layout/images),
  // keep the chat pinned to bottom only if the user is already following.
  useEffect(() => {
    const target = chat_content_ref.current;
    if (!target) return;
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => {
      if (!chat_at_bottom_ref.current) return;
      schedule_scroll_to_bottom();
    });
    ro.observe(target);
    return () => ro.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function autosize_composer(el: HTMLTextAreaElement | null): void {
    if (!el) return;
    try {
      const style = window.getComputedStyle(el);
      const max_h = Number.parseFloat(style.maxHeight || "");
      const cap = Number.isFinite(max_h) ? max_h : 200;
      el.style.height = "auto";
      el.style.height = `${Math.min(el.scrollHeight, cap)}px`;
    } catch {
      // Best-effort.
    }
  }

  useEffect(() => {
    autosize_composer(input_ref.current);
  }, [composer]);

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
      let output_preview = output_raw == null ? "" : typeof output_raw === "string" ? output_raw : safe_json(output_raw);
      let rendered_text: string | null = null;
      if (output_raw && typeof output_raw === "object" && !Array.isArray(output_raw)) {
        const rendered = (output_raw as any).rendered;
        if (typeof rendered === "string" && rendered.trim()) {
          rendered_text = rendered;
          output_preview = rendered;
        }
      }

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
    root_run_ref.current = "";
    set_root_last_record(null);
    cursor_by_run_ref.current = {};
    seen_keys_ref.current = new Set();
    seen_wait_keys_ref.current = new Set();
    seen_tool_call_ids_ref.current = new Set();
    clear_follow_run();
    set_subworkflow_label("");

    const user_ts = now_iso();
    finalized_run_ref.current = "";
    const attachments_for_turn = attached_files
      .filter((f) => !f.loading && !String(f.error || "").trim() && f.attachment && typeof f.attachment === "object")
      .map((f) => f.attachment as AttachmentRef)
      .slice(0, 16)
      .map((a) => ({ ...a }));
    append_message({
      role: "user",
      content: t,
      ts: user_ts,
      meta: attachments_for_turn.length ? { attachments: attachments_for_turn } : undefined,
    });

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

    try {
      const input_data = build_run_input_data({
        prompt: t,
        settings: props.settings,
        repl_messages: props.repl.messages || [],
        session_id: props.session_id,
        attached_files,
      });
      const run_id = await props.gateway.start_run(props.repl.template.flow_id, input_data, {
        bundle_id: props.repl.template.bundle_id,
        session_id: String(props.session_id || "").trim() || undefined,
      });
      save_active_run_id(props.session_id, run_id);
      save_run_cursor(run_id, 0);
      update_repl((prev) => ({
        ...prev,
        messages: (prev.messages || []).map((m) => {
          if (m.role !== "user") return m;
          if (m.ts !== user_ts) return m;
          if (String(m.content || "").trim() !== t) return m;
          return { ...m, run_id };
        }),
        updated_at: now_iso(),
      }));
      set_active_run_id(run_id);
      if (!props.settings.files_keep) set_attached_files([]);
    } catch (e: any) {
      clear_status();
      set_error(String(e?.message || e || "Failed to start run"));
    }
  }

  function stop_stream(): void {
    if (abort_ref.current) abort_ref.current.abort();
    abort_ref.current = null;
    if (follow_abort_ref.current) follow_abort_ref.current.abort();
    follow_abort_ref.current = null;
  }

  function finish_run_with_response(resp: { response: string; meta: any }, run_id: string): void {
    const rid = String(run_id || "").trim();
    if (rid && finalized_run_ref.current === rid) return;
    if (rid) finalized_run_ref.current = rid;
    const stats = compute_run_stats(records_ref.current);
    const tool_sigs = extract_tool_signatures(records_ref.current);
    const meta_obj: any = {};
    if (resp.meta !== null && resp.meta !== undefined) meta_obj.workflow_meta = resp.meta;
    meta_obj._repl = {
      duration_ms: stats.duration_ms,
      llm_calls: stats.llm_calls,
      tool_calls: stats.tool_calls,
      usage: stats.usage,
      tok_s: stats.duration_ms > 0 && stats.usage.total_tokens > 0 ? stats.usage.total_tokens / (stats.duration_ms / 1000) : null,
    };

    const resp_text = String(resp.response || "").trim();
    const existing_msgs = repl_messages_ref.current || [];
    let last_assistant: ReplMessage | null = null;
    for (let i = existing_msgs.length - 1; i >= 0; i--) {
      const m = existing_msgs[i];
      if (m.role !== "assistant") continue;
      if (String(m.content || "").trim()) {
        last_assistant = m;
        break;
      }
    }
    const already_has_final = Boolean(last_assistant && String(last_assistant.content || "").trim() === resp_text);
    if (already_has_final) {
      // When replaying a completed run, multiple end-of-run records may be observed.
      // Update metadata in-place, but avoid duplicating the assistant message.
      update_repl((prev) => {
        const msgs = [...(prev.messages || [])];
        for (let i = msgs.length - 1; i >= 0; i--) {
          const m = msgs[i];
          if (m.role !== "assistant") continue;
          if (String(m.content || "").trim() !== resp_text) continue;
          const merged_meta: any = m.meta && typeof m.meta === "object" ? { ...(m.meta as any) } : {};
          if (meta_obj.workflow_meta != null && merged_meta.workflow_meta == null) merged_meta.workflow_meta = meta_obj.workflow_meta;
          merged_meta._repl = meta_obj._repl;
          msgs[i] = { ...m, run_id: String(m.run_id || "").trim() ? m.run_id : rid, meta: merged_meta };
          break;
        }
        return { ...prev, messages: msgs.slice(-200), updated_at: now_iso() };
      });
    } else {
      append_message({ role: "assistant", content: resp_text || String(resp.response || ""), ts: now_iso(), meta: meta_obj, run_id: rid });
    }

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
    if (load_active_run_id(props.session_id) === run_id) clear_active_run_id(props.session_id);
    clear_run_cursor(run_id);
    clear_status();
    stop_stream();
    set_active_run_id(null);
  }

  function finish_run_without_output(outcome: "completed" | "failed" | "cancelled", run_id: string): void {
    const rid = String(run_id || "").trim();
    if (rid && finalized_run_ref.current === rid) return;
    if (rid) finalized_run_ref.current = rid;
    const stats = compute_run_stats(records_ref.current);
    const tool_sigs = extract_tool_signatures(records_ref.current);

    append_message({
      role: "system",
      level: outcome === "failed" ? "error" : outcome === "cancelled" ? "warn" : "info",
      title: "Digest",
      ts: now_iso(),
      run_id,
      content: "",
      meta: {
        _kind: "run_digest",
        digest: {
          outcome,
          duration_ms: stats.duration_ms,
          llm_calls: stats.llm_calls,
          tool_calls: stats.tool_calls,
          tokens: stats.usage.total_tokens
            ? { input: stats.usage.input_tokens, output: stats.usage.output_tokens, total: stats.usage.total_tokens }
            : null,
          speed_tok_s: stats.duration_ms > 0 && stats.usage.total_tokens > 0 ? stats.usage.total_tokens / (stats.duration_ms / 1000) : null,
          tools: tool_sigs.slice(0, 200),
        },
      },
    });
    if (load_active_run_id(props.session_id) === run_id) clear_active_run_id(props.session_id);
    clear_run_cursor(run_id);
    clear_status();
    stop_stream();
    set_active_run_id(null);
  }

  function handle_record(ev: LedgerStreamEvent, source_run_id: string): void {
    const rec = ev.record as StepRecord;
    const src = String(source_run_id || rec?.run_id || "").trim();
    if (!src) return;
    const key = `${src}:${ev.cursor}`;
    if (seen_keys_ref.current.has(key)) return;
    seen_keys_ref.current.add(key);
    cursor_by_run_ref.current[src] = Math.max(Number(cursor_by_run_ref.current[src] || 0), Number(ev.cursor || 0));
    schedule_cursor_flush(src, cursor_by_run_ref.current[src]);

    records_ref.current = [...records_ref.current, ev].slice(-4000);
    set_records(records_ref.current);
    if (src === root_run_ref.current) set_root_last_record(rec);

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
        const output_preview = output_s;
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

      // Track the direct subworkflow run id from the ROOT run (parent chat context).
      // We intentionally do not overwrite this from nested subruns to avoid "losing" the parent subrun
      // when it waits on its own children.
      if (src === root_run_ref.current && reason === "subworkflow") {
        const sub =
          String((w as any)?.details?.sub_run_id || "").trim() ||
          (wk.startsWith("subworkflow:") ? String(wk.split(":", 2)[1] || "").trim() : "");
        if (sub) {
          const kind = infer_subworkflow_follow_kind(w);
          maybe_set_follow_run(sub, kind === "unknown" ? "background" : kind);
        }
      }

      if (wk && prompt && (is_user_wait || is_ask_wait) && !is_tool_wait && !seen_wait_keys_ref.current.has(wk)) {
        clear_status(); // matches AbstractCode UX (spinner clears when awaiting user input)
        seen_wait_keys_ref.current.add(wk);
        append_message({ role: "assistant", content: prompt, ts: now_iso(), meta: { kind: "ask", wait_key: wk } });
      }
    }

    const out = extract_flow_end_output(rec);
    if (out && active_run_id && (src === String(active_run_id || "").trim() || src === String(follow_run_id || "").trim())) {
      finish_run_with_response(out, String(active_run_id || "").trim());
    }

    if (st === "failed" && active_run_id && (src === String(active_run_id || "").trim() || src === String(follow_run_id || "").trim())) {
      const err = String((rec as any)?.error || (rec as any)?.result?.error || "step failed").trim();
      append_message({ role: "assistant", content: `Error: ${err}`, ts: now_iso(), run_id: active_run_id });
      finish_run_without_output("failed", active_run_id);
    }
  }

  const append_page = async (run_id: string, after: number): Promise<number> => {
    const page = await props.gateway.get_ledger(run_id, { after, limit: 2000 });
    const items = Array.isArray(page.items) ? page.items : [];
    const start_cursor = after + 1;
    for (let i = 0; i < items.length; i++) {
      const rec = items[i] as StepRecord;
      handle_record({ cursor: start_cursor + i, record: rec }, run_id);
    }
    const next = typeof page.next_after === "number" ? page.next_after : after + items.length;
    cursor_by_run_ref.current[run_id] = Math.max(Number(cursor_by_run_ref.current[run_id] || 0), Number(next || 0));
    schedule_cursor_flush(run_id, cursor_by_run_ref.current[run_id]);
    return next;
  };

  useEffect(() => {
    const rid = String(active_run_id || "").trim();
    if (!rid) return;

    let stopped = false;
    set_error("");
    clear_status();
    records_ref.current = [];
    set_records([]);
    root_run_ref.current = rid;
    set_root_last_record(null);
    cursor_by_run_ref.current = { [rid]: load_run_cursor(rid) ?? 0 };
    if (force_full_replay_run_ref.current === rid) {
      cursor_by_run_ref.current = { [rid]: 0 };
      // Ensure we fully rebuild a History-open replay even if a stale cursor exists.
      save_run_cursor(rid, 0);
      force_full_replay_run_ref.current = "";
    }
    seen_keys_ref.current = new Set();
    seen_wait_keys_ref.current = new Set(
      (props.repl.messages || [])
        .map((m) => String((m as any)?.meta?.wait_key || "").trim())
        .filter(Boolean)
    );
    seen_tool_call_ids_ref.current = new Set(
      (props.repl.messages || [])
        .map((m) => String((m as any)?.meta?._kind === "tool" ? (m as any)?.meta?.tool?.call_id || "" : "").trim())
        .filter(Boolean)
    );
    clear_follow_run();
    set_subworkflow_label("");

    if (abort_ref.current) abort_ref.current.abort();
    abort_ref.current = new AbortController();
    if (follow_abort_ref.current) follow_abort_ref.current.abort();
    follow_abort_ref.current = null;

    const run = async () => {
      try {
        // If attaching to an existing run (Sessions/Open), seed the UI with the original user prompt.
        //
        // Note: the durable ledger for subworkflow runs may only contain an "enriched prompt".
        // The original user prompt is typically stored in the root parent run input_data.
        if (!props.repl.messages.length) {
          try {
            const bundle = await props.gateway.get_run_history_bundle(rid, {
              include_subruns: false,
              include_session: true,
              session_turn_limit: 200,
              ledger_mode: "tail",
              ledger_max_items: 50,
            });
            if (stopped) return;

            const workflow_id = String(bundle?.run?.workflow_id || "").trim();
            const ref = parse_workflow_ref(workflow_id);
            if (ref) {
              const match = infer_agent_template_from_workflow_id(workflow_id, templates);
              update_repl((prev) => ({
                ...prev,
                template: { bundle_id: ref.bundle_id, flow_id: ref.flow_id, name: match?.name || prev.template?.name || ref.bundle_id },
                updated_at: now_iso(),
              }));
            }

            const seeded = seed_repl_messages_from_history_bundle(bundle, { now_iso });
            if (seeded.length) update_repl((prev) => ({ ...prev, messages: seeded, updated_at: now_iso() }));
          } catch {
            // best-effort
          }
        } else {
          // Even when we don't seed messages, try to keep the agent label consistent with the run.
          try {
            const bundle = await props.gateway.get_run_history_bundle(rid, { include_subruns: false, include_session: false, ledger_mode: "tail", ledger_max_items: 1 });
            if (stopped) return;
            const workflow_id = String(bundle?.run?.workflow_id || "").trim();
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
        const resume_after = Math.max(0, Number(cursor_by_run_ref.current[rid] || 0));
        await append_page(rid, Math.max(0, resume_after - 1));
        if (stopped) return;
        while (!stopped) {
          try {
            await props.gateway.stream_ledger(rid, {
              after: Number(cursor_by_run_ref.current[rid] || 0),
              signal: abort_ref.current?.signal,
              on_step: (ev) => {
                if (stopped) return;
                handle_record(ev, rid);
              },
            });
          } catch (e: any) {
            if (stopped) return;
            if (String(e?.name || "") === "AbortError") return;
            throw e;
          }

          if (stopped) return;

          try {
            await append_page(rid, Number(cursor_by_run_ref.current[rid] || 0));
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

  useEffect(() => {
    const rid = String(active_run_id || "").trim();
    const fid = String(follow_run_id || "").trim();
    if (!rid) return;
    if (!fid || fid === rid) return;

    let stopped = false;
    if (follow_abort_ref.current) follow_abort_ref.current.abort();
    const ctrl = new AbortController();
    follow_abort_ref.current = ctrl;

    const run = async () => {
      try {
        const ensure_cursor = () => {
          if (typeof cursor_by_run_ref.current[fid] !== "number") cursor_by_run_ref.current[fid] = 0;
        };
        ensure_cursor();
        try {
          await append_page(fid, Number(cursor_by_run_ref.current[fid] || 0));
        } catch {
          // ignore
        }
        if (stopped || ctrl.signal.aborted) return;

        while (!stopped && !ctrl.signal.aborted) {
          try {
            await props.gateway.stream_ledger(fid, {
              after: Number(cursor_by_run_ref.current[fid] || 0),
              signal: ctrl.signal,
              on_step: (ev) => {
                if (stopped) return;
                handle_record(ev, fid);
              },
            });
          } catch (e: any) {
            if (stopped) return;
            if (String(e?.name || "") === "AbortError") return;
            // Follow-stream errors are best-effort; retry after a brief delay.
          }
          if (stopped || ctrl.signal.aborted) return;

          try {
            await append_page(fid, Number(cursor_by_run_ref.current[fid] || 0));
          } catch {
            // ignore
          }
          if (stopped || ctrl.signal.aborted) return;

          // Streams may close while still active; poll + reconnect.
          let run_status = "";
          try {
            const info = await props.gateway.get_run(fid);
            if (stopped) return;
            run_status = String(info?.status || "").trim().toLowerCase();
          } catch {
            run_status = "";
          }
          if (run_status === "completed" || run_status === "failed" || run_status === "cancelled") return;

          await new Promise((r) => window.setTimeout(r, 900));
        }
      } catch {
        // ignore
      }
    };
    void run();

    return () => {
      stopped = true;
      ctrl.abort();
      if (follow_abort_ref.current === ctrl) follow_abort_ref.current = null;
    };
  }, [active_run_id, follow_run_id, props.gateway]);

  async function submit_resume(payload_obj: any): Promise<void> {
    const rid = String(wait_run_id || active_run_id || "").trim();
    if (!rid || !wait_key) return;
    set_error("");
    set_resuming(true);
    try {
      await props.gateway.submit_command({
        command_id: random_id(),
        run_id: rid,
        type: "resume",
        payload: { wait_key, payload: payload_obj || {} },
        client_id: props.settings.client_id || "abstractcode_web",
      });
      try {
        const after = Number(cursor_by_run_ref.current[rid] || 0);
        await append_page(rid, after);
      } catch {
        // ignore
      }
    } catch (e: any) {
      set_error(String(e?.message || e || "resume failed"));
    } finally {
      set_resuming(false);
    }
  }

  async function submit_cancel(): Promise<void> {
    const rid = String(root_run_ref.current || active_run_id || "").trim();
    if (!rid) return;
    set_error("");
    set_cancelling(true);
    try {
      await props.gateway.submit_command({
        command_id: random_id(),
        run_id: rid,
        type: "cancel",
        payload: {},
        client_id: props.settings.client_id || "abstractcode_web",
      });
    } catch (e: any) {
      set_error(String(e?.message || e || "cancel failed"));
    } finally {
      set_cancelling(false);
    }
  }

  async function submit_answer(text: string): Promise<void> {
    const t = String(text || "").trim();
    if (!t) return;
    append_message({ role: "user", content: t, ts: now_iso() });
    await submit_resume({ response: t });
  }

  const context_meter = useMemo(() => {
    const history = (props.repl.messages || []).filter((m) => m.role === "user" || m.role === "assistant");
    const joined = history.map((m) => `${m.role}: ${m.content}`).join("\n\n");
    const next_text = composer.trim() ? `${joined}\n\nuser: ${composer.trim()}` : joined;
    const text_tokens = Math.max(0, Math.ceil(next_text.length / 4));

    const files = (attached_files || []).filter((f) => !String(f.error || "").trim()).slice(0, 16);
    const unknown_files = files.filter((f) => !(typeof f.size_bytes === "number" && Number.isFinite(f.size_bytes) && f.size_bytes >= 0)).length;
    const files_bytes = files.reduce((acc, f) => {
      const sb = typeof f.size_bytes === "number" && Number.isFinite(f.size_bytes) && f.size_bytes >= 0 ? Math.trunc(f.size_bytes) : 0;
      return acc + sb;
    }, 0);
    const file_tokens = Math.ceil(files_bytes / 4) + unknown_files * 256;

    const used = Math.max(0, text_tokens + file_tokens);
    const caps = model_caps && typeof model_caps === "object" ? (model_caps as any).capabilities : null;
    const max_tokens_raw = caps && typeof caps === "object" ? Number((caps as any).max_tokens ?? 0) : 0;
    const max_tokens = Number.isFinite(max_tokens_raw) && max_tokens_raw > 0 ? Math.trunc(max_tokens_raw) : null;
    const pct = max_tokens ? (used / max_tokens) * 100 : 0;

    return { used, max_tokens, pct, text_tokens, file_tokens, file_count: files.length };
  }, [props.repl.messages, composer, attached_files, model_caps]);

  const ctx_used_label = format_tokens_k(context_meter.used);
  const ctx_max_label = context_meter.max_tokens ? format_tokens_k(context_meter.max_tokens) : "";
  const ctx_badge_label = ctx_max_label ? `${ctx_used_label}/${ctx_max_label}` : ctx_used_label;

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
  const file_target = useMemo(() => parse_file_target_query(file_query), [file_query]);

  const format_bytes_short = (n: number | null | undefined): string => {
    const v = typeof n === "number" && Number.isFinite(n) ? Math.max(0, n) : NaN;
    if (!Number.isFinite(v)) return "";
    if (v < 1024) return `${Math.trunc(v)} B`;
    const kb = v / 1024;
    if (kb < 1024) return `${Math.max(1, Math.round(kb))} KB`;
    const mb = kb / 1024;
    if (mb < 1024) return `${mb.toFixed(mb < 10 ? 1 : 0)} MB`;
    const gb = mb / 1024;
    return `${gb.toFixed(gb < 10 ? 1 : 0)} GB`;
  };

  useEffect(() => {
    set_file_active(0);
  }, [file_query, file_matches.length]);

  useEffect(() => {
    // When auth/settings change (new GatewayClient instance), allow file search again.
    file_search_blocked_until_ref.current = 0;
  }, [props.gateway]);

  useEffect(() => {
    if (!file_token) {
      set_file_matches([]);
      set_file_match_sizes({});
      set_file_error("");
      set_file_loading(false);
      return;
    }
    const parsed = file_target;
    if (parsed.target === "client") {
      set_file_matches([]);
      set_file_match_sizes({});
      set_file_error("");
      set_file_loading(false);
      return;
    }
    const q = String(parsed.query || "").trim();
    if (!q) {
      set_file_matches([]);
      set_file_match_sizes({});
      set_file_error("");
      set_file_loading(false);
      return;
    }

    let stopped = false;
    const now = Date.now();
    const blocked_until = Number(file_search_blocked_until_ref.current || 0);
    if (blocked_until > now) {
      const wait_s = Math.max(1, Math.ceil((blocked_until - now) / 1000));
      set_file_matches([]);
      set_file_match_sizes({});
      set_file_loading(false);
      set_file_error(`Gateway rate limited; retry in ${wait_s}s.`);
      return;
    }

    set_file_loading(true);
    set_file_error("");

    const ctrl = new AbortController();
    const handle = window.setTimeout(() => {
      void (async () => {
        try {
          const scope = (() => {
            const wr = String(props.settings.workspace_root || "").trim();
            const wm = String(props.settings.workspace_access_mode || "").trim();
            const wa = String(props.settings.workspace_allowed_paths || "").trim();
            const wi = String(props.settings.workspace_ignored_paths || "").trim();
            const enabled = Boolean(wr || wa || wi) || (wm && wm !== "workspace_only");
            if (!enabled) return undefined;
            return { workspace_root: wr, workspace_access_mode: wm, workspace_allowed_paths: wa, workspace_ignored_paths: wi };
          })();
          const res = await props.gateway.files_search(q, {
            limit: 12,
            signal: ctrl.signal,
            ...(scope ? { scope } : {}),
          });
          if (stopped) return;
          const items = Array.isArray(res?.items) ? res.items : [];
          const sizes: Record<string, number> = {};
          const paths = items
            .map((it: any) => {
              const p = String(it?.path || "").trim();
              const sb = it?.size_bytes;
              if (p && typeof sb === "number" && Number.isFinite(sb) && sb >= 0) sizes[p] = Math.trunc(sb);
              return p;
            })
            .filter(Boolean)
            .slice(0, 12);
          set_file_matches(paths);
          set_file_match_sizes(sizes);
        } catch (e: any) {
          if (stopped) return;
          if (String(e?.name || "") === "AbortError") return;
          set_file_matches([]);
          set_file_match_sizes({});
          if (e instanceof GatewayHttpError) {
            const status = Number((e as any).status || 0);
            if (status === 401 || status === 403) {
              file_search_blocked_until_ref.current = Date.now() + 30_000;
              set_file_error("Unauthorized. Set the Gateway auth token in Settings to search server files.");
              return;
            }
            if (status === 429) {
              const ra = typeof (e as any).retry_after_s === "number" && Number.isFinite((e as any).retry_after_s) ? Math.max(1, Math.trunc((e as any).retry_after_s)) : 30;
              file_search_blocked_until_ref.current = Date.now() + ra * 1000;
              set_file_error(`Gateway rate limited; retry in ${ra}s.`);
              return;
            }
          }
          set_file_error(String(e?.message || e || "File search failed"));
        } finally {
          if (!stopped) set_file_loading(false);
        }
      })();
    }, 240);

    return () => {
      stopped = true;
      window.clearTimeout(handle);
      ctrl.abort();
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
          "- `/files`",
          "- `/files-keep [on|off]`",
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

    if (cmd === "files") {
      const included = attached_files.filter((f) => !f.loading && !String(f.error || "").trim() && f.attachment);
      const excluded = attached_files.filter((f) => f.loading || String(f.error || "").trim() || !f.attachment);

      const lines: string[] = [];
      lines.push(`files_keep: ${props.settings.files_keep ? "on" : "off"}`);
      lines.push(`next_run: ${included.length}/${attached_files.length} ok`);
      if (attached_files.length) {
        lines.push("");
        for (const f of attached_files.slice(0, 50)) {
          const p = String(f.path || "").trim();
          const st = f.loading ? "loading" : String(f.error || "").trim() ? "error" : f.attachment ? "ok" : "none";
          const extra = st === "error" ? ` — ${String(f.error || "").trim()}` : "";
          const target_raw = String((f.attachment as any)?.target || "").trim().toLowerCase();
          const target: FileTarget =
            target_raw === "client" ? "client" : target_raw === "server" ? "server" : p.startsWith("client:") ? "client" : "server";
          lines.push(`- @${p} (${target}): ${st}${extra}`);
        }
      } else {
        lines.push("next_run: (none)");
      }
      if (excluded.length) {
        lines.push("");
        lines.push("Only ok files are included in next_run automatically.");
      }

      try {
        const run_id = await session_memory_owner_run_id(props.session_id);
        const res = await props.gateway.list_run_artifacts(run_id, { limit: 200 });
        const items = Array.isArray(res?.items) ? res.items : [];
        const attachments = items
          .filter((it: any) => it && typeof it === "object" && (it as any).tags && typeof (it as any).tags === "object")
          .filter((it: any) => String((it as any).tags?.kind || "").trim() === "attachment");
        lines.push("");
        lines.push(`session: ${attachments.length} attachment(s)`);
        if (!attachments.length) {
          lines.push("session: (none)");
        } else {
          for (const it of attachments.slice(0, 30)) {
            const tags: any = (it as any).tags || {};
            const handle = String(tags.path || tags.source_path || tags.filename || "").trim();
            const target_tag = String(tags.target || "").trim().toLowerCase();
            const target: FileTarget =
              target_tag === "client" ? "client" : target_tag === "server" ? "server" : handle.startsWith("client:") ? "client" : "server";
            const aid = String((it as any).artifact_id || "").trim();
            const sha = String(tags.sha256 || "").trim();
            const sha_disp = sha ? `${sha.slice(0, 8)}…` : "";
            const size = typeof (it as any).size_bytes === "number" ? format_bytes_short((it as any).size_bytes) : "";
            const meta_bits = [`target=${target}`, `id=${aid}`].concat(sha_disp ? [`sha=${sha_disp}`] : []).concat(size ? [size] : []).filter(Boolean);
            lines.push(`- @${handle || aid}${meta_bits.length ? ` (${meta_bits.join(", ")})` : ""}`);
          }
          if (attachments.length > 30) lines.push(`- …and ${attachments.length - 30} more`);
        }
      } catch (e: any) {
        lines.push("");
        lines.push(`session: (failed to load) ${String(e?.message || e || "")}`.trim());
      }

      if (!attached_files.length && !props.settings.files_keep) {
        lines.push("");
        lines.push("Tip: use /files-keep on to pin files across turns (next_run).");
      }
      lines.push("");
      lines.push("Open stored attachments via: open_attachment(handle='@…', start_line=..., end_line=...)");

      say(lines.join("\n"));
      return true;
    }

    if (cmd === "files-keep" || cmd === "files_keep" || cmd === "keep-files" || cmd === "keep_files") {
      if (!args.length) {
        say(`files_keep: ${props.settings.files_keep ? "on" : "off"}`);
        return true;
      }
      const raw = String(args[0] || "").trim().toLowerCase();
      const on = raw === "on" || raw === "true" || raw === "1" || raw === "yes";
      const off = raw === "off" || raw === "false" || raw === "0" || raw === "no";
      if (!on && !off) {
        say("Usage: /files-keep on|off");
        return true;
      }
      props.on_settings({ ...props.settings, files_keep: on });
      say(`files_keep set: ${on ? "on" : "off"}`);
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
      const rid = String(active_run_id || "").trim();
      if (rid) clear_run_cursor(rid);
      clear_active_run_id(props.session_id);
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

  function consume_token(token: ActiveToken | null): void {
    if (!token) return;
    if (token.start < 0 || token.end < token.start || token.end > composer.length) return;
    const before = composer.slice(0, token.start);
    const after = composer.slice(token.end);
    const next = `${before}${after}`.replace(/\s{2,}/g, " ");
    set_composer(next);
    set_composer_cursor(before.length);
  }

  async function attach_file(path: string, token: ActiveToken | null): Promise<void> {
    const p = String(path || "").trim();
    if (!p) return;

    const size_bytes = typeof (file_match_sizes as any)?.[p] === "number" && Number.isFinite((file_match_sizes as any)[p]) ? Math.max(0, Math.trunc((file_match_sizes as any)[p])) : undefined;

    // Clear the active @token from the composer (Cursor-style chips instead of inline tags).
    consume_token(token);
    set_file_matches([]);
    set_file_error("");
    set_file_loading(false);

    set_attached_files((prev) => {
      if (prev.some((f) => f.path === p)) return prev;
      return [...prev, { path: p, attachment: null, loading: true, size_bytes }].slice(-12);
    });

    try {
      const sid = String(props.session_id || "").trim();
      if (!sid) throw new Error("session_id is required");
      const scope = (() => {
        const wr = String(props.settings.workspace_root || "").trim();
        const wm = String(props.settings.workspace_access_mode || "").trim();
        const wa = String(props.settings.workspace_allowed_paths || "").trim();
        const wi = String(props.settings.workspace_ignored_paths || "").trim();
        const enabled = Boolean(wr || wa || wi) || (wm && wm !== "workspace_only");
        if (!enabled) return undefined;
        return { workspace_root: wr, workspace_access_mode: wm, workspace_allowed_paths: wa, workspace_ignored_paths: wi };
      })();
      const attachment = await props.gateway.attachments_ingest(sid, p, {
        ...(scope ? { scope } : {}),
      });
      set_attached_files((prev) =>
        prev.map((f) => (f.path === p ? { ...f, loading: false, attachment, error: undefined } : f))
      );
    } catch (e: any) {
      const msg = String(e?.message || e || "Failed to ingest attachment");
      set_attached_files((prev) => prev.map((f) => (f.path === p ? { ...f, loading: false, attachment: null, error: msg } : f)));
    } finally {
      try {
        input_ref.current?.focus();
      } catch {
        // ignore
      }
    }
  }

  function attach_client_picker(token: ActiveToken | null): void {
    consume_token(token);
    set_file_matches([]);
    set_file_error("");
    set_file_loading(false);
    try {
      upload_input_ref.current?.click();
    } catch {
      // ignore
    }
  }

  async function attach_upload(file: File): Promise<void> {
    if (!file) return;
    const sid = String(props.session_id || "").trim();
    if (!sid) {
      set_error("session_id is required");
      return;
    }
    const name = String((file as any)?.name || "").trim() || "upload.bin";
    const handle = `client:${name}`;
    const size_bytes = Number.isFinite(Number((file as any)?.size)) ? Math.max(0, Math.trunc(Number((file as any).size))) : undefined;

    set_attached_files((prev) => {
      const idx = prev.findIndex((f) => f.path === handle);
      if (idx >= 0) {
        const next = prev.slice();
        next[idx] = { ...next[idx], loading: true, error: undefined, size_bytes };
        return next;
      }
      return [...prev, { path: handle, attachment: null, loading: true, size_bytes }].slice(-12);
    });

    try {
      const attachment = await props.gateway.attachments_upload(sid, file, {
        filename: name,
        content_type: String((file as any)?.type || "").trim() || undefined,
      });
      set_attached_files((prev) => prev.map((f) => (f.path === handle ? { ...f, loading: false, attachment, error: undefined } : f)));
    } catch (e: any) {
      const msg = String(e?.message || e || "Upload failed");
      set_attached_files((prev) => prev.map((f) => (f.path === handle ? { ...f, loading: false, error: msg } : f)));
    }
  }

  async function attach_uploads(files: File[]): Promise<void> {
    const list = Array.isArray(files) ? files : [];
    if (!list.length) return;
    for (const f of list.slice(0, 8)) {
      await attach_upload(f);
    }
  }

  function remove_attached_file(path: string): void {
    const p = String(path || "").trim();
    if (!p) return;
    set_attached_files((prev) => prev.filter((f) => f.path !== p));
  }

  return (
    <div className="repl">
      <div className="panel repl_frame">
        <Header
          active="console"
          on_nav={props.on_nav}
          center={
            <div
              className="repl_context_badge"
              title={`Estimated next-run context: text≈${context_meter.text_tokens.toLocaleString()} tok; files≈${context_meter.file_tokens.toLocaleString()} tok (${context_meter.file_count} files).`}
            >
              <span className="mono">{ctx_badge_label}</span>
            </div>
          }
          monitor_gpu_enabled={props.monitor_gpu_enabled}
          monitor_gpu_ref={props.monitor_gpu_ref}
          gateway_url={props.gateway_url}
        />
        <div className="repl_panel">
          <div className="repl_inset">

        {template_error ? (
          <Notice
            variant={template_error.includes("switched to") ? "info" : "warn"}
            onClick={() => set_template_error("")}
            style={{ cursor: "pointer" }}
            title="Click to dismiss"
          >
            {template_error}
          </Notice>
        ) : null}
        {!props.settings.provider.trim() || !props.settings.model.trim() ? (
          <Notice variant="warn">Set provider + model in Settings. (These agent workflows require them.)</Notice>
        ) : null}
        {!props.settings.gateway_url.trim() ? (
          <Notice variant="warn">Set a Gateway URL in Settings (or host this app on the same origin as the gateway).</Notice>
        ) : null}

        {error ? <Notice variant="error">{error}</Notice> : null}

        <div className="repl_chat_wrap">
          <div
            className="repl_chat"
            ref={chat_scroll_ref}
            onScroll={(e) => {
              const el = e.currentTarget;
              // Treat "near bottom" as at-bottom so we keep following naturally.
              const threshold_px = 80;
              const at_bottom = el.scrollTop + el.clientHeight >= el.scrollHeight - threshold_px;
              chat_at_bottom_ref.current = at_bottom;
              set_chat_at_bottom(at_bottom);
            }}
          >
        <div className="repl_chat_content" ref={chat_content_ref}>
          {!props.repl.messages.length ? <div className="muted">Start typing to begin.</div> : null}
	          {props.repl.messages.map((m, idx) => (
	            <ChatMessageCard
	              key={`${m.ts}:${idx}`}
	              m={m}
	              gateway={props.gateway}
	              session_id={props.session_id}
	              context_badge_label={ctx_badge_label}
	              tool_specs_by_name={tool_specs_by_name}
	            />
	          ))}
          <div ref={chat_end_ref} />
        </div>
      </div>
          {!chat_at_bottom && props.repl.messages.length ? (
            <button
              className="btn scroll_to_bottom"
              type="button"
              aria-label="Scroll to latest"
              title="Scroll to latest"
              onClick={() => {
                const el = chat_scroll_ref.current;
                if (!el) return;
                el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
                chat_at_bottom_ref.current = true;
                set_chat_at_bottom(true);
              }}
            >
              <Icon name="chevronDown" size={16} />
              Latest
            </button>
          ) : null}
        </div>

        {active_run_id ? (
          <div className={wait_is_compact ? "repl_wait compact" : "repl_wait"}>
            {wait_state ? (
              <>
                {tool_calls_for_wait.length ? (
                  <>
                    <div className="wait_line shimmer" aria-live="polite">
                      <span className="mono">Waiting</span>
                      <span className="muted">for approval to run tools…</span>
                    </div>
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
                            ts={now_iso()}
                            tool_specs_by_name={tool_specs_by_name}
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
                ) : wait_reason === "subworkflow" ? (
                  <div className="thinking_line shimmer" aria-live="polite" title={wait_key ? `wait_key: ${wait_key}` : undefined}>
                    <span className="run_spinner" aria-label="working" />
                    <span className="thinking_label">Thinking…</span>
                    {iteration_badge ? <span className="thinking_iters mono">{iteration_badge}</span> : null}
                    <span className="thinking_spacer" />
                    <span className="muted mono thinking_detail">{subworkflow_label || template_label || "agent"}</span>
                  </div>
                ) : (
                  <div className="wait_line shimmer" aria-live="polite" title={wait_key ? `wait_key: ${wait_key}` : undefined}>
                    <span className="mono">Waiting</span>
                    <span className="muted">
                      for{" "}
                      {`${wait_reason || "unknown"}${wait_event_name ? `:${wait_event_name}` : ""}`}
                      …
                    </span>
                  </div>
                )}
              </>
            ) : is_working ? (
              <div className="thinking_line" aria-live="polite">
                <span className="run_spinner" aria-label="working" />
                <span className="thinking_label">Thinking…</span>
                {iteration_badge ? <span className="thinking_iters mono">{iteration_badge}</span> : null}
                <span className="thinking_spacer" />
                <span className="muted mono thinking_detail">{status_text}</span>
              </div>
            ) : null}
          </div>
        ) : null}

        {attached_files.length ? (
          <div className="file_chips">
            {attached_files.map((f) => {
              const p = String(f.path || "").trim();
              const cls = f.error ? "file_chip error" : f.loading ? "file_chip loading" : "file_chip";
              const icon: IconName = f.error ? "warning" : f.loading ? "loader" : "paperclip";
              const aid = String((f.attachment as any)?.$artifact || "").trim();
              const target_raw = String((f.attachment as any)?.target || "").trim().toLowerCase();
              const target: FileTarget = target_raw === "client" ? "client" : target_raw === "server" ? "server" : p.startsWith("client:") ? "client" : "server";
              const can_preview = !f.loading && !String(f.error || "").trim() && Boolean(aid);
              const tooltip = f.error ? String(f.error) : can_preview ? `@${p} (click to preview)` : p;
              return (
                <div
                  key={p}
                  className={cls}
                  title={tooltip}
                  role={can_preview ? "button" : undefined}
                  tabIndex={can_preview ? 0 : undefined}
                  onClick={can_preview ? () => set_attachment_preview(f.attachment) : undefined}
                  onKeyDown={
                    can_preview
                      ? (e) => {
                          if (e.key !== "Enter" && e.key !== " ") return;
                          e.preventDefault();
                          set_attachment_preview(f.attachment);
                        }
                      : undefined
                  }
                >
                  <span className="file_chip_icon" aria-hidden="true">
                    <Icon name={icon} size={14} className={f.loading ? "spin" : undefined} />
                  </span>
                  <span className={`file_chip_target ${target}`}>{target}</span>
                  <span className="mono">@{p}</span>
                  {f.loading ? <span className="muted">ingesting…</span> : null}
                  {f.error ? <span className="muted">{String(f.error)}</span> : null}
                  <button
                    className="chip_remove"
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      remove_attached_file(p);
                    }}
                    aria-label="Remove file"
                  >
                    ×
                  </button>
                </div>
              );
            })}
          </div>
        ) : null}

        {file_token ? (
          <div className="cmd_menu">
            {file_target.target === "client" ? (
              <button className="cmd_item active" type="button" onClick={() => attach_client_picker(file_token)}>
                <span className="mono">@client:</span>
                <span className="muted">upload from device…</span>
              </button>
            ) : null}

            {file_target.target !== "client" ? (
              <button className="cmd_item" type="button" onClick={() => attach_client_picker(file_token)} title="Upload a local file from this device">
                <span className="mono">@client:</span>
                <span className="muted">upload from device…</span>
              </button>
            ) : null}

            {file_target.target !== "client" ? (
              <>
                {file_loading ? <div className="cmd_notice muted">Searching files…</div> : null}
                {file_error ? <div className="cmd_notice error">{file_error}</div> : null}
                {!file_loading && !file_error && !file_matches.length ? (
                  <div className="cmd_notice muted">{file_target.query ? "No matches." : "Type to search server files (`@server:…`) or upload (`@client:`)."}</div>
                ) : null}
                {file_matches.map((p, idx) => (
                  <button
                    key={p}
                    className={`cmd_item ${idx === file_active ? "active" : ""}`}
                    type="button"
                    onClick={() => void attach_file(p, file_token)}
                  >
                    <span className="mono">@{p}</span>
                    <span className="muted">{format_bytes_short(file_match_sizes[p]) || "attach"}</span>
                  </button>
                ))}
              </>
            ) : null}
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

          </div>
        <div className="repl_composer">
          <div
            className={drop_active ? "repl_input drop_active" : "repl_input"}
            onDragEnter={(e) => {
              try {
                if (!e.dataTransfer?.types?.includes?.("Files")) return;
              } catch {
                // ignore
              }
              e.preventDefault();
              set_drop_active(true);
            }}
            onDragOver={(e) => {
              try {
                if (!e.dataTransfer?.types?.includes?.("Files")) return;
                e.dataTransfer.dropEffect = "copy";
              } catch {
                // ignore
              }
              e.preventDefault();
              set_drop_active(true);
            }}
            onDragLeave={() => set_drop_active(false)}
            onDrop={(e) => {
              e.preventDefault();
              e.stopPropagation();
              set_drop_active(false);
              const files = Array.from(e.dataTransfer?.files || []);
              if (!files.length) return;
              void attach_uploads(files);
            }}
	          >
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
	              placeholder={!can_type ? "Waiting for the current run…" : pending_files ? "Loading attached files…" : "Type a message and use @ to attach files"}
	              disabled={!can_type}
	              onKeyDown={(e) => {
	                if (file_token) {
	                  if (file_target.target === "client") {
                    if (e.key === "Tab") {
                      e.preventDefault();
                      attach_client_picker(file_token);
                      return;
                    }
                  }
                  if (file_target.target !== "client") {
                    if (e.key === "Tab" && !file_loading && !file_error && !file_matches.length) {
                      const raw = String(file_target.query || "").trim();
                      if (raw) {
                        e.preventDefault();
                        void attach_file(raw, file_token);
                        return;
                      }
                    }
                  }
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
	          </div>
	          <div className="repl_send_panel">
	            <div className="composer_shortcuts" aria-hidden="true">
	              <span>
	                <kbd>Enter</kbd> send
	              </span>
	              <span>
	                <kbd>Shift</kbd> <kbd>Enter</kbd> nl
	              </span>
	              <span>
	                <kbd>/</kbd> cmd
	              </span>
	            </div>
	            <div className="repl_send_row">
	              <button
	                className="btn attach_btn"
                type="button"
                title="Attach files from this device"
                aria-label="Attach files from this device"
                onClick={() => {
                  try {
                    upload_input_ref.current?.click();
                  } catch {
                    // ignore
                  }
                }}
              >
                <Icon name="paperclip" size={16} />
                <span className="attach_btn_label">Attach</span>
              </button>
              <button
                className={`btn ${active_run_id ? "danger cancel_btn" : "primary"} send_btn`}
                disabled={active_run_id ? cancelling : !can_send || !composer.trim()}
                onClick={() => {
                  if (active_run_id) {
                    void submit_cancel();
                    return;
                  }
                  const v = composer;
                  set_composer("");
                  void (async () => {
                    const handled = await run_command(v);
                    if (handled) return;
                    await start_turn(v);
                  })();
                }}
                type="button"
              >
                <Icon name={active_run_id ? "x" : "send"} size={16} />
                <span className="send_btn_label">{active_run_id ? (cancelling ? "Cancelling…" : "Cancel") : "Send"}</span>
              </button>
              <input
                ref={upload_input_ref}
                type="file"
                multiple
                style={{ display: "none" }}
                onChange={(e) => {
                  const files = Array.from(e.currentTarget.files || []);
                  e.currentTarget.value = "";
                  if (!files.length) return;
                  void attach_uploads(files);
                }}
              />
            </div>
          </div>
        </div>

        {attachment_preview ? (
          <AttachmentPreviewModal
            gateway={props.gateway}
            session_id={props.session_id}
            attachment={attachment_preview}
            on_close={() => set_attachment_preview(null)}
          />
        ) : null}

      </div>
      </div>

    </div>
  );
}

function ChatMessageCard(props: {
  m: ReplMessage;
  gateway: GatewayClient;
  session_id: string;
  context_badge_label?: string;
  tool_specs_by_name?: Record<string, any>;
}): React.ReactElement | null {
  const m = props.m;
  const meta_obj: any = m.meta && typeof m.meta === "object" ? (m.meta as any) : null;
  const kind = meta_obj && typeof meta_obj._kind === "string" ? String(meta_obj._kind) : "";
  const attachments = Array.isArray(meta_obj?.attachments) ? (meta_obj.attachments as any[]) : [];
  const attachment_items: AttachmentRef[] = attachments
    .filter((a) => a && typeof a === "object" && !Array.isArray(a))
    .slice(0, 16)
    .map((a) => ({ ...(a as any) }));
  const repl_meta = meta_obj && meta_obj._repl && typeof meta_obj._repl === "object" ? (meta_obj._repl as any) : null;
  const usage = repl_meta && repl_meta.usage && typeof repl_meta.usage === "object" ? (repl_meta.usage as any) : null;
  const usage_parsed = parse_usage_summary(usage);
  const dur_ms = repl_meta && Number.isFinite(Number(repl_meta.duration_ms)) ? Number(repl_meta.duration_ms) : null;
  const tok_s = repl_meta && Number.isFinite(Number(repl_meta.tok_s)) ? Number(repl_meta.tok_s) : null;
  const llm_calls = repl_meta && Number.isFinite(Number(repl_meta.llm_calls)) ? Number(repl_meta.llm_calls) : null;
  const tool_calls = repl_meta && Number.isFinite(Number(repl_meta.tool_calls)) ? Number(repl_meta.tool_calls) : null;

  const role_config: Record<string, { label: string; icon: IconName }> = {
    user: { label: "You", icon: "user" },
    assistant: { label: "Agent", icon: "bot" },
    system: {
      label: m.title || (m.level === "error" ? "Error" : m.level === "warn" ? "Warning" : "System"),
      icon: m.level === "error" ? "error" : m.level === "warn" ? "warning" : "info",
    },
  };
  const role_info = role_config[m.role] || role_config.system;
  
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
  const [copy_state, set_copy_state] = useState<"idle" | "copied" | "failed">("idle");

  const is_digest = m.role === "system" && String(m.title || "").trim() === "Digest" && (m.level || "info") === "info";
  const run_id = String(m.run_id || "").trim();
  const [ctx_open, set_ctx_open] = useState(false);
  const [attachment_open, set_attachment_open] = useState<AttachmentRef | null>(null);
  const workflow_meta = meta_obj && meta_obj.workflow_meta && typeof meta_obj.workflow_meta === "object" ? (meta_obj.workflow_meta as any) : null;
  const inspect_run_id =
    String(workflow_meta?.context_appended_sub_run_id || "").trim() ||
    String(workflow_meta?.sub_run_id || "").trim() ||
    String(workflow_meta?.llm_run_id || "").trim() ||
    run_id;
  
  if (kind === "tool") {
    return (
      <div className="chat_item tool_item">
        <ToolBlockCard meta={meta_obj?.tool} ts={m.ts} tool_specs_by_name={props.tool_specs_by_name} />
      </div>
    );
  }

  if (kind === "run_digest") {
    const d: any = meta_obj?.digest && typeof meta_obj.digest === "object" ? meta_obj.digest : {};
    const outcome = String(d?.outcome || "").trim() || String(m.level || "").trim() || "completed";
    const title = outcome === "failed" ? "Run failed" : outcome === "cancelled" ? "Run cancelled" : "Run completed";
    const duration_ms = Number.isFinite(Number(d?.duration_ms)) ? Number(d.duration_ms) : 0;
    const llm_calls2 = Number.isFinite(Number(d?.llm_calls)) ? Number(d.llm_calls) : 0;
    const tool_calls2 = Number.isFinite(Number(d?.tool_calls)) ? Number(d.tool_calls) : 0;
    const tokens = d?.tokens && typeof d.tokens === "object" ? d.tokens : null;
    const speed_tok_s = Number.isFinite(Number(d?.speed_tok_s)) ? Number(d.speed_tok_s) : null;
    const tools = Array.isArray(d?.tools) ? (d.tools as any[]).map((x) => String(x || "").trim()).filter(Boolean) : [];

    return (
      <div className={`chat_item ${cls}`}>
        <div className="chat_header">
          <div className="chat_avatar" aria-hidden="true">
            <Icon name={outcome === "failed" ? "error" : outcome === "cancelled" ? "warning" : "info"} size={14} />
          </div>
          <span className="chat_role">{title}</span>
          <span className="chat_header_spacer" />
          <span className="chat_time">{new Date(m.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span>
        </div>
        <div className="run_digest_card">
          <div className="run_digest_grid">
            <div className="run_digest_item">
              <span className="muted">duration</span>
              <span className="mono">{format_duration_short(duration_ms)}</span>
            </div>
            <div className="run_digest_item">
              <span className="muted">llm</span>
              <span className="mono">{llm_calls2}</span>
            </div>
            <div className="run_digest_item">
              <span className="muted">tools</span>
              <span className="mono">{tool_calls2}</span>
            </div>
            <div className="run_digest_item">
              <span className="muted">tokens</span>
              <span className="mono">
                {tokens && typeof tokens?.total === "number" ? `${tokens.total}` : "—"}
              </span>
            </div>
            <div className="run_digest_item">
              <span className="muted">speed</span>
              <span className="mono">{speed_tok_s != null ? `${speed_tok_s.toFixed(1)} tok/s` : "—"}</span>
            </div>
          </div>

          {tools.length ? (
            <details className="run_digest_tools">
              <summary className="run_digest_tools_summary">
                <span className="mono">tools used</span>
                <span className="muted">({tools.length})</span>
              </summary>
              <div className="run_digest_tools_list">
                {tools.slice(0, 120).map((t, i) => (
                  <div key={`${t}:${i}`} className="mono run_digest_tool">
                    {t}
                  </div>
                ))}
                {tools.length > 120 ? <div className="muted">…and {tools.length - 120} more</div> : null}
              </div>
            </details>
          ) : null}
        </div>
      </div>
    );
  }
  
  // Digest messages are now hidden - their info is in the assistant message stats
  if (is_digest) {
    return null;
  }

  // Build stats items for assistant messages
  const has_stats = m.role === "assistant" && (Boolean(run_id) || usage_parsed || dur_ms !== null || llm_calls !== null || tool_calls !== null);
  
  return (
    <div className={`chat_item ${cls}`}>
      <div className="chat_header">
        <div className="chat_avatar" aria-hidden="true">
          <Icon name={role_info.icon} size={14} />
        </div>
        <span className="chat_role">{role_info.label}</span>
        <span className="chat_header_spacer" />
        <span className="chat_time">{new Date(m.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span>
        <button
          className={`btn mini chat_copy ${copy_state}`}
          onClick={async () => {
            const ok = await copy_text(String(m.content || ""));
            set_copy_state(ok ? "copied" : "failed");
            window.setTimeout(() => set_copy_state("idle"), 900);
          }}
          type="button"
          aria-label="Copy message"
        >
          {copy_state === "copied" ? <Icon name="check" size={14} /> : copy_state === "failed" ? <Icon name="x" size={14} /> : <Icon name="copy" size={14} />}
        </button>
      </div>
      <div className="body markdown">
        <ChatMessageContent text={m.content} renderMarkdown={(markdown) => <MarkdownRenderer markdown={markdown} />} />
      </div>
      {attachment_items.length ? (
        <div className="chat_attachments" aria-label="Attachments">
          {attachment_items.map((a, idx) => {
            const aid = String((a as any)?.$artifact || "").trim();
            const source = String((a as any)?.source_path || (a as any)?.filename || "").trim();
            const target_raw = String((a as any)?.target || "").trim().toLowerCase();
            const target: FileTarget = target_raw === "client" ? "client" : target_raw === "server" ? "server" : source.startsWith("client:") ? "client" : "server";
            const label = (source || aid).split("/").pop() || source || aid || "attachment";
            const title = source ? `@${source}` : aid ? `artifact: ${aid}` : "attachment";
            const can_preview = Boolean(aid);
            return (
              <button
                key={`${aid || label}:${idx}`}
                type="button"
                className="chat_attachment_chip"
                title={can_preview ? `${title} (click to preview)` : title}
                onClick={can_preview ? () => set_attachment_open(a) : undefined}
                disabled={!can_preview}
              >
                <Icon name="paperclip" size={14} />
                <span className={`chat_attachment_target ${target}`}>{target}</span>
                <span className="mono chat_attachment_name">{label}</span>
              </button>
            );
          })}
        </div>
      ) : null}
      {has_stats ? (
        <div className="chat_stats_bar">
          {run_id ? (
            <button
              className="stat_item clickable"
              type="button"
              title="Inspect system prompt, user prompt, and tool calls"
              onClick={() => set_ctx_open(true)}
            >
              <span className="stat_icon">☰</span>
              context
            </button>
          ) : null}
          {usage_parsed ? (
            <span className="stat_item" title="Tokens in/out/total">
              <span className="stat_icon">◈</span>
              {usage_parsed.input_tokens}/{usage_parsed.output_tokens}
            </span>
          ) : null}
          {dur_ms !== null ? (
            <span className="stat_item" title="Duration">
              <span className="stat_icon">◷</span>
              {format_duration_short(dur_ms)}
            </span>
          ) : null}
          {tok_s !== null ? (
            <span className="stat_item" title="Tokens per second">
              <span className="stat_icon">◐</span>
              {tok_s.toFixed(0)}/s
            </span>
          ) : null}
          {llm_calls !== null && llm_calls > 0 ? (
            <span className="stat_item" title="LLM calls">
              <span className="stat_icon">◉</span>
              {llm_calls}
            </span>
          ) : null}
          {tool_calls !== null && tool_calls > 0 ? (
            <span className="stat_item" title="Tool calls">
              <span className="stat_icon">⚙</span>
              {tool_calls}
            </span>
          ) : null}
        </div>
      ) : null}
      {ctx_open && run_id ? (
        <ContextInspectorModal
          gateway={props.gateway}
          root_run_id={run_id}
          inspect_run_id={inspect_run_id}
          context_badge_label={props.context_badge_label}
          on_close={() => set_ctx_open(false)}
        />
      ) : null}
      {attachment_open ? (
        <AttachmentPreviewModal
          gateway={props.gateway}
          session_id={props.session_id}
          attachment={attachment_open}
          on_close={() => set_attachment_open(null)}
        />
      ) : null}
    </div>
  );
}

function ContextInspectorModal(props: {
  gateway: GatewayClient;
  root_run_id: string;
  inspect_run_id: string;
  context_badge_label?: string;
  on_close: () => void;
}): React.ReactElement {
  const [selected_run_id, set_selected_run_id] = useState<string>(props.inspect_run_id);
  const [run_options, set_run_options] = useState<string[]>([]);
  const [ledger_len_by_run, set_ledger_len_by_run] = useState<Record<string, number>>({});
  const [discovering, set_discovering] = useState(true);
  const [loading, set_loading] = useState(true);
  const [error, set_error] = useState("");
  const [ledger_items, set_ledger_items] = useState<LedgerRecordItem[]>([]);

  useEffect(() => {
    const on_key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      props.on_close();
    };
    window.addEventListener("keydown", on_key);
    return () => window.removeEventListener("keydown", on_key);
  }, [props.on_close]);

  useEffect(() => {
    // Reset when the requested run changes (new message / different subrun).
    set_selected_run_id(props.inspect_run_id);
    set_run_options([]);
    set_ledger_len_by_run({});
    set_discovering(true);
  }, [props.inspect_run_id]);

  useEffect(() => {
    let cancelled = false;
    set_discovering(true);

    const uniq = (xs: string[]): string[] => {
      const out: string[] = [];
      const seen = new Set<string>();
      for (const x of xs) {
        const s = String(x || "").trim();
        if (!s || seen.has(s)) continue;
        seen.add(s);
        out.push(s);
      }
      return out;
    };

    const list_descendants = (runs: any[], root_run_id: string): string[] => {
      const children_by_parent = new Map<string, string[]>();
      for (const r of runs) {
        const rid = String(r?.run_id || "").trim();
        if (!rid) continue;
        const parent = String(r?.parent_run_id || "").trim();
        if (!parent) continue;
        const prev = children_by_parent.get(parent) || [];
        prev.push(rid);
        children_by_parent.set(parent, prev);
      }

      const out: string[] = [];
      const queue: string[] = [root_run_id];
      const seen = new Set<string>();
      while (queue.length && out.length < 500) {
        const cur = String(queue.shift() || "").trim();
        if (!cur || seen.has(cur)) continue;
        seen.add(cur);
        const kids = children_by_parent.get(cur) || [];
        for (const k of kids) {
          const kk = String(k || "").trim();
          if (!kk || seen.has(kk)) continue;
          out.push(kk);
          queue.push(kk);
        }
      }
      return out;
    };

    void (async () => {
      try {
        const base_candidates = uniq([props.inspect_run_id, props.root_run_id]);

        // Best-effort: expand candidates to include this root run's subruns so Context works
        // even when workflow_meta points to a run with an empty ledger.
        let expanded = base_candidates.slice();
        let run_lens: Record<string, number> = {};
        try {
          const root = await props.gateway.get_run(props.root_run_id);
          const session_id = String(root?.session_id || "").trim();
          if (session_id) {
            const runs_res = await props.gateway.list_runs({ limit: 500, session_id, include_ledger_len: false, include_metrics: false });
            const runs = Array.isArray((runs_res as any)?.items) ? ((runs_res as any).items as any[]) : [];
            for (const r of runs) {
              const rid = String(r?.run_id || "").trim();
              const ll = r?.ledger_len;
              if (!rid) continue;
              if (typeof ll === "number" && Number.isFinite(ll) && ll >= 0) run_lens[rid] = Math.max(0, Math.trunc(ll));
            }
            const descendants = list_descendants(runs, props.root_run_id);
            expanded = uniq([...base_candidates, ...descendants]);
          }
        } catch {
          // ignore
        }

        if (cancelled) return;
        set_run_options(expanded);
        set_ledger_len_by_run(run_lens);

        // Auto-pick a run that actually has LLM/tool activity (so the inspector isn't empty).
        const INTERESTING = new Set(["llm_call", "tool_calls", "ask_user", "answer_user"]);
        const has_trace_preview = async (run_id: string): Promise<boolean> => {
          const rid = String(run_id || "").trim();
          if (!rid) return false;
          try {
            const res = await props.gateway.get_ledger(rid, { after: 0, limit: 200 });
            const page = Array.isArray(res?.items) ? res.items : [];
            return page.some((rec: any) => INTERESTING.has(String(rec?.effect?.type || "").trim()));
          } catch {
            return false;
          }
        };

        const ordered = uniq([props.inspect_run_id, props.root_run_id, ...expanded]);
        let picked = props.inspect_run_id;
        for (const rid of ordered.slice(0, 12)) {
          if (await has_trace_preview(rid)) {
            picked = rid;
            break;
          }
        }

        if (!cancelled) set_selected_run_id(picked);
      } catch (e: any) {
        if (cancelled) return;
        set_run_options(uniq([props.inspect_run_id, props.root_run_id]));
      } finally {
        if (!cancelled) set_discovering(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [props.gateway, props.inspect_run_id, props.root_run_id]);

  useEffect(() => {
    let cancelled = false;
    set_loading(true);
    set_error("");
    set_ledger_items([]);

    const fetch_all_ledger = async (run_id: string) => {
      const rid = String(run_id || "").trim();
      if (!rid) return [];
      const max_items = 2000;
      const bundle = await props.gateway.get_run_history_bundle(rid, {
        include_subruns: false,
        include_session: false,
        ledger_mode: "tail",
        ledger_max_items: max_items,
      });
      if (cancelled) return [];
      const ledgers = bundle?.ledgers && typeof bundle.ledgers === "object" ? (bundle.ledgers as any) : null;
      const ledger = ledgers && ledgers[rid] && typeof ledgers[rid] === "object" ? (ledgers[rid] as any) : null;
      const items_raw = Array.isArray(ledger?.items) ? (ledger.items as any[]) : [];
      const out: LedgerRecordItem[] = [];
      for (const it of items_raw) {
        if (!it || typeof it !== "object") continue;
        const cursor = Number((it as any).cursor);
        const record = (it as any).record;
        if (!Number.isFinite(cursor)) continue;
        if (!record || typeof record !== "object") continue;
        out.push({ run_id: rid, cursor, record: record as StepRecord });
      }
      return out;
    };

    void (async () => {
      try {
        const ledger_items = await fetch_all_ledger(selected_run_id);
        if (cancelled) return;
        set_ledger_items(ledger_items);
      } catch (e: any) {
        if (cancelled) return;
        set_error(String(e?.message || e || "Failed to load context"));
      } finally {
        if (!cancelled) set_loading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [props.gateway, selected_run_id, ledger_len_by_run[selected_run_id]]);

  const trace = useMemo(() => build_agent_trace(ledger_items, { run_id: selected_run_id }), [ledger_items, selected_run_id]);
  const is_loading = discovering || loading;

  return (
    <div
      className="modal_overlay"
      role="dialog"
      aria-modal="true"
      aria-label="Context inspector"
      onMouseDown={() => props.on_close()}
    >
	      <div className="modal_card" onMouseDown={(e) => e.stopPropagation()}>
	        <div className="modal_header">
	          <div style={{ minWidth: 0 }}>
	            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
	              <div className="modal_title">Context</div>
	              {props.context_badge_label ? (
	                <div className="repl_context_badge modal_ctx_badge" title="Estimated next-run context">
	                  <span className="mono">{props.context_badge_label}</span>
	                </div>
	              ) : null}
	            </div>
	            <div className="muted mono" style={{ marginTop: 2 }}>
	              run_id:{" "}
	              {run_options.length > 1 ? (
	                <select
                  className="mono"
                  value={selected_run_id}
                  onChange={(e) => set_selected_run_id(String(e.target.value || "").trim())}
                  style={{ maxWidth: 520 }}
                >
                  {run_options.map((rid) => (
                    <option key={rid} value={rid}>
                      {rid}
                    </option>
                  ))}
                </select>
              ) : (
                selected_run_id
              )}
              {selected_run_id !== props.root_run_id ? (
                <>
                  {" "}
                  (from root {props.root_run_id})
                </>
              ) : null}
            </div>
          </div>
          <button className="btn mini modal_close_btn" type="button" onClick={() => props.on_close()} aria-label="Close context inspector">
            <Icon name="x" size={14} />
            <span className="modal_close_label">Close</span>
          </button>
        </div>

        <div className="modal_body">
          {is_loading ? (
            <div className="context_modal_loading">
              <div className="thinking_line shimmer">
                <span className="run_spinner" aria-hidden="true" />
                <span className="thinking_label">Reconstructing context…</span>
                <span className="thinking_spacer" />
                <span className="thinking_iters">This can take ~30s over remote gateways</span>
              </div>
              <div className="muted" style={{ marginTop: 10, lineHeight: 1.5 }}>
                Loading agent trace (LLM/tool calls) from the durable ledger…
              </div>
            </div>
          ) : error ? (
            <Notice variant="error" style={{ marginTop: 12 }}>
              {error}
            </Notice>
          ) : !trace.items.length ? (
            <Notice variant="info" style={{ marginTop: 12 }}>
              No trace entries found for this run. Try selecting a different <span className="mono">run_id</span> from the dropdown.
            </Notice>
          ) : (
            <AgentCyclesPanel
              items={trace.items}
              title="Agent"
              subtitle={trace.node_id ? `node_id: ${trace.node_id}` : "Agent trace (LLM/tool calls)."}
              subRunId={selected_run_id}
              defaultOpenLatest={true}
            />
          )}
        </div>
      </div>
    </div>
  );
}

function AttachmentPreviewModal(props: {
  gateway: GatewayClient;
  session_id: string;
  attachment: AttachmentRef;
  on_close: () => void;
}): React.ReactElement {
  const artifact_id = String((props.attachment as any)?.$artifact || "").trim();
  const source_path = String((props.attachment as any)?.source_path || (props.attachment as any)?.filename || "").trim();
  const sha256 = String((props.attachment as any)?.sha256 || "").trim();
  const label = (source_path || artifact_id).split("/").pop() || source_path || artifact_id || "attachment";

  const [loading, set_loading] = useState(true);
  const [error, set_error] = useState("");
  const [text, set_text] = useState("");

  useEffect(() => {
    const on_key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      props.on_close();
    };
    window.addEventListener("keydown", on_key);
    return () => window.removeEventListener("keydown", on_key);
  }, [props.on_close]);

  useEffect(() => {
    let cancelled = false;
    set_loading(true);
    set_error("");
    set_text("");

    void (async () => {
      try {
        if (!artifact_id) throw new Error("Missing attachment artifact id");
        const run_id = await session_memory_owner_run_id(props.session_id);
        const t = await props.gateway.get_run_artifact_text(run_id, artifact_id, { max_bytes: 600_000 });
        if (cancelled) return;
        set_text(String(t || ""));
      } catch (e: any) {
        if (cancelled) return;
        set_error(String(e?.message || e || "Failed to load attachment"));
      } finally {
        if (!cancelled) set_loading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [props.gateway, props.session_id, artifact_id]);

  return (
    <div className="modal_overlay" role="dialog" aria-modal="true" aria-label="Attachment preview" onMouseDown={() => props.on_close()}>
      <div className="modal_card" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal_header">
          <div style={{ minWidth: 0 }}>
            <div className="modal_title">Attachment</div>
            <div className="muted mono" style={{ marginTop: 2, overflowWrap: "anywhere" }}>
              {label}
              {source_path ? ` • @${source_path}` : ""}
              {sha256 ? ` • sha=${sha256.slice(0, 8)}…` : ""}
            </div>
          </div>
          <button className="btn mini modal_close_btn" type="button" onClick={() => props.on_close()} aria-label="Close attachment preview">
            <Icon name="x" size={14} />
            <span className="modal_close_label">Close</span>
          </button>
        </div>
        <div className="modal_body">
          {loading ? (
            <div className="muted" style={{ marginTop: 12 }}>
              Loading…
            </div>
          ) : error ? (
            <Notice variant="error" style={{ marginTop: 12 }}>
              {error}
              {error.toLowerCase().includes("too large") ? (
                <>
                  {" "}
                  Try using the `open_attachment` tool for a bounded excerpt instead.
                </>
              ) : null}
            </Notice>
          ) : (
            <div className="attachment_preview">
              <MarkdownRenderer markdown={`\`\`\`\n${text}\n\`\`\``} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function ToolBlockCard(props: { meta: any; ts?: string; tool_specs_by_name?: Record<string, any> }): React.ReactElement {
  const t: any = props.meta && typeof props.meta === "object" ? props.meta : {};
  const name = String(t.name || "").trim() || "(unknown tool)";
  const call_id = String(t.call_id || "").trim();
  const pending = t.pending === true;
  const success_raw = t.success;
  const success = typeof success_raw === "boolean" ? success_raw : null;
  const error = String(t.error || "").trim();
  const args = t.arguments;
  const output_preview = String(t.output_preview || "").trim();
  const ts = String(props.ts || "").trim();
  const tool_specs_by_name = props.tool_specs_by_name || {};
  const tool_spec = tool_specs_by_name[name];
  const sig = tool_call_signature_primary(name, args, tool_spec);
  const [copy_state, set_copy_state] = useState<"idle" | "copied" | "failed">("idle");

  const status = pending ? "pending" : error ? "error" : success === false ? "failed" : success === true ? "ok" : "done";
  const status_cls =
    status === "pending"
      ? "tool_pending"
      : status === "error" || status === "failed"
        ? "tool_error"
        : status === "ok"
          ? "tool_ok"
          : "tool_done";
  const output_trim = output_preview.trim();
  const output_is_json = output_trim.startsWith("{") || output_trim.startsWith("[");

  return (
    <details className={`tool_block ${status_cls}`}>
      <summary className="tool_summary">
        <div className="tool_left">
          <span className="mono tool_sig">{sig}</span>
        </div>
        <div className="tool_right">
          <span className="muted mono tool_time">{ts ? new Date(ts).toLocaleTimeString() : ""}</span>
          <button
            className={`btn mini tool_copy_btn ${copy_state}`}
            type="button"
            aria-label="Copy tool call"
            onClick={async (e) => {
              e.preventDefault();
              e.stopPropagation();
              const payload = safe_json({
                name,
                call_id: call_id || null,
                status,
                success,
                error: error || null,
                arguments: args ?? null,
                output_preview: output_preview || null,
              });
              const ok = await copy_text(payload);
              set_copy_state(ok ? "copied" : "failed");
              window.setTimeout(() => set_copy_state("idle"), 900);
            }}
          >
            {copy_state === "copied" ? <Icon name="check" size={14} /> : copy_state === "failed" ? <Icon name="x" size={14} /> : <Icon name="copy" size={14} />}
          </button>
        </div>
      </summary>

      <div className="tool_body">
        {error ? <div className="tool_error_text">Error: {error}</div> : null}
        {pending ? <div className="muted">Awaiting approval / execution.</div> : null}
        {call_id ? <div className="muted mono tool_call_id">call_id: {call_id}</div> : null}
        <div className="field">
          <label>arguments</label>
          <div className="tool_json_preview">
            <MarkdownRenderer markdown={`\`\`\`json\n${safe_json(args)}\n\`\`\``} />
          </div>
        </div>
        {!pending || output_preview ? (
          <div className="field">
            <label>output</label>
            <div className="tool_json_preview">
              <MarkdownRenderer markdown={`\`\`\`${output_is_json ? "json" : ""}\n${output_preview}\n\`\`\``} />
            </div>
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
