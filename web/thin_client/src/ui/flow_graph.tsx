import React, { useEffect, useMemo, useRef, useState } from "react";

import { PinLegend } from "./pin_legend";

type VisualFlow = {
  id?: string;
  name?: string;
  nodes?: any[];
  edges?: any[];
  entryNode?: string;
};

type GraphNode = {
  id: string;
  x: number;
  y: number;
  label: string;
  type: string;
  color: string;
};

type GraphEdge = {
  id: string;
  source: string;
  target: string;
};

type ViewBox = { x: number; y: number; w: number; h: number };

function safe_str(v: any): string {
  return typeof v === "string" ? v : v === null || v === undefined ? "" : String(v);
}

function clamp_text(s: string, max_len: number): string {
  const text = safe_str(s).trim();
  if (text.length <= max_len) return text;
  return `${text.slice(0, Math.max(0, max_len - 1))}…`;
}

function is_exec_edge(e: any): boolean {
  const th = safe_str(e?.targetHandle);
  const sh = safe_str(e?.sourceHandle);
  if (th === "exec-in") return true;
  if (sh === "exec-out") return true;
  if (th.includes("exec") || sh.includes("exec")) return true;
  return false;
}

function node_type_from_raw(n: any): string {
  if (!n || typeof n !== "object") return "";
  const data = n?.data && typeof n.data === "object" ? n.data : {};
  return safe_str((data as any)?.nodeType || n?.type || "").trim();
}

function subflow_id_from_raw(n: any): string {
  if (!n || typeof n !== "object") return "";
  const data = n?.data && typeof n.data === "object" ? n.data : {};
  const sid = (data as any)?.subflowId || (data as any)?.flowId;
  const s = safe_str(sid).trim();
  if (!s) return "";
  // Accept namespaced "bundle:flow" but return only the local flow id.
  if (s.includes(":")) {
    const parts = s.split(":", 2);
    if (parts.length === 2 && parts[1]) return parts[1].trim();
  }
  return s;
}

function entry_node_id(flow: any): string {
  const en = safe_str(flow?.entryNode).trim();
  if (en) return en;
  const nodes = Array.isArray(flow?.nodes) ? flow.nodes : [];
  for (const n of nodes) {
    if (node_type_from_raw(n) === "on_flow_start") {
      const id = safe_str(n?.id).trim();
      if (id) return id;
    }
  }
  return "";
}

function node_pos(n: any): { x: number; y: number } {
  const pos = n?.position && typeof n.position === "object" ? n.position : {};
  const x = typeof (pos as any).x === "number" ? (pos as any).x : 0;
  const y = typeof (pos as any).y === "number" ? (pos as any).y : 0;
  return { x, y };
}

function prefixed_id(prefix: string, id: string): string {
  const p = safe_str(prefix).trim();
  const s = safe_str(id).trim();
  if (!s) return "";
  return p ? `${p}::${s}` : s;
}

