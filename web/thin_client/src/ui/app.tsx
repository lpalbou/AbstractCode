import React, { useEffect, useMemo, useRef, useState } from "react";

import { GatewayClient } from "../lib/gateway_client";
import { random_id } from "../lib/ids";
import { McpWorkerClient } from "../lib/mcp_worker_client";
import { extract_emit_event, extract_tool_calls_from_wait, extract_wait_from_record } from "../lib/runtime_extractors";
import { LedgerStreamEvent, StepRecord, ToolCall, ToolResult, WaitState } from "../lib/types";
import { FlowGraph } from "./flow_graph";
import { JsonViewer } from "./json_viewer";
import { MultiSelect } from "./multi_select";

type Settings = {
  gateway_url: string;
  auth_token: string;
  worker_url: string;
  worker_token: string;
};

type UiLogItem = {
  id: string;
  ts: string;
  kind: "step" | "event" | "message" | "error" | "info";
  title: string;
  preview?: string;
  data?: any;
  cursor?: number;
  run_id?: string;
  node_id?: string;
  status?: string;
  effect_type?: string;
  emit_name?: string;
};

type BundlePinDef = {
  id: string;
  label?: string;
  type?: string;
  default?: any;
};

type BundleEntrypoint = {
  flow_id?: string;
  workflow_id?: string | null;
  name?: string | null;
  description?: string;
  interfaces?: string[];
  inputs?: BundlePinDef[];
  node_index?: Record<string, any>;
};

type BundleInfo = {
  bundle_id?: string;
  bundle_version?: string;
  created_at?: string;
  default_entrypoint?: string | null;
  entrypoints?: BundleEntrypoint[];
  flows?: string[];
  metadata?: any;
};

type WorkflowOption = {
  workflow_id: string; // bundle_id:flow_id
  bundle_id: string;
  flow_id: string;
  label: string;
  description?: string;
};

type RunSummary = {
  run_id: string;
  workflow_id?: string | null;
  status?: string;
  created_at?: string | null;
  updated_at?: string | null;
  ledger_len?: number | null;
  parent_run_id?: string | null;
};

function now_iso(): string {
  return new Date().toISOString();
}

function safe_json(v: any): string {
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}

function safe_json_inline(v: any, max_len: number): string {
  try {
    const s = JSON.stringify(v);
    if (typeof s !== "string") return String(v);
    if (s.length <= max_len) return s;
    return `${s.slice(0, Math.max(0, max_len - 1))}…`;
  } catch {
    const s = String(v);
    if (s.length <= max_len) return s;
    return `${s.slice(0, Math.max(0, max_len - 1))}…`;
  }
}

function parse_namespaced_workflow_id(workflow_id: string): { bundle_id: string; flow_id: string } | null {
  const s = String(workflow_id || "").trim();
  const idx = s.indexOf(":");
  if (idx <= 0 || idx >= s.length - 1) return null;
  return { bundle_id: s.slice(0, idx), flow_id: s.slice(idx + 1) };
}

function normalize_ui_event_name(name: string): string {
  const s = String(name || "").trim();
  if (s.startsWith("abstractcode.")) return `abstract.${s.slice("abstractcode.".length)}`;
  return s;
}

function is_ui_event_name(name: string): boolean {
  const s = String(name || "").trim();
  return s.startsWith("abstract.") || s.startsWith("abstractcode.");
}

function event_name_from_wait_key(wait_key: string): string {
  const wk = String(wait_key || "").trim();
  if (wk.startsWith("evt:")) {
    const parts = wk.split(":", 4);
    if (parts.length === 4 && parts[3]) return String(parts[3]).trim();
  }
  return wk;
}

function extract_start_pins_from_visualflow(raw: any): BundlePinDef[] {
  if (!raw || typeof raw !== "object") return [];
  const nodes = Array.isArray((raw as any).nodes) ? (raw as any).nodes : [];
  if (!nodes.length) return [];

  let start_node: any = null;
  for (const n of nodes) {
    if (!n || typeof n !== "object") continue;
    const data = n.data && typeof n.data === "object" ? n.data : {};
    const nt = String((data as any).nodeType || n.type || "").trim();
    if (nt === "on_flow_start") {
      start_node = n;
      break;
    }
  }
  if (!start_node) return [];

  const data = start_node.data && typeof start_node.data === "object" ? start_node.data : {};
  const outputs = Array.isArray((data as any).outputs) ? (data as any).outputs : [];
  const pin_defaults = data.pinDefaults && typeof data.pinDefaults === "object" ? data.pinDefaults : {};

  const out: BundlePinDef[] = [];
  for (const p of outputs) {
    if (!p || typeof p !== "object") continue;
    const pid = String((p as any).id || "").trim();
    if (!pid) continue;
    const ptype = String((p as any).type || "").trim();
    if (ptype === "execution" || pid === "exec-out" || pid === "exec") continue;
    const label = String((p as any).label || pid).trim() || pid;
    const item: BundlePinDef = { id: pid, label, type: ptype || "unknown" };
    if (pin_defaults && Object.prototype.hasOwnProperty.call(pin_defaults, pid)) {
      item.default = (pin_defaults as any)[pid];
    }
    out.push(item);
  }
  return out;
}

function clamp_preview(text: string, opts?: { max_chars?: number; max_lines?: number }): string {
  const max_chars = typeof opts?.max_chars === "number" ? opts.max_chars : 360;
  const max_lines = typeof opts?.max_lines === "number" ? opts.max_lines : 2;
  const raw = String(text || "");
  const lines = raw.replace(/\r\n/g, "\n").replace(/\r/g, "\n").split("\n");
  const head = lines.slice(0, Math.max(1, max_lines)).join("\n");
  const more_lines = lines.length > max_lines;
  const trimmed = head.length > max_chars ? `${head.slice(0, Math.max(0, max_chars - 1))}…` : head;
  if (more_lines && trimmed === head) return `${head}…`;
  return trimmed;
}

function short_id(id: string, keep: number): string {
  const s = String(id || "");
  if (s.length <= keep) return s;
  return `${s.slice(0, Math.max(0, keep - 1))}…`;
}

function extract_textish(payload: any): { text: string; duration: number } {
  if (typeof payload === "string") return { text: payload, duration: -1 };
  if (payload && typeof payload === "object") {
    const text =
      typeof (payload as any).text === "string"
        ? String((payload as any).text)
        : typeof (payload as any).value === "string"
          ? String((payload as any).value)
          : typeof (payload as any).message === "string"
            ? String((payload as any).message)
            : safe_json_inline(payload, 320);
    const duration = typeof (payload as any).duration === "number" ? Number((payload as any).duration) : -1;
    return { text, duration };
  }
  return { text: safe_json_inline(payload, 320), duration: -1 };
}

function load_settings(): Settings {
  try {
    const raw = localStorage.getItem("abstractcode_thin_client_settings");
    if (!raw) throw new Error("missing");
    const parsed = JSON.parse(raw);
    return {
      gateway_url: String(parsed?.gateway_url || ""),
      auth_token: String(parsed?.auth_token || ""),
      worker_url: String(parsed?.worker_url || ""),
      worker_token: String(parsed?.worker_token || ""),
    };
  } catch {
    return { gateway_url: "", auth_token: "", worker_url: "", worker_token: "" };
  }
}

function save_settings(s: Settings): void {
  localStorage.setItem("abstractcode_thin_client_settings", JSON.stringify(s));
}

function format_step_summary(rec: StepRecord): string {
  const node = String(rec?.node_id || "");
  const st = String(rec?.status || "");
  const eff = String(rec?.effect?.type || "");
  return `${node || "(node?)"} • ${st || "(status?)"} • ${eff || "(effect?)"}`;
}

function is_waiting_status(rec: StepRecord | null): boolean {
  return Boolean(rec && String(rec.status || "") === "waiting");
}

function parse_iso_ms(ts: any): number | null {
  const s = typeof ts === "string" ? ts.trim() : "";
  if (!s) return null;
  const ms = Date.parse(s);
  return Number.isFinite(ms) ? ms : null;
}

function fixed_col(text: string, width: number): string {
  const raw = String(text || "");
  if (raw.length === width) return raw;
  if (raw.length > width) {
    if (width <= 1) return "…";
    return `${raw.slice(0, Math.max(0, width - 1))}…`;
  }
  return raw + "\u00A0".repeat(width - raw.length);
}

