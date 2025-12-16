from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from typing import Optional, Sequence

from .react_shell import ReactShell


def _default_state_file() -> str:
    env = os.getenv("ABSTRACTCODE_STATE_FILE")
    if env:
        return env
    return str(Path.home() / ".abstractcode" / "state.json")


def _default_max_iterations() -> int:
    env = os.getenv("ABSTRACTCODE_MAX_ITERATIONS")
    if env:
        try:
            value = int(env)
        except ValueError:
            raise SystemExit("ABSTRACTCODE_MAX_ITERATIONS must be an integer.")
        if value < 1:
            raise SystemExit("ABSTRACTCODE_MAX_ITERATIONS must be >= 1.")
        return value
    return 25


def _default_max_tokens() -> Optional[int]:
    env = os.getenv("ABSTRACTCODE_MAX_TOKENS")
    if env:
        try:
            value = int(env)
        except ValueError:
            raise SystemExit("ABSTRACTCODE_MAX_TOKENS must be an integer.")
        if value < 1024:
            raise SystemExit("ABSTRACTCODE_MAX_TOKENS must be >= 1024.")
        return value
    return 32768  # Default 32k context


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="abstractcode",
        description="AbstractCode: an interactive terminal shell for AbstractFramework agents (MVP).",
    )
    parser.add_argument(
        "--agent",
        choices=("react", "codeact"),
        default=os.getenv("ABSTRACTCODE_AGENT", "react"),
        help="Agent type to run (react|codeact).",
    )
    parser.add_argument("--provider", default="ollama", help="LLM provider (e.g. ollama, openai)")
    parser.add_argument("--model", default="qwen3:1.7b-q4_K_M", help="Model name")
    parser.add_argument(
        "--state-file",
        default=_default_state_file(),
        help="Path to save the current run reference (enables durable file-backed stores).",
    )
    parser.add_argument(
        "--no-state",
        action="store_true",
        help="Disable persistence (keeps run state in memory; cannot resume after quitting).",
    )
    parser.add_argument(
        "--auto-approve",
        "--auto-accept",
        action="store_true",
        dest="auto_approve",
        help="Automatically approve tool calls (unsafe; disables interactive approvals).",
    )
    parser.add_argument(
        "--max-iterations",
        type=int,
        default=_default_max_iterations(),
        help="Maximum ReAct reasoning iterations per task (default: 25).",
    )
    parser.add_argument(
        "--max-tokens",
        type=int,
        default=_default_max_tokens(),
        help="Maximum context tokens for LLM calls (default: 32768).",
    )
    parser.add_argument("--no-color", action="store_true", help="Disable ANSI colors")
    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = build_parser().parse_args(list(argv) if argv is not None else None)
    state_file = None if args.no_state else args.state_file

    shell = ReactShell(
        agent=str(args.agent),
        provider=args.provider,
        model=args.model,
        state_file=state_file,
        auto_approve=bool(args.auto_approve),
        max_iterations=int(args.max_iterations),
        max_tokens=args.max_tokens,
        color=not bool(args.no_color),
    )
    shell.run()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
