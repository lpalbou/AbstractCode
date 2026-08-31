#!/usr/bin/env python3
"""SUPERSEDED (abstracttui 0.2.6, 2026-07-23): this harness verified the
app-side veil/heal machinery — the ~5s `heal_chrome_rows` chrome
heartbeat and the translucent-veil Ctrl+L — which is DELETED. The engine
now owns the whole job (our 0299 filing): Ctrl+L / `/redraw` call
`abstracttui::app::request_full_redraw()` (poison-prev + presenter-
invalidate, images re-place, the transcript pane heals too), and
`set_redraw_on_focus_gained(true)` at boot auto-heals an externally
cleared screen at the next focus round-trip — so there is no heartbeat
to verify and its step 2/3 assertions would fail against a healthy app.
Kept for the record of the original defect + method; a fresh live proof
of the engine-owned heal should assert Ctrl+L full-frame recovery and
the focus-in redraw instead.

Original header — HDR-2 live proof: external screen clear -> heartbeat +
Ctrl+L recovery.

The defect (maintainer's blank-screenshot, review-current-state §2): the
engine repaints only DAMAGED cells and diffs against its model of the
terminal — after an EXTERNAL clear (Cmd+K class) the model still believes
the old cells, so byte-identical repaints emit NOTHING and the screen
stays blank forever. This harness reproduces the wipe faithfully: the
clear is fed to the PYTE SCREEN ONLY (our terminal model), never through
the app — exactly what Cmd+K does to a real emulator.

Proved, against the LIVE gateway with a real run active (pre-0.2.6):
  1. NEGATIVE — for ~3s after the wipe the header stays blank while the
     run ticks (dyn re-runs alone cannot re-emit byte-identical cells);
  2. HEARTBEAT — within ~8s the chrome band (header + strip + composer +
     status) self-heals with NO user input (heal_chrome_rows @ 5s);
  3. SCOPE HONESTY — the transcript pane does NOT heal from the
     heartbeat (chrome-band-only by design: cost + protocol images);
  4. Ctrl+L — a second wipe, then one keystroke restores the FULL frame.

Env: ACODE_GATEWAY_TOKEN (required), ACODE_GATEWAY_URL, ACODE_TUI_BIN,
     ACODE_PROVIDER, ACODE_MODEL.
Evidence frames land in untracked/lane_c_redraw/.
Exit: 0 pass, 1 fail, 2 config error.
"""

import sys as _sys

print(
    "SUPERSEDED: the chrome heartbeat this harness verifies was deleted "
    "with abstracttui 0.2.6 (engine-owned request_full_redraw + "
    "redraw-on-focus-gained). See the module docstring.",
    file=_sys.stderr,
)
# 3, not 2: the original contract reserves 2 for "config error" — a
# superseded harness is a distinct outcome.
_sys.exit(3)

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
URL = os.environ.get("ACODE_GATEWAY_URL", "http://127.0.0.1:8080")
BIN = os.environ.get("ACODE_TUI_BIN", "target/release/abstractcode-tui")
PROVIDER = os.environ.get("ACODE_PROVIDER", "lmstudio")
MODEL = os.environ.get("ACODE_MODEL", "qwen/qwen3.6-35b-a3b")
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EVIDENCE = os.path.join(REPO, "untracked", "lane_c_redraw")
COLS, ROWS = 120, 36

if not TOKEN:
    print("ACODE_GATEWAY_TOKEN required", file=sys.stderr)
    sys.exit(2)


class Session:
    def __init__(self):
        os.makedirs(EVIDENCE, exist_ok=True)
        self.screen = pyte.Screen(COLS, ROWS)
        self.stream = pyte.ByteStream(self.screen)
        self.checks = []
        self.snap_n = 0
        prefs = f"/tmp/acode-lane-c-prefs-{os.getpid()}.json"
        cmd = [
            os.path.abspath(BIN),
            "--gateway", URL,
            "--token", TOKEN,
            "--workflow", "basic-agent",
            "--provider", PROVIDER,
            "--model", MODEL,
            "--session", f"acode-lanec-redraw-{int(time.time())}",
            "--workspace", "/tmp/acode-lane-c-ws",
        ]
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            env = dict(os.environ)
            env["TERM"] = "xterm-256color"
            env["COLORTERM"] = "truecolor"
            env["ABSTRACTCODE_TUI_PREFS_FILE"] = prefs
            os.execvpe(cmd[0], cmd, env)
            os._exit(127)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

    def pump(self, seconds):
        end = time.time() + seconds
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.15)
            if self.fd in r:
                try:
                    chunk = os.read(self.fd, 65536)
                except OSError:
                    return
                if not chunk:
                    return
                self.stream.feed(chunk)

    def text(self):
        return "\n".join(self.screen.display)

    def row(self, y):
        return self.screen.display[y]

    def send(self, data):
        os.write(self.fd, data)

    def wipe(self):
        # The EXTERNAL clear: terminal-side only (Cmd+K class). The app
        # never sees these bytes — its model keeps believing the old
        # cells, which is the whole defect.
        self.stream.feed(b"\x1b[2J\x1b[H")

    def blank_rows(self):
        return sum(1 for r in self.screen.display if not r.strip())

    def snap(self, label):
        self.snap_n += 1
        path = os.path.join(EVIDENCE, f"{self.snap_n:02d}-{label}.txt")
        with open(path, "w", encoding="utf-8") as f:
            f.write(f"[blank rows: {self.blank_rows()}/{ROWS}]\n")
            for i, r in enumerate(self.screen.display):
                f.write(f"{i:3d} |{r}|\n")
        print(f"  · snap {os.path.basename(path)} (blank {self.blank_rows()}/{ROWS})")

    def check(self, label, ok):
        self.checks.append((label, bool(ok)))
        print(f"  {'PASS' if ok else 'FAIL'}  {label}", flush=True)
        return ok

    def wait_screen(self, needle, timeout, label):
        end = time.time() + timeout
        while time.time() < end:
            if needle in self.text():
                return self.check(label, True)
            self.pump(0.25)
        return self.check(label, False)

    def quit(self):
        self.send(b"\x03")
        end = time.time() + 8
        while time.time() < end:
            done, status = os.waitpid(self.pid, os.WNOHANG)
            if done:
                return os.waitstatus_to_exitcode(status)
            self.pump(0.2)
        os.kill(self.pid, 9)
        return -9


