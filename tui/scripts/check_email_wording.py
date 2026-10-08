#!/usr/bin/env python3
"""Wording parity of /schedule's email parts and dialog words with the kit (R17.2).

1. EMAIL_TEXT — diffs, key by key, three copies of the kit's email wording table:
     a. the kit's own `automation_controls.json` → `email` (ui-kit `src/automations/`),
     b. the vendored copy `assets/automation_controls.json` → `email` (what `cargo test`
        diffs against: automation_email::tests::wording_table_matches_the_kit),
     c. the terminal's table `TEXT` in `src/automation_email.rs` (its `pub const` strings).
2. Dialog words — the AfScheduleDialog / email_fields / automation_tools_picker / panel_core
   literals the terminal shows (legends, radios, hints): each must appear verbatim in the kit
   source AND in the terminal source (`src/ui/schedule_view.rs`, `src/automations.rs`,
   `src/automation_email.rs`).

Exit 0 = everything agrees byte for byte; 1 = any difference (printed); 2 = a copy could not be read.

Usage:
  python3 scripts/check_email_wording.py [KIT_AUTOMATIONS_DIR]

KIT_AUTOMATIONS_DIR defaults to the first that exists of:
  ../../abstractuic/ui-kit/src/automations
  ../web/node_modules/@abstractframework/ui-kit/src/automations
"""

import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent.parent  # tui/
VENDORED = HERE / "assets/automation_controls.json"
RUST = HERE / "src/automation_email.rs"
TUI_SOURCES = [HERE / "src/ui/schedule_view.rs", HERE / "src/automations.rs", HERE / "src/automation_email.rs"]
DEFAULTS = [
    HERE / "../../abstractuic/ui-kit/src/automations",
    HERE / "../web/node_modules/@abstractframework/ui-kit/src/automations",
]
KIT_FILES = ["AfScheduleDialog.tsx", "email_fields.tsx", "automation_tools_picker.tsx", "panel_core.ts"]

# The dialog's non-table words the terminal shows (kit literals, verbatim).
DIALOG_WORDS = [
    "What",
    "When (UTC)",
    "Context",
    "Tools",
    "Mailbox",
    "Title and limits",
    "Repeat",
    "Once at…",
    "Independent — each run starts fresh",
    "Growing — each run sees the previous runs",
    "Max growing context (tokens)",
    "Run without asking",
    "Ask me before each tool call (the run waits for you)",
    "Each tool call waits for your approval in the automation's timeline.",
    "Use workflow default tools",
    "Choose which tools this automation can use. An empty selection disables tools. Gateway restrictions always apply.",
    "Tools run without asking (you approve them now by creating this automation)",
    "Limits history carried into the next run, keeping recent whole turns. The newest turn is kept even if oversized. New messages and tool results can grow context beyond this budget.",
    "Turn on Email result to choose yourself or other email addresses.",
    "Connect a mailbox first.",
    "Run once at (UTC)",
    "First run at (UTC; empty = now)",
    "Stop after this many runs",
    "Stop at (UTC)",
    "Defaults to the task's first line",
    "Create automation",
    "Incomplete email trigger.",
    "Incomplete schedule.",
]


def rust_pairs(text: str) -> list:
    consts = {
        name: json.loads(value)
        for name, value in re.findall(r'pub const ([A-Z_]+): &str\s*=\s*("(?:[^"\\]|\\.)*");', text, re.S)
    }
    m = re.search(r"pub const TEXT: &\[\(&str, &str\)\] = &\[(.*?)\];", text, re.S)
    if not m:
        sys.exit(f"{RUST}: no TEXT table")
    return [(k, consts[c]) for k, c in re.findall(r'\("([a-z_]+)",\s*([A-Z_]+)\)', m.group(1))]


def email_pairs(path: pathlib.Path) -> list:
    data = json.loads(path.read_text(), object_pairs_hook=lambda kv: kv)
    for key, value in data:
        if key == "email":
            return [(k, v) for k, v in value]
    sys.exit(f"{path}: no email table")


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
    kit_dir = pathlib.Path(args[0]) if args else next((p for p in DEFAULTS if (p / "automation_controls.json").exists()), None)
    if kit_dir is None or not (kit_dir / "automation_controls.json").exists():
        print(f"kit automations dir not found (tried {', '.join(str(p) for p in DEFAULTS)}); pass its path", file=sys.stderr)
        return 2
    kit = email_pairs(kit_dir / "automation_controls.json")
    vendored = email_pairs(VENDORED)
    ours = rust_pairs(RUST.read_text())
    problems = diff("kit", kit, "vendored", vendored) + diff("vendored", vendored, "terminal", ours)
    kit_src = "\n".join((kit_dir / f).read_text() for f in KIT_FILES if (kit_dir / f).exists())
    tui_src = "\n".join(p.read_text() for p in TUI_SOURCES)
    for word in DIALOG_WORDS:
        if word not in kit_src:
            problems.append(f"dialog word not in the kit source: {word!r}")
        if word not in tui_src:
            problems.append(f"dialog word not in the terminal source: {word!r}")
    print(f"kit: {kit_dir.resolve()}")
    print(f"EMAIL_TEXT keys: kit {len(kit)} · vendored {len(vendored)} · terminal {len(ours)}; dialog words: {len(DIALOG_WORDS)}")
    for p in problems:
        print("DIFF", p)
    print("PASS: 0 diffs" if not problems else f"FAIL: {len(problems)} diffs")
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())
