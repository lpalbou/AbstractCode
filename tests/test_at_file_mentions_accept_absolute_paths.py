from __future__ import annotations

from pathlib import Path


def test_at_file_mentions_accept_absolute_paths_under_mount(monkeypatch, tmp_path: Path) -> None:
    """Ensure `@/abs/path` works when the path is under a whitelisted mount root."""
    ws = tmp_path / "ws"
    ws.mkdir()
    desk = tmp_path / "Desktop"
    desk.mkdir()
    (desk / "toto.png").write_bytes(b"png")

    monkeypatch.setenv("ABSTRACTCODE_WORKSPACE_DIR", str(ws))
    monkeypatch.setenv("ABSTRACTCODE_WORKSPACE_MOUNTS", f"Desktop={desk}")

    class DummyRuntime:
        def set_artifact_store(self, _store):
            return None

        def set_workflow_registry(self, _registry):
            return None

    def fake_create_local_runtime(*, artifact_store=None, **_kwargs):
        # ReactShell wires the artifact_store into the runtime; we don't need more here.
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

        def _ensure_session_id(self) -> str:
            return "test-session"

    import abstractruntime.integrations.abstractcore as arc
    import abstractagent.agents.react as react_mod

    monkeypatch.setattr(arc, "create_local_runtime", fake_create_local_runtime)
    monkeypatch.setattr(arc, "LocalAbstractCoreLLMClient", DummyLLMClient)
    monkeypatch.setattr(react_mod, "ReactAgent", DummyAgent)

    from abstractcode.fullscreen_ui import SubmittedInput
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

    abs_path = str((desk / "toto.png").resolve())
    shell._handle_input(SubmittedInput(text=f"@{abs_path}", attachments=[]))

    # The absolute path should be converted into the mount-relative virtual path.
    assert "Desktop/toto.png" in shell._attachment_ref_cache
    assert "Desktop/toto.png" in (shell._ui.get_composer_state().get("attachments") or [])


def test_at_file_mentions_accept_absolute_paths_outside_workspace(monkeypatch, tmp_path: Path) -> None:
    """Ensure `@/abs/path` works for any local file (even outside workspace/mounts)."""
    ws = tmp_path / "ws"
    ws.mkdir()
    outside = tmp_path / "outside.txt"
    outside.write_text("hello")

    monkeypatch.setenv("ABSTRACTCODE_WORKSPACE_DIR", str(ws))
    monkeypatch.delenv("ABSTRACTCODE_WORKSPACE_MOUNTS", raising=False)

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

        def _ensure_session_id(self) -> str:
            return "test-session"

    import abstractruntime.integrations.abstractcore as arc
    import abstractagent.agents.react as react_mod

    monkeypatch.setattr(arc, "create_local_runtime", fake_create_local_runtime)
    monkeypatch.setattr(arc, "LocalAbstractCoreLLMClient", DummyLLMClient)
    monkeypatch.setattr(react_mod, "ReactAgent", DummyAgent)

    from abstractcode.fullscreen_ui import SubmittedInput
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

    abs_path = str(outside.resolve())
    shell._handle_input(SubmittedInput(text=f"@{abs_path}", attachments=[]))

    assert abs_path in shell._attachment_ref_cache
    assert abs_path in (shell._ui.get_composer_state().get("attachments") or [])