function merge_flow_with_subflows(args: {
  root: any;
  flow_by_id: Record<string, any>;
  expand_subflows: boolean;
  max_depth: number;
  max_nodes: number;
  max_edges: number;
}): VisualFlow {
  const root = args.root && typeof args.root === "object" ? args.root : {};
  if (!args.expand_subflows) return root;

  const merged_nodes: any[] = [];
  const merged_edges: any[] = [];
  const seen = new Set<string>();

  const add_flow = (flow: any, prefix: string, offset: { x: number; y: number }, depth: number) => {
    if (!flow || typeof flow !== "object") return;
    if (merged_nodes.length >= args.max_nodes || merged_edges.length >= args.max_edges) return;

    const raw_nodes = Array.isArray(flow.nodes) ? flow.nodes : [];
    const raw_edges = Array.isArray(flow.edges) ? flow.edges : [];

    for (const n of raw_nodes) {
      const id = safe_str(n?.id).trim();
      if (!id) continue;
      const nid = prefixed_id(prefix, id);
      if (!nid || seen.has(nid)) continue;
      const pos = node_pos(n);
      const next = { ...(n as any), id: nid, position: { x: pos.x + offset.x, y: pos.y + offset.y } };
      merged_nodes.push(next);
      seen.add(nid);
      if (merged_nodes.length >= args.max_nodes) break;
    }

    for (const e of raw_edges) {
      if (!is_exec_edge(e)) continue;
      const source = safe_str(e?.source).trim();
      const target = safe_str(e?.target).trim();
      if (!source || !target) continue;
      const sid = prefixed_id(prefix, source);
      const tid = prefixed_id(prefix, target);
      if (!sid || !tid) continue;
      const raw_id = safe_str(e?.id).trim();
      const edge_id = raw_id ? `${sid}->${tid}::${raw_id}` : `${sid}->${tid}`;
      merged_edges.push({
        ...(e as any),
        id: edge_id,
        source: sid,
        target: tid,
      });
      if (merged_edges.length >= args.max_edges) break;
    }

    if (depth >= args.max_depth) return;

    for (const n of raw_nodes) {
      if (merged_nodes.length >= args.max_nodes || merged_edges.length >= args.max_edges) return;
      if (node_type_from_raw(n) !== "subflow") continue;
      const child_fid = subflow_id_from_raw(n);
      if (!child_fid) continue;
      const child = args.flow_by_id[child_fid];
      if (!child || typeof child !== "object") continue;

      const parent_id = prefixed_id(prefix, safe_str(n?.id));
      if (!parent_id) continue;
      const en = entry_node_id(child);
      if (!en) continue;
      const child_nodes = Array.isArray((child as any).nodes) ? (child as any).nodes : [];
      const entry_raw = child_nodes.find((x: any) => safe_str(x?.id).trim() === en) || null;
      if (!entry_raw) continue;

      const parent_pos = node_pos(n);
      const entry_pos = node_pos(entry_raw);
      const dx = 240;
      const dy = 0;
      const child_offset = { x: parent_pos.x + offset.x + dx - entry_pos.x, y: parent_pos.y + offset.y + dy - entry_pos.y };

      const child_entry_id = prefixed_id(parent_id, en);
      merged_edges.push({
        id: `${parent_id}=>${child_entry_id}`,
        source: parent_id,
        target: child_entry_id,
        sourceHandle: "exec-out",
        targetHandle: "exec-in",
      });

      add_flow(child, parent_id, child_offset, depth + 1);
    }
  };

  add_flow(root, "", { x: 0, y: 0 }, 0);
  return {
    ...(root as any),
    id: safe_str((root as any).id),
    nodes: merged_nodes,
    edges: merged_edges,
  };
}

function is_plumbing_type(node_type: string): boolean {
  const t = safe_str(node_type).trim().toLowerCase();
  if (!t) return false;
  if (t.startsWith("literal")) return true;
  if (t.includes("literal")) return true;
  if (t === "concat") return true;
  if (t === "join") return true;
  if (t === "cast") return true;
  if (t === "parse_json" || t === "stringify_json") return true;
  if (t === "break_object") return true;
  if (t === "get_property" || t === "set_property") return true;
  if (t.includes("get_property") || t.includes("set_property")) return true;
  if (t === "get_variable" || t === "set_variable") return true;
  if (t.includes("get_variable") || t.includes("set_variable")) return true;
  if (t === "json_schema") return true;
  return false;
}

function simplify_exec_graph(args: {
  nodes: GraphNode[];
  edges: GraphEdge[];
  keep_id: (id: string) => boolean;
  max_edges: number;
}): { nodes: GraphNode[]; edges: GraphEdge[] } {
  const nodes = args.nodes.filter((n) => args.keep_id(n.id));
  const keep = new Set(nodes.map((n) => n.id));

  const outgoing: Record<string, string[]> = {};
  for (const e of args.edges) {
    if (!outgoing[e.source]) outgoing[e.source] = [];
    outgoing[e.source].push(e.target);
  }

  const dedup = new Map<string, GraphEdge>();
  for (const s of nodes) {
    const seen = new Set<string>();
    const stack = (outgoing[s.id] || []).slice();
    while (stack.length) {
      const t = stack.pop() as string;
      if (!t || seen.has(t)) continue;
      seen.add(t);
      if (keep.has(t)) {
        const key = `${s.id}->${t}`;
        if (!dedup.has(key)) dedup.set(key, { id: key, source: s.id, target: t });
        continue;
      }
      const nexts = outgoing[t] || [];
      for (const nx of nexts) {
        if (!seen.has(nx)) stack.push(nx);
      }
      if (dedup.size >= args.max_edges) break;
    }
    if (dedup.size >= args.max_edges) break;
  }

  return { nodes, edges: Array.from(dedup.values()) };
}

