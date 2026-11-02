#!/usr/bin/env python3
"""Fail when a tracked path cannot be checked out on Windows.

Run from the repository root (CI runs it before any build):

    python3 scripts/check_portable_paths.py

Windows refuses file names containing < > : " | ? * or control characters,
names ending in a dot or a space, and the reserved device names (CON, PRN,
AUX, NUL, COM1-9, LPT1-9, with or without an extension). One such tracked
file makes `actions/checkout` fail on every Windows runner, which stops the
release's Windows binary and, with it, the GitHub release.
"""

from __future__ import annotations

import subprocess
import sys

FORBIDDEN = set('<>:"|?*')
RESERVED = {"CON", "PRN", "AUX", "NUL", *(f"COM{i}" for i in range(1, 10)), *(f"LPT{i}" for i in range(1, 10))}


def problem(component: str) -> str | None:
    bad = sorted({c for c in component if c in FORBIDDEN or ord(c) < 32})
    if bad:
        return "contains " + " ".join(repr(c) for c in bad)
    if component.endswith((".", " ")):
        return "ends with a dot or a space"
    if component.split(".")[0].upper() in RESERVED:
        return "is a reserved Windows device name"
    return None


def main() -> int:
    out = subprocess.run(["git", "ls-files", "-z"], check=True, capture_output=True).stdout
    paths = [p.decode("utf-8", "surrogateescape") for p in out.split(b"\0") if p]
    failures = []
    for path in paths:
        for component in path.split("/"):
            why = problem(component)
            if why:
                failures.append(f"{path}: {component!r} {why}")
                break
    if failures:
        print("Tracked paths Windows cannot check out:", file=sys.stderr)
        for line in failures:
            print(f"  {line}", file=sys.stderr)
        return 1
    print(f"check_portable_paths: OK ({len(paths)} tracked paths)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
