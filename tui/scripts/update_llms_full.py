#!/usr/bin/env python3
"""Regenerate llms-full.txt from the documentation corpus.

Run from the repository root after any docs change:
    python3 scripts/update_llms_full.py
"""

import sys
from pathlib import Path

INCLUDED = [
    "README.md",
    "docs/getting-started.md",
    "docs/architecture.md",
    "docs/api.md",
    "docs/faq.md",
    "docs/troubleshooting.md",
    "docs/orchestration-cards.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "ACKNOWLEDGEMENTS.md",
    "CHANGELOG.md",
]

HEADER = """\
# abstractcode — full documentation corpus

This file aggregates the complete external documentation of abstractcode
for AI assistants and tools. The canonical sources are the individual files
named in each section header; regenerate with scripts/update_llms_full.py.

"""


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    missing = [p for p in INCLUDED if not (root / p).exists()]
    if missing:
        print(f"missing docs: {missing}", file=sys.stderr)
        return 2
    parts = [HEADER]
    for rel in INCLUDED:
        body = (root / rel).read_text(encoding="utf-8").rstrip()
        parts.append(f"\n{'=' * 72}\nFILE: {rel}\n{'=' * 72}\n\n{body}\n")
    out = root / "llms-full.txt"
    out.write_text("".join(parts), encoding="utf-8")
    print(f"wrote {out} ({out.stat().st_size} bytes from {len(INCLUDED)} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
