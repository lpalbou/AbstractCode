from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from abstractcode.react_shell import ReactShell
from abstractruntime.core.models import RunState, RunStatus


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


class _FakeRunStore:
    def __init__(self) -> None:
        self.saved: list[RunState] = []

    def save(self, state: RunState) -> None:
        self.saved.append(state)


class _FakePromptCacheClient:
    def __init__(self, payload: dict[str, Any]) -> None:
        self.payload = dict(payload)
        self.calls: list[dict[str, str]] = []

    def get_prompt_cache_capabilities(self, **kwargs: Any) -> dict[str, Any]:
        self.calls.append({k: str(v) for k, v in kwargs.items() if isinstance(v, str)})
        return dict(self.payload)


class _FakeRuntime:
    def __init__(self, state: RunState, client: _FakePromptCacheClient) -> None:
        self._state = state
        self._abstractcore_llm_client = client
        self.run_store = _FakeRunStore()

    def get_state(self, run_id: str) -> RunState:
        assert run_id == self._state.run_id
        return self._state


def _state_with_runtime_ns(*, session_id: str = "sess-1", prompt_cache: Any = None) -> RunState:
    runtime_ns: dict[str, Any] = {"inbox": []}
    if prompt_cache is not None:
        runtime_ns["prompt_cache"] = prompt_cache
    return RunState(
        run_id="rid",
        workflow_id="wf",
        status=RunStatus.RUNNING,
        current_node="reason",
        vars={
            "context": {"task": "t", "messages": []},
            "scratchpad": {"iteration": 0, "max_iterations": 2},
            "_runtime": runtime_ns,
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
        session_id=session_id,
        parent_run_id=None,
    )


def _minimal_shell(*, state: RunState, client_payload: dict[str, Any]) -> ReactShell:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._provider = "stub"
    shell._model = "stub-model"
    shell._prompt_cache_mode = "auto"
    shell._tool_prompt_examples = True
    shell._check_plan = False
    shell._system_prompt_override = None
    shell._workspace_root = Path("/tmp")
    shell._workspace_root_source = "cwd"
    shell._workspace_mounts = {}
    shell._workspace_blocked_paths = []

    client = _FakePromptCacheClient(client_payload)
    shell._runtime = _FakeRuntime(state, client)
    shell._safe_get_state = lambda: state  # type: ignore[assignment]
    shell._print = lambda text: shell._output_lines.append(str(text))  # type: ignore[assignment]
    return shell


def test_sync_tool_prompt_settings_uses_runtime_prompt_cache_capabilities() -> None:
    state = _state_with_runtime_ns()
    shell = _minimal_shell(
        state=state,
        client_payload={
            "supported": True,
            "capabilities": {"supported": True, "mode": "keyed"},
        },
    )

    shell._sync_tool_prompt_settings_to_run("rid")

    runtime_ns = state.vars.get("_runtime")
    assert isinstance(runtime_ns, dict)
    prompt_cache = runtime_ns.get("prompt_cache")
    assert isinstance(prompt_cache, dict)
    assert prompt_cache["enabled"] is True
    assert str(prompt_cache["key"]).startswith("acode:")
    assert shell._runtime.run_store.saved


def test_handle_cache_reports_runtime_prompt_cache_capabilities() -> None:
    state = _state_with_runtime_ns(prompt_cache={"key": "acode:abc123"})
    shell = _minimal_shell(
        state=state,
        client_payload={
            "supported": True,
            "capabilities": {
                "supported": True,
                "mode": "local_control_plane",
                "supports_update": True,
                "supports_fork": True,
                "supports_prepare_modules": True,
            },
        },
    )

    shell._handle_cache("")

    assert shell._output_lines
    last = shell._output_lines[-1]
    assert "provider_mode=local_control_plane" in last
    assert "ops=update,fork,modules" in last
    assert "key=acode:abc123" in last
