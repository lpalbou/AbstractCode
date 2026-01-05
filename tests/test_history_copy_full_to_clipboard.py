from __future__ import annotations

import json
from datetime import datetime, timezone
from typing import Any, Dict, Optional

from abstractcode.react_shell import ReactShell
from abstractruntime.core.models import RunState, RunStatus


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


class _FakeUI:
    def append_output(self, text: str) -> None:
        _ = text


class _FakeAgent:
    def __init__(self, *, state: RunState) -> None:
        self._state = state
        self.session_messages: list[dict[str, Any]] = []

    def get_state(self) -> Optional[RunState]:
        return self._state


def _minimal_shell(*, state: RunState) -> ReactShell:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()
    shell._agent = _FakeAgent(state=state)
    shell._RunStatus = RunStatus
    shell._agent_kind = "react"
    shell._provider = "test"
    shell._model = "test"
    shell._last_run_id = None
    return shell


def test_history_copy_copies_full_transcript(monkeypatch) -> None:
    state = RunState(
        run_id="rid",
        workflow_id="wf",
        status=RunStatus.RUNNING,
        current_node="n",
        vars={
            "context": {
                "messages": [
                    {"role": "system", "content": "sys"},
                    {"role": "user", "content": "u1"},
                    {"role": "assistant", "content": "a1"},
                    {"role": "tool", "content": "[read_file]: ok", "metadata": {"name": "read_file"}},
                    {"role": "assistant", "content": "a1b"},
                    {"role": "user", "content": "u2"},
                    {"role": "assistant", "content": "a2"},
                ]
            }
        },
        waiting=None,
        output=None,
        error=None,
        created_at=_now_iso(),
        updated_at=_now_iso(),
        actor_id=None,
        session_id=None,
        parent_run_id=None,
    )
    shell = _minimal_shell(state=state)

    captured: Dict[str, str] = {}

    def _capture(text: str) -> bool:
        captured["text"] = str(text)
        return True

    monkeypatch.setattr(shell, "_copy_to_clipboard", _capture)

    ReactShell._dispatch_command(shell, "history copy")

    assert "Copied." in "\n".join(shell._output_lines)
    transcript = captured.get("text", "")
    assert "# Turn 1" in transcript
    assert "system:\nsys" in transcript
    assert "user:\nu1" in transcript
    assert "assistant:\na1" in transcript
    assert "tool[read_file]:\n[read_file]: ok" in transcript
    assert "# Turn 2" in transcript
    assert "user:\nu2" in transcript
    assert "assistant:\na2" in transcript


def test_help_mentions_history_copy() -> None:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()

    ReactShell._show_help(shell)

    combined = "\n".join(shell._output_lines)
    assert "/history copy" in combined
    assert "/log runtime" in combined
    assert "/log provider" in combined
    assert "/context" not in combined
    assert "/llm" not in combined


def test_help_mentions_mcp_and_executor_usage() -> None:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()

    ReactShell._show_help(shell)

    combined = "\n".join(shell._output_lines)
    assert "/mcp add <id> <url>" in combined
    assert "/mcp add <id> stdio" in combined
    assert "/mcp sync" in combined
    assert "mcp::<id>::<tool_name>" in combined
    assert "/executor use <server_id>" in combined
    assert "ABSTRACTCODE_BASE_URL" in combined
