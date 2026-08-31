#!/usr/bin/env python3
"""T3 — external ask_user resolution (lane 3).

Same wait plumbing as T2 (one `pending_wait` slot, one `resume` command,
the fold's answered-elsewhere rule), exercised on the ASK modal: the
agent asks a question, the answer arrives from ANOTHER app (POST
/commands with {"response": ...}), and the TUI's ask modal must close
without local typing while the run continues with that answer.

GATEWAY PREREQUISITE (live finding 2026-07-23): this scenario needs an
ask-style tool in the gateway inventory. The conformance gateway served
14 tools with NO ask/user/question tool, so the basic-agent cannot raise
an ask wait from any prompt — the scenario is unconstructible there and
T3 is covered BY MECHANISM instead:
- both wait kinds land in the ONE `Fold::pending_wait` slot
  (`consider_wait`, transcript.rs) and differ only in modal rendering;
- the answered-elsewhere clearing rule is kind-agnostic (any later
  record from the waiting run clears the slot);
- `wire_wait_modals` closes whichever wait prompt is open on the None
  edge (`wait_modal_for` tracks the occurrence, not the kind);
- both modals resume through the same `Cmd::Resume` → POST /commands.
Headless proof for BOTH kinds:
`tests/headless_ui.rs::wait_resolved_elsewhere_closes_the_modal_without_local_answer`.
Live proof of the shared plumbing: `pty_conformance_t2.py` turn 1.

Exit: 0 pass, 1 fail, 2 config error.
"""

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pty_conformance_common import Checks, Gw, Tui, env_config, tui_cmd  # noqa: E402

PROMPT = (
    "Use the ask_user tool to ask me which color I want. "
    "After I answer, reply with just that color and finish."
)


def main() -> int:
    bin_path, gateway, token = env_config()
    session = f"acode-conf-t3-{int(time.time())}"
    gw = Gw(gateway, token)
    c = Checks("T3 external ask_user")
    print(f"T3 session: {session}")

    t = Tui(tui_cmd(bin_path, gateway, token, session), label="t3")
    try:
        t.pump(2.0)
        c.check("booted", t.wait_raw("AbstractCode", 15, "boot"))
        t.type_line(PROMPT)
        # The ask modal title is "the agent asks".
        c.check("ask modal appeared", t.wait_screen("the agent asks", 150))
        t.pump(0.5)
        run_id, wait = None, None
        end = time.time() + 60
        while time.time() < end:
            run_id, wait = gw.waiting_user_run(session)
            if run_id:
                break
            time.sleep(1.5)
        c.check("ask wait discoverable via REST", run_id is not None)
        if run_id:
            c.note(f"waiting run {run_id[:8]} key {wait['wait_key'][:40]}")
            gw.resume(run_id, wait["wait_key"], {"response": "blue"})
            c.note("answered EXTERNALLY with 'blue' (no local typing)")
            c.check("ask modal closed by itself", t.wait_screen_gone("the agent asks", 30))
        c.check("turn concluded (composer idle)", t.wait_idle(180))
        t.pump(1.5)
        scr = t.screen()
        c.check("no stale ask modal/strip at end", "the agent asks" not in scr
                and "agent asked a question" not in scr)
        c.check("no 'resume failed' toast anywhere", "resume failed" not in t.raw())
        code = t.quit_ctrl_c()
        c.check("clean exit", code == 0)
    finally:
        t.ensure_dead()
        gw.cancel_session_runs(session)

    return c.verdict()


if __name__ == "__main__":
    import signal as _signal

    _signal.signal(_signal.SIGALRM, lambda *_: sys.exit(3))
    _signal.alarm(480)
    sys.exit(main())
