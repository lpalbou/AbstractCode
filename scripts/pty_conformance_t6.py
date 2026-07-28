#!/usr/bin/env python3
"""T6 — second client, same session (lane 3).

Contract (maintainer): "Something started in code-tui should be able to
continue on other apps." Here the other app is a second code-tui: both
render the run's progress; neither corrupts the other; an approval
answered in client A clears in client B without any local key there.

Notes on exclusivity: the client holds NO lock on a session — attach is
a read-only stream + durable commands. Each instance gets its own prefs
file in this harness; in real usage two instances share prefs.json
(last-writer-wins on save — documented in docs/troubleshooting.md).

Exit: 0 pass, 1 fail, 2 config error.
"""

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pty_conformance_common import Checks, Gw, Tui, env_config, tui_cmd  # noqa: E402

PROMPT = (
    "Create a file duet.txt containing exactly: duet. Then read it back and "
    "confirm its contents."
)


def main() -> int:
    bin_path, gateway, token = env_config()
    session = f"acode-conf-t6-{int(time.time())}"
    gw = Gw(gateway, token)
    c = Checks("T6 second client, same session")
    print(f"T6 session: {session}")

    a = b = None
    try:
        a = Tui(tui_cmd(bin_path, gateway, token, session), label="t6a")
        a.pump(2.0)
        c.check("A booted", a.wait_raw("AbstractCode", 15, "boot"))
        a.type_line(PROMPT)
        c.check("A: approval modal up", a.wait_raw("tool approval", 150, "modal"))

        # Second client on the SAME session while A holds the approval.
        b = Tui(tui_cmd(bin_path, gateway, token, session), label="t6b")
        b.pump(2.0)
        c.check("B booted", b.wait_raw("AbstractCode", 15, "boot"))
        c.check(
            "B reattached to A's live run",
            b.wait_raw("reattaching to live run", 30, "reattach"),
        )
        c.check(
            "B surfaces the SAME approval from the ledger",
            b.wait_raw("tool approval", 45, "modal in B"),
        )

        # Answer in A; B must clear without any local key.
        a.pump(0.5)
        a.send(b"a")
        c.note("approved in client A")
        c.check(
            "B: approval cleared by A's answer (no local key in B)",
            b.wait_screen_gone_verified("tool approval", 30),
        )
        c.check(
            "B: waiting strip cleared too",
            b.wait_screen_gone_verified("approval needed", 15),
        )

        # Both render the progress to conclusion.
        c.check("A: answer rendered", a.wait_raw("✦ assistant", 240, "answer in A"))
        c.check("B: answer rendered", b.wait_raw("✦ assistant", 60, "answer in B"))
        c.check("A: turn concluded", a.wait_idle(120))
        c.check("B: turn concluded", b.wait_idle(60))

        # Neither corrupted the other.
        c.check("A: no resume failures", "resume failed" not in a.raw())
        c.check("B: no resume failures", "resume failed" not in b.raw())
        code_a = a.quit_ctrl_c()
        code_b = b.quit_ctrl_c()
        c.check("A: clean exit", code_a == 0)
        c.check("B: clean exit", code_b == 0)
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
