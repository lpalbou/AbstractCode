#!/usr/bin/env python3
"""Cross-client Zelda benchmark: abstractcode, abstractcode-tui, opencode, pi.

Answers the operator's ORIGINAL question — "is abstractcode a better coder than
abstractcode-tui?" — which the review_mode A/B could not, because that varied
one flag inside one client.

Route, identical for every client: gpt-5.4 on the local airelays relay
(http://127.0.0.1:8317/v1), subscription-backed, NO API key. `opencode` and `pi`
carry a literal `"bench"` placeholder in their provider config because their
schema demands the field; the relay ignores it. Verified: no *_API_KEY is
exported into any child.

KNOWN ASYMMETRY, recorded rather than hidden — reasoning effort:
  abstractcode-tui  medium, applied via the gateway's `_runtime.thinking`  (CONFIRMED)
  opencode          `--variant medium`                                     (declared)
  pi                `--model gpt-5.4:medium`                               (declared)
  abstractcode      REQUESTED but NOT APPLIED. abstractcore's
                    openai_compatible provider has no thinking-control
                    mapping for this model and says so at runtime:
                      "thinking='medium' requested but provider
                       'openai-compatible' does not implement a thinking
                       control mapping for model 'gpt-5.4'; no control was
                       applied"
                    so that arm runs at the relay default (`none`).
This is a real confound on the abstractcode arm and is reported with the result,
not buried. It is also a cross-package defect worth fixing in abstractcore.

Isolation, per run: a fresh workspace OUTSIDE the framework tree, asserted to
contain no prior game anywhere in its ancestry (agents have shell access and
`cd ..` reaches archived Zelda games otherwise). The product is copied back for
scoring only AFTER the client exits, then the build tree is deleted so run N+1
cannot read run N.

Usage:
  python3 scripts/bench_clients.py                 # all clients, 3 repeats
  python3 scripts/bench_clients.py --clients pi,opencode
  python3 scripts/bench_clients.py --repeats 1 --smoke
  python3 scripts/bench_clients.py --dry-run
"""
from __future__ import annotations

import argparse
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
OUT = REPO / "untracked" / os.environ.get("CB_OUT", "client-bench")
TUI_BIN = REPO / "target" / "release" / "abstractcode-tui"
RUNTIME_STORE = Path(os.environ.get(
    "CB_RUNTIME_STORE", "/Users/albou/tmp/abstractframework/runtime"))
# Arms that execute through the gateway and therefore leave a durable
# run_<id>.json whose `_runtime` block is the ONLY trustworthy route evidence.
GATEWAY_ARMS = {"tui-basic", "tui-coder", "tui-multi",
                "abstractcode-basic", "abstractcode-coder", "abstractcode-tui"}

MODEL = os.environ.get("CB_MODEL", "gpt-5.4")
REASONING = os.environ.get("CB_REASONING", "medium")
BASE_URL = os.environ.get("CB_BASE_URL", "http://127.0.0.1:8317/v1")
REPEATS = int(os.environ.get("CB_REPEATS", "3"))
# Operator ruling 2026-08-01: per-run isolation makes 3-5 concurrent safe.
PARALLEL = max(1, int(os.environ.get("CB_PARALLEL", "3")))
REQUIRED_ROUTE = ("gpt-5.4", "medium", "http://127.0.0.1:8317/v1")

# #[WARNING:TIMEOUT] Uncapped by default (ADR-0014 §2 / ADR-0027 §2-§3): a
# complex agentic build legitimately runs for an hour, and truncating it scores
# an interrupted run as a bad coder. The finite value is a hang-catcher only and
# is reported as an overrun, never as a quality verdict.
WALL_S = int(os.environ.get("CB_WALL_S", "0"))
REAP_S = int(os.environ.get("CB_REAP_S", "10800"))

