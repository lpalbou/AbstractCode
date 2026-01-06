from __future__ import annotations

import re
import threading

from abstractcode.react_shell import ReactShell
from abstractruntime.core.models import RunStatus, WaitReason


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


def _minimal_shell() -> ReactShell:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()
    shell._run_thread = None
    shell._run_thread_lock = threading.Lock()
    shell._RunStatus = RunStatus
    shell._WaitReason = WaitReason
    shell._turn_task = None
    shell._turn_trace = []
    return shell


_COPY_LINE_RE = re.compile(r"^\[\[COPY:(?P<id>[^\]]+)\]\]")


def _copy_id_from_output_line(line: str) -> str:
    m = _COPY_LINE_RE.match(str(line or ""))
    assert m is not None, f"expected a COPY marker line, got: {line!r}"
    cid = str(m.group("id") or "")
    assert cid, f"empty COPY id in line: {line!r}"
    return cid


def test_answer_markdown_unescapes_literal_newlines_for_rendering_only() -> None:
    shell = _minimal_shell()

    ReactShell._on_step(shell, "done", {"answer": "Line1\\nLine2"})

    # The rendered answer should contain a real newline (not the two-character sequence "\n").
    rendered = shell._output_lines[2]
    assert rendered == "Line1\nLine2"
    assert "\\n" not in rendered

    # Copy payload remains lossless / raw (it should still contain the literal "\n").
    copy_id = _copy_id_from_output_line(shell._output_lines[-2])
    payload = shell._ui.copy_payloads[copy_id]
    assert "Line1\\nLine2" in payload



