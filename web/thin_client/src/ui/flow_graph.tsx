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

export function FlowGraph(props: {
  flow: VisualFlow | null;
  active_node_id?: string;
  recent_nodes?: Record<string, number>;
  now_ms?: number;
}): React.ReactElement {
  const now_ms = typeof props.now_ms === "number" ? props.now_ms : Date.now();
  const flow = props.flow;
  const active = safe_str(props.active_node_id);
  const recent = props.recent_nodes || {};

  const { nodes, edges, bounds } = useMemo(() => {
    const vf: any = flow || {};
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
  }, [flow]);

  if (!flow) {
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

