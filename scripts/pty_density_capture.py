#!/usr/bin/env python3
"""Cycle-2 presence/density live proof (reviewer 2): pty + pyte at 120x36.

Two phases against the LIVE gateway:
  idle  — boot with fresh prefs/session: the IDLE-1 fact card renders
          (workflow/route/session/gateway/skills/mcp/context rows), the
          footer carries the right cluster, the wordmark appears ONCE.
  run   — one tiny agent run ("write the word hi to hi.txt"): the
          OBS-1a-live strip names the in-flight model call from second
          zero ("model call Ns"), the approval prompt is answered with
          `a`, the final answer lands, and the file exists on disk with
          mtime after the run started (structure-gated, never
          model-computed content — pty smoke lesson).

Needle discipline: no success needle appears in the typed prompt, and
nothing gates on model-chosen words.

Evidence frames -> untracked/cycle2-presence/.

Env: ACODE_GATEWAY_TOKEN (required), ACODE_GATEWAY_URL, ACODE_TUI_BIN,
     ACODE_PROVIDER, ACODE_MODEL.
Exit: 0 pass, 1 fail, 2 config error.
"""

import fcntl
import os
import pty
import select
import struct
import sys
import termios
import time

try:
    import pyte
except ImportError:
    print("pyte required", file=sys.stderr)
    sys.exit(2)

TOKEN = os.environ.get("ACODE_GATEWAY_TOKEN", "")
URL = os.environ.get("ACODE_GATEWAY_URL", "http://127.0.0.1:8080").rstrip("/")
BIN = os.environ.get("ACODE_TUI_BIN", "target/release/abstractcode-tui")
PROVIDER = os.environ.get("ACODE_PROVIDER", "lmstudio")
MODEL = os.environ.get("ACODE_MODEL", "qwen/qwen3.6-35b-a3b")
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EVIDENCE = os.path.join(REPO, "untracked", "cycle2-presence")
WORKSPACE = "/tmp/acode-density-ws"
COLS, ROWS = 120, 36
ENTER = b"\r"
CTRL_C = b"\x03"


class Driver:
    def __init__(self, session_id, prefs_path):
        os.makedirs(EVIDENCE, exist_ok=True)
        self.raw = bytearray()
        self.screen = pyte.Screen(COLS, ROWS)
        self.stream = pyte.ByteStream(self.screen)
        self.failures = 0
        self.snap_n = 0
        cmd = [
            os.path.abspath(BIN),
            "--gateway", URL,
            "--token", TOKEN,
            "--workflow", "basic-agent",
            "--provider", PROVIDER,
            "--model", MODEL,
            "--session", session_id,
            "--workspace", WORKSPACE,
        ]
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            env = dict(os.environ)
            env["TERM"] = "xterm-256color"
            env["ABSTRACTCODE_TUI_PREFS_FILE"] = prefs_path
            os.execvpe(cmd[0], cmd, env)
            os._exit(127)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

    def pump(self, seconds):
        end = time.time() + seconds
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.1)
            if self.fd in r:
                try:
                    chunk = os.read(self.fd, 65536)
                except OSError:
                    return
                if not chunk:
                    return
                self.raw.extend(chunk)
                self.stream.feed(chunk)

    def screen_text(self):
        return "\n".join(self.screen.display)

    def check(self, label, ok):
        print(f"  {'PASS' if ok else 'FAIL'}  {label}", flush=True)
        if not ok:
            self.failures += 1
        return ok

    def wait_screen(self, needle, timeout, label=None):
        end = time.time() + timeout
        while time.time() < end:
            if needle in self.screen_text():
                return self.check(label or f"screen: {needle!r}", True)
            self.pump(0.2)
        return self.check(label or f"screen: {needle!r}", False)

    def type_line(self, text):
        for ch in text:
            os.write(self.fd, ch.encode())
            time.sleep(0.005)
        time.sleep(0.15)
        os.write(self.fd, ENTER)

    def snap(self, label):
        self.snap_n += 1
        path = os.path.join(EVIDENCE, f"frame-{self.snap_n:02d}-{label}.txt")
        with open(path, "w", encoding="utf-8") as f:
            f.write(self.screen_text())
        print(f"  · frame {os.path.basename(path)}", flush=True)
        return path

    def quit(self):
        os.write(self.fd, CTRL_C)
        end = time.time() + 8
        while time.time() < end:
            done, status = os.waitpid(self.pid, os.WNOHANG)
            if done:
                return self.check(
                    "clean exit code 0", os.waitstatus_to_exitcode(status) == 0
                )
            self.pump(0.2)
        os.kill(self.pid, 9)
        return self.check("clean exit code 0", False)


