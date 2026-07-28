#!/usr/bin/env python3
"""T2 — external approval resolution (the observer scenario), lane 3.

Contract (maintainer): "If in observer I see that a decision was gated, I
should be able to accept there." The TUI must notice a wait resolved
elsewhere: the modal closes WITHOUT a local answer, the run continues, no
stale modal, and a late local approve must not corrupt anything (the
gateway's command lane is durable accept-then-apply — a stale resume is
accepted with HTTP 200 and dropped at apply time; verified live
2026-07-23).

Three turns, three orderings:
1. external-only: approval modal up → resolve via POST /commands from a
   foreign client id → modal must close with NO local key; run concludes.
2. race: external resume posted, local `a` pressed immediately after —
   both resumes land; exactly one applies; no "resume failed", no
   reopened modal, turn concludes.
3. inverse: local `a` first, then the SAME wait resumed externally
   (another app double-resolving) — dropped quietly; turn concludes.

Robustness: each turn REST-confirms a new root run actually started
(model/gateway lottery can skip a turn — that is outside this contract)
and retries once with a fresh filename before judging.

Exit: 0 pass, 1 fail, 2 config error.
"""

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pty_conformance_common import Checks, Gw, Tui, env_config, tui_cmd  # noqa: E402

PROMPT = (
    "Use the write_file tool to create {name}.txt containing exactly: {name}. "
    "Then confirm you are done."
)


def start_turn(t, gw, session, c, label, names):
    """Type a prompt and REST-confirm a NEW root run started; retry once
    with a fresh filename. Returns the set of known root ids after."""
    known = {r["run_id"] for r in gw.runs(session, root_only=True)}
    for attempt, name in enumerate(names):
        t.type_line(PROMPT.format(name=name))
        end = time.time() + 25
        while time.time() < end:
            t.pump(1.0)
            now = {r["run_id"] for r in gw.runs(session, root_only=True)}
            fresh = now - known
            if fresh:
                c.note(f"{label}: run started ({next(iter(fresh))[:8]}, prompt {name!r})")
                return True
        c.note(f"{label}: no run started for {name!r} (attempt {attempt + 1}) — retrying")
        # Clear any leftover draft before retrying (Esc clears a non-empty
        # composer and is key-safe: with an empty composer it only arms the
        # cancel hint once).
        t.send(b"\x1b")
        t.pump(0.5)
    return False


def await_approval(t, gw, session, c, label):
    """Wait for the approval modal + the REST-discoverable wait."""
    if not c.check(f"{label}: approval modal up", t.wait_screen("tool approval", 150)):
        return None, None
    t.pump(0.5)
    end = time.time() + 60
    while time.time() < end:
        run_id, wait = gw.waiting_user_run(session)
        if run_id:
            c.note(f"{label}: waiting run {run_id[:8]} key {wait['wait_key'][:40]}")
            c.check(f"{label}: wait discoverable via REST", True)
            return run_id, wait
        time.sleep(1.5)
    c.check(f"{label}: wait discoverable via REST", False)
    return None, None


