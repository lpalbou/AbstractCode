from __future__ import annotations

import re
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
        # ReactShell._print appends to shell._output_lines already; no-op here.
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

    def step(self) -> RunState:
        if not self._states:
            raise RuntimeError("No more states")
        return self._states.pop(0)

    def resume(self, response: str) -> RunState:  # pragma: no cover
        raise AssertionError(f"resume() should not be called (got {response!r})")


def _minimal_shell() -> ReactShell:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()
    shell._run_thread = None
    shell._run_thread_lock = threading.Lock()
    shell._RunStatus = RunStatus
    shell._WaitReason = WaitReason
    return shell


_COPY_LINE_RE = re.compile(r"^\[\[COPY:(?P<id>[^\]]+)\]\]")


def _copy_id_from_output_line(line: str) -> str:
    m = _COPY_LINE_RE.match(str(line or ""))
    assert m is not None, f"expected a COPY marker line, got: {line!r}"
    cid = str(m.group("id") or "")
    assert cid, f"empty COPY id in line: {line!r}"
    return cid


def test_user_prompt_copy_marker_is_at_end_with_trailing_blank_line() -> None:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    text = ReactShell._format_user_prompt_block(shell, "hello", copy_id="cid", footer="2026-01-02 12:34Z")
    lines = text.split("\n")
    assert lines[-1] == ""
    assert lines[-2].startswith("[[COPY:cid]]")
    assert "2026-01-02" in lines[-2]

def test_user_prompt_copy_marker_is_separated_from_colored_frame() -> None:
    shell = ReactShell.__new__(ReactShell)
    shell._color = True
    shell._terminal_width = lambda: 60  # type: ignore[assignment]

    text = ReactShell._format_user_prompt_block(shell, "hello", copy_id="cid", footer="2026-01-02 12:34Z")
    lines = text.split("\n")

    assert lines[-1] == ""
    assert lines[-2].startswith("[[COPY:cid]]")
    assert lines[-3].startswith("\033[48;5;238m")


def test_answer_copy_marker_is_last_in_done_step() -> None:
    shell = _minimal_shell()
    shell._turn_task = None
    shell._turn_trace = []

    ReactShell._on_step(shell, "done", {"answer": "ok"})

    # Expect the last visible marker to be the copy marker, followed by a blank spacer line.
    assert shell._output_lines[-1] == ""
    assert isinstance(shell._output_lines[-2], str) and shell._output_lines[-2].startswith("[[COPY:assistant_")
    assert not any(line.startswith("[[COPY:") for line in shell._output_lines[:-2])


def test_run_loop_exits_cleanly_on_cancelled() -> None:
    shell = _minimal_shell()

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
    shell._agent = _FakeAgent([cancelled])

    ReactShell._run_loop(shell, "rid")

    assert any("Run cancelled" in line for line in shell._output_lines)
    assert shell._output_lines[-1] == ""
    assert shell._output_lines[-2].startswith("[[COPY:assistant_")
    copy_id = _copy_id_from_output_line(shell._output_lines[-2])
    assert copy_id in shell._ui.copy_payloads
    assert "partial" in shell._ui.copy_payloads[copy_id]


def test_run_loop_treats_pause_wait_as_terminal_without_prompt() -> None:
    shell = _minimal_shell()

    paused = RunState(
        run_id="rid",
        workflow_id="wf",
        status=RunStatus.WAITING,
        current_node="n",
        vars={"context": {"messages": [{"role": "user", "content": "x"}, {"role": "assistant", "content": "partial"}]}},
        waiting=WaitState(
            reason=WaitReason.USER,
            wait_key="pause:rid",
            until=None,
            resume_to_node="n",
            result_key=None,
            prompt="Paused",
            choices=None,
            allow_free_text=False,
            details={"kind": "pause"},
        ),
        output=None,
        error=None,
        created_at=_now_iso(),
        updated_at=_now_iso(),
        actor_id=None,
        session_id=None,
        parent_run_id=None,
    )
    shell._agent = _FakeAgent([paused])

    def _should_not_prompt(prompt: str, choices: Any) -> str:  # pragma: no cover
        raise AssertionError("pause wait must not trigger a user prompt")

    shell._prompt_user = _should_not_prompt  # type: ignore[assignment]

    ReactShell._run_loop(shell, "rid")

    assert any("Paused. Type '/resume' to continue." in line for line in shell._output_lines)
    assert shell._output_lines[-1] == ""
    assert shell._output_lines[-2].startswith("[[COPY:assistant_")
    copy_id = _copy_id_from_output_line(shell._output_lines[-2])
    assert copy_id in shell._ui.copy_payloads
    assert "partial" in shell._ui.copy_payloads[copy_id]


def test_run_loop_emits_copy_button_on_max_iterations_completion() -> None:
    shell = _minimal_shell()

    completed = RunState(
        run_id="rid",
        workflow_id="wf",
        status=RunStatus.COMPLETED,
        current_node="max_iterations",
        vars={"context": {"messages": [{"role": "user", "content": "x"}, {"role": "assistant", "content": "partial"}]}},
        waiting=None,
        output={
            "answer": "partial",
            "iterations": 25,
            "messages": [{"role": "user", "content": "x"}, {"role": "assistant", "content": "partial"}],
        },
        error=None,
        created_at=_now_iso(),
        updated_at=_now_iso(),
        actor_id=None,
        session_id=None,
        parent_run_id=None,
    )
    shell._agent = _FakeAgent([completed])

    ReactShell._run_loop(shell, "rid")

    assert any("Max iterations reached" in line for line in shell._output_lines)
    assert shell._output_lines[-1] == ""
    assert shell._output_lines[-2].startswith("[[COPY:assistant_")
    copy_id = _copy_id_from_output_line(shell._output_lines[-2])
    assert copy_id in shell._ui.copy_payloads
    assert "partial" in shell._ui.copy_payloads[copy_id]


def test_pause_and_cancel_do_not_block_while_run_thread_is_active() -> None:
    shell = _minimal_shell()

    started = threading.Event()
    finish = threading.Event()

    def _blocking_run_loop(run_id: str) -> None:
        _ = run_id
        started.set()
        finish.wait(timeout=2)

    shell._run_loop = _blocking_run_loop  # type: ignore[assignment]

    class _FakeRuntime:
        def __init__(self) -> None:
            self.paused: list[str] = []
            self.cancelled: list[str] = []

        def pause_run(self, run_id: str, *, reason: str | None = None) -> None:
            _ = reason
            self.paused.append(run_id)

        def cancel_run(self, run_id: str, *, reason: str | None = None) -> None:
            _ = reason
            self.cancelled.append(run_id)

    shell._runtime = _FakeRuntime()

    class _RunIdAgent:
        run_id = "rid"

    shell._agent = _RunIdAgent()
    shell._last_run_id = "rid"

    ReactShell._run_in_background(shell, "rid")
    assert started.wait(timeout=1)

    ReactShell._pause(shell)
    ReactShell._cancel(shell)

    assert shell._runtime.paused == ["rid"]
    assert shell._runtime.cancelled == ["rid"]

    t = shell._run_thread
    assert t is not None
    finish.set()
    t.join(timeout=1)
