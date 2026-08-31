#!/usr/bin/env python3
"""Live proof that AGENTS.md reaches the model through the gateway.

A contract test, not a vibe check: the temp workspace's AGENTS.md carries an
instruction the prompt itself never mentions. If the answer obeys it, the
project context rode `_runtime.system_prompt_extra` all the way into the
agent's system prompt. Then the same run is repeated with
`--no-project-context` — the token must DISAPPEAR, which is what proves the
first result came from the injection and not from the model guessing.

Usage:
  python3 scripts/project_context_live_verify.py [workflow-ref]
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
BIN = REPO / "target" / "release" / "abstractcode"
TOKEN = "AGENTS-MD-SEEN-7Q4X"
WORKFLOW = sys.argv[1] if len(sys.argv) > 1 else "basic-agent"
PROVIDER = os.environ.get("WFP_PROVIDER", "lmstudio")
MODEL = os.environ.get("WFP_MODEL", "qwen/qwen3-4b")
# #[WARNING:TIMEOUT] Uncapped by default (0 → `exec --timeout 0`), per
# ADR-0027 §2/§3. This probe asks one trivial question, so it normally answers
# in seconds — but a cap here would still turn a slow substrate into a fake
# "AGENTS.md not injected" verdict, which is the one conclusion this script
# exists to state reliably. WFP_TIMEOUT_S imposes a cap deliberately.
TIMEOUT_S = int(os.environ.get("WFP_TIMEOUT_S", "0"))
SUBPROCESS_REAP_S = int(os.environ.get("WFP_REAP_S", "14400"))

AGENTS_MD = f"""# Project instructions

- Answer in one short sentence.
- End EVERY reply with this exact token on its own line: {TOKEN}
"""
# Deliberately says nothing about the token — only AGENTS.md does.
PROMPT = "What is 2 + 2? Answer briefly."


def gateway() -> tuple[str, str]:
    p = Path.home() / ".abstractcode" / "gateway.json"
    if p.is_file():
        d = json.loads(p.read_text())
        return str(d.get("base_url", "http://127.0.0.1:8080")), str(d.get("token", ""))
    return (
        os.environ.get("ABSTRACTGATEWAY_URL", "http://127.0.0.1:8080"),
        os.environ.get("ABSTRACTGATEWAY_AUTH_TOKEN", ""),
    )


def run(ws: Path, *, opt_out: bool) -> tuple[int, str]:
    url, tok = gateway()
    cmd = [
        str(BIN), "exec", PROMPT,
        "--workflow", WORKFLOW,
        "--provider", PROVIDER,
        "--model", MODEL,
        "--gateway", url,
        "--token", tok,
        "--permissions", "read",
        "--ungated",
        "--max-iterations", "4",
        "--timeout", str(TIMEOUT_S),
        "--workspace", str(ws),
    ]
    if opt_out:
        cmd.append("--no-project-context")
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("ABSTRACTCODE_", "ABSTRACTGATEWAY_", "ABSTRACTMEMORY_"))}
    try:
        p = subprocess.run(
            cmd, cwd=str(REPO), env=env, capture_output=True,
            text=True,
            # #[WARNING:TIMEOUT] hang-catcher only — see SUBPROCESS_REAP_S.
            timeout=(TIMEOUT_S + 30) if TIMEOUT_S > 0 else SUBPROCESS_REAP_S,
        )
        return p.returncode, (p.stdout or "") + (p.stderr or "")
    except subprocess.TimeoutExpired as exc:
        out = (exc.stdout or b"") + (exc.stderr or b"")
        return 124, out.decode(errors="replace") if isinstance(out, bytes) else str(out)


def main() -> int:
    if not BIN.is_file():
        print(f"missing binary: {BIN} (cargo build --release)", file=sys.stderr)
        return 2
    with tempfile.TemporaryDirectory(prefix="acode-pctx-live-") as td:
        ws = Path(td)
        (ws / "AGENTS.md").write_text(AGENTS_MD)

        rc_on, out_on = run(ws, opt_out=False)
        seen_on = TOKEN in out_on
        announced = "project context:" in out_on
        print(f"injected   : exit={rc_on} token_seen={seen_on} announced={announced}")

        rc_off, out_off = run(ws, opt_out=True)
        seen_off = TOKEN in out_off
        print(f"opted out  : exit={rc_off} token_seen={seen_off}")

        for tag, text in (("injected", out_on), ("opted-out", out_off)):
            dest = REPO / "untracked" / "workflow-conformance" / f"pctx-{tag}.log"
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(text)

    ok = seen_on and announced and not seen_off
    print("\nVERDICT:", "PASS — AGENTS.md reaches the model, and only when injected"
          if ok else "FAIL — see untracked/workflow-conformance/pctx-*.log")
    if not seen_on:
        print("  the injected run never showed the token: context did not reach the model")
    if seen_off:
        print("  the opted-out run showed the token: --no-project-context is not honored")
    if not announced:
        print("  the client never announced the injection on stderr")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
