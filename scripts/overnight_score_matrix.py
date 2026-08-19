#!/usr/bin/env python3
"""Consolidated overnight capability scoring (artifact + multi-coder forensic)."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from overnight_multicoder_forensic import analyze_log, scoring_recommendation  # noqa: E402
from overnight_score_utils import resolve_capability_hello, score_hello  # noqa: E402


def build_matrix(
    report_path: Path,
    *,
    monorepo_root: Path,
    workspace_root: str,
) -> dict:
    data = json.loads(report_path.read_text())
    rows_in = data.get("tiers", {}).get("capability") or []

    rows_out: list[dict] = []
    official_ok = 0
    artifact_strict_ok = 0
    artifact_norm_ok = 0

    for row in rows_in:
        hello = resolve_capability_hello(
            row, monorepo_root=monorepo_root, workspace_root=workspace_root
        )
        gate = score_hello(hello)
        forensic: dict | None = None
        log_path = row.get("log_path")
        if row.get("mode") == "multi-coder" and log_path and Path(log_path).is_file():
            forensic = analyze_log(Path(log_path))
            forensic["recommendation"] = scoring_recommendation(
                forensic, artifact_ok=gate["content_ok"]
            )

        if row.get("ok"):
            official_ok += 1
        if gate["content_ok"]:
            artifact_strict_ok += 1
        if gate["content_ok_normalized"]:
            artifact_norm_ok += 1

        rows_out.append(
            {
                "client": row.get("client"),
                "mode": row.get("mode"),
                "iteration": row.get("iteration"),
                "official_ok": bool(row.get("ok")),
                "report_exit_code": row.get("exit_code"),
                **gate,
                "multicoder_forensic": forensic,
            }
        )

    total = len(rows_in)
    return {
        "report": str(report_path),
        "summary": {
            "official_ok": official_ok,
            "official_total": total,
            "artifact_strict_ok": artifact_strict_ok,
            "artifact_normalized_ok": artifact_norm_ok,
            "artifact_total": total,
        },
        "rows": rows_out,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--report",
        type=Path,
        default=Path("untracked/overnight-bench/report.json"),
    )
    parser.add_argument(
        "--monorepo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    parser.add_argument("--workspace-root", default="abstractcode-tui")
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()

    repo = Path(__file__).resolve().parents[1]
    report_path = args.report if args.report.is_absolute() else repo / args.report
    matrix = build_matrix(
        report_path,
        monorepo_root=args.monorepo_root,
        workspace_root=args.workspace_root,
    )
    print(json.dumps(matrix, indent=2))

    if args.write:
        out = report_path.with_name("report.scoring-matrix.json")
        out.write_text(json.dumps(matrix, indent=2) + "\n")
        print(f"wrote {out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
