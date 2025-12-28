from __future__ import annotations

import json
import os
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Sequence, Tuple

from prompt_toolkit.formatted_text import HTML

from .input_handler import create_prompt_session, create_simple_session
from .fullscreen_ui import FullScreenUI


def _supports_color() -> bool:
    if os.environ.get("NO_COLOR"):
        return False
    return bool(getattr(sys.stdout, "isatty", lambda: False)())


class _C:
    RESET = "\033[0m"
    DIM = "\033[2m"
    BOLD = "\033[1m"
    CYAN = "\033[36m"
    GREEN = "\033[32m"
    YELLOW = "\033[33m"
    MAGENTA = "\033[35m"
    RED = "\033[31m"


def _style(text: str, *codes: str, enabled: bool) -> str:
    if not enabled or not codes:
        return text
    return "".join(codes) + text + _C.RESET


def _xml_safe(text: str) -> str:
    """Escape text for safe inclusion in prompt_toolkit HTML.

    Removes XML-invalid control characters and then escapes HTML entities.
    """
    import html as html_lib
    import re
    # Remove control characters except tab (\x09), newline (\x0a), carriage return (\x0d)
    text = re.sub(r'[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]', '', str(text))
    return html_lib.escape(text)


@dataclass
class _ToolSpec:
    name: str
    description: str
    parameters: Dict[str, Any]


def _now_iso() -> str:
    from datetime import datetime, timezone

    return datetime.now(timezone.utc).isoformat()


def _get_message_id(message: Dict[str, Any]) -> Optional[str]:
    meta = message.get("metadata")
    if not isinstance(meta, dict):
        return None
    msg_id = meta.get("message_id")
    if isinstance(msg_id, str) and msg_id:
        return msg_id
    return None


def _insert_archived_span(
    *,
    active_messages: List[Dict[str, Any]],
    archived_messages: List[Dict[str, Any]],
    artifact_id: str,
) -> Tuple[List[Dict[str, Any]], int, int]:
    """Insert archived messages into an active context view.

    Insertion rule:
    - If a `memory_summary` system message references `artifact_id`, insert immediately after it.
    - Otherwise, insert after the last system message.

    Deduplication:
    - Skip archived messages whose `metadata.message_id` already exists in active context.
    """
    import uuid

    insert_at = 0
    for i, m in enumerate(active_messages):
        if m.get("role") == "system":
            insert_at = i + 1

    for i, m in enumerate(active_messages):
        if m.get("role") != "system":
            continue
        meta = m.get("metadata")
        if not isinstance(meta, dict):
            continue
        if meta.get("kind") == "memory_summary" and meta.get("source_artifact_id") == artifact_id:
            insert_at = i + 1
            break

    existing_ids = {mid for m in active_messages for mid in [_get_message_id(m)] if mid}
    to_insert: List[Dict[str, Any]] = []
    skipped = 0

    for m in archived_messages:
        if not isinstance(m, dict):
            continue
        m_copy = dict(m)
        meta = m_copy.get("metadata")
        if not isinstance(meta, dict):
            meta = {}
            m_copy["metadata"] = meta
        mid = meta.get("message_id")
        if not isinstance(mid, str) or not mid:
            mid = f"msg_{uuid.uuid4().hex}"
            meta["message_id"] = mid
        if mid in existing_ids:
            skipped += 1
            continue
        existing_ids.add(mid)
        if not m_copy.get("timestamp"):
            m_copy["timestamp"] = _now_iso()
        to_insert.append(m_copy)

    new_messages = list(active_messages[:insert_at]) + to_insert + list(active_messages[insert_at:])
    return new_messages, len(to_insert), skipped


