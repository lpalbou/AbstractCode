#!/usr/bin/env python3
"""Forensic pass on multi-coder capability logs (scout vs coder phase)."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

SCOUT_TOOLS = frozenset({"skim_folders", "search_files", "list_files", "skim_files"})
WRITE_TOOLS = frozenset({"write_file", "execute_command"})


def _parse_jsonl(path: Path) -> list[dict]:
    rows: list[dict] = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or not line.startswith("{"):
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return rows


def _parse_text_log(path: Path) -> dict:
    text = path.read_text(errors="replace")
    tools = re.findall(r"[✓»✗]\s+(\w+)\s+\{", text)
    scout_only = "scout-only" in text.lower()
    return {
        "format": "text",
        "tools": tools,
        "write_file_count": sum(1 for t in tools if t == "write_file"),
        "scout_tool_count": sum(1 for t in tools if t in SCOUT_TOOLS),
        "scout_only_phrase": scout_only,
    }


def analyze_log(path: Path) -> dict:
    if path.suffix == ".jsonl":
        events = _parse_jsonl(path)
        tools: list[str] = []
        final_answer = ""
        for ev in events:
            if ev.get("event") == "tool_call":
                tools.append(str(ev.get("tool") or ""))
            if ev.get("event") == "final":
                final_answer = str(ev.get("answer") or "")
        scout_only = "scout-only" in final_answer.lower()
        if not scout_only and tools and not any(t in WRITE_TOOLS for t in tools):
            if final_answer and "no files were written" in final_answer.lower():
                scout_only = True
        return {
            "format": "jsonl",
            "path": str(path),
            "tools": tools,
            "write_file_count": sum(1 for t in tools if t == "write_file"),
            "scout_tool_count": sum(1 for t in tools if t in SCOUT_TOOLS),
            "scout_only_phrase": scout_only,
            # ADR-0026: whole answer (a [:240] clip hid the evidence the
            # scoring recommendation below is derived from).
            "final_snippet": final_answer,
        }
    return {"path": str(path), **_parse_text_log(path)}


def scoring_recommendation(row: dict, *, artifact_ok: bool | None = None) -> str:
    if artifact_ok:
        return "score_artifact_despite_timeout"
    if row.get("write_file_count", 0) > 0:
        return "score_coder_phase_write"
    if row.get("scout_only_phrase") or (
        row.get("scout_tool_count", 0) > 0 and row.get("write_file_count", 0) == 0
    ):
        return "fail_scout_only_or_rerun_coder_phase"
    return "inspect_manually"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("logs", nargs="+", type=Path, help="Capability log paths")
    parser.add_argument("--write", type=Path, help="Write JSON report")
    args = parser.parse_args()

    out_rows = []
    for log in args.logs:
        if not log.is_file():
            print(f"missing: {log}", file=sys.stderr)
            continue
        row = analyze_log(log)
        row["recommendation"] = scoring_recommendation(row)
        out_rows.append(row)

    payload = {"rows": out_rows}
    print(json.dumps(payload, indent=2))
    if args.write:
        args.write.write_text(json.dumps(payload, indent=2) + "\n")
        print(f"wrote {args.write}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
