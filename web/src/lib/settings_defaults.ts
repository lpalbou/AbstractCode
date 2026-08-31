import type { Settings } from "./storage";

export function compute_settings_on_gateway_connect(args: {
  current: Settings;
  discovered_providers: any[];
  discovered_tools?: any[];
  default_provider?: string;
  default_model?: string;
}): { next: Settings; changed: boolean } {
  const cur = args.current;
  const next: Settings = { ...cur };
  let changed = false;

  // Mark as previously connected so auto-connect can work on reload.
  if (!next.gateway_was_connected) {
    next.gateway_was_connected = true;
    changed = true;
  }

  // Ensure a stable client id.
  if (!String(next.client_id || "").trim()) {
    next.client_id = "abstractcode_web";
    changed = true;
  }

  const providers = Array.isArray(args.discovered_providers) ? args.discovered_providers : [];
  const provider_names = new Set<string>();
  for (const p of providers) {
    const name = String((p as any)?.name || "").trim();
    if (name) provider_names.add(name);
  }

  const def_provider = String(args.default_provider || "").trim();
  const pick_provider = () => {
    if (String(next.provider || "").trim() && provider_names.has(String(next.provider || "").trim())) return String(next.provider || "").trim();
    if (def_provider && provider_names.has(def_provider)) return def_provider;
    const first = providers.length ? String((providers[0] as any)?.name || "").trim() : "";
    return first || String(next.provider || "").trim();
  };

  const provider_final = pick_provider();
  if (provider_final && provider_final !== String(next.provider || "").trim()) {
    next.provider = provider_final;
    changed = true;
  }

  const def_model = String(args.default_model || "").trim();
  if (!String(next.model || "").trim() && def_model) {
    next.model = def_model;
    changed = true;
  }

  // Tools: default to all available tools on first connect (user can later restrict).
  //
  // The gateway's /discovery/tools now returns the FULL catalog including
  // env-gated toolsets (email/telegram/whatsapp/agora/persistent-shell) as
  // `enabled:false` rows with real specs (gateway ship c4562/c4573). Those
  // rows are DISCOVERABLE so users can see what exists, but seeding them into
  // the run allowlist would offer a tool the operator never enabled — and the
  // gateway deliberately clamps disabled rows to approval `ask`, never auto.
  // Seed and prune only ENABLED tools; a row is enabled unless it explicitly
  // carries enabled === false (older gateways omit the field → treated as
  // enabled, byte-identical to prior behavior).
  const is_enabled = (t: any): boolean => (t as any)?.enabled !== false;
  if (!next.tools_initialized) {
    const tools = Array.isArray(args.discovered_tools) ? args.discovered_tools : [];
    const names: string[] = [];
    for (const t of tools) {
      if (!is_enabled(t)) continue;
      const n = String((t as any)?.name || "").trim();
      if (n) names.push(n);
    }
    names.sort((a, b) => a.localeCompare(b));
    next.tools = names;
    next.tools_initialized = true;
    changed = true;
  } else if (Array.isArray(next.tools) && next.tools.length && Array.isArray(args.discovered_tools)) {
    // Best-effort: drop tools that no longer exist OR are now disabled on the
    // gateway (a toolset the operator turned off must leave the allowlist).
    const allowed = new Set<string>();
    for (const t of args.discovered_tools) {
      if (!is_enabled(t)) continue;
      const n = String((t as any)?.name || "").trim();
      if (n) allowed.add(n);
    }
    const filtered = next.tools.map((x) => String(x || "").trim()).filter((n) => n && allowed.has(n));
    if (filtered.length !== next.tools.length) {
      next.tools = filtered;
      changed = true;
    }
  }

  return { next, changed };
}

