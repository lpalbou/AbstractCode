#!/usr/bin/env python3
"""T5 — pause/resume durability (lane 3).

Contract: /pause an active run, quit the client entirely, relaunch — the
paused state must render from SERVER truth (not local memory), and
/resume continues the run.

Exit: 0 pass, 1 fail, 2 config error.
"""

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pty_conformance_common import Checks, Gw, Tui, env_config, tui_cmd  # noqa: E402

PROMPT = (
    "Create story.txt containing a 6-line story about a lighthouse, then read "
    "it back, then list the workspace files, then count the story's words, "
    "then summarize everything you did."
)


def main() -> int:
    bin_path, gateway, token = env_config()
    session = f"acode-conf-t5-{int(time.time())}"
    gw = Gw(gateway, token)
    c = Checks("T5 pause/resume durability")
    print(f"T5 session: {session}")

    a = b = None
    root_id = ""
    try:
        a = Tui(tui_cmd(bin_path, gateway, token, session), label="t5a")
        a.pump(2.0)
        c.check("A booted", a.wait_raw("AbstractCode", 15, "boot"))
        a.type_line(PROMPT)
        c.check("A: approval modal up", a.wait_raw("tool approval", 150, "modal"))
        a.pump(0.5)
        a.send(b"a")
        c.check("A: write executed", a.wait_raw("write_file", 60, "tool"))
        roots = gw.runs(session, root_only=True)
        root_id = roots[0]["run_id"] if roots else ""
        c.check("run exists", bool(root_id))
        print(f"  · root run {root_id}")

        # ---- pause durably ----------------------------------------------------
        a.type_line("/pause")
        c.check(
            "A: pause acknowledged",
            a.wait_raw("run paused durably on the gateway", 30, "pause toast"),
        )
        paused = False
        end = time.time() + 30
        while time.time() < end:
            if gw.run(root_id).get("paused"):
                paused = True
                break
            time.sleep(2)
        c.check("gateway reports paused=true", paused)

        code = a.quit_ctrl_c()
        c.check("A: clean exit while paused", code == 0)
        time.sleep(3)
        rec = gw.run(root_id)
        c.check(
            f"run still paused server-side after quit (status {rec.get('status')})",
            bool(rec.get("paused"))
            and rec.get("status") not in ("cancelled", "failed", "completed"),
        )

        # ---- relaunch: paused state from server truth --------------------------
        b = Tui(tui_cmd(bin_path, gateway, token, session), label="t5b")
        b.pump(2.0)
        c.check("B booted", b.wait_raw("AbstractCode", 15, "boot"))
        c.check(
            "B reattached to the paused run",
            b.wait_raw("reattaching to live run", 30, "reattach"),
        )
        c.check(
            "B renders the paused state from server truth",
            b.wait_screen("run paused durably on the gateway", 30),
        )

        # ---- /resume continues --------------------------------------------------
        b.type_line("/resume")
        c.check("B: resume acknowledged", b.wait_raw("run resumed", 30, "resume toast"))
        unpaused = False
        end = time.time() + 30
        while time.time() < end:
            if not gw.run(root_id).get("paused"):
                unpaused = True
                break
            time.sleep(2)
        c.check("gateway reports paused=false after /resume", unpaused)
        c.check(
            "B: run continues to the final answer",
            b.wait_raw("✦ assistant", 240, "answer"),
        )
        c.check("B: turn concluded (composer idle)", b.wait_idle(120))
        code = b.quit_ctrl_c()
        c.check("B: clean exit", code == 0)
    finally:
        for t in (a, b):
            if t is not None:
                t.ensure_dead()
        gw.cancel_session_runs(session)

    return c.verdict()


if __name__ == "__main__":
    import signal as _signal

    _signal.signal(_signal.SIGALRM, lambda *_: sys.exit(3))
    _signal.alarm(720)
    sys.exit(main())
