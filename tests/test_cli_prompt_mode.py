def test_cli_prompt_runs_one_shot_and_exits(monkeypatch, capsys):
    from abstractruntime.core.models import RunStatus

    class _State:
        def __init__(self, status, *, output=None, waiting=None, error=None):
            self.status = status
            self.output = output
            self.waiting = waiting
            self.error = error

    class DummyAgent:
        def __init__(self):
            self.workflow = object()
            self.started = None

        def start(self, task, *, allowed_tools=None, attachments=None):
            self.started = {"task": task, "allowed_tools": allowed_tools, "attachments": attachments}
            return "run_123"

        def step(self):
            return _State(RunStatus.COMPLETED, output={"answer": "done"})

        def resume(self, response):
            raise AssertionError("resume should not be called in this test")

        def save_state(self, filepath):
            raise AssertionError("save_state should not be called when --no-state is set")

    class DummyRuntime:
        def resume(self, *, workflow, run_id, wait_key, payload):
            raise AssertionError("resume should not be called in this test")

    class DummyShell:
        def __init__(self, **_kwargs):
            self._agent = DummyAgent()
            self._runtime = DummyRuntime()
            self._allowed_tools = None
            self._tool_runner = object()
            self._auto_approve = False
            self._state_file = None

        def _ingest_workspace_attachments(self, rel_paths):
            assert rel_paths == ["foo.txt"]
            return [
                {
                    "$artifact": "a1",
                    "filename": "foo.txt",
                    "content_type": "text/plain",
                    "source_path": "foo.txt",
                }
            ]

        def _try_load_state(self):
            raise AssertionError("_try_load_state should not be called when --no-state is set")

        def run(self):
            raise AssertionError("Interactive shell should not be started in --prompt mode")

    from abstractcode import cli as cli_mod

    import abstractcode.react_shell as react_shell_mod

    monkeypatch.setattr(react_shell_mod, "ReactShell", DummyShell)

    rc = cli_mod.main(["--no-state", "--prompt", "Hello @foo.txt"])
    assert rc == 0

    out = capsys.readouterr()
    assert out.out.strip() == "done"
    assert "Attachments: foo.txt" in out.err
    assert out.err.strip().endswith("Attachments: foo.txt")
    assert out.err.count("Attachments:") == 1


def test_cli_prompt_accepts_absolute_path_mentions(monkeypatch, tmp_path, capsys):
    from abstractruntime.core.models import RunStatus

    abs_file = tmp_path / "outside.jpg"
    abs_file.write_bytes(b"jpg")
    abs_path = str(abs_file.resolve())

    class _State:
        def __init__(self, status, *, output=None, waiting=None, error=None):
            self.status = status
            self.output = output
            self.waiting = waiting
            self.error = error

    class DummyAgent:
        def __init__(self):
            self.workflow = object()
            self.started = None

        def start(self, task, *, allowed_tools=None, attachments=None):
            self.started = {"task": task, "allowed_tools": allowed_tools, "attachments": attachments}
            return "run_123"

        def step(self):
            return _State(RunStatus.COMPLETED, output={"answer": "done"})

        def resume(self, response):
            raise AssertionError("resume should not be called in this test")

        def save_state(self, filepath):
            raise AssertionError("save_state should not be called when --no-state is set")

    class DummyRuntime:
        def resume(self, *, workflow, run_id, wait_key, payload):
            raise AssertionError("resume should not be called in this test")

    class DummyShell:
        def __init__(self, **_kwargs):
            self._agent = DummyAgent()
            self._runtime = DummyRuntime()
            self._allowed_tools = None
            self._tool_runner = object()
            self._auto_approve = False
            self._state_file = None

        def _normalize_attachment_token(self, tok):
            # In real ReactShell this canonicalizes mounts and accepts absolute paths.
            return str(tok or "").strip()

        def _ingest_workspace_attachments(self, rel_paths):
            assert rel_paths == [abs_path]
            return [
                {
                    "$artifact": "a1",
                    "filename": "outside.jpg",
                    "content_type": "image/jpeg",
                    "source_path": abs_path,
                }
            ]

        def _try_load_state(self):
            raise AssertionError("_try_load_state should not be called when --no-state is set")

        def run(self):
            raise AssertionError("Interactive shell should not be started in --prompt mode")

    from abstractcode import cli as cli_mod

    import abstractcode.react_shell as react_shell_mod

    monkeypatch.setattr(react_shell_mod, "ReactShell", DummyShell)

    rc = cli_mod.main(["--no-state", "--prompt", f"Hello @{abs_path}"])
    assert rc == 0

    out = capsys.readouterr()
    assert out.out.strip() == "done"
    # Avoid printing the full absolute path to stderr; show a safe display name.
    assert "Attachments: outside.jpg" in out.err


