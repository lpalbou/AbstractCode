from __future__ import annotations

import threading
from datetime import datetime, timezone
from typing import Any, List, Optional

from abstractcode.react_shell import ReactShell
from abstractruntime.core.models import RunState, RunStatus, WaitReason, WaitState


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


class _FakeUI:
    def __init__(self) -> None:
        self.copy_payloads: dict[str, str] = {}

    def append_output(self, text: str) -> None:
        _ = text

    def clear_spinner(self) -> None:
        return

    def scroll_to_bottom(self) -> None:
        return

    def register_copy_payload(self, copy_id: str, payload: str) -> None:
        self.copy_payloads[str(copy_id)] = str(payload)


class _FakeAgent:
    def __init__(self, states: List[RunState]) -> None:
        self._states = list(states)
        self.session_messages: list[dict[str, Any]] = []
        self.run_id: Optional[str] = None
        self.workflow: Any = object()

    def step(self) -> RunState:
        if not self._states:
            raise RuntimeError("No more states")
        return self._states.pop(0)

    def resume(self, response: str) -> RunState:  # pragma: no cover
        raise AssertionError(f"resume() should not be called (got {response!r})")


class _FakeRuntime:
    def __init__(self) -> None:
        self.resumed: list[dict[str, Any]] = []

    def resume(self, **kwargs: Any) -> Any:  # pragma: no cover
        self.resumed.append(dict(kwargs))
        raise AssertionError("runtime.resume() must not be called when approval is cancelled")


def test_run_loop_treats_tool_approval_cancel_mode_as_continue_until_cancelled_state() -> None:
    waiting = RunState(
        run_id="rid",
        workflow_id="wf",
        status=RunStatus.WAITING,
        current_node="n",
        vars={"context": {"messages": [{"role": "user", "content": "x"}, {"role": "assistant", "content": "partial"}]}},
        waiting=WaitState(
            reason=WaitReason.EVENT,
            wait_key="tool:rid",
            until=None,
            resume_to_node="n",
            result_key=None,
            prompt=None,
            choices=None,
            allow_free_text=False,
            details={
                "tool_calls": [
                    {"name": "read_file", "arguments": {"file_path": "x"}, "call_id": "c1"},
                ]
            },
        ),
        output=None,
        error=None,
        created_at=_now_iso(),
        updated_at=_now_iso(),
        actor_id=None,
        session_id=None,
        parent_run_id=None,
    )
    cancelled = RunState(
        run_id="rid",
        workflow_id="wf",
        status=RunStatus.CANCELLED,
        current_node="n",
        vars={"context": {"messages": [{"role": "user", "content": "x"}, {"role": "assistant", "content": "partial"}]}},
        waiting=None,
        output=None,
        error="Cancelled",
        created_at=_now_iso(),
        updated_at=_now_iso(),
        actor_id=None,
        session_id=None,
        parent_run_id=None,
    )

    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()
    shell._run_thread = None
    shell._run_thread_lock = threading.Lock()
    shell._RunStatus = RunStatus
    shell._WaitReason = WaitReason
    shell._runtime = _FakeRuntime()
    shell._agent = _FakeAgent([waiting, cancelled])
    shell._turn_task = None
    shell._turn_trace = []

    def _cancel_mode(_tool_calls: Any) -> dict[str, Any]:
        return {"mode": "cancelled"}

    shell._approve_and_execute = _cancel_mode  # type: ignore[assignment]

    ReactShell._run_loop(shell, "rid")

    assert not shell._runtime.resumed
    assert any("Run cancelled" in line for line in shell._output_lines)
    assert shell._output_lines[-1] == ""
    assert shell._output_lines[-2].startswith("[[COPY:assistant_")

