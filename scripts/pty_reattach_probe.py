#!/usr/bin/env python3
"""Read-only reattach probe: boot the TUI into an existing session and
report what the screen claims about the run state.

Diagnostic for the "agent never finishes" P0 (lane A, 2026-07-23): a
session whose live run tree is parked on a deep-subrun tool approval must
re-surface that approval after a client restart. This script sends NO
approval keys and NO prompts — it only boots, waits, and greps the
ANSI-stripped output for the approval modal + activity strip, then quits.

Env: ACODE_TUI_BIN, ACODE_GATEWAY_URL, ACODE_GATEWAY_TOKEN, ACODE_SESSION.
Exit: 0 = approval surfaced, 3 = reattached but NO approval visible,
      2 = config error, 1 = boot failure.
"""

import fcntl
import os
import pty
import re
import select
import signal
import struct
import sys
import termios
import time

ANSI = re.compile(
    rb"\x1b\[[0-9;:?<=>]*[a-zA-Z@`~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[_P^][^\x1b]*\x1b\\|\x1b[=>NOPZ78cM]|\x1b\([B0]"
)

COLS, ROWS = 120, 36


def main() -> int:
    bin_path = os.environ.get("ACODE_TUI_BIN", "target/release/abstractcode-tui")
    gateway = os.environ.get("ACODE_GATEWAY_URL", "http://127.0.0.1:8080")
    token = os.environ.get("ACODE_GATEWAY_TOKEN", "")
    session = os.environ.get("ACODE_SESSION", "")
    watch_secs = float(os.environ.get("ACODE_WATCH_SECS", "45"))
    if not token or not session:
        print("ACODE_GATEWAY_TOKEN and ACODE_SESSION required", file=sys.stderr)
        return 2

    cmd = [
        os.path.abspath(bin_path),
        "--gateway", gateway,
        "--token", token,
        "--session", session,
        # No prior-turn rehydration: this probe watches the LIVE attach only
        # (rehydrating 20 turns of 19MB coder bundles would dominate the
        # probe window without changing what it measures).
        "--replay-turns", "0",
    ]
    prefs_path = f"/tmp/acode-tui-reattach-prefs-{os.getpid()}.json"
    pid, master = pty.fork()
    if pid == 0:
        env = dict(os.environ)
        env["TERM"] = "xterm-256color"
        env["ABSTRACTCODE_TUI_PREFS_FILE"] = prefs_path
        os.execvpe(cmd[0], cmd, env)
        os._exit(127)

    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    buf = bytearray()

    def pump(seconds: float) -> None:
        end = time.time() + seconds
        while time.time() < end:
            r, _, _ = select.select([master], [], [], 0.2)
            if master in r:
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    return
                if not chunk:
                    return
                buf.extend(chunk)

    def text() -> str:
        return ANSI.sub(b"", bytes(buf)).decode("utf-8", errors="replace")

    exited = False
    code = 1
    try:
        pump(3.0)
        if "AbstractCode" not in text():
            pump(8.0)
        if "AbstractCode" not in text():
            print("✗ TUI did not boot")
            return 1
        print("✓ TUI booted")
        deadline = time.time() + watch_secs
        surfaced = False
        reattached = False
        while time.time() < deadline:
            snap = text()
            if not reattached and "reattaching to live run" in snap:
                reattached = True
                print("✓ reattach notice seen")
            if "tool approval" in snap or "awaiting approval" in snap:
                surfaced = True
                break
            pump(0.5)
        snap = text()
        print(f"— after {watch_secs:.0f}s watch —")
        print("  reattach notice:", "yes" if "reattaching to live run" in snap else "NO")
        print("  approval visible:", "yes" if surfaced else "NO")
        for needle in ["waiting for tool approval", "execute_command", "cargo test"]:
            print(f"  contains {needle!r}:", "yes" if needle in snap else "no")
        code = 0 if surfaced else 3
        print("--- tail ---")
        print(snap[-2200:])
    finally:
        try:
            os.write(master, b"\x03")
        except OSError:
            pass
        end = time.time() + 5
        while time.time() < end:
            done, _status = os.waitpid(pid, os.WNOHANG)
            if done:
                exited = True
                break
            pump(0.2)
        if not exited:
            try:
                os.kill(pid, signal.SIGKILL)
                os.waitpid(pid, 0)
            except Exception:
                pass
        os.close(master)
        try:
            os.unlink(prefs_path)
        except OSError:
            pass
    return code


if __name__ == "__main__":
    signal.signal(signal.SIGALRM, lambda *_: sys.exit(3))
    signal.alarm(180)
    sys.exit(main())
