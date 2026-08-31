#!/usr/bin/env python3
"""Live pty smoke: drive the real TUI against a running gateway.

Forks the TUI under a REAL controlling terminal (pty.fork sets the ctty;
TIOCSWINSZ gives it a size — both required for first paint), sends a
prompt that forces a write_file approval, approves it with `a`, waits for
the final answer, then quits with Ctrl+C. Assertions run against the
ANSI-stripped accumulated output (coarse needles only — raw pty streams
fragment arbitrarily).

Env: ACODE_TUI_BIN, ACODE_GATEWAY_URL, ACODE_GATEWAY_TOKEN,
     ACODE_PROVIDER, ACODE_MODEL.
Exit: 0 pass, 1 fail (prints the transcript tail), 2 config error.
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

COLS, ROWS = 110, 32


def main() -> int:
    bin_path = os.environ.get("ACODE_TUI_BIN", "target/debug/abstractcode")
    gateway = os.environ.get("ACODE_GATEWAY_URL", "http://127.0.0.1:8080")
    token = os.environ.get("ACODE_GATEWAY_TOKEN", "")
    provider = os.environ.get("ACODE_PROVIDER", "lmstudio")
    model = os.environ.get("ACODE_MODEL", "qwen/qwen3.6-35b-a3b")
    if not token:
        print("ACODE_GATEWAY_TOKEN required", file=sys.stderr)
        return 2

    cmd = [
        os.path.abspath(bin_path),
        "--gateway", gateway,
        "--token", token,
        "--workflow", "basic-agent",
        "--provider", provider,
        "--model", model,
        "--session", f"acode-ptysmoke-{int(time.time())}",
        "--workspace", "/tmp/acode-tui-live",
    ]

    # Prefs path computed in the PARENT so it can be cleaned up afterwards
    # (the old child-pid-derived name leaked one temp file per run).
    prefs_path = f"/tmp/acode-tui-smoke-prefs-{os.getpid()}.json"
    pid, master = pty.fork()
    if pid == 0:  # child: exec the TUI on the fresh ctty
        env = dict(os.environ)
        env["TERM"] = "xterm-256color"
        env["ABSTRACTCODE_PREFS_FILE"] = prefs_path
        os.execvpe(cmd[0], cmd, env)
        os._exit(127)

    # Give the terminal a size BEFORE the app measures it.
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

    buf = bytearray()
    start_ts = time.time()
    deadline = time.time() + 240

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

    def wait_for(needle: str, timeout: float, label: str) -> bool:
        end = time.time() + timeout
        while time.time() < end and time.time() < deadline:
            if needle in text():
                print(f"  ✓ {label}")
                return True
            pump(0.3)
        print(f"  ✗ {label} (needle {needle!r} not seen)")
        return False

    def send(data: bytes) -> None:
        os.write(master, data)

    ok = True
    exited = False
    try:
        pump(2.0)
        ok &= wait_for("AbstractCode", 15, "TUI booted with wordmark")
        ok &= wait_for("session", 10, "session line visible")

        # Success is judged STRUCTURALLY (final assistant card + the tool
        # write proven on disk) — never on model-computed strings: a model
        # can misspell a derived token while doing the task perfectly
        # (live lesson), and prompt echoes self-match.
        prompt = "Create a file named pty-proof.txt containing exactly 'pty smoke', then confirm you are done."
        send(prompt.encode())
        pump(0.5)
        send(b"\r")
        print("  · prompt sent")

        ok &= wait_for("tool approval", 150, "approval modal appeared")
        pump(0.5)
        send(b"a")
        print("  · approved with 'a'")

        # The final answer card renders with the "✦ assistant" header. The
        # client retries transient resume failures itself; re-approve ONLY
        # if the approval modal is visibly back.
        answered = False
        approvals_seen = text().count("tool approval")
        for _round in range(3):
            end = time.time() + 90
            while time.time() < end:
                snapshot = text()
                if "✦ assistant" in snapshot or "✦ assistant" in snapshot.replace("\n", ""):
                    answered = True
                    break
                if snapshot.count("tool approval") > approvals_seen:
                    approvals_seen = snapshot.count("tool approval")
                    print("  · approval modal reopened; approving again")
                    send(b"a")
                pump(0.4)
            if answered:
                break
        if answered:
            print("  ✓ final assistant answer rendered")
        else:
            print("  ✗ final assistant answer rendered")
            ok = False

        # Filesystem proof (fragment-immune): the approved write really
        # happened, in the gateway-managed workspace.
        import glob
        proof = None
        for path in glob.glob("/Users/albou/tmp/abstractframework/runtime/workspaces/*/pty-proof.txt"):
            # Strictly THIS run's write: older smoke leftovers must not alias.
            if os.path.getmtime(path) > start_ts:
                proof = path
                break
        if proof and open(proof).read().strip() == "pty smoke":
            print(f"  ✓ tool write proven on disk ({proof.split('/')[-2][:8]}…)")
        else:
            print("  ✗ tool write not found on disk")
            ok = False
        pump(1.0)

        send(b"\x03")  # Ctrl+C quits
        end = time.time() + 6
        while time.time() < end:
            done, status = os.waitpid(pid, os.WNOHANG)
            if done:
                exited = True
                code = os.waitstatus_to_exitcode(status)
                print(f"  ✓ clean exit on Ctrl+C (code {code})")
                ok &= code == 0
                break
            pump(0.2)
        if not exited:
            print("  ✗ did not exit on Ctrl+C")
            ok = False
    finally:
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

    if not ok:
        print("--- transcript tail ---")
        print(text()[-3000:])
        return 1
    print("PTY LIVE SMOKE: PASS")
    return 0


if __name__ == "__main__":
    signal.signal(signal.SIGALRM, lambda *_: sys.exit(3))
    signal.alarm(330)
    sys.exit(main())
