#!/usr/bin/env python3
"""Live pty check for the capability-visibility wave.

Drives the REAL binary against the LIVE gateway and asserts, from rendered
screens, that: the header names what "gateway defaults" resolves to, /tools
is a toggling selector, /skills lists the gateway shelf, /cache reports the
prompt-cache posture, and /sessions lists remembered sessions.

Env: ACODE_GATEWAY_TOKEN (required), ACODE_GATEWAY_URL (default local),
ACODE_TUI_BIN (default target/release/abstractcode-tui).

Prefs are ISOLATED to a temp file (ABSTRACTCODE_TUI_PREFS_FILE) — a live
check must never touch the operator's real preferences.
"""

import fcntl
import json
import os
import pty
import re
import select
import struct
import sys
import tempfile
import termios
import time

TOKEN = os.environ.get("ACODE_GATEWAY_TOKEN", "")
URL = os.environ.get("ACODE_GATEWAY_URL", "http://127.0.0.1:8080")
BIN = os.environ.get("ACODE_TUI_BIN", "target/release/abstractcode-tui")

if not TOKEN:
    print("SKIP: ACODE_GATEWAY_TOKEN not set")
    sys.exit(2)

ANSI = re.compile(rb"\x1b\[[0-9;:?]*[a-zA-Z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[=>]|\x1b\([B0]")


def plain(buf: bytes) -> str:
    return ANSI.sub(b"", buf).decode("utf-8", "replace")


def main() -> int:
    prefs = tempfile.NamedTemporaryFile(
        mode="w", suffix=".json", prefix="acode-prefs-", delete=False
    )
    # Two remembered sessions so /sessions has something to show.
    # NOTE: the seeded last_used below is the isolation-proof sentinel; the
    # binary replaces it at boot (touch_session), which is the only write
    # the final assertion accepts as proof.
    json.dump(
        {
            "session_id": "acode-feature-check",
            "recent_sessions": [
                {"id": "acode-feature-check", "label": "current", "last_used": "2026-07-21T18:00:00Z"},
                {"id": "acode-older-one", "label": "older work", "last_used": "2026-07-20T10:00:00Z"},
            ],
        },
        prefs,
    )
    prefs.close()

    env = dict(os.environ)
    env["ABSTRACTCODE_TUI_PREFS_FILE"] = prefs.name
    env["TERM"] = "xterm-256color"

    pid, fd = pty.fork()
    if pid == 0:
        os.execvpe(
            BIN,
            [BIN, "--gateway", URL, "--token", TOKEN, "--session", "acode-feature-check"],
            env,
        )

    # Give the terminal a size BEFORE the app measures it (required for
    # first paint — the smoke script's hard-won lesson).
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))

    buf = b""

    def read_for(seconds: float) -> None:
        nonlocal buf
        end = time.time() + seconds
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.2)
            if r:
                try:
                    chunk = os.read(fd, 65536)
                except OSError:
                    return
                if not chunk:
                    return
                buf += chunk

    def type_line(text: str) -> None:
        for ch in text:
            os.write(fd, ch.encode())
            time.sleep(0.01)
        time.sleep(0.15)
        os.write(fd, b"\r")

    checks: list[tuple[str, bool]] = []

    def check(label: str, needle: str, timeout: float = 12.0) -> None:
        nonlocal buf
        end = time.time() + timeout
        while time.time() < end:
            if needle in plain(buf):
                checks.append((label, True))
                return
            read_for(0.4)
        checks.append((label, False))
        print(f"  MISSING needle: {needle!r}")

    read_for(4.0)
    # 1. Header names the resolved default route (capability defaults fetch).
    # NEEDLE NOTE: damage-tracked repaints emit only CHANGED cells, so a
    # full-line needle can never match an update — assert on the payload
    # substring that arrives with the route repaint.
    check("header resolves gateway defaults", "(lmstudio", 20.0)

    # 2. /tools selector with toggle affordances. Boot discovery serializes
    # behind the providers probe (~5s); wait for a checked row before
    # toggling — Space on a still-loading inventory is a deliberate no-op.
    type_line("/tools")
    check("tools selector opens", "gateway tools —", 8.0)
    check("tools inventory rows", "[✓]", 30.0)
    check("tools toggle hint", "Space toggles", 4.0)
    # Toggle the first tool OFF. The title repaint is damage-fragmented in
    # the raw stream, so the assertion is the DURABLE proof: the isolated
    # prefs file must carry exactly one disabled tool at exit.
    os.write(fd, b" ")
    read_for(1.0)
    os.write(fd, b"\x1b")  # esc closes
    read_for(0.6)

    # 3. /skills shelf from the live gateway.
    type_line("/skills")
    check("skills shelf", "on the shelf", 10.0)
    os.write(fd, b"\x1b")
    read_for(0.6)

    # 4. /cache posture for the effective route.
    type_line("/cache")
    check("cache modal", "prompt cache + context", 8.0)
    check("cache posture", "cache      supported", 8.0)
    os.write(fd, b"\x1b")
    read_for(0.6)

    # 5. /sessions listing the remembered sessions.
    type_line("/sessions")
    check("sessions picker", "older work", 8.0)
    os.write(fd, b"\x1b")
    read_for(0.6)

    # Quit.
    os.write(fd, b"\x11")  # Ctrl+Q
    read_for(1.0)
    try:
        os.kill(pid, 15)
    except ProcessLookupError:
        pass

    # Durable proofs from the isolated prefs file: the toggle persisted,
    # and every write stayed out of the operator's real preferences.
    # The isolation proof must assert something ONLY THE BINARY writes —
    # asserting the seeded session id back was a tautology that passed even
    # with isolation broken (adversary finding 8). The binary touches the
    # current session at boot, replacing the seeded last_used timestamp.
    with open(prefs.name, encoding="utf-8") as f:
        saved = json.load(f)
    checks.append(
        ("tools toggle persisted one disabled tool", len(saved.get("disabled_tools") or []) == 1)
    )
    seeded_ts = "2026-07-21T18:00:00Z"
    entry = next(
        (e for e in saved.get("recent_sessions") or [] if e.get("id") == "acode-feature-check"),
        None,
    )
    binary_wrote_isolated = bool(entry) and entry.get("last_used") != seeded_ts
    checks.append(("binary wrote INTO the isolated prefs file", binary_wrote_isolated))
    os.unlink(prefs.name)

    print()
    ok = True
    for label, passed in checks:
        print(f"  {'PASS' if passed else 'FAIL'}  {label}")
        ok &= passed
    print()
    print("FEATURES CHECK:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
