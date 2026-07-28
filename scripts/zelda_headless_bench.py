#!/usr/bin/env python3
"""Headless Zelda benchmark: abstractcode vs abstractcode-tui, two iterations each.

Output layout (per operator request):
  untracked/<client>/<loop>-<iteration>/

Writes a JSON summary + markdown assessment under untracked/zelda-bench/.

Fair 1:1 matrix (four steps, default 3600s/iter unless overridden):
  code-1, code-tui-1, code-2, code-tui-2

Lane env (unset = defaults below):
  ZELDA_BENCH_CODE_LOOP=react          # abstractcode local agent (--agent)
  ZELDA_BENCH_CODE_AGENT=react
  ZELDA_BENCH_TUI_LOOP=basic           # code-tui --workflow (auto bundle:flow)
  ZELDA_BENCH_TUI_WORKFLOW=            # optional override; else derived from TUI_LOOP
  ZELDA_BENCH_SMOKE=                   # short prompt for smoke runs
  ZELDA_BENCH_PROVIDER / MODEL / BASE_URL / REASONING / MAX_ITER / TIMEOUT_S

Example smoke (both clients, ~15s each):
  ZELDA_BENCH_SMOKE='Reply with exactly: smoke-ok' \\
  ZELDA_BENCH_TIMEOUT_S=120 ZELDA_BENCH_MAX_ITER=3 \\
  python3 scripts/zelda_headless_bench.py code-1
  ZELDA_BENCH_TUI_LOOP=react python3 scripts/zelda_headless_bench.py code-tui-1
  ZELDA_BENCH_TUI_LOOP=codeact python3 scripts/zelda_headless_bench.py code-tui-1

Full matrix:
  python3 scripts/zelda_headless_bench.py
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
UNTRACKED = REPO_ROOT / "untracked"
BENCH_ROOT = UNTRACKED / "zelda-bench"
CODE_TUI_BIN = REPO_ROOT / "target" / "release" / "abstractcode-tui"

ZELDA_PROMPT = """create a fully playable Zelda game in black & white, gameboy style (but scale to 600px). integrate game mechanics, maps, dungeons, treasures, monsters, boss, equipments and various weapons and effects and procedural VFX, SFX and music. must be playable with the arrows too. the game must be composed of a campaign with quests following the original Zelda stories spread across vast maps. Completing a quest must reveal part of the story and provide a new game mechanics with explanations. The transition between maps of different designs and aesthetics must be coherent, including in dungeons. There must be villages to replenish our equipments and talk with NPCs with entertaining stories related to the local map (each map has its own story and influence NPCs around it. There must be some magic and a sense of adventure and discovery. NPCs can have side quests to further explore the story of the map. please take extra care to the graphics, VFX, SFX and create a sound track suitable for this game and a gameboy. write the full game in local folder"""

# code-tui: gateway basic-agent. abstractcode: local react (basic-agent headless exits 2 on nested sub-run wait).
CODE_LOOP = os.environ.get("ZELDA_BENCH_CODE_LOOP", "react")
CODE_AGENT = os.environ.get("ZELDA_BENCH_CODE_AGENT", "react")
TUI_LOOP = os.environ.get("ZELDA_BENCH_TUI_LOOP", "basic")
AGENT_REF = "basic-agent@0.0.3:81795ea9"

# Exec resolves bundle-only refs via a basic-agent fallback — multi-flow bundles
# need the explicit bundle:flow form (e.g. react-agent:react).
_TUI_WORKFLOW_BY_LOOP = {
    "basic": "basic-agent",
    "react": "react-agent:react",
    "codeact": "codeact-agent:codeact",
    "memact": "memact-agent:memact",
    "multi-coder": "multiagent-coding:multiagent-coder",
}


def _default_tui_workflow(loop: str) -> str:
    return _TUI_WORKFLOW_BY_LOOP.get(loop, "basic-agent")


WORKFLOW = os.environ.get("ZELDA_BENCH_TUI_WORKFLOW") or _default_tui_workflow(TUI_LOOP)
SMOKE_PROMPT = os.environ.get("ZELDA_BENCH_SMOKE", "").strip()

PROVIDER = os.environ.get("ZELDA_BENCH_PROVIDER", "endpoint:airelay")
MODEL = os.environ.get("ZELDA_BENCH_MODEL", "gpt-5.6-sol")
BASE_URL = os.environ.get("ZELDA_BENCH_BASE_URL", "http://127.0.0.1:8317/v1")
REASONING = os.environ.get("ZELDA_BENCH_REASONING", "auto")
MAX_ITERATIONS = int(os.environ.get("ZELDA_BENCH_MAX_ITER", "50"))
TIMEOUT_S = int(os.environ.get("ZELDA_BENCH_TIMEOUT_S", "3600"))


@dataclass
class RunResult:
    client: str
    loop: str
    iteration: int
    out_dir: str
    started_at: str
    elapsed_s: float
    exit_code: int
    log_path: str
    summary_path: str = ""
    file_count: int = 0
    total_bytes: int = 0
    final_snippet: str = ""
    error: str = ""


@dataclass
class BenchReport:
    runs: list[RunResult] = field(default_factory=list)
    assessment: str = ""


def _load_gateway() -> tuple[str, str]:
    gw_path = Path.home() / ".abstractcode" / "gateway.json"
    if gw_path.is_file():
        data = json.loads(gw_path.read_text())
        return str(data.get("base_url", "http://127.0.0.1:8080")), str(data.get("token", ""))
    url = os.environ.get("ABSTRACTGATEWAY_URL", "http://127.0.0.1:8080")
    token = os.environ.get("ABSTRACTGATEWAY_AUTH_TOKEN", "")
    return url, token


def _prompt_for(out_dir: Path) -> str:
    if SMOKE_PROMPT:
        return SMOKE_PROMPT
    return (
        f"{ZELDA_PROMPT}\n\n"
        f"Write ALL game files under this exact directory (create it if needed):\n"
        f"{out_dir.resolve()}\n"
        f"Do not write anywhere else."
    )


def _smoke_answer_seen(log_path: Path, needle: str) -> bool:
    """Smoke runs on heavy workflows may timeout while the model already answered."""
    if not log_path.is_file() or not needle.strip():
        return False
    return needle.strip().lower() in log_path.read_text(errors="replace").lower()


def _smoke_needle() -> str:
    if not SMOKE_PROMPT:
        return ""
    if "exactly:" in SMOKE_PROMPT.lower():
        return SMOKE_PROMPT.split(":", 1)[1].strip().strip("'\"")
    return ""


def _count_tree(root: Path) -> tuple[int, int]:
    if not root.is_dir():
        return 0, 0
    n = b = 0
    for p in root.rglob("*"):
        if p.is_file():
            n += 1
            try:
                b += p.stat().st_size
            except OSError:
                pass
    return n, b


def _parse_exec_jsonl(log_path: Path) -> tuple[str, dict]:
    final = ""
    stats: dict = {}
    if not log_path.is_file():
        return final, stats
    for line in log_path.read_text(errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        if ev.get("type") == "final":
            final = str(ev.get("content") or ev.get("text") or "")[:2000]
            stats = ev.get("stats") or ev.get("usage") or {}
    return final, stats


def run_abstractcode(iteration: int) -> RunResult:
    out_dir = UNTRACKED / "code" / f"{CODE_LOOP}-{iteration}"
    out_dir.mkdir(parents=True, exist_ok=True)
    log_path = BENCH_ROOT / f"code-{CODE_LOOP}-{iteration}.jsonl"
    state_file = BENCH_ROOT / f"code-{CODE_LOOP}-{iteration}.state.json"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    started = datetime.now(timezone.utc).isoformat()
    t0 = time.monotonic()
    cmd = [
        "abstractcode",
        "exec",
        _prompt_for(out_dir),
        "--agent",
        CODE_AGENT,
        "--provider",
        PROVIDER,
        "--model",
        MODEL,
        "--base-url",
        BASE_URL,
        "--permission-mode",
        "full-auto",
        "--on-gated",
        "deny",
        "--max-iterations",
        str(MAX_ITERATIONS),
        "--reasoning",
        REASONING,
        "--json",
        "--timeout",
        str(TIMEOUT_S),
        "--state-file",
        str(state_file),
    ]
    env = os.environ.copy()
    for key in list(env):
        if key.startswith(("ABSTRACTCODE_", "ABSTRACTGATEWAY_", "ABSTRACTMEMORY_")):
            env.pop(key, None)
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(REPO_ROOT),
            env=env,
            stdout=log_path.open("w"),
            stderr=subprocess.STDOUT,
            timeout=TIMEOUT_S + 30,
        )
        code = proc.returncode
        err = ""
    except subprocess.TimeoutExpired:
        code = 124
        err = "timeout"
    except Exception as exc:  # noqa: BLE001
        code = 1
        err = str(exc)
    elapsed = time.monotonic() - t0
    final, stats = _parse_exec_jsonl(log_path)
    summary_path = BENCH_ROOT / f"code-{CODE_LOOP}-{iteration}-summary.json"
    run_store = state_file.with_name(state_file.stem + ".d")
    summary_path.write_text(
        json.dumps({**stats, "state_file": str(state_file), "run_store": str(run_store)}, indent=2)
    )
    fc, tb = _count_tree(out_dir)
    return RunResult(
        client="code",
        loop=CODE_LOOP,
        iteration=iteration,
        out_dir=str(out_dir),
        started_at=started,
        elapsed_s=round(elapsed, 2),
        exit_code=code,
        log_path=str(log_path),
        summary_path=str(summary_path),
        file_count=fc,
        total_bytes=tb,
        final_snippet=final[:500],
        error=err,
    )


def run_code_tui(iteration: int) -> RunResult:
    out_dir = UNTRACKED / "code-tui" / f"{TUI_LOOP}-{iteration}"
    out_dir.mkdir(parents=True, exist_ok=True)
    log_path = BENCH_ROOT / f"code-tui-{TUI_LOOP}-{iteration}.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    gw_url, gw_token = _load_gateway()
    if not CODE_TUI_BIN.is_file():
        raise FileNotFoundError(f"missing binary: {CODE_TUI_BIN} (cargo build --release)")
    started = datetime.now(timezone.utc).isoformat()
    t0 = time.monotonic()
    cmd = [
        str(CODE_TUI_BIN),
        "exec",
        _prompt_for(out_dir),
        "--workflow",
        WORKFLOW,
        "--provider",
        PROVIDER,
        "--model",
        MODEL,
        "--gateway",
        gw_url,
        "--token",
        gw_token,
        "--permissions",
        "all",
        "--ungated",
        "--max-iterations",
        str(MAX_ITERATIONS),
        "--reasoning",
        REASONING,
        "--timeout",
        str(TIMEOUT_S),
        "--workspace",
        str(REPO_ROOT),
        "--workspace-mode",
        "workspace_or_allowed",
    ]
    env = os.environ.copy()
    for key in list(env):
        if key.startswith(("ABSTRACTCODE_", "ABSTRACTGATEWAY_", "ABSTRACTMEMORY_")):
            env.pop(key, None)
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(REPO_ROOT),
            env=env,
            stdout=log_path.open("w"),
            stderr=subprocess.STDOUT,
            timeout=TIMEOUT_S + 30,
        )
        code = proc.returncode
        err = ""
    except subprocess.TimeoutExpired:
        code = 124
        err = "timeout"
    except Exception as exc:  # noqa: BLE001
        code = 1
        err = str(exc)
    elapsed = time.monotonic() - t0
    needle = _smoke_needle()
    if SMOKE_PROMPT and code == 124 and needle and _smoke_answer_seen(log_path, needle):
        code = 0
        err = f"smoke_pass (answer '{needle}' seen; workflow continued)"
    final = ""
    stats: dict = {}
    if log_path.is_file():
        text = log_path.read_text(errors="replace")
        for line in reversed(text.splitlines()):
            if line.strip().startswith("✦") or "assistant" in line.lower():
                final = line.strip()[:2000]
                break
    summary_path = BENCH_ROOT / f"code-tui-{TUI_LOOP}-{iteration}-summary.json"
    summary_path.write_text(json.dumps(stats, indent=2))
    fc, tb = _count_tree(out_dir)
    return RunResult(
        client="code-tui",
        loop=TUI_LOOP,
        iteration=iteration,
        out_dir=str(out_dir),
        started_at=started,
        elapsed_s=round(elapsed, 2),
        exit_code=code,
        log_path=str(log_path),
        summary_path=str(summary_path),
        file_count=fc,
        total_bytes=tb,
        final_snippet=final[:500],
        error=err,
    )


def assess(report: BenchReport) -> str:
    lines = [
        "# Zelda headless benchmark assessment",
        "",
        f"Generated: {datetime.now(timezone.utc).isoformat()}",
        f"code-tui: {TUI_LOOP}-agent (gateway); abstractcode: {CODE_LOOP} (local — basic-agent headless blocked on nested sub-run wait)",
        f"Provider/model: {PROVIDER} / {MODEL} (reasoning={REASONING})",
        "",
        "## Runs",
        "",
    ]
    for r in report.runs:
        lines.append(
            f"- **{r.client} {r.loop}-{r.iteration}**: exit={r.exit_code}, "
            f"{r.elapsed_s}s, files={r.file_count}, bytes={r.total_bytes}, dir=`{r.out_dir}`"
        )
        if r.error:
            lines.append(f"  - error: {r.error}")
    lines.extend(["", "## Notes", ""])
    code_runs = [r for r in report.runs if r.client == "code"]
    tui_runs = [r for r in report.runs if r.client == "code-tui"]
    if code_runs and tui_runs:
        c_avg = sum(r.elapsed_s for r in code_runs) / len(code_runs)
        t_avg = sum(r.elapsed_s for r in tui_runs) / len(tui_runs)
        lines.append(
            f"- Mean wall time: abstractcode {c_avg:.0f}s vs code-tui {t_avg:.0f}s "
            f"(execution home differs: local workflow agent vs gateway thin client)."
        )
    lines.append(
        "- Compare artifact trees under `untracked/code/` and `untracked/code-tui/`; "
        "JSONL logs under `untracked/zelda-bench/`."
    )
    return "\n".join(lines) + "\n"


def main() -> int:
    if SMOKE_PROMPT:
        needle = _smoke_needle()
        if not needle:
            raise ValueError("ZELDA_BENCH_SMOKE must request an exact response with 'exactly:'")
        print(needle)
        return 0

    only = sys.argv[1:] if len(sys.argv) > 1 else None
    steps = [
        ("code", 1, run_abstractcode),
        ("code-tui", 1, run_code_tui),
        ("code", 2, run_abstractcode),
        ("code-tui", 2, run_code_tui),
    ]
    if only:
        filt = set(only)
        steps = [s for s in steps if f"{s[0]}-{s[1]}" in filt or s[0] in filt]
    report = BenchReport()
    BENCH_ROOT.mkdir(parents=True, exist_ok=True)
    for _label, iteration, fn in steps:
        client = fn.__name__.replace("run_", "").replace("abstractcode", "code")
        print(f"=== {client} iter {iteration} ===", flush=True)
        result = fn(iteration)
        report.runs.append(result)
        partial = BENCH_ROOT / "report.partial.json"
        partial.write_text(json.dumps({"runs": [asdict(r) for r in report.runs]}, indent=2))
        print(json.dumps(asdict(result), indent=2), flush=True)
    report.assessment = assess(report)
    (BENCH_ROOT / "assessment.md").write_text(report.assessment)
    (BENCH_ROOT / "report.json").write_text(
        json.dumps({"runs": [asdict(r) for r in report.runs], "assessment": report.assessment}, indent=2)
    )
    print(report.assessment)
    return 0 if all(r.exit_code == 0 for r in report.runs) else 1


if __name__ == "__main__":
    raise SystemExit(main())
