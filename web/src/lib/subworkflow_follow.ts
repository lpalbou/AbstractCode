import { WaitState } from "./types";

export type FollowRunKind = "unknown" | "background" | "user_facing";

function _rank(kind: FollowRunKind): number {
  if (kind === "user_facing") return 3;
  if (kind === "background") return 2;
  return 1;
}

export function choose_follow_run(
  prev: { run_id: string; kind: FollowRunKind },
  next: { run_id: string; kind: FollowRunKind }
): { run_id: string; kind: FollowRunKind } {
  const a = { run_id: String(prev?.run_id || "").trim(), kind: (prev?.kind || "unknown") as FollowRunKind };
  const b = { run_id: String(next?.run_id || "").trim(), kind: (next?.kind || "unknown") as FollowRunKind };
  if (!b.run_id) return a;
  if (!a.run_id) return b;

  if (a.run_id === b.run_id) {
    return _rank(b.kind) >= _rank(a.kind) ? b : a;
  }

  // Keep a user-facing follow unless the new candidate is also user-facing.
  if (a.kind === "user_facing" && b.kind !== "user_facing") return a;
  if (b.kind === "user_facing" && a.kind !== "user_facing") return b;

  // Otherwise, prefer higher-ranked kinds.
  if (_rank(b.kind) > _rank(a.kind)) return b;
  return a;
}

export function infer_subworkflow_follow_kind(wait: WaitState | null | undefined): FollowRunKind {
  const w: any = wait as any;
  const details = w?.details;
  if (details && typeof details === "object") {
    const k = String(details?.follow_kind || details?.kind || "").trim().toLowerCase();
    if (k === "user_facing" || k === "user-facing") return "user_facing";
    if (k === "background") return "background";
  }
  return "unknown";
}

