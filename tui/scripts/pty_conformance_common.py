#!/usr/bin/env python3
"""Shared harness for the interruptibility/continuation conformance pty
scenarios (`pty_conformance_t1..t6.py`).

Two halves:
- `Tui`: fork the real TUI under a pty (controlling terminal + size —
  both required for first paint), with prefs isolation per instance.
  Assertions run against BOTH the accumulated ANSI-stripped stream
  (`raw()` — catches toasts and anything that ever painted) and the
  CURRENT pyte-rendered screen (`screen()` — catches what is on screen
  NOW, which is what modal open/close proofs need: a closed modal's
  text stays in the raw stream forever).
- `Gw`: stdlib urllib gateway client mirroring the exact command shapes
  the Rust client posts (`src/gateway/mod.rs::submit_command`) so the
  scenarios can resolve waits "from outside" the way another app would.

Every scenario must cancel the runs it created (`Gw.cancel_session_runs`)
and print the run ids — live-gateway hygiene.

Env: ACODE_TUI_BIN (default target/release/abstractcode),
     ACODE_GATEWAY_URL (default http://127.0.0.1:8080),
     ACODE_GATEWAY_TOKEN (required).
"""

import fcntl
import json
import os
import pty
import re
import select
import signal
import struct
import sys
import termios
import time
import urllib.error
import urllib.request

ANSI = re.compile(
    rb"\x1b\[[0-9;:?<=>]*[a-zA-Z@`~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[_P^][^\x1b]*\x1b\\|\x1b[=>NOPZ78cM]|\x1b\([B0]"
)

COLS, ROWS = 120, 44


def env_config():
    bin_path = os.environ.get("ACODE_TUI_BIN", "target/release/abstractcode")
    gateway = os.environ.get("ACODE_GATEWAY_URL", "http://127.0.0.1:8080")
    token = os.environ.get("ACODE_GATEWAY_TOKEN", "")
    if not token:
        print("ACODE_GATEWAY_TOKEN required", file=sys.stderr)
        sys.exit(2)
    return os.path.abspath(bin_path), gateway, token


class Checks:
    """✓/✗ bookkeeping with a final verdict."""

    def __init__(self, name: str):
        self.name = name
        self.failed = []

    def check(self, label: str, ok: bool) -> bool:
        print(f"  {'✓' if ok else '✗'} {label}")
        if not ok:
            self.failed.append(label)
        return ok

    def note(self, label: str) -> None:
        print(f"  · {label}")

    def verdict(self) -> int:
        if self.failed:
            print(f"{self.name}: FAIL ({len(self.failed)} failed)")
            for f in self.failed:
                print(f"  ✗ {f}")
            return 1
        print(f"{self.name}: PASS")
        return 0


