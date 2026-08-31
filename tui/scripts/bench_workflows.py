#!/usr/bin/env python3
"""Workflow bench: abstractcode only, one arm per agentic-loop design.

The point is NOT a leaderboard: it is to see whether loop STRUCTURE (plain
ReAct vs fresh-context Ralph cycles vs builder+verifier gates vs multi-agent
pipeline) changes what actually gets built, so the loops themselves can be
improved. Every arm runs the same client, same prompt, same route:
gpt-5.4 / medium on the local airelays relay (subscription, no API key).

Arms (gateway bundle:flow, all abstractcode.agent.v1 chat entries):
  react     react-coding:react-coder          llm_call+tool_calls loop, no agent node
  ralph     ralph-coding:ralph-coder          fixed prompt, FRESH subflow per cycle
  coder     coding-agent:coder                builder + independent verifier + gates
  multi     multiagent-coding:multiagent-coder scouts->planner->builder->doc pipeline

Isolation is the hardened design that survived the benchmark-capture incident:
workspace outside the framework tree, git-init'd so project-root walks stop,
product archived after exit then the build tree deleted, stray-write and
grader-read detectors, infra failures retried instead of consuming verdicts.

Usage:
  python3 scripts/bench_workflows.py            # 4 arms x 3 repeats
  python3 scripts/bench_workflows.py --arms react,ralph --repeats 1 --smoke
  python3 scripts/bench_workflows.py --dry-run
"""
from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
import re
import random
import shutil
import signal
import threading
import subprocess
import sys
import time
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
OUT = REPO / "untracked" / os.environ.get("WB_OUT", "workflow-bench")
TUI_BIN = REPO / "target" / "release" / "abstractcode"
RUNTIME_STORE = Path(os.environ.get(
    "WB_RUNTIME_STORE", "/workspace/runtime"))

MODEL = "gpt-5.4"
REASONING = "medium"
PROVIDER = "endpoint:airelay"
BASE_URL = "http://127.0.0.1:8317/v1"
REPEATS = int(os.environ.get("WB_REPEATS", "3"))

# #[WARNING:TIMEOUT] Uncapped (ADR-0014/0027) — the multi-agent pipeline can
# legitimately run a long time. Finite reap is a hang-catcher, reported as an
# overrun, never a quality verdict.
REAP_S = int(os.environ.get("WB_REAP_S", "14400"))

# Parallel workers (operator ruling 2026-08-01): runs are isolated per-run
# (own workspace outside the tree, own gateway run, own log/product), so 3-5
# can execute concurrently; the relay handles the load. Submissions are
# staggered with jitter so a quota blip doesn't hit all workers at once.
PARALLEL = max(1, int(os.environ.get("WB_PARALLEL", "4")))

ARMS = {
    "react": "react-coding:react-coder",
    "ralph": "ralph-coding:ralph-coder",
    "coder": "coding-agent:coder",
    "multi": "multiagent-coding:multiagent-coder",
    "spec": "spec-coding:spec-coder",
    "specstd": "spec-std-coding:spec-std-coder",
}

# Per-arm extra CLI args. Ralph gets a deterministic verify_command (via the
# new wrapper pass-through pins + the client's new --param): without one its
# completion check degenerates to grepping a model-written DONE: marker —
# ralph-3 exploited exactly that at cycle 8/8.
ARM_EXTRA = {
    "ralph": [
        "--param",
        ("verify_command=set -e; test -s index.html; "
         "for f in $(find . -name '*.js' -not -path './.git/*'); do node --check \"$f\"; done"),
        "--param", "max_steps_per_cycle=16",
    ],
}

