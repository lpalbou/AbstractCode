import React, { useMemo } from "react";

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
      merged_edges.push({
        ...(e as any),
        id: safe_str(e?.id || `${sid}->${tid}`),
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

export function FlowGraph(props: {
  flow: VisualFlow | null;
  flow_by_id?: Record<string, any>;
  expand_subflows?: boolean;
  active_node_id?: string;
  recent_nodes?: Record<string, number>;
  now_ms?: number;
}): React.ReactElement {
  const now_ms = typeof props.now_ms === "number" ? props.now_ms : Date.now();
  const flow_in = props.flow;
  const active = safe_str(props.active_node_id);
  const recent = props.recent_nodes || {};

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

    const nodes_out: GraphNode[] = [];
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

    const edges_out: GraphEdge[] = [];
    for (const e of raw_edges) {
      if (!is_exec_edge(e)) continue;
      const source = safe_str(e?.source).trim();
      const target = safe_str(e?.target).trim();
      if (!source || !target) continue;
      edges_out.push({ id: safe_str(e?.id || `${source}->${target}`), source, target });
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
  }, [flow_in, props.flow_by_id, props.expand_subflows]);

  if (!flow_in) {
    return (
      <div className="graph_empty mono">
        (no graph loaded)
      </div>
    );
  }

  const view_box = `${bounds.x} ${bounds.y} ${bounds.w} ${bounds.h}`;
  const node_by_id: Record<string, GraphNode> = {};
  for (const n of nodes) node_by_id[n.id] = n;

  return (
    <div className="graph_wrap">
      <svg className="graph_svg" viewBox={view_box} role="img" aria-label="Workflow execution graph">
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
          return <line key={e.id} x1={x1} y1={y1} x2={x2} y2={y2} className="graph_edge" markerEnd="url(#arrow)" />;
        })}

        {nodes.map((n) => {
          const until = typeof recent[n.id] === "number" ? recent[n.id] : 0;
          const is_recent = until > now_ms;
          const is_active = active && n.id === active;
          const cls = `graph_node ${is_active ? "active" : is_recent ? "recent" : ""}`;
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
