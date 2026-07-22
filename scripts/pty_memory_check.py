#!/usr/bin/env python3
"""Live two-turn memory proof: conversation context must survive turns.

Drives the real TUI through a pty for two turns in ONE session:
  turn 1: "Remember: my project codename is <nonce>. Reply OK."
  turn 2: "What is my project codename? Reply with just the codename."

The assertion is fragment-immune: the second turn's answer is read from the
GATEWAY (the agent subrun's flow output), never grepped from the raw pty
stream. Env: ACODE_GATEWAY_TOKEN (required), ACODE_TUI_BIN, ACODE_GATEWAY_URL,
ACODE_PROVIDER, ACODE_MODEL. Exit 0 pass / 1 fail / 2 config.
"""

import fcntl
import json
import os
import pty
import re
import secrets
import select
import signal
import struct
import subprocess
import sys
import termios
import time
import urllib.request

ANSI = re.compile(
    rb"\x1b\[[0-9;:?<=>]*[a-zA-Z@`~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[_P^][^\x1b]*\x1b\\|\x1b[=>NOPZ78cM]|\x1b\([B0]"
)
COLS, ROWS = 110, 32


def gw_json(base, token, path):
    req = urllib.request.Request(
        f"{base}{path}", headers={"Authorization": f"Bearer {token}"}
    )
    return json.load(urllib.request.urlopen(req, timeout=10))


def main() -> int:
    bin_path = os.path.abspath(os.environ.get("ACODE_TUI_BIN", "target/release/abstractcode-tui"))
    gateway = os.environ.get("ACODE_GATEWAY_URL", "http://127.0.0.1:8080")
    token = os.environ.get("ACODE_GATEWAY_TOKEN", "")
    provider = os.environ.get("ACODE_PROVIDER", "lmstudio")
    model = os.environ.get("ACODE_MODEL", "qwen/qwen3.6-35b-a3b")
    if not token:
        print("ACODE_GATEWAY_TOKEN required", file=sys.stderr)
        return 2

    nonce = f"zephyr{secrets.token_hex(3)}"
    session = f"acode-memcheck-{int(time.time())}"
    cmd = [
        bin_path,
        "--gateway", gateway, "--token", token,
        "--workflow", "basic-agent",
        "--provider", provider, "--model", model,
        "--session", session,
        "--no-workspace",
    ]
    # Prefs path computed in the PARENT so it can be cleaned up afterwards.
    prefs_path = f"/tmp/acode-memcheck-prefs-{os.getpid()}.json"
    pid, master = pty.fork()
    if pid == 0:
        env = dict(os.environ)
        env["TERM"] = "xterm-256color"
        env["ABSTRACTCODE_TUI_PREFS_FILE"] = prefs_path
        os.execvpe(cmd[0], cmd, env)
        os._exit(127)
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

    buf = bytearray()

    def pump(seconds):
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

    def text():
        return ANSI.sub(b"", bytes(buf)).decode("utf-8", errors="replace")

    def wait_assistant_cards(n, timeout, label):
        end = time.time() + timeout
        while time.time() < end:
            if text().count("✦ assistant") >= n:
                print(f"  ✓ {label}")
                return True
            pump(0.4)
        print(f"  ✗ {label}")
        return False

    ok = True
    try:
        pump(2.5)
        os.write(master, f"Remember: my project codename is {nonce}. Reply OK.".encode())
        pump(0.4)
        os.write(master, b"\r")
        ok &= wait_assistant_cards(1, 120, "turn 1 answered")

        os.write(master, b"What is my project codename? Reply with just the codename.")
        pump(0.4)
        os.write(master, b"\r")
        ok &= wait_assistant_cards(2, 120, "turn 2 answered")
        pump(1.0)
        os.write(master, b"\x03")
        pump(1.0)
    finally:
        try:
            os.kill(pid, signal.SIGKILL)
        except Exception:
            pass
        try:
            os.waitpid(pid, 0)
        except Exception:
            pass
        try:
            os.unlink(prefs_path)
        except OSError:
            pass
        os.close(master)

    if not ok:
        print(text()[-1500:])
        return 1

    # Structural verdict from the gateway: the newest run's agent answer.
    runs = gw_json(gateway, token, f"/api/gateway/runs?limit=5&root_only=true&session_id={session}")
    newest = runs["items"][0]["run_id"]
    ledger = gw_json(gateway, token, f"/api/gateway/runs/{newest}/ledger?after=0&limit=100")
    subs = []
    for rec in ledger.get("items", []):
        det = ((rec.get("result") or {}).get("wait") or {}).get("details") or {}
        if det.get("sub_run_id"):
            subs.append(det["sub_run_id"])
    answer = ""
    for sub in subs:
        sl = gw_json(gateway, token, f"/api/gateway/runs/{sub}/ledger?after=0&limit=200")
        for rec in sl.get("items", []):
            out = (rec.get("result") or {}).get("output")
            if isinstance(out, dict) and out.get("answer"):
                answer = out["answer"]
    print(f"  turn-2 answer (from the gateway ledger): {answer!r}")
    if nonce in answer:
        print(f"MEMORY CHECK: PASS (codename {nonce} recalled across turns)")
        return 0
    print(f"MEMORY CHECK: FAIL (codename {nonce} not in the answer)")
    return 1


if __name__ == "__main__":
    signal.signal(signal.SIGALRM, lambda *_: sys.exit(3))
    signal.alarm(330)
    sys.exit(main())
