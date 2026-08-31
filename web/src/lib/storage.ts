import { random_id } from "./ids";

export type Settings = {
  gateway_url: string;
  auth_token: string;
  gateway_user: string;
  gateway_auth_mode: "session" | "direct";
  gateway_remember: boolean;
  gateway_was_connected: boolean;

  // Appearance (theme/font_scale/header_density) is NOT part of this object:
  // it is owned by the kit's useAppearanceSettings("abstractcode") under
  // af_appearance_abstractcode_v1 (tier-1 theme compliance). This composite
  // key is the hook's legacyKey — old stored blobs carry the three fields and
  // migrate forward exactly once.

  client_id: string;

  provider: string;
  model: string;

  max_iterations: number;
  max_in_tokens: number;
  temperature: number;
  seed: number;
  use_context: boolean;
  prompt_cache: boolean;

  system: string;
  resp_schema: string;

  tools_initialized: boolean;
  tools: string[];

  files_keep: boolean;

  // Optional workspace scope controls for /files/search + attachment ingestion.
  workspace_root?: string;
  workspace_access_mode?: string;
  workspace_allowed_paths?: string;
  workspace_ignored_paths?: string;
};

export type ReplTemplate = {
  bundle_id: string;
  flow_id: string;
  name?: string;
};

export type ReplMessage = {
  role: "user" | "assistant" | "system";
  content: string;
  ts: string;
  run_id?: string;
  title?: string;
  level?: "info" | "warn" | "error";
  meta?: any;
};

export type ReplState = {
  template: ReplTemplate | null;
  messages: ReplMessage[];
  created_at: string;
  updated_at: string;
};

type KV = {
  getItem: (k: string) => string | null;
  setItem: (k: string, v: string) => void;
  removeItem: (k: string) => void;
};

function _has_local_storage(): boolean {
  try {
    const ls: any = (globalThis as any)?.localStorage;
    if (!ls) return false;
    const k = "__acode_probe__";
    ls.setItem(k, "1");
    ls.removeItem(k);
    return true;
  } catch {
    return false;
  }
}

const _memory = new Map<string, string>();

function _store(): KV {
  if (_has_local_storage()) return (globalThis as any).localStorage as KV;
  return {
    getItem: (k) => (_memory.has(k) ? (_memory.get(k) as string) : null),
    setItem: (k, v) => void _memory.set(k, v),
    removeItem: (k) => void _memory.delete(k),
  };
}

export function storage_mode(): string {
  return _has_local_storage() ? "localStorage" : "memory";
}

function _now_iso(): string {
  try {
    return new Date().toISOString();
  } catch {
    return "";
  }
}

const KEY_SETTINGS = "abstractcode.settings.v1";
const KEY_CURRENT_SESSION = "abstractcode.current_session_id.v1";
const KEY_SESSION_PREFIX = "abstractcode.session.v1:";
const KEY_ACTIVE_RUN_PREFIX = "abstractcode.active_run_id.v1:";
const KEY_RUN_CURSOR_PREFIX = "abstractcode.run_cursor.v1:";
const KEY_SESSION_TOOL_APPROVE_ALL_PREFIX = "abstractcode.session_tool_approve_all.v1:";
const KEY_SESSION_WORKSPACE_ROOT_PREFIX = "abstractcode.session_workspace_root.v1:";

function _safe_parse<T>(raw: string | null, fallback: T): T {
  try {
    if (!raw) return fallback;
    const obj = JSON.parse(raw);
    return (obj as T) ?? fallback;
  } catch {
    return fallback;
  }
}

function _default_settings(): Settings {
  return {
    gateway_url: "",
    auth_token: "",
    gateway_user: "",
    gateway_auth_mode: "session",
    gateway_remember: true,
    gateway_was_connected: false,

    client_id: "abstractcode_web",

    provider: "",
    model: "",

    max_iterations: 20,
    max_in_tokens: 0,
    temperature: 0.7,
    seed: -1,
    use_context: true,
    prompt_cache: false,

    system: "",
    resp_schema: "",

    tools_initialized: false,
    tools: [],

    files_keep: true,

    workspace_root: "",
    workspace_access_mode: "workspace_only",
    workspace_allowed_paths: "",
    workspace_ignored_paths: "",
  };
}

