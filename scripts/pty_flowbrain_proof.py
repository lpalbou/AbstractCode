#!/usr/bin/env python3
"""LIVE flow-brain conversation proof (c5190/c5280, code-tui seat).

Two FRESH TUI processes against the real gateway:
  session 1: /brain veya  → teach a unique fact → /end → quit
  session 2: /brain veya  → ask for the fact    → RECALL proves the
             entity's own memory graph carried it across conversations
             (each session mints its own session id; nothing client-side
             persists between the two processes).

Gates on STRUCTURE (the chip's ◆name ready/✎ states), with exactly ONE
content assertion — the recalled token — because recall IS the thing
being proven. Frames are saved as text + PNG screenshots.

Env: ACODE_TUI_BIN (default target/release/abstractcode-tui),
     ACODE_GATEWAY_URL (default http://127.0.0.1:8080),
     ACODE_GATEWAY_TOKEN (required).
"""

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pty_conformance_common import Checks, Tui, env_config  # noqa: E402

OUT_DIR = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "untracked",
    "reports",
    "flowbrain-proof",
)

ENTITY = os.environ.get("PROOF_ENTITY", "veya")
TOKEN_PHRASE = "saffron-kestrel-42"
TEACH = (
    f"Please remember this precisely: the code-tui doorway's proof token is "
    f"{TOKEN_PHRASE}. I will ask you for it in a completely fresh conversation."
)
RECALL_ASK = "What is the code-tui doorway's proof token? Say the token itself."

# Uncontended flow turns land in ~10-20s; contended ran minutes in the
# reference. The client's own poll bound is 300s — the harness waits a
# little past it so the CLIENT's honesty line (not the harness) decides.
TURN_TIMEOUT = 320.0


def snap(tui: Tui, name: str) -> None:
    """Save the CURRENT screen as .txt and .png (a TUI screenshot)."""
    os.makedirs(OUT_DIR, exist_ok=True)
    text = tui.screen()
    with open(os.path.join(OUT_DIR, f"{name}.txt"), "w") as f:
        f.write(text)
    try:
        from PIL import Image, ImageDraw, ImageFont

        try:
            font = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 13)
        except OSError:
            font = ImageFont.load_default()
        lines = text.split("\n")
        cw, ch = 8, 17
        img = Image.new("RGB", (tui.cols * cw + 16, len(lines) * ch + 16), (14, 17, 22))
        draw = ImageDraw.Draw(img)
        for i, line in enumerate(lines):
            draw.text((8, 8 + i * ch), line, fill=(214, 219, 228), font=font)
        img.save(os.path.join(OUT_DIR, f"{name}.png"))
    except Exception as e:  # PNG is a bonus; the text frame is the record
        print(f"    (png render skipped: {e})")


def wait_turn_done(tui: Tui, checks: Checks, label: str) -> bool:
    """A turn ran and finished: chip shows ✎ then returns to ready."""
    saw_running = tui.wait_raw(f"◆{ENTITY} ✎", 30, f"{label}: turn starts")
    end = time.time() + TURN_TIMEOUT
    while time.time() < end:
        if f"◆{ENTITY} ready" in tui.screen():
            return checks.check(f"{label}: turn completed (chip back to ready)", True)
        tui.pump(1.0)
    checks.note(f"(chip never returned to ready; saw_running={saw_running})")
    return checks.check(f"{label}: turn completed (chip back to ready)", False)


def session(checks: Checks, label: str, message: str) -> Tui:
    bin_path, _, _ = env_config()
    tui = Tui([bin_path], label=label)
    checks.check(
        f"{label}: first paint",
        tui.wait_raw("AbstractCode", 20, f"{label} paint"),
    )
    tui.type_line(f"/brain {ENTITY}")
    checks.check(
        f"{label}: flow-brain conversation opened (teaching line)",
        tui.wait_raw("each message is one door summon", 10, f"{label} open"),
    )
    snap(tui, f"{label}-1-opened")
    tui.type_line(message)
    checks.check(
        f"{label}: message rendered",
        tui.wait_raw(message[:40], 10, f"{label} echo"),
    )
    ok = wait_turn_done(tui, checks, label)
    tui.pump(1.0)
    snap(tui, f"{label}-2-reply")
    if not ok:
        print("    --- current screen ---")
        print(tui.screen())
    return tui


def close_and_quit(tui: Tui, checks: Checks, label: str) -> None:
    tui.type_line("/end")
    checks.check(
        f"{label}: local end note",
        tui.wait_raw("memory of it persists", 8, f"{label} end"),
    )
    snap(tui, f"{label}-3-ended")
    tui.type_line("/quit")
    tui.pump(1.0)
    tui.ensure_dead()


def main() -> int:
    checks = Checks("flowbrain-proof")
    os.makedirs(OUT_DIR, exist_ok=True)

    print(f"== session 1: teach the fact (entity: {ENTITY})")
    t1 = session(checks, "teach", TEACH)
    close_and_quit(t1, checks, "teach")

    print("== session 2: FRESH process, fresh session id — recall")
    t2 = session(checks, "recall", RECALL_ASK)
    recalled = "kestrel" in t2.raw().lower()
    checks.check(
        "recall: the token came back from the entity's graph "
        f"({TOKEN_PHRASE!r} fragment present)",
        recalled,
    )
    if not recalled:
        print("    --- raw tail ---")
        print(t2.raw()[-2000:])
    close_and_quit(t2, checks, "recall")

    print(f"frames: {OUT_DIR}")
    return checks.verdict()


if __name__ == "__main__":
    sys.exit(main())