def main() -> int:
    bin_path, gateway, token = env_config()
    session = f"acode-conf-t2-{int(time.time())}"
    gw = Gw(gateway, token)
    c = Checks("T2 external approval (observer scenario)")
    print(f"T2 session: {session}")

    t = Tui(tui_cmd(bin_path, gateway, token, session), label="t2")
    try:
        t.pump(2.0)
        c.check("booted", t.wait_raw("AbstractCode", 15, "boot"))

        # ---- turn 1: external-only resolution --------------------------------
        c.check("turn1: run started", start_turn(t, gw, session, c, "turn1", ["one", "uno"]))
        run_id, wait = await_approval(t, gw, session, c, "turn1")
        if run_id:
            gw.resume(run_id, wait["wait_key"], {"approved": True})
            c.note("turn1: resumed EXTERNALLY (no local key)")
            c.check(
                "turn1: modal closed by itself",
                t.wait_screen_gone("tool approval", 30),
            )
            c.check(
                "turn1: waiting strip cleared",
                t.wait_screen_gone("approval needed", 15),
            )
        c.check("turn1: answer card rendered", t.wait_raw("✦ assistant", 180, "answer"))
        c.check("turn1: turn concluded (composer idle)", t.wait_idle(120))
        t.pump(1.5)
        c.check(
            "turn1: no stale 'awaiting approval' card on screen",
            "awaiting approval" not in t.screen(),
        )

        # ---- turn 2: race — external + immediate local approve ---------------
        c.check("turn2: run started", start_turn(t, gw, session, c, "turn2", ["two", "dos"]))
        run_id, wait = await_approval(t, gw, session, c, "turn2")
        if run_id:
            gw.resume(run_id, wait["wait_key"], {"approved": True})
            # Immediately press the local approve — the modal is still up
            # (the client cannot have observed the external resolution yet).
            t.send(b"a")
            c.note("turn2: external resume + immediate local 'a' (double resume)")
        t.pump(1.5)
        # Neutralize a stray 'a' if the modal happened to close first (a
        # backspace is a no-op on an empty composer; the modal ignores it).
        t.send(b"\x7f")
        c.check("turn2: turn concluded (composer idle)", t.wait_idle(180))
        t.pump(1.5)
        scr2 = t.screen()
        clean2 = "tool approval" not in scr2 and "approval needed" not in scr2
        if not clean2:
            for line in scr2.splitlines():
                if "tool approval" in line or "approval needed" in line:
                    c.note(f"offending screen line: {line.strip()!r}")
            # Discriminate: Ctrl+L re-emits the ENGINE's model. A phantom
            # that survives = the engine still holds the modal (real
            # defect); one that clears = stale pixels only (an emission
            # gap or a harness VT divergence — the bytes dump decides).
            with open("/tmp/conf_t2-phantom-bytes.bin", "wb") as f:
                f.write(bytes(t.buf))
            t.send(b"\x0c")
            t.pump(2.0)
            scr2b = t.screen()
            survived = "tool approval" in scr2b or "approval needed" in scr2b
            c.note(
                "phantom "
                + ("SURVIVED Ctrl+L — engine model holds the modal" if survived
                   else "cleared by Ctrl+L — stale pixels only, engine model clean")
            )
            clean2 = not survived
        c.check(
            "turn2: no stale modal in the ENGINE's model after the double resume",
            clean2,
        )

        # ---- turn 3: local answer, then external duplicate -------------------
        c.check(
            "turn3: run started", start_turn(t, gw, session, c, "turn3", ["three", "tres"])
        )
        run_id, wait = await_approval(t, gw, session, c, "turn3")
        if run_id:
            t.send(b"a")
            c.note("turn3: approved LOCALLY first")
            time.sleep(2.0)
            gw.resume(run_id, wait["wait_key"], {"approved": True})
            c.note("turn3: same wait resumed externally afterwards (duplicate)")
        c.check("turn3: turn concluded (composer idle)", t.wait_idle(180))
        t.pump(2.0)

        # ---- global honesty checks -------------------------------------------
        raw = t.raw()
        c.check("no 'resume failed' toast anywhere", "resume failed" not in raw)
        scr = t.screen()
        c.check("no stale approval modal at end", "tool approval" not in scr)
        c.check("no stale waiting strip at end", "approval needed" not in scr)
        code = t.quit_ctrl_c()
        c.check("clean exit", code == 0)
    finally:
        # Failure forensics: the full raw stream + final screen.
        try:
            with open("/tmp/conf_t2-raw.txt", "w") as f:
                f.write(t.raw())
            with open("/tmp/conf_t2-screen.txt", "w") as f:
                f.write(t.screen())
        except Exception:
            pass
        t.ensure_dead()
        gw.cancel_session_runs(session)

    return c.verdict()


if __name__ == "__main__":
    import signal as _signal

    _signal.signal(_signal.SIGALRM, lambda *_: sys.exit(3))
    _signal.alarm(900)
    sys.exit(main())
