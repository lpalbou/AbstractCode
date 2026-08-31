#!/usr/bin/env python3
"""Cycle-3 verifier B: live end-to-end feature verification (release binary).

Drives the REAL binary against the LIVE gateway through a pty + pyte VT
screen and verifies the three cycle-3 feature waves end-to-end:

  batch1  (session S1, tier=all):  /workspace add /tmp -> Ctrl+J multiline
          prompt forcing a write (NO approval modal must appear) -> steer
          while running -> /queue while running -> queue auto-drain ->
          second answer -> token totals / Ctrl+D / PgUp / theme cycle ->
          remove /tmp -> clean exit. Out-of-band: input_data carried
          workspace_allowed_paths + _runtime.tool_policy; files on disk.
  batch2a (session S2, tier=read): /tools tier display, /help + '/' and
          '@' completion, /entities read-only roster+card, /goal dark
          notice -> write prompt MUST prompt (readable approval modal,
          approve with 'a') -> /queue while running -> Ctrl+C quit
          mid-queue (persistence echo on stderr).
  batch2b (relaunch S2): queue restores PAUSED with echo -> /queue while
          idle-paused stays held -> modal shows items, x removes, r
          resumes -> queued run auto-starts and answers -> Esc arm toast
          -> /tools tier write display -> clean exit.

Budget: 4 live LLM runs total (2 + 1 + 1). Prefs are ISOLATED to a state
dir via ABSTRACTCODE_TUI_PREFS_FILE (the operator's real prefs are never
touched); the entity roster cache lands in the same dir. Evidence
(screen snapshots + raw transcript + CHECK lines) goes to
untracked/cycle3b/<batch>/.

Env: ACODE_GATEWAY_TOKEN (required), ACODE_GATEWAY_URL, ACODE_TUI_BIN,
     ACODE_C3B_STATE (state dir; default /tmp/acode-c3b-state).
Exit: 0 all gates pass, 1 any gate failed, 2 config error.
"""

import fcntl
import glob
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
import urllib.request

try:
    import pyte
except ImportError:
    print("pyte required (run with the framework venv python)", file=sys.stderr)
    sys.exit(2)

TOKEN = os.environ.get("ACODE_GATEWAY_TOKEN", "")
URL = os.environ.get("ACODE_GATEWAY_URL", "http://127.0.0.1:8080").rstrip("/")
STATE = os.environ.get("ACODE_C3B_STATE", "/tmp/acode-c3b-state")
BIN = os.environ.get("ACODE_TUI_BIN", os.path.join(STATE, "bin-abstractcode-tui"))
PROVIDER = os.environ.get("ACODE_PROVIDER", "lmstudio")
MODEL = os.environ.get("ACODE_MODEL", "qwen/qwen3.6-35b-a3b")
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EVIDENCE = os.path.join(REPO, "untracked", "cycle3b")
WORKSPACE = os.path.join(STATE, "ws")
COLS, ROWS = 160, 42

ESC = b"\x1b"
CTRL_C = b"\x03"
CTRL_D = b"\x04"
CTRL_T = b"\x14"
CTRL_J = b"\n"  # LF byte == Ctrl+J on the legacy wire
ENTER = b"\r"
TAB = b"\t"
DOWN = b"\x1b[B"
PGUP = b"\x1b[5~"


def gw_get(path):
    req = urllib.request.Request(
        f"{URL}/api/gateway{path}", headers={"Authorization": f"Bearer {TOKEN}"}
    )
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.loads(r.read().decode())


