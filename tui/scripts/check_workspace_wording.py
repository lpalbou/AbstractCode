#!/usr/bin/env python3
"""Wording parity of the terminal's workspace chooser with the kit (R14.4).

Diffs, key by key, three copies of the kit's WORKSPACE_CHOOSER_TEXT table:

  1. the kit's own source (ui-kit `src/workspace_chooser_core.ts`, or a
     built `dist/*.js` that carries the table),
  2. the vendored copy `tests/fixtures/workspaces/kit_workspace_chooser_text.ts`
     (what `cargo test` diffs against: workspaces::tests::wording_table_matches_the_kit),
  3. the terminal's table `TEXT` in `src/workspaces.rs` (its `pub const` strings).

Exit 0 = the three agree byte for byte (36 keys); 1 = any difference (printed);
2 = a copy could not be read. `--write` refreshes the vendored copy (2) from the
kit source (1) — then re-run `cargo test` to see what the terminal must change.

Usage:
  python3 scripts/check_workspace_wording.py [KIT_SOURCE] [--write]

KIT_SOURCE defaults to the first that exists of:
  ../web/node_modules/@abstractframework/ui-kit/src/workspace_chooser_core.ts
  ../../abstractuic/ui-kit/src/workspace_chooser_core.ts
"""

import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent.parent  # tui/
FIXTURE = HERE / "tests/fixtures/workspaces/kit_workspace_chooser_text.ts"
RUST = HERE / "src/workspaces.rs"
DEFAULTS = [
    HERE / "../web/node_modules/@abstractframework/ui-kit/src/workspace_chooser_core.ts",
    HERE / "../../abstractuic/ui-kit/src/workspace_chooser_core.ts",
]

PAIR = re.compile(r'([A-Za-z]+)\s*:\s*("(?:[^"\\]|\\.)*")')


def kit_block(text: str, where: str) -> str:
    m = re.search(r"WORKSPACE_CHOOSER_TEXT\s*=\s*\{(.*?)\}\s*as const", text, re.S) or re.search(
        r"WORKSPACE_CHOOSER_TEXT\s*=\s*\{(.*?)\}", text, re.S
    )
    if not m:
        sys.exit(f"{where}: no WORKSPACE_CHOOSER_TEXT block")
    return m.group(1)


def kit_pairs(text: str, where: str) -> list:
    return [(k, json.loads(v)) for k, v in PAIR.findall(kit_block(text, where))]


def rust_pairs(text: str) -> list:
    consts = {
        name: json.loads(value)
        for name, value in re.findall(r'pub const ([A-Z_]+): &str\s*=\s*("(?:[^"\\]|\\.)*");', text, re.S)
    }
    m = re.search(r"pub const TEXT: &\[\(&str, &str\)\] = &\[(.*?)\];", text, re.S)
    if not m:
        sys.exit(f"{RUST}: no TEXT table")
    return [(k, consts[c]) for k, c in re.findall(r'\("([A-Za-z]+)",\s*([A-Z_]+)\)', m.group(1))]


def diff(a_name: str, a: list, b_name: str, b: list) -> list:
    out = []
    if len(a) != len(b):
        out.append(f"{a_name} has {len(a)} keys, {b_name} has {len(b)}")
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            out.append(f"#{i}: {a_name} {x!r} != {b_name} {y!r}")
    return out


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    write = "--write" in sys.argv
    src = pathlib.Path(args[0]) if args else next((p for p in DEFAULTS if p.exists()), None)
    if src is None or not src.exists():
        print(f"kit source not found (tried {', '.join(str(p) for p in DEFAULTS)}); pass its path", file=sys.stderr)
        return 2
    kit = kit_pairs(src.read_text(), str(src))
    if write:
        body = "\n".join(f"  {k}: {json.dumps(v, ensure_ascii=False)}," for k, v in kit)
        head = [l for l in FIXTURE.read_text().splitlines() if l.startswith("//")]
        FIXTURE.write_text("\n".join(head) + "\nexport const WORKSPACE_CHOOSER_TEXT = {\n" + body + "\n} as const;\n")
        print(f"wrote {FIXTURE} from {src}")
    vendored = kit_pairs(FIXTURE.read_text(), str(FIXTURE))
    ours = rust_pairs(RUST.read_text())
    problems = diff("kit", kit, "vendored", vendored) + diff("vendored", vendored, "terminal", ours)
    print(f"kit source: {src.resolve()}")
    print(f"keys: kit {len(kit)} · vendored {len(vendored)} · terminal {len(ours)}")
    for p in problems:
        print("DIFF", p)
    print("PASS: 0 diffs" if not problems else f"FAIL: {len(problems)} diffs")
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())
