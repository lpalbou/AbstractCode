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


def build_agent_parser() -> argparse.ArgumentParser:
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


def build_flow_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="abstractcode flow",
        description="Run AbstractFlow visual workflows from AbstractCode.",
    )
    sub = parser.add_subparsers(dest="command")

    run = sub.add_parser("run", help="Start a new flow run")
    run.add_argument("flow", help="Flow id (from flows dir) or path to a VisualFlow .json file")
    run.add_argument("--flows-dir", default=None, help="Directory containing VisualFlow JSON files")
    run.add_argument(
        "--input-json",
        default=None,
        help='JSON object string passed to the flow entry (e.g. \'{"query":"..."}\')',
    )
    run.add_argument(
        "--input-file",
        "--input-json-file",
        dest="input_file",
        default=None,
        help="Path to a JSON file (object) passed to the flow entry",
    )
    run.add_argument(
        "--param",
        action="append",
        default=[],
        help="Set an input param as key=value (repeatable). Example: --param max_web_search=15",
    )
    run.add_argument(
        "--flow-state-file",
        default=None,
        help="Path to store the last flow run reference (default: ~/.abstractcode/flow_state.json).",
    )
    run.add_argument("--no-state", action="store_true", help="Disable persistence (cannot resume after quitting).")
    run.add_argument(
        "--auto-approve",
        "--accept-tools",
        "--auto-accept",
        action="store_true",
        dest="auto_approve",
        help="Automatically approve tool calls (unsafe; disables interactive approvals).",
    )
    run.add_argument(
        "--wait-until",
        action="store_true",
        help="If waiting on a time-based event (WAIT_UNTIL), keep sleeping and resuming automatically.",
    )

    resume = sub.add_parser("resume", help="Resume the last saved flow run and drive until it blocks again")
    resume.add_argument(
        "--flow-state-file",
        default=None,
        help="Path to the saved run reference (default: ~/.abstractcode/flow_state.json).",
    )
    resume.add_argument(
        "--auto-approve",
        "--accept-tools",
        "--auto-accept",
        action="store_true",
        dest="auto_approve",
        help="Automatically approve tool calls (unsafe; disables interactive approvals).",
    )
    resume.add_argument(
        "--wait-until",
        action="store_true",
        help="If waiting on a time-based event (WAIT_UNTIL), keep sleeping and resuming automatically.",
    )

    pause = sub.add_parser("pause", help="Pause the last saved flow run (best-effort includes descendants)")
    pause.add_argument(
        "--flow-state-file",
        default=None,
        help="Path to the saved run reference (default: ~/.abstractcode/flow_state.json).",
    )

    resume_run = sub.add_parser("resume-run", help="Resume a previously paused run (does not advance execution)")
    resume_run.add_argument(
        "--flow-state-file",
        default=None,
        help="Path to the saved run reference (default: ~/.abstractcode/flow_state.json).",
    )

    cancel = sub.add_parser("cancel", help="Cancel the last saved flow run (best-effort includes descendants)")
    cancel.add_argument(
        "--flow-state-file",
        default=None,
        help="Path to the saved run reference (default: ~/.abstractcode/flow_state.json).",
    )

    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    argv_list = list(argv) if argv is not None else sys.argv[1:]

    if argv_list and argv_list[0] == "flow":
        parser = build_flow_parser()
        args, unknown = parser.parse_known_args(argv_list[1:])
        from .flow_cli import control_flow_command, resume_flow_command, run_flow_command

        cmd = getattr(args, "command", None)
        if cmd == "run":
            run_flow_command(
                flow_ref=str(args.flow),
                flows_dir=args.flows_dir,
                input_json=args.input_json,
                input_file=args.input_file,
                params=list(getattr(args, "param", []) or []),
                extra_args=list(unknown or []),
                flow_state_file=args.flow_state_file,
                no_state=bool(args.no_state),
                auto_approve=bool(args.auto_approve),
                wait_until=bool(args.wait_until),
            )
            return 0
        if cmd == "resume":
            if unknown:
                parser.error(f"Unknown arguments: {' '.join(unknown)}")
            resume_flow_command(
                flow_state_file=args.flow_state_file,
                no_state=False,
                auto_approve=bool(args.auto_approve),
                wait_until=bool(args.wait_until),
            )
            return 0
        if cmd == "pause":
            if unknown:
                parser.error(f"Unknown arguments: {' '.join(unknown)}")
            control_flow_command(action="pause", flow_state_file=args.flow_state_file)
            return 0
        if cmd == "resume-run":
            if unknown:
                parser.error(f"Unknown arguments: {' '.join(unknown)}")
            control_flow_command(action="resume", flow_state_file=args.flow_state_file)
            return 0
        if cmd == "cancel":
            if unknown:
                parser.error(f"Unknown arguments: {' '.join(unknown)}")
            control_flow_command(action="cancel", flow_state_file=args.flow_state_file)
            return 0

        build_flow_parser().print_help()
        return 2

    args = build_agent_parser().parse_args(argv_list)
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
