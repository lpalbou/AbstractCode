from __future__ import annotations

import json
import os
import sys
import threading
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
    # Use an explicit 256-color blue for better contrast/readability on dark terminal themes.
    BLUE = "\033[38;5;39m"
    GREEN = "\033[32m"
    YELLOW = "\033[33m"
    MAGENTA = "\033[35m"
    RED = "\033[31m"
    ORANGE = "\033[38;5;214m"


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
        plan_mode: bool = False,
        review_mode: bool = False,
        review_max_rounds: int = 1,
        max_iterations: int,
        max_tokens: Optional[int] = None,
        color: bool,
    ):
        self._agent_kind = str(agent or "react").strip().lower()
        if self._agent_kind not in ("react", "codeact"):
            raise ValueError("agent must be 'react' or 'codeact'")
        self._provider = provider
        self._model = model
        self._state_file = state_file or None
        self._auto_approve = auto_approve
        self._plan_mode = bool(plan_mode)
        self._review_mode = bool(review_mode)
        self._review_max_rounds = int(review_max_rounds)
        if self._review_max_rounds < 0:
            self._review_max_rounds = 0
        self._max_iterations = int(max_iterations)
        if self._max_iterations < 1:
            raise ValueError("max_iterations must be >= 1")
        # `None` means "auto from model capabilities". CLI may pass `-1` for auto.
        try:
            self._max_tokens = None if isinstance(max_tokens, int) and max_tokens <= 0 else max_tokens
        except Exception:
            self._max_tokens = None
        # Enable ANSI colors - fullscreen_ui uses ANSI class to parse escape codes
        self._color = bool(color and _supports_color())
        # Session-level tool allowlist (None = default/all tools for the agent kind).
        self._allowed_tools: Optional[List[str]] = None

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
            plan_mode=self._plan_mode,
            review_mode=self._review_mode,
            review_max_rounds=self._review_max_rounds,
        )

        # Session-level tool approval (persists across all requests)
        self._approve_all_session = False

        # Output buffer for full-screen mode
        self._output_lines: List[str] = []

        # Initialize full-screen UI with scrollable history
        self._ui = FullScreenUI(
            get_status_text=self._get_status_text,
            on_input=self._handle_input,
            on_copy_payload=self._copy_to_clipboard,
            color=self._color,
        )

        # Keep simple session for tool approvals (runs within full-screen)
        self._simple_session = create_simple_session(color=self._color)

        # Pending input for the run loop
        self._pending_input: Optional[str] = None

        # Per-turn observability (for copy + traceability)
        self._turn_task: Optional[str] = None
        self._turn_trace: List[str] = []
        # Simple in-session dedup for obviously repeated shell commands.
        self._last_execute_command: Optional[str] = None
        self._last_execute_command_result: Optional[Dict[str, Any]] = None
        # Simple in-session dedup for repeated file mutations (common model glitch).
        self._last_mutating_tool_call_key: Optional[Tuple[str, str]] = None
        self._last_mutating_tool_call_result: Optional[Dict[str, Any]] = None
        # Pending tool-line spinner markers (one per emitted act event).
        self._pending_tool_markers: List[str] = []
        # Pending tool call metadata (aligned with tool markers/results).
        self._pending_tool_metas: List[Dict[str, Any]] = []
        # Keep the last started run id so /context can show traces even after completion.
        self._last_run_id: Optional[str] = None
        # Status bar cache (token counting can be expensive; avoid per-frame rescans).
        self._status_cache_key: Optional[Tuple[Any, ...]] = None
        self._status_cache_text: str = ""
        # Run execution happens in a dedicated background thread so the UI worker thread
        # can keep processing commands (/pause, /cancel, /status, ...).
        self._run_thread: Optional[threading.Thread] = None
        self._run_thread_lock = threading.Lock()

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
            state = self._agent.get_state()
            if state is not None:
                return state
            # If there's no active run, still allow inspecting the last run (durable via RunStore).
            if isinstance(self._last_run_id, str) and self._last_run_id:
                return self._runtime.get_state(self._last_run_id)
            return None
        except (KeyError, Exception):
            # Run doesn't exist (completed/cleaned up) or other error
            return None

    def _get_status_text(self) -> str:
        """Generate status text for the status bar."""
        # Keep this fast: the render thread can call this frequently.
        state = self._safe_get_state()

        # Prefer the exact LLM-visible message view when a run is attached.
        if state is not None and hasattr(state, "vars") and isinstance(getattr(state, "vars", None), dict):
            try:
                from abstractruntime.memory.active_context import ActiveContextPolicy

                messages = ActiveContextPolicy.select_active_messages_for_llm_from_run(state)
            except Exception:
                messages = self._messages_from_state(state)

            limits = state.vars.get("_limits") if isinstance(state.vars.get("_limits"), dict) else {}
            max_tokens_raw = limits.get("max_tokens")
            try:
                max_tokens = int(max_tokens_raw) if max_tokens_raw is not None else int(self._max_tokens or 32768)
            except Exception:
                max_tokens = int(self._max_tokens or 32768)
        else:
            messages = list(self._agent.session_messages or [])
            max_tokens = int(self._max_tokens or 32768)

        if max_tokens <= 0:
            try:
                caps = self._llm_client.get_model_capabilities()
                max_tokens = int(caps.get("max_tokens", 32768) or 32768)
            except Exception:
                max_tokens = 32768

        # Cache by a cheap signature to avoid rescanning large contexts every frame.
        last = messages[-1] if isinstance(messages, list) and messages else {}
        last_id = ""
        last_ts = ""
        last_len = 0
        if isinstance(last, dict):
            meta = last.get("metadata")
            if isinstance(meta, dict):
                last_id = str(meta.get("message_id") or "")
            last_ts = str(last.get("timestamp") or "")
            last_len = len(str(last.get("content") or ""))
        cache_key = (getattr(state, "run_id", None) if state is not None else None, len(messages), last_id, last_ts, last_len, max_tokens, self._model)
        if self._status_cache_key == cache_key and self._status_cache_text:
            return self._status_cache_text

        tokens_used_source = "estimate"
        # Token estimation (AbstractCore; uses precise counting when possible, else robust heuristics).
        try:
            from abstractcore.utils.token_utils import TokenUtils

            llm_payload: Optional[Dict[str, Any]] = None
            llm_usage: Optional[Dict[str, Any]] = None
            if state is not None and hasattr(state, "vars") and isinstance(getattr(state, "vars", None), dict):
                runtime_ns = state.vars.get("_runtime") if isinstance(state.vars.get("_runtime"), dict) else {}
                traces = runtime_ns.get("node_traces") if isinstance(runtime_ns, dict) else None
                latest_ts = ""
                if isinstance(traces, dict):
                    for node_trace in traces.values():
                        if not isinstance(node_trace, dict):
                            continue
                        steps = node_trace.get("steps")
                        if not isinstance(steps, list):
                            continue
                        for step in steps:
                            if not isinstance(step, dict):
                                continue
                            eff = step.get("effect")
                            if not isinstance(eff, dict):
                                continue
                            if str(eff.get("type") or "") != "llm_call":
                                continue
                            ts = str(step.get("ts") or "")
                            payload = eff.get("payload") if isinstance(eff.get("payload"), dict) else None
                            if ts and payload is not None and ts > latest_ts:
                                latest_ts = ts
                                llm_payload = dict(payload)
                                result = step.get("result")
                                if isinstance(result, dict):
                                    usage = result.get("usage")
                                    llm_usage = dict(usage) if isinstance(usage, dict) else None
                                else:
                                    llm_usage = None

            effective_model = str((llm_payload or {}).get("model") or self._model or "").strip() or None

            def count_tokens(text: str) -> int:
                return int(TokenUtils.count_tokens(str(text or ""), model=effective_model))

            def _usage_prompt_tokens(usage: Dict[str, Any]) -> Optional[int]:
                raw = usage.get("prompt_tokens")
                if raw is None:
                    raw = usage.get("input_tokens")
                if raw is None:
                    return None
                try:
                    value = int(raw)
                except Exception:
                    return None
                return value if value >= 0 else None

            provider_prompt_tokens = None
            if isinstance(llm_usage, dict):
                provider_prompt_tokens = _usage_prompt_tokens(llm_usage)
            if isinstance(provider_prompt_tokens, int):
                tokens_used = provider_prompt_tokens
                tokens_used_source = "provider"
            elif llm_payload is not None:
                sys_prompt = str(llm_payload.get("system_prompt") or "")
                prompt = str(llm_payload.get("prompt") or "")
                if not prompt:
                    raw_messages = llm_payload.get("messages")
                    if isinstance(raw_messages, list) and raw_messages:
                        text_parts: List[str] = []
                        for m in raw_messages:
                            if not isinstance(m, dict):
                                continue
                            content = str(m.get("content") or "")
                            if not content:
                                continue
                            role = str(m.get("role") or "").strip()
                            if role:
                                text_parts.append(f"{role}:\n{content}")
                            else:
                                text_parts.append(content)
                        prompt = "\n\n".join(text_parts).strip()

                tools = llm_payload.get("tools") or []

                system_tokens = count_tokens(sys_prompt) if sys_prompt else 0
                prompt_tokens = count_tokens(prompt) if prompt else 0

                tool_prompt_tokens = 0
                if isinstance(tools, list) and tools:
                    try:
                        from abstractcore.tools.handler import UniversalToolHandler

                        handler = UniversalToolHandler(str(effective_model or ""))
                        tool_prompt = handler.format_tools_prompt(tools)
                        tool_prompt_tokens = count_tokens(tool_prompt) if tool_prompt else 0
                    except Exception:
                        tool_prompt_tokens = count_tokens(json.dumps(tools, ensure_ascii=False, sort_keys=True))

                tokens_used = system_tokens + prompt_tokens + tool_prompt_tokens
            else:
                # Fallback: approximate from the active message view.
                text_parts = []
                for m in messages:
                    if not isinstance(m, dict):
                        continue
                    content = str(m.get("content") or "")
                    if not content:
                        continue
                    role = str(m.get("role") or "").strip()
                    if role:
                        text_parts.append(f"{role}:\n{content}")
                    else:
                        text_parts.append(content)
                joined = "\n\n".join(text_parts).strip()
                tokens_used = count_tokens(joined) if joined else 0
        except Exception:
            # Conservative fallback (≈4 chars/token).
            tokens_used = sum(max(1, len(str(m.get("content", ""))) // 4) for m in messages if isinstance(m, dict) and m.get("content")) if messages else 0

        pct = (tokens_used / max_tokens) * 100 if max_tokens > 0 else 0.0
        approx = "~" if tokens_used_source != "provider" else ""
        status = f"{self._provider} | {self._model} | Context: {approx}{tokens_used:,}/{max_tokens:,} tk ({pct:.0f}%)"
        self._status_cache_key = cache_key
        self._status_cache_text = status
        return status

    def _print(self, text: str = "") -> None:
        """Append text to the UI output area."""
        self._output_lines.append(text)
        self._ui.append_output(text)

    def _terminal_width(self) -> int:
        """Best-effort current terminal width (for full-line ANSI background blocks)."""
        try:
            import shutil

            width = int(shutil.get_terminal_size(fallback=(120, 40)).columns)
        except Exception:
            width = 120
        return max(40, width)

    def _truncate_for_ui(self, value: Any, *, max_chars: int) -> Any:
        """Truncate long string values for UI display only (agent state is unchanged)."""
        if isinstance(value, str):
            if len(value) <= max_chars:
                return value
            if max_chars <= 1:
                return "…"
            return value[: max_chars - 1] + "…"
        if isinstance(value, dict):
            return {k: self._truncate_for_ui(v, max_chars=max_chars) for k, v in value.items()}
        if isinstance(value, list):
            return [self._truncate_for_ui(v, max_chars=max_chars) for v in value]
        if isinstance(value, tuple):
            return tuple(self._truncate_for_ui(v, max_chars=max_chars) for v in value)
        return value

    def _strip_tool_prefix(self, raw: str, tool_name: str) -> str:
        raw = "" if raw is None else str(raw)
        tool_name = str(tool_name or "")
        if not tool_name:
            return raw
        prefix = f"[{tool_name}]:"
        if raw.startswith(prefix):
            return raw[len(prefix) :].lstrip()
        return raw

    def _print_tool_observation(
        self,
        *,
        tool_name: str,
        raw: str,
        ok: Optional[bool] = None,
        indent: str = "  ",
        tool_args: Optional[Dict[str, Any]] = None,
    ) -> None:
        """Render tool output in a compact, readable way for the UI."""
        tool_name = str(tool_name or "")
        raw = "" if raw is None else str(raw)

        if tool_name in ("write_file", "read_file"):
            import re

            cwd = os.getcwd()
            cleaned = self._strip_tool_prefix(raw, tool_name=tool_name).strip()

            if tool_name == "write_file":
                line = (cleaned.splitlines() or [""])[0].strip()
                if ok is False and line and not line.startswith("❌"):
                    line = f"❌ {line}" if line else "❌ Failed"
                if line and "(" in line and line.endswith(")"):
                    head, tail = line.rsplit("(", 1)
                    inner = tail[:-1]
                    line = f"{head}(current folder: {cwd}, {inner})"
                elif line:
                    line = f"{line} (current folder: {cwd})"
                self._print(_style(f"{indent}{line}", _C.DIM, enabled=self._color))
                return

            # read_file: avoid dumping full file contents into the chat view.
            file_path = ""
            if isinstance(tool_args, dict):
                file_path = str(tool_args.get("file_path") or "")

            if ok is False or cleaned.startswith("Error:") or cleaned.startswith("❌"):
                if not cleaned.startswith("❌"):
                    cleaned = f"❌ {cleaned}".rstrip()
                self._print(_style(f"{indent}{cleaned}", _C.DIM, enabled=self._color))
                return

            header = (cleaned.splitlines() or [""])[0].strip()
            m = re.match(r"^File:\s*(?P<path>.+?)\s*\((?P<lines>[\d,]+)\s+lines\)\s*$", header)
            if m:
                file_path = file_path or m.group("path").strip()
                try:
                    line_count = int(m.group("lines").replace(",", ""))
                except Exception:
                    line_count = None

                split_idx = cleaned.find("\n\n")
                if split_idx >= 0:
                    content = cleaned[split_idx + 2 :]
                else:
                    content = "\n".join(cleaned.splitlines()[1:])

                bytes_read = len(content.encode("utf-8"))
                lines_read = line_count if isinstance(line_count, int) else len(content.splitlines())
                summary = f"✅ Read '{file_path}' (current folder: {cwd}, {bytes_read:,} bytes, {lines_read:,} lines)"
                self._print(_style(f"{indent}{summary}", _C.DIM, enabled=self._color))
                return

            bytes_read = len(cleaned.encode("utf-8"))
            lines_read = len(cleaned.splitlines())
            path_part = f" '{file_path}'" if file_path else ""
            summary = f"✅ Read{path_part} (current folder: {cwd}, {bytes_read:,} bytes, {lines_read:,} lines)"
            self._print(_style(f"{indent}{summary}", _C.DIM, enabled=self._color))
            return

        for line in (raw.splitlines() or [""]):
            style_codes: Tuple[str, ...] = (_C.DIM,)

            if tool_name == "edit_file":
                if line.startswith("Edited ") or line.startswith("Preview "):
                    style_codes = (_C.BOLD,)
                elif line.startswith("@@"):
                    style_codes = (_C.DIM,)
                elif line.startswith(" "):
                    # Context lines should remain high-contrast (default terminal fg) so it's easy
                    # to see *where* an edit applied.
                    style_codes = ()
                elif line.startswith("+") and not line.startswith("+++"):
                    style_codes = (_C.BLUE,)
                elif line.startswith("-") and not line.startswith("---"):
                    style_codes = (_C.RED,)
                else:
                    style_codes = (_C.DIM,)

            self._print(_style(f"{indent}{line}", *style_codes, enabled=self._color))

    def _format_user_prompt_block(self, text: str, *, copy_id: Optional[str] = None) -> str:
        """Render a user prompt as a padded, full-line background block (no truncation)."""
        lines = text.splitlines() or [""]
        copy_marker = f"[[COPY:{copy_id}]]" if isinstance(copy_id, str) and copy_id else ""

        prefix_first = "> "
        prefix_next = "  "
        prefix_len = len(prefix_first)
        width = self._terminal_width()
        avail = max(1, width - prefix_len)

        def chunk_line(s: str) -> List[str]:
            if s == "":
                return [""]
            return [s[i : i + avail] for i in range(0, len(s), avail)]

        if not self._color:
            out: List[str] = [""]
            first_visual = True
            for line in lines:
                for chunk in chunk_line(line):
                    prefix = prefix_first if first_visual else prefix_next
                    out.append(prefix + chunk)
                    first_visual = False
            # Separate the copy button from the prompt content so it reads as a control, not content.
            if copy_marker:
                out.append("")
                out.append(copy_marker)
            out.append("")
            return "\n".join(out)

        bg = "\033[48;5;238m"
        fg = "\033[38;5;255m"
        reset = _C.RESET

        def style_full(line_text: str) -> str:
            padded = line_text + (" " * max(0, width - len(line_text)))
            return f"{bg}{fg}{padded}{reset}"

        blank = f"{bg}{' ' * width}{reset}"
        # Add black spacer lines above/below the grey block for readability.
        out_lines: List[str] = [""]
        out_lines.append(blank)

        first_visual = True
        for line in lines:
            for chunk in chunk_line(line):
                prefix = prefix_first if first_visual else prefix_next
                out_lines.append(style_full(prefix + chunk))
                first_visual = False

        out_lines.append(blank)
        # Separate the copy button from the framed block for better visual grouping.
        if copy_marker:
            out_lines.append("")
            out_lines.append(prefix_next + copy_marker)
        # Keep a black spacer line after the copy button for readability.
        out_lines.append("")
        return "\n".join(out_lines)

    def _handle_input(self, text: str) -> None:
        """Handle user input from the UI (called from worker thread)."""
        import uuid

        text = text.strip()
        if not text:
            return

        # Echo user input (styled so user prompts are easy to spot).
        copy_id = f"user_{uuid.uuid4().hex}"
        self._ui.register_copy_payload(copy_id, text)
        self._print(self._format_user_prompt_block(text, copy_id=copy_id))

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
            "plan",
            "review",
            "compact",
            "spans",
            "expand",
            "vars",
            "var",
            "context",
            "remember",
            "recall",
            "copy",
            "mouse",
            "flow",
            "history",
            "resume",
            "pause",
            "cancel",
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

    def _build_answer_copy_payload(self, *, answer_text: str, prompt_text: Optional[str] = None) -> str:
        """Build the payload for the assistant copy button (best-effort, lossless)."""
        blocks: List[str] = []

        prompt = prompt_text
        if prompt is None:
            prompt = getattr(self, "_turn_task", None)
        if isinstance(prompt, str) and prompt.strip():
            blocks.append("User:\n" + prompt.strip())

        trace = getattr(self, "_turn_trace", None)
        if isinstance(trace, list) and trace:
            trace_text = "\n\n".join([t for t in trace if isinstance(t, str) and t.strip()]).strip()
            if trace_text:
                blocks.append("Trace:\n" + trace_text)

        blocks.append("Answer:\n" + str(answer_text or "").strip())
        return "\n\n".join([b for b in blocks if b.strip()]).strip()

    def _print_answer_block(self, *, title: str, answer_text: str, prompt_text: Optional[str] = None) -> None:
        import uuid

        answer = "" if answer_text is None else str(answer_text)
        if not answer.strip():
            answer = "(no assistant answer produced yet)"

        copy_id = f"assistant_{uuid.uuid4().hex}"
        payload = self._build_answer_copy_payload(answer_text=answer, prompt_text=prompt_text)
        self._ui.register_copy_payload(copy_id, payload)

        self._print(_style(f"\n{title}", _C.GREEN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        self._print(answer)
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        self._print(f"[[COPY:{copy_id}]]")
        self._print("")

    def _extract_latest_turn_prompt_and_answer(self, state: Any) -> tuple[Optional[str], str]:
        """Best-effort: return (latest turn prompt, latest assistant answer after that prompt)."""
        messages = self._messages_from_state(state)
        if not messages:
            prompt = getattr(self, "_turn_task", None)
            return (prompt if isinstance(prompt, str) else None, "")

        turn_task = getattr(self, "_turn_task", None)

        user_idx: Optional[int] = None
        if isinstance(turn_task, str) and turn_task:
            for i in range(len(messages) - 1, -1, -1):
                m = messages[i]
                if not isinstance(m, dict):
                    continue
                if m.get("role") == "user" and str(m.get("content") or "") == turn_task:
                    user_idx = i
                    break

        if user_idx is None:
            for i in range(len(messages) - 1, -1, -1):
                m = messages[i]
                if not isinstance(m, dict):
                    continue
                if m.get("role") == "user":
                    user_idx = i
                    break

        prompt_text: Optional[str] = None
        if user_idx is not None:
            m = messages[user_idx]
            if isinstance(m, dict):
                prompt_text = str(m.get("content") or "")

        answer_text = ""
        if user_idx is not None:
            for j in range(len(messages) - 1, user_idx, -1):
                m = messages[j]
                if not isinstance(m, dict):
                    continue
                if m.get("role") == "assistant":
                    answer_text = str(m.get("content") or "")
                    break
        else:
            for j in range(len(messages) - 1, -1, -1):
                m = messages[j]
                if not isinstance(m, dict):
                    continue
                if m.get("role") == "assistant":
                    answer_text = str(m.get("content") or "")
                    break

        return prompt_text, answer_text

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
            self._ui.set_spinner(f"Thinking (step {it}/{max_it})...")
        elif step == "parse":
            # Show the agent's actual "thinking" (rationale) when it is about to act.
            # We only print this for tool-using iterations to avoid duplicating final answers.
            has_tool_calls = bool(data.get("has_tool_calls"))
            content = str(data.get("content", "") or "")
            if has_tool_calls and content.strip():
                text = content.strip()
                self._turn_trace.append("Thought:\n" + text)
                self._print("")
                self._print(_style("Thought", _C.ORANGE, _C.BOLD, enabled=self._color))
                self._print(_style(text, _C.ORANGE, enabled=self._color))
                self._print("")
        elif step == "act":
            import uuid

            tool = data.get("tool", "unknown")
            args = data.get("args") or {}
            call_id_raw = data.get("call_id")
            call_id = str(call_id_raw).strip() if call_id_raw is not None else ""
            ui_args = self._truncate_for_ui(args, max_chars=200)
            try:
                args_str = json.dumps(ui_args, ensure_ascii=False, sort_keys=True)
            except Exception:
                args_str = str(ui_args)
            call_suffix = f" [{call_id}]" if call_id else ""
            marker_id = f"tool_{uuid.uuid4().hex}"
            marker = f"[[SPINNER:{marker_id}]]"
            self._pending_tool_markers.append(marker)
            self._pending_tool_metas.append({"tool": tool, "args": dict(args), "call_id": call_id})
            header = f"Tool: {tool}{call_suffix}({args_str})"
            self._print(_style(header, _C.GREEN, _C.BOLD, enabled=self._color) + f" {marker}")
            # Track full arguments for copy payloads (no truncation).
            try:
                args_full = json.dumps(args, ensure_ascii=False, sort_keys=True)
            except Exception:
                args_full = str(args)
            self._turn_trace.append(f"Tool: {tool}{call_suffix}({args_full})")
            self._ui.set_spinner(f"Running {tool}...")
        elif step == "observe":
            raw = str(data.get("result", "") or "")
            success = data.get("success")
            ok = bool(success) if success is not None else True

            tool_name = str(data.get("tool", "") or "tool")
            # Some tools return "Error: ..." strings instead of raising exceptions. Treat those
            # as failures for UI badges (✅/❌) even if the executor reported success.
            try:
                cleaned = self._strip_tool_prefix(raw, tool_name=tool_name).lstrip()
                if cleaned.startswith(("Error:", "❌", "🚫", "⏰")):
                    ok = False
            except Exception:
                pass
            if self._pending_tool_markers:
                marker = self._pending_tool_markers.pop(0)
                icon = "✅" if ok else "❌"
                # Best-effort in-place update; if not found, fall back silently.
                self._ui.replace_output_marker(marker, icon)
            tool_args = None
            if self._pending_tool_metas:
                try:
                    meta = self._pending_tool_metas.pop(0)
                    if isinstance(meta, dict):
                        args_meta = meta.get("args")
                        if isinstance(args_meta, dict):
                            tool_args = args_meta
                except Exception:
                    tool_args = None
            # Keep observability compact, but render diffs with clear colors.
            self._print_tool_observation(tool_name=tool_name, raw=raw, ok=ok, indent="", tool_args=tool_args)
            self._turn_trace.append(f"Result ({tool_name}):\n{raw}".rstrip())
            self._ui.set_spinner("Processing result...")
        elif step == "ask_user":
            self._ui.clear_spinner()
            self._ui.scroll_to_bottom()
            self._print(_style("Agent question:", _C.MAGENTA, _C.BOLD, enabled=self._color))
        elif step == "done":
            self._ui.clear_spinner()
            self._ui.scroll_to_bottom()
            answer_text = str(data.get("answer", "") or "")
            self._print_answer_block(title="ANSWER", answer_text=answer_text)
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
        if command in ("tools", "tool", "toolset"):
            self._handle_tools(arg)
            return False
        if command in ("tool-specs", "tool_specs", "toolspecs"):
            self._show_tools()
            return False
        if command == "status":
            self._show_status()
            return False
        if command in ("auto-accept", "auto_accept"):
            self._set_auto_accept(arg)
            return False
        if command == "plan":
            self._handle_plan(arg)
            return False
        if command == "review":
            self._handle_review(arg)
            return False
        if command == "resume":
            self._resume()
            return False
        if command == "pause":
            self._pause()
            return False
        if command == "cancel":
            self._cancel()
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
        if command == "clear":
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
        if command == "context":
            self._handle_context(arg)
            return False
        if command == "llm":
            self._handle_llm(arg)
            return False
        if command == "mouse":
            self._handle_mouse_toggle()
            return False
        if command == "copy":
            self._handle_copy(arg)
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
            self._print(_style("  /flow run <flow_id_or_path> [--verbosity none|default|full] [--key value ...]", _C.DIM, enabled=self._color))
            self._print(_style("  /flow resume [--verbosity none|default|full] [--wait-until]", _C.DIM, enabled=self._color))
            self._print(_style("  /flow pause|resume-run|cancel", _C.DIM, enabled=self._color))
            return

        action = parts[0].strip().lower()

        from .flow_cli import control_flow_command, resume_flow_command, run_flow_command

        def _emit_answer_user(message: str) -> None:
            import uuid

            copy_id = f"assistant_{uuid.uuid4().hex}"
            self._ui.register_copy_payload(copy_id, message)
            self._print(message)
            self._print(f"[[COPY:{copy_id}]]")
            self._print("")
            self._append_to_active_context(
                role="assistant",
                content=message,
                metadata={"kind": "flow_output"},
            )

        def _emit_flow_trace(trace: Any) -> None:
            """Add a durable tool-like trace so follow-up questions can reference what happened."""
            try:
                flow_name = str(getattr(trace, "flow_name", "") or "")
                flow_id = str(getattr(trace, "flow_id", "") or "")
                run_id = str(getattr(trace, "run_id", "") or "")
                status = str(getattr(trace, "status", "") or "")
                tool_calls = getattr(trace, "tool_calls", None)
                if not isinstance(tool_calls, list):
                    tool_calls = []
            except Exception:
                return

            lines: List[str] = []
            lines.append("Flow trace (AbstractFlow via AbstractCode):")
            if flow_name or flow_id:
                lines.append(f"- flow: {flow_name or flow_id} ({flow_id})".rstrip())
            if run_id:
                lines.append(f"- run_id: {run_id}")
            if status:
                lines.append(f"- status: {status}")

            if tool_calls:
                lines.append("- tools:")
                for tc in tool_calls:
                    if not isinstance(tc, dict):
                        continue
                    name = str(tc.get("name") or "")
                    args = tc.get("arguments") if isinstance(tc.get("arguments"), dict) else {}
                    if name == "fetch_url":
                        url = str(args.get("url") or "")
                        lines.append(f"  - fetch_url: {url}" if url else "  - fetch_url")
                    elif name == "web_search":
                        q = str(args.get("query") or "")
                        lines.append(f"  - web_search: {q}" if q else "  - web_search")
                    else:
                        # Keep args compact but untruncated for fidelity.
                        try:
                            args_json = json.dumps(args, ensure_ascii=False, sort_keys=True)
                        except Exception:
                            args_json = str(args)
                        lines.append(f"  - {name}: {args_json}" if name else f"  - tool: {args_json}")

            self._append_to_active_context(
                role="tool",
                content="\n".join(lines),
                metadata={
                    "kind": "flow_trace",
                    "name": "flow",
                    "flow_id": flow_id,
                    "flow_name": flow_name,
                    "run_id": run_id,
                    "status": status,
                },
            )

        if action == "run":
            if len(parts) < 2:
                self._print(_style("Usage: /flow run <flow_id_or_path> [--verbosity none|default|full] [--key value ...]", _C.DIM, enabled=self._color))
                return

            flow_ref = parts[1]
            rest = parts[2:]

            flows_dir: Optional[str] = None
            input_json: Optional[str] = None
            input_file: Optional[str] = None
            params: List[str] = []
            extra_args: List[str] = []
            wait_until = False
            verbosity = "default"
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

                if token.startswith("--verbosity"):
                    val = _opt_value()
                    if val is None:
                        self._print(_style("Missing value for --verbosity", _C.YELLOW, enabled=self._color))
                        return
                    v = str(val).strip().lower()
                    if v not in ("none", "default", "full"):
                        self._print(_style("Verbosity must be one of: none, default, full", _C.YELLOW, enabled=self._color))
                        return
                    verbosity = v
                    i += 2 if "=" not in token else 1
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
                trace = run_flow_command(
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
                    verbosity=verbosity,  # type: ignore[arg-type]
                    print_fn=self._print,
                    prompt_fn=self._simple_prompt,
                    ask_user_fn=self._prompt_user,
                    on_answer_user=_emit_answer_user,
                )
                _emit_flow_trace(trace)
            except Exception as e:
                self._print(_style(f"Flow run failed: {e}", _C.YELLOW, enabled=self._color))
            return

        if action == "resume":
            # Allow `--verbosity` and `--wait-until` for resume.
            rest = parts[1:]
            wait_until = False
            verbosity = "default"
            i = 0
            while i < len(rest):
                token = rest[i]

                def _opt_value() -> Optional[str]:
                    if "=" in token:
                        return token.split("=", 1)[1]
                    if i + 1 < len(rest):
                        return rest[i + 1]
                    return None

                if token == "--wait-until":
                    wait_until = True
                    i += 1
                    continue
                if token.startswith("--verbosity"):
                    val = _opt_value()
                    if val is None:
                        self._print(_style("Missing value for --verbosity", _C.YELLOW, enabled=self._color))
                        return
                    v = str(val).strip().lower()
                    if v not in ("none", "default", "full"):
                        self._print(_style("Verbosity must be one of: none, default, full", _C.YELLOW, enabled=self._color))
                        return
                    verbosity = v
                    i += 2 if "=" not in token else 1
                    continue
                # Ignore unknown tokens for forward-compat (treat like `/flow run` extra args)
                i += 1

            try:
                trace = resume_flow_command(
                    flow_state_file=None,
                    no_state=False,
                    auto_approve=bool(self._auto_approve),
                    wait_until=bool(wait_until),
                    verbosity=verbosity,  # type: ignore[arg-type]
                    print_fn=self._print,
                    prompt_fn=self._simple_prompt,
                    ask_user_fn=self._prompt_user,
                    on_answer_user=_emit_answer_user,
                )
                _emit_flow_trace(trace)
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

    def _handle_plan(self, raw: str) -> None:
        value = raw.strip().lower()
        if not value:
            status = "ON" if self._plan_mode else "OFF"
            self._print(_style(f"Plan mode: {status}", _C.DIM, enabled=self._color))
            return

        if value in ("toggle",):
            self._plan_mode = not self._plan_mode
        elif value in ("on", "true", "1", "yes", "y", "enabled"):
            self._plan_mode = True
        elif value in ("off", "false", "0", "no", "n", "disabled"):
            self._plan_mode = False
        else:
            self._print(_style("Usage: /plan [on|off]", _C.DIM, enabled=self._color))
            return

        if hasattr(self._agent, "_plan_mode"):
            self._agent._plan_mode = self._plan_mode  # type: ignore[attr-defined]
        status = "ON" if self._plan_mode else "OFF"
        self._print(_style(f"Plan mode set to {status}.", _C.DIM, enabled=self._color))
        self._save_config()

    def _handle_review(self, raw: str) -> None:
        value = raw.strip()
        if not value:
            status = "ON" if self._review_mode else "OFF"
            self._print(_style(f"Review mode: {status} (max_rounds={self._review_max_rounds})", _C.DIM, enabled=self._color))
            return

        parts = value.split()
        head = parts[0].lower()

        if head in ("toggle",):
            self._review_mode = not self._review_mode
        elif head in ("on", "true", "1", "yes", "y", "enabled"):
            self._review_mode = True
        elif head in ("off", "false", "0", "no", "n", "disabled"):
            self._review_mode = False
        elif head in ("rounds", "max-rounds", "max_rounds"):
            # Just set rounds, keep review mode as-is.
            if len(parts) < 2:
                self._print(_style("Usage: /review rounds <N>", _C.DIM, enabled=self._color))
                return
            head = "rounds"
        else:
            self._print(_style("Usage: /review [on|off] [max_rounds]  OR  /review rounds <N>", _C.DIM, enabled=self._color))
            return

        if head == "rounds" or (self._review_mode and len(parts) >= 2):
            raw_rounds = parts[1] if len(parts) >= 2 else ""
            try:
                rounds = int(raw_rounds)
            except ValueError:
                self._print(_style("review max_rounds must be an integer >= 0", _C.DIM, enabled=self._color))
                return
            if rounds < 0:
                rounds = 0
            self._review_max_rounds = rounds

        if hasattr(self._agent, "_review_mode"):
            self._agent._review_mode = self._review_mode  # type: ignore[attr-defined]
        if hasattr(self._agent, "_review_max_rounds"):
            self._agent._review_max_rounds = self._review_max_rounds  # type: ignore[attr-defined]

        status = "ON" if self._review_mode else "OFF"
        self._print(_style(f"Review mode set to {status} (max_rounds={self._review_max_rounds}).", _C.DIM, enabled=self._color))
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
                    try:
                        self._agent.update_limits(max_tokens=self._max_tokens)
                    except Exception:
                        pass
                    self._print(_style(f"Max tokens auto-detected: {detected:,} (from model capabilities)", _C.GREEN, enabled=self._color))
                except Exception as e:
                    self._print(_style(f"Auto-detection failed: {e}. Using default 32768.", _C.YELLOW, enabled=self._color))
                    self._max_tokens = 32768
                    self._reconfigure_agent()
                    try:
                        self._agent.update_limits(max_tokens=self._max_tokens)
                    except Exception:
                        pass
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
        try:
            self._agent.update_limits(max_tokens=self._max_tokens)
        except Exception:
            pass
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
        # Also update plan/review toggles (applies to the next started run).
        if hasattr(self._agent, "_plan_mode"):
            self._agent._plan_mode = self._plan_mode  # type: ignore[attr-defined]
        if hasattr(self._agent, "_review_mode"):
            self._agent._review_mode = self._review_mode  # type: ignore[attr-defined]
        if hasattr(self._agent, "_review_max_rounds"):
            self._agent._review_max_rounds = self._review_max_rounds  # type: ignore[attr-defined]
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
                try:
                    val = int(config["max_tokens"])
                except Exception:
                    val = None
                self._max_tokens = None if isinstance(val, int) and val <= 0 else val
            if "max_history_messages" in config:
                self._max_history_messages = config["max_history_messages"]
            if "auto_approve" in config:
                self._auto_approve = config["auto_approve"]
            if "plan_mode" in config:
                self._plan_mode = bool(config["plan_mode"])
            if "review_mode" in config:
                self._review_mode = bool(config["review_mode"])
            if "review_max_rounds" in config:
                try:
                    self._review_max_rounds = int(config["review_max_rounds"])
                except Exception:
                    self._review_max_rounds = 1
                if self._review_max_rounds < 0:
                    self._review_max_rounds = 0
            if "allowed_tools" in config:
                raw = config.get("allowed_tools")
                if raw is None:
                    self._allowed_tools = None
                elif isinstance(raw, list):
                    self._allowed_tools = [str(t).strip() for t in raw if isinstance(t, str) and t.strip()]
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
                "plan_mode": self._plan_mode,
                "review_mode": self._review_mode,
                "review_max_rounds": self._review_max_rounds,
                "allowed_tools": self._allowed_tools,
            }
            self._config_file.write_text(json.dumps(config, indent=2))
        except Exception:
            pass  # Silently fail if we can't write

    def _handle_tools(self, raw: str) -> None:
        """List or configure the session tool allowlist.

        Usage:
          /tools
          /tools reset
          /tools only <name...>
          /tools enable <name...>
          /tools disable <name...>

        Notes:
        - The selection is persisted in the session config (when state_file is set).
        - If a run is active, changes are applied immediately by updating `run.vars["_runtime"]["allowed_tools"]`.
        """
        import shlex

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        sub = parts[0].lower() if parts else "list"
        args = parts[1:] if len(parts) > 1 else []
        if sub not in ("list", "reset", "only", "enable", "disable", "help", "-h", "--help"):
            self._print(_style("Usage:", _C.DIM, enabled=self._color))
            self._print(_style("  /tools", _C.DIM, enabled=self._color))
            self._print(_style("  /tools reset", _C.DIM, enabled=self._color))
            self._print(_style("  /tools only <name...>", _C.DIM, enabled=self._color))
            self._print(_style("  /tools enable <name...>", _C.DIM, enabled=self._color))
            self._print(_style("  /tools disable <name...>", _C.DIM, enabled=self._color))
            return

        def _split_names(tokens: List[str]) -> List[str]:
            out: List[str] = []
            for t in tokens:
                for part in str(t).split(","):
                    name = part.strip()
                    if name:
                        out.append(name)
            # de-dup preserving order
            seen: set[str] = set()
            deduped: List[str] = []
            for n in out:
                if n in seen:
                    continue
                seen.add(n)
                deduped.append(n)
            return deduped

        def _available_tool_defs() -> Dict[str, Any]:
            logic = getattr(self._agent, "logic", None)
            tools = getattr(logic, "tools", None) if logic is not None else None
            out: Dict[str, Any] = {}
            if isinstance(tools, list):
                for t in tools:
                    name = getattr(t, "name", None)
                    desc = getattr(t, "description", None)
                    if isinstance(name, str) and name:
                        out[name] = {"name": name, "description": str(desc or "")}
            # Fallback to CLI-known tools (may omit runtime built-ins).
            if not out:
                for name, spec in (self._tool_specs or {}).items():
                    out[name] = {"name": name, "description": str(getattr(spec, "description", "") or "")}
            return out

        available = _available_tool_defs()
        available_names = sorted(available.keys())

        def _effective_allowlist_from_state() -> Optional[List[str]]:
            state = self._safe_get_state()
            if state is None or not hasattr(state, "vars") or not isinstance(state.vars, dict):
                return None
            runtime_ns = state.vars.get("_runtime")
            if not isinstance(runtime_ns, dict):
                return None
            raw_allow = runtime_ns.get("allowed_tools")
            if raw_allow is None:
                return None
            if not isinstance(raw_allow, list):
                return None
            return [str(t).strip() for t in raw_allow if isinstance(t, str) and t.strip()]

        def _apply_to_active_run(allow: Optional[List[str]]) -> None:
            state = self._safe_get_state()
            if state is None or not hasattr(state, "vars") or not isinstance(state.vars, dict):
                return
            runtime_ns = state.vars.get("_runtime")
            if not isinstance(runtime_ns, dict):
                runtime_ns = {}
                state.vars["_runtime"] = runtime_ns
            if allow is None:
                runtime_ns.pop("allowed_tools", None)
            else:
                runtime_ns["allowed_tools"] = list(allow)
            try:
                self._runtime.run_store.save(state)
            except Exception:
                pass

        if sub in ("help", "-h", "--help"):
            self._print(_style("Usage:", _C.DIM, enabled=self._color))
            self._print(_style("  /tools", _C.DIM, enabled=self._color))
            self._print(_style("  /tools reset", _C.DIM, enabled=self._color))
            self._print(_style("  /tools only <name...>", _C.DIM, enabled=self._color))
            self._print(_style("  /tools enable <name...>", _C.DIM, enabled=self._color))
            self._print(_style("  /tools disable <name...>", _C.DIM, enabled=self._color))
            return

        if sub == "reset":
            self._allowed_tools = None
            _apply_to_active_run(None)
            self._save_config()
            self._print(_style("✅ Tools reset to default (all enabled).", _C.GREEN, enabled=self._color))
            sub = "list"

        if sub in ("only", "enable", "disable"):
            names = _split_names(args)
            if not names:
                self._print(_style(f"Usage: /tools {sub} <name...>", _C.DIM, enabled=self._color))
                return
            unknown = [n for n in names if n not in available]
            if unknown:
                self._print(_style("Unknown tool(s): " + ", ".join(unknown), _C.YELLOW, enabled=self._color))
                self._print(_style("Use /tools to list available tools.", _C.DIM, enabled=self._color))
                return

            current = _effective_allowlist_from_state()
            if current is None:
                # Fall back to persisted selection if no active override.
                current = list(self._allowed_tools) if isinstance(self._allowed_tools, list) else list(available_names)
            current_set = set(current)

            if sub == "only":
                new_allow = names
            elif sub == "enable":
                new_allow = list(dict.fromkeys(list(current) + list(names)))
            else:  # disable
                new_allow = [n for n in current if n not in set(names)]
            new_set = set(new_allow)
            added = [n for n in new_allow if n not in current_set]
            removed = [n for n in current if n not in new_set]

            self._allowed_tools = list(new_allow)
            _apply_to_active_run(self._allowed_tools)
            self._save_config()
            parts: List[str] = []
            if added:
                parts.append("+" + ", ".join(added))
            if removed:
                parts.append("-" + ", ".join(removed))
            delta = f" ({' '.join(parts)})" if parts else ""
            self._print(
                _style(
                    f"✅ Tools updated: {len(self._allowed_tools)}/{len(available_names)} enabled{delta}.",
                    _C.GREEN,
                    enabled=self._color,
                )
            )
            sub = "list"

        # Default: list tools with enabled status.
        effective_from_run = _effective_allowlist_from_state()
        source = "active run" if isinstance(effective_from_run, list) else ("session config" if isinstance(self._allowed_tools, list) else "default")
        effective = effective_from_run
        if effective is None:
            effective = list(self._allowed_tools) if isinstance(self._allowed_tools, list) else list(available_names)

        enabled_set = set(effective)
        self._print(_style("\nTools", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        saved = "yes" if self._config_file else "no"
        self._print(_style(f"Enabled: {len(effective)}/{len(available_names)}  Saved: {saved}  Source: {source}", _C.DIM, enabled=self._color))
        for name in available_names:
            icon = "✅" if name in enabled_set else "❌"
            desc = available.get(name, {}).get("description") or ""
            line = f"  {icon} {name}"
            self._print(line)
            if isinstance(desc, str) and desc.strip():
                self._print(_style(f"     {desc.strip()}", _C.DIM, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        self._print(_style("Tip: /tools only list_files read_file write_file", _C.DIM, enabled=self._color))

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
        """Show structured Active Memory token usage + total next-LLM prompt usage."""
        import copy

        state = self._safe_get_state()
        if state is None or not hasattr(state, "vars") or not isinstance(state.vars, dict):
            self._print(_style("No run loaded. Start a task or /resume first.", _C.DIM, enabled=self._color))
            return

        # Token estimation (AbstractCore; uses precise counting when possible, else robust heuristics).
        try:
            from abstractcore.utils.token_utils import TokenUtils
        except Exception as e:
            self._print(_style("Token counting unavailable (AbstractCore import failed).", _C.YELLOW, enabled=self._color))
            self._print(_style(str(e), _C.DIM, enabled=self._color))
            return

        from abstractruntime.memory.active_memory import compute_active_memory_token_breakdown

        limits = state.vars.get("_limits") if isinstance(state.vars.get("_limits"), dict) else {}
        max_tokens_raw = limits.get("max_tokens")

        # Derive the next LLM_CALL payload (same approach as /context, but focused on LLM token accounting).
        sim_run = copy.deepcopy(state)
        start_node = str(getattr(sim_run, "current_node", "") or "")

        logic = getattr(self._agent, "logic", None)
        if logic is None:
            self._print(_style("Memory error: agent logic is not available.", _C.YELLOW, enabled=self._color))
            return

        try:
            if self._agent_kind == "react":
                from abstractagent.adapters.react_runtime import create_react_workflow

                workflow = create_react_workflow(logic=logic, on_step=None)
            else:
                from abstractagent.adapters.codeact_runtime import create_codeact_workflow

                workflow = create_codeact_workflow(logic=logic, on_step=None)
        except Exception as e:
            self._print(_style("Memory error: failed to build dry workflow.", _C.YELLOW, enabled=self._color) + f" {e}")
            return

        class _Ctx:
            @staticmethod
            def now_iso() -> str:  # pragma: no cover
                return _now_iso()

        ctx = _Ctx()

        visited = set()
        node_id = start_node
        llm_payload: Optional[Dict[str, Any]] = None
        llm_payload_source = "next llm_call (derived)"
        for _ in range(100):
            if not node_id or node_id in visited:
                break
            visited.add(node_id)
            sim_run.current_node = node_id
            try:
                handler = workflow.get_node(node_id)
            except Exception:
                break
            plan = handler(sim_run, ctx)
            effect = getattr(plan, "effect", None)
            if effect is None:
                node_id = str(plan.next_node or "")
                continue
            etype = effect.type.value if hasattr(effect.type, "value") else str(effect.type)
            if str(etype) == "llm_call" and isinstance(effect.payload, dict):
                llm_payload = dict(effect.payload)
                break
            break

        def _usage_prompt_tokens(usage: Dict[str, Any]) -> Optional[int]:
            raw = usage.get("prompt_tokens")
            if raw is None:
                raw = usage.get("input_tokens")
            if raw is None:
                return None
            try:
                value = int(raw)
            except Exception:
                return None
            return value if value >= 0 else None

        def _usage_completion_tokens(usage: Dict[str, Any]) -> Optional[int]:
            raw = usage.get("completion_tokens")
            if raw is None:
                raw = usage.get("output_tokens")
            if raw is None:
                return None
            try:
                value = int(raw)
            except Exception:
                return None
            return value if value >= 0 else None

        def _usage_total_tokens(usage: Dict[str, Any]) -> Optional[int]:
            raw = usage.get("total_tokens")
            if raw is None:
                return None
            try:
                value = int(raw)
            except Exception:
                return None
            return value if value >= 0 else None

        # Prefer usage from the last executed LLM_CALL when available (provider-native tokenization).
        runtime_ns = state.vars.get("_runtime") if isinstance(state.vars.get("_runtime"), dict) else {}
        traces = runtime_ns.get("node_traces") if isinstance(runtime_ns, dict) else None
        last_trace_payload: Optional[Dict[str, Any]] = None
        last_trace_usage: Optional[Dict[str, Any]] = None
        last_trace_ts = ""
        if isinstance(traces, dict):
            for node_trace in traces.values():
                if not isinstance(node_trace, dict):
                    continue
                steps = node_trace.get("steps")
                if not isinstance(steps, list):
                    continue
                for step in steps:
                    if not isinstance(step, dict):
                        continue
                    eff = step.get("effect")
                    if not isinstance(eff, dict):
                        continue
                    if str(eff.get("type") or "") != "llm_call":
                        continue
                    ts = str(step.get("ts") or "")
                    payload = eff.get("payload") if isinstance(eff.get("payload"), dict) else None
                    if not ts or payload is None:
                        continue
                    if ts > last_trace_ts:
                        last_trace_ts = ts
                        last_trace_payload = dict(payload)
                        result = step.get("result")
                        if isinstance(result, dict) and isinstance(result.get("usage"), dict):
                            last_trace_usage = dict(result["usage"])
                        else:
                            last_trace_usage = None

        # Fallback: if we cannot derive the next LLM_CALL (e.g., run is mid-tool),
        # use the most recent executed LLM_CALL from runtime traces.
        if llm_payload is None and last_trace_payload is not None:
            llm_payload = dict(last_trace_payload)
            llm_payload_source = "last llm_call (runtime trace)"

        effective_model = str((llm_payload or {}).get("model") or self._model or "").strip() or None

        # Show both (a) the model's capability and (b) the run/session's effective max.
        model_cap_max_tokens = 32768
        try:
            from abstractcore.architectures.detection import get_model_capabilities

            caps = get_model_capabilities(str(effective_model or self._model or ""))
            model_cap_max_tokens = int(caps.get("max_tokens", 32768) or 32768)
        except Exception:
            model_cap_max_tokens = 32768

        max_tokens: int
        max_tokens = 0
        try:
            if max_tokens_raw is not None:
                max_tokens = int(max_tokens_raw)
        except Exception:
            max_tokens = 0
        if max_tokens <= 0:
            try:
                if self._max_tokens is not None:
                    max_tokens = int(self._max_tokens)
            except Exception:
                max_tokens = 0
        if max_tokens <= 0:
            max_tokens = model_cap_max_tokens if model_cap_max_tokens > 0 else 32768

        def count_tokens(text: str) -> int:
            try:
                return int(TokenUtils.count_tokens(str(text or ""), model=effective_model))
            except Exception:
                # Extremely conservative fallback
                return max(1, len(str(text or "")) // 4) if str(text or "") else 0

        # Per-component Active Memory accounting.
        breakdown = compute_active_memory_token_breakdown(
            state.vars,
            token_counter=count_tokens,
            include_tools_summary=True,
        )
        components = breakdown.get("components") if isinstance(breakdown.get("components"), dict) else {}
        active_mem_max = int(breakdown.get("active_memory_max_tokens") or 0)

        # Total next-LLM prompt accounting.
        sys_prompt = str((llm_payload or {}).get("system_prompt") or "")
        prompt = str((llm_payload or {}).get("prompt") or "")
        tools = (llm_payload or {}).get("tools") or []

        system_tokens = count_tokens(sys_prompt) if sys_prompt else 0
        prompt_tokens = count_tokens(prompt) if prompt else 0

        tool_prompt_tokens = 0
        if isinstance(tools, list) and tools:
            try:
                from abstractcore.tools.handler import UniversalToolHandler

                handler = UniversalToolHandler(str(effective_model or ""))
                tool_prompt = handler.format_tools_prompt(tools)
                tool_prompt_tokens = count_tokens(tool_prompt) if tool_prompt else 0
            except Exception:
                # Fallback: count raw JSON if formatting is unavailable.
                tool_prompt_tokens = count_tokens(json.dumps(tools, ensure_ascii=False, sort_keys=True))

        estimated_total_used = system_tokens + prompt_tokens + tool_prompt_tokens

        provider_prompt_used: Optional[int] = None
        if isinstance(last_trace_usage, dict):
            provider_prompt_used = _usage_prompt_tokens(last_trace_usage)

        total_used = provider_prompt_used if isinstance(provider_prompt_used, int) else estimated_total_used

        def fmt(n: int) -> str:
            try:
                return f"{int(n):,}"
            except Exception:
                return str(n)

        def bar(*, used: int, cap: int, width: int = 26) -> str:
            used_i = max(0, int(used))
            cap_i = max(0, int(cap))
            if cap_i <= 0:
                return "[" + (" " * width) + "]"
            ratio = min(1.0, float(used_i) / float(cap_i))
            filled = int(round(ratio * width))
            filled = max(0, min(width, filled))
            empty = width - filled

            if ratio < 0.70:
                color = _C.GREEN
            elif ratio < 0.90:
                color = _C.YELLOW
            else:
                color = _C.RED

            fill = _style("█" * filled, color, enabled=self._color)
            rest = _style("░" * empty, _C.DIM, enabled=self._color)
            return "[" + fill + rest + "]"

        label_colors = {
            "persona": _C.BLUE,
            "memory_organization": _C.MAGENTA,
            "tools": _C.GREEN,
            "current_tasks": _C.CYAN,
            "current_context": _C.CYAN,
            "critical_insights": _C.ORANGE,
            "key_history": _C.YELLOW,
            "total": _C.CYAN,
        }

        order = [
            ("persona", "persona"),
            ("memory_organization", "memory_organization"),
            ("tools", "tools"),
            ("current_tasks", "current_tasks"),
            ("current_context", "current_context"),
            ("critical_insights", "critical_insights"),
            ("key_history", "key_history"),
        ]

        self._print(_style("\nMemory", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))
        model_text = effective_model or str(self._model or "")
        self._print(_style(f"Model: {model_text}    Max tokens: {fmt(max_tokens)} tk", _C.DIM, enabled=self._color))
        self._print(_style(f"Model capability: {fmt(model_cap_max_tokens)} tk", _C.DIM, enabled=self._color))
        if isinstance(provider_prompt_used, int):
            self._print(_style("Total bar source: last llm_call usage (provider)", _C.DIM, enabled=self._color))
        else:
            self._print(_style(f"Total bar source: {llm_payload_source}", _C.DIM, enabled=self._color))
        if active_mem_max > 0:
            self._print(_style(f"Active Memory budget: {fmt(active_mem_max)} tokens", _C.DIM, enabled=self._color))
        if isinstance(last_trace_usage, dict):
            p = _usage_prompt_tokens(last_trace_usage)
            c = _usage_completion_tokens(last_trace_usage)
            t = _usage_total_tokens(last_trace_usage)
            parts: list[str] = []
            if p is not None:
                parts.append(f"prompt={fmt(p)}")
            if c is not None:
                parts.append(f"completion={fmt(c)}")
            if t is not None:
                parts.append(f"total={fmt(t)}")
            if parts:
                self._print(_style(f"Last LLM usage: {'  '.join(parts)} tk", _C.DIM, enabled=self._color))
        if isinstance(provider_prompt_used, int):
            self._print(_style(f"Estimated (for reference): {fmt(estimated_total_used)} tk from {llm_payload_source}", _C.DIM, enabled=self._color))
        self._print("")

        # Render component bars.
        label_width = max(len(lbl) for _, lbl in order + [("total", "total")]) + 2
        for cid, label in order:
            comp = components.get(cid) if isinstance(components, dict) else None
            used = int(comp.get("used_tokens") or 0) if isinstance(comp, dict) else 0
            cap = int(comp.get("max_tokens") or 0) if isinstance(comp, dict) else 0
            pct_used = (float(used) / float(cap)) if cap > 0 else (1.0 if used > 0 else 0.0)
            pct_total = (float(used) / float(max_tokens)) if max_tokens > 0 else 0.0
            label_styled = _style(label.ljust(label_width), label_colors.get(cid, _C.CYAN), enabled=self._color)
            line = (
                f"{label_styled} {bar(used=used, cap=cap)}  "
                f"{fmt(used)}/{fmt(cap)} tk ({pct_used*100:0.0f}% cap, {pct_total*100:0.1f}% total)"
            )
            self._print(line)

        # Extra padding before total.
        self._print("")

        total_pct = (float(total_used) / float(max_tokens)) if max_tokens > 0 else 0.0
        total_label = _style("total".ljust(label_width), label_colors.get("total", _C.CYAN), _C.BOLD, enabled=self._color)
        self._print(
            f"{total_label} {bar(used=total_used, cap=max_tokens)}  {fmt(total_used)}/{fmt(max_tokens)} tk ({total_pct*100:0.0f}%)"
        )

        details = f"system={fmt(system_tokens)}  prompt={fmt(prompt_tokens)}  tools={fmt(tool_prompt_tokens)}"
        self._print(_style(details, _C.DIM, enabled=self._color))
        self._print(_style("Note: token counts are best-effort estimates; exact billing varies by provider/tool-calling mode.", _C.DIM, enabled=self._color))

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

    def _handle_context(self, raw: str) -> None:
        """Show the exact context that will be sent with the next LLM call.

        Usage:
          /context [--json-only] [--derived]
        """
        import copy
        import shlex
        import uuid

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        json_only = False
        derived = False
        for p in parts:
            if p in ("--json", "--json-only"):
                json_only = True
                continue
            if p in ("--derived", "--reconstructed"):
                derived = True
                continue
            self._print(_style(f"Unknown flag: {p}", _C.YELLOW, enabled=self._color))
            self._print(_style("Usage: /context [--json-only] [--derived]", _C.DIM, enabled=self._color))
            return

        state = self._safe_get_state()

        # If there's no active run (or the last run already completed), show the session context
        # that will seed the next /task.
        if state is None or not hasattr(state, "vars") or getattr(state, "status", None) in (
            self._RunStatus.COMPLETED,
            self._RunStatus.FAILED,
            self._RunStatus.CANCELLED,
        ):
            payload: Dict[str, Any] = {
                "agent_kind": self._agent_kind,
                "provider": self._provider,
                "model": self._model,
                "note": "No active run. This is the current session context that will be included in the next /task.",
                "tip": "Use /llm to inspect verbatim LLM_CALL payloads from the last run (from runtime node_traces).",
                "session_messages": list(self._agent.session_messages or []),
            }
            if state is not None and hasattr(state, "run_id") and hasattr(state, "status"):
                status_val = getattr(getattr(state, "status", None), "value", None)
                payload["last_run"] = {"run_id": getattr(state, "run_id", None), "status": status_val or str(state.status)}
                out = getattr(state, "output", None)
                if isinstance(out, dict):
                    last_out: Dict[str, Any] = {}
                    if "answer" in out:
                        last_out["answer"] = out.get("answer")
                    if "iterations" in out:
                        last_out["iterations"] = out.get("iterations")
                    if last_out:
                        payload["last_run_output"] = last_out

                # Small trace summary to help debug repeated tool calls.
                runtime_ns = state.vars.get("_runtime") if isinstance(state.vars, dict) else None
                traces = runtime_ns.get("node_traces") if isinstance(runtime_ns, dict) else None
                if isinstance(traces, dict) and traces:
                    counts: Dict[str, int] = {}
                    tool_steps: List[Dict[str, Any]] = []
                    llm_steps: List[Dict[str, Any]] = []
                    llm_steps_verbatim: List[Dict[str, Any]] = []
                    tool_steps_verbatim: List[Dict[str, Any]] = []
                    for node_trace in traces.values():
                        if not isinstance(node_trace, dict):
                            continue
                        steps = node_trace.get("steps")
                        if not isinstance(steps, list):
                            continue
                        for step in steps:
                            if not isinstance(step, dict):
                                continue
                            eff = step.get("effect")
                            if not isinstance(eff, dict):
                                continue
                            etype = str(eff.get("type") or "")
                            counts[etype] = int(counts.get(etype, 0) or 0) + 1

                            if etype == "llm_call":
                                result = step.get("result") if isinstance(step.get("result"), dict) else {}
                                llm_steps.append(
                                    {
                                        "ts": step.get("ts"),
                                        "node_id": step.get("node_id"),
                                        "status": step.get("status"),
                                        "finish_reason": result.get("finish_reason"),
                                        "model": result.get("model"),
                                        "reasoning": result.get("reasoning"),
                                        "content": result.get("content"),
                                        "tool_calls": result.get("tool_calls"),
                                    }
                                )
                                meta = result.get("metadata") if isinstance(result.get("metadata"), dict) else {}
                                runtime_obs = meta.get("_runtime_observability") if isinstance(meta, dict) else None
                                captured = (
                                    runtime_obs.get("llm_generate_kwargs")
                                    if isinstance(runtime_obs, dict)
                                    else None
                                )
                                llm_steps_verbatim.append(
                                    {
                                        "ts": step.get("ts"),
                                        "node_id": step.get("node_id"),
                                        "status": step.get("status"),
                                        "duration_ms": step.get("duration_ms"),
                                        "llm_call_payload": eff.get("payload") if isinstance(eff.get("payload"), dict) else {},
                                        "llm_generate_kwargs_captured": captured,
                                        "result": result,
                                    }
                                )
                                continue
                            if etype != "tool_calls":
                                continue
                            pl = eff.get("payload") if isinstance(eff.get("payload"), dict) else {}
                            tcs = pl.get("tool_calls") if isinstance(pl, dict) else None
                            if not isinstance(tcs, list):
                                tcs = []
                            tool_steps.append(
                                {
                                    "ts": step.get("ts"),
                                    "node_id": step.get("node_id"),
                                    "status": step.get("status"),
                                    "tool_calls": tcs,
                                }
                            )
                            tool_steps_verbatim.append(
                                {
                                    "ts": step.get("ts"),
                                    "node_id": step.get("node_id"),
                                    "status": step.get("status"),
                                    "duration_ms": step.get("duration_ms"),
                                    "tool_calls_payload": pl,
                                    "result": step.get("result") if isinstance(step.get("result"), dict) else {},
                                    "error": step.get("error"),
                                }
                            )

                    payload["last_run_trace_summary"] = {
                        "counts_by_effect_type": dict(counts),
                        "tool_calls_steps": tool_steps,
                        "llm_call_steps": llm_steps,
                    }
                    payload["last_run_traces_verbatim"] = {
                        "llm_call_steps": llm_steps_verbatim,
                        "tool_calls_steps": tool_steps_verbatim,
                    }

            text = json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=False, default=str)
            copy_id = f"context_{uuid.uuid4().hex}"
            self._ui.register_copy_payload(copy_id, text)
            self._print(_style("\nContext (next /task seed)", _C.CYAN, _C.BOLD, enabled=self._color))
            self._print(f"[[COPY:{copy_id}]]")
            self._print(_style("─" * 80, _C.DIM, enabled=self._color))
            self._print(text)
            return

        sim_run = copy.deepcopy(state)

        start_node = str(getattr(sim_run, "current_node", "") or "")

        # Build a "dry" workflow that doesn't emit UI events (on_step=None).
        logic = getattr(self._agent, "logic", None)
        if logic is None:
            self._print(_style("Context error: agent logic is not available.", _C.YELLOW, enabled=self._color))
            return

        try:
            if self._agent_kind == "react":
                from abstractagent.adapters.react_runtime import create_react_workflow

                workflow = create_react_workflow(logic=logic, on_step=None)
            else:
                from abstractagent.adapters.codeact_runtime import create_codeact_workflow

                workflow = create_codeact_workflow(logic=logic, on_step=None)
        except Exception as e:
            self._print(_style("Context error: failed to build dry workflow.", _C.YELLOW, enabled=self._color) + f" {e}")
            return

        class _Ctx:
            @staticmethod
            def now_iso() -> str:  # pragma: no cover
                return _now_iso()

        ctx = _Ctx()

        visited = set()
        node_id = start_node
        next_effect: Dict[str, Any] = {}

        for _ in range(100):
            if not node_id:
                next_effect = {"kind": "error", "error": "empty start node"}
                break
            if node_id in visited:
                next_effect = {"kind": "error", "error": f"loop detected at node '{node_id}'"}
                break
            visited.add(node_id)

            sim_run.current_node = node_id
            try:
                handler = workflow.get_node(node_id)
            except Exception as e:
                next_effect = {"kind": "error", "error": f"unknown node '{node_id}': {e}"}
                break

            plan = handler(sim_run, ctx)

            if getattr(plan, "complete_output", None) is not None:
                next_effect = {"kind": "complete", "node_id": plan.node_id, "complete_output": plan.complete_output}
                break

            effect = getattr(plan, "effect", None)
            if effect is None:
                if not plan.next_node:
                    next_effect = {"kind": "error", "node_id": plan.node_id, "error": "node returned no effect and no next_node"}
                    break
                node_id = str(plan.next_node)
                continue

            etype = effect.type.value if hasattr(effect.type, "value") else str(effect.type)
            next_effect = {
                "kind": "effect",
                "node_id": plan.node_id,
                "type": str(etype),
                "next_node": plan.next_node,
                "result_key": effect.result_key,
                "payload": dict(effect.payload or {}),
            }
            break

        stored_messages = self._messages_from_state(sim_run)
        try:
            from abstractruntime.memory.active_context import ActiveContextPolicy

            active_messages_view = ActiveContextPolicy.select_active_messages_for_llm_from_run(sim_run)
        except Exception:
            active_messages_view = []

        waiting_info: Optional[Dict[str, Any]] = None
        wait_state = getattr(state, "waiting", None)
        if wait_state is not None:
            reason = getattr(wait_state, "reason", None)
            waiting_info = {
                "reason": reason.value if hasattr(reason, "value") else (str(reason) if reason is not None else None),
                "wait_key": getattr(wait_state, "wait_key", None),
                "resume_to_node": getattr(wait_state, "resume_to_node", None),
            }

        out: Dict[str, Any] = {
            "agent_kind": self._agent_kind,
            "provider": self._provider,
            "model": self._model,
            "run": {
                "run_id": getattr(state, "run_id", None),
                "status": getattr(getattr(state, "status", None), "value", None) or str(getattr(state, "status", "")),
                "current_node": getattr(state, "current_node", None),
                "waiting": waiting_info,
            },
            "context": {
                "stored_messages": stored_messages,
                "active_messages_view": active_messages_view,
            },
            "next_effect": next_effect,
        }

        if next_effect.get("type") == "llm_call":
            llm_payload_raw = next_effect.get("payload")
            out["llm_call_payload"] = llm_payload_raw

            # Anything beyond the durable LLM_CALL payload is *derived* (not yet sent).
            # Keep derived fields opt-in for debugging to avoid confusing "exact" vs "reconstructed".
            if derived and isinstance(llm_payload_raw, dict):
                try:
                    from abstractcore.tools.handler import UniversalToolHandler

                    handler = UniversalToolHandler(str(self._model or ""))
                    tool_prompt = handler.format_tools_prompt(llm_payload_raw.get("tools") or [])
                except Exception:
                    tool_prompt = ""
                out["derived_tool_prompt"] = tool_prompt

        text = json.dumps(out, ensure_ascii=False, indent=2, sort_keys=False, default=str)
        copy_id = f"context_{uuid.uuid4().hex}"
        self._ui.register_copy_payload(copy_id, text)

        self._print(_style("\nContext (next LLM call)", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(f"[[COPY:{copy_id}]]")
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))
        self._print(text)

        if json_only:
            return

        llm_payload = out.get("llm_call_payload")
        if not isinstance(llm_payload, dict):
            return

        sys_prompt = llm_payload.get("system_prompt")
        if isinstance(sys_prompt, str) and sys_prompt:
            sid = f"context_system_{uuid.uuid4().hex}"
            self._ui.register_copy_payload(sid, sys_prompt)
            self._print(_style("\nSystem prompt (verbatim)", _C.CYAN, _C.BOLD, enabled=self._color))
            self._print(f"[[COPY:{sid}]]")
            self._print(_style("─" * 80, _C.DIM, enabled=self._color))
            self._print(sys_prompt)

        prompt = llm_payload.get("prompt")
        if isinstance(prompt, str) and prompt:
            pid = f"context_prompt_{uuid.uuid4().hex}"
            self._ui.register_copy_payload(pid, prompt)
            self._print(_style("\nPrompt (verbatim)", _C.CYAN, _C.BOLD, enabled=self._color))
            self._print(f"[[COPY:{pid}]]")
            self._print(_style("─" * 80, _C.DIM, enabled=self._color))
            self._print(prompt)
        if derived:
            tool_prompt = out.get("derived_tool_prompt")
            if isinstance(tool_prompt, str) and tool_prompt.strip():
                tid = f"context_tool_prompt_{uuid.uuid4().hex}"
                self._ui.register_copy_payload(tid, tool_prompt)
                self._print(_style("\nDerived tool prompt (not yet sent)", _C.CYAN, _C.BOLD, enabled=self._color))
                self._print(f"[[COPY:{tid}]]")
                self._print(_style("─" * 80, _C.DIM, enabled=self._color))
                self._print(tool_prompt)

    def _handle_llm(self, raw: str) -> None:
        """Show verbatim LLM_CALL payloads captured by AbstractRuntime.

        Source of truth: `RunState.vars["_runtime"]["node_traces"]`.

        Usage:
          /llm [--last] [--verbatim] [--json-only] [--save <path>]
        """
        import shlex
        import uuid
        from pathlib import Path

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        json_only = False
        last_only = False
        verbatim = False
        save_path: Optional[str] = None
        i = 0
        while i < len(parts):
            p = parts[i]
            if p in ("--json", "--json-only"):
                json_only = True
                i += 1
                continue
            if p in ("--last", "--latest"):
                last_only = True
                i += 1
                continue
            if p in ("--verbatim", "--context", "--messages"):
                verbatim = True
                i += 1
                continue
            if p in ("--save", "--out", "--output"):
                if i + 1 >= len(parts):
                    self._print(_style("Usage: /llm [--last] [--verbatim] [--json-only] [--save <path>]", _C.DIM, enabled=self._color))
                    return
                save_path = parts[i + 1]
                i += 2
                continue
            self._print(_style(f"Unknown flag: {p}", _C.YELLOW, enabled=self._color))
            self._print(_style("Usage: /llm [--last] [--verbatim] [--json-only] [--save <path>]", _C.DIM, enabled=self._color))
            return

        state = self._safe_get_state()
        if state is None or not hasattr(state, "vars"):
            self._print(_style("No run loaded. Use /resume or start a task first.", _C.DIM, enabled=self._color))
            return

        runtime_ns = state.vars.get("_runtime") if isinstance(state.vars, dict) else None
        traces = runtime_ns.get("node_traces") if isinstance(runtime_ns, dict) else None
        if not isinstance(traces, dict) or not traces:
            self._print(_style("No runtime node_traces found for this run.", _C.DIM, enabled=self._color))
            return

        llm_steps: List[Dict[str, Any]] = []
        for node_trace in traces.values():
            if not isinstance(node_trace, dict):
                continue
            steps = node_trace.get("steps")
            if not isinstance(steps, list):
                continue
            for step in steps:
                if not isinstance(step, dict):
                    continue
                eff = step.get("effect")
                if not isinstance(eff, dict):
                    continue
                if str(eff.get("type") or "") == "llm_call":
                    llm_steps.append(step)

        if not llm_steps:
            self._print(_style("No llm_call steps found in node_traces.", _C.DIM, enabled=self._color))
            return

        llm_steps.sort(key=lambda d: str(d.get("ts") or ""))
        if last_only:
            llm_steps = [llm_steps[-1]]

        calls_out: List[Dict[str, Any]] = []
        for idx, step in enumerate(llm_steps, 1):
            eff = step.get("effect") if isinstance(step.get("effect"), dict) else {}
            payload = eff.get("payload") if isinstance(eff.get("payload"), dict) else {}
            result = step.get("result") if isinstance(step.get("result"), dict) else {}

            captured_kwargs = None
            meta = result.get("metadata") if isinstance(result.get("metadata"), dict) else None
            runtime_obs = meta.get("_runtime_observability") if isinstance(meta, dict) else None
            if isinstance(runtime_obs, dict):
                captured_kwargs = runtime_obs.get("llm_generate_kwargs")

            calls_out.append(
                {
                    "index": idx,
                    "ts": step.get("ts"),
                    "node_id": step.get("node_id"),
                    "status": step.get("status"),
                    "duration_ms": step.get("duration_ms"),
                    "llm_call_payload": payload,
                    "llm_generate_kwargs_captured": captured_kwargs,
                    "result": result,
                }
            )

        out: Dict[str, Any] = {
            "run_id": getattr(state, "run_id", None),
            "provider": self._provider,
            "model": self._model,
            "llm_calls": calls_out,
        }

        text = json.dumps(out, ensure_ascii=False, indent=2, sort_keys=False, default=str)

        if save_path:
            try:
                path = Path(save_path).expanduser()
                if not path.is_absolute():
                    path = Path.cwd() / path
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text, encoding="utf-8")
                self._print(_style(f"✅ Saved verbatim LLM payloads to {path}", _C.DIM, enabled=self._color))
            except Exception as e:
                self._print(_style(f"❌ Failed to save: {e}", _C.DIM, enabled=self._color))

        copy_id = f"llm_{uuid.uuid4().hex}"
        self._ui.register_copy_payload(copy_id, text)

        def _provider_context_text(provider_req: Any) -> str:
            if not isinstance(provider_req, dict):
                return ""
            payload = provider_req.get("payload") if isinstance(provider_req.get("payload"), dict) else {}
            messages = payload.get("messages")
            if not isinstance(messages, list) or not messages:
                return ""

            blocks: List[str] = []
            for i_msg, msg in enumerate(messages, 1):
                if not isinstance(msg, dict):
                    continue
                role = str(msg.get("role") or "")
                content = msg.get("content")
                if content is None:
                    content_str = ""
                else:
                    content_str = str(content)
                blocks.append(f"--- message {i_msg} role={role or 'unknown'} ---")
                blocks.append(content_str)
            return "\n".join(blocks).rstrip()

        self._print(_style("\nLLM calls (runtime; verbatim payloads)", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(f"[[COPY:{copy_id}]]")
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))

        if json_only:
            self._print(text)
            return

        # Human-scannable rendering: one block per call with separate copy payloads.
        for call in calls_out:
            idx = call.get("index")
            ts = call.get("ts")
            node_id = call.get("node_id")
            status = call.get("status")
            dur = call.get("duration_ms")
            header = f"LLM call #{idx} ({status}) node={node_id} ts={ts}"
            if isinstance(dur, (int, float)):
                header += f" duration_ms={dur:.1f}"
            self._print(_style("\n" + header, _C.CYAN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 80, _C.DIM, enabled=self._color))

            res_payload = call.get("result") or {}
            meta = res_payload.get("metadata") if isinstance(res_payload, dict) else None
            provider_req = meta.get("_provider_request") if isinstance(meta, dict) else None

            if verbatim:
                verb_text = _provider_context_text(provider_req)
                if not verb_text:
                    self._print(_style("Provider request context unavailable for this call.", _C.DIM, enabled=self._color))
                else:
                    vid = f"llm_ctx_{uuid.uuid4().hex}"
                    self._ui.register_copy_payload(vid, verb_text)
                    self._print(_style("Provider context (verbatim; exact messages sent)", _C.DIM, enabled=self._color))
                    self._print(f"[[COPY:{vid}]]")
                    self._print(verb_text)
                continue

            # 1) Durable runtime payload (what the runtime scheduled)
            runtime_payload = call.get("llm_call_payload") or {}
            runtime_text = json.dumps(runtime_payload, ensure_ascii=False, indent=2, sort_keys=False, default=str)
            rid = f"llm_runtime_{uuid.uuid4().hex}"
            self._ui.register_copy_payload(rid, runtime_text)
            self._print(_style("Runtime LLM_CALL payload (durable)", _C.DIM, enabled=self._color))
            self._print(f"[[COPY:{rid}]]")
            self._print(runtime_text)

            # 2) Captured generate kwargs (closest view at AbstractCore boundary, if present)
            captured = call.get("llm_generate_kwargs_captured")
            if captured is not None:
                cap_text = json.dumps(captured, ensure_ascii=False, indent=2, sort_keys=False, default=str)
                cap_id = f"llm_captured_{uuid.uuid4().hex}"
                self._ui.register_copy_payload(cap_id, cap_text)
                self._print(_style("\nGenerate kwargs (captured at AbstractCore boundary)", _C.DIM, enabled=self._color))
                self._print(f"[[COPY:{cap_id}]]")
                self._print(cap_text)

            # 3) Normalized response (content/tool_calls/metadata).
            # Remove provider-request echo from this view to avoid confusing "response" with "request".
            res_view = res_payload
            if isinstance(res_payload, dict):
                try:
                    res_view = dict(res_payload)
                    meta_view = res_view.get("metadata") if isinstance(res_view.get("metadata"), dict) else None
                    if isinstance(meta_view, dict) and "_provider_request" in meta_view:
                        meta_clean = dict(meta_view)
                        meta_clean.pop("_provider_request", None)
                        res_view["metadata"] = meta_clean
                except Exception:
                    res_view = res_payload
            res_text = json.dumps(res_view, ensure_ascii=False, indent=2, sort_keys=False, default=str)
            res_id = f"llm_res_{uuid.uuid4().hex}"
            self._ui.register_copy_payload(res_id, res_text)
            self._print(_style("\nResponse (normalized)", _C.DIM, enabled=self._color))
            self._print(f"[[COPY:{res_id}]]")
            self._print(res_text)

            # Provider-level observability: some AbstractCore providers attach the exact HTTP/client
            # request payload they sent under metadata._provider_request.
            if provider_req is not None:
                prov_text = json.dumps(provider_req, ensure_ascii=False, indent=2, sort_keys=False, default=str)
                prov_id = f"llm_provider_{uuid.uuid4().hex}"
                self._ui.register_copy_payload(prov_id, prov_text)
                self._print(_style("\nProvider request (verbatim; as sent)", _C.DIM, enabled=self._color))
                self._print(f"[[COPY:{prov_id}]]")
                self._print(prov_text)

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
            "  /tools              List/configure tool allowlist [saved]\n"
            "  /tool-specs         Show full tool schemas (params)\n"
            "  /status             Show current run status\n"
            "  /auto-accept        Toggle auto-accept for tools [saved]\n"
            "  /plan [on|off]      Toggle Plan mode (TODO list first) [saved]\n"
            "  /review ...         Toggle Review mode (self-check) [saved]\n"
            "                     - /review [on|off] [max_rounds]\n"
            "                     - /review rounds <N>\n"
            "  /max-tokens [N]     Show or set max tokens (-1 = auto) [saved]\n"
            "  /max-messages [N]   Show or set max history messages (-1 = unlimited) [saved]\n"
            "  /memory             Show current token usage breakdown\n"
            "  /compact [mode]     Compress conversation context [light|standard|heavy]\n"
            "  /spans              List archived conversation spans (from /compact)\n"
            "  /expand <span>      Expand an archived span (--show, --into-context)\n"
            "  /recall [opts]      Recall spans by time/tags/query (--into-context)\n"
            "  /vars [path]        Inspect run vars (scratchpad, _runtime, ...)\n"
            "  /context            Show the exact context for the next LLM call\n"
            "  /llm                Show verbatim LLM_CALL payloads for the run\n"
            "  /remember <note>    Store a durable memory note (tags + provenance)\n"
            "  /mouse              Toggle mouse mode (wheel scroll vs terminal selection)\n"
            "  /flow ...           Run AbstractFlow workflows inside this REPL\n"
            "                     - /flow run <flow_id_or_path> [--verbosity none|default|full] [--key value ...]\n"
            "                     - /flow resume [--verbosity none|default|full] [--wait-until]\n"
            "                     - /flow pause | resume-run | cancel\n"
            "                     - Example: /flow run deep-research-pro --query \"who are you?\" --max_web_search 10\n"
            "  /copy ...           Copy messages to clipboard\n"
            "                     - /copy user [turn] | assistant [turn] | turn <N>\n"
            "  /history [N]        Show recent conversation history\n"
            "  /resume             Resume the saved/attached run\n"
            "  /pause              Pause the current run (durable)\n"
            "  /cancel             Cancel the current run (durable)\n"
            "  /clear              Clear memory and clear the screen\n"
            "  /snapshot save <n>  Save current state as named snapshot\n"
            "  /snapshot load <n>  Load snapshot by name\n"
            "  /snapshot list      List available snapshots\n"
            "  /quit               Exit\n"
            "\nTasks:\n"
            "  /task <text>        Start a new task\n"
        )

    def _handle_mouse_toggle(self) -> None:
        enabled = self._ui.toggle_mouse_support()
        if enabled:
            self._print(_style("Mouse mode: ON (wheel scroll enabled).", _C.DIM, enabled=self._color))
        else:
            self._print(_style("Mouse mode: OFF (terminal selection enabled).", _C.DIM, enabled=self._color))

    def _show_tools(self) -> None:
        self._print(_style("\nTool schemas", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        rendered: Dict[str, _ToolSpec] = {}
        logic = getattr(self._agent, "logic", None)
        tool_defs = getattr(logic, "tools", None) if logic is not None else None
        if isinstance(tool_defs, list):
            for t in tool_defs:
                name = getattr(t, "name", None)
                if not isinstance(name, str) or not name.strip():
                    continue
                rendered[name.strip()] = _ToolSpec(
                    name=name.strip(),
                    description=str(getattr(t, "description", "") or ""),
                    parameters=dict(getattr(t, "parameters", None) or {}),
                )
        if not rendered:
            rendered = dict(self._tool_specs or {})

        for name, spec in sorted(rendered.items()):
            params = ", ".join(sorted((spec.parameters or {}).keys()))
            self._print(f"- {name}({params})")
            if spec.description:
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

    def _group_messages_into_turns(self, messages: List[Dict[str, Any]]) -> list[list[Dict[str, Any]]]:
        """Group messages into turns starting at each user message (prompt + following messages)."""
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

        return turns

    def _show_history(self, *, limit: int = 12) -> None:
        import uuid

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

        turns = self._group_messages_into_turns(messages)

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
                if role == "user":
                    mid = _get_message_id(msg) or f"user_{uuid.uuid4().hex}"
                    self._ui.register_copy_payload(mid, content)
                    self._print(self._format_user_prompt_block(content, copy_id=mid))
                    continue

                if role == "tool":
                    meta = msg.get("metadata") if isinstance(msg.get("metadata"), dict) else {}
                    name = meta.get("name") if isinstance(meta, dict) else None
                    label = f"[tool:{name}]" if isinstance(name, str) and name else "[tool]"
                    mid = _get_message_id(msg) or f"tool_{uuid.uuid4().hex}"
                    self._ui.register_copy_payload(mid, f"{label}\n{content}".strip())
                    self._print(f"[[COPY:{mid}]]")
                    self._print(_style(label, _C.DIM, enabled=self._color))
                    self._print(content)
                    continue

                if role == "system":
                    mid = _get_message_id(msg) or f"system_{uuid.uuid4().hex}"
                    self._ui.register_copy_payload(mid, content)
                    self._print(f"[[COPY:{mid}]]")
                    self._print(_style("[system]", _C.DIM, enabled=self._color))
                    self._print(content)
                    continue

                # Default: assistant/other roles (no role prefix; rely on styling/structure).
                mid = _get_message_id(msg) or f"assistant_{uuid.uuid4().hex}"
                self._ui.register_copy_payload(mid, content)
                self._print(f"[[COPY:{mid}]]")
                self._print(content)

        self._print(_style("\n" + "─" * 80, _C.DIM, enabled=self._color))

    def _copy_to_clipboard(self, text: str) -> bool:
        """Best-effort copy to OS clipboard (no truncation)."""
        import shutil
        import subprocess

        value = str(text or "")

        try:
            import pyperclip  # type: ignore

            pyperclip.copy(value)
            return True
        except Exception:
            pass

        try:
            if sys.platform == "darwin" and shutil.which("pbcopy"):
                subprocess.run(["pbcopy"], input=value.encode("utf-8"), check=True)
                return True
        except Exception:
            pass

        try:
            if shutil.which("wl-copy"):
                subprocess.run(["wl-copy"], input=value.encode("utf-8"), check=True)
                return True
        except Exception:
            pass

        try:
            if shutil.which("xclip"):
                subprocess.run(["xclip", "-selection", "clipboard"], input=value.encode("utf-8"), check=True)
                return True
        except Exception:
            pass

        try:
            if shutil.which("xsel"):
                subprocess.run(["xsel", "--clipboard", "--input"], input=value.encode("utf-8"), check=True)
                return True
        except Exception:
            pass

        return False

    def _handle_copy(self, raw: str) -> None:
        """Copy a user/assistant message (or full turn) to clipboard.

        Usage:
          /copy user [turn]
          /copy assistant [turn]
          /copy turn <N>
        """
        import shlex

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        if not parts:
            self._print(_style("Usage: /copy user|assistant [turn]  |  /copy turn <N>", _C.DIM, enabled=self._color))
            return

        state = self._safe_get_state()
        messages = list(self._agent.session_messages or []) if state is None else self._messages_from_state(state)
        turns = self._group_messages_into_turns(messages)
        if not turns:
            self._print("No history yet.")
            return

        def _resolve_turn_index(value: str) -> Optional[int]:
            try:
                idx = int(value)
            except Exception:
                return None
            if idx < 1 or idx > len(turns):
                return None
            return idx - 1  # zero-based

        action = parts[0].strip().lower()

        if action == "turn":
            if len(parts) < 2:
                self._print(_style("Usage: /copy turn <N>", _C.DIM, enabled=self._color))
                return
            turn_idx = _resolve_turn_index(parts[1])
            if turn_idx is None:
                self._print(_style(f"Invalid turn index. Valid range: 1..{len(turns)}", _C.YELLOW, enabled=self._color))
                return

            turn = turns[turn_idx]
            blocks: List[str] = []
            for msg in turn:
                role = str(msg.get("role") or "unknown")
                content = "" if msg.get("content") is None else str(msg.get("content"))
                if role == "tool":
                    meta = msg.get("metadata") if isinstance(msg.get("metadata"), dict) else {}
                    name = meta.get("name") if isinstance(meta, dict) else None
                    label = f"tool[{name}]" if isinstance(name, str) and name else "tool"
                else:
                    label = role
                blocks.append(f"{label}:\n{content}".rstrip())

            payload = "\n\n".join(blocks).strip()
            ok = self._copy_to_clipboard(payload)
            self._print(_style("Copied." if ok else "Copy failed (no clipboard helper found).", _C.DIM, enabled=self._color))
            return

        if action in ("user", "assistant", "ai"):
            role = "assistant" if action in ("assistant", "ai") else "user"
            turn_idx = len(turns) - 1
            if len(parts) >= 2:
                resolved = _resolve_turn_index(parts[1])
                if resolved is None:
                    self._print(_style(f"Invalid turn index. Valid range: 1..{len(turns)}", _C.YELLOW, enabled=self._color))
                    return
                turn_idx = resolved

            turn = turns[turn_idx]
            if role == "user":
                msg = next((m for m in turn if m.get("role") == "user"), None)
                content = "" if not isinstance(msg, dict) or msg.get("content") is None else str(msg.get("content"))
            else:
                chunks = [
                    "" if m.get("content") is None else str(m.get("content"))
                    for m in turn
                    if isinstance(m, dict) and m.get("role") == "assistant"
                ]
                content = "\n\n".join([c for c in chunks if c]).strip()

            if not content.strip():
                self._print(_style(f"No {role} content found for that turn.", _C.YELLOW, enabled=self._color))
                return

            ok = self._copy_to_clipboard(content)
            self._print(_style("Copied." if ok else "Copy failed (no clipboard helper found).", _C.DIM, enabled=self._color))
            return

        self._print(_style("Usage: /copy user|assistant [turn]  |  /copy turn <N>", _C.DIM, enabled=self._color))

    def _clear_screen(self) -> None:
        """Clear the visible UI output area (screen).

        Best-effort: clearing output should never crash the REPL.
        """
        try:
            self._ui.clear_output()
        except Exception:
            pass
        self._output_lines = []

    def _clear_memory(self) -> None:
        """Clear in-memory conversation context and reset to a fresh state.

        Also clears the visible UI output so the user gets an actual clean slate.
        """
        self._clear_screen()
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

    def _run_thread_active(self) -> bool:
        t = self._run_thread
        return t is not None and t.is_alive()

    def _run_in_background(self, run_id: str) -> None:
        rid = str(run_id or "").strip()
        if not rid:
            return

        def _target() -> None:
            try:
                self._run_loop(rid)
            except Exception as e:
                self._ui.clear_spinner()
                self._ui.scroll_to_bottom()
                self._print(_style("\nRun error:", _C.RED, enabled=self._color) + f" {e}")
            finally:
                with self._run_thread_lock:
                    if self._run_thread is threading.current_thread():
                        self._run_thread = None

        with self._run_thread_lock:
            if self._run_thread is not None and self._run_thread.is_alive():
                self._print(_style("A run is already executing. Use /pause or /cancel first.", _C.DIM, enabled=self._color))
                return
            self._run_thread = threading.Thread(target=_target, daemon=True, name="abstractcode-run")
            self._run_thread.start()

    def _attached_run_id(self) -> Optional[str]:
        rid = getattr(self._agent, "run_id", None)
        if isinstance(rid, str) and rid.strip():
            return rid.strip()
        rid2 = self._last_run_id
        if isinstance(rid2, str) and rid2.strip():
            return rid2.strip()
        return None

    def _start(self, task: str) -> None:
        if self._run_thread_active():
            self._print(_style("A run is already executing. Use /pause or /cancel first.", _C.DIM, enabled=self._color))
            return
        # Note: _approve_all_session is NOT reset here - it persists for the entire session
        self._turn_task = str(task or "").strip() or None
        self._turn_trace = []
        run_id = self._agent.start(task, allowed_tools=self._allowed_tools)
        self._last_run_id = run_id
        if self._state_file:
            self._agent.save_state(self._state_file)
        self._run_in_background(run_id)

    def _resume(self) -> None:
        if self._run_thread_active():
            self._print(_style("A run is already executing. Use /pause or /cancel first.", _C.DIM, enabled=self._color))
            return
        if self._agent.run_id is None and self._state_file:
            self._try_load_state()

        run_id = self._agent.run_id
        if run_id is None:
            self._print("No run to resume.")
            return

        self._last_run_id = run_id
        # If paused, unpause first (ADR-0013) then continue.
        try:
            self._runtime.resume_run(run_id)
        except Exception:
            pass
        self._run_in_background(run_id)

    def _pause(self) -> None:
        run_id = self._attached_run_id()
        if run_id is None:
            self._print(_style("No run loaded. Start a task or /resume first.", _C.DIM, enabled=self._color))
            return
        try:
            self._runtime.pause_run(run_id, reason="Paused via AbstractCode")
        except Exception as e:
            self._print(_style("Pause failed:", _C.YELLOW, enabled=self._color) + f" {e}")
            return
        self._print(_style(f"Pause requested (run_id={run_id}).", _C.DIM, enabled=self._color))

    def _cancel(self) -> None:
        run_id = self._attached_run_id()
        if run_id is None:
            self._print(_style("No run loaded. Start a task or /resume first.", _C.DIM, enabled=self._color))
            return
        try:
            self._runtime.cancel_run(run_id, reason="Cancelled via AbstractCode")
        except Exception as e:
            self._print(_style("Cancel failed:", _C.YELLOW, enabled=self._color) + f" {e}")
            return
        self._print(_style(f"Cancel requested (run_id={run_id}).", _C.DIM, enabled=self._color))

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
            state = self._agent.step()

            if state.status == self._RunStatus.COMPLETED:
                self._ui.clear_spinner()
                self._ui.scroll_to_bottom()
                if state.output and isinstance(state.output.get("messages"), list):
                    self._agent.session_messages = list(state.output["messages"])
                # When the run stops due to safety limits (e.g. max iterations), still emit an
                # answer block with a copy button so users can grab partial output + trace.
                if str(getattr(state, "current_node", "") or "") == "max_iterations":
                    iterations = "?"
                    if isinstance(state.output, dict):
                        iterations = str(state.output.get("iterations") or "?")
                    self._print(_style(f"\nMax iterations reached ({iterations}).", _C.YELLOW, enabled=self._color))
                    prompt_text, answer_text = self._extract_latest_turn_prompt_and_answer(state)
                    if isinstance(state.output, dict) and isinstance(state.output.get("answer"), str):
                        answer_text = str(state.output.get("answer") or "")
                    self._print_answer_block(title="ANSWER (partial)", answer_text=answer_text, prompt_text=prompt_text)
                return

            if state.status == self._RunStatus.CANCELLED:
                self._ui.clear_spinner()
                self._ui.scroll_to_bottom()
                self._print(_style("\nRun cancelled. State preserved.", _C.YELLOW, enabled=self._color))
                prompt_text, answer_text = self._extract_latest_turn_prompt_and_answer(state)
                self._print_answer_block(title="ANSWER (partial)", answer_text=answer_text, prompt_text=prompt_text)
                loaded = self._messages_from_state(state)
                if loaded:
                    self._agent.session_messages = loaded
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
                details = wait.details or {}
                if (isinstance(details, dict) and details.get("kind") == "pause") or (
                    isinstance(getattr(wait, "wait_key", None), str) and getattr(wait, "wait_key", None) == f"pause:{run_id}"
                ):
                    self._ui.clear_spinner()
                    self._ui.scroll_to_bottom()
                    self._print(_style("\nPaused. Type '/resume' to continue.", _C.YELLOW, enabled=self._color))
                    prompt_text, answer_text = self._extract_latest_turn_prompt_and_answer(state)
                    self._print_answer_block(title="ANSWER (partial)", answer_text=answer_text, prompt_text=prompt_text)
                    loaded = self._messages_from_state(state)
                    if loaded:
                        self._agent.session_messages = loaded
                    return
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
        auto = bool(self._auto_approve or self._approve_all_session)

        if not auto:
            self._print(_style("\nTool approval required", _C.CYAN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 60, _C.DIM, enabled=self._color))

        approve_all = False
        results: List[Dict[str, Any]] = []

        for tc in tool_calls:
            name = str(tc.get("name", "") or "")
            args = dict(tc.get("arguments") or {})
            call_id = str(tc.get("call_id") or "")

            # Keep approval prompts compact: the agent already printed the tool call itself
            # in the "act" step. Only show a diff preview for edit_file when explicitly
            # approving (no argument dumps).
            if name == "edit_file" and (not auto and not approve_all):
                try:
                    preview_args = dict(args)
                    preview_args["preview_only"] = True
                    preview_out = self._tool_runner.execute(
                        tool_calls=[{"name": name, "arguments": preview_args, "call_id": call_id}]
                    )
                    preview_results = preview_out.get("results") or []
                    if preview_results and isinstance(preview_results[0], dict):
                        preview_raw = preview_results[0].get("output")
                        if preview_raw is None:
                            preview_raw = preview_results[0].get("error")
                        preview_raw = "" if preview_raw is None else str(preview_raw)
                        self._print(_style("preview:", _C.DIM, enabled=self._color))
                        self._print_tool_observation(tool_name=name, raw=preview_raw, indent="  ")
                except Exception:
                    pass

            if not auto and not approve_all:
                while True:
                    choice = self._simple_prompt(f"Approve {name}? [y]es/[n]o/[a]ll/[e]dit/[q]uit: ").lower()
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

            # Additional confirmation for shell execution (skip if auto/approve_all is set)
            if name == "execute_command" and not auto and not approve_all:
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

            # Dedup identical execute_command calls that already succeeded (common model glitch).
            if name == "execute_command":
                cmd = str(args.get("command") or "")
                if cmd and cmd == (self._last_execute_command or ""):
                    prev = self._last_execute_command_result or {}
                    if isinstance(prev, dict) and prev.get("success") is True:
                        cached = dict(prev)
                        cached["call_id"] = call_id
                        # Preserve fidelity but make it obvious this wasn't re-executed.
                        cached_output = cached.get("output")
                        cached["output"] = f"[cached duplicate execute_command]\n{cached_output}"
                        results.append(cached)
                        self._print(_style("Reused cached execute_command result (duplicate).", _C.DIM, enabled=self._color))
                        continue

            # Dedup identical file-mutation calls that already succeeded (common model glitch).
            if name in ("edit_file", "write_file"):
                try:
                    import hashlib

                    material = json.dumps(args, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
                    key = (name, hashlib.sha256(material.encode("utf-8")).hexdigest())
                except Exception:
                    key = (name, str(args))

                if key == self._last_mutating_tool_call_key:
                    prev = self._last_mutating_tool_call_result or {}
                    if isinstance(prev, dict) and prev.get("success") is True:
                        cached = dict(prev)
                        cached["call_id"] = call_id
                        cached_output = cached.get("output")
                        cached["output"] = f"[cached duplicate {name}]\n{cached_output}"
                        results.append(cached)
                        self._print(_style(f"Reused cached {name} result (duplicate).", _C.DIM, enabled=self._color))
                        continue

            single = {"name": name, "arguments": args, "call_id": call_id}
            out = self._tool_runner.execute(tool_calls=[single])
            out_results = out.get("results") or []
            results.extend(out_results)
            if name == "execute_command" and out_results:
                try:
                    self._last_execute_command = str(args.get("command") or "")
                    first = out_results[0]
                    if isinstance(first, dict):
                        self._last_execute_command_result = dict(first)
                except Exception:
                    pass
            if name in ("edit_file", "write_file") and out_results:
                try:
                    import hashlib

                    material = json.dumps(args, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
                    self._last_mutating_tool_call_key = (name, hashlib.sha256(material.encode("utf-8")).hexdigest())
                    first = out_results[0]
                    if isinstance(first, dict):
                        self._last_mutating_tool_call_result = dict(first)
                except Exception:
                    pass

        return {"mode": "executed", "results": results}


 
