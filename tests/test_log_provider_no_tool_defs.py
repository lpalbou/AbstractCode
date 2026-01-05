from __future__ import annotations

import json
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
    shell._provider = "ollama"
    shell._model = "qwen3"
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


def test_log_provider_copy_json_no_tool_defs_simplifies_tools_field() -> None:
    tool_defs = [
        {"type": "function", "function": {"name": "list_files", "description": "x", "parameters": {"type": "object"}}},
        {"type": "function", "function": {"name": "edit_file", "description": "y", "parameters": {"type": "object"}}},
    ]
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
                        "url": "http://localhost:1234/v1/chat/completions",
                        "payload": {"model": "m", "messages": [{"role": "user", "content": "hi"}], "tools": tool_defs},
                    }
                },
                "raw_response": {"choices": [{"message": {"content": "ok"}}]},
            },
        }
    ]
    shell = _minimal_shell_for_log_provider(records=records)

    shell._handle_log_provider("copy --json-only --no-tool-defs --last")

    copied = shell._captured_copy.get("text")  # type: ignore[attr-defined]
    assert isinstance(copied, str) and copied.strip()
    doc = json.loads(copied)
    tools = doc["calls"][0]["request_sent"]["payload"]["tools"]
    assert tools == ["list_files", "edit_file"]
    assert all(isinstance(t, str) for t in tools)


def test_log_provider_copy_json_keeps_tool_defs_by_default() -> None:
    tool_defs = [
        {"type": "function", "function": {"name": "list_files", "description": "x", "parameters": {"type": "object"}}},
    ]
    records = [
        {
            "effect": {"type": "llm_call"},
            "status": "completed",
            "ended_at": "2026-01-05T00:00:01Z",
            "node_id": "n1",
            "result": {
                "metadata": {
                    "_provider_request": {
                        "url": "http://localhost:1234/v1/chat/completions",
                        "payload": {"model": "m", "messages": [{"role": "user", "content": "hi"}], "tools": tool_defs},
                    }
                },
                "raw_response": {"choices": [{"message": {"content": "ok"}}]},
            },
        }
    ]
    shell = _minimal_shell_for_log_provider(records=records)

    shell._handle_log_provider("copy --json-only --last")

    copied = shell._captured_copy.get("text")  # type: ignore[attr-defined]
    doc = json.loads(copied)
    tools = doc["calls"][0]["request_sent"]["payload"]["tools"]
    assert isinstance(tools, list)
    assert isinstance(tools[0], dict)
    assert tools[0]["function"]["name"] == "list_files"



