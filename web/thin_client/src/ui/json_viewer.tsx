import React, { useEffect, useMemo, useState } from "react";

type CollapsedMap = Record<string, boolean>;
type ExpandedStringMap = Record<string, boolean>;

function is_plain_object(v: unknown): v is Record<string, unknown> {
  return Boolean(v && typeof v === "object" && !Array.isArray(v));
}

function type_summary(v: unknown): string {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  if (Array.isArray(v)) return `Array(${v.length})`;
  if (is_plain_object(v)) return `Object(${Object.keys(v).length})`;
  return typeof v;
}

function build_default_collapsed(value: unknown, opts: { depth_threshold: number; min_items: number }): CollapsedMap {
  const out: CollapsedMap = {};
  const visit = (v: unknown, path: string, depth: number) => {
    if (Array.isArray(v)) {
      if (depth >= opts.depth_threshold && v.length >= opts.min_items) out[path] = true;
      v.forEach((it, idx) => visit(it, `${path}[${idx}]`, depth + 1));
      return;
    }
    if (is_plain_object(v)) {
      const keys = Object.keys(v);
      if (depth >= opts.depth_threshold && keys.length >= opts.min_items) out[path] = true;
      keys.forEach((k) => visit((v as Record<string, unknown>)[k], `${path}.${k}`, depth + 1));
    }
  };
  visit(value, "$", 0);
  return out;
}

export function JsonViewer(props: { value: unknown; max_string_len?: number }): React.ReactElement {
  const max_string_len = typeof props.max_string_len === "number" ? props.max_string_len : 240;
  const root = useMemo(() => props.value, [props.value]);

  const [collapsed, set_collapsed] = useState<CollapsedMap>(() => build_default_collapsed(root, { depth_threshold: 2, min_items: 6 }));
  const [expanded_string, set_expanded_string] = useState<ExpandedStringMap>({});

  useEffect(() => {
    set_collapsed(build_default_collapsed(root, { depth_threshold: 2, min_items: 6 }));
    set_expanded_string({});
  }, [root]);

  const toggle_collapse = (path: string) => set_collapsed((prev) => ({ ...prev, [path]: !prev[path] }));
  const toggle_string = (path: string) => set_expanded_string((prev) => ({ ...prev, [path]: !prev[path] }));

  return (
    <div className="json_viewer">
      {render_any({
        value: root,
        path: "$",
        depth: 0,
        is_last: true,
        collapsed,
        expanded_string,
        toggle_collapse,
        toggle_string,
        max_string_len,
      })}
    </div>
  );
}

function render_any(args: {
  value: unknown;
  path: string;
  depth: number;
  is_last: boolean;
  collapsed: CollapsedMap;
  expanded_string: ExpandedStringMap;
  toggle_collapse: (path: string) => void;
  toggle_string: (path: string) => void;
  max_string_len: number;
  prefix?: React.ReactNode;
}): React.ReactNode {
  const v = args.value;
  const prefix = args.prefix || null;

  if (v === null) return render_line(args.depth, prefix, <span className="json_null">null</span>, args.is_last);
  if (v === undefined) return render_line(args.depth, prefix, <span className="json_null">undefined</span>, args.is_last);

  if (typeof v === "string") return render_line(args.depth, prefix, render_string(v, args), args.is_last);
  if (typeof v === "number") return render_line(args.depth, prefix, <span className="json_number">{String(v)}</span>, args.is_last);
  if (typeof v === "boolean") return render_line(args.depth, prefix, <span className="json_bool">{String(v)}</span>, args.is_last);

  if (Array.isArray(v)) return render_array(v, args);
  if (is_plain_object(v)) return render_object(v, args);

  // Fallback (non-JSON-safe)
  return render_line(args.depth, prefix, <span className="json_string">{JSON.stringify(String(v))}</span>, args.is_last);
}

