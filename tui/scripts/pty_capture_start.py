#!/usr/bin/env python3
"""Drive the TUI against the capture proxy and dump the /runs/start body.

Zero LLM budget: the proxy 503s the start, so no run is created. Proves
what the client SERIALIZES (workspace_allowed_paths, _runtime.tool_policy)
at a given tier — the surface the gateway /input_data echo normalizes.

Env: ACODE_TUI_BIN, CAPTURE_PORT, ACODE_TIER (all|read|write), ACODE_OUT.
"""

import fcntl
import json
import os
import pty
import signal
import struct
import subprocess
import sys
import termios
import time

BIN = os.environ["ACODE_TUI_BIN"]
PORT = os.environ.get("CAPTURE_PORT", "8899")
TIER = os.environ.get("ACODE_TIER", "all")
OUT = os.environ.get("ACODE_OUT", "/tmp/start-body.json")
STATE = os.environ.get("ACODE_C3B_STATE", "/tmp/acode-c3b-state")
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main():
    if os.path.exists(OUT):
        os.unlink(OUT)
    prefs_path = os.path.join(STATE, f"prefs-capture-{TIER}.json")
    with open(prefs_path, "w", encoding="utf-8") as f:
        json.dump(
            {
                "session_id": "acode-capture",
                "workspace_mode": "workspace_or_allowed",
                "workspace_allowed": ["/tmp"],
                "tool_approval": {"accepted_tier": TIER},
            },
            f,
        )

    proxy = subprocess.Popen(
        [sys.executable, os.path.join(REPO, "scripts", "capture_start_body.py"), OUT],
        env={**os.environ, "CAPTURE_PORT": PORT},
    )
    time.sleep(0.8)

    pid, fd = pty.fork()
    if pid == 0:
        env = dict(os.environ)
        env["TERM"] = "xterm-256color"
        env["ABSTRACTCODE_TUI_PREFS_FILE"] = prefs_path
        cmd = [
            os.path.abspath(BIN),
            "--gateway", f"http://127.0.0.1:{PORT}",
            "--token", "x",
            "--workflow", "basic-agent",
            "--provider", "lmstudio",
            "--model", "qwen/qwen3.6-35b-a3b",
            "--session", "acode-capture",
            "--workspace", os.path.join(STATE, "ws"),
            "--workspace-mode", "workspace_or_allowed",
        ]
        os.execvpe(cmd[0], cmd, env)
        os._exit(127)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 42, 160, 0, 0))

    def drain(sec):
        import select

        end = time.time() + sec
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.15)
            if fd in r:
                try:
                    if not os.read(fd, 65536):
                        return
                except OSError:
                    return

    drain(4.0)  # boot: catalog/tools must load so the policy has classes
    for ch in "write the word hi to hi.txt":
        os.write(fd, ch.encode())
        time.sleep(0.006)
    time.sleep(0.2)
    os.write(fd, b"\r")
    # Wait for the capture (start posted) or timeout.
    end = time.time() + 15
    while time.time() < end and not os.path.exists(OUT):
        drain(0.4)
    drain(0.5)
    os.write(fd, b"\x03")
    time.sleep(0.5)
    try:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
    except Exception:
        pass
    os.close(fd)
    proxy.terminate()
    try:
        proxy.wait(timeout=3)
    except Exception:
        proxy.kill()

    if not os.path.exists(OUT):
        print("FAIL: no /runs/start body captured", file=sys.stderr)
        return 1
    with open(OUT, encoding="utf-8") as f:
        body = json.load(f)
    idata = body.get("input_data", {})
    runtime = idata.get("_runtime", {})
    policy = runtime.get("tool_policy", {})
    print(json.dumps(
        {
            "prompt": idata.get("prompt"),
            "workspace_access_mode": idata.get("workspace_access_mode"),
            "workspace_allowed_paths": idata.get("workspace_allowed_paths"),
            "runtime_keys": sorted(runtime.keys()),
            "tool_policy": policy,
        },
        indent=1,
    ))
    return 0


if __name__ == "__main__":
    signal.signal(signal.SIGALRM, lambda *_: sys.exit(3))
    signal.alarm(60)
    sys.exit(main())
