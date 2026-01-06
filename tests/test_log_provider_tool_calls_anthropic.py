from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from abstractcode.react_shell import ReactShell


class _FakeUI:
    def __init__(self) -> None:
        self.lines: List[str] = []

    def append_output(self, text: str) -> None:
        self.lines.append(str(text))


class _FakeLedgerStore:
    def __init__(self, records: List[Dict[str, Any]]) -> None:
        self._records = list(records)

    def list(self, run_id: str) -> List[Dict[str, Any]]:
        _ = run_id
        return list(self._records)


@dataclass
class _FakeState:
    run_id: str
    session_id: Optional[str]
    vars: Dict[str, Any]


class _FakeRuntime:
    def __init__(self, records: List[Dict[str, Any]]) -> None:
        self.ledger_store = _FakeLedgerStore(records)
        self.run_store = object()


def _minimal_shell_for_log_provider(*, records: List[Dict[str, Any]]) -> ReactShell:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._provider = "anthropic"
    shell._model = "claude"
    shell._output_lines = []
    shell._ui = _FakeUI()

    captured: Dict[str, str] = {}

    def _copy(text: str) -> bool:
        captured["text"] = str(text)
        return True

    shell._copy_to_clipboard = _copy  # type: ignore[assignment]
    shell._runtime = _FakeRuntime(records)  # type: ignore[assignment]

    state = _FakeState(run_id="r1", session_id=None, vars={})
    shell._safe_get_state = lambda: state  # type: ignore[assignment]
    shell._captured_copy = captured  # type: ignore[attr-defined]
    return shell


def test_log_provider_detects_anthropic_tool_use_blocks_in_pretty_output() -> None:
    records = [
        {
            "effect": {"type": "llm_call"},
            "status": "completed",
            "started_at": "2026-01-05T00:00:00Z",
            "ended_at": "2026-01-05T00:00:01Z",
            "node_id": "n1",
            "result": {
                "metadata": {
                    "_provider_request": {
                        "url": "https://api.anthropic.com/v1/messages",
                        "payload": {"model": "claude", "messages": [{"role": "user", "content": "hi"}]},
                    }
                },
                "raw_response": {
                    "id": "msg_1",
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "Let me check."},
                        {"type": "tool_use", "id": "toolu_1", "name": "list_files", "input": {"path": "."}},
                    ],
                },
            },
        }
    ]
    shell = _minimal_shell_for_log_provider(records=records)

    shell._handle_log_provider("copy --last")

    copied = shell._captured_copy.get("text")  # type: ignore[attr-defined]
    assert isinstance(copied, str) and copied.strip()
    assert "Model generated tool calls" in copied
    # Ensure the extracted tool call is visible in the summary line output.
    assert "list_files" in copied
    assert "tool_use" in copied