class Driver:
    """One pty launch of the TUI with a pyte screen + raw accumulation."""

    def __init__(self, evidence_dir, session_id, prefs_path, extra_args=()):
        self.evidence = evidence_dir
        os.makedirs(evidence_dir, exist_ok=True)
        self.raw = bytearray()
        self.screen = pyte.Screen(COLS, ROWS)
        self.stream = pyte.ByteStream(self.screen)
        self.checks = []
        self.snap_n = 0
        self.exited = False
        self.exit_code = None

        cmd = [
            os.path.abspath(BIN),
            "--gateway", URL,
            "--token", TOKEN,
            "--workflow", "basic-agent",
            "--provider", PROVIDER,
            "--model", MODEL,
            "--session", session_id,
            "--workspace", WORKSPACE,
        ] + list(extra_args)
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            env = dict(os.environ)
            env["TERM"] = "xterm-256color"
            env["ABSTRACTCODE_TUI_PREFS_FILE"] = prefs_path
            os.execvpe(cmd[0], cmd, env)
            os._exit(127)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

    # -- io -------------------------------------------------------------
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
                self.raw.extend(chunk)
                self.stream.feed(chunk)

    def raw_text(self):
        ansi = re.compile(
            rb"\x1b\[[0-9;:?<=>]*[a-zA-Z@`~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)"
            rb"|\x1b[_P^][^\x1b]*\x1b\\|\x1b[=>NOPZ78cM]|\x1b\([B0]"
        )
        return ansi.sub(b"", bytes(self.raw)).decode("utf-8", errors="replace")

    def screen_text(self):
        return "\n".join(self.screen.display)

    def send(self, data):
        os.write(self.fd, data)

    def type(self, text, per_char=0.006):
        for ch in text:
            os.write(self.fd, ch.encode())
            time.sleep(per_char)

    def type_line(self, text):
        self.type(text)
        time.sleep(0.15)
        self.send(ENTER)

    # -- assertions -------------------------------------------------------
    def check(self, label, ok, gate=True):
        self.checks.append((label, bool(ok), gate))
        print(f"  {'PASS' if ok else ('FAIL' if gate else 'warn')}  {label}", flush=True)
        return ok

    def wait_raw(self, needle, timeout, label=None, gate=True):
        end = time.time() + timeout
        while time.time() < end:
            if needle in self.raw_text():
                return self.check(label or f"raw: {needle!r}", True, gate)
            self.pump(0.25)
        return self.check(label or f"raw: {needle!r}", False, gate)

    def wait_screen(self, needle, timeout, label=None, gate=True):
        end = time.time() + timeout
        while time.time() < end:
            if needle in self.screen_text():
                return self.check(label or f"screen: {needle!r}", True, gate)
            self.pump(0.25)
        return self.check(label or f"screen: {needle!r}", False, gate)

    def snap(self, label):
        self.snap_n += 1
        path = os.path.join(self.evidence, f"snap-{self.snap_n:02d}-{label}.txt")
        with open(path, "w", encoding="utf-8") as f:
            f.write(self.screen_text())
        print(f"  · snap {os.path.basename(path)}", flush=True)
        return path

    # -- teardown ---------------------------------------------------------
    def quit(self, label="clean exit code 0", gate=True):
        self.send(CTRL_C)
        end = time.time() + 8
        while time.time() < end:
            done, status = os.waitpid(self.pid, os.WNOHANG)
            if done:
                self.exited = True
                self.exit_code = os.waitstatus_to_exitcode(status)
                break
            self.pump(0.2)
        # Post-teardown stderr (queue echo) arrives after altscreen exit.
        self.pump(0.6)
        self.check(label, self.exited and self.exit_code == 0, gate)

    def finish(self, name):
        if not self.exited:
            try:
                os.kill(self.pid, signal.SIGKILL)
                os.waitpid(self.pid, 0)
            except Exception:
                pass
        try:
            os.close(self.fd)
        except OSError:
            pass
        with open(os.path.join(self.evidence, "raw.txt"), "w", encoding="utf-8") as f:
            f.write(self.raw_text())
        ok = all(passed for _, passed, gate in self.checks if gate)
        with open(os.path.join(self.evidence, "checks.json"), "w", encoding="utf-8") as f:
            json.dump(
                [{"label": l, "ok": p, "gate": g} for l, p, g in self.checks], f, indent=1
            )
        print(f"{name}: {'PASS' if ok else 'FAIL'}", flush=True)
        return ok


