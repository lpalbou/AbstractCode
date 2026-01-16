def test_react_shell_wires_artifact_store_into_runtime_factory(monkeypatch):
    captured = {"artifact_store": None}

    class DummyRuntime:
        def __init__(self):
            self._artifact_store = None

        def set_artifact_store(self, store):
            self._artifact_store = store

        def set_workflow_registry(self, _registry):
            return None

    def fake_create_local_runtime(*, artifact_store=None, **_kwargs):
        captured["artifact_store"] = artifact_store
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

    assert captured["artifact_store"] is shell._artifact_store