# Operator's R-Type prompt (2026-08-02). IDENTICAL for every arm; only the
# delivery instruction is workspace-relative so no arm gets a path advantage.
# OPERATOR'S CORRECTED PROMPT (2026-08-04). The previous text was NOT the
# operator's spec — it lacked the R-Type signatures (orbs, item drops, weapon
# progression), the arcade mode, the per-level aesthetics and the win sequence,
# and it carried a cheat-code item the real spec does not have. Two typo-level
# normalizations from the operator's message, flagged rather than silent:
# "boos" -> "boss", missing space after "accomplished." — nothing else changed.
RTYPE_PROMPT = (
    "create a fully playable r-type game in black & white, gameboy style (scaled to 600px). "
    "integrate game mechanics, orbs, monsters dropping powered items and boss at the end of each "
    "level. The game must have multiple weapons (gained with dropped items) and procedurally "
    "generate VFX and SFX effects as well as music. The game must be playable with the arrows "
    "too. The game must be composed of a campaign, with multiple levels, each level with a short "
    "message intro explaining the mission. each level must take about 2mn play and finish with a "
    "boss. Defeating a boss reveals part of the storyline and reward with a new unique item with "
    "dedicated game mechanics. each level is harder than the previous. each level has a different "
    "level design and aesthetics. At the end of the campaign mode, we must have some graphics, "
    "animations to show the game is won and the mission accomplished. There must also be an "
    "arcade mode without storyline but with boss encounters every 2mn. The game difficulty must "
    "continue to increase after each boss kill. Take extra care of the graphics, VFX, SFX and "
    "create a sound track suitable for this game and a gameboy."
)

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
SMOKE_PROMPT = (
    "Create a minimal but genuinely playable browser game in this folder: index.html plus "
    "game.js. A <canvas>, a player square moved with the arrow keys via "
    "addEventListener('keydown'), a requestAnimationFrame loop redrawing every frame, one wall "
    "the player cannot cross, and one beep via AudioContext. Under 150 lines, no build step."
)


@dataclass
class Run:
    arm: str                 # client id — the scorer keys on this
    rep: int
    out_dir: str = ""
    started_at: str = ""
    elapsed_s: float = 0.0
    exit_code: int = -1
    verdict: str = "PENDING"
    discard_reason: str = ""
    file_count: int = 0
    total_bytes: int = 0
    log_path: str = ""
    archived_product: str = ""
    client_version: str = ""
    agent: str = ""
    model_requested: str = MODEL
    reasoning_requested: str = REASONING
    reasoning_applied: str = "unknown"
    stray_writes: list = field(default_factory=list)
    loadavg: float = 0.0
    notes: str = ""
    infra_failure: bool = False
    # Route verification (gateway arms only; external clients have no run store)
    run_id: str = ""
    # The workspace the client actually saw. out_dir is rewritten to the
    # archive path once the product is copied out, and the build tree is then
    # deleted — without this, correlating a run against codex/opencode/pi's own
    # session stores (all keyed by cwd) means guessing the path back.
    workspace_used: str = ""
    llm_calls: int = 0
    tool_calls: int = 0
    effort_reported: bool = False
    wire_model: object = None
    wire_thinking: object = None
    iterations_used: object = None
    route_verified: str = "unverifiable"
    concurrent_peers: list = field(default_factory=list)


def client_version(cmd: list[str]) -> str:
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        return (p.stdout or p.stderr or "").strip().splitlines()[0][:60]
    except Exception:  # noqa: BLE001
        return "?"


