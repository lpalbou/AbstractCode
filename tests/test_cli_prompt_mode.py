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

    monkeypatch.setattr(cli_mod, "ReactShell", DummyShell)

    rc = cli_mod.main(["--no-state", "--prompt", "Hello @foo.txt"])
    assert rc == 0

    out = capsys.readouterr()
    assert out.out.strip() == "done"
    assert "Attachments: foo.txt" in out.err
    assert out.err.strip().endswith("Attachments: foo.txt")
    assert out.err.count("Attachments:") == 1
