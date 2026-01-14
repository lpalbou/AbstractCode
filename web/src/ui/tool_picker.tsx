import React, { useMemo, useState } from "react";

type ToolSpec = { name?: string; toolset?: string; description?: string };

function uniq_sorted(arr: string[]): string[] {
  const out = Array.from(new Set(arr.map((x) => String(x || "").trim()).filter(Boolean)));
  out.sort((a, b) => a.localeCompare(b));
  return out;
}

function toolset_label(toolset: string): string {
  const t = String(toolset || "").trim().toLowerCase();
  if (!t) return "Other";
  if (t === "files") return "Files";
  if (t === "web") return "Web";
  if (t === "system") return "System";
  return t.charAt(0).toUpperCase() + t.slice(1);
}

export function ToolPicker(props: {
  tools: ToolSpec[];
  selected: string[];
  disabled?: boolean;
  onChange: (next: string[]) => void;
}): React.ReactElement {
  const disabled = props.disabled === true;
  const [query, set_query] = useState("");

  const by_name = useMemo(() => {
    const m = new Map<string, ToolSpec>();
    for (const t of props.tools || []) {
      const name = String((t as any)?.name || "").trim();
      if (!name) continue;
      if (!m.has(name)) m.set(name, t);
    }
    return m;
  }, [props.tools]);

  const all_names = useMemo(() => uniq_sorted(Array.from(by_name.keys())), [by_name]);
  const selected = useMemo(() => new Set(uniq_sorted(props.selected || [])), [props.selected]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return all_names;
    return all_names.filter((n) => n.toLowerCase().includes(q));
  }, [all_names, query]);

  const grouped = useMemo(() => {
    const groups = new Map<string, string[]>();
    for (const name of filtered) {
      const spec = by_name.get(name);
      const toolset = String((spec as any)?.toolset || "").trim().toLowerCase() || "other";
      if (!groups.has(toolset)) groups.set(toolset, []);
      groups.get(toolset)!.push(name);
    }
    for (const [k, names] of groups.entries()) {
      names.sort((a, b) => a.localeCompare(b));
      groups.set(k, names);
    }
    const order = ["files", "web", "system", "other"];
    const sorted_keys = Array.from(groups.keys()).sort((a, b) => {
      const ia = order.indexOf(a);
      const ib = order.indexOf(b);
      const ra = ia === -1 ? 999 : ia;
      const rb = ib === -1 ? 999 : ib;
      if (ra !== rb) return ra - rb;
      return a.localeCompare(b);
    });
    return sorted_keys.map((k) => ({ key: k, label: toolset_label(k), names: groups.get(k) || [] }));
  }, [by_name, filtered]);

  return (
    <div className="tool_picker">
      <div className="tool_picker_top">
        <input
          className="tool_picker_filter mono"
          value={query}
          onChange={(e) => set_query(String(e.target.value || ""))}
          placeholder="Filter tools…"
          disabled={disabled}
        />
      </div>

      <div className="tool_picker_list" aria-label="Tool allowlist">
        {!all_names.length ? <div className="muted mono">(no tools)</div> : null}
        {all_names.length && !filtered.length ? <div className="muted mono">(no matches)</div> : null}
        {grouped.map((g) => (
          <div key={g.key} className="tool_group">
            <div className="tool_group_header">{g.label}</div>
            <div className="tool_group_list">
              {g.names.map((name) => {
                const checked = selected.has(name);
                const spec = by_name.get(name);
                const desc = String((spec as any)?.description || "").trim();
                return (
                  <label key={name} className={`tool_row ${checked ? "checked" : ""}`} data-toolset={g.key}>
                    <input
                      type="checkbox"
                      checked={checked}
                      disabled={disabled}
                      onChange={(e) => {
                        const next = new Set(selected);
                        if (e.target.checked) next.add(name);
                        else next.delete(name);
                        props.onChange(Array.from(next.values()));
                      }}
                    />
                    <span className="mono tool_name">{name}</span>
                    {desc ? <span className="muted tool_desc">{desc}</span> : null}
                  </label>
                );
              })}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