# Each entry: how to invoke, what agent it runs, and whether the reasoning
# request demonstrably reaches the provider.
def build_cmd(client: str, prompt: str, ws: Path) -> tuple[list[str], str, str]:
    if client in ("tui-basic", "tui-coder", "tui-multi"):
        gw = json.loads((Path.home() / ".abstractcode/gateway.json").read_text())
        wf = {"tui-basic": "basic-agent",
              "tui-coder": "coding-agent:coder",
              "tui-multi": "multiagent-coding:multiagent-coder"}[client]
        return ([
            str(TUI_BIN), "exec", prompt,
            "--workflow", wf,
            "--provider", "endpoint:airelay", "--model", MODEL,
            "--gateway", gw.get("base_url", "http://127.0.0.1:8080"),
            "--token", gw.get("token", ""),
            "--reasoning", REASONING, "--permissions", "all", "--ungated",
            "--max-iterations", "120", "--timeout", "0",
            "--workspace", str(ws), "--workspace-mode", "workspace_only",
            "--no-project-context",
        ], f"{wf} (gateway)", "applied via _runtime.thinking")
    if client == "abstractcode-basic":
        return ([
            "abstractcode", "exec", prompt,
            "--agent", "basic-agent",
            "--provider", "endpoint:airelay", "--model", MODEL,
            "--base-url", BASE_URL, "--reasoning", REASONING,
            "--permission-mode", "full-auto", "--on-gated", "deny",
            "--max-iterations", "120", "--timeout", str(WALL_S or 10800),
        ], "basic-agent (via abstractcode)", "NOT APPLIED locally")
    if client == "codex":
        # Operator subscription (auth_mode=chatgpt); NO api key — verified:
        # codex runs with OPENAI_API_KEY unset.
        #
        # ROUTE ASYMMETRY, corrected 2026-08-02. The previous comment claimed
        # the model was "pinned to the same route via the bench8317 provider
        # profile". There is no bench8317 profile in ~/.codex/config.toml (only
        # ollama-launch), and this command never referenced one, so codex has
        # always run on `provider: openai` — its own ChatGPT-subscription
        # backend — while every other arm goes through the relay on 8317.
        # Confirmed from codex's own banner and its rollout record
        # (payload.model_provider = "openai"). Model and reasoning effort DO
        # match (gpt-5.4 / medium, the config default). Left on the native
        # subscription rather than force-routed: the operator's standing rule
        # for this arm is "must use the ChatGPT subscription, no API key", and
        # an ad-hoc -c provider override would need a key field. The asymmetry
        # is reported with the result rather than hidden.
        return (["codex", "exec", "--full-auto", "--skip-git-repo-check",
                 "-C", str(ws), "-m", MODEL, prompt],
                "codex default", "medium (codex config default; NOT via the relay)")
    if client == "abstractcode":
        return ([
            "abstractcode", "exec", prompt,
            "--agent", "react",
            "--provider", "endpoint:airelay", "--model", MODEL,
            "--base-url", BASE_URL, "--reasoning", REASONING,
            "--permission-mode", "full-auto", "--on-gated", "deny",
            # ADR-0027 + bench fairness: `WALL_S or 10800` turned the explicit
            # CB_WALL_S=0 ("no wall clock") into a hidden 3h kill for these two
            # arms while the tui arm below correctly got 0 — the comparison was
            # biased by a cap nobody asked for. Pass the operator's value through.
            "--max-iterations", "120", "--timeout", str(WALL_S),
        ], "react (local loop)", "NOT APPLIED (abstractcore has no thinking map for this model)")
    if client == "abstractcode-coder":
        # ISOLATION ARM (operator ask 2026-08-01): the OLD Python client running
        # a gateway coding workflow, to separate "the workflow improved" from
        # "the client improved".
        #
        # CAVEAT, measured 2026-08-02: this is NOT the same flow as tui-coder.
        # `--agent coder` resolves to bundle flow `coding-agent:coding-agent`,
        # while tui-coder runs `coding-agent:coder` — different flows in the
        # same bundle. The arm therefore does NOT isolate client-vs-workflow as
        # its original comment claimed; it compares two different flows run by
        # two different clients. Reported as such rather than as a clean control.
        return ([
            "abstractcode", "exec", prompt,
            "--agent", "coder",
            "--provider", "endpoint:airelay", "--model", MODEL,
            "--base-url", BASE_URL, "--reasoning", REASONING,
            "--permission-mode", "full-auto", "--on-gated", "deny",
            # ADR-0027 + bench fairness: `WALL_S or 10800` turned the explicit
            # CB_WALL_S=0 ("no wall clock") into a hidden 3h kill for these two
            # arms while the tui arm below correctly got 0 — the comparison was
            # biased by a cap nobody asked for. Pass the operator's value through.
            "--max-iterations", "120", "--timeout", str(WALL_S),
        ], "coding-agent:coder (gateway, via abstractcode)",
            "NOT APPLIED locally; the gateway lane applies it")
    if client == "abstractcode-tui":
        gw = json.loads((Path.home() / ".abstractcode/gateway.json").read_text())
        return ([
            str(TUI_BIN), "exec", prompt,
            "--workflow", "react-agent:react",
            "--provider", "endpoint:airelay", "--model", MODEL,
            "--gateway", gw.get("base_url", "http://127.0.0.1:8080"),
            "--token", gw.get("token", ""),
            "--reasoning", REASONING, "--permissions", "all", "--ungated",
            "--max-iterations", "120", "--timeout", str(WALL_S),
            "--workspace", str(ws), "--workspace-mode", "workspace_only",
            "--no-project-context",
        ], "react-agent:react (gateway)", "applied via _runtime.thinking")
    if client == "opencode":
        return (["opencode", "run", "--dir", str(ws), "--model", f"bench8317/{MODEL}",
                 "--variant", REASONING, prompt],
                "opencode default", f"--variant {REASONING}")
    if client == "pi":
        return (["pi", "-p", "--provider", "bench8317",
                 "--model", f"{MODEL}:{REASONING}", prompt],
                "pi default", f"model suffix :{REASONING}")
    raise SystemExit(f"unknown client {client}")


def model_available() -> bool:
    try:
        out = subprocess.run(["curl", "-s", "-m", "15", f"{BASE_URL}/models"],
                             capture_output=True, text=True, timeout=30).stdout
        return MODEL in {m.get("id") for m in (json.loads(out).get("data") or [])}
    except Exception:  # noqa: BLE001
        return False