ZELDA_PROMPT = (
    "create a fully playable Zelda game in black & white, gameboy style (but scale to 600px). "
    "integrate game mechanics, maps, dungeons, treasures, monsters, boss, equipments and various "
    "weapons and effects and procedural VFX, SFX and music. must be playable with the arrows too. "
    "the game must be composed of a campaign with quests following the original Zelda stories "
    "spread across vast maps. Completing a quest must reveal part of the story and provide a new "
    "game mechanics with explanations. The transition between maps of different designs and "
    "aesthetics must be coherent, including in dungeons. There must be villages to replenish our "
    "equipments and talk with NPCs with entertaining stories related to the local map (each map "
    "has its own story and influence NPCs around it. There must be some magic and a sense of "
    "adventure and discovery. NPCs can have side quests to further explore the story of the map. "
    "please take extra care to the graphics, VFX, SFX and create a sound track suitable for this "
    "game and a gameboy. write the full game in local folder"
)
# Operator-scenario thin prompt (the derived-standards test): a user who did
# NOT state requirements. spec should hold the agent only to what it can
# extract (~2 items); spec-std should hold it to the field's standards.
THIN_PROMPT = (
    "Create a small browser game in this folder. Something fun with a canvas."
)

SMOKE_PROMPT = (
    "Create a minimal but genuinely playable browser game in this folder: index.html plus "
    "game.js. A <canvas>, a player square moved with the arrow keys via "
    "addEventListener('keydown'), a requestAnimationFrame loop redrawing every frame, one wall "
    "the player cannot cross, and one beep via AudioContext. Under 150 lines, no build step."
)


@dataclass
class Run:
    arm: str
    rep: int
    workflow: str = ""
    out_dir: str = ""
    started_at: str = ""
    elapsed_s: float = 0.0
    exit_code: int = -1
    verdict: str = "PENDING"
    discard_reason: str = ""
    infra_failure: bool = False
    run_id: str = ""
    wire_model: object = None
    wire_thinking: object = None
    llm_calls: int = 0
    tool_calls: int = 0
    iterations_used: object = None
    tokens_in: int = 0
    tokens_out: int = 0
    file_count: int = 0
    total_bytes: int = 0
    log_path: str = ""
    archived_product: str = ""
    stray_writes: list = field(default_factory=list)
    loadavg: float = 0.0
    notes: str = ""


def gateway() -> tuple[str, str]:
    d = json.loads((Path.home() / ".abstractcode/gateway.json").read_text())
    return d.get("base_url", "http://127.0.0.1:8080"), d.get("token", "")


def model_available() -> bool:
    try:
        out = subprocess.run(["curl", "-s", "-m", "15", f"{BASE_URL}/models"],
                             capture_output=True, text=True, timeout=30).stdout
        return MODEL in {m.get("id") for m in (json.loads(out).get("data") or [])}
    except Exception:  # noqa: BLE001
        return False


def repo_dirty() -> set[str]:
    OPERATOR_AREAS = ("scripts/", "docs/", "untracked/", ".git/")
    try:
        out = subprocess.run(["git", "status", "--porcelain"], cwd=str(REPO),
                             capture_output=True, text=True, timeout=60).stdout
        return {ln[3:].strip() for ln in out.splitlines() if ln.strip()
                and not ln[3:].strip().startswith(OPERATOR_AREAS)}
    except Exception:  # noqa: BLE001
        return set()


def parse_log(text: str, r: Run) -> None:
    m = re.search(r"run\s+([0-9a-f-]{8,})", text)
    if m:
        r.run_id = m.group(1)
    m = re.search(r"(?:done(?::\s*\w+)?|stopped[^·\n]*)\s·\s(\d+)\s+llm calls\s·\s(\d+)\s+tools", text)
    if m:
        r.llm_calls, r.tool_calls = int(m.group(1)), int(m.group(2))
    m = (re.search(r"(\d+)\s*↑\s*(\d+)\s*↓\s*tk", text)
         or re.search(r"(\d+)()\s*tk total", text))
    if m:
        r.tokens_in = int(m.group(1))
        r.tokens_out = int(m.group(2) or 0)


def read_store(r: Run) -> None:
    """Route verification from the runtime's own durable record — never from
    what this harness intended to send."""
    if not r.run_id:
        return
    p = RUNTIME_STORE / f"run_{r.run_id}.json"
    if not p.is_file():
        return
    try:
        v = json.loads(p.read_text()).get("vars") or {}
    except Exception:  # noqa: BLE001
        return
    rt = v.get("_runtime") or {}
    r.wire_model = rt.get("model")
    r.wire_thinking = rt.get("thinking")
    r.iterations_used = (v.get("_limits") or {}).get("current_iteration")


