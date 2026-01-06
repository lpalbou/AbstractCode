import pytest


def _make_agent_v1_flow_dict(*, flow_id: str, name: str, declare_interface: bool) -> dict:
    interfaces = ["abstractcode.agent.v1"] if declare_interface else []
    return {
        "id": flow_id,
        "name": name,
        "interfaces": interfaces,
        "nodes": [
            {
                "id": "start",
                "type": "on_flow_start",
                "position": {"x": 0, "y": 0},
                "data": {
                    "outputs": [
                        {"id": "exec-out", "label": "", "type": "execution"},
                        {"id": "request", "label": "request", "type": "string"},
                    ]
                },
            },
            {
                "id": "end",
                "type": "on_flow_end",
                "position": {"x": 10, "y": 0},
                "data": {
                    "inputs": [
                        {"id": "exec-in", "label": "", "type": "execution"},
                        {"id": "response", "label": "response", "type": "string"},
                    ]
                },
            },
        ],
        "edges": [
            {
                "id": "edge-exec",
                "source": "start",
                "sourceHandle": "exec-out",
                "target": "end",
                "targetHandle": "exec-in",
                "animated": True,
            },
            {
                "id": "edge-data",
                "source": "start",
                "sourceHandle": "request",
                "target": "end",
                "targetHandle": "response",
                "animated": False,
            },
        ],
        "entryNode": "start",
    }


def test_workflow_agent_runs_deterministic_flow(tmp_path) -> None:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception:
        pytest.skip("abstractflow not installed")

    from abstractruntime import InMemoryLedgerStore, InMemoryRunStore, Runtime
    from abstractruntime.core.models import RunStatus

    from abstractcode.workflow_agent import WorkflowAgent

    vf = VisualFlow.model_validate(_make_agent_v1_flow_dict(flow_id="wf1", name="wf1", declare_interface=True))
    flow_path = tmp_path / "wf.json"
    flow_path.write_text(vf.model_dump_json(indent=2), encoding="utf-8")

    runtime = Runtime(run_store=InMemoryRunStore(), ledger_store=InMemoryLedgerStore())
    agent = WorkflowAgent(runtime=runtime, flow_ref=str(flow_path), tools=[])

    agent.start("hello")
    state = agent.step()
    while state.status == RunStatus.RUNNING:
        state = agent.step()

    assert state.status == RunStatus.COMPLETED
    assert isinstance(state.output, dict)
    assert isinstance(state.output.get("result"), dict)
    assert state.output["result"]["response"] == "hello"

    ctx = state.vars.get("context") if isinstance(state.vars, dict) else None
    assert isinstance(ctx, dict)
    messages = ctx.get("messages")
    assert isinstance(messages, list)
    assert messages[-2].get("role") == "user"
    assert messages[-2].get("content") == "hello"
    assert messages[-1].get("role") == "assistant"
    assert messages[-1].get("content") == "hello"


def test_workflow_agent_resolves_by_name(tmp_path) -> None:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception:
        pytest.skip("abstractflow not installed")

    from abstractruntime import InMemoryLedgerStore, InMemoryRunStore, Runtime
    from abstractruntime.core.models import RunStatus

    from abstractcode.workflow_agent import WorkflowAgent

    vf = VisualFlow.model_validate(
        _make_agent_v1_flow_dict(flow_id="wf2", name="My Workflow Agent", declare_interface=True)
    )
    (tmp_path / "wf2.json").write_text(vf.model_dump_json(indent=2), encoding="utf-8")

    runtime = Runtime(run_store=InMemoryRunStore(), ledger_store=InMemoryLedgerStore())
    agent = WorkflowAgent(runtime=runtime, flow_ref="My Workflow Agent", flows_dir=str(tmp_path), tools=[])

    agent.start("ping")
    state = agent.step()
    while state.status == RunStatus.RUNNING:
        state = agent.step()

    assert state.status == RunStatus.COMPLETED
    assert isinstance(state.output, dict)
    assert isinstance(state.output.get("result"), dict)
    assert state.output["result"]["response"] == "ping"


def test_workflow_agent_requires_interface_marker(tmp_path) -> None:
    try:
        from abstractflow.visual.models import VisualFlow
    except Exception:
        pytest.skip("abstractflow not installed")

    from abstractruntime import InMemoryLedgerStore, InMemoryRunStore, Runtime

    from abstractcode.workflow_agent import WorkflowAgent

    vf = VisualFlow.model_validate(_make_agent_v1_flow_dict(flow_id="wf3", name="wf3", declare_interface=False))
    flow_path = tmp_path / "wf.json"
    flow_path.write_text(vf.model_dump_json(indent=2), encoding="utf-8")

    runtime = Runtime(run_store=InMemoryRunStore(), ledger_store=InMemoryLedgerStore())
    with pytest.raises(ValueError, match="does not implement 'abstractcode\\.agent\\.v1'"):
        WorkflowAgent(runtime=runtime, flow_ref=str(flow_path), tools=[])

