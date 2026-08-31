#!/usr/bin/env python3
"""Live verify the failed-agent-subrun conclusion (lane A P0).

Read-only against the gateway: boots the TUI into a session whose live
run tree is the P0 shape (root `waiting` forever on its status poller,
ANSWER-SOURCE agent subrun terminally `failed`) and asserts the client
now CONCLUDES the turn (error card naming the agent terminal + composer
freed) instead of spinning forever. Sends no prompts, answers no waits.

Default session: acode-ptysmoke-1784707419 (root 76fc3fcb…, agent
9c5cad22… "Model unloaded." — see docs/roadmap/lane-a-diagnosis.md §1).

Env: ACODE_TUI_BIN, ACODE_GATEWAY_URL, ACODE_GATEWAY_TOKEN, ACODE_SESSION.
Exit: 0 = concluded honestly, 3 = reattached but still spinning, 1 = boot
failure, 2 = config error.
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
    session = os.environ.get("ACODE_SESSION", "acode-ptysmoke-1784707419")
    if not token:
        print("ACODE_GATEWAY_TOKEN required", file=sys.stderr)
        return 2

    cmd = [
        os.path.abspath(bin_path),
        "--gateway", gateway,
        "--token", token,
        "--session", session,
        "--replay-turns", "0",
    ]
    prefs_path = f"/tmp/acode-tui-failverify-prefs-{os.getpid()}.json"
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
        deadline = time.time() + 45
        concluded = False
        while time.time() < deadline:
            snap = text()
            # The conclusion card from Fold::subrun_terminal("failed").
            if "the agent run ended: failed" in snap:
                concluded = True
                break
            pump(0.5)
        snap = text()
        print("  reattach notice:", "yes" if "reattaching to live run" in snap else "NO")
        print("  provider error card:", "yes" if "Model unloaded" in snap else "no")
        print("  conclusion card:", "yes" if concluded else "NO — still spinning (the P0)")
        code = 0 if concluded else 3
        if not concluded:
            print("--- tail ---")
            print(snap[-2000:])
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
    signal.alarm(150)
    sys.exit(main())