def count_tree(root: Path) -> tuple[int, int]:
    n = b = 0
    for p in root.rglob("*"):
        if p.is_file() and ".git" not in p.parts:
            n += 1
            b += p.stat().st_size
    return n, b


_INFLIGHT: dict = {"procs": {}}
_LOCK = threading.Lock()


def _abort(signum, _frame):
    with _LOCK:
        procs = list(_INFLIGHT["procs"].values())
    for p in procs:
        if p and p.poll() is None:
            p.terminate()
    print(f"\n⚠ aborted — terminated {len(procs)} in-flight run(s)", flush=True)
    raise SystemExit(130)


def one_run(arm: str, rep: int, ws_root: Path, prompt: str) -> Run:
    r = Run(arm=arm, rep=rep, workflow=ARMS[arm])
    out_dir = ws_root / f"{arm}-{rep}" / "product"
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True)
    for anc in (out_dir.parent, ws_root):
        strays = [q for q in anc.rglob("game.js")] + [q for q in anc.rglob("index.html")]
        strays = [q for q in strays if not q.is_relative_to(out_dir)]
        assert not strays, f"game artifact reachable from {anc}: {strays[:2]}"
    # Root-walk stopper (benchmark-capture lesson).
    subprocess.run(["git", "init", "-q"], cwd=str(out_dir), capture_output=True, timeout=60)

    # Build in the WORKSPACE ROOT — "local folder" = the workspace. The
    # earlier exact-directory redirect (an absolute path the gateway clamp
    # guarantees is outside the real workspace) fought every loop that runs
    # workspace-side machinery: ralph's verify_command and PLAN/PROGRESS
    # memory, spec's coverage probes. It split ralph-1's memory in half.
    # Products are harvested from the minted workspace after the run.
    prompt = (f"{prompt}\n\nBuild in the current workspace root — the 'local "
              f"folder' is the workspace itself. Keep any planning/progress "
              f"files (PLAN.md, PROGRESS.md) at the workspace root too.")
    gw_url, gw_tok = gateway()
    cmd = [
        str(TUI_BIN), "exec", prompt,
        "--workflow", ARMS[arm],
        *ARM_EXTRA.get(arm, []),
        "--provider", PROVIDER, "--model", MODEL,
        "--gateway", gw_url, "--token", gw_tok,
        "--reasoning", REASONING, "--permissions", "all", "--ungated",
        "--max-iterations", "120", "--timeout", "0",
        "--workspace", str(out_dir), "--workspace-mode", "workspace_only",
        "--no-project-context",
    ]
    log = OUT / f"{arm}-{rep}.log"
    r.log_path, r.out_dir = str(log), str(out_dir)
    r.started_at = datetime.now(timezone.utc).isoformat()
    r.loadavg = round(os.getloadavg()[0], 2)
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("ABSTRACTCODE_", "ABSTRACTGATEWAY_", "ABSTRACTMEMORY_"))
           and not k.endswith(("_API_KEY", "_APIKEY"))}
    before = repo_dirty()
    t0 = time.monotonic()
    try:
        with log.open("w") as fh:
            p = subprocess.Popen(cmd, cwd=str(out_dir), env=env,
                                 stdout=fh, stderr=subprocess.STDOUT)
            with _LOCK:
                _INFLIGHT["procs"][f"{arm}-{rep}"] = p
            r.exit_code = p.wait(timeout=REAP_S)
    except subprocess.TimeoutExpired:
        p.kill()
        r.exit_code = 124
    except Exception as exc:  # noqa: BLE001
        r.exit_code, r.notes = 1, str(exc)[:200]
    finally:
        with _LOCK:
            _INFLIGHT["procs"].pop(f"{arm}-{rep}", None)
    r.elapsed_s = round(time.monotonic() - t0, 1)
    r.stray_writes = sorted(repo_dirty() - before)[:10]
    r.file_count, r.total_bytes = count_tree(out_dir)

    text = log.read_text(errors="replace") if log.is_file() else ""
    parse_log(text, r)
    read_store(r)
    # HARVEST BELT: when the workspace clamp fired and the agent used
    # workspace-scoped tools (write_file), its product sits in the gateway's
    # minted workspace, not here. Copy it out rather than scoring a real
    # product as "no files produced". Recorded in notes — a harvested run is
    # honest about where its files came from.
    if r.file_count == 0 and r.run_id:
        sp = RUNTIME_STORE / f"run_{r.run_id}.json"
        try:
            v = json.loads(sp.read_text()).get("vars") or {}
            mint = ((v.get("_runtime") or {}).get("workspace_root")
                    or v.get("workspace_root") or "")
            mp = Path(str(mint))
            if mp.is_dir() and mp != out_dir:
                for q in mp.rglob("*"):
                    if not q.is_file() or ".git" in q.parts:
                        continue
                    if q.name == ".abstractgateway-workspace.json":
                        continue
                    rel = q.relative_to(mp)
                    dest = out_dir / rel
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(q, dest)
                r.file_count, r.total_bytes = count_tree(out_dir)
                if r.file_count:
                    r.notes = (f"harvested {r.file_count} files from the gateway's "
                               f"minted workspace {mp.name} (workspace_root clamp)")
        except Exception:  # noqa: BLE001
            pass
    tl = text.lower()
    if "zelda_review_score" in tl or "zelda_headless_bench" in tl:
        r.verdict, r.discard_reason = (
            "DISCARD", "read or ran the grader — benchmark capture, not a coding result")
    elif ("at their limits" in tl or "upstream connection failed" in tl
          or (r.exit_code != 0 and r.llm_calls == 0 and "available models" in tl)):
        r.infra_failure = True
        r.verdict, r.discard_reason = "INFRA", "relay/upstream unavailable (not an agent failure)"
    elif r.wire_model is not None and str(r.wire_model) != MODEL:
        r.verdict, r.discard_reason = "DISCARD", f"model drift: ran {r.wire_model!r}"
    elif r.wire_thinking is not None and str(r.wire_thinking) != REASONING:
        r.verdict, r.discard_reason = "DISCARD", f"reasoning drift: ran {r.wire_thinking!r}"
    elif r.exit_code == 124:
        r.verdict, r.discard_reason = "DISCARD", f"exceeded the {REAP_S}s hang-catcher"
    elif r.exit_code == 125:
        r.verdict, r.discard_reason = "DISCARD", "iteration budget exhausted (truncated, not finished)"
    elif r.stray_writes:
        tag = " (parallel window — attribution approximate)" if PARALLEL > 1 else ""
        r.verdict, r.discard_reason = (
            "DISCARD", f"wrote outside the workspace: {r.stray_writes[:3]}{tag}")
    elif r.file_count == 0:
        r.verdict, r.discard_reason = "DISCARD", "no files produced"
    else:
        r.verdict = "VALID"
        if r.exit_code != 0:
            r.notes = f"exit {r.exit_code} with {r.file_count} files produced"

    try:
        keep = OUT / f"{arm}-{rep}-product"
        if keep.exists():
            shutil.rmtree(keep)
        shutil.copytree(out_dir, keep, ignore=shutil.ignore_patterns(".git"))
        r.archived_product = str(keep)
        r.out_dir = str(keep)
        shutil.rmtree(out_dir.parent, ignore_errors=True)
    except Exception as exc:  # noqa: BLE001
        r.archived_product = f"archive failed: {exc}"
    return r


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--arms", default="react,ralph,coder,multi,spec")
    ap.add_argument("--repeats", type=int, default=REPEATS)
    ap.add_argument("--smoke", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    arms = [x.strip() for x in a.arms.split(",") if x.strip() in ARMS]

    prov = {
        "generated": datetime.now(timezone.utc).isoformat(),
        "mode": "SMOKE" if a.smoke else "MATRIX",
        "client": "abstractcode",
        "client_version": subprocess.run([str(TUI_BIN), "--version"],
                                         capture_output=True, text=True).stdout.strip(),
        "model": MODEL, "reasoning": REASONING, "provider": PROVIDER,
        "base_url": BASE_URL, "subscription_backed": True, "api_key_used": False,
        "repeats": a.repeats,
        "arms": {k: ARMS[k] for k in arms},
        "purpose": "compare agentic-loop designs to find loop improvements, not to rank clients",
    }
    print(json.dumps(prov, indent=2))
    if a.dry_run:
        for rep in range(1, a.repeats + 1):
            for arm in arms:
                print(f"  {arm}-{rep}  ({ARMS[arm]})")
        return 0
    if OUT.exists() and any(OUT.iterdir()):
        print(f"✗ refusing to start: {OUT} exists and is not empty — archive it first.",
              file=sys.stderr)
        return 2
    if not model_available():
        print(f"✗ {MODEL} not advertised by the relay — quota window closed; retry later.",
              file=sys.stderr)
        return 2
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "provenance.json").write_text(json.dumps(prov, indent=2))
    signal.signal(signal.SIGINT, _abort)
    signal.signal(signal.SIGTERM, _abort)

    # OUT name + pid in the path: two lanes launched within the same second
    # collided on identical ws_roots (same arm/rep dirs -> mkdir File exists +
    # the isolation assert tripping on the SIBLING lane's product) and killed
    # 4 of 6 cells instantly.
    # IN-SCOPE by necessity: the gateway now REFUSES an out-of-scope
    # workspace_root with 400 instead of silently clamping (backlog 0232 §1,
    # the fail-loud fix). /private/tmp is outside the operator roots, so builds
    # live in a dedicated game-free dir inside the allowed tree.
    ws_root = Path(os.environ.get(
        "BENCH_WS_BASE", "/workspace/.bench-ws")) / (
        f"wb-{OUT.name}-{os.getpid()}-"
        f"{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}")
    ws_root.mkdir(parents=True, exist_ok=True)
    base_prompt = SMOKE_PROMPT if a.smoke else ZELDA_PROMPT
    if os.environ.get("WB_PROMPT") == "thin":
        base_prompt = THIN_PROMPT
    elif os.environ.get("WB_PROMPT"):
        base_prompt = os.environ["WB_PROMPT"]

    def run_cell(arm: str, rep: int, stagger_s: float) -> Run:
        time.sleep(stagger_s)               # smooth the relay ramp-up
        for attempt in range(1, 4):
            r = one_run(arm, rep, ws_root, base_prompt)
            if not r.infra_failure:
                return r
            print(f"    INFRA    {arm}-{rep} attempt {attempt}/3 — waiting for the relay",
                  flush=True)
            for _ in range(40):
                time.sleep(15)
                if model_available():
                    break
        return r

    reps = list(range(1, a.repeats + 1))
    if os.environ.get("WB_REPS"):
        reps = [int(x) for x in os.environ["WB_REPS"].split(",") if x.strip()]
    cells = [(arm, rep) for rep in reps for arm in arms]
    print(f"parallel workers: {PARALLEL} — {len(cells)} cells", flush=True)
    runs: list[Run] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=PARALLEL) as pool:
        futs = {}
        for i, (arm, rep) in enumerate(cells):
            stagger = i * random.uniform(12.0, 25.0) if i < PARALLEL else 0.0
            futs[pool.submit(run_cell, arm, rep, stagger)] = (arm, rep)
        for fut in concurrent.futures.as_completed(futs):
            arm, rep = futs[fut]
            try:
                r = fut.result()
            except Exception as exc:  # noqa: BLE001
                r = Run(arm=arm, rep=rep, workflow=ARMS[arm], verdict="DISCARD",
                        discard_reason=f"harness error: {str(exc)[:160]}")
            runs.append(r)
            print(f"    {r.verdict:8} {arm}-{rep} exit={r.exit_code} {r.elapsed_s}s "
                  f"files={r.file_count} bytes={r.total_bytes} llm={r.llm_calls} "
                  f"tools={r.tool_calls} {r.discard_reason}", flush=True)
            with _LOCK:
                (OUT / "runs.json").write_text(json.dumps(
                    {"provenance": prov,
                     "runs": [asdict(x) for x in sorted(runs, key=lambda q: (q.rep, q.arm))]},
                    indent=2))

    ok = sum(1 for r in runs if r.verdict == "VALID")
    print(f"\n{ok}/{len(runs)} VALID")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