class Tui:
    """One TUI instance on its own pty. `label` names it in output."""

    def __init__(self, argv, label="tui", cols=COLS, rows=ROWS):
        import pyte

        self.label = label
        self.cols, self.rows = cols, rows
        self.buf = bytearray()
        self.exited = None  # exit code once reaped
        self._pyte_screen = pyte.Screen(cols, rows)
        self._pyte_stream = pyte.ByteStream(self._pyte_screen)
        self.prefs_path = f"/tmp/acode-conf-{label}-{os.getpid()}-{time.time_ns()}.json"
        pid, master = pty.fork()
        if pid == 0:
            env = dict(os.environ)
            env["TERM"] = "xterm-256color"
            env["ABSTRACTCODE_PREFS_FILE"] = self.prefs_path
            os.execvpe(argv[0], argv, env)
            os._exit(127)
        self.pid, self.master = pid, master
        fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

    # -- io ------------------------------------------------------------------
    def pump(self, seconds: float) -> None:
        end = time.time() + seconds
        while time.time() < end:
            r, _, _ = select.select([self.master], [], [], 0.2)
            if self.master in r:
                try:
                    chunk = os.read(self.master, 65536)
                except OSError:
                    return
                if not chunk:
                    return
                self.buf.extend(chunk)
                self._pyte_stream.feed(chunk)

    def raw(self) -> str:
        """Accumulated ANSI-stripped output (everything that ever painted)."""
        return ANSI.sub(b"", bytes(self.buf)).decode("utf-8", errors="replace")

    def screen(self) -> str:
        """The CURRENT terminal screen (pyte state) as one string."""
        return "\n".join(self._pyte_screen.display)

    def send(self, data: bytes) -> None:
        os.write(self.master, data)

    def type_line(self, text: str) -> None:
        self.send(text.encode())
        self.pump(0.4)
        self.send(b"\r")

    # -- waiting -------------------------------------------------------------
    def wait_raw(self, needle: str, timeout: float, label: str = "") -> bool:
        end = time.time() + timeout
        while time.time() < end:
            if needle in self.raw():
                return True
            self.pump(0.3)
        if label:
            print(f"    (wait_raw timed out: {label!r} — needle {needle!r})")
        return False

    def wait_screen_gone(self, needle: str, timeout: float) -> bool:
        """Wait until `needle` is ABSENT from the current screen."""
        end = time.time() + timeout
        while time.time() < end:
            if needle not in self.screen():
                return True
            self.pump(0.3)
        return False

    def wait_screen(self, needle: str, timeout: float) -> bool:
        end = time.time() + timeout
        while time.time() < end:
            if needle in self.screen():
                return True
            self.pump(0.3)
        return False

    def wait_idle(self, timeout: float) -> bool:
        """Wait for the turn to conclude: the composer placeholder reads
        "describe a task — Enter sends…" only at phase Idle with an empty
        composer — a structural turn-boundary signal (never a
        model-computed string; raw-stream occurrence counting is
        repaint-unreliable)."""
        return self.wait_screen("describe a task", timeout)

    def wait_screen_gone_verified(self, needle: str, timeout: float) -> bool:
        """`wait_screen_gone` with a Ctrl+L verification pass: pyte's VT
        emulation can diverge from the engine's emitted bytes and keep a
        phantom of a CLOSED modal on its model (proven 2026-07-23: the
        engine's own VtScreen interpreter, fed the identical capture,
        showed the clean screen — `tests/headless_ui.rs::vt_replay_probe`).
        Ctrl+L forces the engine to re-emit its whole model, which
        resyncs ANY interpreter: a needle that survives the redraw is
        really there; one that clears was a harness phantom."""
        if self.wait_screen_gone(needle, timeout):
            return True
        self.send(b"\x0c")
        self.pump(2.0)
        return needle not in self.screen()

    # -- lifecycle -------------------------------------------------------------
    def sigkill(self) -> None:
        """Crash-kill (T-scenarios: a crash, not a clean quit).

        macOS pty gotcha (live wedge, 2026-07-23): a blocking
        `waitpid(pid, 0)` after SIGKILL can hang FOREVER when the child
        was mid-render — the child parks in kernel exit (`ps` state
        `?Es`, "trying to exit") until its pty output drains, and the
        master buffer is full because the harness stopped pumping.
        SIGKILL cannot help a process already in uninterruptible exit.
        Reap with WNOHANG while DRAINING the master.
        """
        try:
            os.kill(self.pid, signal.SIGKILL)
        except OSError:
            pass
        end = time.time() + 10
        while time.time() < end:
            try:
                done, _status = os.waitpid(self.pid, os.WNOHANG)
            except ChildProcessError:
                break
            if done:
                break
            self.pump(0.1)  # drain so the child can finish exiting
        self.exited = -9
        self._close_master()

    def quit_ctrl_c(self, timeout: float = 8.0):
        """Clean quit via Ctrl+C; returns the exit code or None on hang."""
        try:
            self.send(b"\x03")
        except OSError:
            pass
        end = time.time() + timeout
        while time.time() < end:
            try:
                done, status = os.waitpid(self.pid, os.WNOHANG)
            except ChildProcessError:
                break
            if done:
                self.exited = os.waitstatus_to_exitcode(status)
                self._close_master()
                return self.exited
            self.pump(0.2)
        return None

    def ensure_dead(self) -> None:
        if self.exited is None:
            self.sigkill()
        try:
            os.unlink(self.prefs_path)
        except OSError:
            pass

    def _close_master(self) -> None:
        try:
            os.close(self.master)
        except OSError:
            pass


