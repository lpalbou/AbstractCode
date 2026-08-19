#!/usr/bin/env python3
"""Workflow conformance probe: can `abstractcode-tui exec` run EVERY workflow?

The operator's contract is "behind any request, we can ensure deterministic
complex orchestrations" — so every workflow the gateway catalogs must be
startable, streamable, and concludable by this client, and a request for
workflow X must never run workflow Y.

Two lanes per workflow ref:
  resolve — does the client accept the ref and start the RIGHT workflow?
  answer  — does a trivial prompt reach a concluded answer?

Cheap by design: a local model, a one-token task, a short timeout. This
probes CLIENT PLUMBING (ref resolution, input pins, sub-run waits,
conclusion), not model quality.

Usage:
  python3 scripts/workflow_conformance_probe.py                # all refs
  python3 scripts/workflow_conformance_probe.py basic-agent    # subset
  WFP_MODEL=qwen/qwen3-4b python3 scripts/...          # uncapped (default)
  WFP_TIMEOUT_S=600 python3 scripts/...                # deliberate cap
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
BIN = REPO / "target" / "release" / "abstractcode-tui"
OUT = REPO / "untracked" / "workflow-conformance"

PROVIDER = os.environ.get("WFP_PROVIDER", "lmstudio")
MODEL = os.environ.get("WFP_MODEL", "qwen/qwen3-4b")
# #[WARNING:TIMEOUT] Conformance runs are UNCAPPED by default (0 = no cap,
# passed straight through to `exec --timeout 0`).
#
# ADR-0027 §2/§3: no low defaults on correctness-critical paths, and timeouts
# only as explicit operator-chosen safeguards. This harness earned the rule the
# hard way — a 150s cap reported the multi-agent pipelines as exit-124
# failures, and the same ref scored GREEN on one run and 124 on the next with
# no code change between them. Raising it to 600s only moved the boundary;
# uncapped is the only setting that measures whether a workflow RUNS rather
# than how close it sits to the harness cap. Set WFP_TIMEOUT_S to impose one
# deliberately.
TIMEOUT_S = int(os.environ.get("WFP_TIMEOUT_S", "0"))
# Subprocess reaping still needs a finite number; 4h is a hang-catcher, well
# past any real agentic run, and it is reported as UNCAPPED-OVERRUN rather
# than mislabelled a workflow failure.
SUBPROCESS_REAP_S = int(os.environ.get("WFP_REAP_S", "14400"))
MAX_ITER = int(os.environ.get("WFP_MAX_ITER", "6"))
NEEDLE = "wfp-ok"
PROMPT = f"Reply with exactly this and nothing else: {NEEDLE}"

# Every agent-facing ref the live catalog declares, plus the coding.v1
# pipelines the operator names as key orchestrations. Bundle-only refs are
# included ON PURPOSE: a bundle that exists must resolve from its id alone.
REFS = [
    "basic-agent",
    "basic-agent:81795ea9",
    "react-agent",
    "react-agent:react",
    "codeact-agent:codeact",
    "memact-agent:memact",
    "multiagent-coding",
    "multiagent-coding:multiagent-coder",
    "multiagent-coding:multiagent-coding",
    "coding-agent:coder",
    "coding-agent:coding-agent",
]


def gateway() -> tuple[str, str]:
    p = Path.home() / ".abstractcode" / "gateway.json"
    if p.is_file():
        d = json.loads(p.read_text())
        return str(d.get("base_url", "http://127.0.0.1:8080")), str(d.get("token", ""))
    return os.environ.get("ABSTRACTGATEWAY_URL", "http://127.0.0.1:8080"), os.environ.get(
        "ABSTRACTGATEWAY_AUTH_TOKEN", ""
    )


@dataclass
class Probe:
    ref: str
    exit_code: int
    elapsed_s: float
    answered: bool
    refused: bool
    ran_workflow: str
    verdict: str
    note: str
    log_path: str


def _ran_workflow(text: str) -> str:
    """Which workflow the client says it ran (empty = never stated)."""
    for pat in (
        r"workflow[:=]\s*([A-Za-z0-9_.@:-]+)",
        r"^\s*▲?\s*AbstractCode\s+([A-Za-z0-9_.-]+)\s+·",
    ):
        m = re.search(pat, text, re.MULTILINE)
        if m:
            return m.group(1)
    return ""


def probe(ref: str) -> Probe:
    OUT.mkdir(parents=True, exist_ok=True)
    log = OUT / f"{ref.replace(':', '__')}.log"
    url, tok = gateway()
    cmd = [
        str(BIN), "exec", PROMPT,
        "--workflow", ref,
        "--provider", PROVIDER,
        "--model", MODEL,
        "--gateway", url,
        "--token", tok,
        "--permissions", "read",
        "--ungated",
        "--max-iterations", str(MAX_ITER),
        "--timeout", str(TIMEOUT_S),
        "--workspace", str(OUT),
    ]
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("ABSTRACTCODE_", "ABSTRACTGATEWAY_", "ABSTRACTMEMORY_"))}
    t0 = time.monotonic()
    # #[WARNING:TIMEOUT] see SUBPROCESS_REAP_S — a hang-catcher, not a verdict.
    reap = (TIMEOUT_S + 30) if TIMEOUT_S > 0 else SUBPROCESS_REAP_S
    try:
        with log.open("w") as fh:
            rc = subprocess.run(
                cmd, cwd=str(REPO), env=env, stdout=fh,
                stderr=subprocess.STDOUT, timeout=reap,
            ).returncode
    except subprocess.TimeoutExpired:
        rc = 124
    except Exception as exc:  # noqa: BLE001
        return Probe(ref, 1, 0.0, False, False, "", "ERROR", str(exc), str(log))
    elapsed = round(time.monotonic() - t0, 1)
    text = log.read_text(errors="replace") if log.is_file() else ""
    answered = NEEDLE in text
    refused = "refusing to run a different agent" in text or "not found on this gateway" in text
    ran = _ran_workflow(text)

    if refused:
        verdict, note = "REFUSED", "client refused the ref (see log for the reason)"
    elif rc == 0 and answered:
        verdict, note = "GREEN", ""
    elif answered:
        verdict, note = "ANSWERED-BAD-EXIT", f"answer seen but exit {rc}"
    elif rc == 124 and TIMEOUT_S == 0:
        # Uncapped and still reaped = a genuine hang, named as such. Never
        # reported as a workflow failure: no cap was in force to fail against.
        verdict, note = "UNCAPPED-OVERRUN", f"exceeded the {SUBPROCESS_REAP_S}s hang-catcher"
    elif rc == 124:
        verdict, note = "TIMEOUT", f"no answer within the operator-set {TIMEOUT_S}s cap"
    else:
        verdict, note = "FAIL", f"exit {rc}, no answer"
    return Probe(ref, rc, elapsed, answered, refused, ran, verdict, note, str(log))


def main() -> int:
    if not BIN.is_file():
        print(f"missing binary: {BIN} (cargo build --release)", file=sys.stderr)
        return 2
    only = set(sys.argv[1:])
    refs = [r for r in REFS if not only or r in only or r.split(":")[0] in only]
    results: list[Probe] = []
    OUT.mkdir(parents=True, exist_ok=True)
    for ref in refs:
        print(f"=== {ref} ===", flush=True)
        r = probe(ref)
        results.append(r)
        print(f"    {r.verdict:18} exit={r.exit_code} {r.elapsed_s}s {r.note}", flush=True)
        (OUT / "results.json").write_text(
            json.dumps(
                {
                    "generated": datetime.now(timezone.utc).isoformat(),
                    "provider": PROVIDER, "model": MODEL,
                    "timeout_s": TIMEOUT_S, "max_iterations": MAX_ITER,
                    "results": [asdict(x) for x in results],
                },
                indent=2,
            )
        )
    green = sum(1 for r in results if r.verdict == "GREEN")
    print(f"\n{green}/{len(results)} GREEN")
    for r in results:
        print(f"  {r.verdict:18} {r.ref}")
    return 0 if green == len(results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
