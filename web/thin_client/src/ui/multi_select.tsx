import React, { useMemo, useState } from "react";

function uniq_sorted(arr: string[]): string[] {
  const out = Array.from(new Set(arr.map((x) => String(x || "").trim()).filter(Boolean)));
  out.sort((a, b) => a.localeCompare(b));
  return out;
}

export function MultiSelect(props: {
  options: string[];
  value: string[];
  disabled?: boolean;
  placeholder?: string;
  max_list_height_px?: number;
  onChange: (next: string[]) => void;
}): React.ReactElement {
  const disabled = props.disabled === true;
  const options = useMemo(() => uniq_sorted(props.options || []), [props.options]);
  const value = useMemo(() => uniq_sorted(props.value || []), [props.value]);

  const [open, set_open] = useState(false);
  const [query, set_query] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter((x) => x.toLowerCase().includes(q));
  }, [options, query]);

  const selected = useMemo(() => new Set(value), [value]);

  const max_h = typeof props.max_list_height_px === "number" ? props.max_list_height_px : 220;
  const summary =
    value.length > 0 ? `${value.length} selected: ${value.slice(0, 3).join(", ")}${value.length > 3 ? "…" : ""}` : props.placeholder || "(none)";

  return (
    <div className="multi_select">
      <div className="multi_select_row">
        <button
          className="btn"
          type="button"
          onClick={() => set_open((v) => !v)}
          disabled={disabled || !options.length}
          aria-expanded={open}
        >
          {open ? "Hide" : "Select"} • {summary}
        </button>
        <button
          className="btn"
          type="button"
          onClick={() => props.onChange([])}
          disabled={disabled || value.length === 0}
          title="Clear selection"
        >
          Clear
        </button>
      </div>

      {open ? (
        <div className="multi_select_panel">
          <input
            className="mono"
            value={query}
            onChange={(e) => set_query(String(e.target.value || ""))}
            placeholder="filter…"
            disabled={disabled}
          />
          <div className="multi_select_list" style={{ maxHeight: `${max_h}px` }}>
            {filtered.map((opt) => {
              const checked = selected.has(opt);
              return (
                <label key={opt} className={`multi_select_item mono ${checked ? "checked" : ""}`}>
                  <input
                    type="checkbox"
                    checked={checked}
                    disabled={disabled}
                    onChange={(e) => {
                      const next = new Set(selected);
                      if (e.target.checked) next.add(opt);
                      else next.delete(opt);
                      props.onChange(Array.from(next.values()));
                    }}
                  />
                  <span>{opt}</span>
                </label>
              );
            })}
            {!filtered.length ? <div className="mono muted">(no matches)</div> : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}

