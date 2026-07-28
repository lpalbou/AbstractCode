#!/usr/bin/env python3
"""Overnight multi-client coder benchmark (abstractcode, code-tui, codex, opencode, pi).

Operator plan: untracked/overnight-bench/plan.md

Usage:
  python3 scripts/overnight_coder_bench.py readiness   # quick toolchain smoke
  python3 scripts/overnight_coder_bench.py capability    # tier-2 writes (2 retries each)
  python3 scripts/overnight_coder_bench.py all          # readiness then capability

Env:
  OVERNIGHT_BENCH_PROVIDER=endpoint:airelay
  OVERNIGHT_BENCH_MODEL=gpt-5.6-sol
  OVERNIGHT_BENCH_BASE_URL=http://127.0.0.1:8317/v1
  OVERNIGHT_BENCH_RETRIES=2
  OVERNIGHT_BENCH_TIMEOUT_S=600
  OVERNIGHT_BENCH_SMOKE_TIMEOUT_S=120
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
OUT_ROOT = REPO_ROOT / "untracked" / "overnight-bench"
BENCH_ROOT = OUT_ROOT
CODE_TUI_BIN = REPO_ROOT / "target" / "release" / "abstractcode-tui"

PROVIDER = os.environ.get("OVERNIGHT_BENCH_PROVIDER", "endpoint:airelay")
MODEL = os.environ.get("OVERNIGHT_BENCH_MODEL", "gpt-5.6-sol")
BASE_URL = os.environ.get("OVERNIGHT_BENCH_BASE_URL", "http://127.0.0.1:8317/v1")
REASONING = os.environ.get("OVERNIGHT_BENCH_REASONING", "auto")
RETRIES = int(os.environ.get("OVERNIGHT_BENCH_RETRIES", "2"))
TIMEOUT_S = int(os.environ.get("OVERNIGHT_BENCH_TIMEOUT_S", "600"))
SMOKE_TIMEOUT_S = int(os.environ.get("OVERNIGHT_BENCH_SMOKE_TIMEOUT_S", "120"))

SMOKE_PROMPT = "Reply with exactly: overnight-smoke-ok"
CAP_MARKER = "overnight-cap-ok"

_TUI_WORKFLOW = {
    "basic": "basic-agent",
    "react": "react-agent:react",
    "multi-coder": "multiagent-coding:multiagent-coder",
}


@dataclass
class RunResult:
    client: str
    mode: str
    iteration: int
    tier: str
    out_dir: str
    started_at: str
    elapsed_s: float
    exit_code: int
    log_path: str
    file_count: int = 0
    total_bytes: int = 0
    ok: bool = False
    error: str = ""


@dataclass
class BenchReport:
    tier: str
    started_at: str = ""
    finished_at: str = ""
    runs: list[RunResult] = field(default_factory=list)


def _utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _load_gateway() -> tuple[str, str]:
    gw_path = Path.home() / ".abstractcode" / "gateway.json"
    if gw_path.is_file():
        data = json.loads(gw_path.read_text())
        return str(data.get("base_url", "http://127.0.0.1:8080")), str(data.get("token", ""))
    return (
        os.environ.get("ABSTRACTGATEWAY_URL", "http://127.0.0.1:8080"),
        os.environ.get("ABSTRACTGATEWAY_AUTH_TOKEN", ""),
    )


def _clean_env(env: dict[str, str]) -> dict[str, str]:
    out = env.copy()
    for key in list(out):
        if key.startswith(("ABSTRACTCODE_", "ABSTRACTGATEWAY_", "ABSTRACTMEMORY_")):
            out.pop(key, None)
    return out


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


def _write_report(report: BenchReport, name: str) -> Path:
    path = BENCH_ROOT / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(asdict(report), indent=2))
    return path


def _run_subprocess(
    cmd: list[str],
    log_path: Path,
    timeout: int,
    cwd: Path | None = None,
) -> tuple[int, str, float]:
    t0 = time.monotonic()
    log_path.parent.mkdir(parents=True, exist_ok=True)
    env = _clean_env(os.environ)
    try:
        with log_path.open("w") as log:
            proc = subprocess.run(
                cmd,
                cwd=str(cwd or REPO_ROOT),
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                timeout=timeout + 30,
            )
        return proc.returncode, "", round(time.monotonic() - t0, 2)
    except subprocess.TimeoutExpired:
        return 124, "timeout", round(time.monotonic() - t0, 2)
    except Exception as exc:  # noqa: BLE001
        return 1, str(exc), round(time.monotonic() - t0, 2)


def _cap_prompt(out_dir: Path) -> str:
    return (
        f"Create the file {out_dir.resolve()}/hello.txt whose entire content is exactly:\n"
        f"{CAP_MARKER}\n"
        f"Do not write anywhere else."
    )


def run_abstractcode(mode: str, iteration: int, tier: str, prompt: str, timeout: int) -> RunResult:
    agent = "react" if mode == "react" else mode
    out_dir = OUT_ROOT / "out" / "code" / f"{mode}-{iteration}"
    out_dir.mkdir(parents=True, exist_ok=True)
    log_path = BENCH_ROOT / "logs" / f"code-{mode}-{iteration}-{tier}.jsonl"
    started = _utc_now()
    cmd = [
        "abstractcode",
        "exec",
        prompt,
        "--agent",
        agent,
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
        "--reasoning",
        REASONING,
        "--json",
        "--timeout",
        str(timeout),
    ]
    code, err, elapsed = _run_subprocess(cmd, log_path, timeout)
    fc, tb = _count_tree(out_dir)
    ok = code == 0
    if tier == "capability":
        hello = out_dir / "hello.txt"
        ok = ok and hello.is_file() and CAP_MARKER in hello.read_text(errors="replace")
    elif tier == "readiness":
        ok = ok and "overnight-smoke-ok" in log_path.read_text(errors="replace").lower()
    return RunResult(
        client="code",
        mode=mode,
        iteration=iteration,
        tier=tier,
        out_dir=str(out_dir),
        started_at=started,
        elapsed_s=elapsed,
        exit_code=code,
        log_path=str(log_path),
        file_count=fc,
        total_bytes=tb,
        ok=ok,
        error=err,
    )


def run_code_tui(mode: str, iteration: int, tier: str, prompt: str, timeout: int) -> RunResult:
    if not CODE_TUI_BIN.is_file():
        raise FileNotFoundError(f"missing {CODE_TUI_BIN} — run cargo build --release")
    workflow = _TUI_WORKFLOW.get(mode, "basic-agent")
    out_dir = OUT_ROOT / "out" / "code-tui" / f"{mode}-{iteration}"
    out_dir.mkdir(parents=True, exist_ok=True)
    log_path = BENCH_ROOT / "logs" / f"code-tui-{mode}-{iteration}-{tier}.log"
    gw_url, gw_token = _load_gateway()
    started = _utc_now()
    cmd = [
        str(CODE_TUI_BIN),
        "exec",
        prompt,
        "--workflow",
        workflow,
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
        "--reasoning",
        REASONING,
        "--timeout",
        str(timeout),
        "--workspace",
        str(REPO_ROOT),
        "--workspace-mode",
        "workspace_or_allowed",
    ]
    if mode == "multi-coder":
        cmd.append("--ungated")
    code, err, elapsed = _run_subprocess(cmd, log_path, timeout)
    fc, tb = _count_tree(out_dir)
    ok = code == 0
    if tier == "capability":
        hello = out_dir / "hello.txt"
        ok = ok and hello.is_file() and CAP_MARKER in hello.read_text(errors="replace")
    elif tier == "readiness":
        ok = ok and "overnight-smoke-ok" in log_path.read_text(errors="replace").lower()
    return RunResult(
        client="code-tui",
        mode=mode,
        iteration=iteration,
        tier=tier,
        out_dir=str(out_dir),
        started_at=started,
        elapsed_s=elapsed,
        exit_code=code,
        log_path=str(log_path),
        file_count=fc,
        total_bytes=tb,
        ok=ok,
        error=err,
    )


def run_codex(iteration: int, tier: str, prompt: str, timeout: int) -> RunResult:
    out_dir = OUT_ROOT / "out" / "codex" / f"run-{iteration}"
    out_dir.mkdir(parents=True, exist_ok=True)
    log_path = BENCH_ROOT / "logs" / f"codex-{iteration}-{tier}.log"
    started = _utc_now()
    cmd = [
        "codex",
        "exec",
        "-C",
        str(REPO_ROOT),
        "-c",
        f'model="{MODEL}"',
        "-c",
        f'model_provider="{PROVIDER}"',
        prompt,
    ]
    code, err, elapsed = _run_subprocess(cmd, log_path, timeout)
    fc, tb = _count_tree(out_dir)
    ok = code == 0 and "overnight-smoke-ok" in log_path.read_text(errors="replace").lower()
    return RunResult(
        client="codex",
        mode="exec",
        iteration=iteration,
        tier=tier,
        out_dir=str(out_dir),
        started_at=started,
        elapsed_s=elapsed,
        exit_code=code,
        log_path=str(log_path),
        file_count=fc,
        total_bytes=tb,
        ok=ok,
        error=err,
    )


def run_opencode(iteration: int, tier: str, prompt: str, timeout: int) -> RunResult:
    out_dir = OUT_ROOT / "out" / "opencode" / f"run-{iteration}"
    out_dir.mkdir(parents=True, exist_ok=True)
    log_path = BENCH_ROOT / "logs" / f"opencode-{iteration}-{tier}.log"
    started = _utc_now()
    cmd = [
        "opencode",
        "run",
        prompt,
        "-m",
        f"bench8317/{MODEL}",
        "--format",
        "json",
    ]
    code, err, elapsed = _run_subprocess(cmd, log_path, timeout)
    fc, tb = _count_tree(out_dir)
    text = log_path.read_text(errors="replace").lower() if log_path.is_file() else ""
    ok = code == 0 and "overnight-smoke-ok" in text
    return RunResult(
        client="opencode",
        mode="run",
        iteration=iteration,
        tier=tier,
        out_dir=str(out_dir),
        started_at=started,
        elapsed_s=elapsed,
        exit_code=code,
        log_path=str(log_path),
        file_count=fc,
        total_bytes=tb,
        ok=ok,
        error=err,
    )


def run_pi(iteration: int, tier: str, prompt: str, timeout: int) -> RunResult:
    out_dir = OUT_ROOT / "out" / "pi" / f"run-{iteration}"
    out_dir.mkdir(parents=True, exist_ok=True)
    log_path = BENCH_ROOT / "logs" / f"pi-{iteration}-{tier}.log"
    started = _utc_now()
    cmd = [
        "pi",
        "--provider",
        "openai",
        "--model",
        MODEL,
        "--api-key",
        "bench",
        prompt,
    ]
    env = _clean_env(os.environ)
    env["OPENAI_BASE_URL"] = BASE_URL
    t0 = time.monotonic()
    log_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with log_path.open("w") as log:
            proc = subprocess.run(
                cmd,
                cwd=str(REPO_ROOT),
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                timeout=timeout + 30,
            )
        code = proc.returncode
        err = ""
    except subprocess.TimeoutExpired:
        code = 124
        err = "timeout"
    except Exception as exc:  # noqa: BLE001
        code = 1
        err = str(exc)
    elapsed = round(time.monotonic() - t0, 2)
    fc, tb = _count_tree(out_dir)
    text = log_path.read_text(errors="replace").lower() if log_path.is_file() else ""
    ok = code == 0 and "overnight-smoke-ok" in text
    return RunResult(
        client="pi",
        mode="one-shot",
        iteration=iteration,
        tier=tier,
        out_dir=str(out_dir),
        started_at=started,
        elapsed_s=elapsed,
        exit_code=code,
        log_path=str(log_path),
        file_count=fc,
        total_bytes=tb,
        ok=ok,
        error=err,
    )


def _scenarios_readiness() -> list[tuple[str, str, int]]:
    rows: list[tuple[str, str, int]] = []
    for it in range(1, RETRIES + 1):
        rows.append(("code", "react", it))
        rows.append(("code-tui", "basic", it))
        rows.append(("code-tui", "react", it))
        rows.append(("codex", "exec", it))
        rows.append(("opencode", "run", it))
        rows.append(("pi", "one-shot", it))
    return rows


def _scenarios_capability() -> list[tuple[str, str, int]]:
    rows: list[tuple[str, str, int]] = []
    for it in range(1, RETRIES + 1):
        for mode in ("react", "multi-coder"):
            rows.append(("code", mode, it))
        for mode in ("basic", "react", "multi-coder"):
            rows.append(("code-tui", mode, it))
        rows.append(("codex", "exec", it))
        rows.append(("opencode", "run", it))
        rows.append(("pi", "one-shot", it))
    return rows


def run_tier(tier: str) -> BenchReport:
    report = BenchReport(tier=tier, started_at=_utc_now())
    if tier == "readiness":
        scenarios = _scenarios_readiness()
        prompt = SMOKE_PROMPT
        timeout = SMOKE_TIMEOUT_S
    else:
        scenarios = _scenarios_capability()
        prompt = ""  # filled per run
        timeout = TIMEOUT_S

    for client, mode, it in scenarios:
        print(f"=== {tier} {client} {mode} iter {it} ===", flush=True)
        if tier == "capability":
            out_dir = OUT_ROOT / "out" / client / f"{mode}-{it}"
            prompt = _cap_prompt(out_dir)
        try:
            if client == "code":
                res = run_abstractcode(mode, it, tier, prompt, timeout)
            elif client == "code-tui":
                res = run_code_tui(mode, it, tier, prompt, timeout)
            elif client == "codex":
                res = run_codex(it, tier, prompt, timeout)
            elif client == "opencode":
                res = run_opencode(it, tier, prompt, timeout)
            elif client == "pi":
                res = run_pi(it, tier, prompt, timeout)
            else:
                continue
        except Exception as exc:  # noqa: BLE001
            res = RunResult(
                client=client,
                mode=mode,
                iteration=it,
                tier=tier,
                out_dir="",
                started_at=_utc_now(),
                elapsed_s=0.0,
                exit_code=1,
                log_path="",
                ok=False,
                error=str(exc),
            )
        report.runs.append(res)
        partial = BENCH_ROOT / f"report.partial.{tier}.json"
        _write_report(report, partial.name)
        print(
            f"  exit={res.exit_code} ok={res.ok} elapsed={res.elapsed_s}s err={res.error}",
            flush=True,
        )

    report.finished_at = _utc_now()
    _write_report(report, f"report.{tier}.json")
    return report


def main(argv: list[str]) -> int:
    BENCH_ROOT.mkdir(parents=True, exist_ok=True)
    cmd = argv[1] if len(argv) > 1 else "readiness"
    if cmd == "readiness":
        run_tier("readiness")
        return 0
    if cmd == "capability":
        run_tier("capability")
        return 0
    if cmd == "all":
        run_tier("readiness")
        run_tier("capability")
        return 0
    print(f"unknown command {cmd!r} — use readiness|capability|all", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