def tui_cmd(bin_path, gateway, token, session, extra=None):
    cmd = [
        bin_path,
        "--gateway", gateway,
        "--token", token,
        "--workflow", "basic-agent",
        "--provider", os.environ.get("ACODE_PROVIDER", "lmstudio"),
        "--model", os.environ.get("ACODE_MODEL", "qwen/qwen3.6-35b-a3b"),
        "--session", session,
        "--workspace", "/tmp/acode-tui-conformance",
    ]
    return cmd + (extra or [])


class Gw:
    """Stdlib gateway REST client — the 'another app' in every scenario.

    Command shapes mirror `GatewayClient::submit_command` exactly:
    POST /api/gateway/commands
      {command_id, run_id, type, payload, client_id}
    with resume payload {"wait_key": ..., "payload": {...}}.
    """

    def __init__(self, base, token):
        self.base = base.rstrip("/")
        self.token = token
        self._cmd_n = 0

    def _req(self, method, path, body=None, timeout=20):
        url = f"{self.base}/api/gateway{path}"
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(url, data=data, method=method)
        req.add_header("Authorization", f"Bearer {self.token}")
        req.add_header("Accept", "application/json")
        if data is not None:
            req.add_header("Content-Type", "application/json")
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read()
        return json.loads(raw) if raw.strip() else None

    def get(self, path):
        return self._req("GET", path)

    def runs(self, session, root_only=False, limit=30):
        q = f"/runs?limit={limit}&session_id={session}"
        if root_only:
            q += "&root_only=true"
        return self.get(q).get("items", [])

    def run(self, run_id):
        return self.get(f"/runs/{run_id}")

    def ledger_total(self, session) -> int:
        """Sum of ledger lengths across every run of the session — the
        'is the gateway still working' odometer."""
        return sum(int(r.get("ledger_len") or 0) for r in self.runs(session))

    def waiting_user_run(self, session):
        """The run parked on a reason=user wait (approval or ask), with its
        wait_key — the discovery an observer-style app performs."""
        for r in self.runs(session):
            w = r.get("waiting") or {}
            if r.get("status") == "waiting" and w.get("reason") == "user":
                full = self.run(r["run_id"])
                wait = full.get("waiting") or {}
                if wait.get("wait_key"):
                    return r["run_id"], wait
        return None, None

    def command(self, run_id, typ, payload):
        self._cmd_n += 1
        body = {
            "command_id": f"cmd_conf_{os.getpid()}_{time.time_ns()}_{self._cmd_n}",
            "run_id": run_id,
            "type": typ,
            "payload": payload,
            "client_id": "conformance-external",
        }
        return self._req("POST", "/commands", body)

    def resume(self, run_id, wait_key, payload):
        return self.command(run_id, "resume", {"wait_key": wait_key, "payload": payload})

    def cancel(self, run_id):
        return self.command(run_id, "cancel", {})

    def wait_status(self, run_id, statuses, timeout):
        end = time.time() + timeout
        while time.time() < end:
            st = self.run(run_id).get("status")
            if st in statuses:
                return st
            time.sleep(2)
        return self.run(run_id).get("status")

    def cancel_session_runs(self, session) -> list:
        """End-of-scenario hygiene: durably cancel every non-terminal ROOT
        run of the session, wait for the cancels to land, and report every
        root run id with its final status."""
        roots = self.runs(session, root_only=True)
        pending = []
        for r in roots:
            if r.get("status") not in ("completed", "failed", "cancelled"):
                try:
                    self.cancel(r["run_id"])
                    pending.append(r["run_id"])
                except urllib.error.URLError as e:
                    print(f"    cancel {r['run_id'][:8]} failed: {e}")
        deadline = time.time() + 45
        while pending and time.time() < deadline:
            time.sleep(3)
            pending = [
                rid
                for rid in pending
                if self.run(rid).get("status") not in ("completed", "failed", "cancelled")
            ]
        report = []
        for r in self.runs(session, root_only=True):
            report.append((r["run_id"], self.run(r["run_id"]).get("status")))
        print("  — session runs at scenario end —")
        for rid, st in report:
            print(f"    {rid} {st}")
        return report
