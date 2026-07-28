#!/usr/bin/env python3
"""T1 — kill/reconnect mid-run (interruptibility conformance, lane 3).

Contract (maintainer): "I should be able to launch something on
abstractcode, disconnect and reconnect later on that same session to see
the progress."

Scenario:
1. Client A starts a multi-cycle run (write poem.txt → approval → read it
   back → confirm) and approves the write.
2. Mid-execution (after the approval, while later cycles run), client A is
   SIGKILLed — a crash, not a quit.
3. The run keeps executing on the gateway (proven by REST in t4; here we
   relaunch quickly).
4. Client B (same session) MUST: reattach automatically, replay the
   progress so far (the approved write_file tool card from the ledger),
   continue live-following, and conclude with the final answer + non-zero
   token totals. No duplicate turn cards.
5. Variant (concluded-while-dead): kill client B after the run finished
   but before quitting cleanly, relaunch client C — the answer must render
   from history alone (rehydration), no live run left.

Exit: 0 pass, 1 fail, 2 config error.
"""

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pty_conformance_common import Checks, Gw, Tui, env_config, tui_cmd  # noqa: E402

PROMPT = (
    "Create a file named poem.txt containing a 4-line poem about terminals, "
    "then read the file back and confirm its contents match."
)


def main() -> int:
    bin_path, gateway, token = env_config()
    session = f"acode-conf-t1-{int(time.time())}"
    gw = Gw(gateway, token)
    c = Checks("T1 kill/reconnect mid-run")
    print(f"T1 session: {session}")

    a = Tui(tui_cmd(bin_path, gateway, token, session), label="t1a")
    b = cx = None
    try:
        a.pump(2.0)
        c.check("A booted", a.wait_raw("AbstractCode", 15, "boot"))
        a.type_line(PROMPT)
        c.check("A: approval modal appeared", a.wait_raw("tool approval", 150, "approval"))
        a.pump(0.5)
        a.send(b"a")
        # Wait for the approved tool to actually run (the write lands),
        # so the SIGKILL hits MID-EXECUTION of the later cycles.
        c.check("A: write_file executed", a.wait_raw("write_file", 60, "tool card"))
        a.pump(2.0)

        roots = gw.runs(session, root_only=True)
        c.check("run exists on the gateway", len(roots) == 1)
        root_id = roots[0]["run_id"] if roots else ""
        print(f"  · root run {root_id}")

        # --- the crash ------------------------------------------------------
        a.sigkill()
        c.note("A SIGKILLed mid-run")
        time.sleep(2)
        status_at_kill = gw.run(root_id).get("status") if root_id else "?"
        c.check(
            f"run survives the client's death (status {status_at_kill})",
            status_at_kill in ("running", "waiting"),
        )

        # --- reconnect ------------------------------------------------------
        b = Tui(tui_cmd(bin_path, gateway, token, session), label="t1b")
        b.pump(2.0)
        c.check("B booted", b.wait_raw("AbstractCode", 15, "boot"))
        c.check(
            "B reattached automatically (notice)",
            b.wait_raw("reattaching to live run", 30, "reattach"),
        )
        c.check(
            "B replays progress made before/while dead (write_file card)",
            b.wait_raw("write_file", 30, "replayed tool card"),
        )
        c.check(
            "B renders the original prompt (turn context)",
            b.wait_raw("poem.txt", 15, "prompt"),
        )
        # Live-follow to the conclusion.
        c.check(
            "B follows live to the final answer",
            b.wait_raw("✦ assistant", 240, "final answer"),
        )
        b.pump(3.0)
        scr = b.screen()
        c.check(
            "no duplicate final answer cards on screen",
            scr.count("✦ assistant") <= 1,
        )
        c.check(
            "token totals visible and non-zero after reattach",
            " tk" in scr and " 0 tk" not in scr,
        )
        # No stale wait: the approval was answered before the kill.
        c.check("no stale approval modal after reattach", "tool approval" not in scr)

        # The turn concluded; the wrapper root may still run its poller.
        # --- variant: conclude while dead ------------------------------------
        b.sigkill()
        c.note("B SIGKILLed after the answer (variant: reopen after conclusion)")
        time.sleep(2)
        cx = Tui(tui_cmd(bin_path, gateway, token, session), label="t1c")
        cx.pump(2.0)
        c.check("C booted", cx.wait_raw("AbstractCode", 15, "boot"))
        # Whether the wrapper root is still live (reattach) or terminal
        # (rehydration), the ANSWER must render from server history.
        c.check(
            "C renders the concluded turn from history",
            cx.wait_raw("✦ assistant", 90, "restored answer")
            or cx.wait_raw("replayed", 30, "rehydration notice"),
        )
        code = cx.quit_ctrl_c()
        c.check("C quits cleanly", code == 0)
    finally:
        for t in (a, b, cx):
            if t is not None:
                t.ensure_dead()
        gw.cancel_session_runs(session)

    return c.verdict()


if __name__ == "__main__":
    import signal as _signal

    _signal.signal(_signal.SIGALRM, lambda *_: sys.exit(3))
    _signal.alarm(600)
    sys.exit(main())
