from __future__ import annotations

from pathlib import Path

import pytest


pytestmark = pytest.mark.basic


def test_react_shell_default_tools_include_skim_files(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    class DummyRuntime:
        def set_artifact_store(self, _store):
            return None

        def set_workflow_registry(self, _registry):
            return None

    def fake_create_local_runtime(*, artifact_store=None, **_kwargs):
        assert artifact_store is not None
        return DummyRuntime()

    class DummyLLMClient:
        def __init__(self, *, provider, model, llm_kwargs=None):
            self._provider = provider
            self._model = model
            self._llm_kwargs = llm_kwargs

        def get_model_capabilities(self):
            return {"max_tokens": 32768}

    class DummyAgent:
        def __init__(self, *, runtime, tools, on_step=None, **_kwargs):
            self.runtime = runtime
            self.tools = tools
            self.on_step = on_step
            self.run_id = None
            self.session_messages = []
            self.workflow = type("_Wf", (), {"workflow_id": "react_agent"})()

    import abstractruntime.integrations.abstractcore as arc
    import abstractagent.agents.react as react_mod

    monkeypatch.setattr(arc, "create_local_runtime", fake_create_local_runtime)
    monkeypatch.setattr(arc, "LocalAbstractCoreLLMClient", DummyLLMClient)
    monkeypatch.setattr(react_mod, "ReactAgent", DummyAgent)

    from abstractcode.react_shell import ReactShell

    shell = ReactShell(
        agent="react",
        provider="ollama",
        model="qwen3:1.7b-q4_K_M",
        state_file=None,
        auto_approve=True,
        max_iterations=1,
        max_tokens=1024,
        color=False,
    )

    names = set()
    for t in shell._tools:
        tool_def = getattr(t, "_tool_definition", None)
        if tool_def is not None and isinstance(getattr(tool_def, "name", None), str):
            names.add(tool_def.name)
        else:
            names.add(str(getattr(t, "__name__", "") or ""))

    assert "skim_files" in names