def test_cli_prompt_drives_subworkflow_waits(monkeypatch, capsys):
    from abstractruntime.core.models import RunStatus, WaitReason

    class _Waiting:
        def __init__(self, reason, *, wait_key=None, prompt=None, choices=None, details=None):
            self.reason = reason
            self.wait_key = wait_key
            self.prompt = prompt
            self.choices = choices
            self.details = details

    class _State:
        def __init__(self, run_id, workflow_id, status, *, output=None, waiting=None, error=None, parent_run_id=None):
            self.run_id = run_id
            self.workflow_id = workflow_id
            self.status = status
            self.output = output
            self.waiting = waiting
            self.error = error
            self.parent_run_id = parent_run_id

    class _Registry:
        def __init__(self, items):
            self._items = dict(items)

        def get(self, workflow_id):
            return self._items.get(workflow_id)

    class DummyRuntime:
        def __init__(self):
            self._parent_wf = type("_Wf", (), {"workflow_id": "parent_wf"})()
            self._child_wf = type("_Wf", (), {"workflow_id": "child_wf"})()
            self.workflow_registry = _Registry({"parent_wf": self._parent_wf, "child_wf": self._child_wf})
            self._states = {
                "run_parent": _State(
                    "run_parent",
                    "parent_wf",
                    RunStatus.WAITING,
                    waiting=_Waiting(
                        WaitReason.SUBWORKFLOW,
                        wait_key="subworkflow:run_child",
                        details={"sub_run_id": "run_child", "sub_workflow_id": "child_wf", "async": True},
                    ),
                ),
                "run_child": _State(
                    "run_child",
                    "child_wf",
                    RunStatus.RUNNING,
                    parent_run_id="run_parent",
                ),
            }

        def get_state(self, run_id):
            return self._states[run_id]

        def tick(self, *, workflow, run_id, max_steps=100):
            st = self._states[run_id]
            if run_id == "run_child":
                st.status = RunStatus.COMPLETED
                st.output = {"answer": "done"}
                st.waiting = None
            return st

        def resume(self, *, workflow, run_id, wait_key, payload, max_steps=100):
            st = self._states[run_id]
            if run_id == "run_parent":
                st.status = RunStatus.COMPLETED
                st.output = {"answer": "done"}
                st.waiting = None
            return st

        def get_node_traces(self, _run_id):
            return [{"node_id": "stub"}]

    class DummyAgent:
        def __init__(self, runtime):
            self.workflow = type("_Wf", (), {"workflow_id": "parent_wf"})()
            self._runtime = runtime
            self._run_id = None

        def start(self, task, *, allowed_tools=None, attachments=None):
            self._run_id = "run_parent"
            return self._run_id

        def step(self):
            return self._runtime.get_state(self._run_id)

        def resume(self, response):
            raise AssertionError("resume should not be called for SUBWORKFLOW parent waits")

        def save_state(self, filepath):
            raise AssertionError("save_state should not be called when --no-state is set")

    class DummyShell:
        def __init__(self, **_kwargs):
            self._runtime = DummyRuntime()
            self._agent = DummyAgent(self._runtime)
            self._allowed_tools = None
            self._tool_runner = object()
            self._auto_approve = True
            self._state_file = None

        def _ingest_workspace_attachments(self, rel_paths):
            raise AssertionError("_ingest_workspace_attachments should not be called without @file mentions")

        def _sync_tool_prompt_settings_to_run(self, _run_id):
            return None

        def _try_load_state(self):
            raise AssertionError("_try_load_state should not be called when --no-state is set")

        def run(self):
            raise AssertionError("Interactive shell should not be started in --prompt mode")

    from abstractcode import cli as cli_mod

    import abstractcode.react_shell as react_shell_mod

    monkeypatch.setattr(react_shell_mod, "ReactShell", DummyShell)

    rc = cli_mod.main(["--no-state", "--agent", "whatever", "--prompt", "Hello"])
    assert rc == 0

    out = capsys.readouterr()
    assert out.out.strip() == "done"
    assert "Run waiting: subworkflow" not in out.err


def test_cli_prompt_prints_workflow_agent_response(monkeypatch, capsys):
    from abstractruntime.core.models import RunStatus

    class _State:
        def __init__(self, status, *, output=None, waiting=None, error=None):
            self.status = status
            self.output = output
            self.waiting = waiting
            self.error = error

    class DummyAgent:
        def __init__(self):
            self.workflow = object()

        def start(self, task, *, allowed_tools=None, attachments=None):
            return "run_123"

        def step(self):
            return _State(RunStatus.COMPLETED, output={"result": {"response": "hello"}})

        def resume(self, response):
            raise AssertionError("resume should not be called in this test")

        def save_state(self, filepath):
            raise AssertionError("save_state should not be called when --no-state is set")

    class DummyRuntime:
        def resume(self, *, workflow, run_id, wait_key, payload):
            raise AssertionError("resume should not be called in this test")

    class DummyShell:
        def __init__(self, **_kwargs):
            self._agent = DummyAgent()
            self._runtime = DummyRuntime()
            self._allowed_tools = None
            self._tool_runner = object()
            self._auto_approve = False
            self._state_file = None

        def _ingest_workspace_attachments(self, rel_paths):
            raise AssertionError("_ingest_workspace_attachments should not be called without @file mentions")

        def _try_load_state(self):
            raise AssertionError("_try_load_state should not be called when --no-state is set")

        def run(self):
            raise AssertionError("Interactive shell should not be started in --prompt mode")

    from abstractcode import cli as cli_mod

    import abstractcode.react_shell as react_shell_mod

    monkeypatch.setattr(react_shell_mod, "ReactShell", DummyShell)

    rc = cli_mod.main(["--no-state", "--prompt", "Hello"])
    assert rc == 0

    out = capsys.readouterr()
    assert out.out.strip() == "hello"
