from __future__ import annotations

import os
import re
from pathlib import Path
from typing import Iterable, List, Tuple


_AT_MENTION_RE = re.compile(r"(^|\s)@([^\s]+)")
_TRAILING_PUNCT = ".,;:!?)]}>\"'"
_MOUNT_NAME_RE = re.compile(r"^[a-zA-Z0-9_-]{1,32}$")


def default_workspace_root(*, cwd: Path | None = None) -> Path:
    """Return the workspace root used by AbstractCode for `@file` mentions.

    Always uses the current working directory (or the provided `cwd`).
    """
    base = cwd if isinstance(cwd, Path) else Path.cwd()
    return base.resolve()


def workspace_root_from_env() -> Path | None:
    """Deprecated: env-based workspace root overrides are not supported."""
    return None


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


def find_at_file_mentions(text: str) -> List[str]:
    """Return `@file` mention tokens without mutating the original text."""
    raw = str(text or "")
    out: List[str] = []
    for m in _AT_MENTION_RE.finditer(raw):
        tok = str(m.group(2) or "")
        tok = tok.rstrip(_TRAILING_PUNCT).strip()
        if tok:
            out.append(tok)
    return out


def parse_workspace_mounts(raw: str) -> dict[str, Path]:
    """Parse newline-separated `name=/abs/path` mount entries (best-effort)."""
    out: dict[str, Path] = {}
    for ln in str(raw or "").splitlines():
        line = str(ln or "").strip()
        if not line or line.startswith("#"):
            continue
        if "=" not in line:
            continue
        name, path = line.split("=", 1)
        name = name.strip()
        path = path.strip()
        if not name or not _MOUNT_NAME_RE.match(name):
            continue
        if not path:
            continue
        try:
            p = Path(path).expanduser()
            if not p.is_absolute():
                continue
            resolved = p.resolve()
        except Exception:
            continue
        try:
            if not resolved.exists() or not resolved.is_dir():
                continue
        except Exception:
            continue
        out[name] = resolved
    return dict(out)


def default_workspace_mounts() -> dict[str, Path]:
    raw = os.environ.get("ABSTRACTCODE_WORKSPACE_MOUNTS") or os.environ.get("ABSTRACTGATEWAY_WORKSPACE_MOUNTS") or ""
    return parse_workspace_mounts(raw)


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
    p = p.replace("\\", "/")
    if not _is_safe_relpath(p):
        return ""
    # Collapse a leading "./" for nicer UX and more stable matching.
    if p.startswith("./"):
        p = p[2:]
    return p


def resolve_workspace_path(
    *,
    raw_path: str,
    workspace_root: Path,
    mounts: dict[str, Path],
) -> tuple[Path, str, str | None, Path]:
    """Resolve a virtual path against workspace_root + mounts.

    Virtual path grammar:
    - Primary root: `docs/readme.md`
    - Mount root: `mount/path/to/file.md` (mount must be in `mounts`)

    Notes:
    - Mount resolution requires `mount/...` (at least one `/`) to avoid collisions.
    - Absolute paths are allowed only when under workspace_root or a mount root.
    """
    raw = str(raw_path or "").strip()
    if not raw:
        raise ValueError("Empty path")

    cleaned = raw.replace("\\", "/")
    p = Path(cleaned).expanduser()
    root = Path(workspace_root).expanduser()

    if p.is_absolute():
        resolved = p.resolve()
        candidates: list[tuple[int, str | None, Path]] = []
        try:
            resolved.relative_to(root)
            candidates.append((len(str(root)), None, root))
        except Exception:
            pass
        for name, mroot in (mounts or {}).items():
            if not isinstance(mroot, Path):
                continue
            try:
                resolved.relative_to(mroot)
                candidates.append((len(str(mroot)), str(name), mroot))
            except Exception:
                continue
        if not candidates:
            raise ValueError("Path is outside workspace roots")
        candidates.sort(key=lambda x: x[0], reverse=True)
        _len, mount, selected_root = candidates[0]
        rel = resolved.relative_to(selected_root).as_posix()
        virt = f"{mount}/{rel}" if mount and rel else (str(mount) if mount else rel)
        return resolved, virt, mount, selected_root

    virt_raw = cleaned
    while virt_raw.startswith("./"):
        virt_raw = virt_raw[2:]

    parts = [seg for seg in virt_raw.split("/") if seg not in ("", ".")]
    mount: str | None = None
    selected_root = root
    rel_part = virt_raw
    if len(parts) >= 2 and parts[0] in (mounts or {}):
        mount = parts[0]
        selected_root = mounts[mount]
        rel_part = "/".join(parts[1:])

    resolved = (selected_root / Path(rel_part)).resolve()
    try:
        resolved.relative_to(selected_root)
    except Exception:
        raise ValueError("Path escapes workspace root")

    rel_norm = resolved.relative_to(selected_root).as_posix()
    virt_norm = f"{mount}/{rel_norm}" if mount and rel_norm else (str(mount) if mount else rel_norm)
    return resolved, virt_norm, mount, selected_root


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