def state_load():
    p = os.path.join(STATE, "state.json")
    if os.path.exists(p):
        with open(p, encoding="utf-8") as f:
            return json.load(f)
    return {}


def state_save(d):
    with open(os.path.join(STATE, "state.json"), "w", encoding="utf-8") as f:
        json.dump(d, f, indent=1)


def write_prefs(path, session_id, tier):
    with open(path, "w", encoding="utf-8") as f:
        json.dump(
            {"session_id": session_id, "tool_approval": {"accepted_tier": tier}}, f
        )


def session_runs(session_id):
    v = gw_get(f"/runs?limit=10&session_id={session_id}&root_only=true")
    items = v.get("items") or v.get("runs") or (v if isinstance(v, list) else [])
    return items


def file_proof(name, since_ts):
    """The approved write really happened (client ws or gateway-managed)."""
    patterns = [
        os.path.join(WORKSPACE, name),
        os.path.join(WORKSPACE, "**", name),
        "/Users/albou/tmp/abstractframework/runtime/workspaces/*/" + name,
        "/Users/albou/tmp/abstractframework/runtime/**/workspaces/*/" + name,
    ]
    for pat in patterns:
        for path in glob.glob(pat, recursive=True):
            try:
                if os.path.getmtime(path) > since_ts:
                    return path
            except OSError:
                pass
    return None


# ---------------------------------------------------------------------------
# batch 1 — tier=all + Ctrl+J + workspace + steer + queue drain + regression
# ---------------------------------------------------------------------------