def main():
    s = Session()
    s.pump(2.0)
    s.wait_screen("AbstractCode", 15, "TUI booted (wordmark on screen)")
    s.pump(3.0)  # let catalog/route resolution and first paints settle
    s.snap("settled")

    # A run long enough to straddle the wipe + heartbeat window: one
    # long generation, no tools (avoids approval modals in the timing
    # path — belt below approves if one appears anyway).
    prompt = (
        "Explain TCP slow start in detail, at least 600 words. "
        "Do not use any tools; just answer directly."
    )
    s.send(prompt.encode())
    s.pump(0.4)
    s.send(b"\r")
    print("  · prompt sent (live run starting)")
    ok_running = s.wait_screen("working", 60, "run is active (strip shows working)")
    if not ok_running:
        s.snap("no-run")
        return finish(s)
    if "tool approval" in s.text():
        s.send(b"a")  # belt: approve and continue the timing test
        s.pump(1.0)
    s.snap("running-before-wipe")

    # ---- the external wipe, mid-run ------------------------------------
    s.wipe()
    t0 = time.time()
    s.snap("wiped")
    s.check("wipe left the screen blank", s.blank_rows() >= ROWS - 2)

    # NEGATIVE (3s): the run ticker re-runs the header dyn ~25 times in
    # this window; none of it may re-emit the byte-identical header. Only
    # the spinner/elapsed/strip cells (values that actually change) and
    # transcript stream cells may reappear.
    s.pump(max(0.0, 3.0 - (time.time() - t0)))
    hdr = s.row(0)
    s.check(
        "t+3s: header row still blank (dyn re-runs alone cannot heal)",
        "AbstractCode" not in hdr,
    )
    s.snap("t3s-still-blank")

    # HEARTBEAT (by t+9s: 5s period + one tick + frame slack): the chrome
    # band re-emits with NO input.
    healed = False
    while time.time() - t0 < 9.5:
        if "AbstractCode" in s.row(0):
            healed = True
            break
        s.pump(0.25)
    s.check("heartbeat healed the header row without input", healed)
    # Status row (bottom) carries theme + gateway host again.
    deadline = time.time() + 2
    status_ok = False
    while time.time() < deadline:
        if "127.0.0.1:8080" in s.row(ROWS - 1) or "?" in s.row(ROWS - 1):
            status_ok = True
            break
        s.pump(0.2)
    s.check("heartbeat healed the status row", status_ok)
    s.snap("after-heartbeat")
    # SCOPE HONESTY: the user card lives in the transcript pane (top
    # third) — the heartbeat must NOT have repainted it.
    pane = "\n".join(s.screen.display[1 : ROWS - 4])
    s.check(
        "transcript pane deliberately NOT healed by the heartbeat",
        "TCP slow start" not in pane,
    )

    # ---- Ctrl+L: full-frame recovery ------------------------------------
    s.wipe()
    s.pump(0.3)
    s.send(b"\x0c")  # Ctrl+L
    s.pump(1.5)
    s.snap("after-ctrl-l")
    s.check("Ctrl+L restored the header", "AbstractCode" in s.row(0))
    s.check(
        "Ctrl+L restored the transcript (user card back)",
        "TCP slow start" in s.text(),
    )
    s.check(
        "Ctrl+L restored the status row",
        "127.0.0.1:8080" in s.row(ROWS - 1) or "Dark" in s.row(ROWS - 1),
    )

    # Cancel the live run before leaving (Esc Esc), then quit.
    s.send(b"\x1b")
    s.pump(0.4)
    s.send(b"\x1b")
    s.pump(1.0)
    return finish(s)


def finish(s):
    code = s.quit()
    print(f"  · app exit code {code}")
    failed = [l for l, ok in s.checks if not ok]
    print(f"\n{'ALL PASS' if not failed else 'FAILURES: ' + ', '.join(failed)}")
    print(f"  evidence: {EVIDENCE}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
