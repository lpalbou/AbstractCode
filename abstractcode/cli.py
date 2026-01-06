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
        if value != -1 and value < 1024:
            raise SystemExit("ABSTRACTCODE_MAX_TOKENS must be -1 (auto) or >= 1024.")
        return value
    return -1  # Auto (use model capabilities)


def build_agent_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="abstractcode",
        description="AbstractCode: an interactive terminal shell for AbstractFramework (agents + workflows).",
        epilog=(
            "Workflows:\n"
            "  abstractcode flow --help   Run AbstractFlow workflows from the terminal\n"
            "REPL:\n"
            "  Use /flow inside the REPL to run workflows while keeping chat context.\n"
        ),
        formatter_class=argparse.RawTextHelpFormatter,
    )
    parser.add_argument(
        "--agent",
        default=os.getenv("ABSTRACTCODE_AGENT", "react"),
        help=(
            "Agent selector:\n"
            "  - Built-ins: react | codeact | memact\n"
            "  - Workflow agent: <flow_id> | <flow_name> | </path/to/flow.json>\n"
            "    (must implement interface 'abstractcode.agent.v1')"
        ),
    )
    parser.add_argument("--provider", default="ollama", help="LLM provider (e.g. ollama, openai)")
    parser.add_argument("--model", default="qwen3:1.7b-q4_K_M", help="Model name")
    parser.add_argument(
        "--base-url",
        default=os.getenv("ABSTRACTCODE_BASE_URL"),
        help="Provider base URL (e.g. http://localhost:1234/v1). Also supports ABSTRACTCODE_BASE_URL.",
    )
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
        "--plan",
        action="store_true",
        help="Enable Plan mode (agent generates a TODO plan before acting).",
    )
    parser.add_argument(
        "--review",
        action="store_true",
        dest="review",
        help="Enable verifier mode (default: enabled).",
    )
    parser.add_argument(
        "--no-review",
        action="store_false",
        dest="review",
        help="Disable verifier mode (not recommended).",
    )
    parser.set_defaults(review=True)
    parser.add_argument(
        "--review-max-rounds",
        type=int,
        default=3,
        help="Max verifier rounds per task (default: 3).",
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
        help="Maximum context tokens for LLM calls (-1 = auto from model capabilities).",
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
        "--verbosity",
        choices=("none", "default", "full"),
        default="default",
        help="Observability level: none|default|full (default: default).",
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
        "--verbosity",
        choices=("none", "default", "full"),
        default="default",
        help="Observability level: none|default|full (default: default).",
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

    runs = sub.add_parser("runs", help="List recent flow runs from the flow store")
    runs.add_argument(
        "--flow-state-file",
        default=None,
        help="Path to the saved run reference (default: ~/.abstractcode/flow_state.json).",
    )
    runs.add_argument("--limit", type=int, default=20, help="Maximum runs to show (default: 20)")

    attach = sub.add_parser("attach", help="Attach to an existing flow run_id (sets the current flow_state.json ref)")
    attach.add_argument("run_id", help="Existing run_id to attach to")
    attach.add_argument("--flows-dir", default=None, help="Directory containing VisualFlow JSON files")
    attach.add_argument(
        "--flow-state-file",
        default=None,
        help="Path to the saved run reference (default: ~/.abstractcode/flow_state.json).",
    )

    emit = sub.add_parser("emit", help="Emit a custom event (or resume a raw wait_key) for the current flow session")
    emit.add_argument("--name", default=None, help="Custom event name to emit")
    emit.add_argument("--wait-key", default=None, help="Raw wait_key to resume (advanced)")
    emit.add_argument("--scope", default="session", help="Event scope: session|workflow|run|global (default: session)")
    emit.add_argument("--payload-json", default=None, help="Event payload as JSON (object preferred)")
    emit.add_argument(
        "--payload-file",
        default=None,
        help="Path to a JSON file containing the event payload",
    )
    emit.add_argument(
        "--session-id",
        default=None,
        help="Target session id (defaults to current root run_id for session scope)",
    )
    emit.add_argument(
        "--max-steps",
        type=int,
        default=0,
        help="Tick budget per resumed run (default: 0; host drives execution)",
    )
    emit.add_argument("--flows-dir", default=None, help="Directory containing VisualFlow JSON files")
    emit.add_argument(
        "--flow-state-file",
        default=None,
        help="Path to the saved run reference (default: ~/.abstractcode/flow_state.json).",
    )
    emit.add_argument(
        "--auto-approve",
        "--accept-tools",
        "--auto-accept",
        action="store_true",
        dest="auto_approve",
        help="Automatically approve tool calls (unsafe; disables interactive approvals).",
    )

    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    argv_list = list(argv) if argv is not None else sys.argv[1:]

    if argv_list and argv_list[0] == "flow":
        parser = build_flow_parser()
        args, unknown = parser.parse_known_args(argv_list[1:])
        from .flow_cli import (
            attach_flow_run_command,
            control_flow_command,
            emit_flow_event_command,
            list_flow_runs_command,
            resume_flow_command,
            run_flow_command,
        )

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
                verbosity=str(getattr(args, "verbosity", "default") or "default"),
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
                verbosity=str(getattr(args, "verbosity", "default") or "default"),
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
        if cmd == "runs":
            if unknown:
                parser.error(f"Unknown arguments: {' '.join(unknown)}")
            list_flow_runs_command(flow_state_file=args.flow_state_file, limit=int(args.limit or 20))
            return 0
        if cmd == "attach":
            if unknown:
                parser.error(f"Unknown arguments: {' '.join(unknown)}")
            attach_flow_run_command(
                run_id=str(args.run_id),
                flows_dir=args.flows_dir,
                flow_state_file=args.flow_state_file,
            )
            return 0
        if cmd == "emit":
            if unknown:
                parser.error(f"Unknown arguments: {' '.join(unknown)}")
            emit_flow_event_command(
                name=args.name,
                wait_key=args.wait_key,
                scope=args.scope,
                payload_json=args.payload_json,
                payload_file=args.payload_file,
                session_id=args.session_id,
                max_steps=int(args.max_steps or 0),
                flows_dir=args.flows_dir,
                flow_state_file=args.flow_state_file,
                auto_approve=bool(args.auto_approve),
            )
            return 0

        build_flow_parser().print_help()
        return 2

    args = build_agent_parser().parse_args(argv_list)
    state_file = None if args.no_state else args.state_file

    shell = ReactShell(
        agent=str(args.agent),
        provider=args.provider,
        model=args.model,
        base_url=getattr(args, "base_url", None),
        state_file=state_file,
        auto_approve=bool(args.auto_approve),
        plan_mode=bool(args.plan),
        review_mode=bool(args.review),
        review_max_rounds=int(args.review_max_rounds),
        max_iterations=int(args.max_iterations),
        max_tokens=args.max_tokens,
        color=not bool(args.no_color),
    )
    shell.run()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