function clamp_view(view: ViewBox, bounds: { x: number; y: number; w: number; h: number }): ViewBox {
  const min_w = 160;
  const min_h = 120;
  const max_w = Math.max(bounds.w * 8, min_w);
  const max_h = Math.max(bounds.h * 8, min_h);

  const w = Math.max(min_w, Math.min(max_w, view.w));
  const h = Math.max(min_h, Math.min(max_h, view.h));
  return { x: view.x, y: view.y, w, h };
}

export function FlowGraph(props: {
  flow: VisualFlow | null;
  flow_by_id?: Record<string, any>;
  expand_subflows?: boolean;
  simplify?: boolean;
  active_node_id?: string;
  recent_nodes?: Record<string, number>;
  visited_nodes?: Record<string, number>;
  highlight_path?: boolean;
  now_ms?: number;
}): React.ReactElement {
  const now_ms = typeof props.now_ms === "number" ? props.now_ms : Date.now();
  const flow_in = props.flow;
  const active = safe_str(props.active_node_id);
  const recent = props.recent_nodes || {};
  const visited = props.visited_nodes || {};
  const highlight_path = props.highlight_path === true;

  const { nodes, edges, bounds } = useMemo(() => {
    const vf: any = merge_flow_with_subflows({
      root: flow_in || {},
      flow_by_id: props.flow_by_id || {},
      expand_subflows: props.expand_subflows === true,
      max_depth: 3,
      max_nodes: 700,
      max_edges: 900,
    });
    const raw_nodes = Array.isArray(vf.nodes) ? vf.nodes : [];
    const raw_edges = Array.isArray(vf.edges) ? vf.edges : [];

    let nodes_out: GraphNode[] = [];
    for (const n of raw_nodes) {
      const id = safe_str(n?.id).trim();
      if (!id) continue;
      const pos = n?.position && typeof n.position === "object" ? n.position : {};
      const x = typeof pos.x === "number" ? pos.x : 0;
      const y = typeof pos.y === "number" ? pos.y : 0;
      const data = n?.data && typeof n.data === "object" ? n.data : {};
      const label = safe_str(data?.label || n?.label || id) || id;
      const type = safe_str(data?.nodeType || n?.type || "unknown") || "unknown";
      const color = safe_str(data?.headerColor || n?.headerColor || "") || "";
      nodes_out.push({ id, x, y, label, type, color });
    }
    const node_lookup: Record<string, GraphNode> = {};
    for (const n of nodes_out) node_lookup[n.id] = n;

    let edges_out: GraphEdge[] = [];
    for (const e of raw_edges) {
      if (!is_exec_edge(e)) continue;
      const source = safe_str(e?.source).trim();
      const target = safe_str(e?.target).trim();
      if (!source || !target) continue;
      edges_out.push({ id: safe_str(e?.id || `${source}->${target}`), source, target });
    }

    // Only keep edges that connect nodes we actually have (important when graphs are truncated).
    const node_ids = new Set(nodes_out.map((n) => n.id));
    edges_out = edges_out.filter((e) => node_ids.has(e.source) && node_ids.has(e.target));

    if (props.simplify === true) {
      const keep_id = (id: string) => {
        if (active && id === active) return true;
        const n = node_lookup[id];
        if (!n) return false;
        const t = safe_str(n.type).trim().toLowerCase();
        if (t === "on_flow_start" || t === "on_flow_end") return true;
        if (t === "subflow") return true;
        return !is_plumbing_type(t);
      };
      const simplified = simplify_exec_graph({ nodes: nodes_out, edges: edges_out, keep_id, max_edges: 900 });
      nodes_out = simplified.nodes;
      edges_out = simplified.edges;
    }

    // Estimate bounds based on node positions.
    const pad = 60;
    const w = 160;
    const h = 56;
    let min_x = 0;
    let min_y = 0;
    let max_x = 0;
    let max_y = 0;
    if (nodes_out.length) {
      min_x = Math.min(...nodes_out.map((n) => n.x));
      min_y = Math.min(...nodes_out.map((n) => n.y));
      max_x = Math.max(...nodes_out.map((n) => n.x + w));
      max_y = Math.max(...nodes_out.map((n) => n.y + h));
    }
    const vb = {
      x: min_x - pad,
      y: min_y - pad,
      w: (max_x - min_x) + pad * 2,
      h: (max_y - min_y) + pad * 2,
      node_w: w,
      node_h: h,
    };
    return { nodes: nodes_out, edges: edges_out, bounds: vb };
  }, [flow_in, props.flow_by_id, props.expand_subflows, props.simplify, active]);

  if (!flow_in) {
    return (
      <div className="graph_empty mono">
        (no graph loaded)
      </div>
    );
  }

  const svg_ref = useRef<SVGSVGElement | null>(null);
  const [view, set_view] = useState<ViewBox>(() => ({ x: bounds.x, y: bounds.y, w: bounds.w, h: bounds.h }));
  const view_ref = useRef<ViewBox>(view);
  view_ref.current = view;

  useEffect(() => {
    set_view({ x: bounds.x, y: bounds.y, w: bounds.w, h: bounds.h });
  }, [bounds.x, bounds.y, bounds.w, bounds.h]);

  const pointers_ref = useRef<Map<number, { x: number; y: number }>>(new Map());
  const gesture_ref = useRef<
    | null
    | {
        mode: "pan" | "pinch";
        start_view: ViewBox;
        start_client: { x: number; y: number };
        start_dist: number;
        anchor_svg: { x: number; y: number };
      }
  >(null);

  const view_box = `${view.x} ${view.y} ${view.w} ${view.h}`;
  const node_by_id: Record<string, GraphNode> = {};
  for (const n of nodes) node_by_id[n.id] = n;

  const client_to_svg = (client_x: number, client_y: number, use_view?: ViewBox): { x: number; y: number } => {
    const vb = use_view || view_ref.current;
    const svg = svg_ref.current;
    if (!svg) return { x: client_x, y: client_y };
    const rect = svg.getBoundingClientRect();
    const px = rect.width > 0 ? (client_x - rect.left) / rect.width : 0;
    const py = rect.height > 0 ? (client_y - rect.top) / rect.height : 0;
    return { x: vb.x + px * vb.w, y: vb.y + py * vb.h };
  };

  const zoom_at = (anchor: { x: number; y: number }, factor: number, base_view?: ViewBox) => {
    const vb = base_view || view_ref.current;
    const f = Math.max(0.12, Math.min(8, factor));
    const next_w = vb.w * f;
    const next_h = vb.h * f;
    const rx = vb.w > 0 ? (anchor.x - vb.x) / vb.w : 0.5;
    const ry = vb.h > 0 ? (anchor.y - vb.y) / vb.h : 0.5;
    const next_x = anchor.x - rx * next_w;
    const next_y = anchor.y - ry * next_h;
    set_view(clamp_view({ x: next_x, y: next_y, w: next_w, h: next_h }, bounds));
  };

  const pan_by = (dx_client: number, dy_client: number, base_view?: ViewBox) => {
    const vb = base_view || view_ref.current;
    const svg = svg_ref.current;
    if (!svg) return;
    const rect = svg.getBoundingClientRect();
    const dx = rect.width > 0 ? (dx_client / rect.width) * vb.w : 0;
    const dy = rect.height > 0 ? (dy_client / rect.height) * vb.h : 0;
    set_view((prev) => clamp_view({ x: (base_view || prev).x - dx, y: (base_view || prev).y - dy, w: (base_view || prev).w, h: (base_view || prev).h }, bounds));
  };

  return (
    <div className="graph_wrap">
      <div className="graph_corner">
        <div className="graph_corner_buttons">
          <button
            className="btn graph_corner_btn"
            type="button"
            onClick={() => set_view({ x: bounds.x, y: bounds.y, w: bounds.w, h: bounds.h })}
            title="Recenter"
          >
            ⤢
          </button>
          <button
            className="btn graph_corner_btn"
            type="button"
            onClick={() => {
              const svg = svg_ref.current;
              if (!svg) return;
              const rect = svg.getBoundingClientRect();
              const anchor = client_to_svg(rect.left + rect.width / 2, rect.top + rect.height / 2);
              zoom_at(anchor, 0.84);
            }}
            title="Zoom in"
          >
            +
          </button>
          <button
            className="btn graph_corner_btn"
            type="button"
            onClick={() => {
              const svg = svg_ref.current;
              if (!svg) return;
              const rect = svg.getBoundingClientRect();
              const anchor = client_to_svg(rect.left + rect.width / 2, rect.top + rect.height / 2);
              zoom_at(anchor, 1.18);
            }}
            title="Zoom out"
          >
            −
          </button>
        </div>
        <PinLegend />
      </div>
      <svg
        ref={svg_ref}
        className="graph_svg"
        viewBox={view_box}
        role="img"
        aria-label="Workflow execution graph"
        onWheel={(e) => {
          e.preventDefault();
          const anchor = client_to_svg(e.clientX, e.clientY);
          const delta = typeof e.deltaY === "number" ? e.deltaY : 0;
          const factor = delta > 0 ? 1.12 : 0.89;
          zoom_at(anchor, factor);
        }}
        onPointerDown={(e) => {
          (e.currentTarget as any).setPointerCapture?.(e.pointerId);
          pointers_ref.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
          const pts = Array.from(pointers_ref.current.values());
          if (pts.length === 1) {
            gesture_ref.current = { mode: "pan", start_view: view_ref.current, start_client: { x: e.clientX, y: e.clientY }, start_dist: 0, anchor_svg: { x: 0, y: 0 } };
          } else if (pts.length >= 2) {
            const a = pts[0];
            const b = pts[1];
            const dx = a.x - b.x;
            const dy = a.y - b.y;
            const dist = Math.max(1, Math.hypot(dx, dy));
            const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
            const start_view = view_ref.current;
            gesture_ref.current = {
              mode: "pinch",
              start_view,
              start_client: mid,
              start_dist: dist,
              anchor_svg: client_to_svg(mid.x, mid.y, start_view),
            };
          }
        }}
        onPointerMove={(e) => {
          if (!pointers_ref.current.has(e.pointerId)) return;
          pointers_ref.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
          const g = gesture_ref.current;
          if (!g) return;
          const pts = Array.from(pointers_ref.current.values());
          if (g.mode === "pan" && pts.length === 1) {
            const dx = e.clientX - g.start_client.x;
            const dy = e.clientY - g.start_client.y;
            pan_by(dx, dy, g.start_view);
          } else if (g.mode === "pinch" && pts.length >= 2) {
            const a = pts[0];
            const b = pts[1];
            const dx = a.x - b.x;
            const dy = a.y - b.y;
            const dist = Math.max(1, Math.hypot(dx, dy));
            const factor = g.start_dist / dist;
            zoom_at(g.anchor_svg, factor, g.start_view);
          }
        }}
        onPointerUp={(e) => {
          pointers_ref.current.delete(e.pointerId);
          if (pointers_ref.current.size === 0) gesture_ref.current = null;
        }}
        onPointerCancel={(e) => {
          pointers_ref.current.delete(e.pointerId);
          if (pointers_ref.current.size === 0) gesture_ref.current = null;
        }}
      >
        <defs>
          <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse">
            <path d="M 0 0 L 10 5 L 0 10 z" fill="rgba(148,163,184,0.55)" />
          </marker>
          <filter id="glow">
            <feGaussianBlur stdDeviation="3" result="coloredBlur" />
            <feMerge>
              <feMergeNode in="coloredBlur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        {edges.map((e) => {
          const s = node_by_id[e.source];
          const t = node_by_id[e.target];
          if (!s || !t) return null;
          const x1 = s.x + bounds.node_w / 2;
          const y1 = s.y + bounds.node_h / 2;
          const x2 = t.x + bounds.node_w / 2;
          const y2 = t.y + bounds.node_h / 2;
          const is_visited_edge = highlight_path && visited && visited[e.source] !== undefined && visited[e.target] !== undefined;
          return <line key={e.id} x1={x1} y1={y1} x2={x2} y2={y2} className={`graph_edge ${is_visited_edge ? "visited" : ""}`} markerEnd="url(#arrow)" />;
        })}

        {nodes.map((n) => {
          const until = typeof recent[n.id] === "number" ? recent[n.id] : 0;
          const is_recent = until > now_ms;
          const is_active = active && n.id === active;
          const is_visited = highlight_path && visited && visited[n.id] !== undefined;
          const cls = `graph_node ${is_active ? "active" : is_recent ? "recent" : ""} ${is_visited ? "visited" : ""}`;
          const bar_color = n.color || "rgba(255,255,255,0.16)";
          const label = clamp_text(n.label || n.id, 22);
          const type = clamp_text(n.type, 18);
          return (
            <g key={n.id} className={cls} transform={`translate(${n.x}, ${n.y})`}>
              <rect className="graph_node_bg" width={bounds.node_w} height={bounds.node_h} rx={12} ry={12} />
              <rect className="graph_node_bar" width={bounds.node_w} height={6} rx={12} ry={12} fill={bar_color} />
              <text className="graph_node_label" x={12} y={28}>
                {label}
              </text>
              <text className="graph_node_type" x={12} y={46}>
                {type}
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}
