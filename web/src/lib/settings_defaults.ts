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
  if (!next.tools_initialized) {
    const tools = Array.isArray(args.discovered_tools) ? args.discovered_tools : [];
    const names: string[] = [];
    for (const t of tools) {
      const n = String((t as any)?.name || "").trim();
      if (n) names.push(n);
    }
    names.sort((a, b) => a.localeCompare(b));
    next.tools = names;
    next.tools_initialized = true;
    changed = true;
  } else if (Array.isArray(next.tools) && next.tools.length && Array.isArray(args.discovered_tools)) {
    // Best-effort: drop tools that no longer exist on the gateway.
    const allowed = new Set<string>();
    for (const t of args.discovered_tools) {
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