class ReactShell:
    def __init__(
        self,
        *,
        agent: str,
        provider: str,
        model: str,
        state_file: Optional[str],
        auto_approve: bool,
        max_iterations: int,
        max_tokens: Optional[int] = 32768,
        color: bool,
    ):
        self._agent_kind = str(agent or "react").strip().lower()
        if self._agent_kind not in ("react", "codeact"):
            raise ValueError("agent must be 'react' or 'codeact'")
        self._provider = provider
        self._model = model
        self._state_file = state_file or None
        self._auto_approve = auto_approve
        self._max_iterations = int(max_iterations)
        if self._max_iterations < 1:
            raise ValueError("max_iterations must be >= 1")
        self._max_tokens = max_tokens
        # Enable ANSI colors - fullscreen_ui uses ANSI class to parse escape codes
        self._color = bool(color and _supports_color())

        # Lazy imports so `abstractcode --help` works even if deps aren't installed.
        try:
            from abstractagent.agents.codeact import CodeActAgent
            from abstractagent.agents.react import ReactAgent
            from abstractagent.tools import execute_python, self_improve
            from abstractcore.tools import ToolDefinition
            from abstractcore.tools.common_tools import (
                list_files,
                search_files,
                read_file,
                write_file,
                edit_file,
                execute_command,
                web_search,
                fetch_url,
            )
            from abstractruntime import InMemoryLedgerStore, InMemoryRunStore, JsonFileRunStore, JsonlLedgerStore
            from abstractruntime.core.models import RunStatus, WaitReason
            from abstractruntime.storage.snapshots import Snapshot, JsonSnapshotStore, InMemorySnapshotStore
            from abstractruntime.storage.artifacts import FileArtifactStore, InMemoryArtifactStore
            from abstractruntime.integrations.abstractcore import (
                LocalAbstractCoreLLMClient,
                MappingToolExecutor,
                PassthroughToolExecutor,
                create_local_runtime,
            )
        except Exception as e:  # pragma: no cover
            raise SystemExit(
                "AbstractCode requires AbstractAgent/AbstractRuntime/AbstractCore to be importable.\n"
                "In this monorepo, run with:\n"
                "  PYTHONPATH=abstractcode:abstractagent/src:abstractruntime/src:abstractcore python -m abstractcode.cli\n"
                f"\nImport error: {e}"
            )

        self._RunStatus = RunStatus
        self._WaitReason = WaitReason
        self._Snapshot = Snapshot
        self._JsonSnapshotStore = JsonSnapshotStore
        self._InMemorySnapshotStore = InMemorySnapshotStore

        # Default tools for AbstractCode (curated subset for coding tasks)
        DEFAULT_TOOLS = [
            list_files,
            search_files,
            read_file,
            write_file,
            edit_file,
            execute_command,
            web_search,
            fetch_url,
            self_improve,
        ]

        if self._agent_kind == "react":
            self._tools = list(DEFAULT_TOOLS)
            agent_cls = ReactAgent
        else:
            self._tools = [execute_python]
            agent_cls = CodeActAgent

        self._tool_specs: Dict[str, _ToolSpec] = {}
        for t in self._tools:
            tool_def = getattr(t, "_tool_definition", None) or ToolDefinition.from_function(t)
            self._tool_specs[tool_def.name] = _ToolSpec(
                name=tool_def.name,
                description=tool_def.description,
                parameters=dict(tool_def.parameters or {}),
            )

        store_dir: Optional[Path] = None
        # Stores: file-backed only when state_file is provided.
        if self._state_file:
            base = Path(self._state_file).expanduser().resolve()
            base.parent.mkdir(parents=True, exist_ok=True)
            store_dir = base.with_name(base.stem + ".d")
            run_store = JsonFileRunStore(store_dir)
            ledger_store = JsonlLedgerStore(store_dir)
            self._snapshot_store = JsonSnapshotStore(store_dir / "snapshots")
        else:
            run_store = InMemoryRunStore()
            ledger_store = InMemoryLedgerStore()
            self._snapshot_store = InMemorySnapshotStore()

        self._store_dir = store_dir

        # Load saved config BEFORE creating agent (so agent gets correct values)
        self._config_file: Optional[Path] = None
        if self._state_file:
            self._config_file = Path(self._state_file).with_suffix(".config.json")
            self._load_config()

        # Tool execution: passthrough by default so we can gate by approval in the CLI.
        tool_executor = PassthroughToolExecutor(mode="approval_required")
        self._tool_runner = MappingToolExecutor.from_tools(self._tools)

        # Create LLM client for capability queries (used by /max-tokens -1)
        self._llm_client = LocalAbstractCoreLLMClient(provider=self._provider, model=self._model)

        self._runtime = create_local_runtime(
            provider=self._provider,
            model=self._model,
            run_store=run_store,
            ledger_store=ledger_store,
            tool_executor=tool_executor,
        )
        # Artifact storage is the durability-safe place for large payloads (including archived memory spans).
        if self._store_dir is not None:
            self._artifact_store = FileArtifactStore(self._store_dir)
        else:
            self._artifact_store = InMemoryArtifactStore()
        self._runtime.set_artifact_store(self._artifact_store)

        self._agent = agent_cls(
            runtime=self._runtime,
            tools=self._tools,
            on_step=self._on_step,
            max_iterations=self._max_iterations,
            max_tokens=self._max_tokens,
        )

        # Session-level tool approval (persists across all requests)
        self._approve_all_session = False

        # Output buffer for full-screen mode
        self._output_lines: List[str] = []

        # Initialize full-screen UI with scrollable history
        self._ui = FullScreenUI(
            get_status_text=self._get_status_text,
            on_input=self._handle_input,
            color=self._color,
        )

        # Keep simple session for tool approvals (runs within full-screen)
        self._simple_session = create_simple_session(color=self._color)

        # Pending input for the run loop
        self._pending_input: Optional[str] = None

    # ---------------------------------------------------------------------
    # UI helpers
    # ---------------------------------------------------------------------

    def _safe_get_state(self):
        """Safely get agent state, returning None if unavailable.

        This handles the race condition where the render thread calls get_state()
        while the worker thread has completed/cleaned up a run. The runtime raises
        KeyError for unknown run_ids, which would crash the render loop.
        """
        try:
            return self._agent.get_state()
        except (KeyError, Exception):
            # Run doesn't exist (completed/cleaned up) or other error
            return None

    def _get_status_text(self) -> str:
        """Generate status text for the status bar."""
        # Get current context usage (safe for render thread)
        state = self._safe_get_state()
        if state:
            messages = self._messages_from_state(state)
            tokens_used = sum(len(str(m.get("content", ""))) // 4 for m in messages)
        else:
            messages = list(self._agent.session_messages or [])
            tokens_used = sum(len(str(m.get("content", ""))) // 4 for m in messages)

        max_tokens = self._max_tokens or 32768
        pct = (tokens_used / max_tokens) * 100 if max_tokens > 0 else 0

        return (
            f"{self._provider} | {self._model} | "
            f"Context: {tokens_used:,}/{max_tokens:,} ({pct:.0f}%)"
        )

    def _print(self, text: str = "") -> None:
        """Append text to the UI output area."""
        self._output_lines.append(text)
        self._ui.append_output(text)

    def _handle_input(self, text: str) -> None:
        """Handle user input from the UI (called from worker thread)."""
        text = text.strip()
        if not text:
            return

        # Echo user input (styled so user prompts are easy to spot).
        if self._color:
            bg = "\033[48;5;238m"
            fg = "\033[38;5;255m"
            reset = "\033[0m"
            lines = text.splitlines() or [""]
            block = "\n".join([f"{bg}{fg}> {line}{reset}" for line in lines])
            self._print("\n" + block)
        else:
            self._print(f"\n> {text}")

        cmd = text.strip()

        if cmd.startswith("/"):
            should_exit = self._dispatch_command(cmd[1:].strip())
            if should_exit:
                self._ui.stop()
            return

        # Reserved words check (commands must be slash-prefixed)
        lower = cmd.lower()
        if lower in (
            "help",
            "tools",
            "status",
            "auto-accept",
            "auto_accept",
            "max-tokens",
            "max_tokens",
            "max-messages",
            "max_messages",
            "memory",
            "compact",
            "spans",
            "expand",
            "recall",
            "history",
            "resume",
            "quit",
            "exit",
            "q",
            "task",
            "clear",
            "reset",
            "new",
            "snapshot",
        ):
            self._print(_style("Commands must start with '/'.", _C.DIM, enabled=self._color))
            self._print(_style(f"Try: /{lower}", _C.DIM, enabled=self._color))
            return

        # Otherwise treat as a task
        self._start(cmd)

    def _simple_prompt(self, message: str) -> str:
        """Single-line prompt for tool approvals (blocks worker thread).

        This uses blocking_prompt which queues a response and waits for user input.
        """
        result = self._ui.blocking_prompt(message)
        if result:
            self._print(f"  → {result}")
        return result.strip()

    def _banner(self) -> None:
        self._print(_style("AbstractCode (MVP)", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        self._print(f"Provider: {self._provider}   Model: {self._model}")
        if self._state_file:
            store = str(self._store_dir) + "/" if self._store_dir else "(unknown)"
            self._print(f"State:    {self._state_file} (store: {store})")
        else:
            self._print("State:    (in-memory; cannot resume after quitting)")
        mode = "auto-approve" if self._auto_approve else "approval-gated"
        self._print(f"Tools:    {len(self._tools)} ({mode})")
        self._print(_style("Type '/help' for commands.", _C.DIM, enabled=self._color))

    def _on_step(self, step: str, data: Dict[str, Any]) -> None:
        if step == "init":
            self._ui.set_spinner("Initializing...")
        elif step == "reason":
            it = data.get("iteration", "?")
            max_it = data.get("max_iterations", "?")
            self._print(_style(f"Thinking (step {it}/{max_it})...", _C.YELLOW, enabled=self._color))
            self._ui.set_spinner(f"Thinking (step {it}/{max_it})...")
        elif step == "act":
            tool = data.get("tool", "unknown")
            args = data.get("args") or {}
            args_str = json.dumps(args, ensure_ascii=False)
            if len(args_str) > 100:
                args_str = args_str[:97] + "..."
            self._print(_style("Tool:", _C.GREEN, enabled=self._color) + f" {tool}({args_str})")
            self._ui.set_spinner(f"Running {tool}...")
        elif step == "observe":
            res = str(data.get("result", ""))[:120]
            self._print(_style("Result:", _C.DIM, enabled=self._color) + f" {res}")
            self._ui.set_spinner("Processing result...")
        elif step == "ask_user":
            self._ui.clear_spinner()
            self._ui.scroll_to_bottom()
            self._print(_style("Agent question:", _C.MAGENTA, _C.BOLD, enabled=self._color))
        elif step == "done":
            self._ui.clear_spinner()
            self._ui.scroll_to_bottom()
            self._print(_style("\nANSWER", _C.GREEN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 60, _C.DIM, enabled=self._color))
            self._print(str(data.get("answer", "")))
            self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        elif step == "error" or step == "failed":
            self._ui.clear_spinner()
            self._ui.scroll_to_bottom()
        elif step == "max_iterations":
            self._ui.clear_spinner()
            self._ui.scroll_to_bottom()

    # ---------------------------------------------------------------------
    # Commands
    # ---------------------------------------------------------------------

    def run(self) -> None:
        # Build initial banner text
        banner_lines = []
        banner_lines.append(_style("AbstractCode (MVP)", _C.CYAN, _C.BOLD, enabled=self._color))
        banner_lines.append(_style("─" * 60, _C.DIM, enabled=self._color))
        banner_lines.append(f"Provider: {self._provider}   Model: {self._model}")
        if self._state_file:
            store = str(self._store_dir) + "/" if self._store_dir else "(unknown)"
            banner_lines.append(f"State:    {self._state_file} (store: {store})")
        else:
            banner_lines.append("State:    (in-memory; cannot resume after quitting)")
        mode = "auto-approve" if self._auto_approve else "approval-gated"
        banner_lines.append(f"Tools:    {len(self._tools)} ({mode})")
        banner_lines.append(_style("Type '/help' for commands.", _C.DIM, enabled=self._color))
        banner_lines.append("")

        # Add tools list to banner
        banner_lines.append(_style("Available tools", _C.CYAN, _C.BOLD, enabled=self._color))
        banner_lines.append(_style("─" * 60, _C.DIM, enabled=self._color))
        for name, spec in sorted(self._tool_specs.items()):
            params = ", ".join(sorted((spec.parameters or {}).keys()))
            banner_lines.append(f"- {name}({params})")
        banner_lines.append(_style("─" * 60, _C.DIM, enabled=self._color))

        if self._state_file:
            self._try_load_state()

        # Run the UI loop - this stays in full-screen mode continuously.
        # All input is handled by _handle_input() via the worker thread.
        self._ui.run_loop(banner="\n".join(banner_lines))

    def _dispatch_command(self, raw: str) -> bool:
        if not raw:
            return False

        parts = raw.split(None, 1)
        command = parts[0].lower()
        arg = parts[1] if len(parts) > 1 else ""

        if command in ("quit", "exit", "q"):
            return True
        if command in ("help", "h", "?"):
            self._show_help()
            return False
        if command == "tools":
            self._show_tools()
            return False
        if command == "status":
            self._show_status()
            return False
        if command in ("auto-accept", "auto_accept"):
            self._set_auto_accept(arg)
            return False
        if command == "resume":
            self._resume()
            return False
        if command == "history":
            limit = 12
            if arg:
                try:
                    limit = int(arg)
                except ValueError:
                    self._print(_style("Usage: /history [N]", _C.DIM, enabled=self._color))
                    return False
            self._show_history(limit=limit)
            return False
        if command == "task":
            task = arg.strip()
            if not task:
                self._print(_style("Usage: /task <your task>", _C.DIM, enabled=self._color))
                return False
            self._start(task)
            return False
        if command in ("clear", "reset", "new"):
            self._clear_memory()
            return False
        if command == "snapshot":
            self._handle_snapshot(arg)
            return False
        if command == "max-tokens":
            self._handle_max_tokens(arg)
            return False
        if command in ("max-messages", "max_messages"):
            self._handle_max_messages(arg)
            return False
        if command == "memory":
            self._handle_memory()
            return False
        if command == "compact":
            self._handle_compact(arg)
            return False
        if command == "spans":
            self._handle_spans()
            return False
        if command == "expand":
            self._handle_expand(arg)
            return False
        if command == "remember":
            self._handle_remember(arg)
            return False
        if command == "recall":
            self._handle_recall(arg)
            return False
        if command in ("vars", "var"):
            self._handle_vars(arg)
            return False
        if command == "flow":
            self._handle_flow(arg)
            return False

        self._print(_style(f"Unknown command: /{command}", _C.YELLOW, enabled=self._color))
        self._print(_style("Type /help for commands.", _C.DIM, enabled=self._color))
        return False

    def _append_to_active_context(self, *, role: str, content: str, metadata: Optional[Dict[str, Any]] = None) -> None:
        """Append a message to the active context view (durably when a run is loaded)."""
        import uuid

        msg: Dict[str, Any] = {
            "role": str(role or "assistant"),
            "content": str(content or ""),
            "timestamp": _now_iso(),
        }
        meta = dict(metadata or {})
        if "message_id" not in meta:
            meta["message_id"] = f"msg_{uuid.uuid4().hex}"
        msg["metadata"] = meta

        state = self._safe_get_state()
        if state is None or not hasattr(state, "vars"):
            self._agent.session_messages = list(self._agent.session_messages or []) + [msg]
            return

        messages = self._messages_from_state(state)
        messages.append(msg)

        self._agent.session_messages = list(messages)
        ctx = state.vars.get("context")
        if isinstance(ctx, dict):
            ctx["messages"] = messages
        if isinstance(getattr(state, "output", None), dict):
            state.output["messages"] = messages
        self._runtime.run_store.save(state)

    def _handle_flow(self, raw: str) -> None:
        """Run/resume/pause/cancel an AbstractFlow VisualFlow from inside the REPL.

        Examples:
          /flow run deep-research-pro --query "who are you?" --max_web_search 10 --follow_up_questions true
          /flow resume
          /flow pause
          /flow resume-run
          /flow cancel
        """
        import shlex

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        if not parts:
            self._print(_style("Usage:", _C.DIM, enabled=self._color))
            self._print(_style("  /flow run <flow_id_or_path> [--key value ...]", _C.DIM, enabled=self._color))
            self._print(_style("  /flow resume|pause|resume-run|cancel", _C.DIM, enabled=self._color))
            return

        action = parts[0].strip().lower()

        from .flow_cli import control_flow_command, resume_flow_command, run_flow_command

        def _emit_answer_user(message: str) -> None:
            self._print(message)
            self._append_to_active_context(
                role="assistant",
                content=message,
                metadata={"kind": "flow_output"},
            )

        if action == "run":
            if len(parts) < 2:
                self._print(_style("Usage: /flow run <flow_id_or_path> [--key value ...]", _C.DIM, enabled=self._color))
                return

            flow_ref = parts[1]
            rest = parts[2:]

            flows_dir: Optional[str] = None
            input_json: Optional[str] = None
            input_file: Optional[str] = None
            params: List[str] = []
            extra_args: List[str] = []
            wait_until = False
            auto_approve = bool(self._auto_approve)
            no_state = self._state_file is None
            flow_state_file: Optional[str] = None

            i = 0
            while i < len(rest):
                token = rest[i]

                def _opt_value() -> Optional[str]:
                    if "=" in token:
                        return token.split("=", 1)[1]
                    if i + 1 < len(rest):
                        return rest[i + 1]
                    return None

                if token.startswith("--flows-dir"):
                    val = _opt_value()
                    if val is None:
                        self._print(_style("Missing value for --flows-dir", _C.YELLOW, enabled=self._color))
                        return
                    flows_dir = val
                    i += 2 if "=" not in token else 1
                    continue

                if token.startswith("--input-json"):
                    val = _opt_value()
                    if val is None:
                        self._print(_style("Missing value for --input-json", _C.YELLOW, enabled=self._color))
                        return
                    input_json = val
                    i += 2 if "=" not in token else 1
                    continue

                if token.startswith("--input-file") or token.startswith("--input-json-file"):
                    val = _opt_value()
                    if val is None:
                        self._print(_style("Missing value for --input-file", _C.YELLOW, enabled=self._color))
                        return
                    input_file = val
                    i += 2 if "=" not in token else 1
                    continue

                if token == "--param" or token.startswith("--param="):
                    val = _opt_value()
                    if val is None:
                        self._print(_style("Missing value for --param (expected key=value)", _C.YELLOW, enabled=self._color))
                        return
                    params.append(val)
                    i += 2 if "=" not in token else 1
                    continue

                if token == "--wait-until":
                    wait_until = True
                    i += 1
                    continue

                if token in ("--auto-approve", "--auto-accept", "--accept-tools"):
                    auto_approve = True
                    i += 1
                    continue

                if token == "--no-state":
                    no_state = True
                    i += 1
                    continue

                if token.startswith("--flow-state-file"):
                    val = _opt_value()
                    if val is None:
                        self._print(_style("Missing value for --flow-state-file", _C.YELLOW, enabled=self._color))
                        return
                    flow_state_file = val
                    i += 2 if "=" not in token else 1
                    continue

                # Unrecognized: treat as dynamic input param (e.g. --query "..." or key=value).
                extra_args.append(token)
                i += 1

            try:
                run_flow_command(
                    flow_ref=str(flow_ref),
                    flows_dir=flows_dir,
                    input_json=input_json,
                    input_file=input_file,
                    params=params,
                    extra_args=extra_args,
                    flow_state_file=flow_state_file,
                    no_state=bool(no_state),
                    auto_approve=bool(auto_approve),
                    wait_until=bool(wait_until),
                    print_fn=self._print,
                    prompt_fn=self._simple_prompt,
                    ask_user_fn=self._prompt_user,
                    on_answer_user=_emit_answer_user,
                )
            except Exception as e:
                self._print(_style(f"Flow run failed: {e}", _C.YELLOW, enabled=self._color))
            return

        if action == "resume":
            try:
                resume_flow_command(
                    flow_state_file=None,
                    no_state=False,
                    auto_approve=bool(self._auto_approve),
                    wait_until=False,
                    print_fn=self._print,
                    prompt_fn=self._simple_prompt,
                    ask_user_fn=self._prompt_user,
                    on_answer_user=_emit_answer_user,
                )
            except Exception as e:
                self._print(_style(f"Flow resume failed: {e}", _C.YELLOW, enabled=self._color))
            return

        if action in ("pause", "resume-run", "cancel"):
            mapping = {"pause": "pause", "resume-run": "resume", "cancel": "cancel"}
            try:
                control_flow_command(action=mapping[action], flow_state_file=None)
            except Exception as e:
                self._print(_style(f"Flow control failed: {e}", _C.YELLOW, enabled=self._color))
            return

        self._print(_style(f"Unknown /flow action: {action}", _C.YELLOW, enabled=self._color))
        self._print(_style("Usage: /flow run|resume|pause|resume-run|cancel ...", _C.DIM, enabled=self._color))

    def _set_auto_accept(self, raw: str) -> None:
        value = raw.strip().lower()
        if not value:
            self._auto_approve = not self._auto_approve
        elif value in ("on", "true", "1", "yes", "y"):
            self._auto_approve = True
        elif value in ("off", "false", "0", "no", "n"):
            self._auto_approve = False
        else:
            self._print(_style("Usage: /auto-accept [on|off]", _C.DIM, enabled=self._color))
            return

        status = "ON (no approval prompts)" if self._auto_approve else "OFF (approval-gated)"
        self._print(_style(f"Auto-accept is now {status}.", _C.DIM, enabled=self._color))
        self._save_config()

    def _handle_max_tokens(self, raw: str) -> None:
        """Show or set max tokens for context."""
        value = raw.strip()
        if not value:
            # Show current
            if self._max_tokens is None:
                self._print("Max tokens: (auto)")
            else:
                self._print(f"Max tokens: {self._max_tokens:,}")
            return

        try:
            tokens = int(value)
            if tokens == -1:
                # Auto-detect from model capabilities via abstractruntime's LLM client
                try:
                    capabilities = self._llm_client.get_model_capabilities()
                    detected = capabilities.get("max_tokens", 32768)
                    self._max_tokens = detected
                    self._reconfigure_agent()
                    self._print(_style(f"Max tokens auto-detected: {detected:,} (from model capabilities)", _C.GREEN, enabled=self._color))
                except Exception as e:
                    self._print(_style(f"Auto-detection failed: {e}. Using default 32768.", _C.YELLOW, enabled=self._color))
                    self._max_tokens = 32768
                    self._reconfigure_agent()
                return
            if tokens < 1024:
                self._print(_style("Max tokens must be -1 (auto) or >= 1024", _C.YELLOW, enabled=self._color))
                return
        except ValueError:
            self._print(_style("Usage: /max-tokens [number or -1 for auto]", _C.DIM, enabled=self._color))
            return

        self._max_tokens = tokens
        # Immediately reconfigure the agent's logic with new max_tokens
        self._reconfigure_agent()
        self._print(_style(f"Max tokens set to {tokens:,} (immediate effect)", _C.GREEN, enabled=self._color))

    def _reconfigure_agent(self) -> None:
        """Reconfigure the agent with updated settings (max_tokens, max_history_messages, etc.)."""
        # Update the logic layer's max_tokens if the agent has a logic attribute
        if hasattr(self._agent, "logic") and self._agent.logic is not None:
            self._agent.logic._max_tokens = self._max_tokens
            # Also update max_history_messages on the logic layer
            if hasattr(self, "_max_history_messages"):
                self._agent.logic._max_history_messages = self._max_history_messages
        # Also update the agent's stored max_tokens
        if hasattr(self._agent, "_max_tokens"):
            self._agent._max_tokens = self._max_tokens
        # Also update the agent's stored max_history_messages
        if hasattr(self._agent, "_max_history_messages") and hasattr(self, "_max_history_messages"):
            self._agent._max_history_messages = self._max_history_messages
        # Save configuration to persist across restarts
        self._save_config()

    def _load_config(self) -> None:
        """Load configuration from file.

        Called during __init__ before agent is created, so it just sets
        instance variables. The agent will be created with these values.
        """
        if not self._config_file or not self._config_file.exists():
            return
        try:
            config = json.loads(self._config_file.read_text())
            # Apply saved settings to instance variables
            if "max_tokens" in config and config["max_tokens"] is not None:
                self._max_tokens = config["max_tokens"]
            if "max_history_messages" in config:
                self._max_history_messages = config["max_history_messages"]
            if "auto_approve" in config:
                self._auto_approve = config["auto_approve"]
        except Exception:
            pass  # Ignore corrupt config files

    def _save_config(self) -> None:
        """Save configuration to file."""
        if not self._config_file:
            return
        try:
            config = {
                "max_tokens": self._max_tokens,
                "max_history_messages": getattr(self, "_max_history_messages", -1),
                "auto_approve": self._auto_approve,
            }
            self._config_file.write_text(json.dumps(config, indent=2))
        except Exception:
            pass  # Silently fail if we can't write

    def _handle_max_messages(self, raw: str) -> None:
        """Show or set max history messages."""
        value = raw.strip()
        if not value:
            # Show current
            if hasattr(self._agent, "_max_history_messages"):
                current = self._agent._max_history_messages
            elif hasattr(self._agent, "logic") and self._agent.logic is not None:
                current = self._agent.logic._max_history_messages
            else:
                current = -1
            if current == -1:
                self._print("Max history messages: -1 (unlimited, uses full history)")
            else:
                self._print(f"Max history messages: {current}")
            return

        try:
            num = int(value)
            if num < -1 or num == 0:
                self._print(_style("Must be -1 (unlimited) or >= 1", _C.YELLOW, enabled=self._color))
                return
        except ValueError:
            self._print(_style("Usage: /max-messages [number]", _C.DIM, enabled=self._color))
            return

        self._max_history_messages = num
        self._reconfigure_agent()
        label = "unlimited" if num == -1 else str(num)
        self._print(_style(f"Max history messages set to {label} (immediate effect)", _C.GREEN, enabled=self._color))

    def _handle_memory(self) -> None:
        """Show memory/token usage breakdown."""
        # Get current state and messages
        state = self._safe_get_state()
        if state is not None:
            messages = self._messages_from_state(state)
        else:
            messages = list(self._agent.session_messages or [])

        # Token estimation function (rough: 1 token ≈ 4 characters)
        def estimate_tokens(text: str) -> int:
            return len(text) // 4

        # System prompt estimation (agent builds inline, estimate ~500 tokens)
        system_tokens = 500

        # History messages
        history_text = ""
        for m in messages:
            history_text += f"{m.get('role', '')}: {m.get('content', '')}\n"
        history_tokens = estimate_tokens(history_text)

        # Tool definitions (schemas are verbose, multiply by ~10)
        tool_names_text = json.dumps([name for name in self._tool_specs.keys()])
        tool_tokens = estimate_tokens(tool_names_text) * 10

        total_used = system_tokens + history_tokens + tool_tokens
        max_tokens = self._max_tokens or 32768

        self._print(_style("\nMemory Usage", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 40, _C.DIM, enabled=self._color))
        self._print(f"Max tokens:        {max_tokens:,}")
        self._print(f"Estimated used:    ~{total_used:,}")
        self._print(f"Available:         ~{max(0, max_tokens - total_used):,}")
        self._print()
        self._print("Breakdown (estimated):")
        self._print(f"  System/prompt:   ~{system_tokens:,}")
        self._print(f"  Tool schemas:    ~{tool_tokens:,}")
        self._print(f"  History ({len(messages)} msgs): ~{history_tokens:,}")

        # Archived spans (stored, not in active context)
        archived_spans = 0
        archived_msgs = 0
        if state is not None and hasattr(state, "vars"):
            runtime_ns = state.vars.get("_runtime")
            spans = runtime_ns.get("memory_spans") if isinstance(runtime_ns, dict) else None
            if isinstance(spans, list):
                archived_spans = len(spans)
                for s in spans:
                    if not isinstance(s, dict):
                        continue
                    try:
                        archived_msgs += int(s.get("message_count") or 0)
                    except Exception:
                        continue

        if archived_spans:
            self._print(f"  Archived spans:  {archived_spans} ({archived_msgs} msgs)  [stored in ArtifactStore]")
            self._print(f"  Stored total:    {len(messages) + archived_msgs} msgs (active + archived)")
        self._print(_style("─" * 40, _C.DIM, enabled=self._color))

    def _handle_compact(self, raw: str) -> Optional[Dict[str, Any]]:
        """Handle /compact command for conversation compression.

        Syntax: /compact [light|standard|heavy] [--preserve N] [focus topics...]

        Examples:
            /compact                     # Standard mode, 6 preserved, auto-focus
            /compact light               # Light compression
            /compact heavy --preserve 4  # Heavy compression, keep 4 messages
            /compact standard API design # Focus on "API design" topics
        """
        import shlex

        # Parse arguments
        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split()

        # Defaults
        compression_mode = "standard"
        preserve_recent = 6
        focus_topics = []

        # Parse arguments
        i = 0
        while i < len(parts):
            part = parts[i].lower()
            if part == "--preserve":
                if i + 1 < len(parts):
                    try:
                        preserve_recent = int(parts[i + 1])
                        if preserve_recent < 0:
                            self._print(_style("--preserve must be >= 0", _C.YELLOW, enabled=self._color))
                            return
                        i += 2
                        continue
                    except ValueError:
                        self._print(_style("--preserve requires a number", _C.YELLOW, enabled=self._color))
                        return
                else:
                    self._print(_style("--preserve requires a number", _C.YELLOW, enabled=self._color))
                    return

            if part in ("light", "standard", "heavy"):
                compression_mode = part
                i += 1
                continue

            # Remaining args are focus topics
            focus_topics.extend(parts[i:])
            break

        # Build focus string
        focus = " ".join(focus_topics) if focus_topics else None

        state = self._safe_get_state()
        if state is not None and state.status == self._RunStatus.RUNNING:
            self._print(_style("Cannot compact while a run is actively running.", _C.YELLOW, enabled=self._color))
            self._print(_style("Interrupt first, or compact between tasks.", _C.DIM, enabled=self._color))
            return

        # Get current messages (active context view)
        if state is not None:
            messages = self._messages_from_state(state)
        else:
            messages = list(self._agent.session_messages or [])
        if not messages:
            self._print(_style("No messages to compact.", _C.YELLOW, enabled=self._color))
            return

        # Ensure message metadata has stable IDs for provenance.
        import uuid
        def now_iso() -> str:
            return _now_iso()

        for m in messages:
            if not isinstance(m, dict):
                continue
            meta = m.get("metadata")
            if not isinstance(meta, dict):
                meta = {}
                m["metadata"] = meta
            meta.setdefault("message_id", f"msg_{uuid.uuid4().hex}")
            if "timestamp" not in m or not m.get("timestamp"):
                m["timestamp"] = now_iso()

        # Check if we have enough messages to warrant compaction
        non_system = [m for m in messages if m.get("role") != "system"]
        if len(non_system) <= preserve_recent:
            self._print(_style(
                f"Only {len(non_system)} non-system messages - nothing to compact (preserving {preserve_recent}).",
                _C.DIM, enabled=self._color
            ))
            return

        # Show what we're doing
        self._print(_style("\nCompacting conversation...", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 40, _C.DIM, enabled=self._color))
        self._print(f"Mode:           {compression_mode}")
        self._print(f"Preserve:       {preserve_recent} recent messages")
        self._print(f"Focus:          {focus or '(auto-detect)'}")
        self._print(f"Total messages: {len(messages)}")
        self._print(_style("─" * 40, _C.DIM, enabled=self._color))

        self._ui.set_spinner("Compacting...")

        try:
            # Runtime-owned compaction (ledgered + provenance-preserving).
            from abstractruntime import Effect, EffectType, StepPlan, WorkflowSpec
            from abstractruntime.core.models import RunStatus

            if state is None:
                raise RuntimeError("No run loaded. Start a task or /resume before /compact.")

            target_run_id = getattr(state, "run_id", None)
            if not isinstance(target_run_id, str) or not target_run_id:
                raise RuntimeError("No run_id available for compaction.")

            payload: Dict[str, Any] = {
                "target_run_id": target_run_id,
                "preserve_recent": int(preserve_recent),
                "compression_mode": compression_mode,
                "tool_name": "compact_memory",
                "call_id": "compact",
            }
            if focus:
                payload["focus"] = focus

            def compact_node(run, ctx) -> StepPlan:
                return StepPlan(
                    node_id="compact",
                    effect=Effect(
                        type=EffectType.MEMORY_COMPACT,
                        payload=payload,
                        result_key="_temp.compact",
                    ),
                    next_node="done",
                )

            def done_node(run, ctx) -> StepPlan:
                temp = run.vars.get("_temp")
                if not isinstance(temp, dict):
                    temp = {}
                return StepPlan(node_id="done", complete_output={"result": temp.get("compact")})

            wf = WorkflowSpec(
                workflow_id="abstractcode_compact_command",
                entry_node="compact",
                nodes={"compact": compact_node, "done": done_node},
            )

            comp_run_id = self._runtime.start(
                workflow=wf,
                vars={"context": {}, "scratchpad": {}, "_runtime": {}, "_temp": {}, "_limits": {}},
                actor_id=getattr(state, "actor_id", None),
                session_id=getattr(state, "session_id", None),
                parent_run_id=target_run_id,
            )

            comp_state = self._runtime.tick(workflow=wf, run_id=comp_run_id)
            if comp_state.status != RunStatus.COMPLETED:
                raise RuntimeError(comp_state.error or "Compaction failed")

            compact_result = (comp_state.output or {}).get("result") or {}
            result_list = compact_result.get("results") if isinstance(compact_result, dict) else None
            first = result_list[0] if isinstance(result_list, list) and result_list else {}
            meta_out = first.get("meta") if isinstance(first, dict) else None
            meta_out = dict(meta_out) if isinstance(meta_out, dict) else {}

            # Reload the target run to get the updated active context.
            updated = self._runtime.run_store.load(target_run_id)
            if updated is None:
                raise RuntimeError("Could not reload run after compaction")

            new_messages = self._messages_from_state(updated)

            # Replace active context view in the agent (host-side mirror).
            self._agent.session_messages = list(new_messages)
            state = updated

            # Calculate stats
            old_tokens = sum(len(str(m.get("content", ""))) // 4 for m in messages)
            new_tokens = sum(len(str(m.get("content", ""))) // 4 for m in new_messages)
            reduction = ((old_tokens - new_tokens) / old_tokens * 100) if old_tokens > 0 else 0

            self._ui.clear_spinner()

            self._print(_style("\n✅ Compaction complete!", _C.GREEN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 40, _C.DIM, enabled=self._color))
            self._print(f"Messages:   {len(messages)} → {len(new_messages)}")
            self._print(f"Tokens:     ~{old_tokens:,} → ~{new_tokens:,} ({reduction:.0f}% reduction)")
            conf = meta_out.get("confidence")
            if isinstance(conf, (int, float)):
                self._print(f"Confidence: {float(conf):.0%}")
            self._print(_style("─" * 40, _C.DIM, enabled=self._color))

            # Show key points
            key_points = meta_out.get("key_points") if isinstance(meta_out, dict) else None
            if isinstance(key_points, list) and key_points:
                self._print(_style("\nKey points preserved:", _C.CYAN, enabled=self._color))
                for point in [str(p) for p in key_points[:5]]:
                    truncated = point[:80] + "..." if len(point) > 80 else point
                    self._print(f"  • {truncated}")
            return {"ok": True, "comp_run_id": comp_run_id, "target_run_id": target_run_id, "meta": meta_out}
        except Exception as e:
            self._ui.clear_spinner()
            self._print(_style(f"Compaction failed: {e}", _C.RED, enabled=self._color))
            return {"ok": False, "error": str(e)}

    def _handle_spans(self) -> None:
        """List archived conversation spans (stored in ArtifactStore)."""
        state = self._safe_get_state()
        if state is None or not hasattr(state, "vars"):
            self._print(_style("No run loaded. Use /resume or start a task first.", _C.DIM, enabled=self._color))
            return

        runtime_ns = state.vars.get("_runtime")
        spans = runtime_ns.get("memory_spans") if isinstance(runtime_ns, dict) else None
        if not isinstance(spans, list) or not spans:
            self._print(_style("No archived spans. Use /compact first.", _C.DIM, enabled=self._color))
            return

        self._print(_style("\nArchived spans", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        for i, s in enumerate(spans, start=1):
            if not isinstance(s, dict):
                continue
            artifact_id = str(s.get("artifact_id") or "")
            count = s.get("message_count") or 0
            created = str(s.get("created_at") or "")
            mode = str(s.get("compression_mode") or "")
            focus = s.get("focus")
            focus_text = f" | focus={focus}" if focus else ""
            self._print(f"[{i}] {artifact_id} | msgs={count} | {created} | mode={mode}{focus_text}")
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))

    def _handle_expand(self, raw: str) -> None:
        """Expand (rehydrate) an archived span.

        Usage:
          /expand <index|artifact_id> [--show] [--into-context]
        """
        import shlex

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        if not parts:
            self._print(_style("Usage: /expand <index|artifact_id> [--show] [--into-context]", _C.DIM, enabled=self._color))
            self._print(_style("Tip: use /spans to list archived spans.", _C.DIM, enabled=self._color))
            return

        selector: Optional[str] = None
        show = True
        into_context = False

        for p in parts:
            if p == "--show":
                show = True
                continue
            if p == "--into-context":
                into_context = True
                continue
            if p.startswith("--"):
                continue
            if selector is None:
                selector = p

        if selector is None:
            self._print(_style("Usage: /expand <index|artifact_id> [--show] [--into-context]", _C.DIM, enabled=self._color))
            return

        state = self._safe_get_state()
        if state is None or not hasattr(state, "vars"):
            self._print(_style("No run loaded. Use /resume or start a task first.", _C.DIM, enabled=self._color))
            return

        runtime_ns = state.vars.get("_runtime")
        spans = runtime_ns.get("memory_spans") if isinstance(runtime_ns, dict) else None
        if not isinstance(spans, list) or not spans:
            self._print(_style("No archived spans. Use /compact first.", _C.DIM, enabled=self._color))
            return

        span: Optional[Dict[str, Any]] = None
        if selector.isdigit():
            idx = int(selector) - 1
            if 0 <= idx < len(spans) and isinstance(spans[idx], dict):
                span = spans[idx]
        else:
            for s in spans:
                if isinstance(s, dict) and s.get("artifact_id") == selector:
                    span = s
                    break

        if not span:
            self._print(_style(f"Span not found: {selector}", _C.YELLOW, enabled=self._color))
            self._print(_style("Tip: use /spans to list archived spans.", _C.DIM, enabled=self._color))
            return

        artifact_id = str(span.get("artifact_id") or "")
        if not artifact_id:
            self._print(_style("Span is missing artifact_id.", _C.YELLOW, enabled=self._color))
            return

        payload = self._artifact_store.load_json(artifact_id)
        if not isinstance(payload, dict):
            self._print(_style(f"Artifact not found or invalid JSON: {artifact_id}", _C.YELLOW, enabled=self._color))
            return

        archived = payload.get("messages")
        if not isinstance(archived, list):
            self._print(_style(f"Artifact payload missing messages list: {artifact_id}", _C.YELLOW, enabled=self._color))
            return

        archived_messages = [m for m in archived if isinstance(m, dict)]

        if show:
            self._print(_style("\nExpanded span (read-only)", _C.CYAN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 60, _C.DIM, enabled=self._color))
            self._print(f"Artifact:  {artifact_id}")
            self._print(f"Messages:  {len(archived_messages)}")
            self._print(_style("─" * 60, _C.DIM, enabled=self._color))

            for m in archived_messages:
                role = str(m.get("role") or "unknown")
                content = str(m.get("content") or "")
                self._print(_style(f"{role}:", _C.BOLD, enabled=self._color))
                self._print(content)

        if not into_context:
            return

        active = self._messages_from_state(state)
        new_messages, inserted, skipped = _insert_archived_span(
            active_messages=active,
            archived_messages=archived_messages,
            artifact_id=artifact_id,
        )

        self._agent.session_messages = new_messages
        ctx = state.vars.get("context")
        if isinstance(ctx, dict):
            ctx["messages"] = new_messages
        if isinstance(getattr(state, "output", None), dict):
            state.output["messages"] = new_messages
        self._runtime.run_store.save(state)

        self._print(_style("\n✅ Span expanded into active context.", _C.GREEN, enabled=self._color))
        self._print(_style(f"Inserted: {inserted} messages (skipped {skipped} duplicates).", _C.DIM, enabled=self._color))

    def _handle_recall(self, raw: str) -> None:
        """Recall archived memory by time range / tags / keyword.

        Usage:
          /recall [--since ISO] [--until ISO] [--tag k=v] [--q text] [--limit N] [--show] [--into-context]
                 [--placement after_summary|after_system|end]
        """
        from .recall import execute_recall, parse_recall_args

        state = self._safe_get_state()
        if state is None or not hasattr(state, "run_id"):
            self._print(_style("No run loaded. Use /resume or start a task first.", _C.DIM, enabled=self._color))
            return

        try:
            req = parse_recall_args(raw)
        except Exception as e:
            self._print(_style(f"Recall parse error: {e}", _C.YELLOW, enabled=self._color))
            self._print(
                _style(
                    "Usage: /recall [--since ISO] [--until ISO] [--tag k=v] [--q text] [--limit N] [--show] [--into-context]",
                    _C.DIM,
                    enabled=self._color,
                )
            )
            return

        try:
            res = execute_recall(
                run_id=str(state.run_id),
                run_store=self._runtime.run_store,
                artifact_store=self._artifact_store,
                request=req,
            )
        except Exception as e:
            self._print(_style(f"Recall failed: {e}", _C.YELLOW, enabled=self._color))
            return

        matches = res.get("matches") if isinstance(res, dict) else None
        matches = matches if isinstance(matches, list) else []
        if not matches:
            self._print(_style("No matching memories.", _C.DIM, enabled=self._color))
            return

        self._print(_style("\nRecall matches", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))
        self._print(
            _style(
                f"Filters: since={req.since or '-'} until={req.until or '-'} tags={len(req.tags)} query={req.query or '-'}",
                _C.DIM,
                enabled=self._color,
            )
        )
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))

        for i, s in enumerate(matches, start=1):
            if not isinstance(s, dict):
                continue
            kind = str(s.get("kind") or "span")
            artifact_id = str(s.get("artifact_id") or "")
            created = str(s.get("created_at") or "")
            count = s.get("message_count")
            tags = s.get("tags") if isinstance(s.get("tags"), dict) else {}
            tags_txt = ", ".join([f"{k}={v}" for k, v in sorted(tags.items()) if isinstance(v, str) and v])

            extra = ""
            if kind == "conversation_span":
                mode = str(s.get("compression_mode") or "")
                focus = s.get("focus")
                focus_txt = f" focus={focus}" if isinstance(focus, str) and focus else ""
                extra = f" msgs={count or 0} mode={mode}{focus_txt}"
            elif kind == "memory_note":
                preview = str(s.get("note_preview") or "")
                if preview:
                    extra = f" note={preview}"

            line = f"[{i}] {artifact_id} kind={kind} created_at={created}{extra}"
            self._print(line)
            if tags_txt:
                self._print(_style(f"     tags: {tags_txt}", _C.DIM, enabled=self._color))

        self._print(_style("─" * 80, _C.DIM, enabled=self._color))

        if req.show:
            for s in matches:
                if not isinstance(s, dict):
                    continue
                if str(s.get("kind") or "") != "memory_note":
                    continue
                artifact_id = str(s.get("artifact_id") or "")
                if not artifact_id:
                    continue
                payload = self._artifact_store.load_json(artifact_id)
                if not isinstance(payload, dict):
                    continue
                note = str(payload.get("note") or "").strip()
                sources = payload.get("sources")
                self._print(_style("\nNote", _C.MAGENTA, _C.BOLD, enabled=self._color))
                self._print(_style("─" * 80, _C.DIM, enabled=self._color))
                self._print(f"span_id={artifact_id}")
                if note:
                    self._print(note)
                if isinstance(sources, dict):
                    self._print(_style("Sources:", _C.DIM, enabled=self._color))
                    self._print(_style(json.dumps(sources, ensure_ascii=False, indent=2), _C.DIM, enabled=self._color))

        rehydration = res.get("rehydration") if isinstance(res, dict) else None
        if isinstance(rehydration, dict) and req.into_context:
            inserted = int(rehydration.get("inserted") or 0)
            skipped = int(rehydration.get("skipped") or 0)
            self._print(_style("\n✅ Rehydrated into active context.", _C.GREEN, enabled=self._color))
            self._print(_style(f"Inserted: {inserted} messages (skipped {skipped} duplicates).", _C.DIM, enabled=self._color))

            updated = self._runtime.run_store.load(str(state.run_id))
            if updated is not None:
                self._agent.session_messages = self._messages_from_state(updated)

    def _handle_vars(self, raw: str) -> None:
        """Inspect durable run variables (especially scratchpad).

        Usage:
          /vars [path] [--keys]

        Examples:
          /vars
          /vars scratchpad
          /vars scratchpad --keys
          /vars scratchpad.some_list[0]
        """
        import json
        import shlex

        from abstractruntime.core.vars import ensure_namespaces, parse_vars_path, resolve_vars_path

        state = self._safe_get_state()
        if state is None or not hasattr(state, "vars"):
            self._print(_style("No run loaded. Use /resume or start a task first.", _C.DIM, enabled=self._color))
            return

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        path: Optional[str] = None
        keys_only = False

        for p in parts:
            if p in ("--keys", "--ls", "--keysonly", "--keys-only"):
                keys_only = True
                continue
            if p.startswith("--"):
                self._print(_style(f"Unknown flag: {p}", _C.YELLOW, enabled=self._color))
                self._print(_style("Usage: /vars [path] [--keys]", _C.DIM, enabled=self._color))
                return
            path = (p if path is None else f"{path} {p}").strip()

        ensure_namespaces(state.vars)

        if not path:
            canonical = ["context", "scratchpad", "_runtime", "_temp", "_limits"]
            keys = [k for k in canonical if k in state.vars]
            keys += sorted([k for k in state.vars.keys() if isinstance(k, str) and k not in set(keys)])
            self._print(_style("\nVars roots", _C.CYAN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 60, _C.DIM, enabled=self._color))
            self._print(json.dumps({"keys": keys}, ensure_ascii=False, indent=2, sort_keys=True))
            return

        try:
            tokens = parse_vars_path(path)
            value = resolve_vars_path(state.vars, tokens)
        except Exception as e:
            self._print(_style(f"Vars error: {e}", _C.YELLOW, enabled=self._color))
            return

        out: Dict[str, Any] = {"path": path, "type": type(value).__name__}
        if keys_only:
            if isinstance(value, dict):
                out["keys"] = sorted([str(k) for k in value.keys()])
            elif isinstance(value, list):
                out["length"] = len(value)
            else:
                out["value"] = value
        else:
            out["value"] = value

        self._print(_style("\nVars", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        self._print(json.dumps(out, ensure_ascii=False, indent=2, sort_keys=True, default=str))

    def _handle_remember(self, raw: str) -> None:
        """Store a durable memory note (runtime MEMORY_NOTE) with optional tags and provenance.

        Usage:
          /remember <note text> [--tag k=v ...] [--span <span_id>] [--last-span] [--last N]
        """
        from .remember import parse_remember_args, store_memory_note

        state = self._safe_get_state()
        if state is None or not hasattr(state, "run_id") or not hasattr(state, "vars"):
            self._print(_style("No run loaded. Use /resume or start a task first.", _C.DIM, enabled=self._color))
            return

        try:
            req = parse_remember_args(raw)
        except Exception as e:
            self._print(_style(f"Remember parse error: {e}", _C.YELLOW, enabled=self._color))
            self._print(
                _style(
                    "Usage: /remember <note text> [--tag k=v ...] [--span <span_id>] [--last-span] [--last N]",
                    _C.DIM,
                    enabled=self._color,
                )
            )
            return

        # Resolve provenance sources (best-effort).
        sources: Dict[str, Any] = {"run_id": str(state.run_id), "span_ids": [], "message_ids": []}

        if req.span_id:
            sources["span_ids"] = [req.span_id]
        elif req.last_span:
            runtime_ns = state.vars.get("_runtime") if isinstance(state.vars, dict) else None
            spans = runtime_ns.get("memory_spans") if isinstance(runtime_ns, dict) else None
            last: Optional[str] = None
            if isinstance(spans, list):
                for s in reversed(spans):
                    if not isinstance(s, dict):
                        continue
                    if str(s.get("kind") or "") != "conversation_span":
                        continue
                    aid = s.get("artifact_id")
                    if isinstance(aid, str) and aid:
                        last = aid
                        break
            if last:
                sources["span_ids"] = [last]
            else:
                self._print(_style("No conversation spans found (use /compact first or omit --last-span).", _C.DIM, enabled=self._color))
        else:
            # Attach the last N non-system message ids.
            last_n = int(req.last_messages or 0)
            if last_n > 0:
                messages = self._messages_from_state(state)
                ids: list[str] = []
                for m in reversed(messages):
                    if not isinstance(m, dict):
                        continue
                    if m.get("role") == "system":
                        continue
                    mid = _get_message_id(m)
                    if isinstance(mid, str) and mid:
                        ids.append(mid)
                    if len(ids) >= last_n:
                        break
                ids.reverse()
                sources["message_ids"] = ids

        try:
            result = store_memory_note(
                runtime=self._runtime,
                target_run_id=str(state.run_id),
                note=req.note,
                tags=req.tags,
                sources=sources,
                actor_id=getattr(state, "actor_id", None),
                session_id=getattr(state, "session_id", None),
                call_id="remember",
            )
        except Exception as e:
            self._print(_style(f"Remember failed: {e}", _C.YELLOW, enabled=self._color))
            return

        # Extract span_id if present.
        span_id = None
        meta = result.get("results") if isinstance(result, dict) else None
        if isinstance(meta, list) and meta:
            first = meta[0] if isinstance(meta[0], dict) else {}
            first_meta = first.get("meta") if isinstance(first, dict) else None
            if isinstance(first_meta, dict):
                span_id = first_meta.get("span_id")

        self._print(_style("\n✅ Remembered.", _C.GREEN, enabled=self._color))
        if isinstance(span_id, str) and span_id:
            self._print(_style(f"span_id={span_id}", _C.DIM, enabled=self._color))
        if req.tags:
            tags_txt = ", ".join([f"{k}={v}" for k, v in sorted(req.tags.items())])
            self._print(_style(f"tags: {tags_txt}", _C.DIM, enabled=self._color))

    def _show_help(self) -> None:
        self._print(
            "\nCommands:\n"
            "  /help               Show this message\n"
            "  /tools              List available tools\n"
            "  /status             Show current run status\n"
            "  /auto-accept        Toggle auto-accept for tools [saved]\n"
            "  /max-tokens [N]     Show or set max tokens (-1 = auto) [saved]\n"
            "  /max-messages [N]   Show or set max history messages (-1 = unlimited) [saved]\n"
            "  /memory             Show current token usage breakdown\n"
            "  /compact [mode]     Compress conversation context [light|standard|heavy]\n"
            "  /spans              List archived conversation spans (from /compact)\n"
            "  /expand <span>      Expand an archived span (--show, --into-context)\n"
            "  /recall [opts]      Recall spans by time/tags/query (--into-context)\n"
            "  /vars [path]        Inspect run vars (scratchpad, _runtime, ...)\n"
            "  /remember <note>    Store a durable memory note (tags + provenance)\n"
            "  /flow ...           Run AbstractFlow workflows (run/resume/pause/cancel)\n"
            "  /history [N]        Show recent conversation history\n"
            "  /resume             Resume the saved/attached run\n"
            "  /clear              Clear memory and start fresh (aliases: /reset, /new)\n"
            "  /snapshot save <n>  Save current state as named snapshot\n"
            "  /snapshot load <n>  Load snapshot by name\n"
            "  /snapshot list      List available snapshots\n"
            "  /quit               Exit\n"
            "\nTasks:\n"
            "  /task <text>        Start a new task\n"
        )

    def _show_tools(self) -> None:
        self._print(_style("\nAvailable tools", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        for name, spec in sorted(self._tool_specs.items()):
            params = ", ".join(sorted((spec.parameters or {}).keys()))
            self._print(f"- {name}({params})")
            self._print(_style(f"  {spec.description}", _C.DIM, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))

    def _show_status(self) -> None:
        state = self._safe_get_state()
        if state is None:
            self._print("No active run.")
            return

        self._print(_style("\nRun status", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 40, _C.DIM, enabled=self._color))
        self._print(f"Run ID:    {state.run_id}")
        self._print(f"Workflow:  {state.workflow_id}")
        self._print(f"Status:    {state.status.value}")
        self._print(f"Node:      {state.current_node}")
        if state.waiting:
            self._print(f"Waiting:   {state.waiting.reason.value}")
            if state.waiting.prompt:
                self._print(f"Prompt:    {state.waiting.prompt}")
        self._print(_style("─" * 40, _C.DIM, enabled=self._color))

    def _messages_from_state(self, state: Any) -> List[Dict[str, Any]]:
        context = state.vars.get("context") if hasattr(state, "vars") else None
        if isinstance(context, dict) and isinstance(context.get("messages"), list):
            return list(context["messages"])
        if hasattr(state, "vars") and isinstance(state.vars.get("messages"), list):
            return list(state.vars["messages"])
        if getattr(state, "output", None) and isinstance(state.output.get("messages"), list):
            return list(state.output["messages"])
        return []

    def _show_history(self, *, limit: int = 12) -> None:
        state = self._safe_get_state()
        if state is None:
            messages = list(self._agent.session_messages or [])
        else:
            messages = self._messages_from_state(state)
        if not messages:
            self._print("No history yet.")
            return

        # Interpret `limit` as number of user turns (prompt + subsequent messages), not raw messages.
        try:
            limit_int = int(limit)
        except Exception:
            limit_int = 12
        if limit_int < 1:
            limit_int = 1

        # Group messages into "turns" starting at each user message.
        turns: list[list[Dict[str, Any]]] = []
        current: list[Dict[str, Any]] = []
        prelude: list[Dict[str, Any]] = []
        for m in messages:
            if not isinstance(m, dict):
                continue
            role = m.get("role")
            if role == "user":
                if current:
                    turns.append(current)
                # Include leading system messages before the first user prompt.
                if not turns and prelude:
                    current = [*prelude, m]
                    prelude = []
                else:
                    current = [m]
                continue
            if not current:
                # Preserve only system messages before the first user message.
                if role == "system":
                    prelude.append(m)
                continue
            current.append(m)
        if current:
            turns.append(current)

        if not turns:
            self._print("No history yet.")
            return

        selected = turns[-limit_int:]

        self._print(_style(f"\nHistory (last {len(selected)} interaction(s))", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))

        for idx, turn in enumerate(selected, start=max(1, len(turns) - len(selected) + 1)):
            self._print(_style(f"\n# Turn {idx}", _C.DIM, enabled=self._color))
            self._print(_style("─" * 80, _C.DIM, enabled=self._color))
            for msg in turn:
                role = str(msg.get("role") or "unknown")
                content = "" if msg.get("content") is None else str(msg.get("content"))
                if role == "tool":
                    meta = msg.get("metadata") if isinstance(msg.get("metadata"), dict) else {}
                    name = meta.get("name") if isinstance(meta, dict) else None
                    label = f"tool[{name}]" if isinstance(name, str) and name else "tool"
                else:
                    label = role
                self._print(_style(f"{label}:", _C.BOLD, enabled=self._color))
                self._print(content)

        self._print(_style("\n" + "─" * 80, _C.DIM, enabled=self._color))

    def _clear_memory(self) -> None:
        """Clear all memory and reset to fresh state."""
        # Clear session messages
        self._agent.session_messages = []

        # Clear run ID so next task starts fresh
        self._agent._current_run_id = None

        # Reset approval state (clear = full reset)
        self._approve_all_session = False

        self._print(_style("Memory cleared. Ready for a fresh start.", _C.GREEN, enabled=self._color))

    def _handle_snapshot(self, arg: str) -> None:
        """Handle /snapshot save|load|list commands."""
        parts = arg.split(None, 1)
        if not parts:
            self._print(_style("Usage: /snapshot save <name>  |  /snapshot load <name>  |  /snapshot list", _C.DIM, enabled=self._color))
            return

        subcommand = parts[0].lower()
        name = parts[1].strip() if len(parts) > 1 else ""

        if subcommand == "save":
            self._snapshot_save(name)
        elif subcommand == "load":
            self._snapshot_load(name)
        elif subcommand == "list":
            self._snapshot_list()
        else:
            self._print(_style(f"Unknown snapshot command: {subcommand}", _C.YELLOW, enabled=self._color))
            self._print(_style("Usage: /snapshot save <name>  |  /snapshot load <name>  |  /snapshot list", _C.DIM, enabled=self._color))

    def _snapshot_save(self, name: str) -> None:
        """Save current state as a named snapshot."""
        if not name:
            self._print(_style("Usage: /snapshot save <name>", _C.DIM, enabled=self._color))
            return

        state = self._safe_get_state()
        if state is None:
            self._print(_style("No active run to snapshot.", _C.YELLOW, enabled=self._color))
            return

        snapshot = self._Snapshot.from_run(run=state, name=name)
        self._snapshot_store.save(snapshot)

        self._print(_style(f"Snapshot saved: {name}", _C.GREEN, enabled=self._color))
        self._print(_style(f"ID: {snapshot.snapshot_id}", _C.DIM, enabled=self._color))

    def _snapshot_load(self, name: str) -> None:
        """Load a snapshot by name."""
        if not name:
            self._print(_style("Usage: /snapshot load <name>", _C.DIM, enabled=self._color))
            return

        # Find snapshot by name
        snapshots = self._snapshot_store.list(query=name)
        if not snapshots:
            self._print(_style(f"No snapshot found matching: {name}", _C.YELLOW, enabled=self._color))
            return

        # Prefer exact match, otherwise use first result
        snapshot = next((s for s in snapshots if s.name.lower() == name.lower()), snapshots[0])

        # Restore run state
        run_state_dict = snapshot.run_state
        if not run_state_dict:
            self._print(_style("Snapshot has no run state.", _C.YELLOW, enabled=self._color))
            return

        # Restore messages to agent
        messages = run_state_dict.get("vars", {}).get("context", {}).get("messages", [])
        if messages:
            self._agent.session_messages = list(messages)

        self._print(_style(f"Snapshot loaded: {snapshot.name}", _C.GREEN, enabled=self._color))
        self._print(_style(f"ID: {snapshot.snapshot_id}", _C.DIM, enabled=self._color))
        if messages:
            self._print(_style(f"Restored {len(messages)} messages.", _C.DIM, enabled=self._color))

    def _snapshot_list(self) -> None:
        """List available snapshots."""
        snapshots = self._snapshot_store.list(limit=20)
        if not snapshots:
            self._print("No snapshots saved.")
            return

        self._print(_style("\nSnapshots", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        for snap in snapshots:
            created = snap.created_at[:19] if snap.created_at else "unknown"
            self._print(f"  {snap.name}")
            self._print(_style(f"    ID: {snap.snapshot_id[:8]}...  Created: {created}", _C.DIM, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))

    # ---------------------------------------------------------------------
    # Execution
    # ---------------------------------------------------------------------

    def _start(self, task: str) -> None:
        # Note: _approve_all_session is NOT reset here - it persists for the entire session
        run_id = self._agent.start(task)
        if self._state_file:
            self._agent.save_state(self._state_file)
        self._run_loop(run_id)

    def _resume(self) -> None:
        if self._agent.run_id is None and self._state_file:
            self._try_load_state()

        run_id = self._agent.run_id
        if run_id is None:
            self._print("No run to resume.")
            return

        self._run_loop(run_id)

    def _try_load_state(self) -> None:
        try:
            state = self._agent.load_state(self._state_file)  # type: ignore[arg-type]
        except Exception as e:
            self._print(_style("State load failed:", _C.YELLOW, enabled=self._color) + f" {e}")
            return
        if state is not None:
            messages: Optional[List[Dict[str, Any]]] = None
            loaded = self._messages_from_state(state)
            if loaded:
                messages = loaded

            if messages is not None:
                self._agent.session_messages = messages

            if state.status == self._RunStatus.WAITING:
                msg = "Loaded saved run. Type '/resume' to continue."
            else:
                msg = "Loaded history from last run."
            self._print(_style(msg, _C.DIM, enabled=self._color))

    def _run_loop(self, run_id: str) -> None:
        while True:
            try:
                state = self._agent.step()
            except KeyboardInterrupt:
                self._ui.clear_spinner()
                state = self._safe_get_state()
                if state is not None:
                    loaded = self._messages_from_state(state)
                    if loaded:
                        self._agent.session_messages = loaded
                self._print(_style("\nInterrupted. Run state preserved.", _C.YELLOW, enabled=self._color))
                return

            if state.status == self._RunStatus.COMPLETED:
                self._ui.clear_spinner()
                if state.output and isinstance(state.output.get("messages"), list):
                    self._agent.session_messages = list(state.output["messages"])
                return

            if state.status == self._RunStatus.FAILED:
                self._ui.clear_spinner()
                self._print(_style("\nRun failed:", _C.RED, enabled=self._color) + f" {state.error}")
                loaded = self._messages_from_state(state)
                if loaded:
                    self._agent.session_messages = loaded
                return

            if state.status != self._RunStatus.WAITING or not state.waiting:
                # Either still RUNNING (max_steps exceeded) or some other non-blocking state.
                continue

            wait = state.waiting
            if wait.reason == self._WaitReason.USER:
                response = self._prompt_user(wait.prompt or "Please respond:", wait.choices)
                state = self._agent.resume(response)
                continue

            # Tool approval waits are modeled as EVENT waits with details.tool_calls.
            details = wait.details or {}
            tool_calls = details.get("tool_calls")
            if isinstance(tool_calls, list):
                self._ui.clear_spinner()  # Clear spinner during approval prompt
                payload = self._approve_and_execute(tool_calls)
                if payload is None:
                    self._print(_style("\nLeft run waiting (not resumed).", _C.DIM, enabled=self._color))
                    return

                state = self._runtime.resume(
                    workflow=self._agent.workflow,
                    run_id=run_id,
                    wait_key=wait.wait_key,
                    payload=payload,
                )
                continue

            self._ui.clear_spinner()
            self._print(
                _style("\nWaiting:", _C.YELLOW, enabled=self._color)
                + f" {wait.reason.value} ({wait.wait_key})"
            )
            return

    def _prompt_user(self, prompt: str, choices: Optional[Sequence[str]]) -> str:
        self._ui.clear_spinner()  # Clear spinner when prompting user
        if choices:
            self._print(_style(prompt, _C.MAGENTA, _C.BOLD, enabled=self._color))
            for i, c in enumerate(choices):
                self._print(f"  [{i+1}] {c}")
            while True:
                raw = self._simple_prompt("Choice (number or text): ")
                if not raw:
                    continue
                if raw.isdigit():
                    idx = int(raw) - 1
                    if 0 <= idx < len(choices):
                        return str(choices[idx])
                return raw
        return self._simple_prompt(prompt + " ")

    def _approve_and_execute(self, tool_calls: List[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
        if self._auto_approve:
            return self._tool_runner.execute(tool_calls=tool_calls)

        # If user already said "all" for this session, just execute without UI clutter
        if self._approve_all_session:
            return self._tool_runner.execute(tool_calls=tool_calls)

        self._print(_style("\nTool approval required", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))

        approve_all = False
        results: List[Dict[str, Any]] = []

        for tc in tool_calls:
            name = str(tc.get("name", "") or "")
            args = dict(tc.get("arguments") or {})
            call_id = str(tc.get("call_id") or "")

            spec = self._tool_specs.get(name)
            descr = spec.description if spec else ""

            self._print(_style(f"\n{name}", _C.GREEN, _C.BOLD, enabled=self._color))
            if descr:
                self._print(_style(descr, _C.DIM, enabled=self._color))
            self._print(
                _style("args:", _C.DIM, enabled=self._color)
                + " "
                + json.dumps(_truncate_json(args), indent=2, ensure_ascii=False)
            )

            if not approve_all:
                while True:
                    choice = self._simple_prompt("Approve? [y]es/[n]o/[a]ll/[e]dit/[q]uit: ").lower()
                    if choice in ("y", "yes"):
                        break
                    if choice in ("a", "all"):
                        approve_all = True
                        self._approve_all_session = True
                        break
                    if choice in ("n", "no"):
                        results.append(
                            {
                                "call_id": call_id,
                                "name": name,
                                "success": False,
                                "output": None,
                                "error": "Rejected by user",
                            }
                        )
                        name = ""
                        break
                    if choice in ("q", "quit"):
                        return None
                    if choice in ("e", "edit"):
                        edited = self._simple_prompt("New arguments (JSON): ")
                        if edited:
                            try:
                                new_args = json.loads(edited)
                            except json.JSONDecodeError as e:
                                self._print(_style(f"Invalid JSON: {e}", _C.YELLOW, enabled=self._color))
                                continue
                            if not isinstance(new_args, dict):
                                self._print(_style("Arguments must be a JSON object.", _C.YELLOW, enabled=self._color))
                                continue
                            args = new_args
                            tc["arguments"] = args
                            self._print(_style("Updated args.", _C.DIM, enabled=self._color))
                        continue

                    self._print("Enter y/n/a/e/q.")

            if not name:
                continue

            # Additional confirmation for shell execution (skip if approve_all is set)
            if name == "execute_command" and not approve_all:
                confirm = self._simple_prompt("Type 'run' to execute this command: ").lower()
                if confirm != "run":
                    results.append(
                        {
                            "call_id": call_id,
                            "name": name,
                            "success": False,
                            "output": None,
                            "error": "Rejected by user",
                        }
                    )
                    continue

            single = {"name": name, "arguments": args, "call_id": call_id}
            out = self._tool_runner.execute(tool_calls=[single])
            results.extend(out.get("results") or [])

        return {"mode": "executed", "results": results}


def _truncate_json(value: Any, *, max_str: int = 800, max_list: int = 50, max_dict: int = 50) -> Any:
    if isinstance(value, str):
        if len(value) <= max_str:
            return value
        head = value[:400]
        tail = value[-200:] if len(value) > 600 else ""
        suffix = f"... ({len(value)} chars total)"
        return head + (("\n" + suffix + "\n" + tail) if tail else ("\n" + suffix))

    if isinstance(value, list):
        trimmed = value[:max_list]
        out = [_truncate_json(v, max_str=max_str, max_list=max_list, max_dict=max_dict) for v in trimmed]
        if len(value) > max_list:
            out.append(f"... ({len(value)} items total)")
        return out

    if isinstance(value, dict):
        items = list(value.items())[:max_dict]
        out_dict: Dict[str, Any] = {}
        for k, v in items:
            out_dict[str(k)] = _truncate_json(v, max_str=max_str, max_list=max_list, max_dict=max_dict)
        if len(value) > max_dict:
            out_dict["..."] = f"({len(value)} keys total)"
        return out_dict

    return value
