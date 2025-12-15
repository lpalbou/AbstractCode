from __future__ import annotations

import argparse
import sys
from typing import Optional, Sequence

from .react_shell import ReactShell


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="abstractcode",
        description="AbstractCode: an interactive terminal shell for AbstractFramework agents (MVP).",
    )
    parser.add_argument("--provider", default="ollama", help="LLM provider (e.g. ollama, openai)")
    parser.add_argument("--model", default="qwen3:1.7b-q4_K_M", help="Model name")
    parser.add_argument(
        "--state-file",
        help="Path to save the current run reference (enables durable file-backed stores).",
    )
    parser.add_argument(
        "--auto-approve",
        action="store_true",
        help="Automatically approve tool calls (unsafe; disables interactive approvals).",
    )
    parser.add_argument("--no-color", action="store_true", help="Disable ANSI colors")
    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = build_parser().parse_args(list(argv) if argv is not None else None)

    shell = ReactShell(
        provider=args.provider,
        model=args.model,
        state_file=args.state_file,
        auto_approve=bool(args.auto_approve),
        color=not bool(args.no_color),
    )
    shell.run()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