export function load_settings(): Settings {
  const st = _store();
  const parsed = _safe_parse<any>(st.getItem(KEY_SETTINGS), null);
  const base = _default_settings();
  if (!parsed || typeof parsed !== "object") return base;
  const out: Settings = { ...base, ...(parsed as any) };
  out.gateway_url = String(out.gateway_url || "");
  out.auth_token = String(out.auth_token || "");
  out.gateway_user = String((out as any).gateway_user || "");
  out.gateway_auth_mode = (out as any).gateway_auth_mode === "direct" ? "direct" : "session";
  out.gateway_remember = (out as any).gateway_remember === false ? false : true;
  out.gateway_was_connected = Boolean((out as any).gateway_was_connected);
  out.client_id = String(out.client_id || base.client_id);
  out.provider = String(out.provider || "");
  out.model = String(out.model || "");
  out.max_iterations = Number.isFinite(Number(out.max_iterations)) ? Math.max(1, Math.trunc(Number(out.max_iterations))) : base.max_iterations;
  out.max_in_tokens = Number.isFinite(Number(out.max_in_tokens)) ? Math.max(0, Math.trunc(Number(out.max_in_tokens))) : base.max_in_tokens;
  out.temperature = Number.isFinite(Number(out.temperature)) ? Number(out.temperature) : base.temperature;
  out.seed = Number.isFinite(Number(out.seed)) ? Math.trunc(Number(out.seed)) : base.seed;
  out.use_context = Boolean((out as any).use_context);
  out.prompt_cache = Boolean((out as any).prompt_cache);
  out.system = String(out.system || "");
  out.resp_schema = String(out.resp_schema || "");
  out.tools_initialized = Boolean((out as any).tools_initialized);
  out.tools = Array.isArray((out as any).tools) ? (out as any).tools.map((x: any) => String(x || "").trim()).filter(Boolean) : [];
  out.files_keep = Boolean((out as any).files_keep);
  out.workspace_root = String((out as any).workspace_root || "");
  out.workspace_access_mode = String((out as any).workspace_access_mode || base.workspace_access_mode);
  out.workspace_allowed_paths = String((out as any).workspace_allowed_paths || "");
  out.workspace_ignored_paths = String((out as any).workspace_ignored_paths || "");
  return out;
}

export function save_settings(s: Settings): void {
  const st = _store();
  try {
    const persisted: Settings = { ...s, auth_token: "" };
    st.setItem(KEY_SETTINGS, JSON.stringify(persisted));
  } catch {
    // ignore
  }
}

export function reset_repl_state(args?: { template?: ReplTemplate | null }, session_id?: string): ReplState {
  const now = _now_iso();
  const t = args?.template ?? null;
  const template = t && String(t.bundle_id || "").trim() && String(t.flow_id || "").trim() ? { ...t, bundle_id: String(t.bundle_id).trim(), flow_id: String(t.flow_id).trim() } : null;
  return {
    template,
    messages: [],
    created_at: now,
    updated_at: now,
  };
}

function _session_key(session_id: string): string {
  return `${KEY_SESSION_PREFIX}${String(session_id || "").trim()}`;
}

export function load_current_repl_session(): { session_id: string; state: ReplState } {
  const st = _store();
  const sid_raw = String(st.getItem(KEY_CURRENT_SESSION) || "").trim();
  const sid = sid_raw || `acode:${random_id()}`;
  const state = (() => {
    const parsed = _safe_parse<any>(st.getItem(_session_key(sid)), null);
    if (!parsed || typeof parsed !== "object") return reset_repl_state(undefined, sid);
    const msgs = Array.isArray((parsed as any).messages) ? (parsed as any).messages : [];
    const template = (parsed as any).template;
    const tpl_ok = template && typeof template === "object" && String((template as any).bundle_id || "").trim() && String((template as any).flow_id || "").trim();
    const out: ReplState = {
      template: tpl_ok ? { bundle_id: String((template as any).bundle_id).trim(), flow_id: String((template as any).flow_id).trim(), name: (template as any).name } : null,
      messages: msgs as any,
      created_at: typeof (parsed as any).created_at === "string" ? (parsed as any).created_at : _now_iso(),
      updated_at: typeof (parsed as any).updated_at === "string" ? (parsed as any).updated_at : _now_iso(),
    };
    if (!Array.isArray(out.messages)) out.messages = [];
    return out;
  })();
  try {
    st.setItem(KEY_CURRENT_SESSION, sid);
  } catch {
    // ignore
  }
  return { session_id: sid, state };
}

export function save_current_repl_session(session_id: string, repl: ReplState): void {
  const st = _store();
  const sid = String(session_id || "").trim();
  if (!sid) return;
  try {
    st.setItem(_session_key(sid), JSON.stringify(repl));
  } catch {
    // ignore
  }
}