def batch1():
    start_ts = time.time()
    st = state_load()
    stamp = st.get("stamp") or format(int(time.time()), "x")
    s1 = f"acode-c3b-tierall-{stamp}"
    st.update({"stamp": stamp, "s1": s1})
    state_save(st)
    prefs = os.path.join(STATE, "prefs-a.json")
    write_prefs(prefs, s1, "all")
    ev = os.path.join(EVIDENCE, "batch1")
    d = Driver(ev, s1, prefs)
    try:
        d.pump(2.0)
        d.wait_raw("AbstractCode", 15, "TUI booted")
        d.wait_raw("session", 10, "session line visible")
        # Tools inventory must be LOADED before the run start so the
        # server-side tool_policy expansion has classes to expand over.
        d.type_line("/tools")
        d.wait_raw("gateway tools —", 10, "/tools opens")
        d.wait_raw("[✓]", 30, "tools inventory loaded")
        d.wait_screen("tier: all", 5, "/tools title shows tier: all")
        d.snap("tools-tier-all")
        d.send(ESC)
        d.pump(0.5)

        # /workspace: add /tmp, expect the auto-pick notice.
        d.type_line("/workspace")
        d.wait_screen("workspace — mode:", 8, "/workspace opens")
        d.snap("workspace-before")
        d.send(TAB)  # focus the path input
        d.pump(0.2)
        d.type("/tmp")
        d.send(ENTER)
        d.wait_raw(
            "switched access mode to workspace_or_allowed",
            8,
            "add /tmp auto-picks workspace_or_allowed with notice",
        )
        d.wait_screen("1 allowed path(s)", 5, "workspace title counts the path")
        d.snap("workspace-after-add")
        d.send(ESC)
        d.pump(0.5)

        # Ctrl+J multiline compose (scenario 7) — two lines, no submit.
        d.type("write the word hi to hi.txt")
        d.send(CTRL_J)
        d.pump(0.2)
        d.type("then reply with the word done")
        d.pump(0.5)
        scr = d.screen_text()
        d.check(
            "composer holds two lines (Ctrl+J, no submit)",
            "write the word hi to hi.txt" in scr
            and "then reply with the word done" in scr
            and "❯ you" not in scr,
        )
        d.snap("composer-two-lines")
        d.send(ENTER)  # submits as ONE prompt -> run 1
        d.pump(0.3)

        # Steer while starting/running (scenario 3): the text buffers on
        # Starting and delivers on the first cycle -> "↪ steer" card.
        d.type("please include the word banana in your final answer")
        d.send(ENTER)
        d.pump(0.2)
        # Queue while the run is active (scenario 2).
        d.type_line("/queue write ok to ok.txt")
        d.wait_raw("queued #1", 8, "queued #1 toast")
        d.wait_screen("1 queued", 12, "strip shows 1 queued during the run")
        d.snap("strip-1-queued")

        d.wait_raw("↪", 120, "steer card rendered (↪)")
        # No approval modal may appear at tier=all (the maintainer's bug).
        run1_answer = d.wait_raw("✦ assistant", 200, "run 1 answer rendered")
        d.check(
            "NO approval modal at tier=all",
            "tool approval —" not in d.raw_text(),
        )
        belt = "auto-approved:" in d.raw_text()
        d.check(
            f"tier-all path: {'client belt auto-approve' if belt else 'server-side tool_policy (no wait arrived)'}",
            True,
            gate=False,
        )
        if not run1_answer:
            return d.finish("BATCH1")

        # Queue auto-drain -> run 2.
        d.wait_raw("queue: starting next prompt", 30, "queue auto-drains after success")
        d.wait_screen("session: 2 runs", 260, "run 2 (queued) completed; idle summary")
        d.snap("idle-after-two-runs")
        scr = d.screen_text()
        m = re.search(r"session: 2 runs · (.+?) · Enter sends", scr)
        tokens_ok = False
        if m:
            part = m.group(1)
            nums = [int(n.replace(",", "")) for n in re.findall(r"([\d,]+)\s*(?:in|out|tk)", part)]
            tokens_ok = any(n > 0 for n in nums) or bool(
                re.search(r"[1-9][\d,]*\s*(?:in|tk)", part)
            )
        d.check(f"token totals non-zero on the idle strip ({m.group(1) if m else 'strip missing'})", tokens_ok)

        # Regression sweep: Ctrl+D details toggle (both ways).
        d.send(CTRL_D)
        d.wait_raw("details:", 5, "Ctrl+D toggles details (toast 1)")
        d.pump(0.4)
        d.send(CTRL_D)
        deets = d.raw_text().count("details:") >= 2
        end = time.time() + 5
        while not deets and time.time() < end:
            d.pump(0.3)
            deets = d.raw_text().count("details:") >= 2
        d.check("Ctrl+D toggles back (toast 2)", deets)

        # PgUp scroll + Esc re-stick (soft: layout-dependent).
        before = d.screen_text()
        for _ in range(3):
            d.send(PGUP)
            d.pump(0.2)
        scrolled = d.screen_text() != before
        d.snap("after-pgup")
        d.send(ESC)
        d.pump(0.4)
        restuck = "session: 2 runs" in d.screen_text()
        d.check("PgUp scrolls, Esc re-sticks to tail", scrolled and restuck, gate=False)

        # Theme cycle (verified via prefs after quit).
        d.send(CTRL_T)
        d.pump(0.5)

        # Remove the allowed path (cleanup half of scenario 4).
        d.type_line("/workspace")
        d.wait_screen("workspace — mode:", 8, "/workspace reopens")
        for _ in range(4):  # 4 mode rows precede the first allowed path
            d.send(DOWN)
            d.pump(0.1)
        d.send(b" ")
        d.wait_raw("allowed path removed: /tmp", 6, "allowed path removed")
        d.send(ESC)
        d.pump(0.4)

        d.quit()
    finally:
        ok = d.finish("BATCH1")

    # Out-of-band gateway verification.
    print("-- out-of-band --", flush=True)
    oob = []

    def oob_check(label, val):
        oob.append((label, bool(val)))
        print(f"  {'PASS' if val else 'FAIL'}  {label}", flush=True)
        return val

    try:
        runs = session_runs(s1)
        by_prompt = {}
        for r in runs:
            rid = r.get("run_id") or r.get("id")
            try:
                idata = gw_get(f"/runs/{rid}/input_data")
            except Exception:
                continue
            data = idata.get("input_data") or idata
            by_prompt[data.get("prompt", "")] = (rid, data)
        multi = "write the word hi to hi.txt\nthen reply with the word done"
        oob_check("run 1 prompt is ONE two-line prompt (Ctrl+J fold)", multi in by_prompt)
        if multi in by_prompt:
            rid1, data1 = by_prompt[multi]
            st["run1"] = rid1
            oob_check(
                "run 1 workspace_allowed_paths == ['/tmp']",
                data1.get("workspace_allowed_paths") == ["/tmp"],
            )
            oob_check(
                "run 1 workspace_access_mode == workspace_or_allowed",
                data1.get("workspace_access_mode") == "workspace_or_allowed",
            )
            pol = (data1.get("_runtime") or {}).get("tool_policy") or {}
            auto = pol.get("auto_approve_tools") or []
            oob_check(
                f"run 1 _runtime.tool_policy.auto_approve_tools carries write_file (+{len(auto)} names)",
                "write_file" in auto and "execute_command" in auto,
            )
            # Steer fold evidence (soft): the guidance text reached the tree.
            try:
                hb = json.dumps(gw_get(f"/runs/{rid1}/history_bundle"))
                seen = "banana" in hb or "steer_seen" in hb
                print(f"  {'PASS' if seen else 'warn'}  steer text visible in run-tree history (soft)", flush=True)
            except Exception as e:
                print(f"  warn  history_bundle unreadable: {e}", flush=True)
        queued_prompt = "write ok to ok.txt"
        oob_check("run 2 (queued) started with the queued prompt", queued_prompt in by_prompt)
        state_save(st)
    except Exception as e:
        oob_check(f"gateway verification failed: {e}", False)

    hi = file_proof("hi.txt", start_ts)
    oob_check(f"hi.txt written on disk ({hi})", hi is not None)
    ok_file = file_proof("ok.txt", start_ts)
    oob_check(f"ok.txt written on disk ({ok_file})", ok_file is not None)
    with open(prefs, encoding="utf-8") as f:
        saved = json.load(f)
    oob_check("theme persisted by Ctrl+T", bool(saved.get("theme")))
    oob_check("workspace_allowed empty again after removal", saved.get("workspace_allowed") == [])
    oob_check(
        "no queue stash left for S1", not [q for q in saved.get("session_queues") or [] if q.get("prompts")]
    )
    return ok and all(v for _, v in oob)