function format_local_ts(ts: any): string {
  const ms = parse_iso_ms(ts);
  if (ms === null) return "—";
  const d = new Date(ms);
  const yyyy = String(d.getFullYear()).padStart(4, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  const hh = String(d.getHours()).padStart(2, "0");
  const mi = String(d.getMinutes()).padStart(2, "0");
  return `${yyyy}-${mm}-${dd} ${hh}:${mi}`;
}

export function App(): React.ReactElement {
  const [is_narrow, set_is_narrow] = useState<boolean>(() => {
    try {
      return typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(max-width: 900px)").matches;
    } catch {
      return false;
    }
  });
  const [mobile_tab, set_mobile_tab] = useState<"controls" | "viewer">("controls");

  const [settings, set_settings] = useState<Settings>(() => load_settings());
  const [run_id, set_run_id] = useState<string>("");
  const [root_run_id, set_root_run_id] = useState<string>("");
  const [flow_id, set_flow_id] = useState<string>("");
  const [bundle_id, set_bundle_id] = useState<string>("");
  const [input_data_text, set_input_data_text] = useState<string>("{}");

  const [bundle_info, set_bundle_info] = useState<BundleInfo | null>(null);
  const [bundle_loading, set_bundle_loading] = useState(false);
  const [bundle_error, set_bundle_error] = useState<string>("");
  const [input_field_drafts, set_input_field_drafts] = useState<Record<string, string>>({});
  const [input_field_errors, set_input_field_errors] = useState<Record<string, string>>({});

  const [discovery_loading, set_discovery_loading] = useState(false);
  const [discovery_error, set_discovery_error] = useState<string>("");
  const [gateway_connected, set_gateway_connected] = useState(false);
  const [workflow_options, set_workflow_options] = useState<WorkflowOption[]>([]);
  const [run_options, set_run_options] = useState<RunSummary[]>([]);
  const [runs_loading, set_runs_loading] = useState(false);
  const [discovered_tool_specs, set_discovered_tool_specs] = useState<any[]>([]);
  const [discovered_providers, set_discovered_providers] = useState<any[]>([]);
  const [discovered_models_by_provider, set_discovered_models_by_provider] = useState<Record<string, { models: string[]; error?: string }>>({});

  const [connected, set_connected] = useState(false);
  const [connecting, set_connecting] = useState(false);
  const [resuming, set_resuming] = useState(false);
  const [cursor, set_cursor] = useState<number>(0);
  const [records, set_records] = useState<Array<{ cursor: number; record: StepRecord }>>([]);
  const [child_records_for_digest, set_child_records_for_digest] = useState<Array<{ run_id: string; cursor: number; record: StepRecord }>>([]);
  const cursor_ref = useRef<number>(0);
  const [run_state, set_run_state] = useState<any>(null);
  const [control_reason, set_control_reason] = useState<string>("");

  const [status_text, set_status_text] = useState<string>("");
  const status_timer_ref = useRef<number | null>(null);
  const status_pulse_timer_ref = useRef<number | null>(null);
  const [status_pulse, set_status_pulse] = useState(false);
  const dismiss_timer_ref = useRef<number | null>(null);
  const [dismissed_wait_key, set_dismissed_wait_key] = useState<string>("");

  const [log, set_log] = useState<UiLogItem[]>([]);
  const [log_open, set_log_open] = useState<Record<string, boolean>>({});
  const [error_text, set_error_text] = useState<string>("");

  const [right_tab, set_right_tab] = useState<"ledger" | "graph" | "digest">("ledger");
  const [graph_flow_id, set_graph_flow_id] = useState<string>("");
  const [graph_flow, set_graph_flow] = useState<any | null>(null);
  const [graph_flow_cache, set_graph_flow_cache] = useState<Record<string, any>>({});
  const [graph_loading, set_graph_loading] = useState(false);
  const [graph_error, set_graph_error] = useState<string>("");
  const [graph_show_subflows, set_graph_show_subflows] = useState(false);
  const [graph_highlight_path, set_graph_highlight_path] = useState(false);
  const [graph_now_ms, set_graph_now_ms] = useState<number>(() => Date.now());
  const [active_node_id, set_active_node_id] = useState<string>("");
  const [recent_nodes, set_recent_nodes] = useState<Record<string, number>>({});
  const [visited_nodes, set_visited_nodes] = useState<Record<string, number>>({});
  const visited_order_ref = useRef<string[]>([]);
  const recent_prune_timer_ref = useRef<number | null>(null);
  const active_node_ref = useRef<string>("");
  const run_prefix_ref = useRef<Record<string, string>>({});
  const subrun_parent_ref = useRef<Record<string, string>>({});
  const root_subrun_ref = useRef<string>("");
  const models_fetch_inflight_ref = useRef<Record<string, boolean>>({});

  const abort_ref = useRef<AbortController | null>(null);
  const child_abort_ref = useRef<AbortController | null>(null);
  const child_cursor_ref = useRef<number>(0);
  const digest_seen_ref = useRef<Set<string>>(new Set());
  const [following_child_run_id, set_following_child_run_id] = useState<string>("");
  const [follow_run_id, set_follow_run_id] = useState<string>("");
  const follow_run_ref = useRef<string>("");

  const gateway = useMemo(() => new GatewayClient({ base_url: settings.gateway_url, auth_token: settings.auth_token }), [settings]);
  const worker = useMemo(
    () => (settings.worker_url.trim() ? new McpWorkerClient({ url: settings.worker_url.trim(), auth_token: settings.worker_token }) : null),
    [settings.worker_url, settings.worker_token]
  );

  const last_record = records.length ? records[records.length - 1].record : null;
  const wait_state: WaitState | null = useMemo(() => extract_wait_from_record(last_record), [last_record]);

  const selected_entrypoint: BundleEntrypoint | null = useMemo(() => {
    const bid = bundle_id.trim();
    if (!bundle_info || !bid) return null;
    if (String(bundle_info.bundle_id || "").trim() && String(bundle_info.bundle_id || "").trim() !== bid) return null;
    const eps = Array.isArray(bundle_info.entrypoints) ? bundle_info.entrypoints : [];
    if (!eps.length) return null;
    const fid = flow_id.trim();
    if (fid) return eps.find((e) => String(e.flow_id || "").trim() === fid) || null;
    if (eps.length === 1) return eps[0];
    const de = String(bundle_info.default_entrypoint || "").trim();
    if (de) return eps.find((e) => String(e.flow_id || "").trim() === de) || null;
    return null;
  }, [bundle_info, bundle_id, flow_id]);

  const entrypoint_pins: BundlePinDef[] = useMemo(() => {
    const pins = selected_entrypoint?.inputs;
    return Array.isArray(pins) ? (pins as BundlePinDef[]) : [];
  }, [selected_entrypoint]);

  const flow_pins: BundlePinDef[] = useMemo(() => extract_start_pins_from_visualflow(graph_flow), [graph_flow]);
  const adaptive_pins: BundlePinDef[] = useMemo(() => (entrypoint_pins.length ? entrypoint_pins : flow_pins), [entrypoint_pins, flow_pins]);

  const node_index_for_run: Record<string, any> = useMemo(() => {
    const wid = typeof run_state?.workflow_id === "string" ? String(run_state.workflow_id) : "";
    const parsed = parse_namespaced_workflow_id(wid);
    if (parsed && bundle_info && String(bundle_info.bundle_id || "").trim() === parsed.bundle_id) {
      const eps = Array.isArray(bundle_info.entrypoints) ? bundle_info.entrypoints : [];
      const ep = eps.find((e) => String(e?.flow_id || "").trim() === parsed.flow_id);
      if (ep && ep.node_index && typeof ep.node_index === "object") return ep.node_index as any;
    }
    const idx = selected_entrypoint?.node_index;
    if (idx && typeof idx === "object") return idx as any;
    return {};
  }, [run_state, bundle_info, selected_entrypoint]);

  useEffect(() => {
    save_settings(settings);
  }, [settings]);

  useEffect(() => {
    return () => {
      if (abort_ref.current) abort_ref.current.abort();
      if (child_abort_ref.current) child_abort_ref.current.abort();
      if (status_timer_ref.current) window.clearTimeout(status_timer_ref.current);
      if (status_pulse_timer_ref.current) window.clearTimeout(status_pulse_timer_ref.current);
      if (recent_prune_timer_ref.current) window.clearTimeout(recent_prune_timer_ref.current);
      if (dismiss_timer_ref.current) window.clearTimeout(dismiss_timer_ref.current);
    };
  }, []);

  useEffect(() => {
    try {
      if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
      const media = window.matchMedia("(max-width: 900px)");
      const on_change = (ev: MediaQueryListEvent) => set_is_narrow(Boolean(ev.matches));
      set_is_narrow(Boolean(media.matches));
      media.addEventListener("change", on_change);
      return () => media.removeEventListener("change", on_change);
    } catch {
      return;
    }
  }, []);

  const input_data_obj: Record<string, any> | null = useMemo(() => {
    const raw = input_data_text.trim();
    if (!raw) return {};
    try {
      const parsed = JSON.parse(raw);
      if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return null;
      return parsed;
    } catch {
      return null;
    }
  }, [input_data_text]);
  const request_value = typeof input_data_obj?.request === "string" ? String(input_data_obj.request) : "";
  const provider_value = typeof input_data_obj?.provider === "string" ? String(input_data_obj.provider) : "";
  const model_value = typeof input_data_obj?.model === "string" ? String(input_data_obj.model) : "";
  const has_adaptive_inputs = adaptive_pins.length > 0 && Boolean(bundle_id.trim());

  const selected_workflow_value = bundle_id.trim() && flow_id.trim() ? `${bundle_id.trim()}:${flow_id.trim()}` : "";

  const workflow_label_by_id = useMemo(() => {
    const out: Record<string, string> = {};
    for (const w of workflow_options) {
      const key = String(w.workflow_id || "").trim();
      const label = String(w.label || "").trim();
      if (key && label) out[key] = label;
    }
    return out;
  }, [workflow_options]);

  const available_tool_names = useMemo(() => {
    const out = new Set<string>();
    for (const s of Array.isArray(discovered_tool_specs) ? discovered_tool_specs : []) {
      const name = String((s as any)?.name || "").trim();
      if (name) out.add(name);
    }
    return Array.from(out).sort();
  }, [discovered_tool_specs]);

  const available_providers = useMemo(() => {
    const out = new Set<string>();
    for (const p of Array.isArray(discovered_providers) ? discovered_providers : []) {
      const name = String((p as any)?.name || "").trim();
      if (name) out.add(name);
    }
    return Array.from(out).sort();
  }, [discovered_providers]);

  const models_for_provider = useMemo(() => {
    const prov = provider_value.trim();
    if (!prov) return { models: [] as string[], error: "" };
    const found = discovered_models_by_provider[prov];
    if (!found) return { models: [] as string[], error: "" };
    const models = Array.isArray(found.models) ? found.models : [];
    return { models: models.map((x) => String(x || "").trim()).filter(Boolean), error: String((found as any).error || "") };
  }, [discovered_models_by_provider, provider_value]);

  useEffect(() => {
    const prov = provider_value.trim();
    if (!prov) return;
    if (discovered_models_by_provider[prov]) return;
    if (models_fetch_inflight_ref.current[prov]) return;
    models_fetch_inflight_ref.current[prov] = true;
    let stopped = false;
    const run = async () => {
      try {
        const res = await gateway.discovery_provider_models(prov);
        if (stopped) return;
        const models = Array.isArray(res?.models) ? res.models : [];
        const err = typeof res?.error === "string" ? String(res.error) : "";
        set_discovered_models_by_provider((prev) => ({ ...prev, [prov]: { models, error: err || undefined } }));
      } catch (e: any) {
        if (stopped) return;
        set_discovered_models_by_provider((prev) => ({ ...prev, [prov]: { models: [], error: String(e?.message || e || "Failed to load models") } }));
      } finally {
        delete models_fetch_inflight_ref.current[prov];
      }
    };
    run();
    return () => {
      stopped = true;
    };
  }, [provider_value, discovered_models_by_provider, gateway]);

  function update_input_data_field(key: string, value: string): void {
    let obj: Record<string, any> = {};
    try {
      const parsed = JSON.parse(input_data_text || "{}");
      if (typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)) obj = parsed;
    } catch {
      obj = {};
    }

    const trimmed = String(value || "").trim();
    if (!trimmed) delete obj[key];
    else obj[key] = trimmed;

    set_input_data_text(JSON.stringify(obj, null, 2));
  }

  function set_input_data_value(key: string, value: any): void {
    if (input_data_obj === null) {
      set_error_text("Invalid input_data JSON (fix it in Advanced JSON).");
      return;
    }
    const obj: Record<string, any> = { ...(input_data_obj || {}) };
    if (value === undefined) delete obj[key];
    else obj[key] = value;
    set_input_data_text(JSON.stringify(obj, null, 2));
  }

  function delete_input_data_key(key: string): void {
    if (input_data_obj === null) {
      set_error_text("Invalid input_data JSON (fix it in Advanced JSON).");
      return;
    }
    const obj: Record<string, any> = { ...(input_data_obj || {}) };
    delete obj[key];
    set_input_data_text(JSON.stringify(obj, null, 2));
  }

  async function load_bundle_info(bid_raw: string): Promise<BundleInfo | null> {
    const bid = String(bid_raw || "").trim();
    if (!bid) {
      set_bundle_error("Missing bundle_id");
      return null;
    }
    set_bundle_error("");
    set_bundle_loading(true);
    try {
      const info = (await gateway.get_bundle(bid)) as BundleInfo;
      set_bundle_info(info);
      set_input_field_drafts({});
      set_input_field_errors({});
      push_log({ ts: now_iso(), kind: "info", title: `Loaded bundle ${bid}` });
      return info;
    } catch (e: any) {
      set_bundle_info(null);
      set_bundle_error(String(e?.message || e || "Failed to load bundle"));
      return null;
    } finally {
      set_bundle_loading(false);
    }
  }

  async function on_load_bundle(): Promise<void> {
    await load_bundle_info(bundle_id.trim());
  }

  function build_workflow_options_from_bundles(resp: any): WorkflowOption[] {
    const out: WorkflowOption[] = [];
    const items = Array.isArray(resp?.items) ? resp.items : [];
    for (const b of items) {
      const bid = String(b?.bundle_id || "").trim();
      if (!bid) continue;
      const eps = Array.isArray(b?.entrypoints) ? b.entrypoints : [];
      if (!eps.length) continue;
      for (const ep of eps) {
        const fid = String(ep?.flow_id || "").trim();
        if (!fid) continue;
        const workflow_id = `${bid}:${fid}`;
        const name = String(ep?.name || "").trim();
        const label = name ? `${bid} · ${name}` : `${bid} · ${fid}`;
        const description = String(ep?.description || "").trim();
        out.push({ workflow_id, bundle_id: bid, flow_id: fid, label, description: description || undefined });
      }
    }
    out.sort((a, b) => a.label.localeCompare(b.label));
    return out;
  }

  async function refresh_runs(): Promise<void> {
    if (runs_loading) return;
    set_runs_loading(true);
    try {
      const runs = await gateway.list_runs({ limit: 80 });
      const items = Array.isArray((runs as any)?.items) ? ((runs as any).items as any[]) : [];
      const next: RunSummary[] = items
        .map((r) => ({
          run_id: String(r?.run_id || "").trim(),
          workflow_id: typeof r?.workflow_id === "string" ? String(r.workflow_id) : r?.workflow_id ?? null,
          status: typeof r?.status === "string" ? String(r.status) : "",
          created_at: typeof r?.created_at === "string" ? String(r.created_at) : r?.created_at ?? null,
          updated_at: typeof r?.updated_at === "string" ? String(r.updated_at) : r?.updated_at ?? null,
          ledger_len: typeof r?.ledger_len === "number" ? Number(r.ledger_len) : r?.ledger_len ?? null,
          parent_run_id: typeof r?.parent_run_id === "string" ? String(r.parent_run_id) : r?.parent_run_id ?? null,
        }))
        .filter((r) => Boolean(r.run_id));
      set_run_options(next);
    } catch (e: any) {
      push_log({ ts: now_iso(), kind: "error", title: "Refresh runs failed", preview: clamp_preview(String(e?.message || e || "")) });
    } finally {
      set_runs_loading(false);
    }
  }

  async function on_discover_gateway(): Promise<void> {
    set_discovery_error("");
    set_gateway_connected(false);
    set_discovery_loading(true);
    try {
      const bundles = await gateway.list_bundles();
      const opts = build_workflow_options_from_bundles(bundles);
      set_workflow_options(opts);

      try {
        await refresh_runs();
      } catch (e: any) {
        set_run_options([]);
        push_log({ ts: now_iso(), kind: "error", title: "Discovery runs failed", preview: clamp_preview(String(e?.message || e || "")) });
      }

      const [tools_res, providers_res] = await Promise.allSettled([gateway.discovery_tools(), gateway.discovery_providers({ include_models: false })]);
      if (tools_res.status === "fulfilled") {
        const items = Array.isArray(tools_res.value?.items) ? tools_res.value.items : [];
        set_discovered_tool_specs(items);
      } else {
        set_discovered_tool_specs([]);
        push_log({ ts: now_iso(), kind: "error", title: "Discovery tools failed", preview: clamp_preview(String(tools_res.reason || "")) });
      }

      if (providers_res.status === "fulfilled") {
        const items = Array.isArray(providers_res.value?.items) ? providers_res.value.items : [];
        set_discovered_providers(items);
      } else {
        set_discovered_providers([]);
        push_log({ ts: now_iso(), kind: "error", title: "Discovery providers failed", preview: clamp_preview(String(providers_res.reason || "")) });
      }

      set_discovered_models_by_provider({});
      set_gateway_connected(true);
      push_log({ ts: now_iso(), kind: "info", title: "Gateway discovery loaded", preview: clamp_preview(`workflows: ${opts.length}`) });
    } catch (e: any) {
      set_discovery_error(String(e?.message || e || "Discovery failed"));
    } finally {
      set_discovery_loading(false);
    }
  }

  function apply_entrypoint_defaults(): void {
    const pins = adaptive_pins;
    if (!pins.length) return;

    let obj: Record<string, any> = {};
    try {
      const parsed = JSON.parse(input_data_text || "{}");
      if (typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)) obj = parsed;
    } catch {
      obj = {};
    }

    let changed = false;
    for (const p of pins) {
      if (!p || typeof p !== "object") continue;
      const k = String(p.id || "").trim();
      if (!k) continue;
      if (obj[k] !== undefined) continue;
      if (p.default === undefined) continue;
      obj[k] = p.default;
      changed = true;
    }

    if (changed) {
      set_input_data_text(JSON.stringify(obj, null, 2));
      set_input_field_drafts({});
      set_input_field_errors({});
    }
  }

  async function copy_to_clipboard(text: string): Promise<void> {
    const payload = String(text ?? "");
    if (!payload) return;
    try {
      await navigator.clipboard.writeText(payload);
      set_status("Copied to clipboard", 2);
      return;
    } catch {
      // fall back
    }
    try {
      const ta = document.createElement("textarea");
      ta.value = payload;
      ta.style.position = "fixed";
      ta.style.left = "-9999px";
      ta.style.top = "0";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.focus();
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
      set_status("Copied to clipboard", 2);
    } catch {
      set_status("Copy failed", 2);
    }
  }

  // Best-effort run state polling (pause/cancel are run-level changes that are not currently ledgered).
  useEffect(() => {
    const rid = run_id.trim();
    if (!connected || !rid) return;

    let stopped = false;
    const poll = async () => {
      try {
        const st = await gateway.get_run(rid);
        if (!stopped) set_run_state(st);
      } catch {
        // ignore
      }
    };

    poll();
    const timer = window.setInterval(poll, 2000);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [connected, run_id, gateway]);

  function push_log(item: Omit<UiLogItem, "id"> & { id?: string }): void {
    const id = String(item.id || "").trim() || random_id();
    set_log((prev) => [{ ...(item as any), id } as UiLogItem, ...prev].slice(0, 200));
  }

  function set_status(text: string, duration_s: number): void {
    set_status_text(text);

    if (status_pulse_timer_ref.current) window.clearTimeout(status_pulse_timer_ref.current);
    set_status_pulse(false);
    window.requestAnimationFrame(() => set_status_pulse(true));
    status_pulse_timer_ref.current = window.setTimeout(() => {
      set_status_pulse(false);
      status_pulse_timer_ref.current = null;
    }, 1500);

    if (status_timer_ref.current) {
      window.clearTimeout(status_timer_ref.current);
      status_timer_ref.current = null;
    }
    if (duration_s > 0) {
      status_timer_ref.current = window.setTimeout(() => {
        set_status_text("");
        status_timer_ref.current = null;
      }, Math.max(1, duration_s) * 1000);
    }
  }

  function graph_node_id_for(run_id_value: string, node_id_value: string): string {
    const rid = String(run_id_value || "").trim();
    const nid = String(node_id_value || "").trim();
    if (!nid) return "";
    const prefix = rid ? String(run_prefix_ref.current[rid] || "").trim() : "";
    return prefix ? `${prefix}::${nid}` : nid;
  }

  function register_subworkflow_child_run(parent_run_id_value: string, parent_node_id_value: string, sub_run_id_value: string): void {
    const parent_run_id = String(parent_run_id_value || "").trim();
    const parent_node_id = String(parent_node_id_value || "").trim();
    const sub_run_id = String(sub_run_id_value || "").trim();
    if (!parent_run_id || !parent_node_id || !sub_run_id) return;
    const prefix = graph_node_id_for(parent_run_id, parent_node_id);
    if (!prefix) return;
    run_prefix_ref.current[sub_run_id] = prefix;
    subrun_parent_ref.current[sub_run_id] = parent_run_id;
  }

  function mark_node_activity(node_id_for_graph: string): void {
    const node_id = String(node_id_for_graph || "").trim();
    if (!node_id) return;
    const now = Date.now();
    const prev_active = active_node_ref.current;
    active_node_ref.current = node_id;
    set_active_node_id(node_id);
    set_graph_now_ms(now);

    set_recent_nodes((prev) => {
      const next: Record<string, number> = {};
      for (const [k, until] of Object.entries(prev)) {
        if (typeof until === "number" && until > now) next[k] = until;
      }
      next[node_id] = Math.max(next[node_id] || 0, now + 2000);
      if (prev_active && prev_active !== node_id) next[prev_active] = Math.max(next[prev_active] || 0, now + 2000);

      // Cap to avoid unbounded growth.
      const keys = Object.keys(next);
      if (keys.length <= 200) return next;
      const keep = keys.sort((a, b) => (next[b] || 0) - (next[a] || 0)).slice(0, 200);
      const pruned: Record<string, number> = {};
      for (const k of keep) pruned[k] = next[k] || 0;
      return pruned;
    });

    set_visited_nodes((prev) => {
      if (typeof prev[node_id] === "number") return prev;
      const next = { ...prev, [node_id]: now };
      visited_order_ref.current.push(node_id);
      if (visited_order_ref.current.length > 8000) {
        const drop = visited_order_ref.current.splice(0, 1500);
        for (const k of drop) delete next[k];
      }
      return next;
    });

    if (recent_prune_timer_ref.current) window.clearTimeout(recent_prune_timer_ref.current);
    recent_prune_timer_ref.current = window.setTimeout(() => {
      const t = Date.now();
      set_recent_nodes((prev) => {
        const next: Record<string, number> = {};
        for (const [k, until] of Object.entries(prev)) {
          if (typeof until === "number" && until > t) next[k] = until;
        }
        return next;
      });
      set_graph_now_ms(t);
      recent_prune_timer_ref.current = null;
    }, 2200);
  }

  function handle_step(ev: LedgerStreamEvent): void {
    cursor_ref.current = ev.cursor;
    set_cursor(ev.cursor);
    set_records((prev) => [...prev, { cursor: ev.cursor, record: ev.record }]);
    if (run_id.trim()) digest_seen_ref.current.add(`${run_id.trim()}:${ev.cursor}`);

    const emit = extract_emit_event(ev.record);
    const emit_name = emit && emit.name ? normalize_ui_event_name(emit.name) : "";
    if (emit_name === "abstract.status") {
      const { text, duration } = extract_textish(emit?.payload);
      set_status(text, duration);
    }

    const rec = ev.record;
    const node_id = typeof rec?.node_id === "string" ? rec.node_id : "";
    const status = typeof rec?.status === "string" ? rec.status : "";
    const effect_type = typeof rec?.effect?.type === "string" ? rec.effect.type : "";
    const rec_run_id = typeof rec?.run_id === "string" ? rec.run_id : "";

    const effective_run_id = rec_run_id || run_id.trim();
    if (status === "waiting") {
      const w = extract_wait_from_record(rec);
      const reason = String(w?.reason || "").trim();
      if (reason === "subworkflow") {
        const sub = typeof (w as any)?.details?.sub_run_id === "string" ? String((w as any).details.sub_run_id) : "";
        if (sub && node_id) register_subworkflow_child_run(effective_run_id, node_id, sub);
      }
    }

    if (node_id) mark_node_activity(graph_node_id_for(effective_run_id, node_id));

    let kind: UiLogItem["kind"] = "step";
    let title = node_id || "(node?)";
    let preview = "";

    if (emit && emit.name && is_ui_event_name(emit.name)) {
      kind = emit_name === "abstract.message" ? "message" : "event";
      title = emit_name || emit.name;
      preview = clamp_preview(extract_textish(emit?.payload).text);
    } else if (rec?.error) {
      kind = "error";
      title = "error";
      preview = clamp_preview(safe_json_inline(rec.error, 360));
    } else if (status === "waiting") {
      const w = extract_wait_from_record(rec);
      const reason = String(w?.reason || "").trim();
      preview = clamp_preview(reason ? `waiting • ${reason}` : "waiting");
    } else if (rec?.result) {
      preview = clamp_preview(safe_json_inline(rec.result, 360));
    } else if (effect_type) {
      preview = clamp_preview(effect_type);
    } else {
      preview = clamp_preview(format_step_summary(rec));
    }

    push_log({
      id: `step:${rec_run_id || run_id.trim() || "?"}:${ev.cursor}`,
      ts: String(rec?.ended_at || rec?.started_at || now_iso()),
      kind,
      title,
      preview,
      data: rec,
      cursor: ev.cursor,
      run_id: rec_run_id || run_id.trim() || undefined,
      node_id,
      status,
      effect_type,
      emit_name: emit_name || emit?.name || undefined,
    });
  }

  function handle_child_step(child_run_id: string, ev: LedgerStreamEvent): void {
    child_cursor_ref.current = Math.max(child_cursor_ref.current, ev.cursor);
    const dig_key = `${child_run_id}:${ev.cursor}`;
    if (!digest_seen_ref.current.has(dig_key)) {
      digest_seen_ref.current.add(dig_key);
      set_child_records_for_digest((prev) => [...prev, { run_id: child_run_id, cursor: ev.cursor, record: ev.record }].slice(-5000));
    }
    const emit = extract_emit_event(ev.record);
    const emit_name = emit && emit.name ? normalize_ui_event_name(emit.name) : "";
    const rec = ev.record;
    const node_id = typeof rec?.node_id === "string" ? rec.node_id : "";
    const status = typeof rec?.status === "string" ? rec.status : "";
    if (status === "waiting") {
      const w = extract_wait_from_record(rec);
      const reason = String(w?.reason || "").trim();
      if (reason === "subworkflow") {
        const sub = typeof (w as any)?.details?.sub_run_id === "string" ? String((w as any).details.sub_run_id) : "";
        if (sub && node_id) register_subworkflow_child_run(child_run_id, node_id, sub);
        // Descend into nested subflows so status/events aren't missed when emitted in grandchildren.
        if (sub && follow_run_ref.current.trim() === String(child_run_id || "").trim()) {
          follow_run_ref.current = sub;
          set_follow_run_id(sub);
        }
      }
    }
    if (node_id) mark_node_activity(graph_node_id_for(child_run_id, node_id));

    // If the currently-followed run completes, fall back to its parent (if any).
    if ((status === "completed" || status === "failed") && follow_run_ref.current.trim() === String(child_run_id || "").trim()) {
      const parent = String(subrun_parent_ref.current[String(child_run_id || "").trim()] || "").trim();
      if (parent && parent !== run_id.trim()) {
        follow_run_ref.current = parent;
        set_follow_run_id(parent);
      } else {
        follow_run_ref.current = "";
        set_follow_run_id("");
      }
    }

    if (!emit || !emit.name) return;

    if (emit_name === "abstract.status") {
      const { text, duration } = extract_textish(emit?.payload);
      set_status(text, duration);
      return;
    }
    if (!is_ui_event_name(emit.name)) return;
    const effect_type = typeof rec?.effect?.type === "string" ? rec.effect.type : "";

    const kind: UiLogItem["kind"] = emit_name === "abstract.message" ? "message" : "event";
    const title = `child • ${emit_name || emit.name}`;
    const preview = clamp_preview(extract_textish(emit?.payload).text);

    push_log({
      id: `child:${child_run_id}:${ev.cursor}`,
      ts: String(rec?.ended_at || rec?.started_at || now_iso()),
      kind,
      title,
      preview,
      data: rec,
      cursor: ev.cursor,
      run_id: child_run_id,
      node_id,
      status,
      effect_type,
      emit_name: emit_name || emit.name,
    });
  }

  async function replay_ledger(run_id_value: string, opts: { after: number }): Promise<number> {
    let after = opts.after;
    while (true) {
      const page = await gateway.get_ledger(run_id_value, { after, limit: 200 });
      const items = Array.isArray(page.items) ? page.items : [];
      if (!items.length) {
        set_cursor(after);
        cursor_ref.current = after;
        return after;
      }
      const base = after;
      for (let i = 0; i < items.length; i++) {
        const record = items[i] as StepRecord;
        handle_step({ cursor: base + i + 1, record });
      }
      after = typeof page.next_after === "number" ? page.next_after : after;
    }
  }

  async function connect_to_run(run_id_value: string): Promise<void> {
    const rid = String(run_id_value || "").trim();
    set_error_text("");
    set_connecting(true);
    set_connected(false);
    set_records([]);
    set_cursor(0);
    cursor_ref.current = 0;
    set_child_records_for_digest([]);
    digest_seen_ref.current = new Set();
    set_log([]);
    set_log_open({});
    set_status_text("");
    set_run_state(null);
    set_input_field_drafts({});
    set_input_field_errors({});
    set_dismissed_wait_key("");
    set_active_node_id("");
    active_node_ref.current = "";
    set_recent_nodes({});
    set_visited_nodes({});
    visited_order_ref.current = [];
    set_graph_now_ms(Date.now());
    run_prefix_ref.current = rid ? { [rid]: "" } : {};
    subrun_parent_ref.current = {};
    root_subrun_ref.current = "";
    follow_run_ref.current = "";
    set_follow_run_id("");
    if (recent_prune_timer_ref.current) window.clearTimeout(recent_prune_timer_ref.current);
    recent_prune_timer_ref.current = null;
    if (dismiss_timer_ref.current) window.clearTimeout(dismiss_timer_ref.current);
    dismiss_timer_ref.current = null;

    if (abort_ref.current) abort_ref.current.abort();
    if (child_abort_ref.current) child_abort_ref.current.abort();
    child_abort_ref.current = null;
    child_cursor_ref.current = 0;
    set_following_child_run_id("");
    const abort = new AbortController();
    abort_ref.current = abort;

    try {
      // Best-effort attach context (bundle/flow + input_data) to make Attach match Start.
      let inferred_bundle_id = "";
      let inferred_flow_id = "";
      try {
        const st = await gateway.get_run(rid);
        set_run_state(st);
        const wid = typeof st?.workflow_id === "string" ? String(st.workflow_id) : "";
        const parsed = parse_namespaced_workflow_id(wid);
        if (parsed) {
          inferred_bundle_id = parsed.bundle_id;
          inferred_flow_id = parsed.flow_id;
        }
      } catch {
        // ignore
      }

      try {
        const inp = await gateway.get_run_input_data(rid);
        if (typeof (inp as any)?.bundle_id === "string") inferred_bundle_id = String((inp as any).bundle_id || "").trim() || inferred_bundle_id;
        if (typeof (inp as any)?.flow_id === "string") inferred_flow_id = String((inp as any).flow_id || "").trim() || inferred_flow_id;

        const data = inp && typeof inp.input_data === "object" && inp.input_data && !Array.isArray(inp.input_data) ? inp.input_data : null;
        if (data) {
          set_input_data_text(JSON.stringify(data, null, 2));
          set_input_field_drafts({});
          set_input_field_errors({});
        }
      } catch {
        // ignore
      }

      if (inferred_bundle_id && inferred_flow_id) {
        set_bundle_id(inferred_bundle_id);
        set_flow_id(inferred_flow_id);
        set_graph_flow_id(inferred_flow_id);
        await load_bundle_info(inferred_bundle_id);
      }

      let after = await replay_ledger(rid, { after: 0 });
      set_connected(true);
      push_log({ ts: now_iso(), kind: "info", title: `Attached to run ${rid}`, data: { run_id: rid } });

      let backoff_ms = 250;
      while (!abort.signal.aborted) {
        // Best-effort resync before streaming (replay-first).
        after = await replay_ledger(rid, { after: cursor_ref.current });
        try {
          await gateway.stream_ledger(rid, {
            after,
            on_step: handle_step,
            signal: abort.signal,
          });
        } catch (e: any) {
          if (abort.signal.aborted) break;
          const msg = String(e?.message || e || "stream error");
          push_log({ ts: now_iso(), kind: "error", title: "Ledger stream error (will retry)", preview: clamp_preview(msg), data: { error: msg } });
        }

        if (abort.signal.aborted) break;
        await new Promise((r) => setTimeout(r, backoff_ms));
        backoff_ms = Math.min(5000, Math.floor(backoff_ms * 1.6));
      }
    } catch (e: any) {
      const msg = String(e?.message || e || "unknown error");
      set_error_text(msg);
      push_log({ ts: now_iso(), kind: "error", title: "Connection error", preview: clamp_preview(msg), data: { error: msg } });
      set_connected(false);
    } finally {
      set_connecting(false);
    }
  }

  async function on_start_run(): Promise<void> {
    const fid = flow_id.trim();
    const bid = bundle_id.trim();
    if (!fid && !bid) {
      set_error_text("Missing flow_id (or bundle_id for bundle entrypoint)");
      return;
    }
    if (bid && !fid && bundle_info && String(bundle_info.bundle_id || "").trim() === bid) {
      const eps = Array.isArray(bundle_info.entrypoints) ? bundle_info.entrypoints : [];
      const de = String(bundle_info.default_entrypoint || "").trim();
      if (eps.length > 1 && !de) {
        set_error_text(`Bundle '${bid}' has multiple entrypoints; set flow_id to select which one to start (or set a bundle default entrypoint).`);
        return;
      }
    }
    set_error_text("");
    set_connecting(true);
    try {
      let input_data: Record<string, any> = {};
      const raw = input_data_text.trim();
      if (raw) {
        try {
          input_data = JSON.parse(raw);
          if (typeof input_data !== "object" || input_data === null || Array.isArray(input_data)) {
            throw new Error("input_data must be a JSON object");
          }
        } catch (e: any) {
          throw new Error(`Invalid input_data JSON: ${String(e?.message || e)}`);
        }
      }
      const rid = await gateway.start_run(fid || undefined, input_data, { bundle_id: bid || undefined });
      set_root_run_id(rid);
      set_run_id(rid);
      await connect_to_run(rid);
    } catch (e: any) {
      set_error_text(String(e?.message || e || "start failed"));
    } finally {
      set_connecting(false);
    }
  }

  async function attach_to_run(rid: string): Promise<void> {
    const run = String(rid || "").trim();
    if (!run) return;
    set_error_text("");
    set_root_run_id(run);
    set_run_id(run);
    await connect_to_run(run);
  }

  function clear_run_view(): void {
    if (abort_ref.current) abort_ref.current.abort();
    abort_ref.current = null;
    if (child_abort_ref.current) child_abort_ref.current.abort();
    child_abort_ref.current = null;
    child_cursor_ref.current = 0;
    set_following_child_run_id("");

    set_run_id("");
    set_root_run_id("");
    run_prefix_ref.current = {};
    subrun_parent_ref.current = {};
    root_subrun_ref.current = "";
    follow_run_ref.current = "";
    set_follow_run_id("");
    set_dismissed_wait_key("");
    if (recent_prune_timer_ref.current) window.clearTimeout(recent_prune_timer_ref.current);
    recent_prune_timer_ref.current = null;
    if (dismiss_timer_ref.current) window.clearTimeout(dismiss_timer_ref.current);
    dismiss_timer_ref.current = null;

    set_connected(false);
    set_connecting(false);
    set_resuming(false);

    cursor_ref.current = 0;
    set_cursor(0);
    set_records([]);
    set_child_records_for_digest([]);
    digest_seen_ref.current = new Set();
    set_run_state(null);
    set_log([]);
    set_log_open({});
    set_active_node_id("");
    active_node_ref.current = "";
    set_recent_nodes({});
    set_visited_nodes({});
    visited_order_ref.current = [];
    set_graph_now_ms(Date.now());
    set_status("", -1);
    set_error_text("");
  }

  async function submit_run_control(type: "pause" | "resume" | "cancel"): Promise<void> {
    const rid = run_id.trim();
    if (!rid) {
      set_error_text("Missing run_id");
      return;
    }
    set_error_text("");
    try {
      const payload: any = {};
      const reason = control_reason.trim();
      if (reason) payload.reason = reason;
      await gateway.submit_command({
        command_id: random_id(),
        run_id: rid,
        type,
        payload,
        client_id: "web_pwa",
      });
      push_log({ ts: now_iso(), kind: "info", title: `${type} submitted`, preview: reason ? `reason: ${reason}` : "", data: { type, reason } });
      // Refresh run state quickly.
      try {
        const st = await gateway.get_run(rid);
        set_run_state(st);
      } catch {
        // ignore
      }
    } catch (e: any) {
      set_error_text(String(e?.message || e || `${type} failed`));
    }
  }

  async function resume_wait(payload_obj: any): Promise<void> {
    const rid = run_id.trim();
    const wk = String(wait_state?.wait_key || "").trim();
    if (!rid || !wk) {
      set_error_text("No active wait to resume");
      return;
    }
    if (String(wait_state?.reason || "").trim() === "subworkflow") {
      set_error_text("This run is waiting on a subworkflow; attach to the child run instead of resuming manually.");
      return;
    }
    set_error_text("");
    set_resuming(true);
    try {
      await gateway.submit_command({
        command_id: random_id(),
        run_id: rid,
        type: "resume",
        payload: { wait_key: wk, payload: payload_obj || {} },
        client_id: "web_pwa",
      });
      push_log({ ts: now_iso(), kind: "info", title: "Resume submitted", preview: clamp_preview(`wait_key: ${wk}`), data: { wait_key: wk, payload: payload_obj || {} } });
      set_dismissed_wait_key(wk);
      if (dismiss_timer_ref.current) window.clearTimeout(dismiss_timer_ref.current);
      dismiss_timer_ref.current = window.setTimeout(() => {
        set_dismissed_wait_key((prev) => (prev === wk ? "" : prev));
      }, 2000);
    } catch (e: any) {
      set_error_text(String(e?.message || e || "resume failed"));
    } finally {
      set_resuming(false);
    }
  }

  async function execute_tools_via_worker(tool_calls: ToolCall[]): Promise<void> {
    if (!worker) {
      set_error_text("No worker configured");
      return;
    }
    if (resuming) return;
    set_error_text("");
    set_resuming(true);

    try {
      const results: ToolResult[] = [];
      for (const tc of tool_calls) {
        // Sequential to keep UX predictable (and avoid flooding).
        // Future: bounded concurrency + cancellation.
        const res = await worker.call_tool(tc);
        results.push(res);
      }

      await resume_wait({ mode: "executed", results });
    } finally {
      set_resuming(false);
    }
  }

  const tool_calls_for_wait = useMemo(() => extract_tool_calls_from_wait(wait_state), [wait_state]);
  const wait_key = String(wait_state?.wait_key || "").trim();
  const wait_reason = String(wait_state?.reason || "").trim();
  const is_waiting = is_waiting_status(last_record) && Boolean(wait_key);
  const is_user_wait = wait_reason === "user";
  const wait_event_name = wait_reason === "event" ? normalize_ui_event_name(event_name_from_wait_key(wait_key)) : "";
  const is_ask_event_wait = wait_reason === "event" && wait_event_name === "abstract.ask";
  const has_tool_wait = tool_calls_for_wait.length > 0;
  const show_wait_modal = is_waiting && wait_key && (is_user_wait || is_ask_event_wait || has_tool_wait) && dismissed_wait_key !== wait_key;
  const sub_run_id = typeof (wait_state as any)?.details?.sub_run_id === "string" ? String((wait_state as any).details.sub_run_id) : "";

  const run_status = typeof run_state?.status === "string" ? String(run_state.status) : "";
  const run_paused = Boolean(run_state?.paused);
  const run_terminal = run_status === "completed" || run_status === "failed" || run_status === "cancelled";
  const pause_resume_label = run_status === "running" && !run_paused ? "Pause" : "Resume";
  const pause_resume_action: "pause" | "resume" = pause_resume_label === "Pause" ? "pause" : "resume";
  const pause_resume_disabled =
    !run_id.trim() || connecting || resuming || run_terminal || (pause_resume_label === "Resume" && !run_paused);

  const digest = useMemo(() => {
    const all: StepRecord[] = [];
    for (const x of records) {
      if (x && x.record) all.push(x.record);
    }
    for (const x of child_records_for_digest) {
      if (x && x.record) all.push(x.record);
    }

    const stats = {
      steps: all.length,
      tool_calls_effects: 0,
      tool_calls: 0,
      unique_tools: 0,
      llm_calls: 0,
      prompt_tokens: 0,
      completion_tokens: 0,
      total_tokens: 0,
      started_at: "",
      ended_at: "",
      duration_s: 0,
    };

    const files: Array<{ tool: string; file_path: string; run_id: string; ts: string }> = [];
    const commands: Array<{ command: string; run_id: string; ts: string }> = [];
    const web: Array<{ tool: string; value: string; run_id: string; ts: string }> = [];
    const tools_used = new Set<string>();

    let min_ms: number | null = null;
    let max_ms: number | null = null;

    for (const rec of all) {
      const rid = typeof rec?.run_id === "string" ? String(rec.run_id) : "";
      const ts_s = String(rec?.ended_at || rec?.started_at || "").trim();
      const ms = parse_iso_ms(ts_s);
      if (ms !== null) {
        if (min_ms === null || ms < min_ms) min_ms = ms;
        if (max_ms === null || ms > max_ms) max_ms = ms;
      }

      const eff_type = typeof rec?.effect?.type === "string" ? String(rec.effect.type) : "";
      if (eff_type === "llm_call") {
        stats.llm_calls += 1;
        const usage = rec?.result && typeof rec.result === "object" ? (rec.result as any).usage || (rec.result as any).token_usage : null;
        if (usage && typeof usage === "object") {
          const pt = Number((usage as any).prompt_tokens ?? (usage as any).input_tokens ?? 0);
          const ct = Number((usage as any).completion_tokens ?? (usage as any).output_tokens ?? 0);
          const tt = Number((usage as any).total_tokens ?? pt + ct);
          if (Number.isFinite(pt)) stats.prompt_tokens += pt;
          if (Number.isFinite(ct)) stats.completion_tokens += ct;
          if (Number.isFinite(tt)) stats.total_tokens += tt;
        }
      }

      if (eff_type !== "tool_calls") continue;
      stats.tool_calls_effects += 1;
      const payload = rec?.effect?.payload;
      const tool_calls = payload && typeof payload === "object" ? (payload as any).tool_calls : null;
      const calls = Array.isArray(tool_calls) ? (tool_calls as any[]) : [];
      if (!calls.length) continue;
      stats.tool_calls += calls.length;

      for (const c of calls) {
        if (!c || typeof c !== "object") continue;
        const name = String((c as any).name || "").trim();
        if (!name) continue;
        tools_used.add(name);
        const args = (c as any).arguments;

        if (name === "write_file" || name === "edit_file" || name === "delete_file") {
          const fp = args && typeof args === "object" ? String((args as any).file_path || (args as any).path || "").trim() : "";
          if (fp) files.push({ tool: name, file_path: fp, run_id: rid, ts: ts_s });
        } else if (name === "execute_command") {
          const cmd = args && typeof args === "object" ? String((args as any).command || "").trim() : "";
          if (cmd) commands.push({ command: cmd, run_id: rid, ts: ts_s });
        } else if (name === "web_search") {
          const q = args && typeof args === "object" ? String((args as any).query || "").trim() : "";
          if (q) web.push({ tool: name, value: q, run_id: rid, ts: ts_s });
        } else if (name === "fetch_url") {
          const u = args && typeof args === "object" ? String((args as any).url || "").trim() : "";
          if (u) web.push({ tool: name, value: u, run_id: rid, ts: ts_s });
        }
      }
    }

    stats.unique_tools = tools_used.size;
    if (min_ms !== null) stats.started_at = new Date(min_ms).toISOString();
    if (max_ms !== null) stats.ended_at = new Date(max_ms).toISOString();
    if (min_ms !== null && max_ms !== null) stats.duration_s = Math.max(0, Math.round((max_ms - min_ms) / 1000));

    return { stats, files, commands, web, tools_used: Array.from(tools_used).sort() };
  }, [records, child_records_for_digest]);

  // Follow the deepest active subworkflow run for status/event UX (not just the immediate child).
  useEffect(() => {
    follow_run_ref.current = follow_run_id.trim();
  }, [follow_run_id]);

  useEffect(() => {
    const rid = run_id.trim();
    const child = sub_run_id.trim();
    const should_follow = connected && wait_reason === "subworkflow" && Boolean(child) && child !== rid;
    if (!should_follow) {
      root_subrun_ref.current = "";
      if (follow_run_id.trim()) set_follow_run_id("");
      return;
    }
    // Only reset when the root's immediate child changes, so deeper-follow can take over.
    if (root_subrun_ref.current !== child) {
      root_subrun_ref.current = child;
      follow_run_ref.current = child;
      set_follow_run_id(child);
    }
  }, [connected, run_id, wait_reason, sub_run_id, follow_run_id]);

  useEffect(() => {
    // Bundle switch should invalidate any cached flow JSON.
    set_graph_flow_cache({});
  }, [bundle_id]);

  const graph_entrypoint_ids = useMemo(() => {
    const eps = Array.isArray(bundle_info?.entrypoints) ? bundle_info?.entrypoints : [];
    return (eps || [])
      .map((e: any) => String(e?.flow_id || "").trim())
      .filter(Boolean);
  }, [bundle_info]);

  useEffect(() => {
    const root = String(selected_entrypoint?.flow_id || "").trim();
    if (!root) return;
    if (!graph_flow_id.trim()) set_graph_flow_id(root);
  }, [selected_entrypoint, graph_flow_id]);

  useEffect(() => {
    const bid = bundle_id.trim();
    const fid = graph_flow_id.trim();
    if (!bid || !fid) {
      set_graph_flow(null);
      set_graph_error("");
      set_graph_loading(false);
      return;
    }
    let stopped = false;
    set_graph_loading(true);
    set_graph_error("");
    gateway
      .get_bundle_flow(bid, fid)
      .then((res) => {
        if (stopped) return;
        const flow = (res as any)?.flow;
        const vf = flow && typeof flow === "object" ? flow : null;
        set_graph_flow(vf);
        if (vf) set_graph_flow_cache((prev) => ({ ...prev, [fid]: vf }));
      })
      .catch((e: any) => {
        if (stopped) return;
        set_graph_flow(null);
        set_graph_error(String(e?.message || e || "Failed to load flow"));
      })
      .finally(() => {
        if (stopped) return;
        set_graph_loading(false);
      });
    return () => {
      stopped = true;
    };
  }, [bundle_id, graph_flow_id, gateway]);

  useEffect(() => {
    const bid = bundle_id.trim();
    if (!graph_show_subflows || !bid || !graph_flow) return;

    let stopped = false;
    const max_depth = 3;

    const extract_subflow_ids = (flow: any): string[] => {
      const nodes = Array.isArray(flow?.nodes) ? flow.nodes : [];
      const out: string[] = [];
      for (const n of nodes) {
        const data = n?.data && typeof n.data === "object" ? n.data : {};
        const nt = String((data as any)?.nodeType || n?.type || "").trim();
        if (nt !== "subflow") continue;
        const sid = (data as any)?.subflowId || (data as any)?.flowId;
        const s = typeof sid === "string" ? sid.trim() : "";
        if (s) out.push(s.includes(":") ? s.split(":", 2)[1] : s);
      }
      return out;
    };

    const seen = new Set<string>();
    const want = new Set<string>();
    const visit = (flow: any, depth: number) => {
      if (!flow || depth >= max_depth) return;
      for (const sid of extract_subflow_ids(flow)) {
        const s = String(sid || "").trim();
        if (!s || seen.has(s)) continue;
        seen.add(s);
        want.add(s);
        const cached = graph_flow_cache[s];
        if (cached) visit(cached, depth + 1);
      }
    };

    visit(graph_flow, 0);
    const missing = Array.from(want).filter((fid) => !graph_flow_cache[fid]).slice(0, 20);
    if (!missing.length) return;

    const run = async () => {
      for (const fid of missing) {
        if (stopped) return;
        try {
          const res = await gateway.get_bundle_flow(bid, fid);
          const flow = (res as any)?.flow;
          const vf = flow && typeof flow === "object" ? flow : null;
          if (!vf) continue;
          set_graph_flow_cache((prev) => (prev[fid] ? prev : { ...prev, [fid]: vf }));
        } catch {
          // ignore missing subflows (best-effort)
        }
      }
    };
    run();

    return () => {
      stopped = true;
    };
  }, [graph_show_subflows, bundle_id, graph_flow, graph_flow_cache, gateway]);

  const graph_flow_options = useMemo(() => {
    const flows = Array.isArray(bundle_info?.flows) ? (bundle_info?.flows as any[]) : [];
    const from_bundle = flows.map((x) => String(x || "").trim()).filter(Boolean);
    if (from_bundle.length) return Array.from(new Set(from_bundle)).sort();
    return Array.from(new Set(graph_entrypoint_ids)).sort();
  }, [bundle_info, graph_entrypoint_ids]);

  // Follow the current deepest descendant run for status/events.
  useEffect(() => {
    const child_id = follow_run_id.trim();
    const parent_id = run_id.trim();
    const should_follow = connected && Boolean(child_id) && child_id !== parent_id;

    if (!should_follow) {
      if (child_abort_ref.current) child_abort_ref.current.abort();
      child_abort_ref.current = null;
      child_cursor_ref.current = 0;
      if (following_child_run_id) set_following_child_run_id("");
      return;
    }

    if (following_child_run_id === child_id) return;

    if (child_abort_ref.current) child_abort_ref.current.abort();
    const abort = new AbortController();
    child_abort_ref.current = abort;
    child_cursor_ref.current = 0;
    set_following_child_run_id(child_id);
    push_log({ ts: now_iso(), kind: "info", title: `Following run ${child_id} (status/events)` });

    let backoff_ms = 250;
    const run = async () => {
      while (!abort.signal.aborted) {
        try {
          await gateway.stream_ledger(child_id, {
            after: child_cursor_ref.current,
            on_step: (ev) => handle_child_step(child_id, ev),
            signal: abort.signal,
          });
          return;
        } catch (e: any) {
          if (abort.signal.aborted) break;
          const msg = String(e?.message || e || "stream error");
          push_log({
            ts: now_iso(),
            kind: "error",
            title: `Child ledger stream error (will retry)`,
            preview: clamp_preview(msg),
            data: { child_run_id: child_id, error: msg },
          });
        }

        if (abort.signal.aborted) break;
        await new Promise((r) => setTimeout(r, backoff_ms));
        backoff_ms = Math.min(5000, Math.floor(backoff_ms * 1.6));
      }
    };
    run();

    return () => {
      abort.abort();
    };
  }, [connected, follow_run_id, run_id, gateway, following_child_run_id]);

  return (
    <div className="app-shell">
      <div className="container">
        <div className="title">
          <h1>AbstractCode Thin Client (Web/PWA)</h1>
          <div className="badge mono">
            {(gateway_connected ? "gateway ok" : discovery_loading ? "gateway…" : "gateway off")} •{" "}
            {(connected ? "run ok" : connecting ? "run…" : "run off")} • cursor {cursor}
          </div>
        </div>

        {is_narrow ? (
          <div className="tab_bar" style={{ justifyContent: "flex-start" }}>
            <button className={`tab mono ${mobile_tab === "controls" ? "active" : ""}`} onClick={() => set_mobile_tab("controls")}>
              Controls
            </button>
            <button className={`tab mono ${mobile_tab === "viewer" ? "active" : ""}`} onClick={() => set_mobile_tab("viewer")}>
              Viewer
            </button>
          </div>
        ) : null}

        <div className="app-main">
          <div className="panel" style={is_narrow && mobile_tab !== "controls" ? { display: "none" } : undefined}>
            <div className="card panel_card scroll_y">
            <div className="section_title mono">Connect</div>
            <div className="field">
              <label>Gateway URL (blank = same origin / dev proxy)</label>
              <div className="field_inline">
                <input
                  className="mono"
                  value={settings.gateway_url}
                  onChange={(e) => set_settings((s) => ({ ...s, gateway_url: e.target.value }))}
                  placeholder="https://your-gateway-host"
                />
                <button className="btn" onClick={on_discover_gateway} disabled={discovery_loading || connecting || resuming}>
                  {discovery_loading ? "Connecting…" : "Connect"}
                </button>
              </div>
              {discovery_error ? (
                <div className="mono" style={{ color: "rgba(239, 68, 68, 0.9)", fontSize: "12px" }}>
                  {discovery_error}
                </div>
              ) : null}
            </div>
            <div className="field">
              <label>Gateway token (Authorization: Bearer …)</label>
              <input
                className="mono"
                value={settings.auth_token}
                onChange={(e) => set_settings((s) => ({ ...s, auth_token: e.target.value }))}
                placeholder="(optional for localhost dev)"
              />
            </div>
            <div className="section_divider" />
            <div className="section_title mono">Workflow</div>
            <div className="field">
              <label>Workflows (discovered)</label>
              <select
                className="mono"
                value={selected_workflow_value}
                onChange={async (e) => {
                  const wid = String(e.target.value || "").trim();
                  if (!wid) return;
                  const parsed = parse_namespaced_workflow_id(wid);
                  if (!parsed) return;
                  set_bundle_id(parsed.bundle_id);
                  set_flow_id(parsed.flow_id);
                  set_graph_flow_id(parsed.flow_id);
                  await load_bundle_info(parsed.bundle_id);
                }}
                disabled={discovery_loading || !workflow_options.length}
              >
                <option value="">{workflow_options.length ? "(select)" : "(empty — click Connect)"}</option>
                {workflow_options.map((w) => (
                  <option key={w.workflow_id} value={w.workflow_id}>
                    {w.label}
                  </option>
                ))}
              </select>
              <div className="mono muted" style={{ fontSize: "12px" }}>
                Workflows are the gateway’s registered `.flow` bundles (configured via `ABSTRACTGATEWAY_FLOWS_DIR`).
              </div>
            </div>
            <div className="row">
              <div className="col">
                <div className="field">
                  <label>Bundle ID (optional)</label>
                  <div className="field_inline">
                    <input className="mono" value={bundle_id} onChange={(e) => set_bundle_id(e.target.value)} placeholder="ac-advanced-agent-v2" />
                    <button className="btn" onClick={on_load_bundle} disabled={bundle_loading || connecting || resuming || !bundle_id.trim()}>
                      {bundle_loading ? "Loading…" : "Load"}
                    </button>
                  </div>
                  {bundle_error ? <div className="mono" style={{ color: "rgba(239, 68, 68, 0.9)", fontSize: "12px" }}>{bundle_error}</div> : null}
                </div>
              </div>
              <div className="col">
                <div className="field">
                  <label>Flow ID (start run; optional with bundle entrypoint)</label>
                  <input
                    className="mono"
                    value={flow_id}
                    onChange={(e) => set_flow_id(e.target.value)}
                    placeholder="(optional) defaults to bundle entrypoint"
                  />
                </div>
              </div>
            </div>

            {bundle_info && String(bundle_info.bundle_id || "").trim() === bundle_id.trim() ? (
              <div className="log_item" style={{ borderColor: "rgba(96, 165, 250, 0.25)" }}>
                <div className="meta">
                  <span className="mono">bundle</span>
                  <span className="mono">
                    {String(bundle_info.bundle_id || "")}
                    {bundle_info.bundle_version ? ` • v${String(bundle_info.bundle_version)}` : ""}
                  </span>
                </div>
                <div className="body">
                  <div className="mono" style={{ fontSize: "12px", color: "var(--muted)" }}>
                    {Array.isArray(bundle_info.entrypoints) ? `${bundle_info.entrypoints.length} entrypoint(s)` : "entrypoints: (unknown)"}
                    {bundle_info.default_entrypoint ? ` • default: ${String(bundle_info.default_entrypoint)}` : ""}
                    {selected_entrypoint?.flow_id ? ` • selected: ${String(selected_entrypoint.flow_id)}` : ""}
                  </div>
                  <div className="log_actions">
                    {(Array.isArray(bundle_info.entrypoints) ? bundle_info.entrypoints : []).map((ep) => {
                      const fid = String(ep?.flow_id || "").trim();
                      if (!fid) return null;
                      const name = String(ep?.name || "").trim();
                      const label = name ? `${name} (${fid})` : fid;
                      return (
                        <button
                          key={fid}
                          className="btn"
                          onClick={() => {
                            set_flow_id(fid);
                            set_graph_flow_id(fid);
                          }}
                          disabled={connecting || resuming}
                        >
                          Use {label}
                        </button>
                      );
                    })}
                    {adaptive_pins.some((p) => p && typeof p === "object" && p.default !== undefined) ? (
                      <button className="btn" onClick={apply_entrypoint_defaults} disabled={connecting || resuming}>
                        Apply defaults
                      </button>
                    ) : null}
                  </div>

                  {adaptive_pins.length ? (
                    <div style={{ marginTop: "8px", display: "flex", flexDirection: "column", gap: "6px" }}>
                      {adaptive_pins.map((p) => {
                        const pid = String(p.id || "").trim();
                        if (!pid) return null;
                        const ptype = String(p.type || "").trim() || "unknown";
                        const def = p.default;
                        return (
                          <div key={pid} style={{ display: "flex", flexWrap: "wrap", gap: "8px", alignItems: "center" }}>
                            <span className="chip mono">{pid}</span>
                            <span className="chip mono muted">{ptype}</span>
                            {def !== undefined ? <span className="chip mono muted">default {safe_json_inline(def, 80)}</span> : null}
                          </div>
                        );
                      })}
                    </div>
                  ) : (
                    <div className="mono" style={{ marginTop: "8px", fontSize: "12px", color: "var(--muted)" }}>
                      (no input pin definitions discovered)
                    </div>
                  )}
                </div>
              </div>
            ) : null}

            <div className="section_divider" />
            <div className="section_title mono">Run</div>
            <div className="field">
              <label>Runs (recent — select to attach)</label>
              <select
                className="mono"
                value={run_options.some((r) => r.run_id === run_id.trim()) ? run_id.trim() : ""}
                onChange={async (e) => {
                  const rid = String(e.target.value || "").trim();
                  if (!rid) return;
                  await attach_to_run(rid);
                }}
                disabled={!gateway_connected || runs_loading || discovery_loading || connecting || resuming}
              >
                <option value="">
                  {runs_loading ? "(loading…)" : run_options.length ? "(select recent run)" : "(empty — click Connect)"}
                </option>
                {/* Keep the currently-selected run visible even if it isn't in the current list. */}
                {run_id.trim() && !run_options.some((r) => r.run_id === run_id.trim()) ? (
                  <option value={run_id.trim()}>{`(attached) ${run_id.trim()}`}</option>
                ) : null}
                {run_options.map((r) => {
                  const rid = String(r.run_id || "").trim();
                  if (!rid) return null;
                  const wid = typeof r.workflow_id === "string" ? String(r.workflow_id) : "";
                  const wf_label = wid ? workflow_label_by_id[wid] || wid : "";
                  const wf_display = wf_label ? wf_label.replace(/\s+/g, " ").trim() : "(unknown workflow)";
                  const st = typeof r.status === "string" ? String(r.status) : "";
                  const start_ts = format_local_ts(r.created_at || r.updated_at);
                  const cnt = typeof r.ledger_len === "number" ? `#${r.ledger_len}` : "";
                  const label = `${fixed_col(start_ts, 16)} | ${fixed_col(wf_display, 34)} | ${fixed_col(
                    st || "unknown",
                    10
                  )} | ${fixed_col(cnt, 6)} | ${fixed_col(short_id(rid, 14), 14)}`;
                  return (
                    <option key={rid} value={rid}>
                      {label}
                    </option>
                  );
                })}
              </select>
              {!run_options.length ? (
                <div className="mono muted" style={{ fontSize: "12px" }}>
                  Tip: click “Connect” to load recent runs from the gateway.
                </div>
              ) : null}
            </div>

            {has_adaptive_inputs ? (
              <>
                {input_data_obj === null ? (
                  <div className="log_item" style={{ borderColor: "rgba(239, 68, 68, 0.35)" }}>
                    <div className="meta">
                      <span className="mono">input error</span>
                      <span className="mono">{now_iso()}</span>
                    </div>
                    <div className="body mono">Invalid input JSON. Fix it in Advanced JSON.</div>
                  </div>
                ) : null}

                {adaptive_pins.map((p) => {
                  const pid = String(p?.id || "").trim();
                  if (!pid) return null;
                  const ptype = String(p?.type || "").trim().toLowerCase() || "unknown";
                  const disabled = input_data_obj === null || connecting || resuming;
                  const cur = input_data_obj && input_data_obj[pid] !== undefined ? input_data_obj[pid] : undefined;
                  const err = input_field_errors[pid];
                  const def = (p as any).default;

                  const label_bits: string[] = [pid];
                  if (ptype) label_bits.push(ptype);
                  if (def !== undefined) label_bits.push(`default ${safe_json_inline(def, 60)}`);

                  const label = label_bits.join(" • ");

                  // Special-cases: tools/context are common and benefit from dedicated widgets.
                  if (pid === "tools") {
                    const cur_arr: string[] = Array.isArray(cur) ? (cur as any[]).map((x) => String(x)).filter(Boolean) : [];
                    const draft = input_field_drafts[pid] ?? cur_arr.join("\n");
                    const has_discovery = available_tool_names.length > 0;
                    return (
                      <div key={pid} className="field">
                        <label>{label}</label>
                        {has_discovery ? (
                          <MultiSelect
                            options={available_tool_names}
                            value={cur_arr}
                            disabled={disabled}
                            placeholder="Select allowed tools…"
                            onChange={(next) => {
                              set_input_field_drafts((prev) => {
                                const n = { ...prev };
                                delete n[pid];
                                return n;
                              });
                              set_input_field_errors((prev) => {
                                const n = { ...prev };
                                delete n[pid];
                                return n;
                              });
                              if (!next.length) delete_input_data_key(pid);
                              else set_input_data_value(pid, next);
                            }}
                          />
                        ) : (
                          <textarea
                            className="mono"
                            disabled={disabled}
                            value={draft}
                            onChange={(e) => {
                              const text = e.target.value;
                              set_input_field_drafts((prev) => ({ ...prev, [pid]: text }));
                              const arr = text
                                .split("\n")
                                .map((x) => x.trim())
                                .filter(Boolean);
                              set_input_field_errors((prev) => {
                                const next = { ...prev };
                                delete next[pid];
                                return next;
                              });
                              if (!arr.length) delete_input_data_key(pid);
                              else set_input_data_value(pid, arr);
                            }}
                            rows={4}
                            placeholder={"list_files\nsearch_files\nread_file\n..."}
                          />
                        )}
                        {err ? <div className="mono" style={{ color: "rgba(239, 68, 68, 0.9)", fontSize: "12px" }}>{err}</div> : null}
                        {!has_discovery ? (
                          <div className="mono muted" style={{ fontSize: "12px" }}>
                            Tip: click “Connect” to load tool list from the gateway.
                          </div>
                        ) : null}
                      </div>
                    );
                  }

                  if (pid === "provider" || ptype === "provider") {
                    const has_discovery = available_providers.length > 0;
                    return (
                      <div key={pid} className="field">
                        <label>{label}</label>
                        {has_discovery ? (
                          <select
                            className="mono"
                            disabled={disabled}
                            value={provider_value}
                            onChange={(e) => {
                              const next = String(e.target.value || "").trim();
                              if (!next) {
                                delete_input_data_key("provider");
                                delete_input_data_key("model");
                                return;
                              }
                              set_input_data_value("provider", next);
                              if (model_value.trim()) delete_input_data_key("model");
                            }}
                          >
                            <option value="">(select)</option>
                            {available_providers.map((pname) => (
                              <option key={pname} value={pname}>
                                {pname}
                              </option>
                            ))}
                          </select>
                        ) : (
                          <input
                            className="mono"
                            disabled={disabled}
                            value={provider_value}
                            onChange={(e) => {
                              const t = String(e.target.value || "");
                              if (!t.trim()) delete_input_data_key(pid);
                              else set_input_data_value(pid, t);
                            }}
                          />
                        )}
                        {!has_discovery ? (
                          <div className="mono muted" style={{ fontSize: "12px" }}>
                            Tip: click “Connect” to load providers from the gateway.
                          </div>
                        ) : null}
                        {err ? <div className="mono" style={{ color: "rgba(239, 68, 68, 0.9)", fontSize: "12px" }}>{err}</div> : null}
                      </div>
                    );
                  }

                  if (pid === "model" || ptype === "model") {
                    const prov = provider_value.trim();
                    const models = models_for_provider.models;
                    const has_models = models.length > 0;
                    const disabled_model = disabled || !prov;
                    return (
                      <div key={pid} className="field">
                        <label>{label}</label>
                        <select
                          className="mono"
                          disabled={disabled_model}
                          value={model_value}
                          onChange={(e) => {
                            const next = String(e.target.value || "").trim();
                            if (!next) delete_input_data_key("model");
                            else set_input_data_value("model", next);
                          }}
                        >
                          <option value="">{prov ? (has_models ? "(select)" : "(loading…)") : "(select provider first)"}</option>
                          {models.map((m) => (
                            <option key={m} value={m}>
                              {m}
                            </option>
                          ))}
                        </select>
                        {models_for_provider.error ? (
                          <div className="mono" style={{ color: "rgba(239, 68, 68, 0.9)", fontSize: "12px" }}>
                            {models_for_provider.error}
                          </div>
                        ) : null}
                        {err ? <div className="mono" style={{ color: "rgba(239, 68, 68, 0.9)", fontSize: "12px" }}>{err}</div> : null}
                      </div>
                    );
                  }

                  if (ptype === "number" || pid === "max_iterations") {
                    const base = typeof cur === "number" ? String(cur) : cur !== undefined ? String(cur) : "";
                    const draft = input_field_drafts[pid] ?? base;
                    return (
                      <div key={pid} className="field">
                        <label>{label}</label>
                        <input
                          className="mono"
                          disabled={disabled}
                          type="number"
                          value={draft}
                          onChange={(e) => {
                            const raw = e.target.value;
                            set_input_field_drafts((prev) => ({ ...prev, [pid]: raw }));
                            if (!raw.trim()) {
                              set_input_field_errors((prev) => {
                                const next = { ...prev };
                                delete next[pid];
                                return next;
                              });
                              delete_input_data_key(pid);
                              return;
                            }
                            const num = Number(raw);
                            if (!Number.isFinite(num)) {
                              set_input_field_errors((prev) => ({ ...prev, [pid]: "Invalid number" }));
                              return;
                            }
                            set_input_field_errors((prev) => {
                              const next = { ...prev };
                              delete next[pid];
                              return next;
                            });
                            set_input_data_value(pid, num);
                          }}
                        />
                        {err ? <div className="mono" style={{ color: "rgba(239, 68, 68, 0.9)", fontSize: "12px" }}>{err}</div> : null}
                      </div>
                    );
                  }

                  if (ptype === "boolean") {
                    const checked = Boolean(cur === true);
                    return (
                      <div key={pid} className="field">
                        <label>{label}</label>
                        <label className="mono" style={{ display: "flex", gap: "10px", alignItems: "center" }}>
                          <input
                            type="checkbox"
                            checked={checked}
                            disabled={disabled}
                            onChange={(e) => {
                              set_input_field_errors((prev) => {
                                const next = { ...prev };
                                delete next[pid];
                                return next;
                              });
                              set_input_data_value(pid, Boolean(e.target.checked));
                            }}
                          />
                          {checked ? "true" : "false"}
                        </label>
                        {err ? <div className="mono" style={{ color: "rgba(239, 68, 68, 0.9)", fontSize: "12px" }}>{err}</div> : null}
                      </div>
                    );
                  }

                  if (ptype === "object" || ptype === "array" || pid === "context") {
                    const draft = input_field_drafts[pid] ?? (cur !== undefined ? safe_json(cur) : "");
                    return (
                      <div key={pid} className="field">
                        <label>{label}</label>
                        <textarea
                          className="mono"
                          disabled={disabled}
                          value={draft}
                          onChange={(e) => {
                            const text = e.target.value;
                            set_input_field_drafts((prev) => ({ ...prev, [pid]: text }));
                            if (!text.trim()) {
                              set_input_field_errors((prev) => {
                                const next = { ...prev };
                                delete next[pid];
                                return next;
                              });
                              delete_input_data_key(pid);
                              return;
                            }
                            try {
                              const parsed = JSON.parse(text);
                              set_input_field_errors((prev) => {
                                const next = { ...prev };
                                delete next[pid];
                                return next;
                              });
                              set_input_data_value(pid, parsed);
                            } catch (e2: any) {
                              set_input_field_errors((prev) => ({ ...prev, [pid]: String(e2?.message || "Invalid JSON") }));
                            }
                          }}
                          rows={4}
                          placeholder={ptype === "array" ? '["a","b"]' : '{"key":"value"}'}
                        />
                        {err ? <div className="mono" style={{ color: "rgba(239, 68, 68, 0.9)", fontSize: "12px" }}>{err}</div> : null}
                      </div>
                    );
                  }

                  const is_textarea = pid === "request" || pid.endsWith("_prompt") || pid.includes("prompt");
                  const sv = typeof cur === "string" ? String(cur) : cur !== undefined ? String(cur) : "";
                  return (
                    <div key={pid} className="field">
                      <label>{label}</label>
                      {is_textarea ? (
                        <textarea
                          className="mono"
                          disabled={disabled}
                          value={sv}
                          onChange={(e) => {
                            const t = e.target.value;
                            if (!t.trim()) delete_input_data_key(pid);
                            else set_input_data_value(pid, t);
                          }}
                          rows={3}
                        />
                      ) : (
                        <input
                          className="mono"
                          disabled={disabled}
                          value={sv}
                          onChange={(e) => {
                            const t = e.target.value;
                            if (!t.trim()) delete_input_data_key(pid);
                            else set_input_data_value(pid, t);
                          }}
                        />
                      )}
                      {err ? <div className="mono" style={{ color: "rgba(239, 68, 68, 0.9)", fontSize: "12px" }}>{err}</div> : null}
                    </div>
                  );
                })}

                <details style={{ marginTop: "6px" }}>
                  <summary className="mono" style={{ color: "var(--muted)", cursor: "pointer" }}>
                    Advanced: input_data JSON
                  </summary>
                  <div className="field" style={{ marginTop: "10px" }}>
                    <label>Input data (JSON)</label>
                    <textarea
                      className="mono"
                      value={input_data_text}
                      onChange={(e) => set_input_data_text(e.target.value)}
                      placeholder='{"request":"..."}'
                      rows={8}
                      disabled={connecting || resuming}
                    />
                  </div>
                </details>
              </>
            ) : (
              <>
                <div className="field">
                  <label>Request (common)</label>
                  <textarea
                    className="mono"
                    value={request_value}
                    onChange={(e) => update_input_data_field("request", e.target.value)}
                    placeholder="What do you want the workflow/agent to do?"
                    rows={3}
                  />
                </div>
                <div className="row">
                  <div className="col">
                    <div className="field">
                      <label>Provider (common)</label>
                      <input
                        className="mono"
                        value={provider_value}
                        onChange={(e) => update_input_data_field("provider", e.target.value)}
                        placeholder="lmstudio / ollama / openai / ..."
                      />
                    </div>
                  </div>
                  <div className="col">
                    <div className="field">
                      <label>Model (common)</label>
                      <input
                        className="mono"
                        value={model_value}
                        onChange={(e) => update_input_data_field("model", e.target.value)}
                        placeholder="qwen/qwen3-next-80b / gpt-4.1 / ..."
                      />
                    </div>
                  </div>
                </div>

                <div className="field">
                  <label>Input data (JSON)</label>
                  <textarea
                    className="mono"
                    value={input_data_text}
                    onChange={(e) => set_input_data_text(e.target.value)}
                    placeholder='{"request":"...","provider":"lmstudio","model":"qwen/qwen3-next-80b"}'
                    rows={6}
                  />
                </div>
              </>
            )}

            <div className="actions">
              <button className="btn" onClick={clear_run_view} disabled={connecting || resuming}>
                New Run
              </button>
              <button className="btn primary" onClick={on_start_run} disabled={connecting || resuming || (connected && run_id.trim() !== "")}>
                Start Run
              </button>
              <button className="btn" onClick={() => submit_run_control(pause_resume_action)} disabled={pause_resume_disabled}>
                {pause_resume_label}
              </button>
              <button className="btn danger" onClick={() => submit_run_control("cancel")} disabled={!run_id.trim() || connecting || resuming || run_terminal}>
                Cancel
              </button>
              <button
                className="btn danger"
                onClick={() => {
                  if (abort_ref.current) abort_ref.current.abort();
                  if (child_abort_ref.current) child_abort_ref.current.abort();
                  child_abort_ref.current = null;
                  child_cursor_ref.current = 0;
                  set_following_child_run_id("");
                  set_connected(false);
                  set_connecting(false);
                  set_resuming(false);
                }}
              >
                Disconnect
              </button>
            </div>

            <div className="field">
              <label>Reason (optional — for pause/cancel audit)</label>
              <input className="mono" value={control_reason} onChange={(e) => set_control_reason(e.target.value)} placeholder="reason…" />
            </div>

            {is_waiting ? (
              <div className="log_item" style={{ borderColor: "rgba(96, 165, 250, 0.25)" }}>
                <div className="meta">
                  <span className="mono">waiting</span>
                  <span className="mono">{wait_reason || "unknown"}</span>
                </div>
                <div className="body mono">
                  {safe_json({
                    wait_key: wait_key,
                    reason: wait_reason,
                    sub_run_id: sub_run_id || undefined,
                  })}
                </div>
                {wait_reason === "subworkflow" && sub_run_id ? (
                  <div className="actions">
                    <button
                      className="btn primary"
                      onClick={async () => {
                        set_run_id(sub_run_id);
                        await connect_to_run(sub_run_id);
                      }}
                      disabled={connecting}
                    >
                      Attach to child run
                    </button>
                    {root_run_id.trim() && root_run_id.trim() !== run_id.trim() ? (
                      <button
                        className="btn"
                        onClick={async () => {
                          set_run_id(root_run_id.trim());
                          await connect_to_run(root_run_id.trim());
                        }}
                        disabled={connecting}
                      >
                        Back to root
                      </button>
                    ) : null}
                  </div>
                ) : null}
              </div>
            ) : null}

            {run_state ? (
              <details style={{ marginTop: "10px" }}>
                <summary className="mono" style={{ color: "var(--muted)", cursor: "pointer" }}>
                  Run state • {run_status || "unknown"}
                  {run_paused ? " • paused" : ""}
                </summary>
                <div className="log_item" style={{ marginTop: "10px" }}>
                  <div className="body mono">
                    {safe_json({
                      status: run_state?.status,
                      paused: run_state?.paused,
                      current_node: run_state?.current_node,
                      waiting: run_state?.waiting
                        ? {
                            reason: run_state.waiting.reason,
                            wait_key: run_state.waiting.wait_key,
                            prompt: run_state.waiting.prompt,
                            details: run_state.waiting.details,
                          }
                        : null,
                      error: run_state?.error,
                    })}
                  </div>
                </div>
              </details>
            ) : null}

            <div className="section_divider" />
            <div className="section_title mono">Remote Worker</div>
            <div className="field">
              <label>Tool worker (advanced / potentially dangerous) — MCP HTTP endpoint</label>
              <input
                className="mono"
                value={settings.worker_url}
                onChange={(e) => set_settings((s) => ({ ...s, worker_url: e.target.value }))}
                placeholder="https://your-mcp-worker-endpoint"
              />
            </div>
            <div className="field">
              <label>Tool worker token (Authorization: Bearer …)</label>
              <input
                className="mono"
                value={settings.worker_token}
                onChange={(e) => set_settings((s) => ({ ...s, worker_token: e.target.value }))}
                placeholder="(optional)"
              />
            </div>

            {error_text ? (
              <div className="log_item" style={{ borderColor: "rgba(239, 68, 68, 0.35)" }}>
                <div className="meta">
                  <span className="mono">error</span>
                  <span className="mono">{now_iso()}</span>
                </div>
                <div className="body mono">{error_text}</div>
              </div>
            ) : null}
          </div>
          </div>

          <div className="panel" style={is_narrow && mobile_tab !== "viewer" ? { display: "none" } : undefined}>
            <div className="card panel_card card_scroll">
              <div className="tab_bar">
                <button className={`tab mono ${right_tab === "ledger" ? "active" : ""}`} onClick={() => set_right_tab("ledger")}>
                  Ledger
                </button>
                <button className={`tab mono ${right_tab === "graph" ? "active" : ""}`} onClick={() => set_right_tab("graph")}>
                  Graph
                </button>
                <button className={`tab mono ${right_tab === "digest" ? "active" : ""}`} onClick={() => set_right_tab("digest")}>
                  Digest
                </button>
              </div>

              {right_tab === "ledger" ? (
                <>
                  <div className="meta" style={{ marginTop: "10px" }}>
                    <span className="mono">ledger log (newest first)</span>
                    <span className="mono">{run_id.trim() ? `run ${run_id.trim()}` : ""}</span>
                  </div>
                  <div className="log_actions" style={{ marginTop: "10px" }}>
                    <button
                      className="btn"
                      disabled={!records.length}
                      onClick={() => {
                        const max = 5000;
                        const items = records.length > max ? records.slice(records.length - max) : records;
                        const text = items.map((x) => JSON.stringify({ cursor: x.cursor, record: x.record })).join("\n");
                        copy_to_clipboard(text);
                      }}
                    >
                      Copy ledger (JSONL)
                    </button>
                  </div>
                  <div className="log log_scroll">
                    {log.map((item) => (
                      <LedgerCard
                        key={item.id}
                        item={item}
                        open={log_open[item.id] === true}
                        on_toggle={() => set_log_open((prev) => ({ ...prev, [item.id]: !prev[item.id] }))}
                        node_index={node_index_for_run}
                        on_copy={copy_to_clipboard}
                      />
                    ))}
                  </div>
                </>
              ) : right_tab === "graph" ? (
                <>
                  <div className="meta" style={{ marginTop: "10px" }}>
                    <span className="mono">workflow graph</span>
                    <span className="mono">{bundle_id.trim() && graph_flow_id.trim() ? `${bundle_id.trim()}:${graph_flow_id.trim()}` : ""}</span>
                  </div>

                  <div className="graph_toolbar">
                    <div className="field" style={{ margin: 0 }}>
                      <label>Flow</label>
                      <select
                        className="mono"
                        value={graph_flow_id}
                        onChange={(e) => set_graph_flow_id(String(e.target.value || ""))}
                        disabled={!bundle_id.trim() || graph_loading}
                      >
                        <option value="">(select)</option>
                        {graph_flow_options.map((fid) => (
                          <option key={fid} value={fid}>
                            {fid}
                          </option>
                        ))}
                      </select>
                    </div>
                    <div className="field" style={{ margin: 0 }}>
                      <label>View</label>
                      <label className="mono" style={{ display: "flex", gap: "10px", alignItems: "center" }}>
                        <input
                          type="checkbox"
                          checked={graph_show_subflows}
                          onChange={(e) => set_graph_show_subflows(Boolean(e.target.checked))}
                        />
                        subflows {graph_show_subflows ? "shown" : "hidden"}
                      </label>
                      <label className="mono" style={{ display: "flex", gap: "10px", alignItems: "center" }}>
                        <input
                          type="checkbox"
                          checked={graph_highlight_path}
                          onChange={(e) => set_graph_highlight_path(Boolean(e.target.checked))}
                        />
                        highlight path
                      </label>
                    </div>
                    <div className="field" style={{ margin: 0 }}>
                      <label>Actions</label>
                      <div className="field_inline">
                        <button
                          className="btn"
                          onClick={() => {
                            const root = String(selected_entrypoint?.flow_id || "").trim();
                            if (root) set_graph_flow_id(root);
                          }}
                          disabled={!selected_entrypoint?.flow_id}
                        >
                          Go to root
                        </button>
                        <button
                          className="btn"
                          onClick={() => {
                            if (graph_flow) copy_to_clipboard(JSON.stringify(graph_flow, null, 2));
                          }}
                          disabled={!graph_flow}
                        >
                          Copy flow JSON
                        </button>
                      </div>
                    </div>
                  </div>

                  {graph_error ? (
                    <div className="log_item" style={{ borderColor: "rgba(239, 68, 68, 0.35)" }}>
                      <div className="meta">
                        <span className="mono">graph error</span>
                        <span className="mono">{now_iso()}</span>
                      </div>
                      <div className="body mono">{graph_error}</div>
                    </div>
                  ) : null}
                  {graph_loading ? (
                    <div className="log_item" style={{ borderColor: "rgba(96, 165, 250, 0.25)" }}>
                      <div className="meta">
                        <span className="mono">loading</span>
                        <span className="mono">{graph_flow_id}</span>
                      </div>
                      <div className="body mono">Loading graph…</div>
                    </div>
                  ) : null}

                  <div className="graph_panel">
                    <FlowGraph
                      flow={graph_flow}
                      flow_by_id={graph_flow_cache}
                      expand_subflows={graph_show_subflows}
                      simplify={true}
                      active_node_id={active_node_id}
                      recent_nodes={recent_nodes}
                      visited_nodes={visited_nodes}
                      highlight_path={graph_highlight_path}
                      now_ms={graph_now_ms}
                    />
                  </div>
                </>
              ) : (
                <>
                  <div className="meta" style={{ marginTop: "10px" }}>
                    <span className="mono">digest</span>
                    <span className="mono">{run_id.trim() ? `run ${run_id.trim()}` : ""}</span>
                  </div>
                  <div className="log_actions" style={{ marginTop: "10px" }}>
                    <button
                      className="btn"
                      onClick={() => {
                        copy_to_clipboard(JSON.stringify(digest, null, 2));
                      }}
                      disabled={!digest.stats.steps}
                    >
                      Copy digest (JSON)
                    </button>
                  </div>

                  <div className="log log_scroll">
                    <div className="log_item" style={{ borderColor: "rgba(96, 165, 250, 0.25)" }}>
                      <div className="meta">
                        <span className="mono">stats</span>
                        <span className="mono">{digest.stats.duration_s ? `${digest.stats.duration_s}s` : ""}</span>
                      </div>
                      <div className="body mono">
                        {safe_json({
                          steps: digest.stats.steps,
                          llm_calls: digest.stats.llm_calls,
                          tool_calls: digest.stats.tool_calls,
                          unique_tools: digest.stats.unique_tools,
                          tokens: digest.stats.total_tokens
                            ? { prompt: digest.stats.prompt_tokens, completion: digest.stats.completion_tokens, total: digest.stats.total_tokens }
                            : null,
                          started_at: digest.stats.started_at || null,
                          ended_at: digest.stats.ended_at || null,
                        })}
                      </div>
                    </div>

                    {digest.files.length ? (
                      <div className="log_item">
                        <div className="meta">
                          <span className="mono">files</span>
                          <span className="mono">{digest.files.length}</span>
                        </div>
                        <div className="body mono">
                          {digest.files.slice(0, 120).map((f, idx) => (
                            <div key={`${f.run_id}:${f.ts}:${f.file_path}:${idx}`}>
                              {f.tool} • {f.file_path}
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : null}

                    {digest.commands.length ? (
                      <div className="log_item">
                        <div className="meta">
                          <span className="mono">commands</span>
                          <span className="mono">{digest.commands.length}</span>
                        </div>
                        <div className="body mono">
                          {digest.commands.slice(0, 80).map((c, idx) => (
                            <div key={`${c.run_id}:${c.ts}:${idx}`}>{c.command}</div>
                          ))}
                        </div>
                      </div>
                    ) : null}

                    {digest.web.length ? (
                      <div className="log_item">
                        <div className="meta">
                          <span className="mono">web</span>
                          <span className="mono">{digest.web.length}</span>
                        </div>
                        <div className="body mono">
                          {digest.web.slice(0, 80).map((w, idx) => (
                            <div key={`${w.run_id}:${w.ts}:${idx}`}>
                              {w.tool} • {w.value}
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : null}

                    {digest.tools_used.length ? (
                      <div className="log_item">
                        <div className="meta">
                          <span className="mono">tools used</span>
                          <span className="mono">{digest.tools_used.length}</span>
                        </div>
                        <div className="body mono">{digest.tools_used.join(", ")}</div>
                      </div>
                    ) : null}
                  </div>
                </>
              )}
            </div>

            <div className={`status_bar ${status_pulse ? "pulse" : ""}`}>
              <strong>Status</strong>: {status_text ? <span className="mono">{status_text}</span> : <span className="mono">(none)</span>}
            </div>
          </div>
        </div>

        {show_wait_modal ? (
          <div className="overlay">
            <div className="modal">
              <h2 className="mono">Run is waiting ({String(wait_state?.reason || "unknown")})</h2>
              <p className="mono">wait_key: {String(wait_state?.wait_key || "")}</p>

              {tool_calls_for_wait.length ? (
                <>
                  <div className="field">
                    <label>Tool calls (from wait.details.tool_calls)</label>
                    <textarea className="mono" readOnly value={safe_json(tool_calls_for_wait)} />
                  </div>
                  <div className="actions">
                    <button className="btn primary" disabled={!worker || resuming} onClick={() => execute_tools_via_worker(tool_calls_for_wait)}>
                      Execute via tool worker + resume
                    </button>
                    <button className="btn" disabled={resuming} onClick={() => resume_wait({ approved: true })}>
                      Resume (manual / advanced)
                    </button>
                  </div>
                </>
              ) : (
                <>
                  <div className="field">
                    <label>Prompt</label>
                    <textarea className="mono" readOnly value={String(wait_state?.prompt || "") || "(no prompt provided)"} />
                  </div>

                  <AskForm wait={wait_state as WaitState} disabled={resuming} on_submit={(val) => resume_wait({ response: val })} />
                </>
              )}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function AskForm(props: { wait: WaitState; disabled?: boolean; on_submit: (value: string) => void }): React.ReactElement {
  const [value, set_value] = useState("");
  const choices = Array.isArray(props.wait.choices) ? props.wait.choices : [];
  const allow_free_text = props.wait.allow_free_text !== false;
  const disabled = props.disabled === true;

  return (
    <>
      {choices.length ? (
        <div className="field">
          <label>Choices</label>
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
          <label>Response</label>
          <input className="mono" value={value} onChange={(e) => set_value(e.target.value)} placeholder="Type response…" />
        </div>
      ) : null}

      <div className="actions">
        <button className="btn primary" disabled={disabled || !value.trim()} onClick={() => props.on_submit(value.trim())}>
          Submit response
        </button>
      </div>
    </>
  );
}

function LedgerCard(props: {
  item: UiLogItem;
  open: boolean;
  on_toggle: () => void;
  node_index: Record<string, any>;
  on_copy: (text: string) => void;
}): React.ReactElement {
  const item = props.item;
  const node_id = String(item.node_id || "").trim();
  const meta = node_id && props.node_index && typeof props.node_index === "object" ? (props.node_index as any)[node_id] : null;
  const node_label = typeof meta?.label === "string" && meta.label.trim() ? meta.label.trim() : node_id || item.title;
  const node_type = typeof meta?.type === "string" && meta.type.trim() ? meta.type.trim() : "";
  const header_color = typeof meta?.headerColor === "string" && meta.headerColor.trim() ? meta.headerColor.trim() : "";
  const display_label = item.kind === "step" ? node_label : item.title;

  const accent =
    header_color ||
    (item.kind === "error"
      ? "rgba(239, 68, 68, 0.85)"
      : item.kind === "message"
        ? "rgba(167, 139, 250, 0.85)"
        : item.kind === "event"
          ? "rgba(96, 165, 250, 0.85)"
          : item.status === "waiting"
            ? "rgba(96, 165, 250, 0.65)"
            : item.status === "completed"
              ? "rgba(34, 197, 94, 0.65)"
              : "rgba(255, 255, 255, 0.14)");

  const status = String(item.status || "").trim();
  const status_chip =
    status === "completed"
      ? "chip ok"
      : status === "failed"
        ? "chip danger"
        : status === "waiting"
          ? "chip"
          : status
            ? "chip muted"
            : "chip muted";

  return (
    <div className="log_item card" style={{ ["--card-accent" as any]: accent }}>
      <div className="meta">
        <span className="mono">
          {item.kind} • {display_label}
        </span>
        <span className="mono">{item.ts}</span>
      </div>
      <div className="meta2">
        {status ? <span className={`mono ${status_chip}`}>{status}</span> : null}
        {node_type ? <span className="chip mono muted">{node_type}</span> : null}
        {item.effect_type ? <span className="chip mono muted">{String(item.effect_type)}</span> : null}
        {item.cursor ? <span className="chip mono muted">#{item.cursor}</span> : null}
        {item.run_id ? <span className="chip mono muted">{short_id(String(item.run_id), 10)}</span> : null}
        {item.kind !== "step" && node_id ? <span className="chip mono muted">{node_id}</span> : null}
      </div>
      {item.preview ? <div className="log_preview mono">{item.preview}</div> : null}
      {item.data ? (
        <div className="log_actions">
          <button className="btn" onClick={props.on_toggle}>
            {props.open ? "Fold JSON" : "Unfold JSON"}
          </button>
          <button
            className="btn"
            onClick={() => {
              try {
                props.on_copy(JSON.stringify(item.data, null, 2));
              } catch {
                props.on_copy(String(item.data));
              }
            }}
          >
            Copy JSON
          </button>
        </div>
      ) : null}
      {props.open && item.data ? (
        <div className="body mono">
          <JsonViewer value={item.data} max_string_len={220} />
        </div>
      ) : null}
    </div>
  );
}