def repo_dirty() -> set[str]:
    # tests/ added 2026-08-04: an operator-side agent created a scorer fixture
    # (tests/rtype_fixtures/probe-backward-fire/) 14 s before a codex cell's
    # window closed, and this detector attributed it to the cell and DISCARDED
    # a healthy run — the third false-discard of this class (scripts/ and
    # docs/ earned their exclusions the same way on 07-30). The detector's job
    # is catching AGENTS escaping their workspace; every path here is an area
    # only the operator's own tooling writes.
    OPERATOR_AREAS = ("scripts/", "docs/", "tests/", "untracked/", ".git/")
    try:
        out = subprocess.run(["git", "status", "--porcelain"], cwd=str(REPO),
                             capture_output=True, text=True, timeout=60).stdout
        return {ln[3:].strip() for ln in out.splitlines() if ln.strip()
                and not ln[3:].strip().startswith(OPERATOR_AREAS)}
    except Exception:  # noqa: BLE001
        return set()


def count_tree(root: Path) -> tuple[int, int]:
    # `.git` is HARNESS state, not product: this bench git-init's every
    # workspace as a project-root-walk stopper, which plants ~15 sample hooks
    # for free, and a client that commits its work (codex does) buries hundreds
    # of loose objects on top. Counting them made file_count/total_bytes a
    # measure of git usage rather than of what was built, and inflated exactly
    # the arms that use version control. Excluded, matching bench_workflows.py.
    # The harness writes `.gitignore` itself as part of the root-walk stopper,
    # so it is HARNESS output, not product. Counting it made file_count >= 1 for
    # every run no matter what, which quietly made the `file_count == 0` ->
    # "no files produced" guard UNREACHABLE: abstractcode-coder-2 died at the
    # door (gateway unreachable), produced nothing but that one empty file, and
    # was recorded VALID. Excluded, so an empty run reads as empty.
    n = b = 0
    for p in root.rglob("*"):
        if not p.is_file() or ".git" in p.parts:
            continue
        if p.name == ".gitignore" and p.parent == root:
            continue
        n += 1
        b += p.stat().st_size
    return n, b


def isolation_strays(out_dir: Path, ws_root: Path) -> list[Path]:
    """Prove no game is reachable by walking up out of the workspace.

    Agents have shell access, so `cd ..` from the workspace must not reach a
    prior game to copy. The original check scanned all of ws_root — correct
    when runs were sequential, fatal at CB_PARALLEL>1: sibling CELLS live in
    ws_root simultaneously, so every cell starting after the first wave found
    an in-flight sibling's game.js and died as `harness error`, which would
    have voided 21 of the 24 matrix cells. Sibling `<cell>/product` subtrees
    are harness-managed build areas and are excluded; anything else loose in
    the ancestry (an archived game, a crashed lane's leftovers) still trips.
    """
    strays: list[Path] = []
    for anc in (out_dir.parent, ws_root):
        for name in ("game.js", "index.html"):
            for q in anc.rglob(name):
                if q.is_relative_to(out_dir):
                    continue
                try:
                    rel = q.relative_to(ws_root)
                except ValueError:
                    strays.append(q)
                    continue
                if len(rel.parts) >= 2 and rel.parts[1] == "product":
                    continue  # a concurrent sibling cell, not contamination
                strays.append(q)
    return sorted(set(strays))


def live_peers(out_dir: Path, ws_root: Path) -> list[str]:
    """Sibling cells holding a product while this one starts — the honest
    record of the parallel window, so cross-run contamination stays auditable
    instead of being silently excluded by isolation_strays()."""
    peers = set()
    for name in ("game.js", "index.html"):
        for q in ws_root.rglob(name):
            if q.is_relative_to(out_dir):
                continue
            rel = q.relative_to(ws_root)
            if len(rel.parts) >= 2 and rel.parts[1] == "product":
                peers.add(rel.parts[0])
    return sorted(peers)


_RUN_ID = re.compile(r"run\s+([0-9a-f]{8}-[0-9a-f-]{4,})")
_EFFORT = re.compile(
    r"(?:done(?::\s*\w+)?|stopped[^·\n]*)\s·\s(\d+)\s+llm calls\s·\s(\d+)\s+tools")


def parse_effort(text: str, r: Run) -> None:
    """llm/tool counts for the report table. This script recorded neither, so
    'how much work did the arm actually do' could not be stated at all — and,
    worse, a run that made ZERO model calls was indistinguishable from one that
    built the game. Gateway arms print the counts; the external clients do not,
    and are left at 0 rather than guessed."""
    m = _EFFORT.search(text)
    if m:
        r.llm_calls, r.tool_calls = int(m.group(1)), int(m.group(2))
        r.effort_reported = True


