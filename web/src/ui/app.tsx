import React, { useEffect, useMemo, useRef, useState } from "react";

import { GatewayClient } from "../lib/gateway_client";
import { random_id } from "../lib/ids";
import { extract_tool_calls_from_wait, extract_wait_from_record } from "../lib/runtime_extractors";
import { LedgerStreamEvent, StepRecord, ToolCall, WaitState } from "../lib/types";
import { MarkdownRenderer } from "./markdown_renderer";
import {
  create_new_repl_session,
  delete_repl_session,
  list_repl_sessions,
  load_current_repl_session,
  load_settings,
  ReplMessage,
  ReplState,
  ReplTemplate,
  ReplSessionSummary,
  reset_repl_state,
  save_current_repl_session,
  save_settings,
  Settings,
  switch_current_repl_session,
} from "../lib/storage";

type Route = { name: "console" } | { name: "new" } | { name: "sessions" } | { name: "settings" };

type AgentTemplate = {
  bundle_id: string;
  flow_id: string;
  name: string;
  description: string;
  interfaces: string[];
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
  const out = r?.result?.output?.result;
  if (!out || typeof out !== "object") return null;
  const msg = out?.response ?? out?.message ?? out?.text ?? out?.content;
  const response = String(msg ?? "").trim();
  if (!response) return null;
  return { response, meta: out?.meta ?? null };
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
            session_id={session_id}
            repl={repl}
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
            current_session_id={session_id}
            on_open={(sid) => {
              const loaded = switch_current_repl_session(sid);
              set_session({ session_id: loaded.session_id, state: loaded.state });
              set_route({ name: "console" });
            }}
            on_delete={(sid) => {
              const next = delete_repl_session(sid);
              set_session({ session_id: next.session_id, state: next.state });
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

function SessionsPage(props: {
  current_session_id: string;
  on_open: (session_id: string) => void;
  on_delete: (session_id: string) => void;
}): React.ReactElement {
  const sessions = list_repl_sessions();
  const current = String(props.current_session_id || "").trim();

  return (
    <div className="panel">
      <h2>Sessions</h2>
      <div className="muted">Local chats saved in this browser.</div>

      {!sessions.length ? <div className="muted" style={{ marginTop: 10 }}>No sessions yet.</div> : null}

      <div className="list">
        {sessions.map((s) => {
          const is_current = current && s.session_id === current;
          const label = s.template?.name || (s.template ? `${s.template.bundle_id}:${s.template.flow_id}` : "");
          return (
            <div key={s.session_id} className="list_item" style={{ display: "flex", justifyContent: "space-between", gap: 10 }}>
              <div style={{ minWidth: 0 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <div className="mono" style={{ fontSize: 13, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                    {s.title}
                  </div>
                  {is_current ? <span className="pill muted">current</span> : null}
                </div>
                <div className="muted mono" style={{ marginTop: 4 }}>
                  {label || "—"} • {new Date(s.updated_at).toLocaleString()}
                </div>
              </div>
              <div style={{ display: "flex", gap: 8, flexShrink: 0 }}>
                <button className="btn" onClick={() => props.on_open(s.session_id)} type="button">
                  Open
                </button>
                <button
                  className="btn danger"
                  onClick={() => {
                    const ok = window.confirm("Delete this session? This cannot be undone.");
                    if (!ok) return;
                    props.on_delete(s.session_id);
                  }}
                  type="button"
                >
                  Delete
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
    let stopped = false;
    const run = async () => {
      set_loading_providers(true);
      set_loading_tools(true);
      set_error_providers("");
      set_error_tools("");
      try {
        const prov_res = await props.gateway.discovery_providers();
        if (stopped) return;
        const items = Array.isArray(prov_res?.items) ? prov_res.items : [];
        set_providers(items);
      } catch (e: any) {
        if (stopped) return;
        set_error_providers(String(e?.message || e || "Failed to load providers"));
        set_providers([]);
      } finally {
        if (!stopped) set_loading_providers(false);
      }
      try {
        const tool_res = await props.gateway.discovery_tools();
        if (stopped) return;
        const items = Array.isArray(tool_res?.items) ? tool_res.items : [];
        set_tools(items);
      } catch (e: any) {
        if (stopped) return;
        set_error_tools(String(e?.message || e || "Failed to load tools"));
        set_tools([]);
      } finally {
        if (!stopped) set_loading_tools(false);
      }
    };
    run();
    return () => {
      stopped = true;
    };
  }, [props.gateway]);

  // Provider → models.
  useEffect(() => {
    let stopped = false;
    const prov = String(s.provider || "").trim();
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
  }, [props.gateway, s.provider]);

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

  const tool_groups = useMemo(() => {
    const groups = new Map<string, { toolset: string; items: { name: string; description: string }[] }>();
    for (const t of tools) {
      const name = String((t as any)?.name || "").trim();
      if (!name) continue;
      const toolset = String((t as any)?.toolset || "other").trim() || "other";
      const description = String((t as any)?.description || "").trim();
      if (!groups.has(toolset)) groups.set(toolset, { toolset, items: [] });
      groups.get(toolset)!.items.push({ name, description });
    }
    const out = Array.from(groups.values());
    out.forEach((g) => g.items.sort((a, b) => a.name.localeCompare(b.name)));
    out.sort((a, b) => a.toolset.localeCompare(b.toolset));
    return out;
  }, [tools]);

  return (
    <div className="panel">
      <h2>Gateway</h2>
      <div className="field">
        <label>Gateway URL</label>
        <input value={s.gateway_url} onChange={(e) => props.on_change({ ...s, gateway_url: e.target.value })} placeholder="http://127.0.0.1:8080" />
      </div>
      <div className="field">
        <label>Auth token</label>
        <input value={s.auth_token} onChange={(e) => props.on_change({ ...s, auth_token: e.target.value })} placeholder="Bearer token (optional)" />
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
          disabled={loading_providers || !provider_options.length}
        >
          {!provider_options.length ? <option value="">(no providers)</option> : null}
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
        <select className="mono" value={s.model} onChange={(e) => props.on_change({ ...s, model: e.target.value })} disabled={!s.provider || loading_models || !models.length}>
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
        <select
          className="mono"
          multiple
          size={Math.min(10, Math.max(5, tools.length ? 10 : 5))}
          value={s.tools}
          onChange={(e) => {
            const selected = Array.from(e.target.selectedOptions).map((o) => String(o.value));
            props.on_change({ ...s, tools: selected, tools_initialized: true });
          }}
          disabled={loading_tools || !tools.length}
        >
          {!tools.length ? <option value="">(no tools)</option> : null}
          {tool_groups.map((g) => (
            <optgroup key={g.toolset} label={g.toolset}>
              {g.items.map((it) => (
                <option key={it.name} value={it.name} title={it.description}>
                  {it.name}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
        <div className="actions">
          <button
            className="btn"
            type="button"
            disabled={!tools.length}
            onClick={() => props.on_change({ ...s, tools: tool_groups.flatMap((g) => g.items.map((it) => it.name)), tools_initialized: true })}
          >
            Select all
          </button>
          <button className="btn" type="button" disabled={!tools.length} onClick={() => props.on_change({ ...s, tools: [], tools_initialized: true })}>
            Select none
          </button>
        </div>
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
  session_id: string;
  repl: ReplState;
  on_repl: (session_id: string, updater: (prev: ReplState) => ReplState) => void;
}): React.ReactElement {
  const [templates, set_templates] = useState<AgentTemplate[]>([]);
  const [template_error, set_template_error] = useState("");

  const [composer, set_composer] = useState("");
  const [error, set_error] = useState("");

  const [active_run_id, set_active_run_id] = useState<string | null>(null);
  const [records, set_records] = useState<LedgerStreamEvent[]>([]);
  const [status_text, set_status_text] = useState<string>("");
  const status_timer_ref = useRef<number | null>(null);

  const [details_open, set_details_open] = useState(false);
  const [resuming, set_resuming] = useState(false);

  const abort_ref = useRef<AbortController | null>(null);
  const cursor_ref = useRef<number>(0);
  const seen_step_ids_ref = useRef<Set<string>>(new Set());
  const seen_wait_keys_ref = useRef<Set<string>>(new Set());

  const last_record: StepRecord | null = records.length ? records[records.length - 1].record : null;
  const wait_state: WaitState | null = useMemo(() => extract_wait_from_record(last_record), [last_record]);
  const tool_calls_for_wait: ToolCall[] = useMemo(() => extract_tool_calls_from_wait(wait_state), [wait_state]);
  const wait_reason = String(wait_state?.reason || "").trim();
  const wait_key = String(wait_state?.wait_key || "").trim();
  const wait_event_name = wait_reason === "event" ? normalize_ui_event_name(event_name_from_wait_key(wait_key)) : "";
  const is_user_wait = wait_reason === "user";
  const is_ask_event_wait = wait_reason === "event" && wait_event_name === "abstract.ask";
  const can_user_answer_wait = is_user_wait || is_ask_event_wait;

  const repl_template = props.repl.template;
  const template_label = repl_template?.name || (repl_template ? `${repl_template.bundle_id}:${repl_template.flow_id}` : "");

  function update_repl(updater: (prev: ReplState) => ReplState): void {
    props.on_repl(props.session_id, updater);
  }

  useEffect(() => {
    let stopped = false;
    const run = async () => {
      set_template_error("");
      try {
        const items = await list_agent_templates(props.gateway);
        if (stopped) return;
        set_templates(items);
        // Auto-select a default template if none is selected yet.
        if (!props.repl.template) {
          const def = items.find((t) => t.bundle_id === "basic-agent") || items[0] || null;
          if (def) {
            update_repl((prev) => ({
              ...prev,
              template: { bundle_id: def.bundle_id, flow_id: def.flow_id, name: def.name },
              updated_at: now_iso(),
            }));
          }
        } else {
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

  function build_input_data(request: string): Record<string, any> {
    const tools = parse_tools_allowlist(props.settings.tools);
    const ctx_messages = (props.repl.messages || [])
      .filter((m) => m.role === "user" || m.role === "assistant")
      .map((m) => ({ role: m.role, content: m.content }));
    return {
      request,
      provider: props.settings.provider || null,
      model: props.settings.model || null,
      tools: tools.length ? tools : null,
      context: { messages: ctx_messages },
      max_iterations: Number.isFinite(Number(props.settings.max_iterations)) ? Number(props.settings.max_iterations) : 20,
      temperature: Number.isFinite(Number(props.settings.temperature)) ? Number(props.settings.temperature) : 0.7,
      seed: Number.isFinite(Number(props.settings.seed)) ? Number(props.settings.seed) : -1,
    };
  }

  async function start_turn(text: string): Promise<void> {
    const t = String(text || "").trim();
    if (!t) return;
    if (active_run_id) return;
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
    set_records([]);
    cursor_ref.current = 0;
    seen_step_ids_ref.current = new Set();
    seen_wait_keys_ref.current = new Set();

    append_message({ role: "user", content: t, ts: now_iso() });

    set_status("working…", -1);
    const input_data = build_input_data(t);
    try {
      const run_id = await props.gateway.start_run(props.repl.template.flow_id, input_data, { bundle_id: props.repl.template.bundle_id });
      set_active_run_id(run_id);
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
    append_message({ role: "assistant", content: resp.response, ts: now_iso(), meta: resp.meta, run_id });
    clear_status();
    stop_stream();
    set_active_run_id(null);
  }

  function handle_record(ev: LedgerStreamEvent): void {
    const rec = ev.record as StepRecord;
    const step_id = String((rec as any)?.step_id || "").trim();
    if (step_id && seen_step_ids_ref.current.has(step_id)) return;
    if (step_id) seen_step_ids_ref.current.add(step_id);
    cursor_ref.current = Math.max(cursor_ref.current, ev.cursor);
    set_records((prev) => [...prev, ev].slice(-2000));

    const emit = extract_emit_event(rec);
    if (emit && is_abstract_status(emit.name)) {
      const { text, duration_s } = parse_status_payload(emit.payload);
      set_status(text, duration_s);
    }
    if (emit && is_abstract_message(emit.name)) {
      const parsed = parse_message_payload(emit.payload);
      if (parsed) {
        clear_status(); // UX-only; keep spinner/status from getting "stuck"
        append_message({ role: "system", content: parsed.text, ts: now_iso(), level: parsed.level, title: parsed.title });
      }
    }
    if (emit && is_abstract_tool_execution(emit.name)) {
      const items = Array.isArray(emit.payload) ? emit.payload : emit.payload != null ? [emit.payload] : [];
      for (const it of items.slice(0, 30)) {
        const tool = String((it as any)?.tool || (it as any)?.name || "").trim();
        clear_status(); // UX-only
        append_message({
          role: "system",
          level: "info",
          title: tool ? `Tool call: ${tool}` : "Tool call",
          content: json_fenced(it),
          ts: now_iso(),
        });
      }
    }
    if (emit && is_abstract_tool_result(emit.name)) {
      const items = Array.isArray(emit.payload) ? emit.payload : emit.payload != null ? [emit.payload] : [];
      for (const it of items.slice(0, 30)) {
        const tool = String((it as any)?.tool || (it as any)?.name || "").trim();
        clear_status(); // UX-only
        append_message({
          role: "system",
          level: "info",
          title: tool ? `Tool result: ${tool}` : "Tool result",
          content: json_fenced(it),
          ts: now_iso(),
        });
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

    const st = String(rec?.status || "").trim();
    if (st === "failed" && active_run_id) {
      const err = String((rec as any)?.error || (rec as any)?.result?.error || "step failed").trim();
      append_message({ role: "assistant", content: `Error: ${err}`, ts: now_iso(), run_id: active_run_id });
      clear_status();
      stop_stream();
      set_active_run_id(null);
    }
  }

  useEffect(() => {
    const rid = String(active_run_id || "").trim();
    if (!rid) return;

    let stopped = false;
    set_error("");
    clear_status();
    set_records([]);
    cursor_ref.current = 0;
    seen_step_ids_ref.current = new Set();

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
        set_status("working…", -1);
        await append_page(0);
        if (stopped) return;
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

  const can_send = !active_run_id && !resuming;

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
                  <div className="warn">This run is waiting on a subworkflow. (Web host does not auto-attach to child runs yet.)</div>
                ) : null}

                {tool_calls_for_wait.length ? (
                  <>
                    <div className="field">
                      <label>tool_calls</label>
                      <textarea className="mono" readOnly value={safe_json(tool_calls_for_wait)} rows={8} />
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
              <div className="muted">Running…</div>
            )}
          </div>
        ) : null}

        <div className="repl_composer">
          <input
            className="mono"
            value={composer}
            onChange={(e) => set_composer(e.target.value)}
            placeholder={can_send ? "Type a message…" : "Waiting for the current run…"}
            disabled={!can_send}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                const v = composer;
                set_composer("");
                start_turn(v);
              }
            }}
          />
          <button
            className="btn primary"
            disabled={!can_send || !composer.trim()}
            onClick={() => {
              const v = composer;
              set_composer("");
              start_turn(v);
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

      {status_text ? <FooterStatus text={status_text} active={Boolean(active_run_id)} /> : null}
    </div>
  );
}

function ChatMessageCard(props: { m: ReplMessage }): React.ReactElement {
  const m = props.m;
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
      <div className="body markdown">
        <MarkdownRenderer markdown={m.content} />
      </div>
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

function FooterStatus(props: { text: string; active: boolean }): React.ReactElement {
  const text = String(props.text || "").trim();
  if (!text) return <></>;
  return (
    <div className={`footer_status ${props.active ? "active" : ""}`}>
      <div className="mono">{text}</div>
    </div>
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
