from __future__ import annotations

from typing import Any, Dict, List, Optional

from abstractcode.react_shell import ReactShell


class _FakeUI:
    def append_output(self, text: str) -> None:
        _ = text


class _FailingToolRunner:
    def __init__(self) -> None:
        self.calls: List[Dict[str, Any]] = []

    def execute(self, *, tool_calls: List[Dict[str, Any]]) -> Dict[str, Any]:
        self.calls.extend(list(tool_calls))
        raise AssertionError("tool_runner.execute() should not be called for blocked write_file calls")


def _minimal_shell_for_tool_exec() -> ReactShell:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()

    shell._auto_approve = True
    shell._approve_all_session = False

    shell._mcp_servers = {}
    shell._mcp_client_factory = None
    shell._tool_runner = _FailingToolRunner()

    shell._last_execute_command = None
    shell._last_execute_command_result = None
    shell._last_mutating_tool_call_key = None
    shell._last_mutating_tool_call_result = None
    return shell


def test_write_file_without_content_is_blocked() -> None:
    shell = _minimal_shell_for_tool_exec()

    payload = ReactShell._approve_and_execute(  # type: ignore[arg-type]
        shell,
        [{"name": "write_file", "arguments": {"file_path": "x.txt"}, "call_id": "c1"}],
    )

    assert payload is not None
    assert payload["mode"] == "executed"
    assert payload["results"] and payload["results"][0]["name"] == "write_file"
    assert payload["results"][0]["success"] is False
    assert "requires a `content` string" in str(payload["results"][0]["error"])


def test_cancel_resets_repeat_guardrail_cache() -> None:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()

    class _FakeRuntime:
        def __init__(self) -> None:
            self.cancelled: list[str] = []

        def cancel_run(self, run_id: str, *, reason: Optional[str] = None) -> None:
            _ = reason
            self.cancelled.append(run_id)

    shell._runtime = _FakeRuntime()

    class _RunIdAgent:
        run_id = "rid"

    shell._agent = _RunIdAgent()
    shell._last_run_id = "rid"

    # Seed the caches to ensure /cancel clears them.
    shell._last_execute_command = "echo hi"
    shell._last_execute_command_result = {"success": True}
    shell._last_mutating_tool_call_key = ("write_file", "deadbeef")
    shell._last_mutating_tool_call_result = {"success": True}

    ReactShell._cancel(shell)

    assert shell._runtime.cancelled == ["rid"]
    assert shell._last_execute_command is None
    assert shell._last_execute_command_result is None
    assert shell._last_mutating_tool_call_key is None
    assert shell._last_mutating_tool_call_result is None