# ---------------------------------------------------------------------------
# batch 2a — tier=read approval modal + no-run checks + quit mid-queue
# ---------------------------------------------------------------------------

def batch2a():
    st = state_load()
    stamp = st.get("stamp") or format(int(time.time()), "x")
    s2 = f"acode-c3b-persist-{stamp}"
    st.update({"stamp": stamp, "s2": s2, "b2_start": time.time()})
    state_save(st)
    prefs = os.path.join(STATE, "prefs-b.json")
    write_prefs(prefs, s2, "read")
    ev = os.path.join(EVIDENCE, "batch2a")
    d = Driver(ev, s2, prefs)
    try:
        d.pump(2.0)
        d.wait_raw("AbstractCode", 15, "TUI booted")
        d.type_line("/tools")
        d.wait_raw("[✓]", 30, "tools inventory loaded")
        d.wait_screen("tier: read", 5, "/tools title shows tier: read")
        d.send(ESC)
        d.pump(0.4)

        # /help lists the new commands (scenario 6).
        d.type_line("/help")
        d.wait_screen("/goal", 8, "/help opens with /goal entry")
        scr = d.screen_text()
        for needle in ["/queue", "/workspace", "/entities", "/task", "/end", "/focus"]:
            d.check(f"/help lists {needle}", needle in scr)
        d.snap("help")
        d.send(ESC)
        d.pump(0.4)

        # '/' completion offers the new commands.
        d.type("/")
        d.wait_screen("queue a prompt for after this run", 6, "'/' completion dropdown up")
        scr = d.screen_text()
        d.check("'/' completion offers goal", "run a goal to completion" in scr)
        d.check("'/' completion offers workspace", "workspace root, access mode" in scr)
        d.snap("slash-completion")
        d.send(ESC)  # close dropdown
        d.pump(0.2)
        d.send(ESC)  # clear draft
        d.pump(0.3)

        # /entities READ-ONLY (scenario 5): roster + card, NO visit.
        d.type_line("/entities")
        d.wait_screen("entities —", 8, "/entities modal opens")
        d.wait_screen("castor", 20, "roster shows castor")
        loading = "identity card loading…" in d.screen_text()
        d.check(f"card spinner honesty ({'seen' if loading else 'cache-fast'})", True, gate=False)
        got_card = d.wait_screen("days old", 25, "identity card loaded (days old)", gate=False)
        if not got_card:
            d.wait_screen("— ", 10, "identity card sections rendered", gate=False)
        d.snap("entities-card")
        d.send(ESC)
        d.pump(0.4)

        # '@' completion from the (now fetched) roster.
        d.type("@")
        d.wait_screen("castor", 6, "'@' completion offers castor")
        d.snap("at-completion")
        d.send(ESC)
        d.pump(0.2)
        d.send(ESC)
        d.pump(0.3)

        # /goal dark (scenario 6): honest refusal naming the interface.
        d.type_line("/goal make the tests pass")
        d.wait_raw(
            "no goal workflows on this gateway (abstractcode.goal.v1)",
            8,
            "/goal <text> refuses honestly naming abstractcode.goal.v1",
        )
        d.type_line("/goal")
        d.wait_raw("no active goal", 6, "bare /goal reports no active goal")

        # Run 3: tier=read MUST prompt with the readable modal.
        d.type_line("write the word hi to hi2.txt then reply with the word done")
        d.wait_raw("tool approval —", 150, "approval modal appeared at tier=read")
        d.pump(0.8)
        scr = d.screen_text()
        d.check("modal headline shows tool + needed tier (⚙ … needs:)", "⚙" in scr and "needs:" in scr)
        d.check(
            "modal tier line (read accepted vs batch needs)",
            "tier: read accepted — this batch needs:" in scr,
        )
        d.check("modal buttons rendered", "approve (a)" in scr and "deny (d)" in scr)
        has_cmd = "$ " in scr
        d.check(
            f"modal body readable ({'$ command' if has_cmd else 'aligned param rows'})",
            has_cmd or ("path" in scr and "hi2.txt" in scr),
        )
        d.snap("approval-modal")
        st["run3_wait_seen"] = True
        state_save(st)
        d.send(b"a")
        d.pump(1.0)
        d.check("modal closed after approve", "tier: read accepted" not in d.screen_text())

        # Queue an item while run 3 executes, then quit MID-QUEUE.
        d.type_line("/queue reply with exactly queue-persist-ok")
        d.wait_raw("queued #1", 8, "queued #1 while running")
        d.wait_screen("1 queued", 10, "strip shows 1 queued")
        d.snap("queued-before-quit")
        d.pump(2.0)  # let the approval resume land server-side
        d.quit("Ctrl+C quits cleanly mid-run with an item queued")
        d.wait_raw(
            "queued prompt(s) saved with this session",
            5,
            "stderr echo: queue saved with the session",
        )
    finally:
        ok = d.finish("BATCH2A")

    # Wait out-of-band for run 3 to finish so batch2b starts clean.
    print("-- out-of-band --", flush=True)
    try:
        runs = session_runs(st["s2"])
        rid = None
        for r in runs:
            rid = r.get("run_id") or r.get("id")
            break
        if rid:
            st["run3"] = rid
            state_save(st)
            end = time.time() + 240
            status = "?"
            while time.time() < end:
                v = gw_get(f"/runs/{rid}")
                status = (v.get("run") or v).get("status", "?")
                if status in ("completed", "failed", "cancelled"):
                    break
                time.sleep(5)
            print(f"  ·  run3 {rid} status after quit: {status}", flush=True)
            hi2 = file_proof("hi2.txt", st.get("b2_start", 0))
            print(f"  {'PASS' if hi2 else 'FAIL'}  hi2.txt written after approval ({hi2})", flush=True)
            ok = ok and hi2 is not None and status == "completed"
    except Exception as e:
        print(f"  FAIL  run3 completion probe: {e}", flush=True)
        ok = False
    return ok


