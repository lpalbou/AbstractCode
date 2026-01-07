from __future__ import annotations

import json
import os
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Sequence, Tuple

from prompt_toolkit.formatted_text import HTML

from .input_handler import create_prompt_session, create_simple_session
from .fullscreen_ui import FullScreenUI
from .terminal_markdown import TerminalMarkdownRenderer


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
        base_url: Optional[str] = None,
        state_file: Optional[str],
        auto_approve: bool,
        plan_mode: bool = False,
        review_mode: bool = True,
        review_max_rounds: int = 3,
        max_iterations: int,
        max_tokens: Optional[int] = None,
        color: bool,
    ):
        raw_agent = str(agent or "react").strip()
        if not raw_agent:
            raw_agent = "react"
        agent_lower = raw_agent.lower()
        self._workflow_agent_ref: Optional[str] = None
        if agent_lower in ("react", "codeact", "memact"):
            self._agent_kind = agent_lower
        else:
            # Treat any other value as a workflow reference (id/name/path).
            self._agent_kind = raw_agent
            self._workflow_agent_ref = raw_agent
        self._provider = provider
        self._model = model
        self._base_url = str(base_url).strip() if isinstance(base_url, str) and base_url.strip() else None
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
        # Whether to include tool usage examples in the prompted tool section (token-expensive).
        #
        # Default OFF: examples can be very large and materially increase prompt size and latency.
        self._tool_prompt_examples = False
        # Optional MCP server configuration (used for remote tool execution and tool discovery).
        # Shape:
        # - HTTP: {server_id: {"transport":"streamable_http", "url":"...", "headers": {...}}}
        # - stdio: {server_id: {"transport":"stdio", "command":[...], "cwd": "...", "env": {...}}}
        #
        # Backwards compatible: {"url": "...", "headers": {...}} implies streamable_http.
        self._mcp_servers: Dict[str, Dict[str, Any]] = {}
        # Optional session-wide default tool executor (MCP server_id). When set, the
        # tool allowlist can be mapped to `mcp::<server_id>::...` names so tools run
        # on the selected remote machine by default.
        self._tool_executor_server_id: Optional[str] = None
        self._executor_synced_server_ids: set[str] = set()
        # Optional factory for tests to inject a custom McpClient per server_id.
        self._mcp_client_factory: Optional[Callable[[str, Dict[str, Any]], Any]] = None

        # Lazy imports so `abstractcode --help` works even if deps aren't installed.
        try:
            from abstractagent.agents.codeact import CodeActAgent
            from abstractagent.agents.memact import MemActAgent
            from abstractagent.agents.react import ReactAgent
            from abstractagent.tools import execute_python, self_improve
            from abstractcore.tools import ToolDefinition
            from abstractcore.tools.common_tools import (
                list_files,
                search_files,
                analyze_code,
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
            analyze_code,
            read_file,
            write_file,
            edit_file,
            execute_command,
            web_search,
            fetch_url,
            self_improve,
        ]

        if self._workflow_agent_ref is not None:
            # Workflow agents use the "safe" default toolset (same as ReAct).
            self._tools = list(DEFAULT_TOOLS)
            agent_cls = None
        elif self._agent_kind == "react":
            self._tools = list(DEFAULT_TOOLS)
            agent_cls = ReactAgent
        elif self._agent_kind == "memact":
            self._tools = list(DEFAULT_TOOLS)
            agent_cls = MemActAgent
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

        llm_kwargs: Dict[str, Any] = {}
        if self._base_url:
            llm_kwargs["base_url"] = self._base_url

        # Create LLM client for capability queries (used by /max-tokens -1)
        self._llm_client = LocalAbstractCoreLLMClient(
            provider=self._provider,
            model=self._model,
            llm_kwargs=llm_kwargs or None,
        )

        self._runtime = create_local_runtime(
            provider=self._provider,
            model=self._model,
            llm_kwargs=llm_kwargs or None,
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

        if self._workflow_agent_ref is not None:
            try:
                from .workflow_agent import WorkflowAgent
            except Exception as e:
                raise SystemExit(f"Workflow agents require AbstractFlow to be installed/importable.\n\n{e}")

            self._agent = WorkflowAgent(
                runtime=self._runtime,
                flow_ref=self._workflow_agent_ref,
                tools=self._tools,
                on_step=self._on_step,
                max_iterations=self._max_iterations,
                max_tokens=self._max_tokens,
            )
        else:
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
        # Turn-level timing (for per-answer stats).
        self._turn_started_at: Optional[float] = None
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
        # Keep the last started run id so /log can show traces even after completion.
        self._last_run_id: Optional[str] = None
        # Status bar cache (token counting can be expensive; avoid per-frame rescans).
        self._status_cache_key: Optional[Tuple[Any, ...]] = None
        self._status_cache_text: str = ""
        # Run execution happens in a dedicated background thread so the UI worker thread
        # can keep processing commands (/pause, /cancel, /status, ...).
        self._run_thread: Optional[threading.Thread] = None
        self._run_thread_lock = threading.Lock()

    def _reset_repeat_guardrails(self) -> None:
        """
        Reset the simple duplicate-tool-call guardrails.

        These caches exist to avoid a common LLM failure mode where the model repeats the
        exact same mutating tool call in a loop. However, when a user explicitly cancels
        a run (or starts a new run), they are signalling “stop and retry”, so stale cache
        state must not block legitimate retries.
        """
        self._last_execute_command = None
        self._last_execute_command_result = None
        self._last_mutating_tool_call_key = None
        self._last_mutating_tool_call_result = None

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

    def _select_messages_for_llm(self, state: Any) -> List[Dict[str, Any]]:
        """Return the best-effort LLM-visible message view for a state."""
        if state is None or not hasattr(state, "vars") or not isinstance(getattr(state, "vars", None), dict):
            return list(self._agent.session_messages or [])

        try:
            from abstractruntime.memory.active_context import ActiveContextPolicy

            selected = ActiveContextPolicy.select_active_messages_for_llm_from_run(state)
            return list(selected) if isinstance(selected, list) else self._messages_from_state(state)
        except Exception:
            return self._messages_from_state(state)

    def _get_effective_model(self, state: Any) -> str:
        """Resolve the effective model for a state (runtime override > session default)."""
        model = str(self._model or "").strip()
        if state is None or not hasattr(state, "vars") or not isinstance(getattr(state, "vars", None), dict):
            return model
        runtime_ns = state.vars.get("_runtime") if isinstance(state.vars.get("_runtime"), dict) else {}
        override = runtime_ns.get("model") if isinstance(runtime_ns, dict) else None
        if isinstance(override, str) and override.strip():
            return override.strip()
        return model

    def _resolve_context_max_tokens(self, state: Any, *, effective_model: Optional[str] = None) -> int:
        """Resolve the effective context/budget token limit (run > session > model capability)."""
        max_tokens = 0
        if state is not None and hasattr(state, "vars") and isinstance(getattr(state, "vars", None), dict):
            limits = state.vars.get("_limits") if isinstance(state.vars.get("_limits"), dict) else {}
            raw = limits.get("max_tokens")
            try:
                if raw is not None:
                    max_tokens = int(raw)
            except Exception:
                max_tokens = 0

        if max_tokens <= 0:
            try:
                if self._max_tokens is not None:
                    max_tokens = int(self._max_tokens)
            except Exception:
                max_tokens = 0

        if max_tokens <= 0:
            # Fall back to model capabilities (best effort).
            try:
                caps = self._llm_client.get_model_capabilities()
                max_tokens = int(caps.get("max_tokens", 32768) or 32768)
            except Exception:
                max_tokens = 32768

        return max_tokens

    def _estimate_next_prompt_tokens(
        self,
        *,
        state: Any,
        messages: List[Dict[str, Any]],
        effective_model: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Estimate the next prompt token usage (best effort).

        Notes:
        - ReAct/CodeAct are conventional chat-history agents; they do not use Active Memory blocks.
        - MemAct prepends the runtime-rendered memory blocks into the system prompt.
        """
        effective_model = str(effective_model or self._get_effective_model(state) or "").strip() or None

        try:
            from abstractcore.utils.token_utils import TokenUtils
        except Exception:
            TokenUtils = None  # type: ignore[assignment]

        def estimate_tokens(text: str) -> int:
            s = str(text or "")
            if not s:
                return 0
            if TokenUtils is None:
                return max(1, len(s) // 4)
            try:
                return max(0, int(TokenUtils.estimate_tokens(s, model=str(effective_model or ""))))
            except Exception:
                return max(1, len(s) // 4)

        system_text = ""
        prompt_text = ""

        logic = getattr(self._agent, "logic", None)
        if (
            state is not None
            and logic is not None
            and hasattr(state, "vars")
            and isinstance(getattr(state, "vars", None), dict)
        ):
            context_ns = state.vars.get("context") if isinstance(state.vars.get("context"), dict) else {}
            task = str(context_ns.get("task") or "")
            limits = state.vars.get("_limits") if isinstance(state.vars.get("_limits"), dict) else {}
            try:
                iteration = int(limits.get("current_iteration", 0) or 0) + 1
                max_iterations = int(limits.get("max_iterations", 25) or 25)
            except Exception:
                iteration = 1
                max_iterations = 25

            try:
                req = logic.build_request(
                    task=task,
                    messages=messages,
                    guidance="",
                    iteration=iteration,
                    max_iterations=max_iterations,
                    vars=state.vars,
                )
                system_text = str(getattr(req, "system_prompt", "") or "").strip()
                prompt_text = str(getattr(req, "prompt", "") or "").strip()
            except Exception:
                system_text = ""
                prompt_text = ""

            if self._agent_kind == "memact":
                try:
                    from abstractruntime.memory.active_memory import render_memact_system_prompt

                    mem_prompt = render_memact_system_prompt(state.vars)
                    if isinstance(mem_prompt, str) and mem_prompt.strip():
                        system_text = (mem_prompt.strip() + ("\n\n" + system_text if system_text else "")).strip()
                except Exception:
                    pass

        # Approximate messages by concatenating content with role labels.
        text_parts: List[str] = []
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
        messages_tokens = estimate_tokens(joined) if joined else 0
        system_tokens = estimate_tokens(system_text) if system_text else 0
        prompt_tokens = estimate_tokens(prompt_text) if prompt_text else 0

        return {
            "effective_model": effective_model,
            "source": "estimate",
            "system_tokens": system_tokens,
            "prompt_tokens": prompt_tokens,
            "messages_tokens": messages_tokens,
            "tools_tokens": 0,
            "total_tokens": int(system_tokens) + int(prompt_tokens) + int(messages_tokens),
        }

    def _get_status_text(self) -> str:
        """Generate status text for the status bar."""
        # Keep this fast: the render thread can call this frequently.
        state = self._safe_get_state()

        effective_model = self._get_effective_model(state)
        messages = self._select_messages_for_llm(state)
        max_tokens = self._resolve_context_max_tokens(state, effective_model=effective_model)

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
        active_mem_updated_at = ""
        current_iteration = 0
        toolset_id = ""
        if state is not None and hasattr(state, "vars") and isinstance(getattr(state, "vars", None), dict):
            runtime_ns = state.vars.get("_runtime") if isinstance(state.vars.get("_runtime"), dict) else {}
            mem = runtime_ns.get("active_memory") if isinstance(runtime_ns, dict) else None
            if isinstance(mem, dict):
                active_mem_updated_at = str(mem.get("updated_at") or "")
            toolset_id = str(runtime_ns.get("toolset_id") or "") if isinstance(runtime_ns, dict) else ""
            limits = state.vars.get("_limits") if isinstance(state.vars.get("_limits"), dict) else {}
            try:
                current_iteration = int(limits.get("current_iteration", 0) or 0)
            except Exception:
                current_iteration = 0

        cache_key = (
            getattr(state, "run_id", None) if state is not None else None,
            len(messages),
            last_id,
            last_ts,
            last_len,
            active_mem_updated_at,
            current_iteration,
            toolset_id,
            max_tokens,
            self._model,
        )
        if self._status_cache_key == cache_key and self._status_cache_text:
            return self._status_cache_text

        tokens_used_source = "estimate"
        try:
            est = self._estimate_next_prompt_tokens(state=state, messages=messages, effective_model=effective_model)
            tokens_used_source = str(est.get("source") or "estimate")
            tokens_used = int(est.get("total_tokens") or 0)
        except Exception:
            tokens_used = sum(
                max(1, len(str(m.get("content", ""))) // 4)
                for m in (messages or [])
                if isinstance(m, dict) and m.get("content")
            )

        if tokens_used < 0:
            tokens_used = 0
        pct = (tokens_used / max_tokens) * 100 if max_tokens > 0 else 0.0
        label = "Context" if tokens_used_source == "provider" else "Context(next)"
        status = f"{self._provider} | {self._model} | {label}: {tokens_used:,}/{max_tokens:,} tk ({pct:.0f}%)"
        self._status_cache_key = cache_key
        self._status_cache_text = status
        return status

    def _print(self, text: str = "") -> None:
        """Append text to the UI output area."""
        self._output_lines.append(text)
        self._ui.append_output(text)

    def _ui_append_fold_region(
        self,
        *,
        fold_id: str,
        visible_lines: List[str],
        hidden_lines: List[str],
        collapsed: bool = True,
    ) -> None:
        """Best-effort: append a collapsible region if the UI supports it."""
        ui = getattr(self, "_ui", None)
        if ui is None:
            for line in (visible_lines or []):
                self._print(line)
            if not collapsed:
                for line in (hidden_lines or []):
                    self._print(line)
            return

        fn = getattr(ui, "append_fold_region", None)
        if callable(fn):
            fn(fold_id=str(fold_id), visible_lines=list(visible_lines or []), hidden_lines=list(hidden_lines or []), collapsed=bool(collapsed))
            return

        # Fallback: no interactivity, print expanded content.
        for line in (visible_lines or []):
            self._print(line)
        for line in (hidden_lines or []):
            self._print(line)

    def _ui_update_fold_region(
        self,
        fold_id: str,
        *,
        visible_lines: Optional[List[str]] = None,
        hidden_lines: Optional[List[str]] = None,
    ) -> bool:
        """Best-effort: update a collapsible region if the UI supports it."""
        ui = getattr(self, "_ui", None)
        fn = getattr(ui, "update_fold_region", None) if ui is not None else None
        if callable(fn):
            return bool(fn(str(fold_id), visible_lines=visible_lines, hidden_lines=hidden_lines))
        return False

    def _tool_result_summary(
        self,
        *,
        tool_name: str,
        raw: str,
        ok: Optional[bool],
        tool_args: Optional[Dict[str, Any]],
        max_chars: int = 160,
    ) -> str:
        """Return a single-line summary for the tool result (UI-facing)."""
        tool_name = str(tool_name or "")
        raw = "" if raw is None else str(raw)

        def strip_status_prefix(s: str) -> str:
            t = str(s or "").lstrip()
            for p in ("✅", "❌", "🟢", "🔴", "⏰", "🚫"):
                if t.startswith(p):
                    return t[len(p) :].lstrip(" :")
            return t

        if tool_name == "write_file":
            cleaned = self._strip_tool_prefix(raw, tool_name=tool_name).strip()
            line = (cleaned.splitlines() or [""])[0].strip()
            return strip_status_prefix(line) or ("Done" if ok is not False else "Failed")

        if tool_name == "read_file":
            import re

            cleaned = self._strip_tool_prefix(raw, tool_name=tool_name).strip()
            file_path = ""
            if isinstance(tool_args, dict):
                file_path = str(tool_args.get("file_path") or "")
            if ok is False or cleaned.startswith("Error:") or cleaned.startswith("❌"):
                msg = cleaned
                msg2 = strip_status_prefix(msg)
                return msg2 or "Failed"

            header = (cleaned.splitlines() or [""])[0].strip()
            m = re.match(r"^File:\s*(?P<path>.+?)\s*\((?P<lines>[\d,]+)\s+lines\)\s*$", header)
            if m:
                file_path = m.group("path").strip() or file_path
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
                return f"Read '{file_path}' ({bytes_read:,} bytes, {lines_read:,} lines)"

            bytes_read = len(cleaned.encode("utf-8"))
            lines_read = len(cleaned.splitlines())
            path_part = f" '{file_path}'" if file_path else ""
            return f"Read{path_part} ({bytes_read:,} bytes, {lines_read:,} lines)"

        cleaned = self._strip_tool_prefix(raw, tool_name=tool_name).strip()
        first = (cleaned.splitlines() or [""])[0].strip()
        if not first:
            return "Done" if ok is not False else "Failed"
        first = strip_status_prefix(first)
        if len(first) <= max_chars:
            return first
        return first[: max(1, max_chars - 1)] + "…"

    def _tool_args_preview(self, *, tool_name: str, args: Dict[str, Any], max_chars: int = 140) -> str:
        """Build a high-signal, low-noise args preview for tool calls.

        The goal is to avoid dumping massive `content` fields into the visible transcript.
        Full args remain available in the expanded tool block.
        """
        name = str(tool_name or "")
        a = dict(args or {})

        def trunc(s: str) -> str:
            s2 = str(s or "")
            if len(s2) <= max_chars:
                return s2
            return s2[: max(1, max_chars - 1)] + "…"

        if name == "write_file":
            fp = str(a.get("file_path") or a.get("path") or "")
            content = a.get("content")
            content_s = str(content or "")
            bytes_n = len(content_s.encode("utf-8")) if content is not None else 0
            lines_n = len(content_s.splitlines()) if content is not None else 0
            return trunc(json.dumps({"file_path": fp, "bytes": bytes_n, "lines": lines_n}, ensure_ascii=False))

        if name == "read_file":
            fp = str(a.get("file_path") or a.get("path") or "")
            offset = a.get("offset")
            limit = a.get("limit")
            payload: Dict[str, Any] = {"file_path": fp}
            if offset is not None:
                payload["offset"] = offset
            if limit is not None:
                payload["limit"] = limit
            return trunc(json.dumps(payload, ensure_ascii=False))

        if name == "edit_file":
            tf = str(a.get("target_file") or a.get("file_path") or "")
            return trunc(json.dumps({"target_file": tf}, ensure_ascii=False))

        if name == "execute_command":
            cmd = str(a.get("command") or "")
            cwd = str(a.get("cwd") or "")
            payload2: Dict[str, Any] = {"command": cmd}
            if cwd:
                payload2["cwd"] = cwd
            return trunc(json.dumps(payload2, ensure_ascii=False))

        if name == "web_search":
            term = str(a.get("search_term") or a.get("query") or "")
            return trunc(json.dumps({"search_term": term}, ensure_ascii=False))

        # Default: compact JSON with truncation.
        return trunc(json.dumps(self._truncate_for_ui(a, max_chars=max_chars), ensure_ascii=False, sort_keys=True, default=str))

    def _format_tool_output_lines(
        self,
        *,
        tool_name: str,
        raw: str,
        indent: str = "    ",
        include_read_file_content: bool = True,
        tool_args: Optional[Dict[str, Any]] = None,
    ) -> List[str]:
        """Format tool output for inclusion inside an expanded tool block."""
        tool_name = str(tool_name or "")
        raw = "" if raw is None else str(raw)

        if tool_name == "read_file" and include_read_file_content:
            cleaned = self._strip_tool_prefix(raw, tool_name=tool_name).strip()
            if cleaned.startswith(("Error:", "❌")):
                return [_style(f"{indent}{cleaned}", _C.DIM, enabled=self._color)]
            split_idx = cleaned.find("\n\n")
            if split_idx >= 0:
                content = cleaned[split_idx + 2 :]
            else:
                content = "\n".join(cleaned.splitlines()[1:])
            out: List[str] = []
            out.append(_style(f"{indent}content:", _C.DIM, enabled=self._color))
            for line in (content.splitlines() or [""]):
                out.append(f"{indent}{line}")
            return out

        out_lines: List[str] = []
        for line in (raw.splitlines() or [""]):
            style_codes: Tuple[str, ...] = (_C.DIM,)
            if tool_name == "edit_file":
                if line.startswith("Edited ") or line.startswith("Preview "):
                    style_codes = (_C.BOLD,)
                elif line.startswith("@@"):
                    style_codes = (_C.DIM,)
                elif line.startswith(" "):
                    style_codes = ()
                elif line.startswith("+") and not line.startswith("+++"):
                    style_codes = (_C.BLUE,)
                elif line.startswith("-") and not line.startswith("---"):
                    style_codes = (_C.RED,)
                else:
                    style_codes = (_C.DIM,)
            out_lines.append(_style(f"{indent}{line}", *style_codes, enabled=self._color))
        return out_lines

    def _terminal_width(self) -> int:
        """Best-effort current terminal width (for full-line ANSI background blocks)."""
        try:
            import shutil

            width = int(shutil.get_terminal_size(fallback=(120, 40)).columns)
        except Exception:
            width = 120
        return max(40, width)

    def _format_timestamp_short(self, ts: Optional[str]) -> str:
        """Format an ISO timestamp as a compact, stable UTC string."""
        raw = str(ts or "").strip()
        if not raw:
            return ""
        try:
            from datetime import datetime, timezone

            # Support both "+00:00" and "Z" suffixes.
            if raw.endswith("Z"):
                dt = datetime.fromisoformat(raw[:-1] + "+00:00")
            else:
                dt = datetime.fromisoformat(raw)
            dt = dt.astimezone(timezone.utc)
            return dt.strftime("%Y-%m-%d %H:%MZ")
        except Exception:
            # Best-effort: keep the leading date/time portion.
            return raw[:16]

    def _aggregate_llm_usage(self, state: Any) -> Optional[Dict[str, Any]]:
        """Aggregate per-run LLM token usage (usage-first, else estimate from captured payloads)."""
        if state is None or not hasattr(state, "vars") or not isinstance(getattr(state, "vars", None), dict):
            return None

        runtime_ns = state.vars.get("_runtime") if isinstance(state.vars.get("_runtime"), dict) else {}
        traces = runtime_ns.get("node_traces") if isinstance(runtime_ns, dict) else None
        if not isinstance(traces, dict) or not traces:
            return None

        try:
            from abstractcore.utils.token_utils import TokenUtils
        except Exception:
            TokenUtils = None  # type: ignore[assignment]

        effective_model = self._get_effective_model(state) or str(self._model or "")

        def estimate_tokens(text: str) -> int:
            s = str(text or "")
            if not s:
                return 0
            if TokenUtils is None:
                return max(1, len(s) // 4)
            try:
                return max(0, int(TokenUtils.estimate_tokens(s, model=str(effective_model or ""))))
            except Exception:
                return max(1, len(s) // 4)

        def usage_int(usage: Dict[str, Any], *keys: str) -> Optional[int]:
            for k in keys:
                raw_val = usage.get(k)
                if raw_val is None:
                    continue
                try:
                    return int(raw_val)
                except Exception:
                    continue
            return None

        def estimate_prompt_tokens(captured: Dict[str, Any]) -> int:
            prompt = str(captured.get("prompt") or "")
            system_prompt = str(captured.get("system_prompt") or "")
            messages = captured.get("messages")
            tools = captured.get("tools")

            msg_text_parts: List[str] = []
            if isinstance(messages, list):
                for m in messages:
                    if not isinstance(m, dict):
                        continue
                    role = str(m.get("role") or "").strip()
                    content = str(m.get("content") or "")
                    if not content:
                        continue
                    msg_text_parts.append(f"{role}:\n{content}" if role else content)
            messages_text = "\n\n".join(msg_text_parts).strip()

            tool_prompt = ""
            if isinstance(tools, list) and tools:
                try:
                    from abstractcore.tools.handler import UniversalToolHandler

                    handler = UniversalToolHandler(str(effective_model or ""))
                    tool_prompt = handler.format_tools_prompt(tools) or ""
                except Exception:
                    tool_prompt = json.dumps(tools, ensure_ascii=False, sort_keys=True, default=str)

            return (
                estimate_tokens(system_prompt)
                + estimate_tokens(prompt)
                + (estimate_tokens(messages_text) if messages_text else 0)
                + (estimate_tokens(tool_prompt) if tool_prompt else 0)
            )

        def estimate_completion_tokens(result: Dict[str, Any]) -> int:
            content = str(result.get("content") or "")
            reasoning = str(result.get("reasoning") or "")
            tool_calls = result.get("tool_calls")
            tool_calls_text = ""
            if tool_calls is not None:
                try:
                    tool_calls_text = json.dumps(tool_calls, ensure_ascii=False, sort_keys=True, default=str)
                except Exception:
                    tool_calls_text = str(tool_calls)
            joined = "\n\n".join([p for p in (reasoning, content, tool_calls_text) if isinstance(p, str) and p.strip()]).strip()
            return estimate_tokens(joined) if joined else 0

        total_in = 0
        total_out = 0
        llm_calls = 0
        used_estimate = False

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
                if not isinstance(eff, dict) or str(eff.get("type") or "") != "llm_call":
                    continue
                llm_calls += 1
                result = step.get("result") if isinstance(step.get("result"), dict) else {}

                usage = result.get("usage") if isinstance(result.get("usage"), dict) else None
                p = usage_int(usage, "prompt_tokens", "input_tokens") if isinstance(usage, dict) else None
                c = usage_int(usage, "completion_tokens", "output_tokens") if isinstance(usage, dict) else None
                if p is not None and c is not None:
                    total_in += int(p)
                    total_out += int(c)
                    continue

                meta = result.get("metadata") if isinstance(result.get("metadata"), dict) else {}
                runtime_obs = meta.get("_runtime_observability") if isinstance(meta, dict) else None
                captured = runtime_obs.get("llm_generate_kwargs") if isinstance(runtime_obs, dict) else None
                if isinstance(captured, dict):
                    try:
                        total_in += int(estimate_prompt_tokens(captured))
                        total_out += int(estimate_completion_tokens(result))
                        used_estimate = True
                        continue
                    except Exception:
                        pass

                # If we couldn't estimate, still mark that totals are incomplete/estimated.
                used_estimate = True

        if llm_calls <= 0:
            return None

        return {
            "input_tokens": int(total_in),
            "output_tokens": int(total_out),
            "total_tokens": int(total_in) + int(total_out),
            "calls": int(llm_calls),
            "estimated": bool(used_estimate),
        }

    def _build_answer_footer(self, *, state: Any) -> str:
        """Build a compact footer: timestamp + (best-effort) token/time stats."""
        ts_iso: Optional[str] = None
        try:
            messages = self._messages_from_state(state) if state is not None else []
            for m in reversed(messages):
                if isinstance(m, dict) and m.get("role") == "assistant":
                    ts_iso = str(m.get("timestamp") or "").strip() or None
                    break
        except Exception:
            ts_iso = None
        if not ts_iso:
            ts_iso = _now_iso()

        ts_text = self._format_timestamp_short(ts_iso)
        parts: List[str] = [ts_text] if ts_text else []

        usage = self._aggregate_llm_usage(state)
        elapsed_s: Optional[float] = None
        turn_started_at = getattr(self, "_turn_started_at", None)
        if isinstance(turn_started_at, (int, float)):
            elapsed_s = max(0.0, float(time.perf_counter() - float(turn_started_at)))
        else:
            try:
                from datetime import datetime, timezone

                created = str(getattr(state, "created_at", "") or "")
                if created:
                    if created.endswith("Z"):
                        dt = datetime.fromisoformat(created[:-1] + "+00:00")
                    else:
                        dt = datetime.fromisoformat(created)
                    elapsed_s = max(0.0, (datetime.now(timezone.utc) - dt.astimezone(timezone.utc)).total_seconds())
            except Exception:
                elapsed_s = None

        if isinstance(usage, dict):
            in_t = int(usage.get("input_tokens") or 0)
            out_t = int(usage.get("output_tokens") or 0)
            est = bool(usage.get("estimated"))
            est_suffix = " est" if est else ""
            parts.append(f"in={in_t:,}{est_suffix}")
            parts.append(f"out={out_t:,}{est_suffix}")
            if isinstance(elapsed_s, (int, float)) and float(elapsed_s) > 0:
                tok_s = float(in_t + out_t) / float(elapsed_s)
                parts.append(f"{tok_s:0.1f} tok/s")
                parts.append(f"{float(elapsed_s):0.1f}s")
        elif isinstance(elapsed_s, (int, float)) and float(elapsed_s) > 0:
            parts.append(f"{float(elapsed_s):0.1f}s")

        return "  ".join([p for p in parts if isinstance(p, str) and p.strip()]).strip()

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

            cleaned = self._strip_tool_prefix(raw, tool_name=tool_name).strip()

            if tool_name == "write_file":
                line = (cleaned.splitlines() or [""])[0].strip()
                if ok is False and line and not line.startswith("❌"):
                    line = f"❌ {line}" if line else "❌ Failed"
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
                file_path = m.group("path").strip() or file_path
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
                summary = f"✅ Read '{file_path}' ({bytes_read:,} bytes, {lines_read:,} lines)"
                self._print(_style(f"{indent}{summary}", _C.DIM, enabled=self._color))
                return

            bytes_read = len(cleaned.encode("utf-8"))
            lines_read = len(cleaned.splitlines())
            path_part = f" '{file_path}'" if file_path else ""
            summary = f"✅ Read{path_part} ({bytes_read:,} bytes, {lines_read:,} lines)"
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

    def _format_user_prompt_block(self, text: str, *, copy_id: Optional[str] = None, footer: Optional[str] = None) -> str:
        """Render a user prompt as a padded, full-line background block (no truncation)."""
        lines = text.splitlines() or [""]
        footer_text = str(footer or "").strip()
        copy_line = ""
        if isinstance(copy_id, str) and copy_id:
            copy_line = f"[[COPY:{copy_id}]]"
            if footer_text:
                copy_line = f"{copy_line} {footer_text}"

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
            if copy_line:
                out.append(copy_line)
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
        if copy_line:
            out_lines.append(copy_line)
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
        ts_text = self._format_timestamp_short(_now_iso())
        footer = _style(ts_text, _C.DIM, enabled=self._color) if ts_text else ""
        self._print(self._format_user_prompt_block(text, copy_id=copy_id, footer=footer))

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
            "log",
            "memorize",
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

    def _print_answer_block(self, *, title: str, answer_text: str, prompt_text: Optional[str] = None, state: Any = None) -> None:
        import uuid

        answer = "" if answer_text is None else str(answer_text)
        if not answer.strip():
            answer = "(no assistant answer produced yet)"

        copy_id = f"assistant_{uuid.uuid4().hex}"
        payload = self._build_answer_copy_payload(answer_text=answer, prompt_text=prompt_text)
        self._ui.register_copy_payload(copy_id, payload)

        self._print(_style(f"\n{title}", _C.GREEN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        # Render Markdown for the terminal, but keep copy payload lossless (raw answer).
        try:
            renderer = TerminalMarkdownRenderer(color=self._color)
            rendered = renderer.render(answer)
        except Exception:
            rendered = answer
        self._print(rendered)
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        footer_text = self._build_answer_footer(state=state)
        footer = _style(footer_text, _C.DIM, enabled=self._color) if footer_text else ""
        self._print(f"[[COPY:{copy_id}]] {footer}".rstrip())
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
        if self._base_url:
            self._print(f"Base URL: {self._base_url}")
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
                import uuid

                text = content.strip()
                self._turn_trace.append("Thought:\n" + text)
                fid = f"thought_{uuid.uuid4().hex}"
                header = _style("Thought", _C.ORANGE, _C.BOLD, enabled=self._color)
                lines = text.splitlines() or [""]
                first_line = lines[0].strip()
                if len(first_line) > 200:
                    first_line = first_line[:199] + "…"
                visible = ["", f"[[FOLD:{fid}]]{header}", _style(f"  {first_line}", _C.ORANGE, enabled=self._color)]
                # Hidden part shows the remaining thought (avoid duplicating the first line).
                rest = lines[1:] if len(lines) > 1 else []
                hidden = [_style(f"  {line}" if line else "  ", _C.ORANGE, enabled=self._color) for line in (rest or [""])]
                hidden.append("")
                self._ui_append_fold_region(fold_id=fid, visible_lines=visible, hidden_lines=hidden, collapsed=True)
        elif step == "act":
            import uuid

            tool = data.get("tool", "unknown")
            args = data.get("args") or {}
            call_id_raw = data.get("call_id")
            call_id = str(call_id_raw).strip() if call_id_raw is not None else ""
            ui_args = self._truncate_for_ui(args, max_chars=200)
            try:
                args_str = self._tool_args_preview(tool_name=str(tool), args=dict(args), max_chars=140)
            except Exception:
                args_str = str(ui_args)
            fold_id = f"tool_{uuid.uuid4().hex}"
            marker = f"[[SPINNER:{fold_id}]]"
            self._pending_tool_markers.append(marker)
            self._pending_tool_metas.append({"tool": tool, "args": dict(args), "call_id": call_id, "fold_id": fold_id})

            header_core = _style(f"Tool Call: {tool}", _C.GREEN, _C.BOLD, enabled=self._color)
            header_line = f"[[FOLD:{fold_id}]]{header_core} {marker}"
            vis = [
                header_line,
                _style(f"  args: {args_str}", _C.DIM, enabled=self._color),
                _style("  result: (running…)", _C.DIM, enabled=self._color),
            ]

            try:
                args_pretty = json.dumps(args, ensure_ascii=False, sort_keys=True, indent=2)
            except Exception:
                args_pretty = str(args)
            hid = [
                _style(f"  call_id: {call_id}" if call_id else "  call_id: (none)", _C.DIM, enabled=self._color),
                _style("  arguments:", _C.DIM, enabled=self._color),
                *[f"    {ln}" if ln else "    " for ln in (args_pretty.splitlines() or [""])],
                "",
                _style("  output:", _C.DIM, enabled=self._color),
                _style("    (pending…)", _C.DIM, enabled=self._color),
                "",
            ]
            self._ui_append_fold_region(fold_id=fold_id, visible_lines=vis, hidden_lines=hid, collapsed=True)

            # Track full arguments for copy payloads (no truncation).
            try:
                args_full = json.dumps(args, ensure_ascii=False, sort_keys=True)
            except Exception:
                args_full = str(args)
            call_id_note = f" [{call_id}]" if call_id else ""
            self._turn_trace.append(f"Tool: {tool}{call_id_note}({args_full})")
            tool_s = str(tool or "")
            if tool_s.startswith("mcp::"):
                self._ui.set_spinner("MCP")
            else:
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
            tool_args = None
            fold_id: Optional[str] = None
            if self._pending_tool_metas:
                try:
                    meta = self._pending_tool_metas.pop(0)
                    if isinstance(meta, dict):
                        fold_id = str(meta.get("fold_id") or "") or None
                        args_meta = meta.get("args")
                        if isinstance(args_meta, dict):
                            tool_args = args_meta
                except Exception:
                    tool_args = None

            icon = "✅" if ok else "❌"
            summary = self._tool_result_summary(tool_name=tool_name, raw=raw, ok=ok, tool_args=tool_args)

            if fold_id:
                # Keep pending marker queue bounded (header is rewritten on update).
                if self._pending_tool_markers:
                    try:
                        self._pending_tool_markers.pop(0)
                    except Exception:
                        pass

                # Update the existing tool block to show status + summary, and populate expanded details.
                tool_call = tool_args or {}
                try:
                    args_pretty = json.dumps(tool_call, ensure_ascii=False, sort_keys=True, indent=2)
                except Exception:
                    args_pretty = str(tool_call)

                call_id = ""
                tool_label = str(data.get("tool", "") or tool_name)
                # Best-effort: preserve original call_id in meta if present.
                if isinstance(meta, dict):
                    call_id = str(meta.get("call_id") or "")

                header_core = _style(f"Tool Call: {tool_label}", _C.GREEN, _C.BOLD, enabled=self._color)
                try:
                    args_preview = self._tool_args_preview(tool_name=tool_label, args=dict(tool_call) if isinstance(tool_call, dict) else {}, max_chars=140)
                except Exception:
                    args_preview = str(self._truncate_for_ui(tool_call, max_chars=200))
                visible = [
                    f"[[FOLD:{fold_id}]]{header_core} {icon}",
                    _style(f"  args: {args_preview}", _C.DIM, enabled=self._color),
                    _style(f"  result: {summary}", _C.DIM, enabled=self._color),
                ]

                hidden = [
                    _style(f"  call_id: {call_id}" if call_id else "  call_id: (none)", _C.DIM, enabled=self._color),
                    _style("  arguments:", _C.DIM, enabled=self._color),
                    *[f"    {ln}" if ln else "    " for ln in (args_pretty.splitlines() or [""])],
                    "",
                    _style("  output:", _C.DIM, enabled=self._color),
                    *self._format_tool_output_lines(tool_name=tool_name, raw=raw, indent="    ", include_read_file_content=True, tool_args=tool_args),
                    "",
                ]
                self._ui_update_fold_region(fold_id, visible_lines=visible, hidden_lines=hidden)

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
            self._print_answer_block(title="ANSWER", answer_text=answer_text, state=self._safe_get_state())
        elif step == "status":
            # Workflow-driven status update (e.g., VisualFlow emit_event name="abstractcode.status").
            text = str(data.get("text", "") or "").strip()
            dur_raw = data.get("duration")
            dur: Optional[float]
            if dur_raw is None:
                dur = None
            else:
                try:
                    dur = float(dur_raw)
                except Exception:
                    dur = None

            if not text:
                # Allow workflows to explicitly clear the status by sending an empty string.
                self._ui.clear_spinner()
            else:
                self._ui.set_spinner(text, duration_s=dur)
        elif step == "message":
            # Workflow-driven message notification (e.g., VisualFlow emit_event name="abstractcode.message").
            text = str(data.get("text") or data.get("message") or "").rstrip()
            if not text.strip():
                return
            level = str(data.get("level") or "info").strip().lower()
            title = str(data.get("title") or "").strip()

            # Keep this simple: messages are UX-only and must not affect execution.
            self._ui.clear_spinner()
            self._ui.scroll_to_bottom()

            if level == "error":
                color = _C.RED
                tag = "ERROR"
            elif level == "warning":
                color = _C.YELLOW
                tag = "WARNING"
            elif level == "success":
                color = _C.GREEN
                tag = "SUCCESS"
            else:
                color = _C.CYAN
                tag = "MESSAGE"

            header = title if title else tag
            self._print(_style(f"\n{header}", color, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 60, _C.DIM, enabled=self._color))
            for ln in (text.splitlines() or [""]):
                self._print(str(ln))
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
        if self._base_url:
            banner_lines.append(f"Base URL: {self._base_url}")
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
        if command == "mcp":
            self._handle_mcp(arg)
            return False
        if command in ("executor", "target"):
            self._handle_executor(arg)
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
            sub = arg.strip()
            if sub:
                head = (sub.split(None, 1) or [""])[0].lower()
                if head == "copy":
                    self._copy_full_history_to_clipboard()
                    return False
                try:
                    limit = int(sub)
                except ValueError:
                    self._print(_style("Usage: /history [N]  |  /history copy", _C.DIM, enabled=self._color))
                    return False
                self._show_history(limit=limit)
                return False

            self._show_history(limit=12)
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
            self._handle_memory(arg)
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
        if command == "memorize":
            self._handle_memorize(arg)
            return False
        if command == "recall":
            self._handle_recall(arg)
            return False
        if command in ("vars", "var"):
            self._handle_vars(arg)
            return False
        if command == "log":
            self._handle_log(arg)
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

    def _handle_log(self, raw: str) -> None:
        """Show durable logs for the current run.

        `/log runtime` is the AbstractRuntime-centric view (step trace, payloads).
        `/log provider` is the provider wire view (request sent + response received).

        Usage:
          /log runtime [copy] [--last] [--json-only] [--save <path>]
          /log provider [copy] [--last|--all] [--json-only] [--save <path>]
        """
        import shlex

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        usage = (
            "Usage:\n"
            "  /log runtime [copy] [--last] [--json-only] [--save <path>]\n"
            "  /log provider [copy] [--last|--all] [--json-only] [--save <path>]\n"
        )
        if not parts:
            self._print(_style(usage, _C.DIM, enabled=self._color))
            return

        kind = str(parts[0] or "").strip().lower()
        rest = parts[1:]
        rest_raw = shlex.join(rest) if hasattr(shlex, "join") else " ".join(rest)

        if kind in ("runtime", "rt"):
            self._handle_log_runtime(rest_raw)
            return
        if kind in ("provider", "wire"):
            self._handle_log_provider(rest_raw)
            return

        self._print(_style(f"Unknown /log kind: {kind}", _C.YELLOW, enabled=self._color))
        self._print(_style(usage, _C.DIM, enabled=self._color))

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
            if "tool_prompt_examples" in config:
                self._tool_prompt_examples = bool(config.get("tool_prompt_examples"))
            if "tool_executor" in config:
                raw_exec = config.get("tool_executor")
                if raw_exec is None:
                    self._tool_executor_server_id = None
                elif isinstance(raw_exec, str) and raw_exec.strip():
                    self._tool_executor_server_id = raw_exec.strip()
            if "mcp_servers" in config:
                raw = config.get("mcp_servers")
                if isinstance(raw, dict):
                    out: Dict[str, Dict[str, Any]] = {}
                    for sid, entry in raw.items():
                        if not isinstance(sid, str) or not sid.strip():
                            continue
                        if not isinstance(entry, dict):
                            continue

                        transport = str(entry.get("transport") or "").strip().lower()
                        if not transport:
                            transport = "stdio" if "command" in entry else "streamable_http"

                        if transport in ("stdio",):
                            cmd_raw = entry.get("command")
                            if isinstance(cmd_raw, list):
                                cmd_out = [str(c) for c in cmd_raw if isinstance(c, str) and c.strip()]
                            elif isinstance(cmd_raw, str) and cmd_raw.strip():
                                cmd_out = [cmd_raw.strip()]
                            else:
                                continue

                            cwd = entry.get("cwd")
                            cwd_out = str(cwd).strip() if isinstance(cwd, str) and cwd.strip() else None

                            env_raw = entry.get("env")
                            if isinstance(env_raw, dict):
                                env_out = {
                                    str(k): str(v)
                                    for k, v in env_raw.items()
                                    if isinstance(k, str) and str(k).strip() and isinstance(v, (str, int, float, bool))
                                }
                            else:
                                env_out = {}

                            cfg: Dict[str, Any] = {"transport": "stdio", "command": cmd_out}
                            if cwd_out:
                                cfg["cwd"] = cwd_out
                            if env_out:
                                cfg["env"] = env_out
                            out[sid.strip()] = cfg
                            continue

                        url = entry.get("url")
                        if not isinstance(url, str) or not url.strip():
                            continue
                        headers = entry.get("headers")
                        if headers is None:
                            headers_out = {}
                        elif isinstance(headers, dict):
                            headers_out = {
                                str(k): str(v)
                                for k, v in headers.items()
                                if isinstance(k, str) and str(k).strip() and isinstance(v, (str, int, float, bool))
                            }
                        else:
                            headers_out = {}
                        out[sid.strip()] = {"transport": transport or "streamable_http", "url": url.strip(), "headers": headers_out}
                    self._mcp_servers = out
        except Exception:
            pass  # Ignore corrupt config files

    def _save_config(self) -> None:
        """Save configuration to file."""
        if not self._config_file:
            return
        try:
            existing: Dict[str, Any] = {}
            if self._config_file.exists():
                try:
                    raw = json.loads(self._config_file.read_text())
                    if isinstance(raw, dict):
                        existing = dict(raw)
                except Exception:
                    existing = {}

            config = dict(existing)
            mcp_servers = getattr(self, "_mcp_servers", None)
            config.update(
                {
                    "max_tokens": self._max_tokens,
                    "max_history_messages": getattr(self, "_max_history_messages", -1),
                    "auto_approve": self._auto_approve,
                    "plan_mode": self._plan_mode,
                    "review_mode": self._review_mode,
                    "review_max_rounds": self._review_max_rounds,
                    "allowed_tools": self._allowed_tools,
                    "tool_prompt_examples": bool(self._tool_prompt_examples),
                    "tool_executor": getattr(self, "_tool_executor_server_id", None),
                    "mcp_servers": dict(mcp_servers or {}) if isinstance(mcp_servers, dict) else {},
                }
            )
            self._config_file.write_text(json.dumps(config, indent=2))
        except Exception:
            pass  # Silently fail if we can't write

    def _handle_tools(self, raw: str) -> None:
        """List or configure the session tool allowlist.

        Usage:
          /tools
          /tools reset
          /tools examples on|off
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
        if sub not in ("list", "reset", "examples", "only", "enable", "disable", "help", "-h", "--help"):
            self._print(_style("Usage:", _C.DIM, enabled=self._color))
            self._print(_style("  /tools", _C.DIM, enabled=self._color))
            self._print(_style("  /tools reset", _C.DIM, enabled=self._color))
            self._print(_style("  /tools examples on|off", _C.DIM, enabled=self._color))
            self._print(_style("  /tools only <name...>", _C.DIM, enabled=self._color))
            self._print(_style("  /tools enable <name...>", _C.DIM, enabled=self._color))
            self._print(_style("  /tools disable <name...>", _C.DIM, enabled=self._color))
            return

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

        if callable(getattr(self, "_maybe_sync_executor_tools", None)):
            self._maybe_sync_executor_tools()

        available = _available_tool_defs()
        available_names = sorted(available.keys())

        def _split_names(tokens: List[str]) -> List[str]:
            out: List[str] = []
            executor_id = str(getattr(self, "_tool_executor_server_id", "") or "").strip()
            for t in tokens:
                for part in str(t).split(","):
                    name = part.strip()
                    if not name:
                        continue
                    if not name.startswith("mcp::") and executor_id:
                        candidate = f"mcp::{executor_id}::{name}"
                        if candidate in available:
                            out.append(candidate)
                            continue
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

        if sub in ("help", "-h", "--help"):
            self._print(_style("Usage:", _C.DIM, enabled=self._color))
            self._print(_style("  /tools", _C.DIM, enabled=self._color))
            self._print(_style("  /tools reset", _C.DIM, enabled=self._color))
            self._print(_style("  /tools examples on|off", _C.DIM, enabled=self._color))
            self._print(_style("  /tools only <name...>", _C.DIM, enabled=self._color))
            self._print(_style("  /tools enable <name...>", _C.DIM, enabled=self._color))
            self._print(_style("  /tools disable <name...>", _C.DIM, enabled=self._color))
            return

        if sub == "reset":
            self._allowed_tools = None
            if callable(getattr(self, "_apply_tool_settings_to_active_run", None)):
                self._apply_tool_settings_to_active_run(allowed_tools=None)
            self._save_config()
            self._print(_style("✅ Tools reset to default (all enabled).", _C.GREEN, enabled=self._color))
            sub = "list"

        if sub == "examples":
            def _apply_examples_to_active_run(enabled: bool) -> None:
                state = self._safe_get_state()
                if state is None or not hasattr(state, "vars") or not isinstance(state.vars, dict):
                    return
                runtime_ns = state.vars.get("_runtime")
                if not isinstance(runtime_ns, dict):
                    runtime_ns = {}
                    state.vars["_runtime"] = runtime_ns
                runtime_ns["tool_prompt_examples"] = bool(enabled)
                self._status_cache_key = None
                self._status_cache_text = ""
                try:
                    self._runtime.run_store.save(state)
                except Exception:
                    pass

            value = args[0].strip().lower() if args else ""
            if value in ("on", "true", "1", "yes"):
                self._tool_prompt_examples = True
                _apply_examples_to_active_run(True)
                self._save_config()
                self._print(_style("✅ Tool examples enabled.", _C.GREEN, enabled=self._color))
                sub = "list"
            elif value in ("off", "false", "0", "no"):
                self._tool_prompt_examples = False
                _apply_examples_to_active_run(False)
                self._save_config()
                self._print(_style("✅ Tool examples disabled.", _C.GREEN, enabled=self._color))
                sub = "list"
            elif not value:
                state = "on" if self._tool_prompt_examples else "off"
                self._print(_style(f"Tool examples: {state}", _C.DIM, enabled=self._color))
                sub = "list"
            else:
                self._print(_style("Usage: /tools examples on|off", _C.DIM, enabled=self._color))
                return

        if sub in ("only", "enable", "disable"):
            names = _split_names(args)
            if not names:
                self._print(_style(f"Usage: /tools {sub} <name...>", _C.DIM, enabled=self._color))
                return
            if sub in ("enable", "disable") and len(names) == 1 and names[0].lower() in ("all", "*"):
                new_allow = list(available_names) if sub == "enable" else []
                self._allowed_tools = list(new_allow)
                if callable(getattr(self, "_apply_tool_settings_to_active_run", None)):
                    self._apply_tool_settings_to_active_run(allowed_tools=self._allowed_tools)
                self._save_config()
                mode = "enabled" if sub == "enable" else "disabled"
                self._print(_style(f"✅ All tools {mode}.", _C.GREEN, enabled=self._color))
                sub = "list"
            else:
                unknown = [n for n in names if n not in available]
                if unknown:
                    self._print(_style("Unknown tool(s): " + ", ".join(unknown), _C.YELLOW, enabled=self._color))
                    self._print(_style("Use /tools to list available tools.", _C.DIM, enabled=self._color))
                    return

                current = _effective_allowlist_from_state()
                if current is None:
                    current = list(self._allowed_tools) if isinstance(self._allowed_tools, list) else list(available_names)
                current_set = set(current)

                if sub == "only":
                    new_allow = names
                elif sub == "enable":
                    new_allow = list(dict.fromkeys(list(current) + list(names)))
                else:
                    new_allow = [n for n in current if n not in set(names)]
                new_set = set(new_allow)
                added = [n for n in new_allow if n not in current_set]
                removed = [n for n in current if n not in new_set]

                self._allowed_tools = list(new_allow)
                if callable(getattr(self, "_apply_tool_settings_to_active_run", None)):
                    self._apply_tool_settings_to_active_run(allowed_tools=self._allowed_tools)
                self._save_config()
                parts2: List[str] = []
                if added:
                    parts2.append("+" + ", ".join(added))
                if removed:
                    parts2.append("-" + ", ".join(removed))
                delta = f" ({' '.join(parts2)})" if parts2 else ""
                self._print(
                    _style(
                        f"✅ Tools updated: {len(self._allowed_tools)}/{len(available_names)} enabled{delta}.",
                        _C.GREEN,
                        enabled=self._color,
                    )
                )
                sub = "list"

        effective_from_run = _effective_allowlist_from_state()
        source = (
            "active run"
            if isinstance(effective_from_run, list)
            else ("session config" if isinstance(self._allowed_tools, list) else "default")
        )
        effective = effective_from_run
        if effective is None:
            effective = list(self._allowed_tools) if isinstance(self._allowed_tools, list) else list(available_names)

        enabled_set = set(effective)
        self._print(_style("\nTools", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        saved = "yes" if self._config_file else "no"
        examples_state = "on" if self._tool_prompt_examples else "off"
        self._print(
            _style(
                f"Enabled: {len(effective)}/{len(available_names)}  Examples: {examples_state}  Saved: {saved}  Source: {source}",
                _C.DIM,
                enabled=self._color,
            )
        )
        for name in available_names:
            icon = "✅" if name in enabled_set else "❌"
            desc = available.get(name, {}).get("description") or ""
            self._print(f"  {icon} {name}")
            if isinstance(desc, str) and desc.strip():
                self._print(_style(f"     {desc.strip()}", _C.DIM, enabled=self._color))
        self._print(_style("─" * 60, _C.DIM, enabled=self._color))
        self._print(_style("Tip: /tools only list_files read_file write_file", _C.DIM, enabled=self._color))

    def _handle_mcp(self, raw: str) -> None:
        """Configure and sync MCP servers for tool discovery/execution.

        Usage:
          /mcp
          /mcp list
          /mcp add <server_id> <url> [--header K=V ...]
          /mcp add <server_id> stdio [--cwd PATH] [--env K=V ...] -- <command...>
          /mcp remove <server_id>
          /mcp sync [server_id|all]
        """
        import shlex

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        sub = parts[0].lower() if parts else "list"
        args = parts[1:] if len(parts) > 1 else []

        if sub in ("help", "-h", "--help"):
            self._print(_style("Usage:", _C.DIM, enabled=self._color))
            self._print(_style("  /mcp list", _C.DIM, enabled=self._color))
            self._print(_style("  /mcp add <server_id> <url> [--header K=V ...]", _C.DIM, enabled=self._color))
            self._print(_style("  /mcp add <server_id> stdio [--cwd PATH] [--env K=V ...] -- <command...>", _C.DIM, enabled=self._color))
            self._print(_style("  /mcp remove <server_id>", _C.DIM, enabled=self._color))
            self._print(_style("  /mcp sync [server_id|all]", _C.DIM, enabled=self._color))
            return

        if sub in ("list",):
            servers = self._mcp_servers or {}
            if not servers:
                self._print(_style("No MCP servers configured.", _C.DIM, enabled=self._color))
                return
            self._print(_style("MCP servers:", _C.CYAN, _C.BOLD, enabled=self._color))
            for sid, entry in sorted(servers.items()):
                transport = str(entry.get("transport") or "").strip() or ("stdio" if "command" in entry else "streamable_http")
                if transport == "stdio":
                    cmd = entry.get("command")
                    cmd_str = " ".join(str(c) for c in cmd) if isinstance(cmd, list) else str(cmd or "")
                    self._print(f"- {sid}: stdio -- {cmd_str}")
                else:
                    url = entry.get("url")
                    self._print(f"- {sid}: {url}")
            return

        if sub in ("add",):
            if len(args) < 2:
                self._print(_style("Usage:", _C.DIM, enabled=self._color))
                self._print(_style("  /mcp add <server_id> <url> [--header K=V ...]", _C.DIM, enabled=self._color))
                self._print(_style("  /mcp add <server_id> stdio [--cwd PATH] [--env K=V ...] -- <command...>", _C.DIM, enabled=self._color))
                return

            server_id = str(args[0] or "").strip()
            if not server_id:
                self._print(_style("server_id must be non-empty.", _C.DIM, enabled=self._color))
                return

            mode = str(args[1] or "").strip()
            if mode.lower() == "stdio":
                # Expected: /mcp add <server_id> stdio [--cwd PATH] [--env K=V ...] -- <command...>
                cwd: Optional[str] = None
                env: Dict[str, str] = {}

                if "--" not in args:
                    self._print(
                        _style(
                            "Usage: /mcp add <server_id> stdio [--cwd PATH] [--env K=V ...] -- <command...>",
                            _C.DIM,
                            enabled=self._color,
                        )
                    )
                    return

                idx = args.index("--")
                flags = args[2:idx]
                cmd = args[idx + 1 :]

                if not cmd:
                    self._print(_style("stdio MCP requires a non-empty command after `--`.", _C.DIM, enabled=self._color))
                    return

                i = 0
                while i < len(flags):
                    token = flags[i]
                    if token == "--cwd" and i + 1 < len(flags):
                        cwd = str(flags[i + 1] or "").strip() or None
                        i += 2
                        continue
                    if token == "--env" and i + 1 < len(flags):
                        kv = str(flags[i + 1] or "")
                        k, sep, v = kv.partition("=")
                        k = k.strip()
                        v = v.strip()
                        if sep == "=" and k:
                            env[k] = v
                        i += 2
                        continue
                    i += 1

                existing = dict(self._mcp_servers or {})
                entry: Dict[str, Any] = {"transport": "stdio", "command": cmd}
                if cwd:
                    entry["cwd"] = cwd
                if env:
                    entry["env"] = env
                existing[server_id] = entry
                self._mcp_servers = existing
                self._save_config()
                self._print(_style(f"Added MCP server '{server_id}' (stdio).", _C.DIM, enabled=self._color))
                return

            url = str(args[1] or "").strip()
            if not url:
                self._print(_style("url must be non-empty.", _C.DIM, enabled=self._color))
                return

            headers: Dict[str, str] = {}
            i = 2
            while i < len(args):
                token = args[i]
                if token == "--header" and i + 1 < len(args):
                    kv = str(args[i + 1] or "")
                    k, sep, v = kv.partition("=")
                    k = k.strip()
                    v = v.strip()
                    if sep == "=" and k and v:
                        headers[k] = v
                    i += 2
                    continue
                i += 1

            existing = dict(self._mcp_servers or {})
            existing[server_id] = {"transport": "streamable_http", "url": url, "headers": headers}
            self._mcp_servers = existing
            self._save_config()
            self._print(_style(f"Added MCP server '{server_id}'.", _C.DIM, enabled=self._color))
            return

        if sub in ("remove", "rm", "delete"):
            if not args:
                self._print(_style("Usage: /mcp remove <server_id>", _C.DIM, enabled=self._color))
                return
            server_id = str(args[0] or "").strip()
            if not server_id:
                return
            existing = dict(self._mcp_servers or {})
            if server_id not in existing:
                self._print(_style(f"Unknown MCP server '{server_id}'.", _C.DIM, enabled=self._color))
                return
            existing.pop(server_id, None)
            self._mcp_servers = existing
            self._save_config()
            self._print(_style(f"Removed MCP server '{server_id}'.", _C.DIM, enabled=self._color))
            return

        if sub in ("sync",):
            target = args[0].strip() if args else "all"
            servers = self._mcp_servers or {}
            if not servers:
                self._print(_style("No MCP servers configured.", _C.DIM, enabled=self._color))
                return
            if target == "all":
                for sid in sorted(servers.keys()):
                    self._sync_mcp_tools(server_id=sid)
                return
            if target not in servers:
                self._print(_style(f"Unknown MCP server '{target}'.", _C.DIM, enabled=self._color))
                return
            self._sync_mcp_tools(server_id=target)
            return

        self._print(_style("Unknown /mcp command. Try: /mcp help", _C.DIM, enabled=self._color))

    def _get_available_tool_defs(self) -> Dict[str, Any]:
        """Return {tool_name -> {name, description}} for the agent's current tool catalog."""
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

    def _apply_tool_settings_to_active_run(
        self,
        *,
        allowed_tools: Optional[List[str]] = None,
        tool_prompt_examples: Optional[bool] = None,
    ) -> None:
        """Best-effort: persist tool allowlist + tool prompt settings into the active run."""
        state = self._safe_get_state()
        if state is None or not hasattr(state, "vars") or not isinstance(state.vars, dict):
            return

        runtime_ns = state.vars.get("_runtime")
        if not isinstance(runtime_ns, dict):
            runtime_ns = {}
            state.vars["_runtime"] = runtime_ns

        runtime_ns["tool_prompt_examples"] = (
            bool(self._tool_prompt_examples) if tool_prompt_examples is None else bool(tool_prompt_examples)
        )

        if allowed_tools is None:
            runtime_ns.pop("allowed_tools", None)
        else:
            runtime_ns["allowed_tools"] = list(allowed_tools)

        # Keep tool metadata in sync so /memory + footer reflect the *current* allowlist
        # without waiting for the next LLM iteration.
        try:
            logic = getattr(self._agent, "logic", None)
            tool_defs = getattr(logic, "tools", None) if logic is not None else None
            if isinstance(tool_defs, list):
                tool_by_name = {t.name: t for t in tool_defs if getattr(t, "name", None)}
                if allowed_tools is None:
                    ordered_names = [str(t.name) for t in tool_defs if getattr(t, "name", None)]
                else:
                    ordered_names = [str(n) for n in allowed_tools if str(n) in tool_by_name]
                tool_specs: List[Dict[str, Any]] = []
                for name in ordered_names:
                    tool = tool_by_name.get(name)
                    if tool is None:
                        continue
                    to_dict = getattr(tool, "to_dict", None)
                    if callable(to_dict):
                        spec = to_dict()
                        if isinstance(spec, dict):
                            tool_specs.append(spec)
                runtime_ns["tool_specs"] = tool_specs
                import hashlib

                normalized = sorted((dict(s) for s in tool_specs), key=lambda s: str(s.get("name", "")))
                payload = json.dumps(
                    normalized, sort_keys=True, ensure_ascii=False, separators=(",", ":")
                ).encode("utf-8")
                runtime_ns["toolset_id"] = f"ts_{hashlib.sha256(payload).hexdigest()}"
        except Exception:
            pass

        # Invalidate footer cache (tools can change token usage materially).
        self._status_cache_key = None
        self._status_cache_text = ""
        try:
            self._runtime.run_store.save(state)
        except Exception:
            pass

    def _maybe_sync_executor_tools(self) -> None:
        sid = str(getattr(self, "_tool_executor_server_id", "") or "").strip()
        if not sid:
            return

        synced = getattr(self, "_executor_synced_server_ids", None)
        if not isinstance(synced, set):
            synced = set()
            self._executor_synced_server_ids = synced

        if sid in synced:
            return

        servers = getattr(self, "_mcp_servers", None) or {}
        if not isinstance(servers, dict) or sid not in servers:
            return
        try:
            self._sync_mcp_tools(server_id=sid)
            synced.add(sid)
        except Exception:
            return

    def _handle_executor(self, raw: str) -> None:
        """Set the default tool executor for this session (local vs remote MCP server).

        Usage:
          /executor
          /executor status
          /executor list
          /executor use <server_id>
          /executor off
        """
        import shlex

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        sub = parts[0].lower() if parts else "status"
        args = parts[1:] if len(parts) > 1 else []

        if sub in ("help", "-h", "--help"):
            self._print(_style("Usage:", _C.DIM, enabled=self._color))
            self._print(_style("  /executor status", _C.DIM, enabled=self._color))
            self._print(_style("  /executor list", _C.DIM, enabled=self._color))
            self._print(_style("  /executor use <server_id>", _C.DIM, enabled=self._color))
            self._print(_style("  /executor off", _C.DIM, enabled=self._color))
            return

        if sub in ("list",):
            servers = self._mcp_servers or {}
            if not servers:
                self._print(_style("No MCP servers configured. Use /mcp add ...", _C.DIM, enabled=self._color))
                return
            current = str(self._tool_executor_server_id or "").strip()
            self._print(_style("Executors (MCP servers):", _C.CYAN, _C.BOLD, enabled=self._color))
            for sid, entry in sorted(servers.items()):
                transport = str(entry.get("transport") or "").strip() or (
                    "stdio" if "command" in entry else "streamable_http"
                )
                marker = " *" if sid == current else ""
                self._print(f"- {sid} ({transport}){marker}")
            return

        if sub in ("off", "disable", "reset"):
            prev = str(self._tool_executor_server_id or "").strip()
            self._tool_executor_server_id = None
            self._save_config()

            if prev and isinstance(self._allowed_tools, list):
                prefix = f"mcp::{prev}::"
                mapped: List[str] = []
                for name in self._allowed_tools:
                    if isinstance(name, str) and name.startswith(prefix):
                        mapped.append(name[len(prefix) :])
                    else:
                        mapped.append(name)
                self._allowed_tools = mapped
                self._apply_tool_settings_to_active_run(allowed_tools=self._allowed_tools)
                self._save_config()

            self._print(_style("Executor: local (default).", _C.DIM, enabled=self._color))
            return

        if sub in ("use",):
            if not args:
                self._print(_style("Usage: /executor use <server_id>", _C.DIM, enabled=self._color))
                return
            server_id = str(args[0] or "").strip()
            if not server_id:
                return
            if server_id not in (self._mcp_servers or {}):
                self._print(_style(f"Unknown MCP server '{server_id}'. Use /mcp list.", _C.DIM, enabled=self._color))
                return

            # Ensure remote tools exist in the agent catalog.
            self._sync_mcp_tools(server_id=server_id)
            self._executor_synced_server_ids.add(server_id)

            available = self._get_available_tool_defs()
            remote_prefix = f"mcp::{server_id}::"
            remote_names = sorted([n for n in available.keys() if isinstance(n, str) and n.startswith(remote_prefix)])
            remote_set = set(remote_names)
            if not remote_names:
                self._print(_style(f"No remote tools found for '{server_id}' (sync may have failed).", _C.YELLOW, enabled=self._color))
                return

            missing: List[str] = []
            if isinstance(self._allowed_tools, list):
                new_allow: List[str] = []
                for name in self._allowed_tools:
                    if not isinstance(name, str) or not name.strip():
                        continue
                    if name.startswith("mcp::"):
                        new_allow.append(name)
                        continue
                    candidate = remote_prefix + name
                    if candidate in remote_set:
                        new_allow.append(candidate)
                    else:
                        missing.append(name)
                allow = list(dict.fromkeys(new_allow)) if new_allow else list(remote_names)
            else:
                allow = list(remote_names)

            self._tool_executor_server_id = server_id
            self._allowed_tools = allow
            self._apply_tool_settings_to_active_run(allowed_tools=self._allowed_tools)
            self._save_config()

            if missing:
                self._print(
                    _style(
                        "Note: local-only tools disabled (not available on executor): " + ", ".join(sorted(set(missing))),
                        _C.DIM,
                        enabled=self._color,
                    )
                )
            self._print(_style(f"Executor: {server_id} (remote MCP).", _C.GREEN, enabled=self._color))
            return

        # Default: status.
        current = str(self._tool_executor_server_id or "").strip()
        if current:
            self._print(_style(f"Executor: {current} (remote MCP).", _C.DIM, enabled=self._color))
        else:
            self._print(_style("Executor: local (default).", _C.DIM, enabled=self._color))

    def _sync_mcp_tools(self, *, server_id: str) -> None:
        server_id = str(server_id or "").strip()
        if not server_id:
            return

        try:
            from abstractcore.mcp import McpToolSource
            from abstractcore.tools import ToolDefinition
        except Exception as e:
            self._print(_style(f"MCP tools unavailable: {e}", _C.YELLOW, enabled=self._color))
            return

        set_spinner = getattr(self._ui, "set_spinner", None)
        clear_spinner = getattr(self._ui, "clear_spinner", None)
        if callable(set_spinner):
            set_spinner("MCP")
        self._print(_style(f"Syncing MCP tools from '{server_id}'...", _C.DIM, enabled=self._color))
        cache: Dict[str, Any] = {}
        try:
            client = self._get_mcp_client(server_id=server_id, cache=cache)
            entry = (self._mcp_servers or {}).get(server_id) or {}
            transport = str(entry.get("transport") or "").strip() or ("stdio" if "command" in entry else "streamable_http")
            origin_url = entry.get("url") if isinstance(entry, dict) else None
            tool_specs = McpToolSource(server_id=server_id, client=client, transport=transport, origin_url=origin_url).list_tool_specs()
        except Exception as e:
            self._print(_style(f"Failed to sync MCP tools from '{server_id}': {e}", _C.YELLOW, enabled=self._color))
            return
        finally:
            if callable(clear_spinner):
                clear_spinner()
            for c in cache.values():
                try:
                    close = getattr(c, "close", None)
                    if callable(close):
                        close()
                except Exception:
                    pass

        added = 0
        new_tool_defs: List[Any] = []
        for spec in tool_specs:
            if not isinstance(spec, dict):
                continue
            name = spec.get("name")
            desc = spec.get("description")
            params = spec.get("parameters")
            if not isinstance(name, str) or not name.strip():
                continue
            if not isinstance(desc, str) or not desc.strip():
                continue
            if not isinstance(params, dict):
                params = {}

            # Update UI tool catalog.
            self._tool_specs[name] = _ToolSpec(name=name, description=desc, parameters=dict(params))

            # Update agent tool catalog for prompted tool use.
            try:
                tool_def = ToolDefinition(name=name, description=desc, parameters=dict(params))
            except Exception:
                # Skip malformed tools (keep sync best-effort).
                continue
            new_tool_defs.append(tool_def)

        # Register synced tools with the agent's logic so the model can actually call them.
        logic = getattr(self._agent, "logic", None)
        if logic is not None and new_tool_defs:
            register = getattr(logic, "add_tools", None)
            if callable(register):
                try:
                    added = int(register(list(new_tool_defs)))
                except Exception:
                    added = 0
            else:
                # Best-effort fallback for older/alternate logic implementations.
                internal = getattr(logic, "_tools", None)
                if isinstance(internal, list):
                    existing = {getattr(t, "name", None) for t in internal}
                    for td in new_tool_defs:
                        n = getattr(td, "name", None)
                        if n not in existing:
                            internal.append(td)
                            existing.add(n)
                            added += 1
                else:
                    tools_list = getattr(logic, "tools", None)
                    if isinstance(tools_list, list):
                        existing = {getattr(t, "name", None) for t in tools_list}
                        for td in new_tool_defs:
                            n = getattr(td, "name", None)
                            if n not in existing:
                                tools_list.append(td)
                                existing.add(n)
                                added += 1

        self._print(_style(f"Synced {len(tool_specs)} tool(s); added {added} new tool(s).", _C.DIM, enabled=self._color))

    def _get_mcp_client(self, *, server_id: str, cache: Dict[str, Any]) -> Any:
        sid = str(server_id or "").strip()
        if not sid:
            raise ValueError("Missing MCP server_id")

        if sid in cache:
            return cache[sid]

        entry = (self._mcp_servers or {}).get(sid)
        if not isinstance(entry, dict):
            raise ValueError(f"Unknown MCP server '{sid}'")

        factory = getattr(self, "_mcp_client_factory", None)
        if callable(factory):
            client = factory(sid, entry)
        else:
            from abstractcore.mcp import create_mcp_client

            client = create_mcp_client(config=entry)

        cache[sid] = client
        return client

    def _execute_mcp_tool_call(
        self,
        *,
        name: str,
        arguments: Dict[str, Any],
        call_id: str,
        cache: Dict[str, Any],
    ) -> Dict[str, Any]:
        from abstractcore.mcp import parse_namespaced_tool_name

        parsed = parse_namespaced_tool_name(name)
        if parsed is None:
            raise ValueError("Not an MCP tool call")
        server_id, tool_name = parsed

        client = self._get_mcp_client(server_id=server_id, cache=cache)
        mcp_result = client.call_tool(name=tool_name, arguments=arguments)

        def _result_text(res: Any) -> str:
            if not isinstance(res, dict):
                return ""
            content = res.get("content")
            if not isinstance(content, list):
                return ""
            texts: list[str] = []
            for item in content:
                if not isinstance(item, dict):
                    continue
                if item.get("type") != "text":
                    continue
                text = item.get("text")
                if isinstance(text, str) and text.strip():
                    texts.append(text.strip())
            return "\n".join(texts).strip()

        is_error = isinstance(mcp_result, dict) and mcp_result.get("isError") is True
        text = _result_text(mcp_result)

        if is_error:
            return {
                "call_id": call_id,
                "name": name,
                "success": False,
                "output": None,
                "error": text or "MCP tool call reported error",
            }

        # Some MCP servers report failures as plain text while leaving `isError=false`.
        if text:
            t = text.strip()
            if t.startswith("Error:"):
                cleaned = t[len("Error:") :].strip()
                return {
                    "call_id": call_id,
                    "name": name,
                    "success": False,
                    "output": None,
                    "error": cleaned or t,
                }
            if t.startswith(("❌", "🚫", "⏰")):
                cleaned = t.lstrip("❌🚫⏰").strip()
                if cleaned.startswith("Error:"):
                    cleaned = cleaned[len("Error:") :].strip()
                return {
                    "call_id": call_id,
                    "name": name,
                    "success": False,
                    "output": None,
                    "error": cleaned or t,
                }

        output: Any
        if text:
            try:
                output = json.loads(text)
            except Exception:
                output = text
        else:
            output = mcp_result

        return {
            "call_id": call_id,
            "name": name,
            "success": True,
            "output": output,
            "error": None,
        }

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

    def _handle_memory(self, raw: str = "") -> None:
        """Show MemAct Active Memory blocks (MemAct only).

        Usage:
          /memory
          /memory <component>
        """
        import shlex

        if self._agent_kind != "memact":
            self._print(_style("Active Memory is only available for MemAct.", _C.DIM, enabled=self._color))
            self._print(_style("Run with: abstractcode --agent memact", _C.DIM, enabled=self._color))
            return

        state = self._safe_get_state()
        if state is None or not hasattr(state, "vars") or not isinstance(state.vars, dict):
            self._print(_style("No run loaded. Start a task or /resume first.", _C.DIM, enabled=self._color))
            return

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        arg0 = parts[0].strip() if parts else ""

        component_aliases = {
            "memory_blueprints": "memory_blueprints",
            "memory-blueprints": "memory_blueprints",
            "blueprints": "memory_blueprints",
            "persona": "persona",
            "relationships": "relationships",
            "rels": "relationships",
            "current_tasks": "current_tasks",
            "current-tasks": "current_tasks",
            "tasks": "current_tasks",
            "current_context": "current_context",
            "current-context": "current_context",
            "context": "current_context",
            "critical_insights": "critical_insights",
            "critical-insights": "critical_insights",
            "insights": "critical_insights",
            "references": "references",
            "refs": "references",
            "history": "history",
        }

        if arg0.lower() in ("help", "-h", "--help"):
            self._print(_style("Usage:", _C.DIM, enabled=self._color))
            self._print(_style("  /memory", _C.DIM, enabled=self._color))
            self._print(_style("  /memory <component>", _C.DIM, enabled=self._color))
            self._print(
                _style(
                    "Components: memory_blueprints, persona, relationships, current_tasks, current_context, critical_insights, references, history",
                    _C.DIM,
                    enabled=self._color,
                )
            )
            return

        from abstractruntime.memory.active_memory import ensure_memact_memory, render_memact_blocks

        ensure_memact_memory(state.vars)
        blocks = render_memact_blocks(state.vars)

        if arg0:
            key = arg0.strip().lower()
            cid = component_aliases.get(key) or component_aliases.get(key.replace("-", "_"))
            if not cid:
                self._print(_style(f"Unknown memory component: {arg0}", _C.YELLOW, enabled=self._color))
                self._print(_style("Type /memory --help for component names.", _C.DIM, enabled=self._color))
                return

            block = next((b for b in blocks if str(b.get("component_id") or "") == cid), None)
            if not isinstance(block, dict):
                self._print(_style(f"Missing component: {cid}", _C.YELLOW, enabled=self._color))
                return

            title = str(block.get("title") or cid).strip()
            content = str(block.get("content") or "").rstrip()
            self._print(_style(f"\nMemory: {cid}", _C.CYAN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 80, _C.DIM, enabled=self._color))
            self._print(_style(f"## {title}", _C.CYAN, enabled=self._color))
            self._print(content if content else "(empty)")
            return

        mem = state.vars.get("_runtime", {}).get("active_memory", {})
        self._print(_style("\nMemAct Active Memory (summary)", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))

        def _count(key: str) -> int:
            v = mem.get(key) if isinstance(mem, dict) else None
            return len(v) if isinstance(v, list) else 0

        self._print(_style(f"relationships: {_count('relationships')}", _C.DIM, enabled=self._color))
        self._print(_style(f"current_tasks: {_count('current_tasks')}", _C.DIM, enabled=self._color))
        self._print(_style(f"current_context: {_count('current_context')}", _C.DIM, enabled=self._color))
        self._print(_style(f"critical_insights: {_count('critical_insights')}", _C.DIM, enabled=self._color))
        self._print(_style(f"references: {_count('references')}", _C.DIM, enabled=self._color))
        self._print(_style(f"history: {_count('history')}", _C.DIM, enabled=self._color))
        self._print(_style("Tip: /memory <component> shows verbatim content.", _C.DIM, enabled=self._color))

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

            # Token deltas should reflect the *LLM-visible* prompt view (respects max_history_messages),
            # not necessarily the full stored context list.
            before_messages_for_llm = self._select_messages_for_llm(state)
            before_est = self._estimate_next_prompt_tokens(
                state=state,
                messages=before_messages_for_llm,
                effective_model=self._get_effective_model(state),
            )
            before_tokens = int(before_est.get("total_tokens") or 0)
            if before_tokens < 0:
                before_tokens = 0

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
            # Force status bar recompute (compaction can rewrite earlier messages without changing the tail).
            self._status_cache_key = None
            self._status_cache_text = ""

            # Calculate stats
            after_messages_for_llm = self._select_messages_for_llm(updated)
            after_est = self._estimate_next_prompt_tokens(
                state=updated,
                messages=after_messages_for_llm,
                effective_model=self._get_effective_model(updated),
            )
            after_tokens = int(after_est.get("total_tokens") or 0)
            if after_tokens < 0:
                after_tokens = 0
            reduction = ((before_tokens - after_tokens) / before_tokens * 100) if before_tokens > 0 else 0

            self._ui.clear_spinner()

            self._print(_style("\n✅ Compaction complete!", _C.GREEN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 40, _C.DIM, enabled=self._color))
            self._print(f"Messages:   {len(messages)} → {len(new_messages)}")
            self._print(f"Tokens (next prompt, est): {before_tokens:,} → {after_tokens:,} ({reduction:.0f}% reduction)")
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
        """(Deprecated) Legacy context preview command.

        The public `/context` and `/llm` commands were removed in favor of:
        - `/log runtime`
        - `/log provider`
        """
        import copy
        import shlex
        import uuid

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        if parts and str(parts[0] or "").strip().lower() in ("last", "prev", "previous"):
            rest = parts[1:]
            rest_raw = shlex.join(rest) if hasattr(shlex, "join") else " ".join(rest)
            self._handle_log_runtime(rest_raw)
            return

        copy_to_clipboard = False
        # Accept `copy` as either a leading or trailing token (UX: "/log runtime ... copy").
        if parts and str(parts[0] or "").strip().lower() == "copy":
            copy_to_clipboard = True
            parts = parts[1:]
        elif parts and str(parts[-1] or "").strip().lower() == "copy":
            copy_to_clipboard = True
            parts = parts[:-1]

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
            self._print(
                _style(
                    "Usage: /log runtime  |  /log provider",
                    _C.DIM,
                    enabled=self._color,
                )
            )
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
                "tip": "Use /log runtime (or /log provider) to inspect durable LLM/tool call payloads from the last run.",
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
            if copy_to_clipboard:
                ok = self._copy_to_clipboard(text)
                self._print(
                    _style(
                        "Copied." if ok else "Copy failed (no clipboard helper found).",
                        _C.DIM,
                        enabled=self._color,
                    )
                )
                return
            copy_id = f"context_{uuid.uuid4().hex}"
            self._ui.register_copy_payload(copy_id, text)
            self._print(_style("\nContext (next /task seed)", _C.CYAN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 80, _C.DIM, enabled=self._color))
            self._print(text)
            self._print(f"[[COPY:{copy_id}]]")
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
            elif self._agent_kind == "codeact":
                from abstractagent.adapters.codeact_runtime import create_codeact_workflow

                workflow = create_codeact_workflow(logic=logic, on_step=None)
            else:
                from abstractagent.adapters.memact_runtime import create_memact_workflow

                workflow = create_memact_workflow(logic=logic, on_step=None)
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
        if copy_to_clipboard:
            ok = self._copy_to_clipboard(text)
            self._print(
                _style(
                    "Copied." if ok else "Copy failed (no clipboard helper found).",
                    _C.DIM,
                    enabled=self._color,
                )
            )
            return
        copy_id = f"context_{uuid.uuid4().hex}"
        self._ui.register_copy_payload(copy_id, text)

        self._print(_style("\nContext (next LLM call)", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))
        self._print(text)

        if json_only:
            self._print(f"[[COPY:{copy_id}]]")
            return

        llm_payload = out.get("llm_call_payload")
        if not isinstance(llm_payload, dict):
            self._print(f"[[COPY:{copy_id}]]")
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

        self._print(f"[[COPY:{copy_id}]]")

    def _handle_log_runtime(self, raw: str) -> None:
        """Show the runtime-centric step trace for LLM/tool calls (durable).

        Source of truth: `RunState.vars["_runtime"]["node_traces"]`.

        Usage:
          /log runtime [copy] [--last] [--json-only] [--save <path>]
        """
        import shlex
        import uuid
        from pathlib import Path

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        copy_to_clipboard = False
        # Accept `copy` as either a leading or trailing token (UX: "/log runtime ... copy").
        if parts and str(parts[0] or "").strip().lower() == "copy":
            copy_to_clipboard = True
            parts = parts[1:]
        elif parts and str(parts[-1] or "").strip().lower() == "copy":
            copy_to_clipboard = True
            parts = parts[:-1]

        json_only = False
        last_only = False
        all_calls = False
        verbatim = False
        save_path: Optional[str] = None
        usage = "Usage: /log runtime [copy] [--last] [--json-only] [--save <path>]"
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
            if p in ("--all", "--full", "--cycle"):
                all_calls = True
                i += 1
                continue
            if p in ("--verbatim", "--context", "--messages"):
                verbatim = True
                i += 1
                continue
            if p in ("--save", "--out", "--output"):
                if i + 1 >= len(parts):
                    self._print(_style(usage, _C.DIM, enabled=self._color))
                    return
                save_path = parts[i + 1]
                i += 2
                continue
            self._print(_style(f"Unknown flag: {p}", _C.YELLOW, enabled=self._color))
            self._print(_style(usage, _C.DIM, enabled=self._color))
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

        counts: Dict[str, int] = {}
        llm_steps: List[Dict[str, Any]] = []
        tool_steps: List[Dict[str, Any]] = []
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
                    llm_steps.append(step)
                    continue
                if etype == "tool_calls":
                    tool_steps.append(step)
                    continue

        if not llm_steps and not tool_steps:
            self._print(
                _style("No llm_call or tool_calls steps found in node_traces.", _C.DIM, enabled=self._color)
            )
            return

        llm_steps.sort(key=lambda d: str(d.get("ts") or ""))
        tool_steps.sort(key=lambda d: str(d.get("ts") or ""))
        if all_calls:
            last_only = False

        # For copy-to-clipboard, default to the last LLM call unless the user explicitly asked for --all.
        if copy_to_clipboard and not last_only and not all_calls:
            last_only = True

        if last_only and llm_steps:
            llm_steps = [llm_steps[-1]]

        prompt_text, answer_text = self._extract_latest_turn_prompt_and_answer(state)

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

        tool_calls_out: List[Dict[str, Any]] = []
        for idx, step in enumerate(tool_steps, 1):
            eff = step.get("effect") if isinstance(step.get("effect"), dict) else {}
            payload = eff.get("payload") if isinstance(eff.get("payload"), dict) else {}
            result = step.get("result") if isinstance(step.get("result"), dict) else step.get("result")
            tool_calls_out.append(
                {
                    "index": idx,
                    "ts": step.get("ts"),
                    "node_id": step.get("node_id"),
                    "status": step.get("status"),
                    "duration_ms": step.get("duration_ms"),
                    "tool_calls_payload": payload,
                    "result": result,
                    "error": step.get("error"),
                }
            )

        out: Dict[str, Any] = {
            "run_id": getattr(state, "run_id", None),
            "run_status": getattr(getattr(state, "status", None), "value", None) or str(getattr(state, "status", "")),
            "provider": self._provider,
            "model": self._model,
            "last_turn": {"prompt": prompt_text, "answer": answer_text},
            "counts_by_effect_type": dict(counts),
            "llm_calls": calls_out,
            "tool_calls": tool_calls_out,
        }
        text = json.dumps(out, ensure_ascii=False, indent=2, sort_keys=False, default=str)

        if save_path:
            try:
                path = Path(save_path).expanduser()
                if not path.is_absolute():
                    path = Path.cwd() / path
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text, encoding="utf-8")
                self._print(_style(f"✅ Saved runtime log payloads to {path}", _C.DIM, enabled=self._color))
            except Exception as e:
                self._print(_style(f"❌ Failed to save: {e}", _C.DIM, enabled=self._color))

        if copy_to_clipboard:
            ok = self._copy_to_clipboard(text)
            self._print(_style("Copied." if ok else "Copy failed (no clipboard helper found).", _C.DIM, enabled=self._color))
            return

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

        self._print(_style("\nContext (last prompt/answer/steps; runtime)", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))

        if json_only:
            self._print(text)
            self._print(f"[[COPY:{copy_id}]]")
            return

        prompt_view = str(prompt_text or "").rstrip()
        answer_view = str(answer_text or "").rstrip()

        self._print(_style("\nLast prompt (user)", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))
        self._print(prompt_view if prompt_view else "(no user prompt captured)")

        self._print(_style("\nLast answer (assistant)", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))
        self._print(answer_view if answer_view else "(no assistant answer produced yet)")

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

        # TOOL_CALLS steps are part of the agent "step trace" (what happened between LLM calls).
        if tool_calls_out:
            self._print(_style("\nTool calls (runtime; verbatim payloads)", _C.CYAN, _C.BOLD, enabled=self._color))
            self._print(_style("─" * 80, _C.DIM, enabled=self._color))

            for batch in tool_calls_out:
                idx = batch.get("index")
                ts = batch.get("ts")
                node_id = batch.get("node_id")
                status = batch.get("status")
                dur = batch.get("duration_ms")
                header = f"TOOL_CALLS #{idx} ({status}) node={node_id} ts={ts}"
                if isinstance(dur, (int, float)):
                    header += f" duration_ms={dur:.1f}"
                self._print(_style("\n" + header, _C.CYAN, _C.BOLD, enabled=self._color))
                self._print(_style("─" * 80, _C.DIM, enabled=self._color))

                runtime_payload = batch.get("tool_calls_payload") or {}
                runtime_text = json.dumps(runtime_payload, ensure_ascii=False, indent=2, sort_keys=False, default=str)
                rid = f"tool_runtime_{uuid.uuid4().hex}"
                self._ui.register_copy_payload(rid, runtime_text)
                self._print(_style("Runtime TOOL_CALLS payload (durable)", _C.DIM, enabled=self._color))
                self._print(f"[[COPY:{rid}]]")
                self._print(runtime_text)

                res_payload = batch.get("result")
                res_text = json.dumps(res_payload, ensure_ascii=False, indent=2, sort_keys=False, default=str)
                res_id = f"tool_res_{uuid.uuid4().hex}"
                self._ui.register_copy_payload(res_id, res_text)
                self._print(_style("\nTool execution result", _C.DIM, enabled=self._color))
                self._print(f"[[COPY:{res_id}]]")
                self._print(res_text)

                err = batch.get("error")
                if err is not None:
                    err_text = json.dumps(err, ensure_ascii=False, indent=2, sort_keys=False, default=str)
                    err_id = f"tool_err_{uuid.uuid4().hex}"
                    self._ui.register_copy_payload(err_id, err_text)
                    self._print(_style("\nTool execution error", _C.DIM, enabled=self._color))
                    self._print(f"[[COPY:{err_id}]]")
                    self._print(err_text)

        self._print(f"[[COPY:{copy_id}]]")

    def _handle_log_provider(self, raw: str) -> None:
        """Show the provider wire request/response for past LLM calls (durable).

        Source of truth: durable runtime state + ledger.
        - request: `result.metadata._provider_request` (captured by AbstractCore providers/clients)
        - response: `result.raw_response` (provider JSON response, best-effort preserved)

        Usage:
          /log provider [copy] [--last] [--run] [--json-only] [--save <path>]
        """
        import shlex
        import uuid
        from pathlib import Path

        try:
            parts = shlex.split(raw) if raw else []
        except ValueError:
            parts = raw.split() if raw else []

        copy_to_clipboard = False
        # Accept `copy` as either a leading or trailing token (UX: "/log provider --all copy").
        if parts and str(parts[0] or "").strip().lower() == "copy":
            copy_to_clipboard = True
            parts = parts[1:]
        elif parts and str(parts[-1] or "").strip().lower() == "copy":
            copy_to_clipboard = True
            parts = parts[:-1]

        json_only = False
        last_only = False
        run_only = False
        no_tool_defs = False
        save_path: Optional[str] = None
        usage = "Usage: /log provider [copy] [--last] [--run] [--json-only] [--no-tool-defs] [--save <path>]"

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
            if p in ("--run", "--current-run", "--this-run"):
                run_only = True
                i += 1
                continue
            if p in ("--no-tool-defs", "--no_tool_defs"):
                no_tool_defs = True
                i += 1
                continue
            if p in ("--save", "--out", "--output"):
                if i + 1 >= len(parts):
                    self._print(_style(usage, _C.DIM, enabled=self._color))
                    return
                save_path = parts[i + 1]
                i += 2
                continue
            self._print(_style(f"Unknown flag: {p}", _C.YELLOW, enabled=self._color))
            self._print(_style(usage, _C.DIM, enabled=self._color))
            return

        state = self._safe_get_state()
        if state is None or not hasattr(state, "vars"):
            self._print(_style("No run loaded. Use /resume or start a task first.", _C.DIM, enabled=self._color))
            return

        # --- Collect provider calls from the durable ledger (authoritative, append-only) ---
        #
        # Why ledger over node_traces?
        # - node_traces are intentionally bounded per-node (default 100 entries) and may drop older calls.
        # - ledger is append-only per run, suitable for "show me everything" observability.
        run_id = getattr(state, "run_id", None)
        if not isinstance(run_id, str) or not run_id:
            self._print(_style("No run_id found on current state.", _C.DIM, enabled=self._color))
            return

        session_id = getattr(state, "session_id", None)

        # Default behavior: include the full session (all runs sharing session_id), unless --run is set.
        run_ids: List[str] = [run_id]
        try:
            from abstractruntime.storage.base import QueryableRunStore

            if not run_only and isinstance(session_id, str) and session_id.strip() and isinstance(self._runtime.run_store, QueryableRunStore):
                # Pull a bounded set of runs and filter client-side by session_id.
                # (QueryableRunStore doesn't expose a session_id filter in v0.1.)
                # Prefer completeness over speed: /log provider is a debugging tool.
                # JsonFileRunStore scans all run_*.json files anyway; a low limit can hide older runs.
                runs = self._runtime.run_store.list_runs(limit=100000)
                run_ids = [r.run_id for r in runs if getattr(r, "session_id", None) == session_id]
                # Ensure deterministic chronological order by created_at (fallback to updated_at).
                runs_by_id = {r.run_id: r for r in runs}
                run_ids.sort(
                    key=lambda rid: (
                        str(getattr(runs_by_id.get(rid), "created_at", "") or ""),
                        str(getattr(runs_by_id.get(rid), "updated_at", "") or ""),
                        str(rid),
                    )
                )
        except Exception:
            # Fall back to current run only.
            run_ids = [run_id]

        calls: List[Dict[str, Any]] = []
        for rid in run_ids:
            try:
                records = self._runtime.ledger_store.list(str(rid))
            except Exception:
                continue
            if not isinstance(records, list):
                continue

            for rec in records:
                if not isinstance(rec, dict):
                    continue
                eff = rec.get("effect")
                if not isinstance(eff, dict):
                    continue
                if str(eff.get("type") or "") != "llm_call":
                    continue
                status = str(rec.get("status") or "")
                result = rec.get("result") if isinstance(rec.get("result"), dict) else {}
                meta = result.get("metadata") if isinstance(result.get("metadata"), dict) else {}
                provider_req = meta.get("_provider_request") if isinstance(meta, dict) else None
                provider_resp = result.get("raw_response")
                err = rec.get("error")

                calls.append(
                    {
                        "run_id": str(rid),
                        "node_id": rec.get("node_id"),
                        "ts": rec.get("ended_at") or rec.get("started_at"),
                        "status": status,
                        "error": err,
                        "request_sent": provider_req,
                        "response_received": provider_resp,
                    }
                )

        if not calls:
            self._print(_style("No completed llm_call records found in the ledger for this scope.", _C.DIM, enabled=self._color))
            return

        calls.sort(key=lambda d: str(d.get("ts") or ""))
        if last_only:
            calls = [calls[-1]]

        def _tool_name_from_def(obj: Any) -> Optional[str]:
            if isinstance(obj, str) and obj.strip():
                return obj.strip()
            if isinstance(obj, dict):
                name = obj.get("name")
                if isinstance(name, str) and name.strip():
                    return name.strip()
                fn = obj.get("function")
                if isinstance(fn, dict):
                    fn_name = fn.get("name")
                    if isinstance(fn_name, str) and fn_name.strip():
                        return fn_name.strip()
                return None
            # Best-effort for ToolDefinition-like objects
            try:
                name2 = getattr(obj, "name", None)
                if isinstance(name2, str) and name2.strip():
                    return name2.strip()
            except Exception:
                pass
            return None

        def _simplify_tools(obj: Any) -> Any:
            """Return a copy of obj where `tools` is replaced with tool names (if present)."""
            if not isinstance(obj, dict):
                return obj
            tools = obj.get("tools")
            if not isinstance(tools, list):
                return obj
            names: List[str] = []
            for t in tools:
                n = _tool_name_from_def(t)
                if isinstance(n, str) and n:
                    names.append(n)
            obj2 = dict(obj)
            obj2["tools"] = names
            return obj2

        def _simplify_provider_request(req: Any) -> Any:
            if not isinstance(req, dict):
                return req
            req2 = dict(req)
            payload = req2.get("payload")
            if isinstance(payload, dict):
                req2["payload"] = _simplify_tools(payload)
            call_params = req2.get("call_params")
            if isinstance(call_params, dict):
                req2["call_params"] = _simplify_tools(call_params)
            return req2

        if no_tool_defs:
            for c in calls:
                if not isinstance(c, dict):
                    continue
                c["request_sent"] = _simplify_provider_request(c.get("request_sent"))

        out: Dict[str, Any] = {
            "kind": "provider_wire_export",
            "scope": "run" if run_only or not isinstance(session_id, str) else "session",
            "session_id": session_id,
            "run_id": run_id,
            "provider": self._provider,
            "model": self._model,
            "calls": calls,
        }

        text = json.dumps(out, ensure_ascii=False, indent=2, sort_keys=False, default=str)

        # LMStudio-ish log rendering (human-friendly; still verbatim bodies).
        def _lmstudio_like_text() -> str:
            from urllib.parse import urlparse

            def _json(obj: Any) -> str:
                return json.dumps(obj, ensure_ascii=False, indent=2, sort_keys=False, default=str)

            def _extract_tool_calls(resp_obj: Any) -> list[Any]:
                """Best-effort tool-call extraction for human-readable logs.

                `/log provider` is intentionally provider-wire oriented, but this line helps
                quickly spot whether the model asked for tools. Not all providers share the
                OpenAI `choices[0].message.tool_calls` shape (e.g. Anthropic `tool_use` blocks).
                """
                if not isinstance(resp_obj, dict):
                    return []

                # OpenAI-compatible shape (including many local servers).
                try:
                    choices = resp_obj.get("choices")
                    if isinstance(choices, list) and choices:
                        ch0 = choices[0] if isinstance(choices[0], dict) else {}
                        msg0 = ch0.get("message") if isinstance(ch0, dict) else {}
                        msg0 = msg0 if isinstance(msg0, dict) else {}
                        tcs = msg0.get("tool_calls")
                        if isinstance(tcs, list):
                            return tcs
                except Exception:
                    pass

                # Anthropic Messages API shape: tool calls are `content` blocks with type=tool_use.
                content = resp_obj.get("content")
                if isinstance(content, list):
                    out: list[dict[str, Any]] = []
                    for block in content:
                        if not isinstance(block, dict):
                            continue
                        if str(block.get("type") or "") != "tool_use":
                            continue
                        out.append(
                            {
                                "type": "tool_use",
                                "call_id": block.get("id"),
                                "name": block.get("name"),
                                "arguments": block.get("input"),
                            }
                        )
                    if out:
                        return out

                return []

            blocks: List[str] = []
            for c in calls:
                ts = str(c.get("ts") or "")
                rid = c.get("run_id")
                node_id = c.get("node_id")
                status = str(c.get("status") or "")
                err = c.get("error")

                req = c.get("request_sent")
                req_url = req.get("url") if isinstance(req, dict) else None
                req_payload = req.get("payload") if isinstance(req, dict) else None
                path = urlparse(str(req_url)).path if isinstance(req_url, str) and req_url else ""
                endpoint = path or (str(req_url) if isinstance(req_url, str) else "(unknown)")

                model = None
                n_messages = None
                if isinstance(req_payload, dict):
                    model = req_payload.get("model")
                    msgs = req_payload.get("messages")
                    n_messages = len(msgs) if isinstance(msgs, list) else None

                resp = c.get("response_received")
                tool_calls_val: Any = _extract_tool_calls(resp) if resp is not None else []

                # Minimal prefix similar to LMStudio; include run/node for disambiguation (not present in LMStudio logs).
                prefix_dbg = f"{ts} [DEBUG]"
                prefix_inf = f"{ts} [INFO]"
                model_tag = f"[{model}]" if isinstance(model, str) and model else ""
                extra = f" (run={rid} node={node_id})" if rid or node_id else ""

                if isinstance(req_payload, dict):
                    blocks.append(f"{prefix_dbg} Received request: POST to {endpoint} with body  {_json(req_payload)}")
                else:
                    blocks.append(f"{prefix_dbg} Received request: (missing provider request capture){extra}")

                if isinstance(n_messages, int):
                    blocks.append(f"{prefix_inf} {model_tag} Running chat completion on conversation with {n_messages} messages.{extra}")
                else:
                    blocks.append(f"{prefix_inf} {model_tag} Running chat completion.{extra}")

                blocks.append(f"{prefix_inf} {model_tag} Model generated tool calls:  {_json(tool_calls_val)}{extra}")

                if resp is None:
                    # Preserve failure info when available.
                    if status and status != "completed":
                        blocks.append(f"{prefix_inf} {model_tag} Generated prediction:  (no provider response captured; status={status}){extra}")
                    elif err:
                        blocks.append(f"{prefix_inf} {model_tag} Generated prediction:  (no provider response captured; error={err}){extra}")
                    else:
                        blocks.append(f"{prefix_inf} {model_tag} Generated prediction:  (missing provider response capture){extra}")
                else:
                    blocks.append(f"{prefix_inf} {model_tag} Generated prediction:  {_json(resp)}{extra}")

                blocks.append("")

            return "\n".join(blocks).rstrip()

        pretty_text = _lmstudio_like_text()

        if save_path:
            try:
                path = Path(save_path).expanduser()
                if not path.is_absolute():
                    path = Path.cwd() / path
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text if json_only else pretty_text, encoding="utf-8")
                self._print(_style(f"✅ Saved provider log payloads to {path}", _C.DIM, enabled=self._color))
            except Exception as e:
                self._print(_style(f"❌ Failed to save: {e}", _C.DIM, enabled=self._color))

        if copy_to_clipboard:
            ok = self._copy_to_clipboard(text if json_only else pretty_text)
            self._print(_style("Copied." if ok else "Copy failed (no clipboard helper found).", _C.DIM, enabled=self._color))
            return

        copy_id = f"log_provider_{uuid.uuid4().hex}"
        self._ui.register_copy_payload(copy_id, text if json_only else pretty_text)

        self._print(_style("\nLog (provider wire)", _C.CYAN, _C.BOLD, enabled=self._color))
        self._print(_style("─" * 80, _C.DIM, enabled=self._color))
        self._print(text if json_only else pretty_text)
        self._print(f"[[COPY:{copy_id}]]")

    def _handle_memorize(self, raw: str) -> None:
        """Store a durable memory note (runtime MEMORY_NOTE) with optional tags and provenance.

        Usage:
          /memorize <note text> [--tag k=v ...] [--span <span_id>] [--last-span] [--last N] [--scope run|session|global]
        """
        from .remember import parse_remember_args, store_memory_note

        state = self._safe_get_state()
        if state is None or not hasattr(state, "run_id") or not hasattr(state, "vars"):
            self._print(_style("No run loaded. Use /resume or start a task first.", _C.DIM, enabled=self._color))
            return

        try:
            req = parse_remember_args(raw)
        except Exception as e:
            self._print(_style(f"Memorize parse error: {e}", _C.YELLOW, enabled=self._color))
            self._print(
                _style(
                    "Usage: /memorize <note text> [--tag k=v ...] [--span <span_id>] [--last-span] [--last N] [--scope run|session|global]",
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
                call_id="memorize",
                scope=req.scope,
            )
        except Exception as e:
            self._print(_style(f"Memorize failed: {e}", _C.YELLOW, enabled=self._color))
            return

        # Extract span_id if present.
        span_id = None
        meta = result.get("results") if isinstance(result, dict) else None
        if isinstance(meta, list) and meta:
            first = meta[0] if isinstance(meta[0], dict) else {}
            first_meta = first.get("meta") if isinstance(first, dict) else None
            if isinstance(first_meta, dict):
                span_id = first_meta.get("span_id")

        self._print(_style("\n✅ Memorized.", _C.GREEN, enabled=self._color))
        if isinstance(span_id, str) and span_id:
            self._print(_style(f"span_id={span_id}", _C.DIM, enabled=self._color))
        if req.tags:
            tags_txt = ", ".join([f"{k}={v}" for k, v in sorted(req.tags.items())])
            self._print(_style(f"tags: {tags_txt}", _C.DIM, enabled=self._color))

    def _show_help(self) -> None:
        self._print(
            "\nCommands:\n"
            "  /help               Show this message\n"
            "  /mcp                Configure MCP tool servers (discovery + execution) [saved]\n"
            "                     - /mcp list\n"
            "                     - /mcp add <id> <url> [--header K=V ...]          (Streamable HTTP)\n"
            "                     - /mcp add <id> stdio [--cwd PATH] [--env K=V] -- <command...>\n"
            "                     - /mcp sync [id|all]\n"
            "                     - /mcp remove <id>\n"
            "                     - Example (HTTP): /mcp add context7 https://mcp.context7.com/mcp\n"
            "                     - Example (HTTP+token): /mcp add remote http://HOST:8765 --header X-Abstract-Worker-Token=TOKEN\n"
            "                     - Example (SSH stdio): /mcp add remote stdio -- ssh user@HOST abstractruntime-mcp-worker --toolsets files,system\n"
            "                     - After sync, tools are available as: mcp::<id>::<tool_name>\n"
            "  /tools              List/configure tool allowlist [saved]\n"
            "                     - /tools reset\n"
            "                     - /tools examples on|off\n"
            "                     - /tools only <name...>\n"
            "                     - /tools enable <name...>\n"
            "                     - /tools disable <name...>\n"
            "  /executor           Set default tool executor [saved]\n"
            "                     - /executor status\n"
            "                     - /executor list\n"
            "                     - /executor use <server_id>\n"
            "                     - /executor off\n"
            "                     - Note: /executor changes where TOOLS run (local vs MCP server).\n"
            "                             It does NOT move the LLM; use --base-url or ABSTRACTCODE_BASE_URL.\n"
            "  /tool-specs         Show full tool schemas (params)\n"
            "  /status             Show current run status\n"
            "  /auto-accept        Toggle auto-accept for tools [saved]\n"
            "  /plan [on|off]      Toggle Plan mode (TODO list first) [saved]\n"
            "  /review ...         Toggle Review mode (self-check) [saved]\n"
            "                     - /review [on|off] [max_rounds]\n"
            "                     - /review rounds <N>\n"
            "  /max-tokens [N]     Show or set max tokens (-1 = auto) [saved]\n"
            "  /max-messages [N]   Show or set max history messages (-1 = unlimited) [saved]\n"
            "  /memory             Show MemAct Active Memory (MemAct only)\n"
            "  /compact [mode]     Compress conversation context [light|standard|heavy]\n"
            "  /spans              List archived conversation spans (from /compact)\n"
            "  /expand <span>      Expand an archived span (--show, --into-context)\n"
            "  /recall [opts]      Recall spans by time/tags/query (--into-context)\n"
            "  /vars [path]        Inspect run vars (scratchpad, _runtime, ...)\n"
            "  /log runtime        Show runtime step trace for LLM/tool calls (durable)\n"
            "                     - /log runtime [copy] [--last] [--json-only] [--save <path>]\n"
            "  /log provider       Show provider wire request+response (durable)\n"
            "                     - /log provider [copy] [--last|--all] [--json-only] [--save <path>]\n"
            "  /memorize <note>    Store a durable memory note (tags + provenance)\n"
            "  /mouse              Toggle mouse mode (wheel scroll vs terminal selection)\n"
            "  /flow ...           Run AbstractFlow workflows inside this REPL\n"
            "                     - /flow run <flow_id_or_path> [--verbosity none|default|full] [--key value ...]\n"
            "                     - /flow resume [--verbosity none|default|full] [--wait-until]\n"
            "                     - /flow pause | resume-run | cancel\n"
            "                     - Example: /flow run deep-research-pro --query \"who are you?\" --max_web_search 10\n"
            "  /copy ...           Copy messages to clipboard\n"
            "                     - /copy user [turn] | assistant [turn] | turn <N>\n"
            "  /history [N]        Show recent conversation history\n"
            "  /history copy       Copy full conversation history to clipboard\n"
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
                ts_text = self._format_timestamp_short(str(msg.get("timestamp") or "")) if isinstance(msg, dict) else ""
                footer = _style(ts_text, _C.DIM, enabled=self._color) if ts_text else ""
                if role == "user":
                    mid = _get_message_id(msg) or f"user_{uuid.uuid4().hex}"
                    self._ui.register_copy_payload(mid, content)
                    self._print(self._format_user_prompt_block(content, copy_id=mid, footer=footer))
                    continue

                if role == "tool":
                    meta = msg.get("metadata") if isinstance(msg.get("metadata"), dict) else {}
                    name = meta.get("name") if isinstance(meta, dict) else None
                    label = f"[tool:{name}]" if isinstance(name, str) and name else "[tool]"
                    mid = _get_message_id(msg) or f"tool_{uuid.uuid4().hex}"
                    self._ui.register_copy_payload(mid, f"{label}\n{content}".strip())
                    self._print(_style(label, _C.DIM, enabled=self._color))
                    self._print(content)
                    self._print(f"[[COPY:{mid}]] {footer}".rstrip())
                    continue

                if role == "system":
                    mid = _get_message_id(msg) or f"system_{uuid.uuid4().hex}"
                    self._ui.register_copy_payload(mid, content)
                    self._print(_style("[system]", _C.DIM, enabled=self._color))
                    self._print(content)
                    self._print(f"[[COPY:{mid}]] {footer}".rstrip())
                    continue

                # Default: assistant/other roles (no role prefix; rely on styling/structure).
                mid = _get_message_id(msg) or f"assistant_{uuid.uuid4().hex}"
                self._ui.register_copy_payload(mid, content)
                self._print(content)
                self._print(f"[[COPY:{mid}]] {footer}".rstrip())

        self._print(_style("\n" + "─" * 80, _C.DIM, enabled=self._color))

    def _copy_full_history_to_clipboard(self) -> None:
        """Copy the full conversation transcript to clipboard (best-effort)."""
        state = self._safe_get_state()
        messages = list(self._agent.session_messages or []) if state is None else self._messages_from_state(state)
        turns = self._group_messages_into_turns(messages)
        if not turns:
            self._print("No history yet.")
            return

        blocks: List[str] = []
        for idx, turn in enumerate(turns, start=1):
            blocks.append(f"# Turn {idx}")
            for msg in turn:
                if not isinstance(msg, dict):
                    continue
                role = str(msg.get("role") or "unknown")
                content = "" if msg.get("content") is None else str(msg.get("content"))
                if role == "tool":
                    meta = msg.get("metadata") if isinstance(msg.get("metadata"), dict) else {}
                    name = meta.get("name") if isinstance(meta, dict) else None
                    label = f"tool[{name}]" if isinstance(name, str) and name else "tool"
                else:
                    label = role
                blocks.append(f"{label}:\n{content}".rstrip())

        payload = "\n\n".join([b for b in blocks if b]).strip()
        ok = self._copy_to_clipboard(payload)
        self._print(_style("Copied." if ok else "Copy failed (no clipboard helper found).", _C.DIM, enabled=self._color))

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

    def _sync_tool_prompt_settings_to_run(self, run_id: str) -> None:
        """Best-effort: persist session tool-prompt settings into the run vars."""
        rid = str(run_id or "").strip()
        if not rid:
            return
        try:
            state = self._runtime.get_state(rid)
        except Exception:
            return
        if state is None or not hasattr(state, "vars") or not isinstance(state.vars, dict):
            return
        runtime_ns = state.vars.get("_runtime")
        if not isinstance(runtime_ns, dict):
            runtime_ns = {}
            state.vars["_runtime"] = runtime_ns
        runtime_ns["tool_prompt_examples"] = bool(self._tool_prompt_examples)
        try:
            self._runtime.run_store.save(state)
        except Exception:
            pass

    def _start(self, task: str) -> None:
        if self._run_thread_active():
            self._print(_style("A run is already executing. Use /pause or /cancel first.", _C.DIM, enabled=self._color))
            return
        # Note: _approve_all_session is NOT reset here - it persists for the entire session
        self._reset_repeat_guardrails()
        self._maybe_sync_executor_tools()
        self._turn_task = str(task or "").strip() or None
        self._turn_trace = []
        self._turn_started_at = time.perf_counter()
        run_id = self._agent.start(task, allowed_tools=self._allowed_tools)
        self._sync_tool_prompt_settings_to_run(run_id)
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

        self._maybe_sync_executor_tools()
        self._last_run_id = run_id
        self._turn_started_at = time.perf_counter()
        # If paused, unpause first (ADR-0013) then continue.
        try:
            self._runtime.resume_run(run_id)
        except Exception:
            pass
        self._sync_tool_prompt_settings_to_run(run_id)
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
        self._reset_repeat_guardrails()

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
                    self._print_answer_block(title="ANSWER (partial)", answer_text=answer_text, prompt_text=prompt_text, state=state)
                return

            if state.status == self._RunStatus.CANCELLED:
                self._ui.clear_spinner()
                self._ui.scroll_to_bottom()
                self._print(_style("\nRun cancelled. State preserved.", _C.YELLOW, enabled=self._color))
                prompt_text, answer_text = self._extract_latest_turn_prompt_and_answer(state)
                self._print_answer_block(title="ANSWER (partial)", answer_text=answer_text, prompt_text=prompt_text, state=state)
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
                    self._print_answer_block(title="ANSWER (partial)", answer_text=answer_text, prompt_text=prompt_text, state=state)
                    loaded = self._messages_from_state(state)
                    if loaded:
                        self._agent.session_messages = loaded
                    return
                response = self._prompt_user(wait.prompt or "Please respond:", wait.choices)
                state = self._agent.resume(response)
                continue

            if wait.reason == self._WaitReason.SUBWORKFLOW:
                # Subworkflow waits require host orchestration:
                # - tick the deepest running child
                # - surface its waits (USER / tool approvals)
                # - bubble completion back up to the parent run(s)
                def _extract_sub_run_id(wait_state: Any) -> Optional[str]:
                    details = getattr(wait_state, "details", None)
                    if isinstance(details, dict):
                        sub_run_id = details.get("sub_run_id")
                        if isinstance(sub_run_id, str) and sub_run_id:
                            return sub_run_id
                    wait_key = getattr(wait_state, "wait_key", None)
                    if isinstance(wait_key, str) and wait_key.startswith("subworkflow:"):
                        return wait_key.split("subworkflow:", 1)[1] or None
                    return None

                def _workflow_for(run_state: Any):
                    reg = getattr(self._runtime, "workflow_registry", None)
                    getter = getattr(reg, "get", None) if reg is not None else None
                    if callable(getter):
                        wf = getter(run_state.workflow_id)
                        if wf is not None:
                            return wf
                    if getattr(self._agent.workflow, "workflow_id", None) == run_state.workflow_id:
                        return self._agent.workflow
                    raise RuntimeError(f"Workflow '{run_state.workflow_id}' not found in runtime registry")

                top_run_id = run_id

                def _bubble_completion(child_state: Any) -> Optional[str]:
                    parent_id = getattr(child_state, "parent_run_id", None)
                    if not isinstance(parent_id, str) or not parent_id:
                        return None
                    parent_state = self._runtime.get_state(parent_id)
                    parent_wait = getattr(parent_state, "waiting", None)
                    if parent_state.status != self._RunStatus.WAITING or parent_wait is None:
                        return None
                    if parent_wait.reason != self._WaitReason.SUBWORKFLOW:
                        return None
                    self._runtime.resume(
                        workflow=_workflow_for(parent_state),
                        run_id=parent_id,
                        wait_key=None,
                        payload={
                            "sub_run_id": child_state.run_id,
                            "output": getattr(child_state, "output", None),
                            "node_traces": self._runtime.get_node_traces(child_state.run_id),
                        },
                        max_steps=0,
                    )
                    return parent_id

                # Drive subruns until we either make progress or hit a non-subworkflow wait.
                for _ in range(200):
                    # Descend to the deepest sub-run referenced by SUBWORKFLOW waits.
                    current_run_id = top_run_id
                    for _ in range(25):
                        cur_state = self._runtime.get_state(current_run_id)
                        cur_wait = getattr(cur_state, "waiting", None)
                        if cur_state.status != self._RunStatus.WAITING or cur_wait is None:
                            break
                        if cur_wait.reason != self._WaitReason.SUBWORKFLOW:
                            break
                        next_id = _extract_sub_run_id(cur_wait)
                        if not next_id:
                            break
                        current_run_id = next_id

                    current_state = self._runtime.get_state(current_run_id)

                    # Tick running subruns until they block/complete.
                    if current_state.status == self._RunStatus.RUNNING:
                        current_state = self._runtime.tick(
                            workflow=_workflow_for(current_state),
                            run_id=current_run_id,
                            max_steps=100,
                        )

                    if current_state.status == self._RunStatus.RUNNING:
                        continue

                    if current_state.status == self._RunStatus.FAILED:
                        raise RuntimeError(current_state.error or "Subworkflow failed")

                    if current_state.status == self._RunStatus.WAITING:
                        cur_wait = getattr(current_state, "waiting", None)
                        if cur_wait is None:
                            break
                        if cur_wait.reason == self._WaitReason.SUBWORKFLOW:
                            continue

                        # Surface child waits to the shell.
                        if cur_wait.reason == self._WaitReason.USER:
                            self._ui.clear_spinner()
                            response = self._prompt_user(cur_wait.prompt or "Please respond:", cur_wait.choices)
                            self._runtime.resume(
                                workflow=_workflow_for(current_state),
                                run_id=current_run_id,
                                wait_key=cur_wait.wait_key,
                                payload={"response": response},
                            )
                            continue

                        if cur_wait.reason == self._WaitReason.EVENT and isinstance(cur_wait.prompt, str) and cur_wait.prompt.strip():
                            # Event waits can also carry a prompt (durable "ask+wait" over event wakeups).
                            self._ui.clear_spinner()
                            try:
                                self._on_step("ask_user", {})
                            except Exception:
                                pass
                            response = self._prompt_user(cur_wait.prompt or "Please respond:", cur_wait.choices)
                            self._runtime.resume(
                                workflow=_workflow_for(current_state),
                                run_id=current_run_id,
                                wait_key=cur_wait.wait_key,
                                payload={"response": response},
                            )
                            continue

                        details = cur_wait.details if isinstance(cur_wait.details, dict) else {}
                        tool_calls = details.get("tool_calls")
                        if isinstance(tool_calls, list):
                            self._ui.clear_spinner()
                            payload = self._approve_and_execute(tool_calls)
                            if payload is None:
                                self._print(_style("\nLeft run waiting (not resumed).", _C.DIM, enabled=self._color))
                                return
                            self._runtime.resume(
                                workflow=_workflow_for(current_state),
                                run_id=current_run_id,
                                wait_key=cur_wait.wait_key,
                                payload=payload,
                            )
                            continue

                        self._ui.clear_spinner()
                        self._print(
                            _style("\nWaiting:", _C.YELLOW, enabled=self._color)
                            + f" {cur_wait.reason.value} ({cur_wait.wait_key})"
                        )
                        return

                    if current_state.status == self._RunStatus.CANCELLED:
                        self._ui.clear_spinner()
                        self._ui.scroll_to_bottom()
                        self._print(_style("\nRun cancelled. State preserved.", _C.YELLOW, enabled=self._color))
                        return

                    if current_state.status != self._RunStatus.COMPLETED:
                        break

                    # Bubble completion to the parent and keep going.
                    parent_id = _bubble_completion(current_state)
                    if not parent_id:
                        break
                    if parent_id == top_run_id:
                        break

                # After bubbling, continue the top-level loop.
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

            # Event waits can also act as durable human prompts (useful for workflows).
            if isinstance(wait.prompt, str) and wait.prompt.strip() and isinstance(wait.wait_key, str) and wait.wait_key:
                self._ui.clear_spinner()
                try:
                    self._on_step("ask_user", {})
                except Exception:
                    pass
                response = self._prompt_user(wait.prompt or "Please respond:", wait.choices)
                state = self._runtime.resume(
                    workflow=self._agent.workflow,
                    run_id=run_id,
                    wait_key=wait.wait_key,
                    payload={"response": response},
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
        mcp_clients: Dict[str, Any] = {}

        try:
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
                        choice = self._simple_prompt(
                            f"Approve {name}? [y]es/[n]o/[a]ll/[e]dit/[q]uit: "
                        ).lower()
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

                # Guardrail: require write_file.content to be present.
                #
                # Without this, models often omit `content` (it's optional in older tool
                # schemas), which creates a 0-byte file and can then get “stuck” repeating
                # the same call (triggering the duplicate-call cache).
                if name == "write_file":
                    if "content" not in args or args.get("content") is None:
                        results.append(
                            {
                                "call_id": call_id,
                                "name": name,
                                "success": False,
                                "output": None,
                                "error": (
                                    "Invalid tool call: write_file requires a `content` string. "
                                    "Provide it explicitly (content may be an empty string)."
                                ),
                            }
                        )
                        self._print(
                            _style(
                                "Blocked write_file without content (repeat guardrail).", _C.YELLOW, enabled=self._color
                            )
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
                            cached_output = cached.get("output")
                            cached["output"] = f"[cached duplicate execute_command]\n{cached_output}"
                            results.append(cached)
                            self._print(
                                _style("Reused cached execute_command result (duplicate).", _C.DIM, enabled=self._color)
                            )
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
                            self._print(
                                _style(f"Reused cached {name} result (duplicate).", _C.DIM, enabled=self._color)
                            )
                            continue

                # MCP tool execution (remote): mcp::<server_id>::<tool_name>.
                try:
                    from abstractcore.mcp import parse_namespaced_tool_name

                    is_mcp = parse_namespaced_tool_name(name) is not None
                except Exception:
                    is_mcp = False

                if is_mcp:
                    try:
                        results.append(
                            self._execute_mcp_tool_call(
                                name=name,
                                arguments=args,
                                call_id=call_id,
                                cache=mcp_clients,
                            )
                        )
                    except Exception as e:
                        results.append(
                            {
                                "call_id": call_id,
                                "name": name,
                                "success": False,
                                "output": None,
                                "error": str(e),
                            }
                        )
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
                        self._last_mutating_tool_call_key = (
                            name,
                            hashlib.sha256(material.encode("utf-8")).hexdigest(),
                        )
                        first = out_results[0]
                        if isinstance(first, dict):
                            self._last_mutating_tool_call_result = dict(first)
                    except Exception:
                        pass
        finally:
            for c in mcp_clients.values():
                try:
                    close = getattr(c, "close", None)
                    if callable(close):
                        close()
                except Exception:
                    pass

        return {"mode": "executed", "results": results}


 
