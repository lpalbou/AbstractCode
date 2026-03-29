import { LedgerStreamEvent, type AttachmentRef } from "./types";
import { SseParser } from "./sse_parser";

export type GatewayClientConfig = {
  base_url: string; // e.g. "http://localhost:8081" (no trailing slash) or "" for same-origin
  auth_token?: string;
};

function _join(base_url: string, path: string): string {
  const base = (base_url || "").trim().replace(/\/+$/, "");
  if (!base) return path;
  return `${base}${path}`;
}

function _auth_headers(token?: string): Record<string, string> {
  const t = (token || "").trim();
  if (!t) return {};
  return { Authorization: `Bearer ${t}` };
}

function _retry_after_s(resp: Response): number | undefined {
  const ra = resp.headers.get("retry-after");
  if (!ra) return undefined;
  const n = Number(ra);
  if (Number.isFinite(n) && n > 0) return n;
  return undefined;
}

async function _read_error(resp: Response): Promise<string> {
  try {
    const ct = String(resp.headers.get("content-type") || "").toLowerCase();
    if (ct.includes("application/json")) {
      const body = await resp.json().catch(() => null);
      if (body && typeof body === "object") {
        const detail = (body as any).detail;
        if (typeof detail === "string" && detail.trim()) return detail.trim();
        const msg = (body as any).error?.message;
        if (typeof msg === "string" && msg.trim()) return msg.trim();
        try {
          const s = JSON.stringify(body);
          if (s && s !== "{}") return s;
        } catch {
          // ignore
        }
      }
    }
    const text = await resp.text();
    return text?.trim() ? text.trim() : `${resp.status}`;
  } catch {
    return `${resp.status}`;
  }
}

export class GatewayHttpError extends Error {
  status: number;
  retry_after_s?: number;
  body_text?: string;

  constructor(message: string, args: { status: number; retry_after_s?: number; body_text?: string }) {
    super(message);
    this.name = "GatewayHttpError";
    this.status = Number.isFinite(Number(args.status)) ? Number(args.status) : 0;
    this.retry_after_s = args.retry_after_s;
    this.body_text = args.body_text;
  }
}

async function _throw_http(resp: Response, label: string): Promise<never> {
  const body_text = await _read_error(resp);
  throw new GatewayHttpError(`${label}: ${body_text}`, { status: resp.status, retry_after_s: _retry_after_s(resp), body_text });
}

export class GatewayClient {
  private _cfg: GatewayClientConfig;

  constructor(cfg: GatewayClientConfig) {
    this._cfg = { ...cfg, base_url: (cfg.base_url || "").trim() };
  }