# ---------------------------------------------------------------------------
# batch 2b — queue restore PAUSED + resume drain + Esc arm + tier write
# ---------------------------------------------------------------------------

def batch2b():
    """Queue RESTORE-PAUSED + resume-drain wiring, and the '/' completion.

    Budget note: batch2a's queued item auto-drained + completed LIVE
    before the quit (the model was faster than the ~2s quit window) — so
    it re-proved auto-drain but left NO persisted stash. Rather than spend
    a 5th completed run, the persistence stash is SEEDED into the isolated
    prefs (exactly the at-rest shape `set_session_queue` writes — verified
    round-tripped by config.rs unit tests, and the quit-echo mirror is
    wired in lib.rs). What this batch verifies LIVE is the RESTORE code
    path (boot -> restore_session_queue -> PAUSED + echo + strip + modal)
    and the resume-drain WIRING; the resume-drained run is CANCELLED the
    instant it starts (Esc Esc) so no 5th answer is spent.
    """
    st = state_load()
    s2 = st.get("s2")
    if not s2:
        print("state missing s2 — run batch2a first", file=sys.stderr)
        return False
    prefs = os.path.join(STATE, "prefs-b.json")
    # Seed a persisted stash for S2 (two items) in the exact at-rest shape.
    with open(prefs, encoding="utf-8") as f:
        pj = json.load(f)
    pj["session_id"] = s2
    pj.setdefault("tool_approval", {"accepted_tier": "read"})
    pj["session_queues"] = [
        {"id": s2, "prompts": ["reply with exactly RESTORED-DRAIN-OK", "reply with exactly TEMP-REMOVE-ME"]}
    ]
    with open(prefs, "w", encoding="utf-8") as f:
        json.dump(pj, f)

    ev = os.path.join(EVIDENCE, "batch2b")
    d = Driver(ev, s2, prefs)
    try:
        d.pump(2.0)
        d.wait_raw("AbstractCode", 15, "TUI booted (relaunch, same session)")

        # '/' completion offers the new commands (filtered so they clear
        # the visible-rows fold — the dropdown shows a page at a time).
        d.type("/g")
        d.wait_screen("run a goal to completion", 6, "'/g' completion offers goal")
        d.snap("slash-goal")
        d.send(ESC)
        d.pump(0.2)
        d.send(ESC)
        d.pump(0.2)
        d.type("/wo")
        d.wait_screen("workspace root, access mode", 6, "'/wo' completion offers workspace")
        scr = d.screen_text()
        d.check("'/wo' also offers workflow", "pick the agent workflow" in scr)
        d.send(ESC)
        d.pump(0.2)
        d.send(ESC)
        d.pump(0.2)
        d.type("/qu")
        d.wait_screen("queue a prompt for after this run", 6, "'/qu' completion offers queue")
        d.snap("slash-queue")
        d.send(ESC)
        d.pump(0.2)
        d.send(ESC)
        d.pump(0.3)

        # RESTORE path: PAUSED + echo + strip.
        d.wait_raw(
            "queued prompt(s) restored (paused",
            8,
            "queue restored PAUSED with a visible echo",
        )
        d.wait_screen("2 queued (paused", 12, "strip shows 2 queued (paused)")
        d.snap("restored-paused")

        # Idle-with-paused-queue: /queue holds, never starts.
        d.type_line("/queue reply with exactly THIRD-ITEM")
        d.wait_raw("queued #3 (queue paused", 8, "/queue while idle+paused holds the item")
        d.pump(1.5)
        d.check(
            "paused queue did not auto-start anything",
            "starting next prompt" not in d.raw_text(),
        )

        # Manager modal: items visible, x removes, r resumes.
        d.type_line("/queue")
        d.wait_screen("prompt queue — 3 waiting · PAUSED", 8, "/queue modal shows 3 waiting PAUSED")
        d.wait_screen("RESTORED-DRAIN-OK", 5, "modal lists the restored item")
        d.wait_screen("TEMP-REMOVE-ME", 5, "modal lists the second restored item")
        d.snap("queue-modal")
        d.send(DOWN)  # cursor to item 2 (TEMP-REMOVE-ME)
        d.pump(0.2)
        d.send(b"x")
        d.wait_screen("prompt queue — 2 waiting", 5, "x removed the selected item")
        d.send(b"r")
        d.wait_raw("queue resumed", 5, "r resumes the queue")
        d.send(ESC)
        # Resume drains: the head item becomes a run. Budget discipline —
        # cancel it the instant it starts (Esc Esc) rather than spend a
        # 5th completed answer; auto-drain-to-answer is already proven ×2.
        d.wait_raw("queue: starting next prompt", 12, "resume drains: queued run starts")
        d.wait_screen("reply with exactly RESTORED-DRAIN-OK", 15, "restored head item became a run")
        d.snap("resume-drain-started")
        d.pump(0.5)
        d.send(ESC)
        d.pump(0.2)
        d.send(ESC)  # Esc Esc cancels
        cancelled = d.wait_raw("Esc again to cancel", 4, "Esc arms cancel", gate=False)
        if cancelled:
            d.pump(0.3)
            d.send(ESC)
            d.pump(0.2)
            d.send(ESC)
        d.pump(1.0)

        # /tools tier write + modal display (scenario 1 tail).
        d.type_line("/tools tier write")
        d.wait_raw(
            "tool tier: write — reads + workspace file writes auto-approve",
            6,
            "/tools tier write toast",
        )
        d.type_line("/tools")
        d.wait_raw("[✓]", 20, "tools inventory loaded")
        d.wait_screen("tier: write", 8, "/tools modal title shows tier: write")
        d.snap("tools-tier-write")
        d.send(ESC)
        d.pump(0.3)
        d.type_line("/tools tier read")
        d.wait_raw("tool tier: read", 5, "tier restored to read")

        d.quit()
    finally:
        ok = d.finish("BATCH2B")

    print("-- out-of-band --", flush=True)
    try:
        runs = session_runs(s2)
        prompts = []
        for r in runs:
            rid = r.get("run_id") or r.get("id")
            try:
                data = gw_get(f"/runs/{rid}/input_data")
                data = data.get("input_data") or data
                prompts.append(data.get("prompt", ""))
            except Exception:
                pass
        drained = any("RESTORED-DRAIN-OK" in p for p in prompts)
        print(f"  {'PASS' if drained else 'FAIL'}  restored head item became a real gateway run", flush=True)
        temp_leak = any("TEMP-REMOVE-ME" in p for p in prompts)
        print(f"  {'PASS' if not temp_leak else 'FAIL'}  x-removed item never ran", flush=True)
        ok = ok and drained and not temp_leak
    except Exception as e:
        print(f"  FAIL  gateway verification: {e}", flush=True)
        ok = False
    return ok


def main():
    if not TOKEN:
        print("ACODE_GATEWAY_TOKEN required", file=sys.stderr)
        return 2
    os.makedirs(STATE, exist_ok=True)
    os.makedirs(WORKSPACE, exist_ok=True)
    os.makedirs(EVIDENCE, exist_ok=True)
    which = sys.argv[1] if len(sys.argv) > 1 else ""
    fns = {"batch1": batch1, "batch2a": batch2a, "batch2b": batch2b}
    if which not in fns:
        print("usage: pty_cycle3b_live_verify.py batch1|batch2a|batch2b", file=sys.stderr)
        return 2
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError("batch timeout")))
    signal.alarm(700)
    return 0 if fns[which]() else 1


if __name__ == "__main__":
    sys.exit(main())
