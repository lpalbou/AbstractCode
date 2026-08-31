#!/usr/bin/env python3
"""A/B: does the verifier loop (`review_mode`) improve coding quality?

Hypothesis (operator): the extra verifier loop is what lets `react-agent:react`
produce a genuinely playable game where `basic-agent` produces something basic.

Arm A = `--review --review-rounds 3`; arm B = `--no-review`. Everything else
held identical. `react-agent:react` is the ONLY correct arm: it is a native-loop
bundle, so root run vars ARE the loop's vars and `review_mode` is consumed
(`abstractagent/adapters/react_runtime.py:2241-2264`). On flow-graph bundles
like `basic-agent` the compiler drops the key at the Agent-node boundary, so an
A/B there would measure nothing.

This harness exists because `zelda_headless_bench.py` cannot run the experiment:
it has no review axis, never cleans its output dirs (a complete prior Zelda sits
in `untracked/code-tui/react-1/`), and its answer extraction does not match what
`exec` prints.

Design guards, each answering a specific way this could produce a false result:
  * output dir wiped + asserted empty     — no inherited artifact to "verify"
  * workspace_only, allowed paths cleared — the agent cannot read a prior build
  * MANIPULATION CHECK per run            — the ledger must show the verifier
                                            ran in A and did not in B, else the
                                            run is DISCARDED, not scored
  * equal explicit wall budget both arms  — arm A makes more LLM calls, so an
                                            uncapped reap would censor A more
                                            often than B (treatment-correlated)
  * alternating arm order A,B,B,A,A,B     — machine drift cannot align with arm
  * n=3 per arm                           — with n=2 the best possible exact
                                            permutation p is 1/C(4,2)=0.167, so
                                            significance is UNREACHABLE; n=3
                                            gives 1/C(6,3)=0.05
  * full provenance per run               — binary sha, git HEAD, wire keys as
                                            the gateway actually stored them

Artifacts persist under untracked/zelda-ab/, so scoring runs separately
(`zelda_review_score.py`) and can be re-run without re-spending tokens.

Usage:
  python3 scripts/zelda_review_ab.py            # full 6-run matrix
  python3 scripts/zelda_review_ab.py --dry-run  # print the plan, spend nothing
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
BIN = REPO / "target" / "release" / "abstractcode"
OUT = REPO / "untracked" / "zelda-ab"

# B1 (adversary, CONFIRMED): the workspace root must live OUTSIDE the framework
# tree. `_runtime.tool_policy.auto_approve_tools` includes `execute_command`, and
# the workspace wall only rewrites `working_directory` — abstractruntime's own
# source says "this is policy for the starting point, not a sandbox: once
# running, the session can cd anywhere". Eight complete prior Zelda/R-Type games
# sit within two `cd ..` hops of `untracked/`, and in an archived run the model's
# FIRST action was reconnaissance on the parent directory. `cp -r ../../_archive-*/`
# would have handed it a finished game to present as its own.
# Products are built here; only logs and JSON stay under untracked/.
WS_ROOT = Path(os.environ.get(
    "AB_WS_ROOT",
    f"/private/tmp/zelda-ab-ws-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}",
))

WORKFLOW = "react-agent:react"
PROVIDER = os.environ.get("AB_PROVIDER", "endpoint:airelay")
MODEL = os.environ.get("AB_MODEL", "gpt-5.4")
BASE_URL = os.environ.get("AB_BASE_URL", "http://127.0.0.1:8317/v1")
REASONING = os.environ.get("AB_REASONING", "medium")
# B4 (adversary): arm A burns strictly MORE iterations for the same product —
# verifier-forced tool batches route back to `act` without resetting the review
# budget — so a shared cap censors the treatment arm preferentially. Observed
# usage on this prompt/model is 6-14, so 120 is ~9x headroom and the cap stops
# being a differential.
MAX_ITER = int(os.environ.get("AB_MAX_ITER", "120"))
REVIEW_ROUNDS = int(os.environ.get("AB_REVIEW_ROUNDS", "3"))
REPEATS = int(os.environ.get("AB_REPEATS", "3"))

# B3 (adversary, CONFIRMED): the drift check compared the wire against the same
# env-settable variable it was sent from, so `AB_MODEL=other` rerouted all six
# runs and every check still passed. The operator's route is a CONSTANT here;
# overriding it requires saying so explicitly on the command line.
REQUIRED_ROUTE = ("gpt-5.4", "endpoint:airelay", "medium")
ROUTE_OVERRIDE = "--allow-route-override" in sys.argv

# #[WARNING:TIMEOUT] Deliberate, EQUAL wall budget per run (ADR-0027 §3:
# explicit operator-chosen safeguard). Not a performance knob and not a low
# default — it is a bias control. Arm A issues strictly more LLM calls, so an
# unequal or absent budget would censor arm A more often than arm B and the
# censoring would correlate with the treatment. A run that hits it is DISCARDED,
# never scored. 90 min is ~4x the longest observed Zelda run on this substrate.
WALL_BUDGET_S = int(os.environ.get("AB_WALL_BUDGET_S", "5400"))
# Smoke is a pipeline test, not a quality measurement: 15 min is ample for a
# 150-line game and keeps a wedged smoke from stalling the real matrix.
_SMOKE_BUDGET_S = int(os.environ.get("AB_SMOKE_BUDGET_S", "900"))

ZELDA_PROMPT = (
    "create a fully playable Zelda game in black & white, gameboy style (but scale to 600px). "
    "integrate game mechanics, maps, dungeons, treasures, monsters, boss, equipments and various "
    "weapons and effects and procedural VFX, SFX and music. must be playable with the arrows too. "
    "the game must be composed of a campaign with quests following the original Zelda stories "
    "spread across vast maps. Completing a quest must reveal part of the story and provide a new "
    "game mechanics with explanations. The transition between maps of different designs and "
    "aesthetics must be coherent, including in dungeons. There must be villages to replenish our "
    "equipments and talk with NPCs with entertaining stories related to the local map (each map has "
    "its own story and influence NPCs around it. There must be some magic and a sense of adventure "
    "and discovery. NPCs can have side quests to further explore the story of the map. please take "
    "extra care to the graphics, VFX, SFX and create a sound track suitable for this game and a "
    "gameboy. write the full game in local folder"
)

# Arm order: alternating so machine drift / gateway warmth cannot align with the
# treatment. Not A,A,A,B,B,B.
ARM_ORDER = ["review", "noreview", "noreview", "review", "review", "noreview"]

# `--smoke`: one run per arm on a SMALL but genuinely playable game, into a
# separate root so it can never be mistaken for a result. Exists because two
# full matrices were voided by harness bugs (a filtered API view, a ledger limit
# that failed to empty) that a two-minute end-to-end pass would have caught.
# It exercises every stage the real matrix does, on the same route.
SMOKE = "--smoke" in sys.argv
SMOKE_PROMPT = (
    "Create a minimal but genuinely playable browser game in this folder: "
    "index.html plus game.js. Requirements: a <canvas>, a player square moved "
    "with the arrow keys via addEventListener('keydown'), a requestAnimationFrame "
    "game loop that redraws every frame, at least one wall the player cannot walk "
    "through (collision), and one short beep via AudioContext on collision. "
    "Keep it under 150 lines. No build step, no external files."
)
if SMOKE:
    OUT = REPO / "untracked" / "zelda-ab-smoke"
    ARM_ORDER = ["review", "noreview"]
    WALL_BUDGET_S = _SMOKE_BUDGET_S
    REPEATS = 1


def gateway() -> tuple[str, str]:
    p = Path.home() / ".abstractcode" / "gateway.json"
    if p.is_file():
        d = json.loads(p.read_text())
        return str(d.get("base_url", "http://127.0.0.1:8080")), str(d.get("token", ""))
    return (
        os.environ.get("ABSTRACTGATEWAY_URL", "http://127.0.0.1:8080"),
        os.environ.get("ABSTRACTGATEWAY_AUTH_TOKEN", ""),
    )


# The runtime's own durable store — the ONLY place that holds the truth this
# experiment has to verify.
#
# Learned by having a correct run discarded by a broken check:
#   * `GET /runs/{id}/input_data` is a DECLARED-PINS view. For a native-loop
#     bundle (no declared pins) it strips the underscore namespaces entirely,
#     so `_runtime` reads as null even when review_mode is live. Verifying
#     against it reports every run as unconfigured.
#   * `GET /runs/{id}/ledger?limit=5000` returns ZERO items — above some server
#     cap the endpoint fails to empty instead of clamping. A review-activity
#     count taken from it is always 0.
# Both bugs point the same way: they manufacture false negatives, which would
# have voided the matrix while the client was working correctly.
RUNTIME_STORE = Path(
    os.environ.get("AB_RUNTIME_STORE", str(REPO.parent / "runtime"))
)


def run_vars(run_id: str) -> dict:
    """`vars` as the runtime durably stored them; {} when unavailable."""
    p = RUNTIME_STORE / f"run_{run_id}.json"
    if not p.is_file():
        return {}
    try:
        return json.loads(p.read_text()).get("vars") or {}
    except Exception:  # noqa: BLE001
        return {}


def ledger_node_counts(run_id: str) -> dict[str, int]:
    """Per-node record counts from the durable ledger. `review` > 0 is the
    manipulation check: proof the verifier node actually executed."""
    p = RUNTIME_STORE / f"ledger_{run_id}.jsonl"
    counts: dict[str, int] = {}
    if not p.is_file():
        return counts
    for line in p.read_text(errors="replace").splitlines():
        try:
            node = json.loads(line).get("node_id", "?")
        except Exception:  # noqa: BLE001
            continue
        counts[node] = counts.get(node, 0) + 1
    return counts


def gw_get(path: str) -> dict | list | None:
    url, tok = gateway()
    try:
        out = subprocess.run(
            ["curl", "-s", "-m", "30", "-H", f"Authorization: Bearer {tok}",
             f"{url}/api/gateway{path}"],
            capture_output=True, text=True, timeout=60,
        ).stdout
        return json.loads(out) if out.strip() else None
    except Exception:  # noqa: BLE001
        return None


@dataclass
class Run:
    arm: str
    rep: int
    out_dir: str
    log_path: str
    started_at: str
    elapsed_s: float
    exit_code: int
    run_id: str = ""
    session_id: str = ""
    final_answer: str = ""
    llm_calls: int = 0
    tool_calls: int = 0
    tokens_in: int = 0
    tokens_out: int = 0
    file_count: int = 0
    total_bytes: int = 0
    # Provenance / verification
    wire_review_mode: object = None
    wire_review_rounds: object = None
    wire_thinking: object = None
    wire_max_iterations: object = None
    run_outcome: str = ""
    run_iterations: object = None
    review_events: int = 0
    node_counts: dict = field(default_factory=dict)
    store_seen: bool = False
    wire_model: object = None
    wire_provider: object = None
    review_count: object = None
    review_skipped: bool = False
    iterations_used: object = None
    tokens_total: int = 0
    loadavg: float = 0.0
    archived_product: str = ""
    infra_failure: bool = False
    stray_writes: list = field(default_factory=list)
    verdict: str = "PENDING"
    discard_reason: str = ""


def ws_snapshot_excluding(out_dir: Path) -> set[str]:
    """Files under WS_ROOT that are NOT the run's own product.

    Both sides must be expressed in the SAME relative base. A first version
    subtracted `snapshot(out_dir)` (paths relative to out_dir, e.g. "game.js")
    from `snapshot(WS_ROOT)` (paths relative to WS_ROOT, e.g.
    "review-1/product/game.js"); nothing cancelled, so the run's own output was
    reported as a stray write and every run was falsely DISCARDED.
    """
    if not WS_ROOT.is_dir():
        return set()
    try:
        prefix = out_dir.relative_to(WS_ROOT).as_posix() + "/"
    except ValueError:
        prefix = None
    out: set[str] = set()
    for q in WS_ROOT.rglob("*"):
        if not q.is_file():
            continue
        rel = q.relative_to(WS_ROOT).as_posix()
        if prefix and rel.startswith(prefix):
            continue
        out.add(rel)
    return out


def repo_dirty() -> set[str]:
    """Tracked+untracked changes in the repo, via git — the only reliable way to
    notice an agent writing into the source tree it was launched from."""
    # Paths the OPERATOR works in while a matrix runs. Excluded because a
    # human editing the harness mid-run is not the agent escaping its workspace:
    # creating `scripts/bench_matrix_page.py` during run 1 was attributed to the
    # agent and DISCARDED a perfectly good run, voiding the matrix. The check
    # still covers `src/`, `tests/`, and everything else the agent could reach.
    OPERATOR_AREAS = ("scripts/", "docs/", "untracked/bench-site/", ".git/")
    try:
        out = subprocess.run(["git", "status", "--porcelain"], cwd=str(REPO),
                             capture_output=True, text=True, timeout=60).stdout
        paths = {ln[3:].strip() for ln in out.splitlines() if ln.strip()}
        return {q for q in paths
                if not q.startswith(OPERATOR_AREAS) and not q.endswith(".log")}
    except Exception:  # noqa: BLE001
        return set()


def snapshot(root: Path) -> set[str]:
    if not root.is_dir():
        return set()
    return {str(p.relative_to(root)) for p in root.rglob("*") if p.is_file()}


def count_tree(root: Path) -> tuple[int, int]:
    n = b = 0
    for p in root.rglob("*"):
        if p.is_file():
            n += 1
            try:
                b += p.stat().st_size
            except OSError:
                pass
    return n, b


def extract_answer(text: str) -> str:
    """The final answer as `exec` actually prints it: a block after a
    `━━━ answer ━━━` rule. The old harness reverse-scanned for `✦`, which marks
    INTERIM assistant text — it captured the wrong thing or nothing."""
    m = re.split(r"━+\s*answer\s*━+", text)
    if len(m) < 2:
        return ""
    tail = m[-1]
    # Stop at the trailing stats line ("· ✓ done · …" / "done · N llm calls").
    tail = re.split(r"\n\s*[·⚠]\s|\n(?:done|stopped)\s·\s", tail)[0]
    return tail.strip()


def parse_stats(text: str) -> dict:
    """`exec`'s own conclusion line is the honest source; `--json` stats are
    empty on this path (existing summary files are literally 2 bytes)."""
    out: dict = {}
    m = re.search(r"(?:done(?::\s*\w+)?|stopped[^·\n]*)\s·\s(\d+)\s+llm calls\s·\s(\d+)\s+tools", text)
    if m:
        out["llm_calls"], out["tool_calls"] = int(m.group(1)), int(m.group(2))
    m = re.search(r"(\d+)\s*↑\s*(\d+)\s*↓\s*tk", text) or re.search(r"(\d+)()\s*tk total", text)
    if m:
        out["tokens_in"], out["tokens_out"] = int(m.group(1)), int(m.group(2))
    m = re.search(r"run\s+([0-9a-f-]{8,})", text)
    if m:
        out["run_id"] = m.group(1)
    return out


def cancel_run(run_id: str) -> bool:
    """Cancel a durable gateway run. Killing the client does NOT stop it —
    `exec` says so itself ("the run stays durable on the gateway"), so an abort
    without this leaks paid gpt-5.4 runs. Route verified live: the /runs/{id}/cancel
    shapes are 404; the real door is POST /commands with type "cancel"."""
    if not run_id:
        return False
    url, tok = gateway()
    body = json.dumps({"command_id": f"ab-cancel-{run_id[:8]}-{int(time.time())}",
                       "run_id": run_id, "type": "cancel", "payload": {},
                       "client_id": "zelda-review-ab"})
    try:
        out = subprocess.run(
            ["curl", "-s", "-m", "20", "-X", "POST",
             "-H", f"Authorization: Bearer {tok}", "-H", "Content-Type: application/json",
             "-d", body, f"{url}/api/gateway/commands"],
            capture_output=True, text=True, timeout=40).stdout
        return '"accepted":true' in out.replace(" ", "")
    except Exception:  # noqa: BLE001
        return False


# Run id of the in-flight run, so a signal handler can cancel it.
_INFLIGHT: dict = {"run_id": "", "log": None}


def _abort(signum, _frame):
    rid = _INFLIGHT.get("run_id") or ""
    if not rid and _INFLIGHT.get("log"):
        try:
            rid = (parse_stats(Path(_INFLIGHT["log"]).read_text(errors="replace"))
                   .get("run_id", ""))
        except Exception:  # noqa: BLE001
            rid = ""
    if rid:
        ok = cancel_run(rid)
        print(f"\n⚠ aborted — cancel {rid[:8]}: {'accepted' if ok else 'FAILED (cancel it manually)'}",
              flush=True)
    else:
        print("\n⚠ aborted before a run id was known; check for active runs", flush=True)
    raise SystemExit(130)


def model_available() -> bool:
    """Is the pinned model actually being served right now?"""
    try:
        out = subprocess.run(["curl", "-s", "-m", "15", f"{BASE_URL}/models"],
                             capture_output=True, text=True, timeout=30).stdout
        ids = {m.get("id") for m in (json.loads(out).get("data") or [])}
        return MODEL in ids
    except Exception:  # noqa: BLE001
        return False


def one_run(arm: str, rep: int, framework_root: Path) -> Run:
    # Product dir lives OUTSIDE the framework tree (B1): nothing in its
    # ancestry is a prior game, so no `cd ..` reaches one.
    # Each run gets its OWN parent. Under a shared parent, run 2 could reach
    # run 1's finished game with `cd ../review-1` — cross-ARM contamination
    # inside the matrix itself. The isolation assertion below caught exactly
    # that on the second smoke run.
    out_dir = WS_ROOT / f"{arm}-{rep}" / "product"
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True)
    assert not any(out_dir.rglob("*")), f"{out_dir} must start empty"
    # Prove the isolation claim rather than asserting it: no game artifact may
    # exist anywhere above the workspace.
    for anc in (out_dir.parent, WS_ROOT):
        strays = sorted(anc.rglob("game.js")) + sorted(anc.rglob("index.html"))
        strays = [q for q in strays if not q.is_relative_to(out_dir)]
        assert not strays, (
            f"a game artifact is reachable from {anc}: {strays[:2]} — the agent has "
            f"execute_command and can cd there, so this run would be contaminated")

    log_path = OUT / f"{arm}-{rep}.log"
    url, tok = gateway()
    task = SMOKE_PROMPT if SMOKE else ZELDA_PROMPT
    prompt = (
        f"{task}\n\nWrite ALL game files under this exact directory "
        f"(create it if needed):\n{out_dir.resolve()}\nDo not write anywhere else."
    )
    cmd = [
        str(BIN), "exec", prompt,
        "--workflow", WORKFLOW,
        "--provider", PROVIDER, "--model", MODEL,
        "--gateway", url, "--token", tok,
        "--permissions", "all", "--ungated",
        "--max-iterations", str(MAX_ITER),
        "--reasoning", REASONING,
        "--timeout", str(WALL_BUDGET_S),
        "--workspace", str(out_dir),
        "--workspace-mode", "workspace_only",
        "--no-project-context",
    ]
    cmd += ["--review", "--review-rounds", str(REVIEW_ROUNDS)] if arm == "review" else ["--no-review"]

    # Strip client env AND every *_API_KEY: this benchmark runs on the
    # operator's subscription-backed relay, and a stray key in the environment is
    # the one way it could silently bill an API instead.
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("ABSTRACTCODE_", "ABSTRACTGATEWAY_", "ABSTRACTMEMORY_"))
           and not k.endswith(("_API_KEY", "_APIKEY"))}

    # B2: the old detector watched framework_root/untracked while products were
    # written under abstractcode/untracked — blind to every realistic stray.
    # Watch the repo's own tree (via git) and the workspace parent.
    before_repo = repo_dirty()
    before_ws = ws_snapshot_excluding(out_dir)
    started = datetime.now(timezone.utc).isoformat()
    load0 = os.getloadavg()[0]
    _INFLIGHT["run_id"] = ""
    _INFLIGHT["log"] = str(log_path)
    t0 = time.monotonic()
    try:
        with log_path.open("w") as fh:
            rc = subprocess.run(cmd, cwd=str(REPO), env=env, stdout=fh,
                                stderr=subprocess.STDOUT,
                                timeout=WALL_BUDGET_S + 120).returncode
    except subprocess.TimeoutExpired:
        rc = 124
    elapsed = round(time.monotonic() - t0, 1)

    text = log_path.read_text(errors="replace") if log_path.is_file() else ""
    stats = parse_stats(text)
    fc, tb = count_tree(out_dir)
    after_repo = repo_dirty()
    after_ws = ws_snapshot_excluding(out_dir)
    stray = sorted((after_repo - before_repo) | (after_ws - before_ws))[:20]

    r = Run(
        arm=arm, rep=rep, out_dir=str(out_dir), log_path=str(log_path),
        started_at=started, elapsed_s=elapsed, exit_code=rc,
        run_id=stats.get("run_id", ""), final_answer=extract_answer(text)[:4000],
        llm_calls=stats.get("llm_calls", 0), tool_calls=stats.get("tool_calls", 0),
        tokens_in=stats.get("tokens_in", 0), tokens_out=stats.get("tokens_out", 0),
        file_count=fc, total_bytes=tb, stray_writes=stray,
    )
    r.loadavg = round(load0, 2)

    # ---- provenance: read the runtime's durable store, not the filtered API
    if r.run_id:
        v = run_vars(r.run_id)
        rt = v.get("_runtime") or {}
        lim = v.get("_limits") or {}
        r.wire_review_mode = rt.get("review_mode")
        r.wire_review_rounds = rt.get("review_max_rounds")
        r.wire_thinking = rt.get("thinking")
        r.wire_model = rt.get("model")
        r.wire_provider = rt.get("provider")
        r.wire_max_iterations = lim.get("max_iterations", v.get("max_iterations"))
        r.run_iterations = lim.get("current_iteration")
        sp = v.get("scratchpad") or {}
        # `scratchpad.outcome` does not exist on react runs (adversary: dead
        # code) — the budget signal is the iteration counter and exec's rc 125.
        r.run_outcome = str(sp.get("outcome") or "")
        r.review_count = sp.get("review_count")
        # A verifier whose response was unparseable falls straight through to
        # `done`. The ledger still shows a `review` record, so the manipulation
        # check passes while the treatment was a NO-OP — that dilutes a real
        # effect into "no detectable difference".
        r.review_skipped = bool(sp.get("review_skipped"))
        r.iterations_used = (v.get("_limits") or {}).get("current_iteration")
        u = v.get("_usage") or sp.get("usage") or {}
        if isinstance(u, dict):
            r.tokens_in = int(u.get("input_tokens") or r.tokens_in or 0)
            r.tokens_out = int(u.get("output_tokens") or r.tokens_out or 0)
            r.tokens_total = int(u.get("total_tokens") or 0)
        r.node_counts = ledger_node_counts(r.run_id)
        # The verifier node executing is the manipulation check.
        r.review_events = int(r.node_counts.get("review", 0))
        r.store_seen = bool(v)

    # ---- verdict: DISCARD beats score. A run we cannot trust is not data.
    expect_review = arm == "review"
    text_l = text.lower()
    if ("at their limits" in text_l or "502" in text_l
            or "upstream connection failed" in text_l
            or "no such model" in text_l or "model not found" in text_l
            or (rc != 0 and r.llm_calls == 0 and "available models" in text_l)):
        r.infra_failure = True
        r.verdict, r.discard_reason = ("INFRA", "relay/upstream unavailable (not an agent failure)")
    elif not r.store_seen:
        # Cannot verify what actually ran -> cannot use the run. Silence here is
        # how a broken check turns into a fabricated result.
        r.verdict, r.discard_reason = (
            "DISCARD", f"runtime store unreadable for run {r.run_id or '<no id>'} "
                       f"(set AB_RUNTIME_STORE)")
    elif r.wire_model is None or r.wire_thinking is None or r.wire_provider is None:
        # Absent is NOT agreement. The original bug in this harness was exactly
        # a null read passing silently; the same shape here would be a false
        # ACCEPT of an unverified route.
        r.verdict, r.discard_reason = (
            "DISCARD", f"route unverifiable: model={r.wire_model!r} "
                       f"thinking={r.wire_thinking!r} provider={r.wire_provider!r}")
    elif str(r.wire_provider) != PROVIDER:
        r.verdict, r.discard_reason = (
            "DISCARD", f"provider drift: ran {r.wire_provider!r}, expected {PROVIDER!r} "
                       f"(a keyed provider serving the same model name would bill an API)")
    elif expect_review and r.wire_review_mode is None:
        r.verdict, r.discard_reason = ("DISCARD", "review arm but review_mode absent on the wire")
    elif expect_review and r.review_skipped:
        r.verdict, r.discard_reason = (
            "DISCARD", "verifier ran but its response was unparseable (review_skipped) "
                       "— the treatment was a no-op")
    elif expect_review and r.review_count in (0, None):
        r.verdict, r.discard_reason = (
            "DISCARD", f"review arm but scratchpad.review_count={r.review_count!r} "
                       f"(independent cross-check of the ledger count disagrees)")
    elif str(r.wire_model) != MODEL:
        # Route drift is silent and ruins comparability: a matrix half-run on a
        # different model is not a benchmark. Verified against the runtime's
        # own record, never against what this harness intended to send.
        r.verdict, r.discard_reason = (
            "DISCARD", f"model drift: ran {r.wire_model!r}, expected {MODEL!r}")
    elif str(r.wire_thinking) != REASONING:
        r.verdict, r.discard_reason = (
            "DISCARD", f"reasoning drift: ran {r.wire_thinking!r}, expected {REASONING!r}")
    elif rc == 124 or elapsed >= WALL_BUDGET_S:
        r.verdict, r.discard_reason = "DISCARD", f"hit the {WALL_BUDGET_S}s wall budget"
    elif rc == 125 or r.run_outcome == "iteration_budget":
        r.verdict, r.discard_reason = "DISCARD", "iteration budget exhausted (truncated, not finished)"
    elif r.wire_review_mode is not None and bool(r.wire_review_mode) != expect_review:
        r.verdict, r.discard_reason = (
            "DISCARD", f"wire review_mode={r.wire_review_mode!r}, arm expects {expect_review}")
    elif expect_review and r.review_events == 0:
        r.verdict, r.discard_reason = (
            "DISCARD", "review arm but the ledger shows no verifier activity")
    elif not expect_review and r.review_events > 0:
        r.verdict, r.discard_reason = (
            "DISCARD", f"no-review arm but ledger shows {r.review_events} review events")
    elif stray:
        r.verdict, r.discard_reason = "DISCARD", f"wrote outside the out dir: {stray[:3]}"
    elif fc == 0:
        r.verdict, r.discard_reason = "DISCARD", "no files produced"
    elif rc != 0:
        r.verdict, r.discard_reason = "DISCARD", f"exit {rc}"
    else:
        r.verdict = "VALID"

    # Archive the product under untracked/ so a result stays auditable after
    # /private/tmp is reaped. Copied only AFTER the agent has exited, so the run
    # can never read it. Must stay at the very END: an earlier version of this
    # block sat above the verification and returned early, so every run came back
    # PENDING with no provenance — caught by the smoke, which is why it exists.
    try:
        keep = OUT / f"{arm}-{rep}-product"
        if keep.exists():
            shutil.rmtree(keep)
        shutil.copytree(out_dir, keep)
        r.archived_product = str(keep)
        # Point scoring at the durable copy, then REMOVE the build tree. The
        # workspace must be empty again before the next run: otherwise run N+1
        # can reach run N's finished game with `cd ..`, which is cross-arm
        # contamination inside the matrix.
        r.out_dir = str(keep)
        shutil.rmtree(out_dir.parent, ignore_errors=True)
    except Exception as exc:  # noqa: BLE001
        r.archived_product = f"archive failed: {exc}"
    return r


def provenance() -> dict:
    def sh(*a: str) -> str:
        try:
            return subprocess.run(a, cwd=str(REPO), capture_output=True,
                                  text=True, timeout=30).stdout.strip()
        except Exception:  # noqa: BLE001
            return ""
    sha = hashlib.sha256(BIN.read_bytes()).hexdigest() if BIN.is_file() else ""
    scorer = REPO / "scripts" / "zelda_review_score.py"
    # Freeze the SCORER identity too: a rubric edited after seeing results is a
    # post-hoc story, so its hash is part of the pre-registration.
    scorer_sha = hashlib.sha256(scorer.read_bytes()).hexdigest() if scorer.is_file() else ""
    return {
        "mode": "SMOKE (pipeline validation, NOT a result)" if SMOKE else "MATRIX",
        "generated": datetime.now(timezone.utc).isoformat(),
        "binary_sha256": sha,
        "scorer_sha256_at_prereg": scorer_sha,
        "binary_version": sh(str(BIN), "--version"),
        "git_head": sh("git", "rev-parse", "HEAD"),
        "git_dirty_stat": sh("git", "diff", "--stat"),
        "workflow": WORKFLOW, "provider": PROVIDER, "model": MODEL,
        "reasoning": REASONING, "max_iterations": MAX_ITER,
        "review_rounds": REVIEW_ROUNDS, "repeats": REPEATS,
        "wall_budget_s": WALL_BUDGET_S, "arm_order": ARM_ORDER,
        "prereg": {
            "n_per_arm": REPEATS,
            "best_possible_exact_p": "1/C(6,3)=0.05 at n=3; UNREACHABLE (0.167) at n=2",
            "SIGNAL": "min(review SCOREs) > max(noreview SCOREs) — strict separation "
                      "in the hypothesised direction, one definition, evaluated by "
                      "zelda_review_score.py and nothing else",
            "NOISE": "any overlap between arms, or within-arm range wider than the "
                     "between-arm gap -> report 'no detectable effect at n=3', NOT "
                     "'review does not work'",
            "VOID": "ANY discard. One discard forces n=2 in an arm, which caps the "
                    "best possible exact p at 0.167 — an underpowered matrix is void, "
                    "not merely weakened.",
            "non_scoring": ["file_count", "total_bytes"],
        },
    }


def main() -> int:
    if not BIN.is_file():
        print(f"missing binary: {BIN} (cargo build --release)", file=sys.stderr)
        return 2
    signal.signal(signal.SIGINT, _abort)
    signal.signal(signal.SIGTERM, _abort)
    # Route is a constant, not a variable this harness can talk itself into.
    if (MODEL, PROVIDER, REASONING) != REQUIRED_ROUTE and not ROUTE_OVERRIDE:
        print(f"✗ route must be {REQUIRED_ROUTE}, got {(MODEL, PROVIDER, REASONING)}\n"
              f"  (pass --allow-route-override only if you intend a different benchmark)",
              file=sys.stderr)
        return 2
    # Pre-flight: the relay DROPS every gpt-* model when its OpenAI accounts hit
    # their limits, leaving only claude:*. A matrix started in that window dies
    # instantly, run after run. Check before spending, and again between runs.
    if not model_available():
        print(f"✗ {MODEL} is not currently advertised by the relay "
              f"({BASE_URL}) — the subscription's accounts are likely at their "
              f"limits. Wait for the quota window to reopen and re-run.",
              file=sys.stderr)
        return 2
    framework_root = REPO.parent

    prov = provenance()

    if "--dry-run" in sys.argv:
        print(json.dumps(prov, indent=2))
        print("\nplan:")
        for i, arm in enumerate(ARM_ORDER, 1):
            print(f"  {i}. {arm}  -> {OUT}/{arm}-{ARM_ORDER[:i].count(arm)}/")
        return 0

    # NO RESUME, structurally. This harness only ever starts a 100% independent
    # matrix: a pre-existing output root means someone is about to mix a new
    # bench with an old one, and partial/contaminated priors are exactly why
    # this experiment had to be restarted. Refuse instead of silently blending;
    # archiving the old root is a deliberate operator act, not a side effect.
    if OUT.exists() and any(OUT.iterdir()):
        print(
            f"✗ refusing to start: {OUT} already exists and is not empty.\n"
            f"  This harness never resumes or merges a prior bench. Archive it first:\n"
            f"    mv {OUT} {OUT.parent}/_archive-$(date -u +%Y%m%dT%H%M%SZ)/\n"
            f"  then re-run for a clean, independent matrix.",
            file=sys.stderr,
        )
        return 2
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "prereg.json").write_text(json.dumps(prov, indent=2))
    print(json.dumps(prov, indent=2), flush=True)

    runs: list[Run] = []
    seen: dict[str, int] = {}
    for arm in ARM_ORDER:
        seen[arm] = seen.get(arm, 0) + 1
        rep = seen[arm]
        print(f"\n=== {arm} rep {rep} ===", flush=True)
        # An infra failure (model withdrawn, 502, upstream quota) says nothing
        # about the agent, so it must not consume the discard budget. Retry the
        # same cell after the quota window instead of voiding the matrix — the
        # pre-registration's intent was "replace the run, never reinterpret".
        for attempt in range(1, 4):
            r = one_run(arm, rep, framework_root)
            if not r.infra_failure:
                break
            print(f"    infra failure ({r.discard_reason}) — attempt {attempt}/3; "
                  f"waiting for the relay to serve {MODEL} again", flush=True)
            for _ in range(40):                       # up to ~10 min
                time.sleep(15)
                if model_available():
                    break
        runs.append(r)
        print(f"    {r.verdict:8} exit={r.exit_code} {r.elapsed_s}s "
              f"files={r.file_count} bytes={r.total_bytes} llm={r.llm_calls} "
              f"tools={r.tool_calls} review_events={r.review_events} "
              f"outcome={r.run_outcome or '-'} {r.discard_reason}", flush=True)
        (OUT / "runs.json").write_text(json.dumps(
            {"provenance": prov, "runs": [asdict(x) for x in runs]}, indent=2))

    valid = [r for r in runs if r.verdict == "VALID"]
    discarded = [r for r in runs if r.verdict != "VALID"]
    print(f"\n{len(valid)}/{len(runs)} VALID")
    for r in discarded:
        print(f"  DISCARD {r.arm}-{r.rep}: {r.discard_reason}")
    if discarded:
        print("\n⚠ MATRIX VOID by pre-registration (any discard voids it). "
              "Fix the cause and re-run; do not interpret these runs.")
    print(f"\nArtifacts: {OUT}\nScore with: python3 scripts/zelda_review_score.py")
    return 0 if not discarded else 1


if __name__ == "__main__":
    raise SystemExit(main())
