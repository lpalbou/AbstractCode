#!/usr/bin/env python3
"""Helpers for overnight capability scoring (workspace + content gates)."""

from __future__ import annotations

from pathlib import Path

EXPECTED = b"overnight-cap-ok"


def content_ok(data: bytes, *, strip_trailing_ws: bool = False) -> bool:
    payload = data.rstrip(b"\r\n") if strip_trailing_ws else data
    return payload == EXPECTED


def resolve_monorepo_artifact(
    monorepo_root: Path,
    *,
    workspace_root: str,
    relative_path: str,
) -> Path:
    """Map gateway workspace_root + tool-relative path to a local path.

    When the gateway stores workspace_root as a basename (e.g. ``abstractcode-tui``)
    and the tool writes ``untracked/...``, the file often lands under the monorepo
    root's ``untracked/`` tree — not under ``<monorepo>/<basename>/untracked/``.
    """
    rel = Path(relative_path)
    ws = (workspace_root or "").strip()
    if ws and rel.parts and rel.parts[0] == "untracked":
        return monorepo_root / rel
    if ws:
        candidate = monorepo_root / ws / rel
        if candidate.is_file():
            return candidate
    return monorepo_root / rel


def find_hello(out_dir: Path) -> Path | None:
    path = out_dir / "hello.txt"
    return path if path.is_file() else None


def resolve_capability_hello(
    row: dict,
    *,
    monorepo_root: Path,
    workspace_root: str = "abstractcode-tui",
) -> Path | None:
    """Locate hello.txt for a capability report row (out_dir, artifact_path, monorepo)."""
    artifact = row.get("artifact_path")
    if artifact:
        p = Path(artifact)
        if p.is_file():
            return p

    out_dir = Path(row.get("out_dir") or "")
    hello = find_hello(out_dir)
    if hello is not None:
        return hello

    client = str(row.get("client") or "")
    mode = str(row.get("mode") or "")
    iteration = int(row.get("iteration") or 0)
    if client == "codex" and mode == "exec":
        rel = f"untracked/overnight-bench/out/codex/exec-{iteration}/hello.txt"
    elif client == "opencode":
        rel = f"untracked/overnight-bench/out/opencode/run-{iteration}/hello.txt"
    elif client == "pi":
        rel = f"untracked/overnight-bench/out/pi/one-shot-{iteration}/hello.txt"
    elif client in {"code", "code-tui"}:
        rel = f"untracked/overnight-bench/out/{client}/{mode}-{iteration}/hello.txt"
    else:
        return None

    candidate = resolve_monorepo_artifact(
        monorepo_root, workspace_root=workspace_root, relative_path=rel
    )
    return candidate if candidate.is_file() else None


def score_hello(path: Path | None, *, strip_trailing_ws: bool = False) -> dict:
    if path is None or not path.is_file():
        return {
            "hello_path": None,
            "bytes": 0,
            "content_ok": False,
            "content_ok_normalized": False,
        }
    data = path.read_bytes()
    return {
        "hello_path": str(path),
        "bytes": len(data),
        "content_ok": content_ok(data, strip_trailing_ws=False),
        "content_ok_normalized": content_ok(data, strip_trailing_ws=True),
    }