function render_string(value: string, ctx: { path: string; expanded_string: ExpandedStringMap; toggle_string: (path: string) => void; max_string_len: number }): React.ReactNode {
  const is_expanded = ctx.expanded_string[ctx.path] === true;
  const too_long = value.length > ctx.max_string_len;
  const shown = too_long && !is_expanded ? value.slice(0, ctx.max_string_len) : value;
  const json = JSON.stringify(shown);
  return (
    <>
      <span className="json_string">{json}</span>
      {too_long ? (
        <>
          {!is_expanded ? <span className="json_ellipsis">…</span> : null}
          <button className="json_toggle json_toggle_inline" onClick={() => ctx.toggle_string(ctx.path)} title={is_expanded ? "Collapse string" : "Expand string"}>
            {is_expanded ? "−" : "+"}
          </button>
          <span className="json_hint">{value.length} chars</span>
        </>
      ) : null}
    </>
  );
}

function render_array(arr: unknown[], args: Parameters<typeof render_any>[0]): React.ReactNode {
  const is_collapsed = args.collapsed[args.path] === true;
  const count = arr.length;

  if (is_collapsed) {
    return render_line(
      args.depth,
      args.prefix,
      <>
        <button className="json_toggle" onClick={() => args.toggle_collapse(args.path)} title="Expand">
          ▶
        </button>
        <span className="json_brace">[</span>
        <span className="json_hint">{type_summary(arr)}</span>
        <span className="json_brace">]</span>
        <span className="json_hint"> {count}</span>
      </>,
      args.is_last
    );
  }

  return (
    <>
      {render_line(
        args.depth,
        args.prefix,
        <>
          <button className="json_toggle" onClick={() => args.toggle_collapse(args.path)} title="Collapse">
            ▼
          </button>
          <span className="json_brace">[</span>
        </>,
        true
      )}
      {arr.map((item, idx) =>
        render_any({
          ...args,
          value: item,
          path: `${args.path}[${idx}]`,
          depth: args.depth + 1,
          is_last: idx === arr.length - 1,
          prefix: null,
        })
      )}
      {render_line(args.depth, null, <span className="json_brace">]</span>, args.is_last)}
    </>
  );
}

function render_object(obj: Record<string, unknown>, args: Parameters<typeof render_any>[0]): React.ReactNode {
  const is_collapsed = args.collapsed[args.path] === true;
  const keys = Object.keys(obj);
  const count = keys.length;

  if (is_collapsed) {
    return render_line(
      args.depth,
      args.prefix,
      <>
        <button className="json_toggle" onClick={() => args.toggle_collapse(args.path)} title="Expand">
          ▶
        </button>
        <span className="json_brace">{"{"}</span>
        <span className="json_hint">{type_summary(obj)}</span>
        <span className="json_brace">{"}"}</span>
        <span className="json_hint"> {count}</span>
      </>,
      args.is_last
    );
  }

  return (
    <>
      {render_line(
        args.depth,
        args.prefix,
        <>
          <button className="json_toggle" onClick={() => args.toggle_collapse(args.path)} title="Collapse">
            ▼
          </button>
          <span className="json_brace">{"{"}</span>
        </>,
        true
      )}
      {keys.map((k, idx) => {
        const child_path = `${args.path}.${k}`;
        const is_last = idx === keys.length - 1;
        const v = obj[k];
        const prefix = (
          <>
            <span className="json_key">{JSON.stringify(k)}</span>
            <span className="json_colon">: </span>
          </>
        );
        return render_any({
          ...args,
          value: v,
          path: child_path,
          depth: args.depth + 1,
          is_last,
          prefix,
        });
      })}
      {render_line(args.depth, null, <span className="json_brace">{"}"}</span>, args.is_last)}
    </>
  );
}

function render_line(depth: number, prefix: React.ReactNode, content: React.ReactNode, is_last: boolean): React.ReactElement {
  return (
    <div className="json_line" style={{ paddingLeft: `${depth * 14}px` }}>
      {prefix ? <span className="json_prefix">{prefix}</span> : null}
      {content}
      {!is_last ? <span className="json_comma">,</span> : null}
    </div>
  );
}
