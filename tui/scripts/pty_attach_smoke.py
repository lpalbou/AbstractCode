#!/usr/bin/env python3
"""Live TUI attachment smoke — the exact lane the verify-pass NEW-1 P0
would have killed: a real attachment send drives Runner::start_run's
upload loop ON THE SPAWNED WORKER THREAD (the exec smoke bypasses it —
no Store, no signals). PASS requires the run to START (worker alive),
the model to answer from the attached content, and the 📎 record to
land. A worker panic renders "gateway worker is dead" — explicitly
checked absent.

Usage: .venv python3 scripts/pty_attach_smoke.py   (gateway on :8080)
"""

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pty_conformance_common import Checks, Tui, env_config, tui_cmd  # noqa: E402


def main() -> int:
    c = Checks("pty_attach_smoke")
    bin_path, gateway, token = env_config()

    # A fact file under the /tmp SYMLINK spelling (macOS: /tmp ->
    # /private/tmp) — the P1-1 class rides the same run.
    workdir = f"/tmp/acode-attach-pty-{os.getpid()}"
    os.makedirs(workdir, exist_ok=True)
    fact_path = os.path.join(workdir, "brief.txt")
    with open(fact_path, "w") as f:
        f.write("The launch window codeword is MOSS-HERON-77.\n")

    session = f"acode-attach-pty-{int(time.time())}"
    argv = tui_cmd(bin_path, gateway, token, session)
    # Point the workspace at OUR dir (tui_cmd's default is fine to
    # override in place — the flag appears once).
    argv[argv.index("--workspace") + 1] = workdir
    tui = Tui(argv, label="attach")
    try:
        c.check("boots to composer", tui.wait_screen("describe a task", 30))

        # Stage via /attach with the symlinked spelling.
        tui.type_line(f"/attach {fact_path}")
        tui.pump(1.0)
        c.check("chip staged (chips row renders)", tui.wait_raw("brief.txt", 10))

        # Send: this drives the worker-thread upload loop (NEW-1 lane).
        tui.type_line(
            "What is the launch window codeword in the attached file? "
            "One line, no tools - the content is in your context."
        )
        c.check("run starts (worker alive)", tui.wait_raw("cycle 1", 120))
        c.check(
            "no dead-worker banner",
            "gateway worker is dead" not in tui.raw(),
        )
        c.check("📎 record lands", "📎" in tui.raw())
        c.check(
            "model answers from the attachment",
            tui.wait_raw("MOSS-HERON-77", 180),
        )
        c.check("turn concludes to idle", tui.wait_idle(120))
    finally:
        tui.ensure_dead()
        import shutil

        shutil.rmtree(workdir, ignore_errors=True)
    return c.verdict()


if __name__ == "__main__":
    sys.exit(main())