def read_store(r: Run, text: str) -> None:
    """Route verification from the runtime's own durable record — never from
    what this harness intended to send. Ported from bench_workflows.py, which
    had it and this script did not: the matrix was reporting an INTENDED route
    for the gateway arms with nothing checking the wire."""
    m = _RUN_ID.search(text)
    if not m:
        return
    r.run_id = m.group(1)
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


# One slot per concurrent cell. A single shared slot (the previous design) is
# only correct at PARALLEL=1: with three workers it holds whichever cell
# registered last, so Ctrl-C terminated ONE client and left the other two
# running. Measured on the 2026-08-02 abort — see _void-abortedlowpower-*.
_INFLIGHT: dict = {"procs": {}}
_INFLIGHT_LOCK = __import__("threading").Lock()


def _abort(signum, _frame):
    """Abort must actually stop the benchmark.

    The previous handler terminated one child and raised SystemExit. SystemExit
    unwinds the results loop, but the ThreadPoolExecutor's `with` block then
    calls shutdown(wait=True) on non-daemon worker threads, so the process kept
    running every remaining cell — clients burning tokens for hours while the
    results loop that would have recorded them was already dead. The operator's
    abort silently produced a zombie benchmark. Terminate every in-flight child,
    then leave immediately via os._exit so no executor shutdown can wait on it.
    """
    with _INFLIGHT_LOCK:
        procs = list(_INFLIGHT["procs"].items())
    for _tag, p in procs:
        try:
            if p and p.poll() is None:
                p.terminate()
        except Exception:  # noqa: BLE001
            pass
    time.sleep(3)
    for _tag, p in procs:
        try:
            if p and p.poll() is None:
                p.kill()
        except Exception:  # noqa: BLE001
            pass
    print(f"\n⚠ aborted — terminated {len(procs)} in-flight client(s)", flush=True)
    sys.stdout.flush()
    sys.stderr.flush()
    os._exit(130)


