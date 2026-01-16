from __future__ import annotations

import os
import re
from pathlib import Path
from typing import Iterable, List, Tuple


_AT_MENTION_RE = re.compile(r"(^|\s)@([^\s]+)")
_TRAILING_PUNCT = ".,;:!?)]}>\"'"


def default_workspace_root(*, cwd: Path | None = None) -> Path:
    """Return the workspace root used by AbstractCode for `@file` mentions.

    Preference order:
    - `ABSTRACTCODE_WORKSPACE_DIR`
    - `ABSTRACTGATEWAY_WORKSPACE_DIR` (common shared convention)
    - `cwd` (or `Path.cwd()`)
    """
    raw = os.environ.get("ABSTRACTCODE_WORKSPACE_DIR") or os.environ.get("ABSTRACTGATEWAY_WORKSPACE_DIR")
    if isinstance(raw, str) and raw.strip():
        return Path(raw).expanduser().resolve()
    base = cwd if isinstance(cwd, Path) else Path.cwd()
    return base.resolve()


def extract_at_file_mentions(text: str) -> Tuple[str, List[str]]:
    """Extract `@file` mentions from a text prompt.

    Returns:
        (cleaned_text, mentions)

    Notes:
    - Mentions must start at the beginning or be preceded by whitespace.
    - Mentions run until the next whitespace.
    - Common trailing punctuation is stripped from the mention token.
    """
    raw = str(text or "")
    mentions: List[str] = []

    def _repl(m: re.Match[str]) -> str:
        tok = str(m.group(2) or "")
        tok = tok.rstrip(_TRAILING_PUNCT).strip()
        if tok:
            mentions.append(tok)
        # Keep the leading whitespace (or empty start-of-string).
        return str(m.group(1) or "")

    cleaned = _AT_MENTION_RE.sub(_repl, raw)
    cleaned = re.sub(r"\s{2,}", " ", cleaned).strip()
    return cleaned, mentions


def _is_safe_relpath(path: str) -> bool:
    p = str(path or "").strip()
    if not p:
        return False
    if p.startswith(("/", "\\")):
        return False
    # Disallow drive-letter absolute paths (Windows-style).
    if len(p) >= 2 and p[1] == ":":
        return False
    return True


def normalize_relative_path(path: str) -> str:
    p = str(path or "").strip()
    if not _is_safe_relpath(p):
        return ""
    # Collapse a leading "./" for nicer UX and more stable matching.
    if p.startswith("./"):
        p = p[2:]
    return p


def list_workspace_files(*, root: Path, ignore, max_files: int = 20000) -> List[str]:
    """Return a best-effort list of workspace-relative file paths (POSIX style)."""
    import os

    base = Path(root).resolve()
    out: List[str] = []

    for dirpath, dirnames, filenames in os.walk(base):
        cur = Path(dirpath)

        # Prune ignored directories (in-place).
        kept: list[str] = []
        for d in dirnames:
            p = cur / d
            try:
                if ignore is not None and ignore.is_ignored(p, is_dir=True):
                    continue
            except Exception:
                pass
            kept.append(d)
        dirnames[:] = kept

        for fn in filenames:
            p = cur / fn
            try:
                if ignore is not None and ignore.is_ignored(p, is_dir=False):
                    continue
            except Exception:
                pass
            try:
                rel = p.relative_to(base).as_posix()
            except Exception:
                continue
            if not rel:
                continue
            out.append(rel)
            if len(out) >= int(max_files):
                return out

    return out


def search_workspace_files(files: Iterable[str], query: str, *, limit: int = 25) -> List[str]:
    """Return ranked file-path candidates for a user query (case-insensitive)."""
    q = str(query or "").strip().lower()
    if not q:
        return []

    scored: list[tuple[tuple[int, int, int, str], str]] = []
    for path in files:
        p = str(path or "").strip()
        if not p:
            continue
        p_low = p.lower()
        name = p.split("/")[-1].lower()

        # Scoring: lower tuple sorts first.
        # - 0: basename startswith
        # - 1: path startswith
        # - 2: basename contains
        # - 3: path contains
        if name.startswith(q):
            key = (0, len(p), 0, p_low)
        elif p_low.startswith(q):
            key = (1, len(p), 0, p_low)
        elif q in name:
            key = (2, len(p), name.find(q), p_low)
        elif q in p_low:
            key = (3, len(p), p_low.find(q), p_low)
        else:
            continue

        scored.append((key, p))

    scored.sort(key=lambda x: x[0])
    out = [p for _, p in scored]
    return out[: int(limit)]
