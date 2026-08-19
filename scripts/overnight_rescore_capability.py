#!/usr/bin/env python3
"""Rescore overnight capability rows using workspace-aware artifact resolution."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from overnight_score_utils import resolve_capability_hello, score_hello  # noqa: E402


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--report",
        type=Path,
        default=Path("untracked/overnight-bench/report.json"),
        help="Path to report.json (relative to repo root unless absolute)",
    )
    parser.add_argument(
        "--monorepo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="Monorepo root for workspace basename resolution",
    )
    parser.add_argument(
        "--workspace-root",
        default="abstractcode-tui",
        help="Gateway workspace_root basename stored on runs",
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="Write report.capability-rescore.json beside the report",
    )
    args = parser.parse_args()

    repo = Path(__file__).resolve().parents[1]
    report_path = args.report if args.report.is_absolute() else repo / args.report
    data = json.loads(report_path.read_text())
    rows = data.get("tiers", {}).get("capability") or []

    scored: list[dict] = []
    strict_ok = norm_ok = 0
    for row in rows:
        hello = resolve_capability_hello(
            row,
            monorepo_root=args.monorepo_root,
            workspace_root=args.workspace_root,
        )
        gate = score_hello(hello)
        if gate["content_ok"]:
            strict_ok += 1
        if gate["content_ok_normalized"]:
            norm_ok += 1
        scored.append(
            {
                "client": row.get("client"),
                "mode": row.get("mode"),
                "iteration": row.get("iteration"),
                "report_ok": row.get("ok"),
                **gate,
            }
        )

    out = {
        "report": str(report_path),
        "strict_ok": strict_ok,
        "strict_total": len(rows),
        "normalized_ok": norm_ok,
        "normalized_total": len(rows),
        "rows": scored,
    }
    print(json.dumps(out, indent=2))

    if args.write:
        sidecar = report_path.with_name("report.capability-rescore.json")
        sidecar.write_text(json.dumps(out, indent=2) + "\n")
        print(f"wrote {sidecar}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
