import type { AttachmentRef } from "./types";
import type { ReplMessage, ReplTemplate, Settings } from "./storage";

type AttachedFile = {
  path: string;
  attachment: AttachmentRef | null;
  loading: boolean;
  error?: string;
  size_bytes?: number;
};

function _hash_hex24(text: string): string {
  // Non-crypto, deterministic: 3x 32-bit hashes -> 24 hex chars.
  let h1 = 0x811c9dc5 >>> 0; // FNV-1a basis
  let h2 = 0x9e3779b1 >>> 0;
  let h3 = 0x85ebca6b >>> 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i) >>> 0;
    h1 ^= c;
    h1 = Math.imul(h1, 0x01000193) >>> 0;
    h2 = (h2 + c + ((h2 << 10) >>> 0)) >>> 0;
    h2 ^= h2 >>> 6;
    h3 ^= c + 0x9e3779b9 + ((h3 << 6) >>> 0) + (h3 >>> 2);
    h3 >>>= 0;
  }
  const a = h1.toString(16).padStart(8, "0");
  const b = h2.toString(16).padStart(8, "0");
  const c = h3.toString(16).padStart(8, "0");
  return `${a}${b}${c}`.slice(0, 24);
}

export function derive_prompt_cache_key(args: {
  namespace: string;
  session_id: string;
  provider: string;
  model: string;
  template: ReplTemplate;
  version?: number;
}): string {
  const ns = String(args?.namespace || "").trim() || "session";
  const sid = String(args?.session_id || "").trim();
  const provider = String(args?.provider || "").trim().toLowerCase();
  const model = String(args?.model || "").trim();
  const bundle_id = String(args?.template?.bundle_id || "").trim();
  const flow_id = String(args?.template?.flow_id || "").trim();
  const ver = Number.isFinite(Number(args?.version)) ? Math.trunc(Number(args.version)) : 1;
  if (!sid || !provider || !model || !bundle_id || !flow_id) return "";
  const raw = `v${ver}|${sid}|${provider}|${model}|${bundle_id}|${flow_id}`;
  return `${ns}:${_hash_hex24(raw)}`;
}

function _to_chat_messages(repl_messages: ReplMessage[], keep: number): Array<{ role: string; content: string }> {
  const msgs = Array.isArray(repl_messages) ? repl_messages : [];
  const out: Array<{ role: string; content: string }> = [];
  for (const m of msgs) {
    const role = String((m as any)?.role || "").trim();
    if (role !== "user" && role !== "assistant" && role !== "system") continue;
    const content = String((m as any)?.content || "");
    if (!content.trim()) continue;
    out.push({ role, content });
  }
  if (keep <= 0) return out;
  return out.slice(-keep);
}

export function build_run_input_data(args: {
  prompt: string;
  settings: Settings;
  repl_messages: ReplMessage[];
  session_id: string;
  attached_files: AttachedFile[];
  template: ReplTemplate | null;
}): Record<string, any> {
  const prompt = String(args?.prompt || "");
  const s = args.settings;
  const provider = String(s?.provider || "").trim();
  const model = String(s?.model || "").trim();
  const system = String(s?.system || "");
  const session_id = String(args?.session_id || "").trim();

  const attachments: AttachmentRef[] = [];
  for (const f of Array.isArray(args?.attached_files) ? args.attached_files : []) {
    if (!f || typeof f !== "object") continue;
    if (f.loading) continue;
    if (String((f as any).error || "").trim()) continue;
    const a = (f as any).attachment;
    if (!a || typeof a !== "object") continue;
    const aid = String((a as any).$artifact || "").trim();
    if (!aid) continue;
    attachments.push({ ...(a as any) });
  }

  const use_context = Boolean((s as any)?.use_context);
  const max_history = 200; // local transcript is already bounded in UI
  const messages = use_context ? _to_chat_messages(args.repl_messages || [], max_history) : [];

  const ctx: any = { task: prompt, messages };
  if (attachments.length) {
    ctx.attachments = attachments;
    ctx.media = attachments;
  }

  const runtime_ns: any = {
    provider: provider || undefined,
    model: model || undefined,
    temperature: (s as any)?.temperature,
    seed: (s as any)?.seed,
  };

  // Tool allowlist (explicit means no tools). When uninitialized, omit so flows can use their defaults.
  if (Boolean((s as any)?.tools_initialized)) {
    runtime_ns.allowed_tools = Array.isArray((s as any)?.tools) ? (s as any).tools.map((x: any) => String(x || "").trim()).filter(Boolean) : [];
  }

  // Prompt caching (provider-dependent) is injected by AbstractRuntime from `_runtime.prompt_cache`.
  if (Boolean((s as any)?.prompt_cache) && session_id && provider && model && args.template) {
    const key = derive_prompt_cache_key({ namespace: "acode", session_id, provider, model, template: args.template });
    runtime_ns.prompt_cache = key ? { enabled: true, key } : { enabled: true };
  }

  const out: Record<string, any> = {
    prompt,
    context: ctx,
    use_context,
    provider,
    model,
    system,
    _runtime: runtime_ns,
    // Keep basic knobs at top-level for VisualFlow pins.
    max_iterations: Number.isFinite(Number((s as any)?.max_iterations)) ? Math.max(1, Math.trunc(Number((s as any).max_iterations))) : 20,
    temperature: Number.isFinite(Number((s as any)?.temperature)) ? Number((s as any).temperature) : 0.7,
    seed: Number.isFinite(Number((s as any)?.seed)) ? Math.trunc(Number((s as any).seed)) : -1,
  };

  if (Boolean((s as any)?.tools_initialized)) {
    out.tools = runtime_ns.allowed_tools;
  }

  const max_in_tokens = Number((s as any)?.max_in_tokens || 0);
  if (Number.isFinite(max_in_tokens) && max_in_tokens > 0) out.max_in_tokens = Math.max(0, Math.trunc(max_in_tokens));

  const resp_schema_raw = String((s as any)?.resp_schema || "").trim();
  if (resp_schema_raw) {
    try {
      const parsed = JSON.parse(resp_schema_raw);
      if (parsed && typeof parsed === "object") out.resp_schema = parsed;
    } catch {
      // ignore (schema is optional)
    }
  }

  // Attachments are also promoted to top-level (gateway normalizes into context.attachments).
  if (attachments.length) out.attachments = attachments;

  return out;
}

