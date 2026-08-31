#!/usr/bin/env python3
"""T4 — disconnect ≠ interrupt (lane 3).

Contract (maintainer): the client's death must never pause or cancel
server work; quit = detach, the run continues; the ONLY cancel is
explicit (/cancel or Esc-Esc).

Leg 1 (crash): start a multi-step run, approve its write, SIGKILL the
client, verify VIA READ-ONLY REST over 60s that the gateway kept
executing (ledger odometer advances / the agent subrun completes — never
cancelled), then reconnect and see the progress.

Leg 2 (clean quit): start a read-only run (auto-approved tools, no local
attendance needed), Ctrl+C mid-execution, assert exit 0 AND the run
stays running/completing on the gateway — never cancelled by quit.

Exit: 0 pass, 1 fail, 2 config error.
"""

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pty_conformance_common import Checks, Gw, Tui, env_config, tui_cmd  # noqa: E402


def agent_subrun_status(gw, session):
    """(status, run_id) of the newest agent subrun (parented, not the
    poller): the run whose completion means the WORK happened."""
    runs = gw.runs(session)
    best = (None, None)
    for r in runs:
        if r.get("parent_run_id") and int(r.get("ledger_len") or 0) > 3:
            best = (r.get("status"), r.get("run_id"))
    return best


def main() -> int:
    bin_path, gateway, token = env_config()
    session = f"acode-conf-t4-{int(time.time())}"
    gw = Gw(gateway, token)
    c = Checks("T4 disconnect ≠ interrupt")
    print(f"T4 session: {session}")

    a = b = None
    try:
        # ---- leg 1: SIGKILL, 60s dead window, REST proof ---------------------
        a = Tui(tui_cmd(bin_path, gateway, token, session), label="t4a")
        a.pump(2.0)
        c.check("A booted", a.wait_raw("AbstractCode", 15, "boot"))
        a.type_line(
            "Create a file facts.txt with three facts about terminals, then read "
            "it back, then list the files in your workspace, then summarize."
        )
        c.check("A: approval modal up", a.wait_raw("tool approval", 150, "modal"))
        a.pump(0.5)
        a.send(b"a")
        c.check("A: write executed", a.wait_raw("write_file", 60, "tool"))
        a.pump(1.0)
        l0 = gw.ledger_total(session)
        a.sigkill()
        c.note(f"A SIGKILLed mid-run (ledger odometer {l0})")

        time.sleep(60)
        l1 = gw.ledger_total(session)
        roots = gw.runs(session, root_only=True)
        root_status = roots[0]["status"] if roots else "?"
        sub_status, sub_id = agent_subrun_status(gw, session)
        c.note(f"after 60s dead: odometer {l0}→{l1}, root {root_status}, agent {sub_status}")
        c.check("gateway kept executing while the client was dead", l1 > l0)
        c.check(
            "nothing was cancelled/failed by the client's death",
            root_status not in ("cancelled", "failed")
            and sub_status not in ("cancelled", "failed"),
        )
        c.check(
            "the agent finished its work unattended",
            sub_status == "completed",
        )

        # Reconnect and SEE the progress. "Progress" is what the run
        # produced while dead — the reattach + the concrete work (tool
        # cards / cycles). NOT necessarily a final answer: a multi-step
        # run can legitimately be parked on a LATER approval nobody
        # answered while the client was gone, in which case the honest
        # thing to show IS the pending prompt (surfaced from the ledger),
        # not a fabricated conclusion. The final-answer-after-reattach
        # path is T1's contract (which answers every approval).
        b = Tui(tui_cmd(bin_path, gateway, token, session), label="t4b")
        b.pump(2.0)
        c.check("B booted", b.wait_raw("AbstractCode", 15, "boot"))
        c.check("B reattached", b.wait_raw("reattaching to live run", 30, "reattach"))
        c.check(
            "B shows concrete progress made while dead",
            b.wait_raw("write_file", 45, "tool card")
            or b.wait_raw("✦ assistant", 45, "answer")
            or b.wait_raw("facts.txt", 20, "prompt/work"),
        )
        # Whatever state B lands in, it must be the REAL one — either the
        # answer or a genuinely-pending approval, never a stuck spinner
        # over a run that already concluded server-side.
        b.pump(2.0)
        live_sub = agent_subrun_status(gw, session)[0]
        if live_sub == "completed":
            c.check(
                "B renders the answer for a concluded run",
                b.wait_raw("✦ assistant", 120, "answer"),
            )

        # ---- leg 2: clean quit must not cancel -------------------------------
        b.type_line(
            "List the files in your workspace, then read facts.txt, then tell "
            "me how many lines it has."
        )
        # Wait for the run to be genuinely executing (a reason cycle).
        started = False
        end = time.time() + 60
        while time.time() < end:
            b.pump(1.0)
            roots_now = gw.runs(session, root_only=True)
            if len(roots_now) > len(roots):
                started = True
                break
        c.check("B: second run started", started)
        roots_after = gw.runs(session, root_only=True)
        new_roots = [r["run_id"] for r in roots_after] and [
            r["run_id"]
            for r in roots_after
            if r["run_id"] not in {x["run_id"] for x in roots}
        ]
        leg2_root = new_roots[0] if new_roots else ""
        b.pump(3.0)
        code = b.quit_ctrl_c()
        c.check("B: clean exit on Ctrl+C", code == 0)
        c.note(f"leg-2 root {leg2_root}")
        time.sleep(8)
        st = gw.run(leg2_root).get("status") if leg2_root else "?"
        c.check(
            f"clean quit did NOT cancel the run (status {st})",
            st in ("running", "waiting", "completed"),
        )
        # Let it advance a little more to prove continued execution.
        l2 = gw.ledger_total(session)
        time.sleep(20)
        l3 = gw.ledger_total(session)
        st2 = gw.run(leg2_root).get("status") if leg2_root else "?"
        c.check(
            "run kept executing after the clean quit",
            l3 > l2 or st2 == "completed",
        )
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
