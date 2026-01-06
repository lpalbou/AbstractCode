from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional

from abstractcode.react_shell import ReactShell
from abstractcore.tools import ToolDefinition
from abstractruntime.core.models import RunState, RunStatus


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


class _FakeUI:
    def append_output(self, text: str) -> None:
        _ = text


class _FakeRunStore:
    def __init__(self) -> None:
        self.saved: list[RunState] = []

    def save(self, state: RunState) -> None:
        self.saved.append(state)


class _FakeRuntime:
    def __init__(self) -> None:
        self.run_store = _FakeRunStore()

    def get_state(self, run_id: str) -> RunState:  # pragma: no cover
        raise KeyError(run_id)


class _FakeAgent:
    def __init__(self, *, state: RunState, tools: list[ToolDefinition]) -> None:
        self._state = state
        self.logic = type("_Logic", (), {"tools": tools})()

    def get_state(self) -> RunState:
        return self._state


def _minimal_shell(*, state: RunState, tools: list[ToolDefinition], config_file: Path) -> ReactShell:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()
    shell._runtime = _FakeRuntime()
    shell._agent = _FakeAgent(state=state, tools=tools)

    shell._agent_kind = "react"
    shell._allowed_tools = None
    shell._tool_prompt_examples = True
    shell._config_file = config_file
    shell._max_tokens = None
    shell._auto_approve = False
    shell._plan_mode = False
    shell._review_mode = False
    shell._review_max_rounds = 1
    shell._tool_specs = {}
    shell._status_cache_key = None
    shell._status_cache_text = ""
    shell._last_run_id = None
    return shell


def _run_state() -> RunState:
    return RunState(
        run_id="rid",
        workflow_id="wf",
        status=RunStatus.RUNNING,
        current_node="reason",
        vars={
            "context": {"task": "t", "messages": []},
            "scratchpad": {"iteration": 0, "max_iterations": 2},
            "_runtime": {"inbox": []},
            "_temp": {},
            "_limits": {
                "max_iterations": 2,
                "current_iteration": 0,
                "max_history_messages": -1,
                "max_tokens": 1024,
            },
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


def test_tools_examples_toggle_persists_and_updates_active_run(tmp_path: Path) -> None:
    state = _run_state()
    tools = [ToolDefinition(name="tool_a", description="A", parameters={})]
    config_file = tmp_path / "cfg.json"
    shell = _minimal_shell(state=state, tools=tools, config_file=config_file)

    shell._handle_tools("examples off")

    assert shell._tool_prompt_examples is False
    runtime_ns = state.vars.get("_runtime")
    assert isinstance(runtime_ns, dict)
    assert runtime_ns.get("tool_prompt_examples") is False
    assert shell._runtime.run_store.saved

    saved_config = json.loads(config_file.read_text())
    assert saved_config.get("tool_prompt_examples") is False


def test_tools_disable_all_updates_active_run_allowlist(tmp_path: Path) -> None:
    state = _run_state()
    tools = [
        ToolDefinition(name="tool_a", description="A", parameters={}),
        ToolDefinition(name="tool_b", description="B", parameters={}),
    ]
    config_file = tmp_path / "cfg.json"
    shell = _minimal_shell(state=state, tools=tools, config_file=config_file)

    shell._handle_tools("disable all")

    runtime_ns = state.vars.get("_runtime")
    assert isinstance(runtime_ns, dict)
    assert runtime_ns.get("allowed_tools") == []
    assert shell._allowed_tools == []