def one_run(client: str, rep: int, ws_root: Path, prompt: str) -> Run:
    r = Run(arm=client, rep=rep)
    out_dir = ws_root / f"{client}-{rep}" / "product"
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True)
    # Prove isolation rather than assert it: agents have shell access, so a
    # prior game anywhere above the workspace is reachable with `cd ..`.
    strays = isolation_strays(out_dir, ws_root)
    assert not strays, f"game artifact reachable above {out_dir}: {strays[:2]}"
    r.concurrent_peers = live_peers(out_dir, ws_root)

    # A bare directory is not a boundary: clients resolve a "project root" by
    # walking up for a VCS marker, and one of them climbed all the way into this
    # repo, read the grader, ran it, and tuned its output to it. An initialised
    # repo at the workspace root stops that walk here.
    subprocess.run(["git", "init", "-q"], cwd=str(out_dir),
                   capture_output=True, timeout=60)
    (out_dir / ".gitignore").write_text("")
    cmd, agent, reasoning_applied = build_cmd(client, prompt, out_dir)
    r.agent, r.reasoning_applied = agent, reasoning_applied
    log = OUT / f"{client}-{rep}.log"
    r.log_path, r.out_dir = str(log), str(out_dir)
    r.workspace_used = str(out_dir)
    r.started_at = datetime.now(timezone.utc).isoformat()
    r.loadavg = round(os.getloadavg()[0], 2)

    # Strip client env AND every *_API_KEY: this benchmark runs on the
    # operator's subscription-backed relay; a stray key would silently bill an API.
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("ABSTRACTCODE_", "ABSTRACTGATEWAY_", "ABSTRACTMEMORY_"))
           and not k.endswith(("_API_KEY", "_APIKEY"))}
    if client in ("abstractcode-coder", "abstractcode-basic"):
        # Workflow mode reaches the gateway through env, not exec flags.
        gw = json.loads((Path.home() / ".abstractcode/gateway.json").read_text())
        env["ABSTRACTCODE_GATEWAY_URL"] = gw.get("base_url", "http://127.0.0.1:8080")
        env["ABSTRACTCODE_GATEWAY_TOKEN"] = gw.get("token", "")
        env["ABSTRACTGATEWAY_AUTH_TOKEN"] = gw.get("token", "")
    before = repo_dirty()
    t0 = time.monotonic()
    try:
        with log.open("w") as fh:
            # cwd IS the workspace for opencode/pi/abstractcode (none take a
            # workspace flag); the TUI gets --workspace explicitly.
            # stdin=DEVNULL: codex `exec` reads stdin ("Reading additional
            # input from stdin...") and an inherited terminal stdin makes it
            # wait for EOF that never comes — a silent 3h hang-catcher burn on
            # every codex cell. With PARALLEL>1 the inherited fd is also shared
            # between concurrent children. No arm needs interactive stdin here.
            p = subprocess.Popen(cmd, cwd=str(out_dir), env=env,
                                 stdin=subprocess.DEVNULL,
                                 stdout=fh, stderr=subprocess.STDOUT)
            with _INFLIGHT_LOCK:
                _INFLIGHT["procs"][f"{client}-{rep}"] = p
            r.exit_code = p.wait(timeout=(WALL_S + 60) if WALL_S else REAP_S)
    except subprocess.TimeoutExpired:
        p.kill()
        r.exit_code = 124
    except Exception as exc:  # noqa: BLE001
        r.exit_code, r.notes = 1, str(exc)[:200]
    finally:
        with _INFLIGHT_LOCK:
            _INFLIGHT["procs"].pop(f"{client}-{rep}", None)
    r.elapsed_s = round(time.monotonic() - t0, 1)
    r.stray_writes = sorted(repo_dirty() - before)[:10]
    r.file_count, r.total_bytes = count_tree(out_dir)

    text = log.read_text(errors="replace") if log.is_file() else ""
    parse_effort(text, r)
    if client in GATEWAY_ARMS:
        read_store(r, text)
        if r.wire_model is None:
            r.route_verified = "unverifiable (no run store record found)"
        elif str(r.wire_model) == MODEL and str(r.wire_thinking) == REASONING:
            r.route_verified = f"CONFIRMED {r.wire_model}/{r.wire_thinking} (run store)"
        else:
            r.route_verified = f"DRIFT {r.wire_model!r}/{r.wire_thinking!r} (run store)"
    else:
        r.route_verified = "unverifiable (external client, no run store)"
    tl = text.lower()
    # Benchmark capture: a run that read or executed the grader is measuring the
    # measuring instrument. Void it regardless of how good the artifact looks.
    if "zelda_review_score" in tl or "zelda_headless_bench" in tl:
        r.verdict, r.discard_reason = (
            "DISCARD", "read or ran the grader — benchmark capture, not a coding result")
    elif "outside the operator workspace scope" in tl:
        # The gateway refuses a client-declared workspace_root under /private/tmp
        # ("The run was NOT started"), so no run, no files. Left to fall through,
        # this surfaces as "no files produced" — a client verdict for a gateway
        # configuration block. Deliberately NOT marked INFRA: the retry loop
        # would re-fail three times against a setting that cannot change while
        # the server is up. Remedy is a gateway restart with
        # ABSTRACTGATEWAY_ALLOW_CLIENT_WORKSPACE_SCOPE=1 or a workspace mount.
        r.verdict, r.discard_reason = (
            "BLOCKED", "gateway refused the workspace root (operator scope) — "
                       "run never started; not a client result")
    elif ("at their limits" in tl or "upstream connection failed" in tl
            or "not found for openai-compatible" in tl
            # A DEAD RELAY DOES NOT LOOK LIKE A FAILURE HERE. Measured against a
            # real outage: the gateway arms exit 0, the agent keeps running its
            # loop against a provider that refuses every connection, the log
            # fills with "circuit breaker open for OpenAICompatibleProvider",
            # and the run ends with `done · 0 llm calls`. None of the patterns
            # above match and the exit code is 0, so the run was headed for
            # VALID — an infra outage scored as a coding verdict, which is the
            # one thing this harness is supposed to never do.
            or "circuit breaker open for" in tl
            # abstractcode is gateway-first and refuses to start when the
            # gateway blips ("requires a healthy AbstractGateway connection …
            # unreachable"). Observed mid-matrix as a transient connection
            # reset under concurrency. That is infrastructure, not the client
            # failing to code, so it must retry rather than score a zero.
            or "requires a healthy abstractgateway connection" in tl
            # Only when the client ITSELF reported "0 llm calls". Inferring the
            # outage from llm_calls==0 alone is wrong: the abstractcode client
            # never prints the counts line at all (checked against every log in
            # _archive-clientbench-20260801T193553Z), so a bare ==0 test marks
            # all six abstractcode-* cells INFRA and retries each of them three
            # times — the harness destroying the arm it is meant to measure.
            or (r.effort_reported and r.llm_calls == 0)
            or (r.exit_code != 0 and r.elapsed_s < 10)):
        # Quota-closed relays WITHDRAW gpt-* models, so clients die at the door
        # ("Model 'gpt-5.4' not found") in under a second. The first confirm
        # run labeled two such deaths VALID off leftover files — an infra
        # death is never a client verdict.
        r.verdict = "INFRA"
        r.discard_reason = "relay/upstream unavailable (not a client failure)"
        r.infra_failure = True
    elif r.wire_model is not None and str(r.wire_model) != MODEL:
        # Operator rule: a run off-route is not a datum. Only assertable for the
        # gateway arms — the external clients have no equivalent record, which is
        # itself reported rather than papered over.
        r.verdict, r.discard_reason = "DISCARD", f"model drift: ran {r.wire_model!r}"
    elif r.wire_thinking is not None and str(r.wire_thinking) != REASONING:
        r.verdict, r.discard_reason = "DISCARD", f"reasoning drift: ran {r.wire_thinking!r}"
    elif r.exit_code == 124:
        r.verdict, r.discard_reason = "DISCARD", f"exceeded the {REAP_S}s hang-catcher"
    elif r.stray_writes:
        r.verdict, r.discard_reason = "DISCARD", f"wrote outside the workspace: {r.stray_writes[:3]}"
    elif r.file_count == 0:
        r.verdict, r.discard_reason = "DISCARD", "no files produced"
    else:
        # A non-zero exit with a real product is NOT discarded: several clients
        # exit non-zero on a trailing tool error while the deliverable is
        # complete. The scorer judges the product; the exit code is recorded.
        r.verdict = "VALID"
        if r.exit_code != 0:
            r.notes = f"exit {r.exit_code} with {r.file_count} files produced"

    try:
        keep = OUT / f"{client}-{rep}-product"
        if keep.exists():
            shutil.rmtree(keep)
        shutil.copytree(out_dir, keep, ignore=shutil.ignore_patterns(".git"))
        r.archived_product, r.out_dir = str(keep), str(keep)
        shutil.rmtree(out_dir.parent, ignore_errors=True)
    except Exception as exc:  # noqa: BLE001
        r.archived_product = f"archive failed: {exc}"
    return r


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--clients", default="tui-basic,tui-coder,tui-multi,abstractcode-basic,abstractcode-coder,codex,opencode,pi")
    ap.add_argument("--repeats", type=int, default=REPEATS)
    ap.add_argument("--smoke", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()

    if (MODEL, REASONING, BASE_URL) != REQUIRED_ROUTE:
        print(f"✗ route must be {REQUIRED_ROUTE}, got {(MODEL, REASONING, BASE_URL)}", file=sys.stderr)
        return 2
    clients = [c.strip() for c in a.clients.split(",") if c.strip()]
    prompt = SMOKE_PROMPT if a.smoke else (RTYPE_PROMPT if os.environ.get("CB_PROMPT", "rtype") == "rtype" else ZELDA_PROMPT)

    # `abstractcode --version` is not a flag — it prints usage, which silently
    # became the "version" string. Read the installed package instead.
    versions = {
        "abstractcode": client_version([sys.executable, "-c",
                                        "import abstractcode;print(abstractcode.__version__)"]),
        "abstractcode-tui": client_version([str(TUI_BIN), "--version"]),
        "opencode": client_version(["opencode", "--version"]),
        "pi": client_version(["pi", "--version"]),
        "codex": client_version(["codex", "--version"]),
    }
    prov = {
        "generated": datetime.now(timezone.utc).isoformat(),
        "mode": "SMOKE" if a.smoke else "MATRIX",
        "model": MODEL, "reasoning": REASONING, "base_url": BASE_URL,
        "subscription_backed": True, "api_key_used": False,
        # Recorded because it was NOT recorded before: a mid-matrix power-mode
        # change contaminates the wall-time column (scores are load-independent
        # by construction, but elapsed_s is not). 5 low-power cells were
        # discarded rather than mixed into a high-power matrix.
        "power_mode": subprocess.run(
            ["pmset", "-g"], capture_output=True, text=True).stdout.strip()[:400],
        "repeats": a.repeats, "clients": clients,
        "client_versions": versions,
        "wall_budget_s": WALL_S or "uncapped",
        "reasoning_asymmetry": {
            "abstractcode": "NOT APPLIED — abstractcore openai_compatible has no "
                            "thinking-control mapping for gpt-5.4; runs at relay default",
            # MEASURED 2026-08-02 by smoke-running both arms and reading the
            # gateway run store, rather than trusting the intent:
            "abstractcode-basic": "NOT APPLIED — runs a LOCAL abstractcore loop "
                                  "(no gateway run is created at all) and emits the "
                                  "explicit RuntimeWarning 'no control was applied'",
            "abstractcode-coder": "NOT APPLIED — the previous note claimed 'the "
                                  "gateway lane applies it'. It does not: the gateway "
                                  "run it creates records _runtime.thinking = None, "
                                  "i.e. the relay default, not medium",
            "abstractcode-tui": "applied via gateway _runtime.thinking "
                                "(CONFIRMED: run store shows thinking='medium')",
            # `--variant` does NOT appear in `opencode run --help` on 1.18.10,
            # so it looked like a silently-dropped no-op. Smoke-tested instead
            # of assumed: opencode.db records variant='medium' alongside
            # providerID='bench8317', modelID='gpt-5.4'. It is applied.
            "opencode": "--variant medium (CONFIRMED — opencode.db records "
                        "variant='medium'; undocumented in `run --help`)",
            "pi": "model suffix :medium (CONFIRMED — pi's own session jsonl "
                  "records thinkingLevel=medium)",
            "codex": "medium (codex config default, shown in its banner and rollout)",
        },
        # NOT every arm is behind the relay. Recorded here so the page and the
        # report cannot silently inherit the "identical route" claim.
        "route_asymmetry": {
            "relay_arms": ["tui-basic", "tui-coder", "tui-multi",
                           "abstractcode-basic", "abstractcode-coder",
                           "opencode", "pi"],
            "codex": "provider=openai (operator ChatGPT subscription), NOT the "
                     "8317 relay — no bench8317 profile exists in ~/.codex/config.toml",
        },
    }
    print(json.dumps(prov, indent=2))
    if a.dry_run:
        for c in clients:
            for i in range(1, a.repeats + 1):
                print(f"  {c}-{i}")
        return 0

    if OUT.exists() and any(OUT.iterdir()):
        print(f"✗ refusing to start: {OUT} exists and is not empty — archive it first "
              f"(this harness never resumes or merges a prior bench).", file=sys.stderr)
        return 2
    OUT.mkdir(parents=True, exist_ok=True)
    signal.signal(signal.SIGINT, _abort)
    signal.signal(signal.SIGTERM, _abort)

    ws_root = Path(os.environ.get("BENCH_WS_BASE",
        "/Users/albou/tmp/abstractframework/.bench-ws")) / (
        # IN-SCOPE by necessity: the gateway now REFUSES an out-of-scope
        # workspace_root with a 400 instead of silently clamping it
        # (backlog 0232 §1, the fail-loud fix). /private/tmp is outside the
        # operator roots, so builds live under a dedicated, game-free
        # directory inside the allowed tree instead of weakening the guard.
        f"{OUT.name}-{os.getpid()}-"
        f"{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}")
    ws_root.mkdir(parents=True, exist_ok=True)
    (OUT / "provenance.json").write_text(json.dumps(prov, indent=2))

    runs: list[Run] = []
    import concurrent.futures, random, threading
    _lock = threading.Lock()

    def _cell(c: str, rep: int, stagger: float) -> Run:
        time.sleep(stagger)
        for attempt in range(1, 4):
            r = one_run(c, rep, ws_root, prompt)
            if not getattr(r, "infra_failure", False):
                return r
            print(f"    INFRA {c}-{rep} attempt {attempt}/3; waiting for the relay", flush=True)
            for _ in range(60):
                time.sleep(20)
                if model_available():
                    break
        return r

    cells = [(c, rep) for rep in range(1, a.repeats + 1) for c in clients]
    print(f"parallel workers: {PARALLEL} — {len(cells)} cells", flush=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=PARALLEL) as pool:
        futs = {pool.submit(_cell, c, rep, i * random.uniform(10, 20) if i < PARALLEL else 0.0): (c, rep)
                for i, (c, rep) in enumerate(cells)}
        for fut in concurrent.futures.as_completed(futs):
            c, rep = futs[fut]
            try:
                r = fut.result()
            except Exception as exc:  # noqa: BLE001
                r = Run(arm=c, rep=rep, verdict="DISCARD",
                        discard_reason=f"harness error: {str(exc)[:160]}")
            runs.append(r)
            print(f"    {r.verdict:8} {c}-{rep} exit={r.exit_code} {r.elapsed_s}s "
                  f"files={r.file_count} bytes={r.total_bytes} {r.discard_reason}", flush=True)
            with _lock:
                (OUT / "runs.json").write_text(json.dumps(
                    {"provenance": prov,
                     "runs": [asdict(x) for x in sorted(runs, key=lambda q: (q.rep, q.arm))]}, indent=2))

    ok = sum(1 for r in runs if r.verdict == "VALID")
    print(f"\n{ok}/{len(runs)} VALID")
    print(f"score with: python3 scripts/zelda_review_score.py --root {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
