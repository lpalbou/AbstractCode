#!/usr/bin/env python3
from __future__ import annotations

import argparse
from datetime import date
from pathlib import Path
import re


DEFAULT_SOURCES: list[str] = [
    "README.md",
    "CHANGELOG.md",
    "CHANGELOD.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "ACKNOWLEDMENTS.md",
    "ACKNOWLEDGMENTS.md",
    "docs/getting-started.md",
    "docs/README.md",
    "docs/architecture.md",
    "docs/cli.md",
    "docs/api.md",
    "docs/faq.md",
    "docs/workflows.md",
    "docs/ui_events.md",
    "docs/web.md",
    "docs/deployment-web.md",
    "docs/deployment-iphone.md",
]


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


_MD_LINK_RE = re.compile(r"\]\(([^)]+)\)")


def _normalize_markdown_links(*, text: str, file_path: Path, root: Path) -> str:
    """Rewrite relative Markdown links to repo-root relative paths.

    Why: In llms-full.txt, many docs are concatenated into a single file, so links
    like (getting-started.md) in docs/README.md would otherwise break.

    We do a best-effort rewrite outside of fenced code blocks only.
    """

    def _rewrite_target(raw_inside_parens: str) -> str:
        raw = str(raw_inside_parens or "").strip()
        if not raw:
            return raw

        # Keep autolink-like targets untouched (common in Markdown).
        if raw.startswith("<") and raw.endswith(">"):
            inner = raw[1:-1].strip()
            if inner.startswith(("http://", "https://", "mailto:")):
                return raw
            raw = inner

        # Split optional title (first token is the URL/path).
        parts = raw.split(None, 1)
        target = parts[0]
        rest = (" " + parts[1]) if len(parts) > 1 else ""

        if not target or target.startswith(("#", "http://", "https://", "mailto:")):
            return raw_inside_parens
        if target.startswith("/"):
            return raw_inside_parens

        # Preserve query and anchor.
        anchor = ""
        query = ""
        base = target
        if "#" in base:
            base, frag = base.split("#", 1)
            anchor = "#" + frag
        if "?" in base:
            base, q = base.split("?", 1)
            query = "?" + q
        if not base:
            return raw_inside_parens

        try:
            resolved = (file_path.parent / base).resolve()
        except Exception:
            return raw_inside_parens

        try:
            rel = resolved.relative_to(root)
        except Exception:
            return raw_inside_parens

        new_target = rel.as_posix() + query + anchor
        return new_target + rest

    out_lines: list[str] = []
    in_fence = False
    for ln in str(text or "").splitlines(keepends=True):
        stripped = ln.lstrip()
        if stripped.startswith("```") or stripped.startswith("~~~"):
            in_fence = not in_fence
            out_lines.append(ln)
            continue
        if in_fence:
            out_lines.append(ln)
            continue

        def _repl(m: re.Match[str]) -> str:
            inner = m.group(1)
            return "](" + _rewrite_target(inner) + ")"

        out_lines.append(_MD_LINK_RE.sub(_repl, ln))

    return "".join(out_lines)


def build_llms_full(*, root: Path, sources: list[str], generated_on: str) -> str:
    lines: list[str] = []
    lines.append("# AbstractCode (llms-full)")
    lines.append("")
    lines.append("> Full, unabridged content of the core documentation files for this repo.")
    lines.append("> Contains docs and project policies; it does not inline source code (see `llms.txt`).")
    lines.append("> Recommended: use `llms.txt` as the index when link-fetching is available; use `llms-full.txt` for offline/single-file context.")
    lines.append("> Generated from the files listed below. Relative Markdown links are normalized to repo-root paths.")
    lines.append("> If you update docs/policies, regenerate this file.")
    lines.append(f"> Last generated: {generated_on}")
    lines.append("")
    lines.append("Files included (source):")
    for rel in sources:
        lines.append(f"- `{rel}`")
    lines.append("")
    lines.append("---")
    lines.append("")

    for rel in sources:
        p = (root / rel).resolve()
        # Keep the marker path stable and repo-relative.
        lines.append(f"<!-- FILE: {rel} -->")
        lines.append("")
        content = _normalize_markdown_links(text=_read_text(p), file_path=p, root=root)
        # Ensure each file ends with a newline to avoid accidental concatenation.
        if content and not content.endswith("\n"):
            content += "\n"
        lines.append(content)
        lines.append("---")
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Generate llms-full.txt from core docs/policies.")
    parser.add_argument("--root", default=".", help="Repo root (default: .)")
    parser.add_argument("--output", default="llms-full.txt", help="Output path (default: llms-full.txt)")
    parser.add_argument("--date", default=None, help="Override generation date (YYYY-MM-DD)")
    parser.add_argument(
        "--sources",
        default=None,
        help="Comma-separated list of repo-relative source paths (default: built-in set).",
    )
    args = parser.parse_args(argv)

    root = Path(args.root).expanduser().resolve()
    out = (root / str(args.output)).resolve()
    generated_on = str(args.date).strip() if args.date else date.today().isoformat()
    if args.sources:
        sources = [s.strip() for s in str(args.sources).split(",") if s.strip()]
    else:
        sources = list(DEFAULT_SOURCES)

    missing = [rel for rel in sources if not (root / rel).exists()]
    if missing:
        raise SystemExit(f"Missing source files: {', '.join(missing)}")

    text = build_llms_full(root=root, sources=sources, generated_on=generated_on)
    out.write_text(text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