def main() -> int:
    if not TOKEN:
        print("ACODE_GATEWAY_TOKEN required", file=sys.stderr)
        return 2
    os.makedirs(WORKSPACE, exist_ok=True)
    target = os.path.join(WORKSPACE, "hi.txt")
    if os.path.exists(target):
        os.unlink(target)
    prefs = f"/tmp/acode-density-prefs-{os.getpid()}.json"
    session = f"acode-density-{int(time.time())}"
    d = Driver(session, prefs)
    start = time.time()

    print("phase 1: idle fact card @120x36")
    d.pump(2.0)
    d.wait_screen("describe a task below", 10, "idle guidance line")
    scr = d.screen_text()
    for needle in [
        "workflow",
        "route",
        # This launch passes --provider/--model, so the route line shows
        # the explicit pair — "gateway defaults" only renders when BOTH
        # are absent (route_label contract).
        PROVIDER,
        "session",
        "connected",
        "window not declared",
        "127.0.0.1:8080",
    ]:
        d.check(f"card/footer carries {needle!r}", needle in scr)
    d.check(
        "wordmark exactly once (IDLE-1)",
        scr.count("▲ AbstractCode") == 1,
    )
    d.snap("idle-card")
    if "--idle-only" in sys.argv:
        d.quit()
        if os.path.exists(prefs):
            os.unlink(prefs)
        print("RESULT:", "PASS" if d.failures == 0 else f"FAIL ({d.failures})")
        return 0 if d.failures == 0 else 1

    print("phase 2: live run — strip ticker + approval + file proof")
    d.type_line("write the word hi to hi.txt")
    # The OBS-1a-live segment must name the in-flight call from second
    # zero — poll the SCREEN for the ticker while the run starts.
    d.wait_screen("model call", 45, "activity strip names the model call")
    d.snap("mid-run-strip")
    # First write on fresh prefs (tier=read): the approval prompt opens.
    if d.wait_screen("approve (a)", 90, "approval prompt opens"):
        d.snap("approval")
        os.write(d.fd, b"a")
    d.wait_screen("assistant", 180, "final answer card lands")
    d.pump(1.0)
    d.snap("answered")
    # File proof: the gateway's server-side workspace policy CLAMPS the
    # client-suggested root — the write lands in the gateway's per-run
    # workspace (live-verified: run 0a8f87bf wrote
    # runtime/workspaces/<id>/hi.txt while /tmp/acode-density-ws stayed
    # empty). Search both roots for a hi.txt fresher than run start.
    candidates = [target]
    gw_ws = os.path.join(os.path.dirname(REPO), "runtime", "workspaces")
    if os.path.isdir(gw_ws):
        for entry in os.listdir(gw_ws):
            candidates.append(os.path.join(gw_ws, entry, "hi.txt"))
    ok_file = any(
        os.path.exists(p) and os.path.getmtime(p) >= start - 1 for p in candidates
    )
    d.check("hi.txt written after run start (client or gateway workspace)", ok_file)
    # Footer instruments after a completed call: session tokens present.
    scr = d.screen_text()
    d.check("footer carries session tokens", "tk session" in scr)
    d.quit()

    if os.path.exists(prefs):
        os.unlink(prefs)
    print("RESULT:", "PASS" if d.failures == 0 else f"FAIL ({d.failures})")
    return 0 if d.failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