export function switch_current_repl_session(session_id: string): { session_id: string; state: ReplState } {
  const st = _store();
  const sid = String(session_id || "").trim();
  if (!sid) return load_current_repl_session();
  try {
    st.setItem(KEY_CURRENT_SESSION, sid);
  } catch {
    // ignore
  }
  const parsed = _safe_parse<any>(st.getItem(_session_key(sid)), null);
  if (!parsed || typeof parsed !== "object") return { session_id: sid, state: reset_repl_state(undefined, sid) };
  const msgs = Array.isArray((parsed as any).messages) ? (parsed as any).messages : [];
  const template = (parsed as any).template;
  const tpl_ok = template && typeof template === "object" && String((template as any).bundle_id || "").trim() && String((template as any).flow_id || "").trim();
  const out: ReplState = {
    template: tpl_ok ? { bundle_id: String((template as any).bundle_id).trim(), flow_id: String((template as any).flow_id).trim(), name: (template as any).name } : null,
    messages: msgs as any,
    created_at: typeof (parsed as any).created_at === "string" ? (parsed as any).created_at : _now_iso(),
    updated_at: typeof (parsed as any).updated_at === "string" ? (parsed as any).updated_at : _now_iso(),
  };
  if (!Array.isArray(out.messages)) out.messages = [];
  return { session_id: sid, state: out };
}

export function create_new_repl_session(template: ReplTemplate | null): { session_id: string; state: ReplState } {
  const sid = `acode:${random_id()}`;
  const state = reset_repl_state({ template }, sid);
  const st = _store();
  try {
    st.setItem(KEY_CURRENT_SESSION, sid);
  } catch {
    // ignore
  }
  save_current_repl_session(sid, state);
  return { session_id: sid, state };
}

export function load_active_run_id(session_id: string): string {
  const sid = String(session_id || "").trim();
  if (!sid) return "";
  const st = _store();
  return String(st.getItem(`${KEY_ACTIVE_RUN_PREFIX}${sid}`) || "").trim();
}

export function save_active_run_id(session_id: string, run_id: string): void {
  const sid = String(session_id || "").trim();
  const rid = String(run_id || "").trim();
  if (!sid || !rid) return;
  const st = _store();
  try {
    st.setItem(`${KEY_ACTIVE_RUN_PREFIX}${sid}`, rid);
  } catch {
    // ignore
  }
}

export function clear_active_run_id(session_id: string): void {
  const sid = String(session_id || "").trim();
  if (!sid) return;
  const st = _store();
  try {
    st.removeItem(`${KEY_ACTIVE_RUN_PREFIX}${sid}`);
  } catch {
    // ignore
  }
}

export function load_session_tool_approve_all(session_id: string): boolean {
  const sid = String(session_id || "").trim();
  if (!sid) return false;
  const st = _store();
  const raw = String(st.getItem(`${KEY_SESSION_TOOL_APPROVE_ALL_PREFIX}${sid}`) || "").trim().toLowerCase();
  return raw === "1" || raw === "true" || raw === "yes" || raw === "on";
}

export function save_session_tool_approve_all(session_id: string, enabled: boolean): void {
  const sid = String(session_id || "").trim();
  if (!sid) return;
  const st = _store();
  const key = `${KEY_SESSION_TOOL_APPROVE_ALL_PREFIX}${sid}`;
  try {
    if (enabled) st.setItem(key, "1");
    else st.removeItem(key);
  } catch {
    // ignore
  }
}

export function load_session_workspace_root(session_id: string): string {
  const sid = String(session_id || "").trim();
  if (!sid) return "";
  const st = _store();
  return String(st.getItem(`${KEY_SESSION_WORKSPACE_ROOT_PREFIX}${sid}`) || "").trim();
}

export function save_session_workspace_root(session_id: string, workspace_root: string): void {
  const sid = String(session_id || "").trim();
  const wr = String(workspace_root || "").trim();
  if (!sid) return;
  const st = _store();
  const key = `${KEY_SESSION_WORKSPACE_ROOT_PREFIX}${sid}`;
  try {
    if (wr) st.setItem(key, wr);
    else st.removeItem(key);
  } catch {
    // ignore
  }
}

export function load_run_cursor(run_id: string): number | null {
  const rid = String(run_id || "").trim();
  if (!rid) return null;
  const st = _store();
  const raw = st.getItem(`${KEY_RUN_CURSOR_PREFIX}${rid}`);
  const n = Number(raw);
  if (!Number.isFinite(n)) return null;
  return Math.max(0, Math.trunc(n));
}

export function save_run_cursor(run_id: string, cursor: number): void {
  const rid = String(run_id || "").trim();
  if (!rid) return;
  const n = Number(cursor);
  if (!Number.isFinite(n)) return;
  const st = _store();
  try {
    st.setItem(`${KEY_RUN_CURSOR_PREFIX}${rid}`, String(Math.max(0, Math.trunc(n))));
  } catch {
    // ignore
  }
}

export function clear_run_cursor(run_id: string): void {
  const rid = String(run_id || "").trim();
  if (!rid) return;
  const st = _store();
  try {
    st.removeItem(`${KEY_RUN_CURSOR_PREFIX}${rid}`);
  } catch {
    // ignore
  }
}