  async start_run(
    flow_id: string | null | undefined,
    input_data: Record<string, any>,
    opts?: { bundle_id?: string; session_id?: string | null }
  ): Promise<string> {
    const bundle_id = String(opts?.bundle_id || "").trim();
    const session_id = opts?.session_id === null || opts?.session_id === undefined ? "" : String(opts.session_id || "").trim();
    const fid = String(flow_id || "").trim();
    const req_body: any = { input_data: input_data || {} };
    if (bundle_id) req_body.bundle_id = bundle_id;
    if (fid) req_body.flow_id = fid;
    if (session_id) req_body.session_id = session_id;
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/runs/start"), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ..._auth_headers(this._cfg.auth_token),
      },
      body: JSON.stringify(req_body),
    });
    if (!r.ok) return await _throw_http(r, "start_run failed");
    const body = await r.json();
    const run_id = body?.run_id;
    if (typeof run_id !== "string" || !run_id) throw new Error("start_run: missing run_id");
    return run_id;
  }

  async get_run(run_id: string): Promise<any> {
    const rid = String(run_id || "").trim();
    if (!rid) throw new Error("get_run: run_id is required");
    const r = await fetch(_join(this._cfg.base_url, `/api/gateway/runs/${encodeURIComponent(rid)}`), {
      headers: { ..._auth_headers(this._cfg.auth_token) },
    });
    if (!r.ok) return await _throw_http(r, "get_run failed");
    return await r.json();
  }

  async get_run_history_bundle(
    run_id: string,
    opts?: {
      include_subruns?: boolean;
      include_session?: boolean;
      session_turn_limit?: number;
      ledger_mode?: "tail" | "full";
      ledger_max_items?: number;
    }
  ): Promise<any> {
    const rid = String(run_id || "").trim();
    if (!rid) throw new Error("get_run_history_bundle: run_id is required");
    const qs = new URLSearchParams();
    if (opts?.include_subruns === false) qs.set("include_subruns", "false");
    if (opts?.include_session === true) qs.set("include_session", "true");
    if (typeof opts?.session_turn_limit === "number" && Number.isFinite(opts.session_turn_limit)) qs.set("session_turn_limit", String(Math.max(1, Math.trunc(opts.session_turn_limit))));
    if (opts?.ledger_mode) qs.set("ledger_mode", String(opts.ledger_mode));
    if (typeof opts?.ledger_max_items === "number" && Number.isFinite(opts.ledger_max_items)) qs.set("ledger_max_items", String(Math.max(0, Math.trunc(opts.ledger_max_items))));
    const url = _join(this._cfg.base_url, `/api/gateway/runs/${encodeURIComponent(rid)}/history_bundle?${qs.toString()}`);
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "get_run_history_bundle failed");
    return await r.json();
  }

  async list_runs(opts?: {
    limit?: number;
    status?: string;
    workflow_id?: string;
    session_id?: string;
    root_only?: boolean;
    include_ledger_len?: boolean;
    include_metrics?: boolean;
  }): Promise<any> {
    const qs = new URLSearchParams();
    const limit = typeof opts?.limit === "number" ? opts.limit : 50;
    qs.set("limit", String(limit));
    const status = String(opts?.status || "").trim();
    if (status) qs.set("status", status);
    const workflow_id = String(opts?.workflow_id || "").trim();
    if (workflow_id) qs.set("workflow_id", workflow_id);
    const session_id = String(opts?.session_id || "").trim();
    if (session_id) qs.set("session_id", session_id);
    if (opts?.root_only === true) qs.set("root_only", "true");
    if (opts?.include_ledger_len === false) qs.set("include_ledger_len", "false");
    if (opts?.include_metrics === true) qs.set("include_metrics", "true");
    const url = _join(this._cfg.base_url, `/api/gateway/runs?${qs.toString()}`);
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "list_runs failed");
    return await r.json();
  }

  async get_ledger(run_id: string, opts: { after: number; limit: number }): Promise<{ items: any[]; next_after: number }> {
    const rid = String(run_id || "").trim();
    if (!rid) throw new Error("get_ledger: run_id is required");
    const after = Number(opts?.after || 0);
    const limit = Number(opts?.limit || 0);
    const url = _join(
      this._cfg.base_url,
      `/api/gateway/runs/${encodeURIComponent(rid)}/ledger?after=${encodeURIComponent(String(after))}&limit=${encodeURIComponent(String(limit))}`
    );
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "get_ledger failed");
    const body = await r.json();
    const items = Array.isArray(body?.items) ? body.items : [];
    const next_after = typeof body?.next_after === "number" ? body.next_after : after;
    return { items, next_after };
  }

  async stream_ledger(run_id: string, opts: { after: number; on_step: (ev: LedgerStreamEvent) => void; signal?: AbortSignal }): Promise<void> {
    const rid = String(run_id || "").trim();
    if (!rid) throw new Error("stream_ledger: run_id is required");
    const after = Number(opts?.after || 0);
    const on_step = opts.on_step;
    const signal = opts.signal;
    const url = _join(this._cfg.base_url, `/api/gateway/runs/${encodeURIComponent(rid)}/ledger/stream?after=${encodeURIComponent(String(after))}`);
    const r = await fetch(url, {
      headers: { Accept: "text/event-stream", ..._auth_headers(this._cfg.auth_token) },
      signal,
    });
    if (!r.ok) return await _throw_http(r, "stream_ledger failed");
    if (!r.body) throw new Error("stream_ledger: response body is missing");

    const reader = r.body.getReader();
    const decoder = new TextDecoder("utf-8");
    const parser = new SseParser();

    while (true) {
      const { value, done } = await reader.read();
      if (done) return;
      const text = decoder.decode(value, { stream: true });
      parser.push(text, (ev) => {
        if (ev.event !== "step" || !ev.data) return;
        try {
          const parsed = JSON.parse(ev.data);
          if (parsed && typeof parsed.cursor === "number" && parsed.record) on_step(parsed as LedgerStreamEvent);
        } catch {
          // ignore malformed events
        }
      });
    }
  }

  async list_bundles(): Promise<any> {
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/bundles"), { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "list_bundles failed");
    return await r.json();
  }

  async reload_bundles(): Promise<any> {
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/bundles/reload"), { method: "POST", headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "reload_bundles failed");
    return await r.json();
  }

  async upload_bundle(file: File, opts?: { overwrite?: boolean; reload?: boolean }): Promise<any> {
    const overwrite = opts?.overwrite === true;
    const reload = opts?.reload !== false;
    const fd = new FormData();
    fd.set("overwrite", overwrite ? "true" : "false");
    fd.set("reload", reload ? "true" : "false");
    fd.set("file", file, file.name || "upload.flow");
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/bundles/upload"), { method: "POST", headers: { ..._auth_headers(this._cfg.auth_token) }, body: fd });
    if (!r.ok) return await _throw_http(r, "upload_bundle failed");
    return await r.json();
  }

  async discovery_tools(): Promise<any> {
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/discovery/tools"), { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "discovery_tools failed");
    return await r.json();
  }

  async discovery_providers(opts?: { include_models?: boolean }): Promise<any> {
    const include_models = opts?.include_models === true;
    const url = _join(this._cfg.base_url, `/api/gateway/discovery/providers?include_models=${encodeURIComponent(String(include_models))}`);
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "discovery_providers failed");
    return await r.json();
  }

  async discovery_provider_models(provider_name: string): Promise<any> {
    const prov = String(provider_name || "").trim();
    if (!prov) throw new Error("discovery_provider_models: provider_name is required");
    const r = await fetch(_join(this._cfg.base_url, `/api/gateway/discovery/providers/${encodeURIComponent(prov)}/models`), {
      headers: { ..._auth_headers(this._cfg.auth_token) },
    });
    if (!r.ok) return await _throw_http(r, "discovery_provider_models failed");
    return await r.json();
  }

  async discovery_model_capabilities(model_name: string): Promise<any> {
    const name = String(model_name || "").trim();
    if (!name) throw new Error("discovery_model_capabilities: model_name is required");
    const url = _join(this._cfg.base_url, `/api/gateway/discovery/models/capabilities?model_name=${encodeURIComponent(name)}`);
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "discovery_model_capabilities failed");
    return await r.json();
  }

  async workspace_policy(): Promise<any> {
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/workspace/policy"), { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "workspace_policy failed");
    return await r.json();
  }

  async files_search(
    query: string,
    opts?: {
      limit?: number;
      scope?: { workspace_root?: string; workspace_access_mode?: string; workspace_allowed_paths?: string; workspace_ignored_paths?: string };
      signal?: AbortSignal;
    }
  ): Promise<any> {
    const q = String(query || "").trim();
    const limit = typeof opts?.limit === "number" && Number.isFinite(opts.limit) ? Math.max(1, Math.trunc(opts.limit)) : 20;
    const qs = new URLSearchParams();
    qs.set("query", q);
    qs.set("limit", String(limit));
    const scope = opts?.scope;
    if (scope && typeof scope === "object") {
      const wr = String(scope.workspace_root || "").trim();
      const wm = String(scope.workspace_access_mode || "").trim();
      const wa = String(scope.workspace_allowed_paths || "").trim();
      const wi = String(scope.workspace_ignored_paths || "").trim();
      if (wr) qs.set("workspace_root", wr);
      if (wm) qs.set("workspace_access_mode", wm);
      if (wa) qs.set("workspace_allowed_paths", wa);
      if (wi) qs.set("workspace_ignored_paths", wi);
    }
    const url = _join(this._cfg.base_url, `/api/gateway/files/search?${qs.toString()}`);
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) }, signal: opts?.signal });
    if (!r.ok) return await _throw_http(r, "files_search failed");
    return await r.json();
  }

  async list_run_artifacts(run_id: string, opts?: { limit?: number }): Promise<any> {
    const rid = String(run_id || "").trim();
    if (!rid) throw new Error("list_run_artifacts: run_id is required");
    const limit = typeof opts?.limit === "number" ? opts.limit : 200;
    const url = _join(this._cfg.base_url, `/api/gateway/runs/${encodeURIComponent(rid)}/artifacts?limit=${encodeURIComponent(String(limit))}`);
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "list_run_artifacts failed");
    return await r.json();
  }

  async get_run_artifact_blob(run_id: string, artifact_id: string, opts?: { max_bytes?: number }): Promise<{ blob: Blob; content_type: string }> {
    const rid = String(run_id || "").trim();
    const aid = String(artifact_id || "").trim();
    if (!rid) throw new Error("get_run_artifact_blob: run_id is required");
    if (!aid) throw new Error("get_run_artifact_blob: artifact_id is required");
    const url = _join(this._cfg.base_url, `/api/gateway/runs/${encodeURIComponent(rid)}/artifacts/${encodeURIComponent(aid)}/content`);
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "get_run_artifact_blob failed");
    const blob = await r.blob();
    const max_bytes = typeof opts?.max_bytes === "number" && Number.isFinite(opts.max_bytes) ? Math.max(0, Math.trunc(opts.max_bytes)) : 0;
    if (max_bytes > 0 && blob.size > max_bytes) throw new Error(`Artifact too large (${blob.size} bytes > ${max_bytes} bytes)`);
    const content_type = String(r.headers.get("content-type") || "").trim() || "application/octet-stream";
    return { blob, content_type };
  }

  async get_run_artifact_text(run_id: string, artifact_id: string, opts?: { max_bytes?: number }): Promise<string> {
    const { blob } = await this.get_run_artifact_blob(run_id, artifact_id, opts);
    return await blob.text();
  }

  async attachments_ingest(
    session_id: string,
    path: string,
    opts?: { scope?: { workspace_root?: string; workspace_access_mode?: string; workspace_allowed_paths?: string; workspace_ignored_paths?: string } }
  ): Promise<AttachmentRef> {
    const sid = String(session_id || "").trim();
    const p = String(path || "").trim();
    if (!sid) throw new Error("attachments_ingest: session_id is required");
    if (!p) throw new Error("attachments_ingest: path is required");
    const body: any = { session_id: sid, path: p };
    const scope = opts?.scope;
    if (scope && typeof scope === "object") {
      const wr = String(scope.workspace_root || "").trim();
      const wm = String(scope.workspace_access_mode || "").trim();
      const wa = String(scope.workspace_allowed_paths || "").trim();
      const wi = String(scope.workspace_ignored_paths || "").trim();
      if (wr) body.workspace_root = wr;
      if (wm) body.workspace_access_mode = wm;
      if (wa) body.workspace_allowed_paths = wa;
      if (wi) body.workspace_ignored_paths = wi;
    }
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/attachments/ingest"), {
      method: "POST",
      headers: { "Content-Type": "application/json", ..._auth_headers(this._cfg.auth_token) },
      body: JSON.stringify(body),
    });
    if (!r.ok) return await _throw_http(r, "attachments_ingest failed");
    const out = await r.json();
    const attachment = out?.attachment;
    if (!attachment || typeof attachment !== "object") throw new Error("attachments_ingest: missing attachment");
    const aid = String((attachment as any).$artifact || "").trim();
    if (!aid) throw new Error("attachments_ingest: missing attachment.$artifact");
    return attachment as AttachmentRef;
  }

  async attachments_upload(session_id: string, file: File, opts?: { filename?: string; content_type?: string }): Promise<AttachmentRef> {
    const sid = String(session_id || "").trim();
    if (!sid) throw new Error("attachments_upload: session_id is required");
    if (!file) throw new Error("attachments_upload: file is required");
    const filename = String(opts?.filename || "").trim() || String((file as any)?.name || "").trim() || "upload.bin";
    const content_type = String(opts?.content_type || "").trim() || String((file as any)?.type || "").trim();

    const form = new FormData();
    form.append("session_id", sid);
    form.append("file", file, filename);
    if (filename) form.append("filename", filename);
    if (content_type) form.append("content_type", content_type);

    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/attachments/upload"), { method: "POST", headers: { ..._auth_headers(this._cfg.auth_token) }, body: form });
    if (!r.ok) return await _throw_http(r, "attachments_upload failed");
    const out = await r.json();
    const attachment = out?.attachment;
    if (!attachment || typeof attachment !== "object") throw new Error("attachments_upload: missing attachment");
    const aid = String((attachment as any).$artifact || "").trim();
    if (!aid) throw new Error("attachments_upload: missing attachment.$artifact");
    return attachment as AttachmentRef;
  }

  async audio_transcribe(run_id: string, req: { audio_artifact: AttachmentRef; request_id?: string; language?: string | null }): Promise<any> {
    const rid = String(run_id || "").trim();
    if (!rid) throw new Error("audio_transcribe: run_id is required");
    const body: any = { audio_artifact: req?.audio_artifact, request_id: req?.request_id || undefined };
    const lang = req?.language === null || req?.language === undefined ? "" : String(req.language || "").trim();
    if (lang) body.language = lang;
    const r = await fetch(_join(this._cfg.base_url, `/api/gateway/runs/${encodeURIComponent(rid)}/audio/transcribe`), {
      method: "POST",
      headers: { "Content-Type": "application/json", ..._auth_headers(this._cfg.auth_token) },
      body: JSON.stringify(body),
    });
    if (!r.ok) return await _throw_http(r, "audio_transcribe failed");
    return await r.json();
  }

  async voice_tts(run_id: string, req: { text: string; voice?: string | null; format?: string | null; request_id?: string | null }): Promise<any> {
    const rid = String(run_id || "").trim();
    if (!rid) throw new Error("voice_tts: run_id is required");
    const body: any = { text: String(req?.text || "") };
    const voice = req?.voice === null || req?.voice === undefined ? "" : String(req.voice || "").trim();
    const format = req?.format === null || req?.format === undefined ? "" : String(req.format || "").trim();
    const request_id = req?.request_id === null || req?.request_id === undefined ? "" : String(req.request_id || "").trim();
    if (voice) body.voice = voice;
    if (format) body.format = format;
    if (request_id) body.request_id = request_id;
    const r = await fetch(_join(this._cfg.base_url, `/api/gateway/runs/${encodeURIComponent(rid)}/voice/tts`), {
      method: "POST",
      headers: { "Content-Type": "application/json", ..._auth_headers(this._cfg.auth_token) },
      body: JSON.stringify(body),
    });
    if (!r.ok) return await _throw_http(r, "voice_tts failed");
    return await r.json();
  }

  async submit_command(command: { command_id: string; run_id: string; type: string; payload?: any; ts?: string; client_id?: string }): Promise<any> {
    const body: any = {
      command_id: String(command?.command_id || "").trim(),
      run_id: String(command?.run_id || "").trim(),
      type: String(command?.type || "").trim(),
      payload: command?.payload && typeof command.payload === "object" ? command.payload : {},
    };
    const ts = String(command?.ts || "").trim();
    if (ts) body.ts = ts;
    const client_id = String(command?.client_id || "").trim();
    if (client_id) body.client_id = client_id;
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/commands"), {
      method: "POST",
      headers: { "Content-Type": "application/json", ..._auth_headers(this._cfg.auth_token) },
      body: JSON.stringify(body),
    });
    if (!r.ok) return await _throw_http(r, "submit_command failed");
    return await r.json();
  }

  async bug_report_create(req: {
    session_id: string;
    description: string;
    active_run_id?: string;
    workflow_id?: string;
    client?: string;
    client_version?: string;
    user_agent?: string;
    url?: string;
    provider?: string;
    model?: string;
    template?: string;
    context?: any;
  }): Promise<any> {
    const body: any = { session_id: String(req?.session_id || "").trim(), description: String(req?.description || "") };
    const active_run_id = String(req?.active_run_id || "").trim();
    if (active_run_id) body.active_run_id = active_run_id;
    const workflow_id = String(req?.workflow_id || "").trim();
    if (workflow_id) body.workflow_id = workflow_id;
    const client = String(req?.client || "").trim();
    if (client) body.client = client;
    const client_version = String(req?.client_version || "").trim();
    if (client_version) body.client_version = client_version;
    const user_agent = String(req?.user_agent || "").trim();
    if (user_agent) body.user_agent = user_agent;
    const url0 = String(req?.url || "").trim();
    if (url0) body.url = url0;
    const provider = String(req?.provider || "").trim();
    if (provider) body.provider = provider;
    const model = String(req?.model || "").trim();
    if (model) body.model = model;
    const template = String(req?.template || "").trim();
    if (template) body.template = template;
    const context = req?.context;
    if (context && typeof context === "object") body.context = context;

    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/bugs/report"), {
      method: "POST",
      headers: { "Content-Type": "application/json", ..._auth_headers(this._cfg.auth_token) },
      body: JSON.stringify(body),
    });
    if (!r.ok) return await _throw_http(r, "bug_report_create failed");
    return await r.json();
  }

  async feature_report_create(req: {
    session_id: string;
    description: string;
    active_run_id?: string;
    workflow_id?: string;
    client?: string;
    client_version?: string;
    user_agent?: string;
    url?: string;
    provider?: string;
    model?: string;
    template?: string;
    context?: any;
  }): Promise<any> {
    const body: any = { session_id: String(req?.session_id || "").trim(), description: String(req?.description || "") };
    const active_run_id = String(req?.active_run_id || "").trim();
    if (active_run_id) body.active_run_id = active_run_id;
    const workflow_id = String(req?.workflow_id || "").trim();
    if (workflow_id) body.workflow_id = workflow_id;
    const client = String(req?.client || "").trim();
    if (client) body.client = client;
    const client_version = String(req?.client_version || "").trim();
    if (client_version) body.client_version = client_version;
    const user_agent = String(req?.user_agent || "").trim();
    if (user_agent) body.user_agent = user_agent;
    const url0 = String(req?.url || "").trim();
    if (url0) body.url = url0;
    const provider = String(req?.provider || "").trim();
    if (provider) body.provider = provider;
    const model = String(req?.model || "").trim();
    if (model) body.model = model;
    const template = String(req?.template || "").trim();
    if (template) body.template = template;
    const context = req?.context;
    if (context && typeof context === "object") body.context = context;

    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/features/report"), {
      method: "POST",
      headers: { "Content-Type": "application/json", ..._auth_headers(this._cfg.auth_token) },
      body: JSON.stringify(body),
    });
    if (!r.ok) return await _throw_http(r, "feature_report_create failed");
    return await r.json();
  }

  async prompt_cache_stats(provider: string, model: string): Promise<any> {
    const prov = String(provider || "").trim();
    const mod = String(model || "").trim();
    if (!prov || !mod) throw new Error("prompt_cache_stats: provider and model are required");
    const url = _join(this._cfg.base_url, `/api/gateway/prompt_cache/stats?provider=${encodeURIComponent(prov)}&model=${encodeURIComponent(mod)}`);
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "prompt_cache_stats failed");
    return await r.json();
  }

  async prompt_cache_capabilities(provider: string, model: string): Promise<any> {
    const prov = String(provider || "").trim();
    const mod = String(model || "").trim();
    if (!prov || !mod) throw new Error("prompt_cache_capabilities: provider and model are required");
    const url = _join(this._cfg.base_url, `/api/gateway/prompt_cache/capabilities?provider=${encodeURIComponent(prov)}&model=${encodeURIComponent(mod)}`);
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "prompt_cache_capabilities failed");
    return await r.json();
  }

  async prompt_cache_saved(provider: string, model: string): Promise<any> {
    const prov = String(provider || "").trim();
    const mod = String(model || "").trim();
    if (!prov || !mod) throw new Error("prompt_cache_saved: provider and model are required");
    const url = _join(this._cfg.base_url, `/api/gateway/prompt_cache/saved?provider=${encodeURIComponent(prov)}&model=${encodeURIComponent(mod)}`);
    const r = await fetch(url, { headers: { ..._auth_headers(this._cfg.auth_token) } });
    if (!r.ok) return await _throw_http(r, "prompt_cache_saved failed");
    return await r.json();
  }

  async prompt_cache_clear(req: { provider: string; model: string; key?: string | null }): Promise<any> {
    const body: any = { provider: String(req?.provider || "").trim(), model: String(req?.model || "").trim() };
    const key = req?.key === null || req?.key === undefined ? "" : String(req.key || "").trim();
    if (key) body.key = key;
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/prompt_cache/clear"), {
      method: "POST",
      headers: { "Content-Type": "application/json", ..._auth_headers(this._cfg.auth_token) },
      body: JSON.stringify(body),
    });
    if (!r.ok) return await _throw_http(r, "prompt_cache_clear failed");
    return await r.json();
  }

  async prompt_cache_save(req: { provider: string; model: string; name: string; key: string; q8?: boolean }): Promise<any> {
    const body: any = { provider: String(req?.provider || "").trim(), model: String(req?.model || "").trim(), name: String(req?.name || "").trim(), key: String(req?.key || "").trim(), q8: Boolean(req?.q8) };
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/prompt_cache/save"), {
      method: "POST",
      headers: { "Content-Type": "application/json", ..._auth_headers(this._cfg.auth_token) },
      body: JSON.stringify(body),
    });
    if (!r.ok) return await _throw_http(r, "prompt_cache_save failed");
    return await r.json();
  }

  async prompt_cache_load(req: { provider: string; model: string; name: string; key: string; clear_existing?: boolean }): Promise<any> {
    const body: any = { provider: String(req?.provider || "").trim(), model: String(req?.model || "").trim(), name: String(req?.name || "").trim(), key: String(req?.key || "").trim(), clear_existing: Boolean(req?.clear_existing) };
    const r = await fetch(_join(this._cfg.base_url, "/api/gateway/prompt_cache/load"), {
      method: "POST",
      headers: { "Content-Type": "application/json", ..._auth_headers(this._cfg.auth_token) },
      body: JSON.stringify(body),
    });
    if (!r.ok) return await _throw_http(r, "prompt_cache_load failed");
    return await r.json();
  }
}
